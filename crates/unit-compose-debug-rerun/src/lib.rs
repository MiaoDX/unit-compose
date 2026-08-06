//! Rerun-backed post-run inspection adapter.
//!
//! The adapter is deliberately allocating and runs only after measured Module
//! execution. It writes native Rerun archetypes and a fixed blueprint either to
//! an `.rrd` file or to a spawned viewer.

use std::fmt;
use std::path::Path;

use re_sdk::external::re_log_types::{BlueprintActivationCommand, StoreId, StoreKind};
use re_sdk::{RecordingStream, RecordingStreamBuilder};
use re_types::archetypes::{Image, LineStrips2D, Points2D, Scalars, SeriesLines};
use re_types::blueprint::archetypes::{
    ContainerBlueprint, PanelBlueprint, ViewBlueprint, ViewContents, ViewportBlueprint,
};
use re_types::blueprint::components::{ContainerKind, PanelState};
use re_types::components::{Color, Radius};
use unit_compose_core::{FixedModuleDescription, RunReportSnapshot};
use unit_compose_debug::{AdapterDescriptor, AdapterExecution, InspectionAdapter};

const APP_ID: &str = "unit-compose-navigation";
const BLUEPRINT_ID: &str = "unit-compose-navigation-blueprint-v1";

pub const NAVIGATION_ENTITY_PATHS: [&str; 5] = [
    "navigation/map_with_binary_clearance",
    "navigation/raw_path",
    "navigation/smoothed_path",
    "navigation/start",
    "navigation/goal",
];

#[derive(Debug)]
pub struct RerunAdapterError(String);

impl fmt::Display for RerunAdapterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for RerunAdapterError {}

/// Borrowed navigation values captured after Module execution.
///
/// Grids are row-major in ROS map coordinates, and points use the same Y-up
/// cell coordinate frame. The adapter owns conversion to display coordinates.
#[derive(Clone, Copy, Debug)]
pub struct NavigationFrame<'a> {
    pub width: usize,
    pub height: usize,
    pub occupancy_grid: &'a [i8],
    pub cost_map: &'a [u8],
    pub raw_path: &'a [[f32; 2]],
    pub smoothed_path: Option<&'a [[f32; 2]]>,
    pub start: [f32; 2],
    pub goal: [f32; 2],
}

impl NavigationFrame<'_> {
    fn validate(self) -> Result<Self, RerunAdapterError> {
        let cells = self
            .width
            .checked_mul(self.height)
            .ok_or_else(|| error("navigation image dimensions overflow"))?;
        if cells == 0 || self.occupancy_grid.len() != cells || self.cost_map.len() != cells {
            return Err(error(
                "navigation images must be nonempty and match width * height",
            ));
        }
        if [self.start, self.goal]
            .into_iter()
            .any(|[x, y]| x < 0.0 || y < 0.0 || x >= self.width as f32 || y >= self.height as f32)
        {
            return Err(error("start and goal must be inside the navigation map"));
        }
        Ok(self)
    }
}

/// Post-run Rerun adapter. Construction and all logging may allocate.
pub struct RerunAdapter {
    recording: RecordingStream,
    run_index: i64,
    unit_ids: Vec<String>,
}

impl RerunAdapter {
    /// Creates a file route that never opens a viewer.
    pub fn save(path: impl AsRef<Path>) -> Result<Self, RerunAdapterError> {
        let recording = RecordingStreamBuilder::new(APP_ID)
            .save(path.as_ref())
            .map_err(display_error)?;
        Self::from_recording(recording)
    }

    /// Spawns an external Rerun viewer and streams to it over gRPC.
    ///
    /// This requires a compatible `rerun` executable on `PATH`.
    pub fn spawn() -> Result<Self, RerunAdapterError> {
        let recording = RecordingStreamBuilder::new(APP_ID)
            .spawn()
            .map_err(display_error)?;
        Self::from_recording(recording)
    }

    fn from_recording(recording: RecordingStream) -> Result<Self, RerunAdapterError> {
        send_fixed_blueprint(&recording)?;
        Ok(Self {
            recording,
            run_index: 0,
            unit_ids: Vec::new(),
        })
    }

