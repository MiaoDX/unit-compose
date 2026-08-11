//! Kornia-backed image registration primitives used by the showcase pipeline.

use kornia_3d::pose::{RansacParams, ransac_homography};
use kornia_algebra::Vec2F64;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use kornia_image::Image;
use kornia_imgproc::color::gray_from_rgb_u8;
use kornia_imgproc::features::{OrbDetector, OrbFeatures, OrbMatchConfig, match_orb_descriptors};
use kornia_imgproc::warp::warp_perspective_u8;
use kornia_tensor::CpuAllocator;
use serde::Deserialize;
use unit_compose_core::{
    AllocationCapability, AllocationDomain, AllocationEvidence, BoundedBufferWriter,
    BoundedStorage, BuildOptions, FixedModuleDescription, Module, PortDescriptor,
    RequirementStatus, ResourceDescriptor, ResourceId, ResourceRegistry, RunError, SemanticType,
    Unit, UnitConfigurationSummary, UnitDescriptor, UnitId, UnitRegistry, UnitTypeName,
    UnitWorkspace,
};
use unit_compose_yaml::{
    BoundSources, CompiledDefinition, FrontendRegistry, ParseLimits, UnitRequirements, load,
};

pub const RANSAC_SEED: u64 = 0x554e_4954;
pub const MAX_IMAGE_PIXELS: usize = 2_000_000;
pub const MAX_FEATURES: usize = 800;

