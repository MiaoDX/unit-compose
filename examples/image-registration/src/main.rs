use std::env;
use std::path::PathBuf;

#[cfg(feature = "rerun")]
use image_registration::PreparedImageRegistration;
use image_registration::{ImagePair, build_from_path};
use kornia_image::Image;
use kornia_imgproc::warp::warp_perspective_u8;
use kornia_tensor::CpuAllocator;

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
        Mode::Run => {
            prepared
                .run(&input)
                .map_err(|error| format!("run failed: {error:?}"))?;
            let snapshot = prepared.snapshot()?;
            println!(
                "matches={} inliers={} reprojection_rmse={:.6} inlier_ratio={:.4}",
                snapshot.matches.len(),
                snapshot.inliers.iter().filter(|&&value| value).count(),
                snapshot.reprojection_rmse,
                snapshot.inliers.iter().filter(|&&value| value).count() as f64
                    / snapshot.matches.len() as f64
            );
        }
        #[cfg(feature = "rerun")]
        Mode::RerunSave(path) => save_rerun(&mut prepared, &input, path, false)?,
        #[cfg(feature = "rerun")]
        Mode::RerunSpawn => save_rerun(&mut prepared, &input, PathBuf::new(), true)?,
        Mode::Inspect(_) => unreachable!(),
    }
    Ok(())
}

fn load_pair() -> Result<ImagePair, String> {
    load_pair_from(std::path::Path::new(
        "target/demo-data/image-registration/building.jpg",
    ))
}

fn load_pair_from(path: &std::path::Path) -> Result<ImagePair, String> {
    if !path.is_file() {
        return Err("missing showcase data; run scripts/fetch-showcase-data.sh".to_owned());
    }
    let source = kornia_io::jpeg::read_image_jpeg_rgb8(path)
        .map_err(|error| error.to_string())?
        .into_inner();
    let mut target = Image::from_size_val(source.size(), 0_u8, CpuAllocator)
        .map_err(|error| error.to_string())?;
    let transform = [1.0, 0.035, 18.0, -0.025, 1.0, 14.0, 0.00008, -0.00004, 1.0];
    warp_perspective_u8(&source, &mut target, &transform).map_err(|error| error.to_string())?;
    Ok(ImagePair { source, target })
}

