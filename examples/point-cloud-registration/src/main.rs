use std::env;
use std::path::PathBuf;

#[cfg(feature = "rerun")]
use point_cloud_registration::PreparedPointCloudRegistration;
use point_cloud_registration::{PointCloudPair, build_from_path};

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
        let output = match view {
            "text" => prepared.description.to_text(),
            "dot" => prepared.description.to_dot(),
            "mermaid" => prepared.description.to_mermaid(),
            _ => unreachable!(),
        };
        print!("{output}");
        return Ok(());
    }
    let input = load_pair()?;
    match mode {
        Mode::Run => {
            prepared
                .run(&input)
                .map_err(|error| format!("run failed: {error:?}"))?;
            let snapshot = prepared.snapshot()?;
            println!(
                "points={} iterations={} initial_rmse={:.6} final_rmse={:.6}",
                snapshot.aligned.len(),
                snapshot.iterations,
                snapshot.initial_rmse,
                snapshot.final_rmse
            );
        }
        Mode::TimedMermaid => {
            let mut reports = Vec::with_capacity(8);
            for _ in 0..8 {
                prepared
                    .run_profiled(&input)
                    .map_err(|error| format!("run failed: {error:?}"))?;
                reports.push(prepared.module.report().snapshot());
            }
            print!("{}", prepared.description.to_mermaid_with_runs(&reports));
        }
        #[cfg(feature = "rerun")]
        Mode::RerunSave(path) => save_rerun(&mut prepared, &input, path, false)?,
        #[cfg(feature = "rerun")]
        Mode::RerunSpawn => save_rerun(&mut prepared, &input, PathBuf::new(), true)?,
        Mode::Inspect(_) => unreachable!(),
    }
    Ok(())
}

fn load_pair() -> Result<PointCloudPair, String> {
    load_pair_from(std::path::Path::new(
        "target/demo-data/point-cloud-registration",
    ))
}

fn load_pair_from(root: &std::path::Path) -> Result<PointCloudPair, String> {
    let source_path = root.join("cloud_bin_0.pcd");
    let target_path = root.join("cloud_bin_1.pcd");
    if !source_path.is_file() || !target_path.is_file() {
        return Err("missing showcase data; run scripts/fetch-showcase-data.sh".to_owned());
    }
    let source_cloud =
        kornia_3d::io::pcd::read_pcd_binary(&source_path).map_err(|error| error.to_string())?;
    let target_cloud =
        kornia_3d::io::pcd::read_pcd_binary(&target_path).map_err(|error| error.to_string())?;
    Ok(PointCloudPair {
        source: source_cloud.points().to_vec(),
        target: target_cloud.points().to_vec(),
        initial_rotation: [
            [0.86214000, 0.01137220, -0.50654700],
            [-0.13881100, 0.96680300, -0.21455100],
            [0.48729000, 0.25528600, 0.83509300],
        ],
        initial_translation: [0.5, 0.7, -1.4],
    })
}