pub struct ImagePair {
    pub source: Image<u8, 3, CpuAllocator>,
    pub target: Image<u8, 3, CpuAllocator>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MatchPoint {
    pub source: [f32; 2],
    pub target: [f32; 2],
    pub inlier: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
struct DemoConfig {
    max_pixels: usize,
    max_matches: usize,
}

pub struct PreparedImageRegistration {
    pub description: FixedModuleDescription,
    pub module: Module<ImageRegistrationUnit>,
}

impl PreparedImageRegistration {
    pub fn snapshot(&self) -> Result<&ImageRegistration, String> {
        self.module
            .unit()
            .snapshot
            .as_ref()
            .ok_or_else(|| "image registration has no successful run".to_owned())
    }

    pub fn run(&mut self, input: &ImagePair) -> Result<(), RunError> {
        self.module.run(input).map(|_| ())
    }

    pub fn run_profiled(
        &mut self,
        input: &ImagePair,
    ) -> Result<unit_compose_core::RunReportSnapshot, RunError> {
        self.module.run_profiled(input, &mut [], None)?;
        Ok(self.module.report().snapshot())
    }
}

pub struct ImageRegistrationUnit {
    max_pixels: usize,
    max_matches: usize,
    snapshot: Option<ImageRegistration>,
}

impl ImageRegistrationUnit {
    fn from_definition(definition: &CompiledDefinition) -> Result<Self, String> {
        let config = definition
            .config::<DemoConfig>(&UnitId::new("metrics"))
            .ok_or_else(|| "missing metrics configuration".to_owned())?;
        validate_config(config)?;
        Ok(Self {
            max_pixels: config.max_pixels,
            max_matches: config.max_matches,
            snapshot: None,
        })
    }

    fn execute(
        &mut self,
        input: &ImagePair,
        output: &mut BoundedBufferWriter<'_, MatchPoint>,
        mut workspace: UnitWorkspace<'_>,
        mut recorder: Option<&mut unit_compose_core::UnitExecutionRecorder>,
    ) -> Result<(), RunError> {
        let operation =
            |unit: usize,
             operation: &mut dyn FnMut() -> Result<(), RunError>,
             recorder: &mut Option<&mut unit_compose_core::UnitExecutionRecorder>| {
                match recorder.as_deref_mut() {
                    Some(recorder) => recorder.measure(unit, operation),
                    None => operation(),
                }
            };
        if input.source.size() != input.target.size()
            || image_pixels(&input.source) > self.max_pixels
        {
            return Err(RunError::InvalidInput {
                message: "image pair dimensions exceed configured bounds",
            });
        }
        workspace.bytes().fill(0);

        let mut grayscale = None;
        let mut grayscale_stage = || {
            let mut source = Image::from_size_val(input.source.size(), 0_u8, CpuAllocator)
                .map_err(|_| registration_error())?;
            let mut target = Image::from_size_val(input.target.size(), 0_u8, CpuAllocator)
                .map_err(|_| registration_error())?;
            gray_from_rgb_u8(&input.source, &mut source).map_err(|_| registration_error())?;
            gray_from_rgb_u8(&input.target, &mut target).map_err(|_| registration_error())?;
            grayscale = Some((source, target));
            Ok(())
        };
        operation(0, &mut grayscale_stage, &mut recorder)?;

        let mut features: Option<(OrbFeatures, OrbFeatures)> = None;
        let mut orb_stage = || {
            let (source, target) = grayscale.as_ref().ok_or_else(registration_error)?;
            let detector = OrbDetector {
                n_keypoints: MAX_FEATURES,
                ..OrbDetector::default()
            };
            features = Some((
                detector
                    .detect_and_extract_u8(source)
                    .map_err(|_| registration_error())?,
                detector
                    .detect_and_extract_u8(target)
                    .map_err(|_| registration_error())?,
            ));
            Ok(())
        };
        operation(1, &mut orb_stage, &mut recorder)?;

        let mut matches = None;
        let mut match_stage = || {
            let (source, target) = features.as_ref().ok_or_else(registration_error)?;
            matches = Some(match_orb_descriptors(
                &source.orientations,
                &source.descriptors,
                &target.orientations,
                &target.descriptors,
                OrbMatchConfig::default(),
            ));
            Ok(())
        };
        operation(2, &mut match_stage, &mut recorder)?;

        let mut registration = None;
        let mut homography_stage = || {
            let (source, target) = features.take().ok_or_else(registration_error)?;
            registration = Some(
                register_correspondences(
                    source.keypoints_xy,
                    target.keypoints_xy,
                    matches.take().ok_or_else(registration_error)?,
                )
                .map_err(|_| registration_error())?,
            );
            Ok(())
        };
        operation(3, &mut homography_stage, &mut recorder)?;

        let mut warp_stage = || {
            let registration = registration.as_mut().ok_or_else(registration_error)?;
            let (source, target) = grayscale.as_ref().ok_or_else(registration_error)?;
            let mut warped = Image::from_size_val(source.size(), 0_u8, CpuAllocator)
                .map_err(|_| registration_error())?;
            warp_perspective_u8(
                source,
                &mut warped,
                &homography_matrix(registration.homography),
            )
            .map_err(|_| registration_error())?;
            registration.warped = warped.as_slice().to_vec();
            registration.target_gray = target.as_slice().to_vec();
            Ok(())
        };
        operation(4, &mut warp_stage, &mut recorder)?;

        let mut metrics_stage = || {
            let registration = registration.as_ref().ok_or_else(registration_error)?;
            for (index, &(source_index, target_index)) in registration.matches.iter().enumerate() {
                if index == self.max_matches {
                    return Err(RunError::Capacity(unit_compose_core::CapacityError {
                        resource: "registration_result",
                        required: index + 1,
                        prepared: self.max_matches,
                        policy: unit_compose_core::CapacityPolicy::RejectOverflow,
                    }));
                }
                output
                    .try_push(MatchPoint {
                        source: registration.source_keypoints[source_index],
                        target: registration.target_keypoints[target_index],
                        inlier: registration.inliers[index],
                    })
                    .map_err(RunError::Capacity)?;
            }
            output.complete();
            Ok(())
        };
        operation(5, &mut metrics_stage, &mut recorder)?;
        self.snapshot = registration;
        Ok(())
    }
}

impl Unit for ImageRegistrationUnit {
    type Input = ImagePair;
    type Storage = BoundedStorage<MatchPoint>;

    fn workspace_requirement(&self) -> usize {
        self.max_pixels / 8 + 4096
    }
    fn output_storage(&self) -> Self::Storage {
        BoundedStorage::new("registration_result", self.max_matches)
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
        if input.source.size() == input.target.size()
            && image_pixels(&input.source) <= self.max_pixels
        {
            Ok(())
        } else {
            Err(RunError::InvalidInput {
                message: "image pair dimensions exceed configured bounds",
            })
        }
    }
    fn run(
        &mut self,
        input: &Self::Input,
        output: &mut BoundedBufferWriter<'_, MatchPoint>,
        workspace: UnitWorkspace<'_>,
    ) -> Result<(), RunError> {
        self.execute(input, output, workspace, None)
    }
    fn run_with_unit_timing(
        &mut self,
        input: &Self::Input,
        output: &mut BoundedBufferWriter<'_, MatchPoint>,
        workspace: UnitWorkspace<'_>,
        recorder: &mut unit_compose_core::UnitExecutionRecorder,
    ) -> Result<(), RunError> {
        self.execute(input, output, workspace, Some(recorder))
    }
}

pub fn build_from_path(path: &Path) -> Result<PreparedImageRegistration, String> {
    build_from_source(&fs::read_to_string(path).map_err(|error| error.to_string())?)
}

fn build_from_source(source: &str) -> Result<PreparedImageRegistration, String> {
    let (units, resources, frontend) = registries()?;
    let bounds = BoundSources {
        host: BTreeMap::from([(ResourceId::new("image_pair"), MAX_IMAGE_PIXELS)]),
        adapters: BTreeMap::new(),
    };
    let definition = load(
        source,
        ParseLimits::default(),
        &frontend,
        &units,
        &resources,
        &bounds,
    )
    .map_err(|error| error.to_string())?
    .compile()
    .map_err(|error| error.to_string())?;
    validate_pipeline(&definition)?;
    let unit = ImageRegistrationUnit::from_definition(&definition)?;
    let module = Module::build(unit, BuildOptions::development())
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
    Ok(PreparedImageRegistration {
        description,
        module,
    })
}

fn registries() -> Result<(UnitRegistry, ResourceRegistry, FrontendRegistry), String> {
    let image_pair = image_pair_type();
    let stage = semantic("image.Stage/v1")?;
    let result = semantic("image.RegistrationResult/v1")?;
    let mut resources = ResourceRegistry::default();
    resources
        .register(ResourceDescriptor::of::<ImagePair>(
            image_pair.clone(),
            "bounded RGB image pair",
            "equal dimensions and bounded pixel count",
        ))
        .map_err(debug)?;
    resources
        .register(ResourceDescriptor::bounded_buffer::<
            Vec<MatchPoint>,
            MatchPoint,
        >(
            result.clone(),
            "registration matches",
            "bounded ORB correspondences",
        ))
        .map_err(debug)?;
    resources
        .register(ResourceDescriptor::bounded_buffer::<
            Vec<MatchPoint>,
            MatchPoint,
        >(
            stage.clone(),
            "registration stage value",
            "domain-local intermediate",
        ))
        .map_err(debug)?;
    let mut units = UnitRegistry::default();
    register_unit(
        &mut units,
        "image.grayscale/v1",
        vec![port::<ImagePair>("pair", &image_pair)],
        vec![port::<Vec<MatchPoint>>("out", &stage)],
    )?;
    for name in [
        "image.orb/v1",
        "image.match/v1",
        "image.homography/v1",
        "image.warp/v1",
        "image.metrics/v1",
    ] {
        let output = if name == "image.metrics/v1" {
            result.clone()
        } else {
            stage.clone()
        };
        register_unit(
            &mut units,
            name,
            vec![port::<Vec<MatchPoint>>("input", &stage)],
            vec![port::<Vec<MatchPoint>>("out", &output)],
        )?;
    }
    let mut frontend = FrontendRegistry::default();
    for name in [
        "image.grayscale/v1",
        "image.orb/v1",
        "image.match/v1",
        "image.homography/v1",
        "image.warp/v1",
        "image.metrics/v1",
    ] {
        frontend
            .register::<DemoConfig, _>(UnitTypeName::new(name), |config, _| {
                validate_config(config)?;
                Ok(UnitRequirements {
                    output_capacities: BTreeMap::from([("out".to_owned(), config.max_matches)]),
                    workspace_bytes: config.max_pixels / 8 + 4096,
                })
            })
            .map_err(debug)?;
    }
    Ok((units, resources, frontend))
}

fn validate_config(config: &DemoConfig) -> Result<(), String> {
    if config.max_pixels == 0
        || config.max_pixels > MAX_IMAGE_PIXELS
        || !(4..=MAX_FEATURES).contains(&config.max_matches)
    {
        return Err(format!(
            "image bounds require max_pixels in 1..={MAX_IMAGE_PIXELS} and max_matches in 4..={MAX_FEATURES}"
        ));
    }
    Ok(())
}

fn validate_pipeline(definition: &CompiledDefinition) -> Result<(), String> {
    const STAGES: [(&str, &str); 6] = [
        ("grayscale", "image.grayscale/v1"),
        ("orb", "image.orb/v1"),
        ("match", "image.match/v1"),
        ("homography", "image.homography/v1"),
        ("warp", "image.warp/v1"),
        ("metrics", "image.metrics/v1"),
    ];
    let expected_order = STAGES.map(|(id, _)| UnitId::new(id));
    if definition.graph.execution_order != expected_order
        || definition.graph.module_outputs != [ResourceId::new("registration_result")]
    {
        return Err("image registration requires the fixed grayscale -> orb -> match -> homography -> warp -> metrics pipeline".to_owned());
    }
    let expected_config = definition
        .config::<DemoConfig>(&expected_order[0])
        .ok_or_else(|| "missing config for grayscale".to_owned())?;
    for (index, ((id, unit_type), unit_id)) in STAGES.iter().zip(&expected_order).enumerate() {
        let unit = definition
            .graph
            .units
            .iter()
            .find(|unit| &unit.id == unit_id)
            .ok_or_else(|| format!("missing fixed image stage {id}"))?;
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
                "image registration stages must use the fixed pipeline and identical bounds"
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
                summary: format!(
                    "max_pixels={},max_matches={}",
                    config.max_pixels, config.max_matches
                ),
            })
        })
        .collect()
}