    /// Records actual map and path values after the measured navigation run.
    pub fn navigation_frame(
        &mut self,
        frame: NavigationFrame<'_>,
    ) -> Result<(), RerunAdapterError> {
        let frame = frame.validate()?;
        self.recording.set_time_sequence("run", self.run_index);

        let dimensions = [
            u32::try_from(frame.width).map_err(|_| error("navigation width exceeds u32"))?,
            u32::try_from(frame.height).map_err(|_| error("navigation height exceeds u32"))?,
        ];
        let map_pixels = navigation_pixels(frame.occupancy_grid, frame.cost_map, frame.width);
        self.recording
            .log(
                NAVIGATION_ENTITY_PATHS[0],
                &Image::from_rgb24(map_pixels, dimensions).with_draw_order(-10.0),
            )
            .map_err(display_error)?;

        log_path(
            &self.recording,
            NAVIGATION_ENTITY_PATHS[1],
            frame.raw_path,
            frame.height,
            Color::from_rgb(65, 105, 225),
        )?;
        if let Some(smoothed_path) = frame.smoothed_path {
            log_path(
                &self.recording,
                NAVIGATION_ENTITY_PATHS[2],
                smoothed_path,
                frame.height,
                Color::from_rgb(34, 139, 34),
            )?;
        }
        log_endpoint(
            &self.recording,
            NAVIGATION_ENTITY_PATHS[3],
            display_point(frame.start, frame.height),
            Color::from_rgb(0, 128, 0),
            "Start",
        )?;
        log_endpoint(
            &self.recording,
            NAVIGATION_ENTITY_PATHS[4],
            display_point(frame.goal, frame.height),
            Color::from_rgb(220, 20, 60),
            "Goal",
        )?;
        Ok(())
    }

    /// Flushes all pending recording batches.
    pub fn flush(&self) {
        self.recording.flush_blocking();
    }
}

impl InspectionAdapter for RerunAdapter {
    type Error = RerunAdapterError;

    fn descriptor(&self) -> AdapterDescriptor {
        AdapterDescriptor {
            name: "rerun-0.24.1",
            execution: AdapterExecution::PostRunAllocating,
            allocation_domains: &["rust-global", "rerun-sdk", "file-or-grpc-io"],
            overhead: "image/path conversion, Arrow serialization, batching, and file or gRPC I/O after the measured run",
        }
    }

    fn fixed_description(
        &mut self,
        description: &FixedModuleDescription,
    ) -> Result<(), Self::Error> {
        self.unit_ids = description
            .graph
            .execution_order
            .iter()
            .map(|unit| unit.as_str().to_owned())
            .collect();
        for (ordinal, unit) in self.unit_ids.iter().enumerate() {
            self.recording
                .log_static(
                    format!("timings/units/{ordinal:02}"),
                    &SeriesLines::new()
                        .with_names([unit.as_str()])
                        .with_colors([timing_color(ordinal)]),
                )
                .map_err(display_error)?;
        }
        log_capacity_plan(&self.recording, description)?;
        Ok(())
    }

    fn run_snapshot(&mut self, report: &RunReportSnapshot) -> Result<(), Self::Error> {
        self.recording.set_time_sequence("run", self.run_index);
        let elapsed_ms = report
            .events()
            .map(|event| event.elapsed.as_secs_f64() * 1_000.0)
            .sum::<f64>();
        self.recording
            .log("metrics/run/elapsed_ms", &Scalars::new([elapsed_ms]))
            .map_err(display_error)?;
        for event in report.unit_timings() {
            let unit = self
                .unit_ids
                .get(event.unit_ordinal)
                .ok_or_else(|| error("Unit timing ordinal is outside the fixed execution order"))?;
            self.recording
                .log(
                    format!("timings/units/{:02}", event.unit_ordinal),
                    &Scalars::new([event.elapsed.as_secs_f64() * 1_000.0]),
                )
                .map_err(display_error)?;
            self.recording
                .log(
                    format!("timings/start_offset_ms/{unit}"),
                    &Scalars::new([event.started_after_module_start.as_secs_f64() * 1_000.0]),
                )
                .map_err(display_error)?;
        }
        self.recording
            .log(
                "metrics/run/observed_capacity",
                &Scalars::new([report.observed_capacity_peak() as f64]),
            )
            .map_err(display_error)?;
        let operations = report.allocation_operations();
        for (entity, value) in [
            ("allocations", operations.allocations),
            ("reallocations", operations.reallocations),
            ("deallocations", operations.deallocations),
        ] {
            self.recording
                .log(
                    format!("metrics/run/{entity}"),
                    &Scalars::new([value as f64]),
                )
                .map_err(display_error)?;
        }
        self.run_index = self.run_index.saturating_add(1);
        Ok(())
    }
}

