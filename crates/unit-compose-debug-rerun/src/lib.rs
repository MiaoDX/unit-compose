//! Rerun-backed post-run inspection adapter.
//!
//! The adapter is deliberately allocating and runs only after measured Module
//! execution. It writes native Rerun archetypes and a fixed blueprint either to
//! an `.rrd` file or to a spawned viewer.

use std::fmt;
use std::path::Path;

use re_sdk::external::re_log_types::{BlueprintActivationCommand, StoreId, StoreKind};
use re_sdk::{AsComponents, RecordingStream, RecordingStreamBuilder};
use re_types::archetypes::{GraphEdges, GraphNodes, Image, LineStrips2D, Scalars};
use re_types::blueprint::archetypes::{
    ContainerBlueprint, PanelBlueprint, ViewBlueprint, ViewportBlueprint,
};
use re_types::blueprint::components::{ContainerKind, PanelState};
use re_types::components::{Color, Radius};
use unit_compose_core::{FixedModuleDescription, Producer, RunReportSnapshot};
use unit_compose_debug::{AdapterDescriptor, AdapterExecution, InspectionAdapter};

const APP_ID: &str = "unit-compose-navigation";
const BLUEPRINT_ID: &str = "unit-compose-navigation-blueprint-v1";

pub const NAVIGATION_ENTITY_PATHS: [&str; 5] = [
    "navigation/map",
    "navigation/cost_map",
    "navigation/raw_path",
    "navigation/smoothed_path",
    "navigation/final_path",
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
#[derive(Clone, Copy, Debug)]
pub struct NavigationFrame<'a> {
    pub width: usize,
    pub height: usize,
    pub binary_map: &'a [u8],
    pub cost_map: &'a [u8],
    pub raw_path: &'a [[f32; 2]],
    pub smoothed_path: Option<&'a [[f32; 2]]>,
    pub final_path: &'a [[f32; 2]],
}

impl NavigationFrame<'_> {
    fn validate(self) -> Result<Self, RerunAdapterError> {
        let cells = self
            .width
            .checked_mul(self.height)
            .ok_or_else(|| error("navigation image dimensions overflow"))?;
        if cells == 0 || self.binary_map.len() != cells || self.cost_map.len() != cells {
            return Err(error(
                "navigation images must be nonempty and match width * height",
            ));
        }
        if self.final_path != self.raw_path && self.smoothed_path.is_none() {
            return Err(error(
                "a final path distinct from the raw path requires a smoothed path",
            ));
        }
        Ok(self)
    }
}

/// Post-run Rerun adapter. Construction and all logging may allocate.
pub struct RerunAdapter {
    recording: RecordingStream,
    run_index: i64,
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
        let binary_pixels = image_pixels(frame.binary_map);
        let cost_pixels = image_pixels(frame.cost_map);
        self.recording
            .log(
                NAVIGATION_ENTITY_PATHS[0],
                &Image::from_l8(binary_pixels, dimensions).with_draw_order(-10.0),
            )
            .map_err(display_error)?;
        self.recording
            .log(
                NAVIGATION_ENTITY_PATHS[1],
                &Image::from_l8(cost_pixels, dimensions)
                    .with_opacity(0.55)
                    .with_draw_order(-9.0),
            )
            .map_err(display_error)?;