#[cfg(feature = "rerun")]
fn save_rerun(
    prepared: &mut PreparedImageRegistration,
    input: &ImagePair,
    path: PathBuf,
    spawn: bool,
) -> Result<(), String> {
    use re_sdk::RecordingStreamBuilder;
    use re_types::archetypes::{Image as RerunImage, LineStrips2D, Points2D, Scalars};
    use re_types::components::Color;
    use re_types::datatypes::ColorModel;
    let recording = if spawn {
        RecordingStreamBuilder::new("unit-compose-image-registration").spawn()
    } else {
        RecordingStreamBuilder::new("unit-compose-image-registration").save(&path)
    }
    .map_err(|error| error.to_string())?;
    send_image_blueprint(&recording)?;
    prepared
        .run(input)
        .map_err(|error| format!("run failed: {error:?}"))?;
    let snapshot = prepared.snapshot()?;
    let size = input.source.size();
    let resolution = [size.width as u32, size.height as u32];
    recording
        .log(
            "image/source",
            &RerunImage::from_elements(input.source.as_slice(), resolution, ColorModel::RGB),
        )
        .map_err(|error| error.to_string())?;
    recording
        .log(
            "image/target",
            &RerunImage::from_elements(input.target.as_slice(), resolution, ColorModel::RGB),
        )
        .map_err(|error| error.to_string())?;
    recording
        .log(
            "features/source_keypoints",
            &Points2D::new(snapshot.source_keypoints.clone())
                .with_colors([Color::from_rgb(0, 137, 123)]),
        )
        .map_err(|error| error.to_string())?;
    recording
        .log(
            "features/target_keypoints",
            &Points2D::new(snapshot.target_keypoints.clone())
                .with_colors([Color::from_rgb(65, 105, 225)]),
        )
        .map_err(|error| error.to_string())?;
    let match_lines = |selection: Option<bool>| {
        snapshot
            .matches
            .iter()
            .enumerate()
            .filter_map(|(index, &(source, target))| {
                selection
                    .is_none_or(|selected| snapshot.inliers[index] == selected)
                    .then_some(vec![
                        snapshot.source_keypoints[source],
                        snapshot.target_keypoints[target],
                    ])
            })
            .collect::<Vec<_>>()
    };
    recording
        .log(
            "matches/candidates",
            &LineStrips2D::new(match_lines(None)).with_colors([Color::from_rgb(120, 120, 120)]),
        )
        .map_err(|error| error.to_string())?;
    recording
        .log(
            "matches/inliers",
            &LineStrips2D::new(match_lines(Some(true))).with_colors([Color::from_rgb(34, 139, 34)]),
        )
        .map_err(|error| error.to_string())?;
    recording
        .log(
            "matches/outliers",
            &LineStrips2D::new(match_lines(Some(false)))
                .with_colors([Color::from_rgb(220, 20, 60)]),
        )
        .map_err(|error| error.to_string())?;
    let overlay = snapshot
        .warped
        .iter()
        .zip(&snapshot.target_gray)
        .map(|(&left, &right)| ((u16::from(left) + u16::from(right)) / 2) as u8)
        .collect::<Vec<_>>();
    recording
        .log(
            "image/warped",
            &RerunImage::from_elements(&snapshot.warped, resolution, ColorModel::L),
        )
        .map_err(|error| error.to_string())?;
    recording
        .log(
            "image/overlay",
            &RerunImage::from_elements(&overlay, resolution, ColorModel::L),
        )
        .map_err(|error| error.to_string())?;
    let inlier_points = snapshot
        .matches
        .iter()
        .enumerate()
        .filter_map(|(index, &(a, _))| {
            snapshot.inliers[index].then_some(snapshot.source_keypoints[a])
        })
        .collect::<Vec<_>>();
    recording
        .log("image/inliers", &Points2D::new(inlier_points))
        .map_err(|error| error.to_string())?;
    recording
        .log(
            "metrics/reprojection_rmse",
            &Scalars::new([snapshot.reprojection_rmse]),
        )
        .map_err(|error| error.to_string())?;
    let inlier_count = snapshot.inliers.iter().filter(|&&value| value).count();
    recording
        .log(
            "metrics/inlier_ratio",
            &Scalars::new([inlier_count as f64 / snapshot.matches.len() as f64]),
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
fn send_image_blueprint(recording: &re_sdk::RecordingStream) -> Result<(), String> {
    use re_sdk::RecordingStreamBuilder;
    use re_sdk::external::re_log_types::{BlueprintActivationCommand, StoreId, StoreKind};
    use re_types::blueprint::archetypes::{
        ContainerBlueprint, ViewBlueprint, ViewContents, ViewportBlueprint,
    };
    use re_types::blueprint::components::ContainerKind;
    let id = StoreId::from_string(
        StoreKind::Blueprint,
        "unit-compose-image-registration-blueprint-v1".to_owned(),
    );
    let (blueprint, storage) = RecordingStreamBuilder::new("unit-compose-image-registration")
        .store_id(id.clone())
        .blueprint()
        .memory()
        .map_err(|error| error.to_string())?;
    let image_view = "view/12121212-1212-1212-1212-121212121212";
    let metrics_view = "view/13131313-1313-1313-1313-131313131313";
    let root = "container/11111111-1212-1313-1414-151515151515";
    blueprint
        .log_static(
            image_view,
            &ViewBlueprint::new("2D")
                .with_display_name("Image registration")
                .with_space_origin("image"),
        )
        .map_err(|error| error.to_string())?;
    blueprint
        .log_static(
            format!("{image_view}/ViewContents"),
            &ViewContents::new(["+ /image/**", "+ /features/**", "+ /matches/**"]),
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
                .with_display_name("UnitCompose image registration")
                .with_contents([image_view, metrics_view])
                .with_col_shares([2.0, 1.0]),
        )
        .map_err(|error| error.to_string())?;
    blueprint
        .log_static(
            "viewport",
            &ViewportBlueprint::new()
                .with_root_container([
                    0x11, 0x11, 0x11, 0x11, 0x12, 0x12, 0x13, 0x13, 0x14, 0x14, 0x15, 0x15, 0x15,
                    0x15, 0x15, 0x15,
                ])
                .with_auto_views(true),
        )
        .map_err(|error| error.to_string())?;
    let messages = storage.take();
    drop(blueprint);
    recording.send_blueprint(messages, BlueprintActivationCommand::make_active(id));
    Ok(())
}

fn usage() -> String {
    "usage: image-registration --module <path> (--run | --inspect <text|dot|mermaid> | --timed-mermaid | --rerun-save <path.rrd> | --rerun-spawn)".to_owned()
}
#[cfg(not(feature = "rerun"))]
fn rerun_disabled() -> String {
    "Rerun output requires rebuilding image-registration with --features rerun".to_owned()
}

#[cfg(test)]
mod tests {
    #[test]
    fn missing_data_names_the_fetch_command() {
        let error = super::load_pair_from(std::path::Path::new("definitely-not-present.jpg"))
            .err()
            .unwrap();
        assert_eq!(
            error,
            "missing showcase data; run scripts/fetch-showcase-data.sh"
        );
    }
}