fn timing_color(ordinal: usize) -> Color {
    const COLORS: [[u8; 3]; 6] = [
        [21, 101, 192],
        [0, 137, 123],
        [239, 108, 0],
        [123, 31, 162],
        [198, 40, 40],
        [84, 110, 122],
    ];
    Color::from_rgb(
        COLORS[ordinal % COLORS.len()][0],
        COLORS[ordinal % COLORS.len()][1],
        COLORS[ordinal % COLORS.len()][2],
    )
}

fn navigation_pixels(occupancy_grid: &[i8], cost_map: &[u8], width: usize) -> Vec<u8> {
    occupancy_grid
        .chunks_exact(width)
        .zip(cost_map.chunks_exact(width))
        .rev()
        .flat_map(|(occupancy_row, cost_row)| occupancy_row.iter().zip(cost_row))
        .flat_map(|(occupancy, cost)| {
            if *occupancy < 0 {
                [160, 166, 172]
            } else if *occupancy >= 50 {
                [24, 28, 32]
            } else if *cost != 0 {
                [245, 158, 11]
            } else {
                [245, 247, 249]
            }
        })
        .collect()
}

fn display_point([x, y]: [f32; 2], height: usize) -> [f32; 2] {
    [x, height as f32 - y]
}

fn display_points(points: &[[f32; 2]], height: usize) -> Vec<[f32; 2]> {
    points
        .iter()
        .copied()
        .map(|point| display_point(point, height))
        .collect()
}

fn log_path(
    recording: &RecordingStream,
    entity_path: &str,
    points: &[[f32; 2]],
    height: usize,
    color: Color,
) -> Result<(), RerunAdapterError> {
    recording
        .log(
            entity_path,
            &LineStrips2D::new([display_points(points, height)])
                .with_colors([color])
                .with_radii([Radius::new_ui_points(2.0)]),
        )
        .map_err(display_error)
}

fn log_endpoint(
    recording: &RecordingStream,
    entity_path: &str,
    point: [f32; 2],
    color: Color,
    label: &str,
) -> Result<(), RerunAdapterError> {
    recording
        .log(
            entity_path,
            &Points2D::new([point])
                .with_colors([color])
                .with_radii([Radius::new_ui_points(6.0)])
                .with_labels([label])
                .with_show_labels(true)
                .with_draw_order(20.0),
        )
        .map_err(display_error)
}

fn log_capacity_plan(
    recording: &RecordingStream,
    description: &FixedModuleDescription,
) -> Result<(), RerunAdapterError> {
    recording
        .log_static(
            "metrics/capacity/storage_slots",
            &Scalars::new([description.storage.slot_count as f64]),
        )
        .map_err(display_error)?;
    recording
        .log_static(
            "metrics/capacity/estimated_peak_bytes",
            &Scalars::new([description.storage.estimated_peak_bytes as f64]),
        )
        .map_err(display_error)?;
    for (resource, requirement) in &description.requirements {
        recording
            .log_static(
                format!("metrics/capacity/resources/{}", resource.as_str()),
                &Scalars::new([requirement.capacity as f64]),
            )
            .map_err(display_error)?;
    }
    Ok(())
}

