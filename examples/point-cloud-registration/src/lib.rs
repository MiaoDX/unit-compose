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
    AllocationCapability, AllocationDomain, AllocationEvidence, BuildOptions,
    FixedModuleDescription, InputHandle, Module, ModuleInputs, OutputHandle, PortDescriptor,
    PreparedModuleDescription, RegistrationInvocation, RequirementStatus, ResourceDescriptor,
    ResourceId, ResourceRegistry, RunError, SemanticType, UnitConfigurationSummary, UnitDescriptor,
    UnitRegistry, UnitTypeName,
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

struct SampledClouds {
    source: Vec<[f64; 3]>,
    target: Vec<[f64; 3]>,
    initial_rotation: [[f64; 3]; 3],
    initial_translation: [f64; 3],
}

#[derive(Clone)]
struct AlignedCloud {
    registration: PointCloudRegistration,
}

struct SampleUnit {
    max_points: usize,
}
#[derive(Default)]
struct IcpUnit;
#[derive(Default)]
struct TransformUnit;
#[derive(Default)]
struct MetricsUnit;

pub struct PreparedPointCloudRegistration {
    pub description: FixedModuleDescription,
    pub module: Module,
    input: InputHandle<PointCloudPair>,
    registration: OutputHandle<PointCloudRegistration>,
    snapshot: Option<PointCloudRegistration>,
}

impl PreparedPointCloudRegistration {
    pub fn run(&mut self, input: &PointCloudPair) -> Result<(), RunError> {
        self.execute(input, false)
    }
    pub fn run_profiled(&mut self, input: &PointCloudPair) -> Result<(), RunError> {
        self.execute(input, true)
    }
    pub fn snapshot(&self) -> Result<&PointCloudRegistration, String> {
        self.snapshot
            .as_ref()
            .ok_or_else(|| "point-cloud registration has no successful run".to_owned())
    }

    fn execute(&mut self, input: &PointCloudPair, profiled: bool) -> Result<(), RunError> {
        let mut inputs = ModuleInputs::with_capacity(1);
        inputs
            .bind(&self.input, input)
            .map_err(|error| RunError::RuntimeBinding {
                message: format!("{error:?}"),
            })?;
        if profiled {
            self.module.run_profiled(&inputs, &mut [], None)?;
        } else {
            self.module.run(&inputs)?;
        }
        self.snapshot = Some(self.module.output(&self.registration)?.clone());
        Ok(())
    }
}

impl SampleUnit {
    fn execute(&mut self, invocation: &RegistrationInvocation<'_>) -> Result<(), RunError> {
        let input = invocation.input_value::<PointCloudPair>(0)?;
        if input.source.len() < 4
            || input.target.len() < 4
            || input.source.len() > MAX_INPUT_POINTS
            || input.target.len() > MAX_INPUT_POINTS
        {
            return Err(RunError::InvalidInput {
                message: "point-cloud pair exceeds configured bounds",
            });
        }
        let count = self
            .max_points
            .min(input.source.len())
            .min(input.target.len());
        invocation.write_value(
            0,
            SampledClouds {
                source: bounded_sample(&input.source, count),
                target: bounded_sample(&input.target, count),
                initial_rotation: input.initial_rotation,
                initial_translation: input.initial_translation,
            },
        )
    }
}

impl IcpUnit {
    fn execute(invocation: &RegistrationInvocation<'_>) -> Result<(), RunError> {
        let sampled = invocation.input_value::<SampledClouds>(0)?;
        let result = icp_vanilla(
            &PointCloud::new(sampled.source.clone(), None, None),
            &PointCloud::new(sampled.target.clone(), None, None),
            sampled.initial_rotation,
            sampled.initial_translation,
            ICPConvergenceCriteria {
                max_iterations: 100,
                tolerance: 1e-9,
            },
        )
        .map_err(|_| registration_error())?;
        invocation.write_value(0, result)
    }
}

impl TransformUnit {
    fn execute(invocation: &RegistrationInvocation<'_>) -> Result<(), RunError> {
        let result = invocation.input_value::<ICPResult>(0)?;
        let sampled = invocation.input_value::<SampledClouds>(1)?;
        let mut initial = vec![[0.0; 3]; sampled.source.len()];
        transform_points3d(
            &sampled.source,
            &sampled.initial_rotation,
            &sampled.initial_translation,
            &mut initial,
        )
        .map_err(|_| registration_error())?;
        let mut aligned = vec![[0.0; 3]; sampled.source.len()];
        transform_points3d(
            &sampled.source,
            &result.rotation,
            &result.translation,
            &mut aligned,
        )
        .map_err(|_| registration_error())?;
        invocation.write_value(
            0,
            AlignedCloud {
                registration: PointCloudRegistration {
                    rotation: result.rotation,
                    translation: result.translation,
                    initial_rmse: nearest_neighbor_rmse(&initial, &sampled.target),
                    final_rmse: result.rmse,
                    iterations: result.num_iterations,
                    aligned,
                },
            },
        )
    }
}

