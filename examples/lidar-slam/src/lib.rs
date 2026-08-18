//! Offline deterministic planar LiDAR SLAM showcase.

use serde::Deserialize;
use slamwich::{Point3D, PointCloud, Pose, ScanContextConfig, SlamConfig, SlamProcessor};
use std::collections::{BTreeMap, VecDeque};
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

pub const DEFAULT_FRAMES: usize = 480;
pub const MIN_EPISODE_FRAMES: usize = 100;
pub const MAX_EPISODE_FRAMES: usize = 500;
pub const MAX_INPUT_POINTS: usize = 2_048;
pub const MAX_SCAN_POINTS: usize = 384;
pub const MAX_TRAIL_POSES: usize = 512;
pub const MAX_KEYFRAME_POSES: usize = 160;
pub const MAX_MAP_POINTS: usize = 2_048;
pub const MAX_EDGES: usize = 320;
const ROOM_HALF_X: f64 = 13.0;
const ROOM_HALF_Y: f64 = 9.0;
const MAX_ABS_POSE_TRANSLATION: f64 = 100.0;
const MAX_ABS_SCAN_COORDINATE: f32 = 100.0;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PlanarPose {
    pub x: f64,
    pub y: f64,
    pub theta: f64,
}

impl From<Pose> for PlanarPose {
    fn from(value: Pose) -> Self {
        Self {
            x: value.x,
            y: value.y,
            theta: value.theta,
        }
    }
}

