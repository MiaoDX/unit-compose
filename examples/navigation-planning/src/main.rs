use std::env;
use std::path::PathBuf;

#[cfg(feature = "rerun")]
use navigation_planning::GridPoint;
use navigation_planning::{build_from_path, demo_grid};
use unit_compose_allocation_test_harness::GlobalProbe;
#[cfg(feature = "rerun")]
use unit_compose_debug::InspectionAdapter;
#[cfg(feature = "rerun")]
use unit_compose_debug_rerun::{NavigationFrame, RerunAdapter};

enum Mode {
    Strict,
    Inspect(&'static str),
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
        Some("--strict") => Mode::Strict,
        Some("--inspect") => match args.next().as_deref() {
            Some("text") => Mode::Inspect("text"),
            Some("dot") => Mode::Inspect("dot"),
            Some("mermaid") => Mode::Inspect("mermaid"),
            _ => return Err("--inspect requires text, dot, or mermaid".to_owned()),
        },
        Some("--rerun-save") => {
            #[cfg(feature = "rerun")]
            {
                Mode::RerunSave(PathBuf::from(
                    args.next()
                        .ok_or_else(|| "--rerun-save requires an .rrd path".to_owned())?,
                ))
            }
            #[cfg(not(feature = "rerun"))]
            return Err(rerun_disabled());
        }
        Some("--rerun-spawn") => {
            #[cfg(feature = "rerun")]
            {
                Mode::RerunSpawn
            }
            #[cfg(not(feature = "rerun"))]
            return Err(rerun_disabled());
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
            _ => unreachable!("view was validated by argument parsing"),
        };
        print!("{output}");
        return Ok(());
    }

    let input = demo_grid();
    prepared
        .warm_up(&input)
        .map_err(|error| format!("warm-up failed: {error:?}"))?;
    let mut probe = GlobalProbe;
    let supplied =
        prepared.supplied_input::<navigation_planning::RosOccupancyGrid>(input.data.len());
    let path = prepared
        .run_checked_profiled(&[supplied], &input, &mut [&mut probe])
        .map_err(|error| format!("strict run failed: {error:?}"))?;
    let first = path
        .first()
        .ok_or_else(|| "planner returned no path".to_owned())?;
    let last = path
        .last()
        .ok_or_else(|| "planner returned no path".to_owned())?;

    #[cfg(feature = "rerun")]
    let rerun_route = match mode {
        Mode::Strict => None,
        Mode::RerunSave(output) => {
            let label = format!("save:{}", output.display());
            Some((RerunAdapter::save(output).map_err(string_error)?, label))
        }
        Mode::RerunSpawn => Some((
            RerunAdapter::spawn().map_err(string_error)?,
            "spawn".to_owned(),
        )),
        Mode::Inspect(_) => unreachable!("inspection returned before execution"),
    };
    #[cfg(feature = "rerun")]
    if let Some((mut adapter, route)) = rerun_route {
        adapter
            .fixed_description(&prepared.description)
            .map_err(string_error)?;
        let snapshot = prepared.post_run_snapshot()?;
        let raw_path = points(snapshot.raw_path);
        let smoothed_path = snapshot.smoothed_path.map(points);
        let final_path = points(snapshot.final_path);
        adapter
            .navigation_frame(NavigationFrame {
                width: snapshot.width,
                height: snapshot.height,
                binary_map: snapshot.binary_map,
                cost_map: snapshot.cost_map,
                raw_path: &raw_path,
                smoothed_path: smoothed_path.as_deref(),
                final_path: &final_path,
            })
            .map_err(string_error)?;
        adapter
            .run_snapshot(&prepared.module.report().snapshot())
            .map_err(string_error)?;
        adapter.flush();
        println!("rerun={route}");
    }

    println!(
        "module={} units={} path_points={} start=({}, {}) goal=({}, {}) allocations=0 reallocations=0 deallocations=0",
        prepared.graph.module,
        prepared.graph.units.len(),
        path.len(),
        first.x,
        first.y,
        last.x,
        last.y
    );
    Ok(())
}

#[cfg(feature = "rerun")]
fn points(path: &[GridPoint]) -> Vec<[f32; 2]> {
    path.iter()
        .map(|point| [f32::from(point.x) + 0.5, f32::from(point.y) + 0.5])
        .collect()
}

#[cfg(feature = "rerun")]
fn string_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn usage() -> String {
    "usage: navigation-planning --module <path> (--strict | --inspect <text|dot|mermaid> | --rerun-save <path.rrd> | --rerun-spawn)".to_owned()
}

#[cfg(not(feature = "rerun"))]
fn rerun_disabled() -> String {
    "Rerun output requires rebuilding navigation-planning with --features rerun".to_owned()
}
