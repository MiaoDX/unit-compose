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
    if args.next().as_deref() != Some("--strict") || args.next().is_some() {
        return Err("the Quickstart requires exactly one --strict flag".to_owned());
    }

    let input = demo_grid();
    let mut prepared = build_from_path(&path)?;
    prepared
        .warm_up(&input)
        .map_err(|error| format!("warm-up failed: {error:?}"))?;
    let mut probe = GlobalProbe;
    let path_view = prepared
        .module
        .run_profiled(&input, &mut [&mut probe], None)
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