impl MetricsUnit {
    fn execute(invocation: &RegistrationInvocation<'_>) -> Result<(), RunError> {
        let aligned = invocation.input_value::<AlignedCloud>(0)?;
        invocation.write_value(0, aligned.registration.clone())
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
    let requirements = definition.requirements.clone();
    let storage = unit_compose_core::plan_storage(&definition.graph, &resources, &requirements)
        .map_err(|error| format!("storage planning failed: {error:?}"))?;
    let configurations = configuration_summaries(&definition)?;
    let graph = definition.graph.clone();
    let workspace_bytes = definition.workspace_bytes.clone();
    let prepared = PreparedModuleDescription {
        options: BuildOptions::development(),
        requirement_status: RequirementStatus::Bounded,
        allocation_capability: AllocationCapability::inspect(
            vec![AllocationDomain {
                name: "rust-global".to_owned(),
                evidence: AllocationEvidence::Unsupported,
            }],
            false,
        ),
        warm_up_is_measured: false,
    };
    let module = Module::build(
        definition.into_executable_definition(),
        &units,
        &resources,
        BuildOptions::development(),
    )
    .map_err(|error| format!("Module build failed: {error:?}"))?;
    let input = module
        .input_handle::<PointCloudPair>(&ResourceId::new("cloud_pair"))
        .map_err(debug)?;
    let registration = module
        .output_handle::<PointCloudRegistration>(&ResourceId::new("registration_result"))
        .map_err(debug)?;
    let description = FixedModuleDescription::new(
        graph,
        configurations,
        requirements,
        workspace_bytes,
        storage.report().clone(),
        prepared,
    );
    Ok(PreparedPointCloudRegistration {
        description,
        module,
        input,
        registration,
        snapshot: None,
    })
}

fn registries() -> Result<(UnitRegistry, ResourceRegistry), String> {
    let pair = cloud_pair_type();
    let sampled = semantic("point.SampledClouds/v1")?;
    let icp = semantic("point.IcpResult/v1")?;
    let aligned = semantic("point.AlignedCloud/v1")?;
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
        .register(ResourceDescriptor::of::<SampledClouds>(
            sampled.clone(),
            "bounded sampled cloud pair",
            "source and target sample counts agree",
        ))
        .map_err(debug)?;
    resources
        .register(ResourceDescriptor::of::<ICPResult>(
            icp.clone(),
            "ICP transform result",
            "finite rigid transform",
        ))
        .map_err(debug)?;
    resources
        .register(ResourceDescriptor::of::<AlignedCloud>(
            aligned.clone(),
            "aligned point cloud",
            "aligned points and metrics are complete",
        ))
        .map_err(debug)?;
    resources
        .register(ResourceDescriptor::of::<PointCloudRegistration>(
            result.clone(),
            "point registration result",
            "transform, metrics, and aligned points are complete",
        ))
        .map_err(debug)?;
    let mut units = UnitRegistry::default();
    register_unit(
        &mut units,
        "point.sample/v1",
        vec![port::<PointCloudPair>("pair", &pair)],
        vec![port::<SampledClouds>("out", &sampled)],
        fixed_point_requirements,
    )?;
    register_unit(
        &mut units,
        "point.icp/v1",
        vec![port::<SampledClouds>("input", &sampled)],
        vec![port::<ICPResult>("out", &icp)],
        fixed_point_requirements,
    )?;
    register_unit(
        &mut units,
        "point.transform/v1",
        vec![
            port::<ICPResult>("icp", &icp),
            port::<SampledClouds>("sampled", &sampled),
        ],
        vec![port::<AlignedCloud>("out", &aligned)],
        fixed_point_requirements,
    )?;
    register_unit(
        &mut units,
        "point.metrics/v1",
        vec![port::<AlignedCloud>("input", &aligned)],
        vec![port::<PointCloudRegistration>("out", &result)],
        fixed_point_requirements,
    )?;
    register_point_executors(&mut units)?;
    Ok((units, resources))
}

fn register_point_executors(units: &mut UnitRegistry) -> Result<(), String> {
    let sample = UnitTypeName::new("point.sample/v1");
    units
        .register_factory::<DemoConfig, SampleUnit, _>(&sample, |config| {
            validate_config(config)?;
            Ok(SampleUnit {
                max_points: config.max_points,
            })
        })
        .map_err(debug)?;
    units
        .register_executor::<SampleUnit, _>(&sample, |unit, invocation, _| unit.execute(invocation))
        .map_err(debug)?;
    macro_rules! stateless {
        ($name:literal, $unit:ty, $execute:expr) => {{
            let kind = UnitTypeName::new($name);
            units
                .register_factory::<DemoConfig, $unit, _>(&kind, |config| {
                    validate_config(config)?;
                    Ok(<$unit>::default())
                })
                .map_err(debug)?;
            units
                .register_executor::<$unit, _>(&kind, $execute)
                .map_err(debug)?;
        }};
    }
    stateless!(
        "point.icp/v1",
        IcpUnit,
        |_, invocation, _| IcpUnit::execute(invocation)
    );
    stateless!("point.transform/v1", TransformUnit, |_, invocation, _| {
        TransformUnit::execute(invocation)
    });
    stateless!("point.metrics/v1", MetricsUnit, |_, invocation, _| {
        MetricsUnit::execute(invocation)
    });
    Ok(())
}

fn fixed_point_requirements(
    config: &DemoConfig,
    _: &BoundSources,
) -> Result<UnitRequirements, String> {
    validate_config(config)?;
    Ok(UnitRequirements {
        output_capacities: BTreeMap::from([("out".to_owned(), 1)]),
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
    fn yaml_rejects_invalid_bindings_but_allows_independent_stage_configuration() {
        let source = include_str!("../point-cloud-registration.yaml");
        let shortened = source
            .replace(
                "  icp:\n    type: point.icp/v1\n    config: { max_points: 4096 }\n    inputs: { input: sampled_clouds }\n    outputs: { out: icp_result }\n  transform:\n    type: point.transform/v1\n    config: { max_points: 4096 }\n    inputs: { input: icp_result }\n    outputs: { out: aligned_cloud }\n",
                "",
            )
            .replace("inputs: { input: aligned_cloud }", "inputs: { input: sampled_clouds }");
        assert!(build_from_source(&shortened).is_err());

        let mismatched = source.replacen("max_points: 4096", "max_points: 4095", 1);
        assert!(build_from_source(&mismatched).is_ok());

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