fn register_unit(
    units: &mut UnitRegistry,
    name: &str,
    inputs: Vec<PortDescriptor>,
    outputs: Vec<PortDescriptor>,
) -> Result<(), String> {
    units
        .register(UnitDescriptor {
            type_name: UnitTypeName::new(name),
            inputs,
            outputs,
        })
        .map_err(debug)
}
fn port<T: 'static>(name: &str, semantic_type: &SemanticType) -> PortDescriptor {
    PortDescriptor::of::<T>(name, semantic_type.clone())
}
fn semantic(name: &str) -> Result<SemanticType, String> {
    SemanticType::new(name).map_err(debug)
}
fn image_pair_type() -> SemanticType {
    SemanticType::new("image.ImagePair/v1").expect("static semantic type is valid")
}
fn debug(error: impl std::fmt::Debug) -> String {
    format!("{error:?}")
}

#[derive(Clone, Debug)]
pub struct ImageRegistration {
    pub source_keypoints: Vec<[f32; 2]>,
    pub target_keypoints: Vec<[f32; 2]>,
    pub matches: Vec<(usize, usize)>,
    pub inliers: Vec<bool>,
    pub homography: [[f64; 3]; 3],
    pub reprojection_rmse: f64,
    pub warped: Vec<u8>,
    pub target_gray: Vec<u8>,
}

