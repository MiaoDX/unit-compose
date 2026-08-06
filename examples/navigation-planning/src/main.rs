use std::env;
use std::path::PathBuf;

use navigation_planning::{build_from_path, demo_grid};
use unit_compose_allocation_test_harness::GlobalProbe;

fn main() -> Result<(), String> {
    let mut args = env::args().skip(1);
    if args.next().as_deref() != Some("--module") {
        return Err("usage: navigation-planning --module <path> --strict".to_owned());
    }
    let path = PathBuf::from(
        args.next()
            .ok_or_else(|| "--module requires a path".to_owned())?,
    );
    let mut prepared = build_from_path(&path)?;
    match args.next().as_deref() {
        Some("--inspect") => {
            let output = match args.next().as_deref() {
                Some("text") => prepared.description.to_text(),
                Some("dot") => prepared.description.to_dot(),
                Some("mermaid") => prepared.description.to_mermaid(),
                _ => return Err("--inspect requires text, dot, or mermaid".to_owned()),
            };
            if args.next().is_some() {
                return Err("unexpected argument after inspection view".to_owned());
            }
            print!("{output}");
            return Ok(());
        }
        Some("--strict") if args.next().is_none() => {}
        _ => {
            return Err(
                "usage: navigation-planning --module <path> (--strict | --inspect <text|dot|mermaid>)"
                    .to_owned(),
            );
        }
    }

    let input = demo_grid();
    prepared
        .warm_up(&input)
        .map_err(|error| format!("warm-up failed: {error:?}"))?;
    let mut probe = GlobalProbe;
    let supplied =
        prepared.supplied_input::<navigation_planning::RosOccupancyGrid>(input.data.len());
    let path_view = prepared
        .run_checked_profiled(&[supplied], &input, &mut [&mut probe])
        .map_err(|error| format!("strict run failed: {error:?}"))?;
    let first = path_view
        .first()
        .ok_or_else(|| "planner returned no path".to_owned())?;
    let last = path_view
        .last()
        .ok_or_else(|| "planner returned no path".to_owned())?;
    println!(
        "module={} units={} path_points={} start=({}, {}) goal=({}, {}) allocations=0 reallocations=0 deallocations=0",
        prepared.graph.module,
        prepared.graph.units.len(),
        path_view.len(),
        first.x,
        first.y,
        last.x,
        last.y
    );
    Ok(())
}
