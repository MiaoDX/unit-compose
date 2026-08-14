use lidar_slam::{MIN_EPISODE_FRAMES, build_from_path, run_episode};
use std::env;
use std::time::Instant;

fn main() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let frames = match args.next() {
        Some(value) => value
            .parse()
            .map_err(|_| format!("invalid frame count: {value}"))?,
        None => MIN_EPISODE_FRAMES,
    };
    if args.next().is_some() {
        return Err("unexpected trailing argument".to_owned());
    }
    let start = Instant::now();
    let mut prepared = build_from_path(std::path::Path::new("examples/lidar-slam/lidar-slam.yaml"))
        .map_err(|error| format!("showcase module failed to build: {error}"))?;
    let (snapshots, _) = run_episode(&mut prepared, frames, false)?;
    let snapshot = snapshots
        .last()
        .ok_or_else(|| "episode produced no snapshot".to_owned())?;
    println!(
        "frames={frames} elapsed_ms={} pose=({:.6},{:.6},{:.6}) keyframes={} edges={}",
        start.elapsed().as_secs_f64() * 1000.0,
        snapshot.estimated.x,
        snapshot.estimated.y,
        snapshot.estimated.theta,
        snapshot.keyframe_count,
        snapshot.edges.len()
    );
    Ok(())
}