fn register_correspondences(
    source_keypoints: Vec<[f32; 2]>,
    target_keypoints: Vec<[f32; 2]>,
    matches: Vec<(usize, usize)>,
) -> Result<ImageRegistration, String> {
    if matches.len() < 4 {
        return Err(format!(
            "at least four matches are required, found {}",
            matches.len()
        ));
    }
    if let Some(&(source, target)) = matches.iter().find(|&&(source, target)| {
        source >= source_keypoints.len() || target >= target_keypoints.len()
    }) {
        return Err(format!(
            "match index ({source}, {target}) exceeds keypoint bounds ({}, {})",
            source_keypoints.len(),
            target_keypoints.len()
        ));
    }
    let source = matches
        .iter()
        .map(|&(a, _)| Vec2F64::new(source_keypoints[a][0] as f64, source_keypoints[a][1] as f64))
        .collect::<Vec<_>>();
    let target = matches
        .iter()
        .map(|&(_, b)| Vec2F64::new(target_keypoints[b][0] as f64, target_keypoints[b][1] as f64))
        .collect::<Vec<_>>();
    let result = ransac_homography(
        &source,
        &target,
        &RansacParams {
            max_iterations: 2_000,
            threshold: 2.0,
            min_inliers: 4,
            random_seed: Some(RANSAC_SEED),
            refit: true,
        },
    )
    .map_err(|error| error.to_string())?;
    let values = result.model.to_cols_array();
    let homography = [
        [values[0], values[3], values[6]],
        [values[1], values[4], values[7]],
        [values[2], values[5], values[8]],
    ];
    let reprojection_rmse = reprojection_rmse(&homography, &source, &target, &result.inliers);
    Ok(ImageRegistration {
        source_keypoints,
        target_keypoints,
        matches,
        inliers: result.inliers,
        homography,
        reprojection_rmse,
        warped: Vec::new(),
        target_gray: Vec::new(),
    })
}