fn send_fixed_blueprint(recording: &RecordingStream) -> Result<(), RerunAdapterError> {
    let blueprint_id = StoreId::from_string(StoreKind::Blueprint, BLUEPRINT_ID.to_owned());
    let (blueprint, storage) = RecordingStreamBuilder::new(APP_ID)
        .store_id(blueprint_id.clone())
        .blueprint()
        .memory()
        .map_err(display_error)?;

    let navigation_view = "view/22222222-2222-2222-2222-222222222222";
    let timings_view = "view/33333333-3333-3333-3333-333333333333";
    let metrics_view = "view/44444444-4444-4444-4444-444444444444";
    let root_container = "container/11111111-1111-1111-1111-111111111111";
    blueprint
        .log_static(
            navigation_view,
            &ViewBlueprint::new("2D")
                .with_display_name("Navigation")
                .with_space_origin("navigation"),
        )
        .map_err(display_error)?;
    blueprint
        .log_static(
            format!("{navigation_view}/ViewContents"),
            &ViewContents::new(["+ /navigation/**"]),
        )
        .map_err(display_error)?;
    blueprint
        .log_static(
            timings_view,
            &ViewBlueprint::new("TimeSeries")
                .with_display_name("Unit timings")
                .with_space_origin("timings"),
        )
        .map_err(display_error)?;
    blueprint
        .log_static(
            format!("{timings_view}/ViewContents"),
            &ViewContents::new(["+ /timings/units/**"]),
        )
        .map_err(display_error)?;
    blueprint
        .log_static(
            metrics_view,
            &ViewBlueprint::new("TimeSeries")
                .with_display_name("Run metrics")
                .with_space_origin("metrics"),
        )
        .map_err(display_error)?;
    blueprint
        .log_static(
            format!("{metrics_view}/ViewContents"),
            &ViewContents::new(["+ /metrics/**"]),
        )
        .map_err(display_error)?;
    blueprint
        .log_static(
            root_container,
            &ContainerBlueprint::new(ContainerKind::Horizontal)
                .with_display_name("UnitCompose navigation")
                .with_contents([navigation_view, timings_view, metrics_view])
                .with_col_shares([2.2, 1.0, 0.8]),
        )
        .map_err(display_error)?;
    blueprint
        .log_static(
            "viewport",
            &ViewportBlueprint::new()
                .with_root_container([0x11; 16])
                .with_auto_views(false),
        )
        .map_err(display_error)?;
    for panel in ["blueprint_panel", "selection_panel"] {
        blueprint
            .log_static(
                panel,
                &PanelBlueprint::new().with_state(PanelState::Collapsed),
            )
            .map_err(display_error)?;
    }

    let messages = storage.take();
    drop(blueprint);
    recording.send_blueprint(
        messages,
        BlueprintActivationCommand::make_active(blueprint_id),
    );
    Ok(())
}

fn display_error(error: impl fmt::Display) -> RerunAdapterError {
    RerunAdapterError(error.to_string())
}