impl From<PlanarPose> for Pose {
    fn from(value: PlanarPose) -> Self {
        Self {
            x: value.x,
            y: value.y,
            theta: value.theta,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScanPoint {
    pub xyz: [f32; 3],
    pub reflectivity: u8,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LidarFrame {
    pub frame_index: usize,
    pub timestamp_ns: u64,
    pub odometry: PlanarPose,
    pub reference: PlanarPose,
    pub scan: Vec<ScanPoint>,
}

#[derive(Clone, Debug)]
struct PreparedScan {
    odometry: PlanarPose,
    sampled: Vec<ScanPoint>,
    cloud: PointCloud,
}

#[derive(Clone, Debug)]
struct SlamObservation {
    estimated: PlanarPose,
    odom_correction: PlanarPose,
    updated: bool,
    keyframe_event: bool,
    loop_event: bool,
    update_count: usize,
    keyframe_count: usize,
    loop_count: usize,
    current_scan: Vec<ScanPoint>,
    keyframe_poses: Vec<PlanarPose>,
    map_points: Vec<[f32; 3]>,
    edges: Vec<PoseEdge>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameStatus {
    Updated,
    NoUpdate,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PoseEdge {
    pub from: usize,
    pub to: usize,
    pub loop_closure: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SlamSnapshot {
    pub frame_index: usize,
    pub timestamp_ns: u64,
    pub status: FrameStatus,
    pub estimated: PlanarPose,
    pub odometry: PlanarPose,
    pub reference: PlanarPose,
    pub odom_correction: PlanarPose,
    pub update_event: bool,
    pub keyframe_event: bool,
    pub loop_event: bool,
    pub update_count: usize,
    pub keyframe_count: usize,
    pub loop_count: usize,
    pub accepted_points: usize,
    pub dropped_points: usize,
    pub scan_capacity: usize,
    pub map_capacity: usize,
    pub estimated_trail: Vec<PlanarPose>,
    pub odometry_trail: Vec<PlanarPose>,
    pub reference_trail: Vec<PlanarPose>,
    pub current_scan: Vec<ScanPoint>,
    pub keyframe_poses: Vec<PlanarPose>,
    pub map_points: Vec<[f32; 3]>,
    pub edges: Vec<PoseEdge>,
    pub translation_error: f64,
    pub rotation_error: f64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
struct DemoConfig {
    max_scan_points: usize,
    max_trail_poses: usize,
    max_keyframe_poses: usize,
    max_map_points: usize,
    max_edges: usize,
}

pub struct PreparedLidarSlam {
    pub description: FixedModuleDescription,
    pub module: CompositeModule<LidarSlamUnit>,
}

impl PreparedLidarSlam {
    pub fn run(&mut self, frame: &LidarFrame) -> Result<SlamSnapshot, RunError> {
        self.module.run(frame).map(|values| values[0].clone())
    }

    pub fn run_profiled(&mut self, frame: &LidarFrame) -> Result<SlamSnapshot, RunError> {
        self.module
            .run_profiled(frame, &mut [], None)
            .map(|values| values[0].clone())
    }

    pub fn last_snapshot(&self) -> Option<&SlamSnapshot> {
        self.module.unit().last_snapshot.as_ref()
    }
}

pub struct LidarSlamUnit {
    config: DemoConfig,
    slam: SlamProcessor,
    last_frame_index: Option<usize>,
    last_timestamp_ns: Option<u64>,
    updates: usize,
    estimated_trail: VecDeque<PlanarPose>,
    odometry_trail: VecDeque<PlanarPose>,
    reference_trail: VecDeque<PlanarPose>,
    last_snapshot: Option<SlamSnapshot>,
}

impl LidarSlamUnit {
    fn from_definition(definition: &CompiledDefinition) -> Result<Self, String> {
        let config = *definition
            .config::<DemoConfig>(&UnitId::new("slam"))
            .ok_or_else(|| "missing slam configuration".to_owned())?;
        validate_config(&config)?;
        let slam_config = SlamConfig {
            keyframe_distance: 0.45,
            keyframe_rotation: 0.22,
            loop_closure_search_radius: 8.0,
            loop_closure_max_disagreement_sigma: 1.0e9,
            scan_context: ScanContextConfig {
                top_k: 20,
                ring_key_prefilter: 96,
                ..ScanContextConfig::default()
            },
            ..SlamConfig::default()
        };
        Ok(Self {
            config,
            slam: SlamProcessor::new(slam_config),
            last_frame_index: None,
            last_timestamp_ns: None,
            updates: 0,
            estimated_trail: VecDeque::with_capacity(config.max_trail_poses),
            odometry_trail: VecDeque::with_capacity(config.max_trail_poses),
            reference_trail: VecDeque::with_capacity(config.max_trail_poses),
            last_snapshot: None,
        })
    }

    fn validate_frame(&self, frame: &LidarFrame) -> Result<(), RunError> {
        let valid_pose = |pose: PlanarPose| {
            pose.x.is_finite()
                && pose.y.is_finite()
                && pose.theta.is_finite()
                && pose.x.abs() <= MAX_ABS_POSE_TRANSLATION
                && pose.y.abs() <= MAX_ABS_POSE_TRANSLATION
        };
        if frame.scan.is_empty()
            || frame.scan.len() > MAX_INPUT_POINTS
            || !valid_pose(frame.odometry)
            || !valid_pose(frame.reference)
            || frame.scan.iter().any(|point| {
                point
                    .xyz
                    .iter()
                    .any(|value| !value.is_finite() || value.abs() > MAX_ABS_SCAN_COORDINATE)
            })
            || self
                .last_frame_index
                .is_some_and(|index| index.checked_add(1) != Some(frame.frame_index))
            || self
                .last_timestamp_ns
                .is_some_and(|timestamp| frame.timestamp_ns <= timestamp)
        {
            return Err(RunError::InvalidInput {
                message: "LiDAR frame is corrupt, non-finite, out of bounds, or out of order",
            });
        }
        Ok(())
    }

    fn prepare_scan(&self, frame: &LidarFrame) -> PreparedScan {
        let sampled = bounded_sample(&frame.scan, self.config.max_scan_points);
        let cloud = PointCloud::new(
            sampled
                .iter()
                .map(|point| Point3D {
                    x: point.xyz[0],
                    y: point.xyz[1],
                    z: point.xyz[2],
                    reflectivity: point.reflectivity,
                    tag: 0,
                })
                .collect(),
        );
        PreparedScan {
            odometry: frame.odometry,
            sampled,
            cloud,
        }
    }

    fn update_slam(&mut self, prepared: PreparedScan) -> SlamObservation {
        self.slam.update_odometry(&prepared.odometry.into());
        let update = self.slam.process_scan(&prepared.cloud);
        self.updates += usize::from(update.is_some());
        let estimated = PlanarPose::from(self.slam.pose());
        let all_keyframe_poses = self.slam.keyframe_poses();
        let keyframe_start = all_keyframe_poses
            .len()
            .saturating_sub(self.config.max_keyframe_poses);
        let keyframe_poses = all_keyframe_poses
            .into_iter()
            .rev()
            .take(self.config.max_keyframe_poses)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .map(|(x, y, theta)| PlanarPose { x, y, theta })
            .collect::<Vec<_>>();
        let map_points = sampled_map(&self.slam, self.config.max_map_points);
        let edges = self
            .slam
            .edges()
            .iter()
            .filter(|edge| edge.from_id >= keyframe_start && edge.to_id >= keyframe_start)
            .rev()
            .take(self.config.max_edges)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .map(|edge| PoseEdge {
                from: edge.from_id - keyframe_start,
                to: edge.to_id - keyframe_start,
                loop_closure: edge.is_loop_closure,
            })
            .collect();
        let odom_correction = self.slam.odom_correction();
        let correction_translation = odom_correction.translation();
        let correction = PlanarPose {
            x: correction_translation.x,
            y: correction_translation.y,
            theta: odom_correction.rotation(),
        };
        SlamObservation {
            estimated,
            odom_correction: correction,
            updated: update.is_some(),
            keyframe_event: update.as_ref().is_some_and(|value| value.keyframe_added),
            loop_event: update
                .as_ref()
                .is_some_and(|value| value.loop_closure_detected),
            update_count: self.updates,
            keyframe_count: self.slam.keyframe_count(),
            loop_count: self.slam.loop_closure_count(),
            current_scan: prepared.sampled,
            keyframe_poses,
            map_points,
            edges,
        }
    }

    fn build_snapshot(&mut self, frame: &LidarFrame, observation: SlamObservation) -> SlamSnapshot {
        push_bounded(
            &mut self.estimated_trail,
            observation.estimated,
            self.config.max_trail_poses,
        );
        push_bounded(
            &mut self.odometry_trail,
            frame.odometry,
            self.config.max_trail_poses,
        );
        push_bounded(
            &mut self.reference_trail,
            frame.reference,
            self.config.max_trail_poses,
        );
        let translation_error = (observation.estimated.x - frame.reference.x)
            .hypot(observation.estimated.y - frame.reference.y);
        let rotation_error =
            normalize_angle(observation.estimated.theta - frame.reference.theta).abs();
        let snapshot = SlamSnapshot {
            frame_index: frame.frame_index,
            timestamp_ns: frame.timestamp_ns,
            status: if observation.updated {
                FrameStatus::Updated
            } else {
                FrameStatus::NoUpdate
            },
            estimated: observation.estimated,
            odometry: frame.odometry,
            reference: frame.reference,
            odom_correction: observation.odom_correction,
            update_event: observation.updated,
            keyframe_event: observation.keyframe_event,
            loop_event: observation.loop_event,
            update_count: observation.update_count,
            keyframe_count: observation.keyframe_count,
            loop_count: observation.loop_count,
            accepted_points: observation.current_scan.len(),
            dropped_points: frame.scan.len() - observation.current_scan.len(),
            scan_capacity: self.config.max_scan_points,
            map_capacity: self.config.max_map_points,
            estimated_trail: self.estimated_trail.iter().copied().collect(),
            odometry_trail: self.odometry_trail.iter().copied().collect(),
            reference_trail: self.reference_trail.iter().copied().collect(),
            current_scan: observation.current_scan,
            keyframe_poses: observation.keyframe_poses,
            map_points: observation.map_points,
            edges: observation.edges,
            translation_error,
            rotation_error,
        };
        self.last_frame_index = Some(frame.frame_index);
        self.last_timestamp_ns = Some(frame.timestamp_ns);
        self.last_snapshot = Some(snapshot.clone());
        snapshot
    }

    fn execute(
        &mut self,
        frame: &LidarFrame,
        mut recorder: Option<&mut unit_compose_core::UnitExecutionRecorder>,
    ) -> Result<SlamSnapshot, RunError> {
        let prepared = measure_stage(recorder.as_deref_mut(), 0, || Ok(self.prepare_scan(frame)))?;
        let observation =
            measure_stage(
                recorder.as_deref_mut(),
                1,
                || Ok(self.update_slam(prepared)),
            )?;
        measure_stage(recorder, 2, || Ok(self.build_snapshot(frame, observation)))
    }
}

fn measure_stage<T>(
    recorder: Option<&mut unit_compose_core::UnitExecutionRecorder>,
    ordinal: usize,
    operation: impl FnOnce() -> Result<T, RunError>,
) -> Result<T, RunError> {
    match recorder {
        Some(recorder) => recorder.measure(ordinal, operation),
        None => operation(),
    }
}

impl Unit for LidarSlamUnit {
    type Input = LidarFrame;
    type Storage = BoundedStorage<SlamSnapshot>;
    fn workspace_requirement(&self) -> usize {
        0
    }
    fn output_storage(&self) -> Self::Storage {
        BoundedStorage::new("slam_snapshot", 1)
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
        self.validate_frame(input)
    }
    fn run(
        &mut self,
        input: &Self::Input,
        output: &mut BoundedBufferWriter<'_, SlamSnapshot>,
        _workspace: UnitWorkspace<'_>,
    ) -> Result<(), RunError> {
        let snapshot = self.execute(input, None)?;
        output.try_push(snapshot).map_err(RunError::Capacity)?;
        output.complete();
        Ok(())
    }

    fn run_with_unit_timing(
        &mut self,
        input: &Self::Input,
        output: &mut BoundedBufferWriter<'_, SlamSnapshot>,
        _workspace: UnitWorkspace<'_>,
        recorder: &mut unit_compose_core::UnitExecutionRecorder,
    ) -> Result<(), RunError> {
        let snapshot = self.execute(input, Some(recorder))?;
        output.try_push(snapshot).map_err(RunError::Capacity)?;
        output.complete();
        Ok(())
    }
}

pub fn build_from_path(path: &Path) -> Result<PreparedLidarSlam, String> {
    build_from_source(&fs::read_to_string(path).map_err(|error| error.to_string())?)
}

pub fn build_from_source(source: &str) -> Result<PreparedLidarSlam, String> {
    let (units, resources) = registries()?;
    let bounds = BoundSources {
        host: BTreeMap::from([(ResourceId::new("lidar_frame"), MAX_INPUT_POINTS)]),
        adapters: BTreeMap::new(),
    };
    let definition = load(source, ParseLimits::default(), &units, &resources, &bounds)
        .map_err(|error| error.to_string())?
        .compile()
        .map_err(|error| error.to_string())?;
    validate_pipeline(&definition)?;
    validate_stage_configs(&definition)?;
    let unit = LidarSlamUnit::from_definition(&definition)?;
    let module = CompositeModule::build(unit, BuildOptions::development())
        .map_err(|error| format!("Module build failed: {error:?}"))?;
    let storage =
        unit_compose_core::plan_storage(&definition.graph, &resources, &definition.requirements)
            .map_err(|error| format!("storage planning failed: {error:?}"))?;
    let config = definition
        .config::<DemoConfig>(&UnitId::new("slam"))
        .ok_or_else(|| "missing slam configuration".to_owned())?;
    let configurations = definition
        .graph
        .units
        .iter()
        .map(|unit| UnitConfigurationSummary {
            unit: unit.id.clone(),
            summary: format!(
                "scan<={} trail<={} keyframes<={} map<={} edges<={}",
                config.max_scan_points,
                config.max_trail_poses,
                config.max_keyframe_poses,
                config.max_map_points,
                config.max_edges
            ),
        })
        .collect();
    let description = FixedModuleDescription::new(
        definition.graph.clone(),
        configurations,
        definition.requirements.clone(),
        definition.workspace_bytes,
        storage.report().clone(),
        module.description().clone(),
    );
    Ok(PreparedLidarSlam {
        description,
        module,
    })
}

fn validate_stage_configs(definition: &CompiledDefinition) -> Result<(), String> {
    let expected = definition
        .config::<DemoConfig>(&UnitId::new("slam"))
        .ok_or_else(|| "missing slam configuration".to_owned())?;
    for unit_id in [UnitId::new("scan_prepare"), UnitId::new("snapshot")] {
        let actual = definition
            .config::<DemoConfig>(&unit_id)
            .ok_or_else(|| format!("missing {} configuration", unit_id.as_str()))?;
        if actual != expected {
            return Err(format!(
                "LiDAR SLAM stage configurations must match; {} differs from slam",
                unit_id.as_str()
            ));
        }
    }
    Ok(())
}

fn registries() -> Result<(UnitRegistry, ResourceRegistry), String> {
    let frame_type = semantic("lidar.SynchronizedFrame/v1")?;
    let prepared_type = semantic("lidar.PreparedFrame/v1")?;
    let observation_type = semantic("lidar.SlamObservation/v1")?;
    let snapshot_type = semantic("lidar.SlamSnapshot/v1")?;
    let mut resources = ResourceRegistry::default();
    resources
        .register(ResourceDescriptor::of::<LidarFrame>(
            frame_type.clone(),
            "bounded synchronized LiDAR, odometry, and evaluation reference frame",
            "finite, ordered, bounded scan; reference is evaluation-only",
        ))
        .map_err(debug)?;
    resources
        .register(ResourceDescriptor::of::<PreparedScan>(
            prepared_type.clone(),
            "bounded sampled LiDAR scan, odometry, and Slamwich point cloud",
            "finite sampled points with evaluation reference kept out of Slamwich",
        ))
        .map_err(debug)?;
    resources
        .register(ResourceDescriptor::of::<SlamObservation>(
            observation_type.clone(),
            "bounded stateful Slamwich observation",
            "optimized pose, bounded map and graph evidence for one frame",
        ))
        .map_err(debug)?;
    resources
        .register(ResourceDescriptor::bounded_buffer::<
            Vec<SlamSnapshot>,
            SlamSnapshot,
        >(
            snapshot_type.clone(),
            "owned bounded SLAM snapshot",
            "exactly one snapshot after successful publication",
        ))
        .map_err(debug)?;
    let mut units = UnitRegistry::default();
    register_yaml_unit::<DemoConfig, _>(
        &mut units,
        UnitDescriptor {
            type_name: UnitTypeName::new("lidar.scan_prepare/v1"),
            inputs: vec![PortDescriptor::of::<LidarFrame>(
                "frame",
                frame_type.clone(),
            )],
            outputs: vec![PortDescriptor::of::<PreparedScan>(
                "prepared",
                prepared_type.clone(),
            )],
        },
        |config, _| lidar_requirements(config, "prepared"),
    )
    .map_err(debug)?;
    register_yaml_unit::<DemoConfig, _>(
        &mut units,
        UnitDescriptor {
            type_name: UnitTypeName::new("lidar.slamwich/v1"),
            inputs: vec![PortDescriptor::of::<PreparedScan>(
                "prepared",
                prepared_type,
            )],
            outputs: vec![PortDescriptor::of::<SlamObservation>(
                "observation",
                observation_type.clone(),
            )],
        },
        |config, _| lidar_requirements(config, "observation"),
    )
    .map_err(debug)?;
    register_yaml_unit::<DemoConfig, _>(
        &mut units,
        UnitDescriptor {
            type_name: UnitTypeName::new("lidar.snapshot/v1"),
            inputs: vec![
                PortDescriptor::of::<LidarFrame>("frame", frame_type),
                PortDescriptor::of::<SlamObservation>("observation", observation_type),
            ],
            outputs: vec![PortDescriptor::of::<Vec<SlamSnapshot>>(
                "snapshot",
                snapshot_type,
            )],
        },
        |config, _| lidar_requirements(config, "snapshot"),
    )
    .map_err(debug)?;
    Ok((units, resources))
}

fn lidar_requirements(config: &DemoConfig, output: &str) -> Result<UnitRequirements, String> {
    validate_config(config)?;
    Ok(UnitRequirements {
        output_capacities: BTreeMap::from([(output.to_owned(), 1)]),
        workspace_bytes: 0,
    })
}

fn validate_pipeline(definition: &CompiledDefinition) -> Result<(), String> {
    type ExpectedUnit<'a> = (
        &'a str,
        &'a str,
        &'a [(&'a str, &'a str)],
        (&'a str, &'a str),
    );

    let expected_order = [
        UnitId::new("scan_prepare"),
        UnitId::new("slam"),
        UnitId::new("snapshot"),
    ];
    let expected_units: [ExpectedUnit<'_>; 3] = [
        (
            "scan_prepare",
            "lidar.scan_prepare/v1",
            &[("frame", "lidar_frame")],
            ("prepared", "prepared_frame"),
        ),
        (
            "slam",
            "lidar.slamwich/v1",
            &[("prepared", "prepared_frame")],
            ("observation", "slam_observation"),
        ),
        (
            "snapshot",
            "lidar.snapshot/v1",
            &[
                ("frame", "lidar_frame"),
                ("observation", "slam_observation"),
            ],
            ("snapshot", "slam_snapshot"),
        ),
    ];
    let topology_matches = definition.graph.units.len() == expected_units.len()
        && definition.graph.units.iter().zip(expected_units).all(
            |(unit, (id, unit_type, inputs, output))| {
                unit.id.as_str() == id
                    && unit.unit_type.as_str() == unit_type
                    && unit.inputs.len() == inputs.len()
                    && unit.inputs.iter().zip(inputs).all(|(actual, expected)| {
                        actual.port == expected.0 && actual.resource.as_str() == expected.1
                    })
                    && unit.outputs.len() == 1
                    && unit.outputs[0].port == output.0
                    && unit.outputs[0].resource.as_str() == output.1
            },
        );
    if definition.graph.execution_order != expected_order
        || definition.graph.module_outputs != [ResourceId::new("slam_snapshot")]
        || !topology_matches
    {
        return Err(
            "LiDAR SLAM requires fixed scan_prepare -> slam -> snapshot bindings and one snapshot output"
                .to_owned(),
        );
    }
    Ok(())
}

fn validate_config(config: &DemoConfig) -> Result<(), String> {
    if !(32..=MAX_SCAN_POINTS).contains(&config.max_scan_points)
        || !(MIN_EPISODE_FRAMES..=MAX_TRAIL_POSES).contains(&config.max_trail_poses)
        || !(1..=MAX_KEYFRAME_POSES).contains(&config.max_keyframe_poses)
        || !(64..=MAX_MAP_POINTS).contains(&config.max_map_points)
        || !(1..=MAX_EDGES).contains(&config.max_edges)
    {
        return Err("LiDAR SLAM YAML bounds exceed the compiled example limits".to_owned());
    }
    Ok(())
}

pub fn synthetic_episode(frames: usize) -> Result<Vec<LidarFrame>, String> {
    if !(MIN_EPISODE_FRAMES..=MAX_EPISODE_FRAMES).contains(&frames) {
        return Err(format!(
            "episode frames must be in {MIN_EPISODE_FRAMES}..={MAX_EPISODE_FRAMES}"
        ));
    }
    let landmarks = room_landmarks();
    let mut result = Vec::with_capacity(frames);
    let moving_steps = frames - frames / 20;
    let mut moving = 0;
    for frame_index in 0..frames {
        if frame_index != 0 && frame_index % 20 != 0 {
            moving += 1;
        }
        let t = moving.min(moving_steps - 1) as f64 / (moving_steps - 1) as f64;
        let reference = route_pose(t);
        let drift = moving as f64;
        let odometry = PlanarPose {
            x: reference.x * 1.012 + 0.0012 * drift,
            y: reference.y * 0.992 - 0.0007 * drift,
            theta: normalize_angle(reference.theta + 0.0009 * drift),
        };
        result.push(LidarFrame {
            frame_index,
            timestamp_ns: 1_000_000_000 + frame_index as u64 * 100_000_000,
            odometry,
            reference,
            scan: sensor_scan(&landmarks, reference),
        });
    }
    Ok(result)
}

pub fn run_episode(
    prepared: &mut PreparedLidarSlam,
    frames: usize,
    profiled: bool,
) -> Result<(Vec<SlamSnapshot>, Vec<unit_compose_core::RunReportSnapshot>), String> {
    let mut snapshots = Vec::with_capacity(frames);
    let mut reports = Vec::with_capacity(frames);
    for frame in synthetic_episode(frames)? {
        let snapshot = if profiled {
            prepared.run_profiled(&frame)
        } else {
            prepared.run(&frame)
        }
        .map_err(|error| format!("frame {} failed: {error:?}", frame.frame_index))?;
        snapshots.push(snapshot);
        reports.push(prepared.module.report().snapshot());
    }
    Ok((snapshots, reports))
}

fn route_pose(t: f64) -> PlanarPose {
    let angle = std::f64::consts::TAU * t;
    let x = 8.8 * angle.sin();
    let y = 6.2 * (2.0 * angle).sin();
    let dx = 8.8 * angle.cos();
    let dy = 12.4 * (2.0 * angle).cos();
    PlanarPose {
        x,
        y,
        theta: dy.atan2(dx),
    }
}

fn room_landmarks() -> Vec<[f64; 3]> {
    let mut points = Vec::new();
    for i in 0..96 {
        let u = i as f64 / 95.0;
        let x = -ROOM_HALF_X + 2.0 * ROOM_HALF_X * u;
        let y = -ROOM_HALF_Y + 2.0 * ROOM_HALF_Y * u;
        for z in [0.15, 0.9, 1.65] {
            points.extend([
                [x, -ROOM_HALF_Y, z],
                [x, ROOM_HALF_Y, z],
                [-ROOM_HALF_X, y, z],
                [ROOM_HALF_X, y, z],
            ]);
        }
    }
    for &(x, y, radius, height) in &[
        (-7.8, -3.1, 0.28, 2.0),
        (-4.6, 5.7, 0.42, 2.8),
        (0.9, -5.9, 0.22, 1.4),
        (7.9, 3.6, 0.55, 3.4),
        (2.2, 6.2, 0.34, 2.3),
        (7.1, -1.0, 0.25, 1.7),
    ] {
        for ring in 0..16 {
            let a = std::f64::consts::TAU * ring as f64 / 16.0;
            for level in 1..=5 {
                let z = height * f64::from(level) / 5.0;
                points.push([x + radius * a.cos(), y + radius * a.sin(), z]);
            }
        }
    }
    points
}

fn sensor_scan(landmarks: &[[f64; 3]], pose: PlanarPose) -> Vec<ScanPoint> {
    let (sin, cos) = pose.theta.sin_cos();
    landmarks
        .iter()
        .filter_map(|point| {
            let dx = point[0] - pose.x;
            let dy = point[1] - pose.y;
            let x = cos * dx + sin * dy;
            let y = -sin * dx + cos * dy;
            ((x * x + y * y).sqrt() <= 14.0).then_some(ScanPoint {
                xyz: [x as f32, y as f32, (point[2] - 1.0) as f32],
                reflectivity: (90.0 + point[2] * 60.0) as u8,
            })
        })
        .collect()
}

fn bounded_sample<T: Copy>(values: &[T], count: usize) -> Vec<T> {
    let count = count.min(values.len());
    (0..count)
        .map(|index| values[index * values.len() / count])
        .collect()
}

fn sampled_map(slam: &SlamProcessor, capacity: usize) -> Vec<[f32; 3]> {
    let keyframes = slam.keyframes();
    let total = keyframes
        .iter()
        .map(|keyframe| keyframe.scan.points.len())
        .sum::<usize>();
    if total == 0 {
        return Vec::new();
    }
    let stride = total.div_ceil(capacity).max(1);
    let mut ordinal = 0_usize;
    let mut output = Vec::with_capacity(capacity.min(total));
    for keyframe in keyframes {
        let pose = keyframe.pose;
        let (sin, cos) = pose.rotation().sin_cos();
        let translation = pose.translation();
        for point in &keyframe.scan.points {
            if ordinal.is_multiple_of(stride) && output.len() < capacity {
                output.push([
                    (translation.x + cos * f64::from(point.x) - sin * f64::from(point.y)) as f32,
                    (translation.y + sin * f64::from(point.x) + cos * f64::from(point.y)) as f32,
                    point.z,
                ]);
            }
            ordinal += 1;
        }
    }
    output
}

fn push_bounded<T>(queue: &mut VecDeque<T>, value: T, capacity: usize) {
    if queue.len() == capacity {
        queue.pop_front();
    }
    queue.push_back(value);
}

fn normalize_angle(angle: f64) -> f64 {
    (angle + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI
}
fn semantic(name: &str) -> Result<SemanticType, String> {
    SemanticType::new(name).map_err(debug)
}
fn debug(error: impl std::fmt::Debug) -> String {
    format!("{error:?}")
}

pub fn episode_summary(snapshot: &SlamSnapshot, frames: usize) -> String {
    format!(
        "frames={frames} updates={} keyframes={} loops={} accepted={} dropped={} translation_error={:.4} rotation_error={:.4} scan_capacity={} map_capacity={}",
        snapshot.update_count,
        snapshot.keyframe_count,
        snapshot.loop_count,
        snapshot.accepted_points,
        snapshot.dropped_points,
        snapshot.translation_error,
        snapshot.rotation_error,
        snapshot.scan_capacity,
        snapshot.map_capacity
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prepared() -> PreparedLidarSlam {
        build_from_source(include_str!("../lidar-slam.yaml")).unwrap()
    }

    #[test]
    fn deterministic_inputs_and_100_frame_progression_are_bounded_and_stateful() {
        assert_eq!(synthetic_episode(120), synthetic_episode(120));
        let mut first = prepared();
        let (a, reports) = run_episode(&mut first, 120, true).unwrap();
        assert_eq!(reports.len(), 120);
        assert!(a.last().unwrap().update_count > 80);
        assert!(a.last().unwrap().keyframe_count > 1);
        assert!(a.iter().any(|snapshot| !snapshot.update_event));
        for snapshot in &a {
            assert!(snapshot.current_scan.len() <= MAX_SCAN_POINTS);
            assert!(snapshot.estimated_trail.len() <= MAX_TRAIL_POSES);
            assert!(snapshot.map_points.len() <= MAX_MAP_POINTS);
            assert!(snapshot.keyframe_poses.len() <= MAX_KEYFRAME_POSES);
            assert!(snapshot.edges.len() <= MAX_EDGES);
        }
    }

    #[test]
    fn default_episode_detects_and_optimizes_a_real_loop_closure() {
        let mut prepared = prepared();
        let (snapshots, _) = run_episode(&mut prepared, DEFAULT_FRAMES, false).unwrap();
        let closure = snapshots
            .iter()
            .find(|snapshot| snapshot.loop_event)
            .expect("default episode must exercise Slamwich loop closure");
        assert_eq!(closure.loop_count, 1);
        assert!(closure.edges.iter().any(|edge| edge.loop_closure));
        let final_snapshot = snapshots.last().unwrap();
        assert!(final_snapshot.loop_count >= 3);
        assert!(final_snapshot.translation_error < 0.2);
        assert!(
            final_snapshot
                .keyframe_poses
                .iter()
                .all(|pose| pose.x.abs() < ROOM_HALF_X && pose.y.abs() < ROOM_HALF_Y)
        );
        assert!(
            final_snapshot
                .keyframe_poses
                .windows(2)
                .all(|pair| (pair[1].x - pair[0].x).hypot(pair[1].y - pair[0].y) < 1.5)
        );
        let (min_x, max_x) = final_snapshot
            .keyframe_poses
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(min, max), pose| {
                (min.min(pose.x), max.max(pose.x))
            });
        let (min_y, max_y) = final_snapshot
            .keyframe_poses
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(min, max), pose| {
                (min.min(pose.y), max.max(pose.y))
            });
        assert!(max_x - min_x > 16.0 && max_y - min_y > 11.0);
    }

    #[test]
    fn stationary_frame_publishes_truthful_no_update_status() {
        let mut prepared = prepared();
        let frames = synthetic_episode(100).unwrap();
        let first = prepared.run(&frames[0]).unwrap();
        assert_eq!(first.status, FrameStatus::Updated);

        let mut stationary = frames[0].clone();
        stationary.frame_index = 1;
        stationary.timestamp_ns += 100_000_000;
        let snapshot = prepared.run(&stationary).unwrap();
        assert_eq!(snapshot.status, FrameStatus::NoUpdate);
        assert!(!snapshot.update_event);
        assert_eq!(snapshot.update_count, first.update_count);
    }

    #[test]
    fn retained_edges_use_local_keyframe_window_indices() {
        let source = include_str!("../lidar-slam.yaml")
            .replace("max_keyframe_poses: 160", "max_keyframe_poses: 8");
        let mut prepared = build_from_source(&source).unwrap();
        let (snapshots, _) = run_episode(&mut prepared, 120, false).unwrap();
        let snapshot = snapshots.last().unwrap();
        assert_eq!(snapshot.keyframe_poses.len(), 8);
        assert!(snapshot.keyframe_count > snapshot.keyframe_poses.len());
        assert!(
            snapshot
                .edges
                .iter()
                .all(|edge| edge.from < snapshot.keyframe_poses.len()
                    && edge.to < snapshot.keyframe_poses.len())
        );
    }

    #[test]
    fn reference_is_not_used_as_odometry() {
        let episode = synthetic_episode(100).unwrap();
        assert!(
            episode
                .iter()
                .skip(1)
                .any(|frame| frame.odometry != frame.reference)
        );
        let mut changed = episode[0].clone();
        changed.reference.x += 100.0;
        let mut a = prepared();
        let mut b = prepared();
        let left = a.run(&episode[0]).unwrap();
        let right = b.run(&changed).unwrap();
        assert_eq!(left.estimated, right.estimated);
        assert_ne!(left.translation_error, right.translation_error);
    }

    #[test]
    fn rejected_frames_do_not_publish_or_replace_snapshot() {
        let frames = synthetic_episode(100).unwrap();
        let mut subject = prepared();
        let baseline = subject.run(&frames[0]).unwrap();

        let mut skipped_index = frames[1].clone();
        skipped_index.frame_index = 2;
        let mut regression = frames[1].clone();
        regression.timestamp_ns = frames[0].timestamp_ns;
        let mut non_finite_pose = frames[1].clone();
        non_finite_pose.odometry.theta = f64::NAN;
        let mut out_of_bounds_pose = frames[1].clone();
        out_of_bounds_pose.reference.x = MAX_ABS_POSE_TRANSLATION + 1.0;
        let mut non_finite_point = frames[1].clone();
        non_finite_point.scan[0].xyz[0] = f32::NAN;
        let mut out_of_bounds_point = frames[1].clone();
        out_of_bounds_point.scan[0].xyz[1] = MAX_ABS_SCAN_COORDINATE + 1.0;
        let mut oversized = frames[1].clone();
        oversized
            .scan
            .resize(MAX_INPUT_POINTS + 1, oversized.scan[0]);
        let mut empty = frames[1].clone();
        empty.scan.clear();

        for bad in [
            skipped_index,
            regression,
            non_finite_pose,
            out_of_bounds_pose,
            non_finite_point,
            out_of_bounds_point,
            oversized,
            empty,
        ] {
            assert!(subject.run(&bad).is_err());
            assert_eq!(subject.last_snapshot(), Some(&baseline));
        }

        let resumed = subject.run(&frames[1]).unwrap();
        assert_eq!(resumed.frame_index, 1);
        assert_eq!(resumed.timestamp_ns, frames[1].timestamp_ns);
        assert_eq!(resumed.update_count, 2);
        assert_eq!(resumed.estimated_trail.len(), 2);
        assert_eq!(
            resumed.odometry_trail,
            frames[..=1]
                .iter()
                .map(|frame| frame.odometry)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            resumed.reference_trail,
            frames[..=1]
                .iter()
                .map(|frame| frame.reference)
                .collect::<Vec<_>>()
        );
        assert_eq!(subject.last_snapshot(), Some(&resumed));
    }

    #[test]
    fn exhausted_frame_index_is_rejected_without_overflow() {
        let frames = synthetic_episode(100).unwrap();
        let mut subject = prepared();
        let mut terminal = frames[0].clone();
        terminal.frame_index = usize::MAX;
        let baseline = subject.run(&terminal).unwrap();

        let mut wrapped = frames[1].clone();
        wrapped.frame_index = 0;
        assert!(subject.run(&wrapped).is_err());
        assert_eq!(subject.last_snapshot(), Some(&baseline));
    }

    #[test]
    fn yaml_and_graph_contract_are_truthful() {
        let prepared = prepared();
        let frame = prepared
            .description
            .graph
            .resources
            .iter()
            .find(|resource| resource.id.as_str() == "lidar_frame")
            .unwrap();
        assert_eq!(
            frame
                .consumers
                .iter()
                .map(|consumer| consumer.unit.as_str())
                .collect::<Vec<_>>(),
            ["scan_prepare", "snapshot"]
        );
        for graph in [
            prepared.description.to_text(),
            prepared.description.to_dot(),
            prepared.description.to_mermaid(),
        ] {
            assert!(graph.contains("scan_prepare"));
            assert!(graph.contains("slam"));
            assert!(graph.contains("snapshot"));
            assert!(graph.contains("lidar_frame"));
            assert!(graph.contains("slam_snapshot"));
        }
        let bad = include_str!("../lidar-slam.yaml")
            .replace("max_scan_points: 384", "max_scan_points: 999");
        assert!(build_from_source(&bad).is_err());

        let mismatched = include_str!("../lidar-slam.yaml").replacen(
            "max_scan_points: 384",
            "max_scan_points: 320",
            1,
        );
        let mismatch_error = match build_from_source(&mismatched) {
            Ok(_) => panic!("mismatched stage configuration should be rejected"),
            Err(error) => error,
        };
        assert!(mismatch_error.contains("stage configurations must match"));

        let rebound_output = include_str!("../lidar-slam.yaml")
            .replace(
                "outputs: { snapshot: slam_snapshot }",
                "outputs: { snapshot: actual_snapshot }",
            )
            .replace("prepared_frame", "slam_snapshot");
        let binding_error = match build_from_source(&rebound_output) {
            Ok(_) => panic!("rebound module output should be rejected"),
            Err(error) => error,
        };
        assert!(binding_error.contains("fixed scan_prepare -> slam -> snapshot bindings"));
    }

    #[test]
    fn timed_graph_and_summary_cover_episode() {
        let mut prepared = prepared();
        let (snapshots, reports) = run_episode(&mut prepared, 100, true).unwrap();
        let timed = prepared.description.to_mermaid_with_runs(&reports);
        assert!(timed.contains("n=100") && timed.contains("avg ") && timed.contains("p99 "));
        let summary = episode_summary(snapshots.last().unwrap(), snapshots.len());
        assert!(summary.contains("frames=100") && summary.contains("translation_error="));
    }
}