        log_path(
            &self.recording,
            NAVIGATION_ENTITY_PATHS[2],
            frame.raw_path,
            Color::from_rgb(65, 105, 225),
        )?;
        if let Some(smoothed_path) = frame.smoothed_path {
            log_path(
                &self.recording,
                NAVIGATION_ENTITY_PATHS[3],
                smoothed_path,
                Color::from_rgb(34, 139, 34),
            )?;
        }
        log_path(
            &self.recording,
            NAVIGATION_ENTITY_PATHS[4],
            frame.final_path,
            Color::from_rgb(220, 20, 60),
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
        log_graph(&self.recording, description)?;
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

fn image_pixels(values: &[u8]) -> Vec<u8> {
    values
        .iter()
        .map(|value| if *value == 0 { 0 } else { 255 })
        .collect()
}

fn log_path(
    recording: &RecordingStream,
    entity_path: &str,
    points: &[[f32; 2]],
    color: Color,
) -> Result<(), RerunAdapterError> {
    recording
        .log(
            entity_path,
            &LineStrips2D::new([points.to_vec()])
                .with_colors([color])
                .with_radii([Radius::new_ui_points(2.0)]),
        )
        .map_err(display_error)
}

fn log_graph(
    recording: &RecordingStream,
    description: &FixedModuleDescription,
) -> Result<(), RerunAdapterError> {
    let graph = &description.graph;
    let mut node_ids = Vec::with_capacity(graph.units.len() + graph.resources.len());
    let mut labels = Vec::with_capacity(node_ids.capacity());
    let mut colors = Vec::with_capacity(node_ids.capacity());

    for unit in &graph.units {
        node_ids.push(format!("unit:{}", unit.id.as_str()));
        labels.push(format!("{}\n{}", unit.id.as_str(), unit.unit_type.as_str()));
        colors.push(Color::from_rgb(84, 110, 122));
    }
    for resource in &graph.resources {
        node_ids.push(format!("resource:{}", resource.id.as_str()));
        labels.push(resource.id.as_str().to_owned());
        let is_output = graph.module_outputs.contains(&resource.id);
        colors.push(if is_output {
            Color::from_rgb(46, 125, 50)
        } else if resource.producer == Producer::ModuleInput {
            Color::from_rgb(21, 101, 192)
        } else {
            Color::from_rgb(117, 117, 117)
        });
    }

    let mut edges = Vec::<(String, String)>::new();
    for resource in &graph.resources {
        let resource_id = format!("resource:{}", resource.id.as_str());
        if let Producer::Unit { unit, .. } = &resource.producer {
            edges.push((format!("unit:{}", unit.as_str()), resource_id.clone()));
        }
        for consumer in &resource.consumers {
            edges.push((
                resource_id.clone(),
                format!("unit:{}", consumer.unit.as_str()),
            ));
        }
    }

    let nodes = GraphNodes::new(node_ids)
        .with_labels(labels)
        .with_colors(colors)
        .with_show_labels(true);
    let edges = GraphEdges::new(edges).with_directed_edges();
    recording
        .log_static(
            "module/graph",
            &[&nodes as &dyn AsComponents, &edges as &dyn AsComponents],
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
    let graph_view = "view/33333333-3333-3333-3333-333333333333";
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
            graph_view,
            &ViewBlueprint::new("Graph")
                .with_display_name("Module graph")
                .with_space_origin("module"),
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
            root_container,
            &ContainerBlueprint::new(ContainerKind::Horizontal)
                .with_display_name("UnitCompose navigation")
                .with_contents([navigation_view, graph_view, metrics_view])
                .with_col_shares([2.0, 1.0, 1.0]),
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

    use unit_compose_debug::{
        AdapterController, AdapterExecution, AdapterFailurePolicy, InspectionAdapter,
    };

    use super::{NAVIGATION_ENTITY_PATHS, NavigationFrame, RerunAdapter};

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
    fn file_route_records_navigation_entities_and_blueprint() {
        let path = recording_path("frame");
        let binary = [0, 1, 0, 0];
        let cost = [1, 1, 0, 0];
        let raw = [[0.5, 0.5], [1.5, 0.5], [1.5, 1.5]];
        let smoothed = [[0.5, 0.5], [1.5, 1.5]];
        let mut adapter = RerunAdapter::save(&path).unwrap();
        adapter
            .navigation_frame(NavigationFrame {
                width: 2,
                height: 2,
                binary_map: &binary,
                cost_map: &cost,
                raw_path: &raw,
                smoothed_path: Some(&smoothed),
                final_path: &smoothed,
            })
            .unwrap();
        adapter.flush();
        drop(adapter);

        assert_eq!(NAVIGATION_ENTITY_PATHS[0], "navigation/map");
        assert_eq!(NAVIGATION_ENTITY_PATHS[1], "navigation/cost_map");
        assert!(std::fs::metadata(&path).unwrap().len() > 0);
        std::fs::remove_file(path).unwrap();
    }
}
