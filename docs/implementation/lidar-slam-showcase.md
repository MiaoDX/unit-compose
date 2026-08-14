# LiDAR SLAM showcase

The LiDAR SLAM example is a best-effort UnitCompose Module backed by commit
`a4e3096` from the MIT-licensed `MiaoDX-fork-and-pruning/slamwich` fork. The
fork inserts a new keyframe before optimizing a closure edge and corrects the
pose-graph Jacobians; the fixes are submitted upstream as `ecto/slamwich#1`
and `ecto/slamwich#2`.
It runs entirely offline over an independently implemented, deterministic
synthetic room episode inspired by the observable shape of the upstream
crate's test scene.

The host prepares one Module and submits 480 synchronized frames in order. The
compiled graph exposes three logical Units: `scan_prepare` validates and bounds
the scan and carries its odometry, `slam` owns the stateful `SlamProcessor`,
and `snapshot` combines the SLAM observation with the original frame to
maintain episode trails and evaluation metrics. The graph's direct
`lidar_frame -> snapshot` edge keeps the generated reference trajectory on an
evaluation-only path; it is never fed to Slamwich as odometry. The current
runtime executes these stages inside one composite Rust Unit, matching the
existing showcase pattern, while preserving their separate graph nodes and
timings.

## Run and inspect

From the repository root:

```bash
cargo run -p lidar-slam --bin lidar-slam --locked -- \
  --module examples/lidar-slam/lidar-slam.yaml --run
cargo run -p lidar-slam --bin lidar-slam --locked -- \
  --module examples/lidar-slam/lidar-slam.yaml --inspect mermaid
cargo run -p lidar-slam --bin lidar-slam --locked -- \
  --module examples/lidar-slam/lidar-slam.yaml --timed-mermaid
```

The default episode publishes at most 384 current-scan points, 512 poses per
trajectory trail, 160 keyframe poses, 2,048 sampled map points, and 320 graph
edges. Input scans contain at most 2,048 points; scan coordinates and
odometry/reference translations outside +/-100 m are rejected. Corrupt or
non-finite scans, bound violations, missing frame indices, and non-increasing
timestamps fail before state mutation or snapshot publication.

A representative 480-frame debug run produced 455 scan updates, 127 keyframes,
three scan-context loop closures, 0.0043 m final translation error, and 0.0016 rad
final rotation error. Timing varies by runner and is reported from actual
Module executions rather than treated as a fixed acceptance value.

## Rerun recording

Rerun is a default-off feature pinned to 0.24.1:

```bash
cargo run -p lidar-slam --bin lidar-slam --features rerun --locked -- \
  --module examples/lidar-slam/lidar-slam.yaml \
  --rerun-save target/lidar-slam-synthetic.rrd
cargo run -p lidar-slam --bin lidar-slam --features rerun --locked -- \
  --module examples/lidar-slam/lidar-slam.yaml --rerun-spawn
```

The fixed blueprint shows the current scan, bounded sampled map, gray estimated
history, orange drifting odometry, blue evaluation reference, green optimized
keyframe trajectory, and a thick magenta loop-closure edge. Translation and
rotation errors, update/keyframe/loop events, capacity metrics, and Unit timing
share the frame timeline.

## CI demo report

`scripts/build-demo-report.sh` builds and runs this episode beside navigation,
image registration, and point-cloud registration. Its report contains the exact
run output, static and 480-sample timed Module graphs, and a fresh `.rrd`
recording. No LiDAR data download is needed; generated reports and recordings
remain under `target/` and are not committed.

## Limitations

Slamwich exposes a planar `x`, `y`, `theta` pose even though scans contain 3D
points, so this example makes an SE(2) SLAM claim rather than an SE(3) or
LiDAR-inertial claim. The dependency's solver may exhibit small floating-point
variation between processors; tests require deterministic inputs, ordering,
stateful progression, finite poses, and bounded outputs rather than cross-run
solver equality or bitwise identical maps. The default figure-eight route
crosses the room center and revisits geometrically distinctive regions so CI
exercises multiple real scan-context matches and pose-graph optimizations
rather than synthesizing events in the presentation layer.
