# Slamwich LiDAR dataset selection

Date: 2026-08-12
Phase: 1 real-data gate
Decision: **BLOCKED_NEEDS_DECISION - no sequence is selected or pinned**

## Decision table

| Candidate | Primary-source finding | Decision |
| --- | --- | --- |
| Boreas | The dataset centers Applanix ground truth. No primary documentation inspected for this gate identifies a separate wheel/vehicle odometry product independent of that reference solution. | Reject for accuracy evaluation: independence is unproved. |
| KITTI raw/odometry | OXTS/benchmark poses are reference products. The published products do not identify a separate synchronized wheel-odometry stream suitable for `update_odometry()` while retaining independent reference poses. | Reject: no evidenced three-stream contract. |
| M2DGR | The primary README documents `/velodyne_points` and external RTK/INS, Leica, or Vicon reference sources, but its published topic list does not document wheel odometry. The smallest listed bag is `door_02` at 10.5 GiB. Terms say MIT and academic purpose, with commercial use requiring contact. | Reject: odometry stream unproved, source exceeds 1 GiB, and public sampled-recording permission is not explicit. |
| BotanicGarden | The primary README lists a wheel encoder in the platform hardware, but the published bag topic table exposes LiDAR, IMUs, and `/gt_poses`, not encoder/odometry data. Its smallest listed no-vision bag is 5.9 GiB (`1006-03`). Ground truth is map-localized in a survey-grade map. Terms state academic purpose but do not expressly allow redistribution in public recordings. | Reject: required odometry is absent from the published interface, source exceeds 1 GiB, and recording permission is unproved. |

## Primary sources

- Boreas project: <https://www.boreas.utias.utoronto.ca/>
- KITTI raw and odometry pages:
  <https://www.cvlibs.net/datasets/kitti/raw_data.php> and
  <https://www.cvlibs.net/datasets/kitti/eval_odometry.php>
- M2DGR primary README:
  <https://github.com/SJTU-ViSYS/M2DGR/blob/main/README.md>
- BotanicGarden primary README:
  <https://github.com/robot-pesg/BotanicGarden/blob/main/README.md>

The M2DGR README identifies a VLP-32C, GNSS/RTK/INS, Leica, and Vicon sensors,
lists `/velodyne_points`, and publishes sequence archives from 10.5 GiB upward.
It does not list wheel encoder or wheel odometry data. The BotanicGarden README
identifies a wheel encoder as hardware, but its actual public topic table lists
only cameras, two LiDARs, two IMUs, and ground-truth poses. Its reduced bags
start at 5.9 GiB. These are documentary product boundaries, not assumptions
about what the robots may have used internally.

## Gate result

No candidate satisfies all of the approved requirements simultaneously:

1. timestamped 3D LiDAR;
2. independently measured odometry exposed in the public data product;
3. separate reference poses;
4. a stable, credential-free bounded source under 1 GiB;
5. terms covering a public Rerun recording containing sampled scan data.

No payload was downloaded, no planar statistics or native baseline were run,
and no manifest was created. Doing so would not repair the missing odometry,
transfer, or publication-rights gates.

## Required decision

The approved product cannot advance to Phase 2 without changing scope. The
maintainer must choose one of the parked alternatives: approve a clearly
labeled real-scan episode without independent accuracy claims, approve a
different dataset/hosting contract that satisfies the gates, or stop this
showcase. Reference poses must not be fed to `slamwich` as odometry.