fn error(message: &str) -> RerunAdapterError {
    RerunAdapterError(message.to_owned())
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use re_sdk::RecordingStreamBuilder;
    use re_sdk::external::re_log_types::LogMsg;
    use unit_compose_debug::{
        AdapterController, AdapterExecution, AdapterFailurePolicy, InspectionAdapter,
    };

    use super::{
        APP_ID, NAVIGATION_ENTITY_PATHS, NavigationFrame, RerunAdapter, display_points,
        navigation_pixels,
    };

    fn recording_path(test: &str) -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "unit-compose-rerun-{test}-{}-{nonce}.rrd",
            std::process::id()
        ))
    }

    #[test]
    fn descriptor_is_explicitly_post_run_allocating_and_rejected_by_strict_controller() {
        let path = recording_path("descriptor");
        let adapter = RerunAdapter::save(&path).unwrap();
        assert_eq!(
            adapter.descriptor().execution,
            AdapterExecution::PostRunAllocating
        );
        assert!(AdapterController::strict(adapter, AdapterFailurePolicy::Report).is_err());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn navigation_frame_requires_matching_grids_and_bounded_endpoints() {
        let occupancy = [0, 100, 0, 0];
        let short_cost = [0, 1, 0];
        let cost = [0, 1, 0, 0];
        let raw = [[0.5, 0.5], [1.5, 0.5], [1.5, 1.5]];
        let smoothed = [[0.5, 0.5], [1.5, 1.5]];

        for frame in [
            NavigationFrame {
                width: 2,
                height: 2,
                occupancy_grid: &occupancy,
                cost_map: &short_cost,
                raw_path: &raw,
                smoothed_path: None,
                start: [0.5, 0.5],
                goal: [1.5, 1.5],
            },
            NavigationFrame {
                width: 2,
                height: 2,
                occupancy_grid: &occupancy,
                cost_map: &cost,
                raw_path: &raw,
                smoothed_path: Some(&smoothed),
                start: [-0.5, 0.5],
                goal: [1.5, 1.5],
            },
        ] {
            assert!(frame.validate().is_err());
        }

        assert!(
            NavigationFrame {
                width: 2,
                height: 2,
                occupancy_grid: &occupancy,
                cost_map: &cost,
                raw_path: &raw,
                smoothed_path: Some(&smoothed),
                start: [0.5, 0.5],
                goal: [1.5, 1.5],
            }
            .validate()
            .is_ok()
        );
    }

    #[test]
    fn visualization_uses_four_ros_map_states_and_y_up_coordinates() {
        let occupancy = [0, 100, -1, 0];
        let cost = [1, 1, 0, 0];
        assert_eq!(
            navigation_pixels(&occupancy, &cost, 2),
            [160, 166, 172, 245, 247, 249, 245, 158, 11, 24, 28, 32]
        );
        assert_eq!(
            display_points(&[[0.5, 0.5], [1.5, 1.5]], 2),
            [[0.5, 1.5], [1.5, 0.5]]
        );
    }

    #[test]
    fn memory_route_contains_navigation_entities_and_active_blueprint() {
        let (recording, storage) = RecordingStreamBuilder::new(APP_ID).memory().unwrap();
        let mut adapter = RerunAdapter::from_recording(recording).unwrap();
        let occupancy = [0, 100, 0, 0];
        let cost = [1, 1, 0, 0];
        let raw = [[0.5, 0.5], [1.5, 0.5], [1.5, 1.5]];
        let smoothed = [[0.5, 0.5], [1.5, 1.5]];
        adapter
            .navigation_frame(NavigationFrame {
                width: 2,
                height: 2,
                occupancy_grid: &occupancy,
                cost_map: &cost,
                raw_path: &raw,
                smoothed_path: Some(&smoothed),
                start: [0.5, 0.5],
                goal: [1.5, 1.5],
            })
            .unwrap();

        let messages = storage.take();
        let entity_paths = messages
            .iter()
            .filter_map(|message| match message {
                LogMsg::ArrowMsg(_, message) => message
                    .batch
                    .schema()
                    .metadata()
                    .get("rerun:entity_path")
                    .cloned(),
                _ => None,
            })
            .collect::<Vec<_>>();
        for required in NAVIGATION_ENTITY_PATHS {
            assert!(
                entity_paths
                    .iter()
                    .any(|path| path.trim_start_matches('/') == required),
                "missing entity {required}"
            );
        }
        for view in [
            "view/22222222-2222-2222-2222-222222222222/ViewContents",
            "view/33333333-3333-3333-3333-333333333333/ViewContents",
            "view/44444444-4444-4444-4444-444444444444/ViewContents",
        ] {
            assert!(
                messages.iter().any(|message| match message {
                    LogMsg::ArrowMsg(_, message) => {
                        let schema = message.batch.schema();
                        schema
                            .metadata()
                            .get("rerun:entity_path")
                            .is_some_and(|path| path.trim_start_matches('/') == view)
                            && schema
                                .fields()
                                .iter()
                                .any(|field| field.name() == "ViewContents:query")
                    }
                    _ => false,
                }),
                "missing contents query for view {view}"
            );
        }
        assert!(
            messages
                .iter()
                .any(|message| matches!(message, LogMsg::BlueprintActivationCommand(_)))
        );
    }

    #[test]
    fn file_route_writes_a_nonempty_recording() {
        let path = recording_path("frame");
        let occupancy = [0, 100, 0, 0];
        let cost = [1, 1, 0, 0];
        let raw = [[0.5, 0.5], [1.5, 0.5], [1.5, 1.5]];
        let smoothed = [[0.5, 0.5], [1.5, 1.5]];
        let mut adapter = RerunAdapter::save(&path).unwrap();
        adapter
            .navigation_frame(NavigationFrame {
                width: 2,
                height: 2,
                occupancy_grid: &occupancy,
                cost_map: &cost,
                raw_path: &raw,
                smoothed_path: Some(&smoothed),
                start: [0.5, 0.5],
                goal: [1.5, 1.5],
            })
            .unwrap();
        adapter.flush();
        drop(adapter);
        assert!(std::fs::metadata(&path).unwrap().len() > 0);
        std::fs::remove_file(path).unwrap();
    }
}