#[cfg(feature = "rerun")]
fn save_rerun(
    prepared: &mut PreparedPointCloudRegistration,
    input: &PointCloudPair,
    path: PathBuf,
    spawn: bool,
) -> Result<(), String> {
    use re_sdk::RecordingStreamBuilder;
    use re_types::archetypes::{LineStrips3D, Points3D, Scalars};
    use re_types::components::Color;
    let recording = if spawn {
        RecordingStreamBuilder::new("unit-compose-point-cloud-registration").spawn()
    } else {
        RecordingStreamBuilder::new("unit-compose-point-cloud-registration").save(&path)
    }
    .map_err(|error| error.to_string())?;
    send_point_blueprint(&recording)?;
    prepared
        .run(input)
        .map_err(|error| format!("run failed: {error:?}"))?;
    let snapshot = prepared.snapshot()?;
    let initial = input
        .source
        .iter()
        .map(|point| {
            [
                (input.initial_rotation[0][0] * point[0]
                    + input.initial_rotation[0][1] * point[1]
                    + input.initial_rotation[0][2] * point[2]
                    + input.initial_translation[0]) as f32,
                (input.initial_rotation[1][0] * point[0]
                    + input.initial_rotation[1][1] * point[1]
                    + input.initial_rotation[1][2] * point[2]
                    + input.initial_translation[1]) as f32,
                (input.initial_rotation[2][0] * point[0]
                    + input.initial_rotation[2][1] * point[1]
                    + input.initial_rotation[2][2] * point[2]
                    + input.initial_translation[2]) as f32,
            ]
        })
        .collect::<Vec<_>>();
    let target = input
        .target
        .iter()
        .map(|point| [point[0] as f32, point[1] as f32, point[2] as f32])
        .collect::<Vec<_>>();
    let aligned = snapshot
        .aligned
        .iter()
        .map(|point| [point[0] as f32, point[1] as f32, point[2] as f32])
        .collect::<Vec<_>>();
    recording.set_time_sequence("registration_step", 0);
    recording
        .log(
            "cloud/target",
            &Points3D::new(target.clone()).with_colors([Color::from_rgb(150, 150, 150)]),
        )
        .map_err(|error| error.to_string())?;
    recording
        .log(
            "cloud/initial_source",
            &Points3D::new(initial).with_colors([Color::from_rgb(220, 20, 60)]),
        )
        .map_err(|error| error.to_string())?;
    recording
        .log(
            "cloud/frames/initial",
            &coordinate_frame(input.initial_rotation, input.initial_translation),
        )
        .map_err(|error| error.to_string())?;
    recording.set_time_sequence("registration_step", 1);
    recording
        .log(
            "cloud/aligned_source",
            &Points3D::new(aligned.clone()).with_colors([Color::from_rgb(65, 105, 225)]),
        )
        .map_err(|error| error.to_string())?;
    recording
        .log(
            "cloud/frames/aligned",
            &coordinate_frame(snapshot.rotation, snapshot.translation),
        )
        .map_err(|error| error.to_string())?;
    let residuals = aligned
        .iter()
        .step_by((aligned.len() / 128).max(1))
        .take(128)
        .map(|&point| {
            let nearest = target
                .iter()
                .min_by(|left, right| {
                    squared_distance(point, **left).total_cmp(&squared_distance(point, **right))
                })
                .copied()
                .unwrap_or(point);
            vec![point, nearest]
        })
        .collect::<Vec<_>>();
    recording
        .log(
            "cloud/residuals",
            &LineStrips3D::new(residuals).with_colors([Color::from_rgb(90, 90, 90)]),
        )
        .map_err(|error| error.to_string())?;
    recording
        .log("metrics/rmse", &Scalars::new([snapshot.final_rmse]))
        .map_err(|error| error.to_string())?;
    recording
        .log(
            "metrics/initial_rmse",
            &Scalars::new([snapshot.initial_rmse]),
        )
        .map_err(|error| error.to_string())?;
    recording
        .log(
            "metrics/transform",
            &Scalars::new(
                snapshot
                    .rotation
                    .iter()
                    .flatten()
                    .copied()
                    .chain(snapshot.translation),
            ),
        )
        .map_err(|error| error.to_string())?;
    recording
        .log(
            "metrics/capacity/points",
            &Scalars::new([snapshot.aligned.len() as f64]),
        )
        .map_err(|error| error.to_string())?;
    for timing in prepared.module.report().unit_timings() {
        recording
            .log(
                format!("timings/units/{}", timing.unit_ordinal),
                &Scalars::new([timing.elapsed.as_secs_f64() * 1_000.0]),
            )
            .map_err(|error| error.to_string())?;
    }
    recording.flush_blocking();
    if !spawn {
        println!("rerun=save:{}", path.display());
    }
    Ok(())
}

#[cfg(feature = "rerun")]
fn coordinate_frame(
    rotation: [[f64; 3]; 3],
    translation: [f64; 3],
) -> re_types::archetypes::Arrows3D {
    use re_types::archetypes::Arrows3D;
    use re_types::components::Color;
    let origin = [
        translation[0] as f32,
        translation[1] as f32,
        translation[2] as f32,
    ];
    let vectors = (0..3)
        .map(|axis| {
            [
                rotation[0][axis] as f32 * 0.4,
                rotation[1][axis] as f32 * 0.4,
                rotation[2][axis] as f32 * 0.4,
            ]
        })
        .collect::<Vec<_>>();
    Arrows3D::from_vectors(vectors)
        .with_origins([origin; 3])
        .with_colors([
            Color::from_rgb(220, 20, 60),
            Color::from_rgb(34, 139, 34),
            Color::from_rgb(65, 105, 225),
        ])
}

