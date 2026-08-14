use std::env;
use std::path::PathBuf;

use lidar_slam::{DEFAULT_FRAMES, build_from_path, episode_summary, run_episode};

enum Mode {
    Inspect(&'static str),
    TimedMermaid,
    Run,
    #[cfg(feature = "rerun")]
    RerunSave(PathBuf),
    #[cfg(feature = "rerun")]
    RerunSpawn,
}

fn main() -> Result<(), String> {
    let mut args = env::args().skip(1);
    if args.next().as_deref() != Some("--module") {
        return Err(usage());
    }
    let module_path = PathBuf::from(
        args.next()
            .ok_or_else(|| "--module requires a path".to_owned())?,
    );
    let mode = match args.next().as_deref() {
        Some("--inspect") => match args.next().as_deref() {
            Some("text") => Mode::Inspect("text"),
            Some("dot") => Mode::Inspect("dot"),
            Some("mermaid") => Mode::Inspect("mermaid"),
            _ => return Err("--inspect requires text, dot, or mermaid".to_owned()),
        },
        Some("--timed-mermaid") => Mode::TimedMermaid,
        Some("--run") => Mode::Run,
        Some("--rerun-save") => {
            #[cfg(feature = "rerun")]
            {
                Mode::RerunSave(PathBuf::from(
                    args.next()
                        .ok_or_else(|| "--rerun-save requires an .rrd path".to_owned())?,
                ))
            }
            #[cfg(not(feature = "rerun"))]
            {
                return Err(rerun_disabled());
            }
        }
        Some("--rerun-spawn") => {
            #[cfg(feature = "rerun")]
            {
                Mode::RerunSpawn
            }
            #[cfg(not(feature = "rerun"))]
            {
                return Err(rerun_disabled());
            }
        }
        _ => return Err(usage()),
    };
    if args.next().is_some() {
        return Err("unexpected trailing argument".to_owned());
    }
    let mut prepared = build_from_path(&module_path)?;
    if let Mode::Inspect(view) = mode {
        print!(
            "{}",
            match view {
                "text" => prepared.description.to_text(),
                "dot" => prepared.description.to_dot(),
                "mermaid" => prepared.description.to_mermaid(),
                _ => unreachable!(),
            }
        );
        return Ok(());
    }
    match mode {
        Mode::Run => {
            let (snapshots, _) = run_episode(&mut prepared, DEFAULT_FRAMES, false)?;
            println!(
                "{}",
                episode_summary(
                    snapshots
                        .last()
                        .ok_or_else(|| "episode produced no snapshots".to_owned())?,
                    snapshots.len()
                )
            );
        }
        Mode::TimedMermaid => {
            let (_, reports) = run_episode(&mut prepared, DEFAULT_FRAMES, true)?;
            print!("{}", prepared.description.to_mermaid_with_runs(&reports));
        }
        #[cfg(feature = "rerun")]
        Mode::RerunSave(path) => record_episode(&mut prepared, Some(path))?,
        #[cfg(feature = "rerun")]
        Mode::RerunSpawn => record_episode(&mut prepared, None)?,
        Mode::Inspect(_) => unreachable!(),
    }
    Ok(())
}

#[cfg(feature = "rerun")]
fn record_episode(
    prepared: &mut lidar_slam::PreparedLidarSlam,
    path: Option<PathBuf>,
) -> Result<(), String> {
    use re_sdk::RecordingStreamBuilder;
    use re_types::archetypes::{Arrows3D, LineStrips3D, Points3D, Scalars, SeriesLines};
    use re_types::components::Color;

    let recording = match &path {
        Some(path) => RecordingStreamBuilder::new("unit-compose-lidar-slam").save(path),
        None => RecordingStreamBuilder::new("unit-compose-lidar-slam").spawn(),
    }
    .map_err(|error| error.to_string())?;
    send_blueprint(&recording)?;
    for (ordinal, name, color) in [
        (0, "scan_prepare", Color::from_rgb(35, 105, 180)),
        (1, "slam", Color::from_rgb(220, 125, 25)),
        (2, "snapshot", Color::from_rgb(35, 150, 90)),
    ] {
        recording
            .log_static(
                format!("timings/units/{ordinal}"),
                &SeriesLines::new().with_names([name]).with_colors([color]),
            )
            .map_err(string_error)?;
    }
    let episode = lidar_slam::synthetic_episode(DEFAULT_FRAMES)?;
    for frame in episode {
        let snapshot = prepared
            .run_profiled(&frame)
            .map_err(|error| format!("frame {} failed: {error:?}", frame.frame_index))?;
        recording.set_time_sequence("episode_frame", snapshot.frame_index as i64);
        let transform = |point: lidar_slam::ScanPoint| {
            let (sin, cos) = snapshot.estimated.theta.sin_cos();
            [
                (snapshot.estimated.x + cos * f64::from(point.xyz[0])
                    - sin * f64::from(point.xyz[1])) as f32,
                (snapshot.estimated.y
                    + sin * f64::from(point.xyz[0])
                    + cos * f64::from(point.xyz[1])) as f32,
                point.xyz[2],
            ]
        };
        let scan = snapshot
            .current_scan
            .iter()
            .copied()
            .map(transform)
            .collect::<Vec<_>>();
        let scan_colors = snapshot
            .current_scan
            .iter()
            .map(|point| {
                let height = ((point.xyz[2] + 1.5) / 3.0).clamp(0.0, 1.0);
                Color::from_rgb(
                    (40.0 + 190.0 * height) as u8,
                    (170.0 - 80.0 * height) as u8,
                    (230.0 - 170.0 * height) as u8,
                )
            })
            .collect::<Vec<_>>();
        recording
            .log(
                "slam/current_scan_estimated",
                &Points3D::new(scan).with_colors(scan_colors),
            )
            .map_err(string_error)?;
        log_trail(
            &recording,
            "slam/trajectory/estimated",
            &snapshot.estimated_trail,
            Color::from_rgb(105, 115, 125),
        )?;
        log_trail(
            &recording,
            "slam/trajectory/odometry",
            &snapshot.odometry_trail,
            Color::from_rgb(230, 135, 25),
        )?;
        log_trail(
            &recording,
            "slam/trajectory/reference",
            &snapshot.reference_trail,
            Color::from_rgb(25, 125, 180),
        )?;
        let keyframes = snapshot
            .keyframe_poses
            .iter()
            .map(|pose| [pose.x as f32, pose.y as f32, 0.0])
            .collect::<Vec<_>>();
        recording
            .log(
                "slam/keyframes",
                &Points3D::new(keyframes).with_colors([Color::from_rgb(170, 50, 165)]),
            )
            .map_err(string_error)?;
        log_trail(
            &recording,
            "slam/trajectory/keyframes_optimized",
            &snapshot.keyframe_poses,
            Color::from_rgb(30, 155, 85),
        )?;
        recording
            .log(
                "slam/map",
                &Points3D::new(snapshot.map_points.clone())
                    .with_colors([Color::from_rgb(115, 125, 135)]),
            )
            .map_err(string_error)?;
        let edge_lines = snapshot
            .edges
            .iter()
            .filter(|edge| !edge.loop_closure)
            .filter_map(|edge| {
                let from = snapshot.keyframe_poses.get(edge.from)?;
                let to = snapshot.keyframe_poses.get(edge.to)?;
                Some(vec![
                    [from.x as f32, from.y as f32, 0.0],
                    [to.x as f32, to.y as f32, 0.0],
                ])
            })
            .collect::<Vec<_>>();
        recording
            .log(
                "slam/edges",
                &LineStrips3D::new(edge_lines)
                    .with_colors([Color::from_rgb(90, 90, 90)])
                    .with_radii([0.015]),
            )
            .map_err(string_error)?;
        let loop_edges = snapshot
            .edges
            .iter()
            .filter(|edge| edge.loop_closure)
            .filter_map(|edge| {
                let from = snapshot.keyframe_poses.get(edge.from)?;
                let to = snapshot.keyframe_poses.get(edge.to)?;
                Some(vec![
                    [from.x as f32, from.y as f32, 0.08],
                    [to.x as f32, to.y as f32, 0.08],
                ])
            })
            .collect::<Vec<_>>();
        recording
            .log(
                "slam/loop_closures",
                &LineStrips3D::new(loop_edges)
                    .with_colors([Color::from_rgb(225, 35, 150)])
                    .with_radii([0.08]),
            )
            .map_err(string_error)?;
        let loop_markers = snapshot
            .edges
            .iter()
            .filter(|edge| edge.loop_closure)
            .filter_map(|edge| {
                let from = snapshot.keyframe_poses.get(edge.from)?;
                let to = snapshot.keyframe_poses.get(edge.to)?;
                Some([
                    [from.x as f32, from.y as f32, 0.12],
                    [to.x as f32, to.y as f32, 0.12],
                ])
            })
            .flatten()
            .collect::<Vec<_>>();
        recording
            .log(
                "slam/loop_closure_markers",
                &Points3D::new(loop_markers)
                    .with_colors([Color::from_rgb(225, 35, 150)])
                    .with_radii([0.22])
                    .with_labels(["Loop closure"])
                    .with_show_labels(true),
            )
            .map_err(string_error)?;
        for (name, pose, color) in [
            (
                "estimated",
                snapshot.estimated,
                Color::from_rgb(25, 105, 190),
            ),
            (
                "reference",
                snapshot.reference,
                Color::from_rgb(30, 155, 85),
            ),
        ] {
            let direction = [pose.theta.cos() as f32, pose.theta.sin() as f32, 0.0];
            recording
                .log(
                    format!("slam/frames/{name}"),
                    &Arrows3D::from_vectors([direction])
                        .with_origins([[pose.x as f32, pose.y as f32, 0.0]])
                        .with_colors([color]),
                )
                .map_err(string_error)?;
        }
        for (name, value) in [
            ("translation_error", snapshot.translation_error),
            ("rotation_error", snapshot.rotation_error),
            ("events/update", f64::from(snapshot.update_event)),
            ("events/keyframe", f64::from(snapshot.keyframe_event)),
            ("events/loop", f64::from(snapshot.loop_event)),
            ("counts/updates", snapshot.update_count as f64),
            ("counts/keyframes", snapshot.keyframe_count as f64),
            ("counts/loops", snapshot.loop_count as f64),
            ("points/accepted", snapshot.accepted_points as f64),
            ("points/dropped", snapshot.dropped_points as f64),
            ("capacity/scan", snapshot.scan_capacity as f64),
            ("capacity/map", snapshot.map_capacity as f64),
        ] {
            recording
                .log(format!("metrics/{name}"), &Scalars::new([value]))
                .map_err(string_error)?;
        }
        for timing in prepared.module.report().unit_timings() {
            recording
                .log(
                    format!("timings/units/{}", timing.unit_ordinal),
                    &Scalars::new([timing.elapsed.as_secs_f64() * 1_000.0]),
                )
                .map_err(string_error)?;
        }
    }
    recording.flush_blocking();
    println!(
        "rerun={} frames={DEFAULT_FRAMES}",
        path.as_ref().map_or_else(
            || "spawn".to_owned(),
            |path| format!("save:{}", path.display())
        )
    );
    Ok(())
}

#[cfg(feature = "rerun")]
fn log_trail(
    recording: &re_sdk::RecordingStream,
    path: &str,
    poses: &[lidar_slam::PlanarPose],
    color: re_types::components::Color,
) -> Result<(), String> {
    use re_types::archetypes::LineStrips3D;
    let points = poses
        .iter()
        .map(|pose| [pose.x as f32, pose.y as f32, 0.0])
        .collect::<Vec<_>>();
    recording
        .log(path, &LineStrips3D::new([points]).with_colors([color]))
        .map_err(string_error)
}

#[cfg(feature = "rerun")]
fn send_blueprint(recording: &re_sdk::RecordingStream) -> Result<(), String> {
    use re_sdk::RecordingStreamBuilder;
    use re_sdk::external::re_log_types::{BlueprintActivationCommand, StoreId, StoreKind};
    use re_types::blueprint::archetypes::{
        ContainerBlueprint, ViewBlueprint, ViewContents, ViewportBlueprint,
    };
    use re_types::blueprint::components::ContainerKind;
    let id = StoreId::from_string(
        StoreKind::Blueprint,
        "unit-compose-lidar-slam-blueprint-v1".to_owned(),
    );
    let (blueprint, storage) = RecordingStreamBuilder::new("unit-compose-lidar-slam")
        .store_id(id.clone())
        .blueprint()
        .memory()
        .map_err(string_error)?;
    let scene = "view/11111111-2222-3333-4444-555555555555";
    let metrics = "view/22222222-3333-4444-5555-666666666666";
    let timings = "view/33333333-4444-5555-6666-777777777777";
    const ROOT_CONTAINER_ID: [u8; 16] = [
        0x44, 0x44, 0x44, 0x44, 0x55, 0x55, 0x66, 0x66, 0x77, 0x77, 0x88, 0x88, 0x88, 0x88, 0x88,
        0x88,
    ];
    let root = "container/44444444-5555-6666-7777-888888888888";
    blueprint
        .log_static(
            scene,
            &ViewBlueprint::new("3D")
                .with_display_name("Synthetic room SLAM")
                .with_space_origin("slam"),
        )
        .map_err(string_error)?;
    blueprint
        .log_static(
            format!("{scene}/ViewContents"),
            &ViewContents::new(["+ /slam/**"]),
        )
        .map_err(string_error)?;
    blueprint
        .log_static(
            metrics,
            &ViewBlueprint::new("TimeSeries")
                .with_display_name("Errors, events, and capacity")
                .with_space_origin("metrics"),
        )
        .map_err(string_error)?;
    blueprint
        .log_static(
            format!("{metrics}/ViewContents"),
            &ViewContents::new(["+ /metrics/**"]),
        )
        .map_err(string_error)?;
    blueprint
        .log_static(
            timings,
            &ViewBlueprint::new("TimeSeries")
                .with_display_name("Unit timings")
                .with_space_origin("timings"),
        )
        .map_err(string_error)?;
    blueprint
        .log_static(
            format!("{timings}/ViewContents"),
            &ViewContents::new(["+ /timings/**"]),
        )
        .map_err(string_error)?;
    blueprint
        .log_static(
            root,
            &ContainerBlueprint::new(ContainerKind::Horizontal)
                .with_display_name("UnitCompose LiDAR SLAM")
                .with_contents([scene, metrics, timings])
                .with_col_shares([2.4, 1.0, 0.8]),
        )
        .map_err(string_error)?;
    blueprint
        .log_static(
            "viewport",
            &ViewportBlueprint::new()
                .with_root_container(ROOT_CONTAINER_ID)
                .with_auto_views(false),
        )
        .map_err(string_error)?;
    let messages = storage.take();
    drop(blueprint);
    recording.send_blueprint(messages, BlueprintActivationCommand::make_active(id));
    Ok(())
}

#[cfg(feature = "rerun")]
fn string_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn usage() -> String {
    "usage: lidar-slam --module <path> (--inspect <text|dot|mermaid> | --run | --timed-mermaid | --rerun-save <path.rrd> | --rerun-spawn)".to_owned()
}
#[cfg(not(feature = "rerun"))]
fn rerun_disabled() -> String {
    "Rerun output requires rebuilding lidar-slam with --features rerun".to_owned()
}