fn homography_matrix(homography: [[f64; 3]; 3]) -> [f32; 9] {
    [
        homography[0][0] as f32,
        homography[0][1] as f32,
        homography[0][2] as f32,
        homography[1][0] as f32,
        homography[1][1] as f32,
        homography[1][2] as f32,
        homography[2][0] as f32,
        homography[2][1] as f32,
        homography[2][2] as f32,
    ]
}

fn image_pixels<const C: usize>(image: &Image<u8, C, CpuAllocator>) -> usize {
    image.size().width * image.size().height
}

fn registration_error() -> RunError {
    RunError::InvalidInput {
        message: "Kornia image registration failed",
    }
}

fn reprojection_rmse(
    homography: &[[f64; 3]; 3],
    source: &[Vec2F64],
    target: &[Vec2F64],
    inliers: &[bool],
) -> f64 {
    let mut squared_error = 0.0;
    let mut count = 0;
    for ((point, expected), &inlier) in source.iter().zip(target).zip(inliers) {
        if !inlier {
            continue;
        }
        let scale = homography[2][0] * point.x + homography[2][1] * point.y + homography[2][2];
        let x =
            (homography[0][0] * point.x + homography[0][1] * point.y + homography[0][2]) / scale;
        let y =
            (homography[1][0] * point.x + homography[1][1] * point.y + homography[1][2]) / scale;
        squared_error += (x - expected.x).powi(2) + (y - expected.y).powi(2);
        count += 1;
    }
    (squared_error / count as f64).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;
    use kornia_image::ImageSize;

    #[test]
    fn seeded_ransac_recovers_known_homography_repeatably() {
        let source = vec![
            [0.0, 0.0],
            [40.0, 0.0],
            [80.0, 0.0],
            [0.0, 30.0],
            [40.0, 30.0],
            [80.0, 30.0],
            [0.0, 60.0],
            [40.0, 60.0],
            [80.0, 60.0],
            [20.0, 45.0],
        ];
        let transform = |[x, y]: [f32; 2]| [1.05 * x + 0.08 * y + 7.0, -0.04 * x + 0.98 * y + 5.0];
        let mut target = source.iter().copied().map(transform).collect::<Vec<_>>();
        target[9] = [300.0, -200.0];
        let matches = (0..source.len())
            .map(|index| (index, index))
            .collect::<Vec<_>>();
        let first =
            register_correspondences(source.clone(), target.clone(), matches.clone()).unwrap();
        let second = register_correspondences(source, target, matches).unwrap();
        assert_eq!(first.inliers, second.inliers);
        assert_eq!(first.homography, second.homography);
        assert_eq!(first.inliers.iter().filter(|&&value| value).count(), 9);
        assert!(
            first.reprojection_rmse < 1e-4,
            "rmse={}",
            first.reprojection_rmse
        );
    }

    #[test]
    fn correspondence_indices_must_reference_existing_keypoints() {
        let points = vec![[0.0, 0.0]; 4];
        let source_error = register_correspondences(
            points.clone(),
            points.clone(),
            vec![(0, 0), (1, 1), (2, 2), (4, 3)],
        )
        .unwrap_err();
        assert!(source_error.contains("match index (4, 3)"));

        let target_error =
            register_correspondences(points.clone(), points, vec![(0, 0), (1, 1), (2, 2), (3, 4)])
                .unwrap_err();
        assert!(target_error.contains("match index (3, 4)"));
    }

    #[test]
    fn yaml_rejects_unsupported_bounds_before_allocating() {
        let source = include_str!("../image-registration.yaml").replace(
            "max_matches: 800",
            &format!("max_matches: {}", MAX_FEATURES + 1),
        );
        assert!(build_from_source(&source).is_err());
    }

    #[test]
    fn yaml_rejects_pipeline_or_config_mismatches() {
        let source = include_str!("../image-registration.yaml");
        let shortened = source.replace(
            "  orb:\n    type: image.orb/v1\n    config: { max_pixels: 2000000, max_matches: 800 }\n    inputs: { input: grayscale_pair }\n    outputs: { out: orb_features }\n  match:\n    type: image.match/v1\n    config: { max_pixels: 2000000, max_matches: 800 }\n    inputs: { input: orb_features }\n    outputs: { out: candidate_matches }\n  homography:\n    type: image.homography/v1\n    config: { max_pixels: 2000000, max_matches: 800 }\n    inputs: { input: candidate_matches }\n    outputs: { out: inlier_matches }\n  warp:\n    type: image.warp/v1\n    config: { max_pixels: 2000000, max_matches: 800 }\n    inputs: { input: inlier_matches }\n    outputs: { out: warped_image }\n",
            "",
        )
        .replace("inputs: { input: warped_image }", "inputs: { input: grayscale_pair }");
        assert!(build_from_source(&shortened).is_err());

        let mismatched = source.replacen("max_matches: 800", "max_matches: 799", 1);
        assert!(build_from_source(&mismatched).is_err());

        let wrong_output = source.replace(
            "outputs:\n  result: registration_result",
            "outputs:\n  result: grayscale_pair",
        );
        assert!(build_from_source(&wrong_output).is_err());
    }

    #[test]
    fn yaml_module_runs_and_times_every_planned_unit() {
        let source = include_str!("../image-registration.yaml");
        let mut prepared = build_from_source(source).unwrap();
        let size = ImageSize {
            width: 256,
            height: 256,
        };
        let grayscale = (0..size.height)
            .flat_map(|y| {
                (0..size.width).map(move |x| {
                    if ((x / 16) ^ (y / 16)) & 1 == 0 {
                        20
                    } else {
                        230
                    }
                })
            })
            .collect::<Vec<_>>();
        let pixels = grayscale
            .iter()
            .flat_map(|&value| [value, value, value])
            .collect::<Vec<_>>();
        let pair = ImagePair {
            source: Image::from_size_slice(size, &pixels, CpuAllocator).unwrap(),
            target: Image::from_size_slice(size, &pixels, CpuAllocator).unwrap(),
        };
        let mut reports = Vec::new();
        for _ in 0..2 {
            reports.push(prepared.run_profiled(&pair).unwrap());
        }
        let graph = prepared.description.to_mermaid();
        for unit in ["grayscale", "orb", "match", "homography", "warp", "metrics"] {
            assert!(graph.contains(&format!("{unit}<br/>Unit")));
        }
        let timed = prepared.description.to_mermaid_with_runs(&reports);
        assert_eq!(timed.matches("n=2").count(), 6);
        assert!(timed.contains("avg ") && timed.contains("p99 "));
    }
}
