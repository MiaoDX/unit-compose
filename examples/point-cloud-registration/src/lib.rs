//! Kornia-backed point-cloud registration primitives used by the showcase pipeline.

use kornia_3d::{
    linalg::transform_points3d,
    pointcloud::PointCloud,
    registration::{ICPConvergenceCriteria, ICPResult, icp_vanilla},
};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use unit_compose_core::{
    AllocationCapability, AllocationDomain, AllocationEvidence, BoundedBufferWriter,
    BoundedStorage, BuildOptions, CompositeModule, FixedModuleDescription, PortDescriptor,
    RequirementStatus, ResourceDescriptor, ResourceId, ResourceRegistry, RunError, SemanticType,
    Unit, UnitConfigurationSummary, UnitDescriptor, UnitId, UnitRegistry, UnitTypeName,
    UnitWorkspace,
};
use unit_compose_yaml::{
    BoundSources, CompiledDefinition, ParseLimits, UnitRequirements, load,
    register_unit as register_yaml_unit,
};

pub const MAX_POINTS: usize = 4_096;
pub const MAX_INPUT_POINTS: usize = 300_000;

pub struct PointCloudPair {
    pub source: Vec<[f64; 3]>,
    pub target: Vec<[f64; 3]>,
    pub initial_rotation: [[f64; 3]; 3],
    pub initial_translation: [f64; 3],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointSample {
    pub source: [f64; 3],
    pub target: [f64; 3],
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
struct DemoConfig {
    max_points: usize,
}

pub struct PreparedPointCloudRegistration {
    pub description: FixedModuleDescription,
    pub module: CompositeModule<PointCloudRegistrationUnit>,
}

impl PreparedPointCloudRegistration {
    pub fn run(&mut self, input: &PointCloudPair) -> Result<(), RunError> {
        self.module.run(input).map(|_| ())
    }
    pub fn run_profiled(&mut self, input: &PointCloudPair) -> Result<(), RunError> {
        self.module.run_profiled(input, &mut [], None).map(|_| ())
    }
    pub fn snapshot(&self) -> Result<&PointCloudRegistration, String> {
        self.module
            .unit()
            .snapshot
            .as_ref()
            .ok_or_else(|| "point-cloud registration has no successful run".to_owned())
    }
}

pub struct PointCloudRegistrationUnit {
    max_points: usize,
    snapshot: Option<PointCloudRegistration>,
}

impl PointCloudRegistrationUnit {
    fn from_definition(definition: &CompiledDefinition) -> Result<Self, String> {
        let config = definition
            .config::<DemoConfig>(&UnitId::new("metrics"))
            .ok_or_else(|| "missing metrics configuration".to_owned())?;
        validate_config(config)?;
        Ok(Self {
            max_points: config.max_points,
            snapshot: None,
        })
    }
    fn execute(
        &mut self,
        input: &PointCloudPair,
        output: &mut BoundedBufferWriter<'_, PointSample>,
        mut workspace: UnitWorkspace<'_>,
        mut recorder: Option<&mut unit_compose_core::UnitExecutionRecorder>,
    ) -> Result<(), RunError> {
        if input.source.len() < 4
            || input.target.len() < 4
            || input.source.len() > MAX_INPUT_POINTS
            || input.target.len() > MAX_INPUT_POINTS
        {
            return Err(RunError::InvalidInput {
                message: "point-cloud pair exceeds configured bounds",
            });
        }
        workspace.bytes().fill(0);
        let operation =
            |unit: usize,
             operation: &mut dyn FnMut() -> Result<(), RunError>,
             recorder: &mut Option<&mut unit_compose_core::UnitExecutionRecorder>| {
                match recorder.as_deref_mut() {
                    Some(recorder) => recorder.measure(unit, operation),
                    None => operation(),
                }
            };

        let mut sampled = None;
        let mut sample_stage = || {
            let count = self
                .max_points
                .min(input.source.len())
                .min(input.target.len());
            sampled = Some((
                bounded_sample(&input.source, count),
                bounded_sample(&input.target, count),
            ));
            Ok(())
        };
        operation(0, &mut sample_stage, &mut recorder)?;

        let mut icp: Option<ICPResult> = None;
        let mut icp_stage = || {
            let (source, target) = sampled.as_ref().ok_or_else(registration_error)?;
            icp = Some(
                icp_vanilla(
                    &PointCloud::new(source.clone(), None, None),
                    &PointCloud::new(target.clone(), None, None),
                    input.initial_rotation,
                    input.initial_translation,
                    ICPConvergenceCriteria {
                        max_iterations: 100,
                        tolerance: 1e-9,
                    },
                )
                .map_err(|_| registration_error())?,
            );
            Ok(())
        };
        operation(1, &mut icp_stage, &mut recorder)?;

        let mut initial_aligned = None;
        let mut aligned = None;
        let mut transform_stage = || {
            let (source, _) = sampled.as_ref().ok_or_else(registration_error)?;
            let result = icp.as_ref().ok_or_else(registration_error)?;
            let mut initial = vec![[0.0; 3]; source.len()];
            transform_points3d(
                source,
                &input.initial_rotation,
                &input.initial_translation,
                &mut initial,
            )
            .map_err(|_| registration_error())?;
            let mut final_points = vec![[0.0; 3]; source.len()];
            transform_points3d(
                source,
                &result.rotation,
                &result.translation,
                &mut final_points,
            )
            .map_err(|_| registration_error())?;
            initial_aligned = Some(initial);
            aligned = Some(final_points);
            Ok(())
        };
        operation(2, &mut transform_stage, &mut recorder)?;

        let mut registration = None;
        let mut metrics_stage = || {
            let (_, target) = sampled.as_ref().ok_or_else(registration_error)?;
            let result = icp.as_ref().ok_or_else(registration_error)?;
            let aligned = aligned.take().ok_or_else(registration_error)?;
            for (&source, &target) in aligned.iter().zip(target) {
                output
                    .try_push(PointSample { source, target })
                    .map_err(RunError::Capacity)?;
            }
            output.complete();
            registration = Some(PointCloudRegistration {
                rotation: result.rotation,
                translation: result.translation,
                initial_rmse: nearest_neighbor_rmse(
                    initial_aligned.as_ref().ok_or_else(registration_error)?,
                    target,
                ),
                final_rmse: result.rmse,
                iterations: result.num_iterations,
                aligned,
            });
            Ok(())
        };
        operation(3, &mut metrics_stage, &mut recorder)?;
        self.snapshot = registration;
        Ok(())
    }
}

impl Unit for PointCloudRegistrationUnit {
    type Input = PointCloudPair;
    type Storage = BoundedStorage<PointSample>;
    fn workspace_requirement(&self) -> usize {
        self.max_points * 24
    }
    fn output_storage(&self) -> Self::Storage {
        BoundedStorage::new("registration_result", self.max_points)
    }
    fn allocation_capability(&self) -> AllocationCapability {
        AllocationCapability::inspect(
            vec![AllocationDomain {
                name: "rust-global".to_owned(),
                evidence: AllocationEvidence::Unsupported,
            }],
            false,
        )
    }
    fn requirement_status(&self) -> RequirementStatus {
        RequirementStatus::Bounded
    }
    fn validate_input(&self, input: &Self::Input) -> Result<(), RunError> {
        if input.source.len() >= 4
            && input.target.len() >= 4
            && input.source.len() <= MAX_INPUT_POINTS
            && input.target.len() <= MAX_INPUT_POINTS
        {
            Ok(())
        } else {
            Err(RunError::InvalidInput {
                message: "point-cloud pair exceeds configured bounds",
            })
        }
    }
    fn run(
        &mut self,
        input: &Self::Input,
        output: &mut BoundedBufferWriter<'_, PointSample>,
        workspace: UnitWorkspace<'_>,
    ) -> Result<(), RunError> {
        self.execute(input, output, workspace, None)
    }
    fn run_with_unit_timing(
        &mut self,
        input: &Self::Input,
        output: &mut BoundedBufferWriter<'_, PointSample>,
        workspace: UnitWorkspace<'_>,
        recorder: &mut unit_compose_core::UnitExecutionRecorder,
    ) -> Result<(), RunError> {
        self.execute(input, output, workspace, Some(recorder))
    }
}

pub fn build_from_path(path: &Path) -> Result<PreparedPointCloudRegistration, String> {
    build_from_source(&fs::read_to_string(path).map_err(|error| error.to_string())?)
}
fn build_from_source(source: &str) -> Result<PreparedPointCloudRegistration, String> {
    let (units, resources) = registries()?;
    let bounds = BoundSources {
        host: BTreeMap::from([(ResourceId::new("cloud_pair"), MAX_INPUT_POINTS)]),
        adapters: BTreeMap::new(),
    };
    let definition = load(source, ParseLimits::default(), &units, &resources, &bounds)
        .map_err(|error| error.to_string())?
        .compile()
        .map_err(|error| error.to_string())?;
    validate_pipeline(&definition)?;
    let unit = PointCloudRegistrationUnit::from_definition(&definition)?;
    let module = CompositeModule::build(unit, BuildOptions::development())
        .map_err(|error| format!("Module build failed: {error:?}"))?;
    let requirements = definition.requirements.clone();
    let storage = unit_compose_core::plan_storage(&definition.graph, &resources, &requirements)
        .map_err(|error| format!("storage planning failed: {error:?}"))?;
    let description = FixedModuleDescription::new(
        definition.graph.clone(),
        configuration_summaries(&definition)?,
        requirements,
        definition.workspace_bytes,
        storage.report().clone(),
        module.description().clone(),
    );
    Ok(PreparedPointCloudRegistration {
        description,
        module,
    })
}

fn registries() -> Result<(UnitRegistry, ResourceRegistry), String> {
    let pair = cloud_pair_type();
    let stage = semantic("point.Stage/v1")?;
    let result = semantic("point.RegistrationResult/v1")?;
    let mut resources = ResourceRegistry::default();
    resources
        .register(ResourceDescriptor::of::<PointCloudPair>(
            pair.clone(),
            "bounded point-cloud pair",
            "equal point counts and finite coordinates",
        ))
        .map_err(debug)?;
    resources
        .register(ResourceDescriptor::bounded_buffer::<
            Vec<PointSample>,
            PointSample,
        >(
            stage.clone(),
            "point registration stage",
            "bounded samples",
        ))
        .map_err(debug)?;
    resources
        .register(ResourceDescriptor::bounded_buffer::<
            Vec<PointSample>,
            PointSample,
        >(
            result.clone(),
            "point registration result",
            "bounded aligned points",
        ))
        .map_err(debug)?;
    let mut units = UnitRegistry::default();
    register_unit(
        &mut units,
        "point.sample/v1",
        vec![port::<PointCloudPair>("pair", &pair)],
        vec![port::<Vec<PointSample>>("out", &stage)],
        point_requirements,
    )?;
    for name in ["point.icp/v1", "point.transform/v1", "point.metrics/v1"] {
        let output = if name == "point.metrics/v1" {
            result.clone()
        } else {
            stage.clone()
        };
        register_unit(
            &mut units,
            name,
            vec![port::<Vec<PointSample>>("input", &stage)],
            vec![port::<Vec<PointSample>>("out", &output)],
            point_requirements,
        )?;
    }
    Ok((units, resources))
}

fn point_requirements(config: &DemoConfig, _: &BoundSources) -> Result<UnitRequirements, String> {
    validate_config(config)?;
    Ok(UnitRequirements {
        output_capacities: BTreeMap::from([("out".to_owned(), config.max_points)]),
        workspace_bytes: config.max_points * 24,
    })
}

fn validate_config(config: &DemoConfig) -> Result<(), String> {
    if !(4..=MAX_POINTS).contains(&config.max_points) {
        return Err(format!(
            "point bound requires max_points in 4..={MAX_POINTS}"
        ));
    }
    Ok(())
}

fn validate_pipeline(definition: &CompiledDefinition) -> Result<(), String> {
    const STAGES: [(&str, &str); 4] = [
        ("sample", "point.sample/v1"),
        ("icp", "point.icp/v1"),
        ("transform", "point.transform/v1"),
        ("metrics", "point.metrics/v1"),
    ];
    let expected_order = STAGES.map(|(id, _)| UnitId::new(id));
    if definition.graph.execution_order != expected_order
        || definition.graph.module_outputs != [ResourceId::new("registration_result")]
    {
        return Err("point-cloud registration requires the fixed sample -> icp -> transform -> metrics pipeline".to_owned());
    }
    let expected_config = definition
        .config::<DemoConfig>(&expected_order[0])
        .ok_or_else(|| "missing config for sample".to_owned())?;
    for (index, ((id, unit_type), unit_id)) in STAGES.iter().zip(&expected_order).enumerate() {
        let unit = definition
            .graph
            .units
            .iter()
            .find(|unit| &unit.id == unit_id)
            .ok_or_else(|| format!("missing fixed point-cloud stage {id}"))?;
        let expected_dependencies = index
            .checked_sub(1)
            .map(|previous| vec![expected_order[previous].clone()])
            .unwrap_or_default();
        let config = definition
            .config::<DemoConfig>(unit_id)
            .ok_or_else(|| format!("missing config for {id}"))?;
        if unit.unit_type.as_str() != *unit_type
            || unit.dependencies != expected_dependencies
            || config != expected_config
        {
            return Err(
                "point-cloud registration stages must use the fixed pipeline and identical bounds"
                    .to_owned(),
            );
        }
    }
    Ok(())
}
fn configuration_summaries(
    definition: &CompiledDefinition,
) -> Result<Vec<UnitConfigurationSummary>, String> {
    definition
        .graph
        .units
        .iter()
        .map(|unit| {
            let config = definition
                .config::<DemoConfig>(&unit.id)
                .ok_or_else(|| format!("missing config for {}", unit.id.as_str()))?;
            Ok(UnitConfigurationSummary {
                unit: unit.id.clone(),
                summary: format!("max_points={}", config.max_points),
            })
        })
        .collect()
}
fn register_unit<F>(
    units: &mut UnitRegistry,
    name: &str,
    inputs: Vec<PortDescriptor>,
    outputs: Vec<PortDescriptor>,
    requirements: F,
) -> Result<(), String>
where
    F: Fn(&DemoConfig, &BoundSources) -> Result<UnitRequirements, String> + 'static,
{
    register_yaml_unit::<DemoConfig, _>(
        units,
        UnitDescriptor {
            type_name: UnitTypeName::new(name),
            inputs,
            outputs,
        },
        requirements,
    )
    .map_err(debug)
}
fn port<T: 'static>(name: &str, semantic_type: &SemanticType) -> PortDescriptor {
    PortDescriptor::of::<T>(name, semantic_type.clone())
}
fn semantic(name: &str) -> Result<SemanticType, String> {
    SemanticType::new(name).map_err(debug)
}
fn cloud_pair_type() -> SemanticType {
    SemanticType::new("point.CloudPair/v1").expect("static semantic type is valid")
}
fn debug(error: impl std::fmt::Debug) -> String {
    format!("{error:?}")
}

fn bounded_sample(points: &[[f64; 3]], count: usize) -> Vec<[f64; 3]> {
    (0..count)
        .map(|index| points[index * points.len() / count])
        .collect()
}

fn registration_error() -> RunError {
    RunError::InvalidInput {
        message: "Kornia ICP registration failed",
    }
}

#[derive(Clone, Debug)]
pub struct PointCloudRegistration {
    pub rotation: [[f64; 3]; 3],
    pub translation: [f64; 3],
    pub aligned: Vec<[f64; 3]>,
    pub initial_rmse: f64,
    pub final_rmse: f64,
    pub iterations: usize,
}

#[cfg(test)]
fn register_points_with_initial(
    source: &[[f64; 3]],
    target: &[[f64; 3]],
    initial_rotation: [[f64; 3]; 3],
    initial_translation: [f64; 3],
) -> Result<PointCloudRegistration, String> {
    if source.len() < 4 || source.len() != target.len() || source.len() > MAX_POINTS {
        return Err("point-cloud pair must have equal lengths in 4..=4096".to_owned());
    }
    let mut initial_aligned = vec![[0.0; 3]; source.len()];
    transform_points3d(
        source,
        &initial_rotation,
        &initial_translation,
        &mut initial_aligned,
    )
    .map_err(|error| error.to_string())?;
    let initial_rmse = nearest_neighbor_rmse(&initial_aligned, target);
    let result = icp_vanilla(
        &PointCloud::new(source.to_vec(), None, None),
        &PointCloud::new(target.to_vec(), None, None),
        initial_rotation,
        initial_translation,
        ICPConvergenceCriteria {
            max_iterations: 100,
            tolerance: 1e-9,
        },
    )
    .map_err(|error| error.to_string())?;
    let mut aligned = vec![[0.0; 3]; source.len()];
    transform_points3d(source, &result.rotation, &result.translation, &mut aligned)
        .map_err(|error| error.to_string())?;
    let final_rmse = result.rmse;
    Ok(PointCloudRegistration {
        rotation: result.rotation,
        translation: result.translation,
        aligned,
        initial_rmse,
        final_rmse,
        iterations: result.num_iterations,
    })
}

fn nearest_neighbor_rmse(source: &[[f64; 3]], target: &[[f64; 3]]) -> f64 {
    let squared = source
        .iter()
        .map(|point| {
            target
                .iter()
                .map(|candidate| {
                    (point[0] - candidate[0]).powi(2)
                        + (point[1] - candidate[1]).powi(2)
                        + (point[2] - candidate[2]).powi(2)
                })
                .fold(f64::INFINITY, f64::min)
        })
        .sum::<f64>();
    (squared / source.len() as f64).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icp_recovers_known_transform_and_reduces_error() {
        let source = (0..8)
            .flat_map(|x| {
                (0..7).flat_map(move |y| {
                    (0..6).map(move |z| {
                        let xf = x as f64 * 0.17;
                        let yf = y as f64 * 0.13;
                        let zf = z as f64 * 0.11;
                        [
                            xf + 0.02 * yf * zf,
                            yf + 0.01 * xf * xf,
                            zf + 0.03 * xf * yf,
                        ]
                    })
                })
            })
            .collect::<Vec<_>>();
        let angle = 0.08_f64;
        let rotation = [
            [angle.cos(), -angle.sin(), 0.0],
            [angle.sin(), angle.cos(), 0.0],
            [0.0, 0.0, 1.0],
        ];
        let translation = [0.04, -0.03, 0.02];
        let mut target = vec![[0.0; 3]; source.len()];
        transform_points3d(&source, &rotation, &translation, &mut target).unwrap();
        let identity = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        let first = register_points_with_initial(&source, &target, identity, [0.0; 3]).unwrap();
        let second = register_points_with_initial(&source, &target, identity, [0.0; 3]).unwrap();
        assert_eq!(first.rotation, second.rotation);
        assert_eq!(first.translation, second.translation);
        assert!(first.final_rmse < 1e-3, "rmse={}", first.final_rmse);
        assert!(
            first.final_rmse < first.initial_rmse * 0.02,
            "initial={} final={}",
            first.initial_rmse,
            first.final_rmse
        );
    }

    #[test]
    fn yaml_rejects_unsupported_bounds_before_workspace_arithmetic() {
        let source = include_str!("../point-cloud-registration.yaml")
            .replace("max_points: 4096", &format!("max_points: {}", usize::MAX));
        assert!(build_from_source(&source).is_err());
    }

    #[test]
    fn yaml_rejects_pipeline_or_config_mismatches() {
        let source = include_str!("../point-cloud-registration.yaml");
        let shortened = source
            .replace(
                "  icp:\n    type: point.icp/v1\n    config: { max_points: 4096 }\n    inputs: { input: sampled_clouds }\n    outputs: { out: icp_result }\n  transform:\n    type: point.transform/v1\n    config: { max_points: 4096 }\n    inputs: { input: icp_result }\n    outputs: { out: aligned_cloud }\n",
                "",
            )
            .replace("inputs: { input: aligned_cloud }", "inputs: { input: sampled_clouds }");
        assert!(build_from_source(&shortened).is_err());

        let mismatched = source.replacen("max_points: 4096", "max_points: 4095", 1);
        assert!(build_from_source(&mismatched).is_err());

        let wrong_output = source.replace(
            "outputs:\n  result: registration_result",
            "outputs:\n  result: sampled_clouds",
        );
        assert!(build_from_source(&wrong_output).is_err());
    }

    #[test]
    fn yaml_module_runs_and_times_every_planned_unit() {
        let source_yaml = include_str!("../point-cloud-registration.yaml");
        let mut prepared = build_from_source(source_yaml).unwrap();
        let source = (0..8)
            .flat_map(|x| {
                (0..7).flat_map(move |y| {
                    (0..6).map(move |z| [x as f64 * 0.17, y as f64 * 0.13, z as f64 * 0.11])
                })
            })
            .collect::<Vec<_>>();
        let target = source
            .iter()
            .map(|point| [point[0] + 0.02, point[1] - 0.01, point[2] + 0.015])
            .collect();
        let pair = PointCloudPair {
            source,
            target,
            initial_rotation: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            initial_translation: [0.0, 0.0, 0.0],
        };
        let mut reports = Vec::new();
        for _ in 0..2 {
            prepared.run_profiled(&pair).unwrap();
            reports.push(prepared.module.report().snapshot());
        }
        let graph = prepared.description.to_mermaid();
        for unit in ["sample", "icp", "transform", "metrics"] {
            assert!(graph.contains(&format!("{unit}<br/>Unit")));
        }
        let timed = prepared.description.to_mermaid_with_runs(&reports);
        assert_eq!(timed.matches("n=2").count(), 4);
        assert!(timed.contains("avg ") && timed.contains("p99 "));
    }
}