#[cfg(feature = "rerun")]
fn send_point_blueprint(recording: &re_sdk::RecordingStream) -> Result<(), String> {
    use re_sdk::RecordingStreamBuilder;
    use re_sdk::external::re_log_types::{BlueprintActivationCommand, StoreId, StoreKind};
    use re_types::blueprint::archetypes::{
        ContainerBlueprint, ViewBlueprint, ViewContents, ViewportBlueprint,
    };
    use re_types::blueprint::components::ContainerKind;
    let id = StoreId::from_string(
        StoreKind::Blueprint,
        "unit-compose-point-registration-blueprint-v1".to_owned(),
    );
    let (blueprint, storage) = RecordingStreamBuilder::new("unit-compose-point-cloud-registration")
        .store_id(id.clone())
        .blueprint()
        .memory()
        .map_err(|error| error.to_string())?;
    let cloud_view = "view/22222222-1212-1212-1212-121212121212";
    let metrics_view = "view/23232323-1313-1313-1313-131313131313";
    let root = "container/21212121-1212-1313-1414-151515151515";
    blueprint
        .log_static(
            cloud_view,
            &ViewBlueprint::new("3D")
                .with_display_name("Point-cloud registration")
                .with_space_origin("cloud"),
        )
        .map_err(|error| error.to_string())?;
    blueprint
        .log_static(
            format!("{cloud_view}/ViewContents"),
            &ViewContents::new(["+ /cloud/**"]),
        )
        .map_err(|error| error.to_string())?;
    blueprint
        .log_static(
            metrics_view,
            &ViewBlueprint::new("TimeSeries")
                .with_display_name("Metrics and Unit timings")
                .with_space_origin("metrics"),
        )
        .map_err(|error| error.to_string())?;
    blueprint
        .log_static(
            format!("{metrics_view}/ViewContents"),
            &ViewContents::new(["+ /metrics/**", "+ /timings/**"]),
        )
        .map_err(|error| error.to_string())?;
    blueprint
        .log_static(
            root,
            &ContainerBlueprint::new(ContainerKind::Horizontal)
                .with_display_name("UnitCompose point-cloud registration")
                .with_contents([cloud_view, metrics_view])
                .with_col_shares([2.0, 1.0]),
        )
        .map_err(|error| error.to_string())?;
    blueprint
        .log_static(
            "viewport",
            &ViewportBlueprint::new()
                .with_root_container([
                    0x21, 0x21, 0x21, 0x21, 0x12, 0x12, 0x13, 0x13, 0x14, 0x14, 0x15, 0x15, 0x15,
                    0x15, 0x15, 0x15,
                ])
                .with_auto_views(false),
        )
        .map_err(|error| error.to_string())?;
    let messages = storage.take();
    drop(blueprint);
    recording.send_blueprint(messages, BlueprintActivationCommand::make_active(id));
    Ok(())
}

#[cfg(feature = "rerun")]
fn squared_distance(left: [f32; 3], right: [f32; 3]) -> f32 {
    (left[0] - right[0]).powi(2) + (left[1] - right[1]).powi(2) + (left[2] - right[2]).powi(2)
}

fn usage() -> String {
    "usage: point-cloud-registration --module <path> (--run | --inspect <text|dot|mermaid> | --timed-mermaid | --rerun-save <path.rrd> | --rerun-spawn)".to_owned()
}
#[cfg(not(feature = "rerun"))]
fn rerun_disabled() -> String {
    "Rerun output requires rebuilding point-cloud-registration with --features rerun".to_owned()
}

#[cfg(test)]
mod tests {
    #[test]
    fn missing_data_names_the_fetch_command() {
        let error = super::load_pair_from(std::path::Path::new("definitely-not-present"))
            .err()
            .unwrap();
        assert_eq!(
            error,
            "missing showcase data; run scripts/fetch-showcase-data.sh"
        );
    }
}
