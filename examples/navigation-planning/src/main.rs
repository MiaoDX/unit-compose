#[cfg(feature = "rerun")]
use std::collections::VecDeque;
use std::env;
use std::path::PathBuf;

#[cfg(feature = "rerun")]
use navigation_planning::GridPoint;
use navigation_planning::{build_from_path, demo_grid, demo_itinerary};
use unit_compose_allocation_test_harness::GlobalProbe;
#[cfg(feature = "rerun")]
use unit_compose_debug::InspectionAdapter;
#[cfg(feature = "rerun")]
use unit_compose_debug_rerun::{MAX_POSE_TRAIL_POINTS, NavigationFrame, RerunAdapter};

enum Mode {
    Strict,
    Inspect(&'static str),
    TimedMermaid,
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
        Some("--timed-mermaid") => Mode::TimedMermaid,
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

    let mut input = demo_grid();
    prepared
        .warm_up(&input)
        .map_err(|error| format!("warm-up failed: {error:?}"))?;
    let mut probe = GlobalProbe;
    let supplied =
        [prepared.supplied_input::<navigation_planning::RosOccupancyGrid>(input.data.len())];
    if matches!(mode, Mode::TimedMermaid) {
        let reports = run_episode(
            &mut prepared,
            &supplied,
            &mut input,
            &mut probe,
            |_, _, _, _| Ok(()),
        )?;
        print!("{}", prepared.description.to_mermaid_with_runs(&reports));
        return Ok(());
    }

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
        Mode::TimedMermaid => unreachable!("timed Mermaid returned after execution"),
    };
    #[cfg(feature = "rerun")]
    if let Some((mut adapter, route)) = rerun_route {
        adapter
            .fixed_description(&prepared.description)
            .map_err(string_error)?;
        let warm = prepared.post_run_snapshot()?;
        adapter
            .navigation_map(warm.width, warm.height, &input.data, warm.cost_map)
            .map_err(string_error)?;
        let mut tick = 0_i64;
        let mut trail = VecDeque::<[f32; 2]>::with_capacity(MAX_POSE_TRAIL_POINTS);
        run_episode(
            &mut prepared,
            &supplied,
            &mut input,
            &mut probe,
            |_, input, snapshot, report| {
                let raw_path = points(snapshot.raw_path);
                let smoothed_path = snapshot.smoothed_path.map(points);
                let final_path = points(snapshot.final_path);
                adapter
                    .navigation_frame_at(
                        tick,
                        NavigationFrame {
                            width: snapshot.width,
                            height: snapshot.height,
                            occupancy_grid: &input.data,
                            cost_map: snapshot.cost_map,
                            raw_path: &raw_path,
                            smoothed_path: smoothed_path.as_deref(),
                            start: point(input.start),
                            goal: point(input.goal),
                        },
                    )
                    .map_err(string_error)?;
                adapter
                    .run_snapshot_at(tick, report)
                    .map_err(string_error)?;
                for pose in final_path {
                    if trail.len() == MAX_POSE_TRAIL_POINTS {
                        trail.pop_front();
                    }
                    trail.push_back(pose);
                    let bounded_trail = trail.iter().copied().collect::<Vec<_>>();
                    adapter
                        .navigation_pose_at(tick, pose, &bounded_trail, snapshot.height)
                        .map_err(string_error)?;
                    tick += 1;
                }
                Ok(())
            },
        )?;
        adapter.flush();
        println!("rerun={route} legs=1000");
        return Ok(());
    }

    let path = prepared
        .run_checked_profiled(&supplied, &input, &mut [&mut probe])
        .map_err(|error| format!("strict run failed: {error:?}"))?;
    let first = *path
        .first()
        .ok_or_else(|| "planner returned no path".to_owned())?;
    let last = *path
        .last()
        .ok_or_else(|| "planner returned no path".to_owned())?;
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

fn run_episode<F>(
    prepared: &mut navigation_planning::PreparedNavigation,
    supplied: &[unit_compose_core::ModuleInput],
    input: &mut navigation_planning::RosOccupancyGrid,
    probe: &mut GlobalProbe,
    mut after_leg: F,
) -> Result<Vec<unit_compose_core::RunReportSnapshot>, String>
where
    F: for<'a> FnMut(
        usize,
        &navigation_planning::RosOccupancyGrid,
        navigation_planning::NavigationPostRunSnapshot<'a>,
        &unit_compose_core::RunReportSnapshot,
    ) -> Result<(), String>,
{
    let itinerary = demo_itinerary();
    let mut reports = Vec::with_capacity(itinerary.legs.len());
    for (leg_index, leg) in itinerary.legs.iter().enumerate() {
        input.start = leg.start;
        input.goal = leg.goal;
        prepared
            .run_checked_profiled(supplied, input, &mut [probe])
            .map_err(|error| format!("episode leg {leg_index} failed: {error:?}"))?;
        let report = prepared.module.report().snapshot();
        let snapshot = prepared.post_run_snapshot()?;
        after_leg(leg_index, input, snapshot, &report)?;
        reports.push(report);
    }
    Ok(reports)
}

#[cfg(feature = "rerun")]
fn points(path: &[GridPoint]) -> Vec<[f32; 2]> {
    path.iter().copied().map(point).collect()
}

#[cfg(feature = "rerun")]
fn point(point: GridPoint) -> [f32; 2] {
    [f32::from(point.x) + 0.5, f32::from(point.y) + 0.5]
}

#[cfg(feature = "rerun")]
fn string_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn usage() -> String {
    "usage: navigation-planning --module <path> (--strict | --inspect <text|dot|mermaid> | --timed-mermaid | --rerun-save <path.rrd> | --rerun-spawn)".to_owned()
}

#[cfg(not(feature = "rerun"))]
fn rerun_disabled() -> String {
    "Rerun output requires rebuilding navigation-planning with --features rerun".to_owned()
}
