# V0 demo implementation plan

- **Status:** Proposed implementation plan
- **Date:** 2026-08-03
- **Architecture baseline:** [UnitCompose V0 architecture](../specification/v0-architecture.md)

This plan defines two complementary demonstrations:

1. a small, reliable **navigation planning Quickstart** that proves the V0 framework contract with minimal custom code;
2. an optional **nuScenes LiDAR Showcase** that proves the same concepts on real autonomous-driving data with rich Rerun visualization.

The Quickstart is part of the V0 implementation and continuous integration target. The nuScenes Showcase follows after the core and Quickstart are working and is not required in default CI.

## 1. Goals

The demos must prove that UnitCompose provides value beyond ordinary function calls:

- one binary can load different YAML Module Definitions;
- compatible Unit implementations can be replaced through configuration;
- YAML can add or remove a Unit and therefore change the DAG;
- Resource fan-out and fan-in are visible and validated;
- Unit code remains independently testable;
- the Module embeds in a normal host application;
- Debug can visualize the graph, selected Resources, timing, and failures;
- the core does not depend on ROS, Rerun, Python, a model runtime, or a dataset SDK.

The demos should reuse maintained public libraries and datasets wherever they provide a clear mechanism. UnitCompose-specific code should focus on composition, contracts, adapters, and validation rather than reimplementing mature algorithms.

## 2. Demo portfolio

| Demo | Role | Default CI | External data | Main proof |
| --- | --- | --- | --- | --- |
| Navigation planning Quickstart | First runnable example | Yes | Small Nav2 map asset committed with attribution | YAML replacement, graph changes, fan-out/fan-in, Rerun Debug |
| nuScenes LiDAR Showcase | Real-data flagship | No | Downloaded by user or preparation tool | Large Resources, real sensor data, rich 3D visualization |

## 3. Demo A: navigation planning Quickstart

### 3.1 Why this is the V0 Quickstart

Navigation planning provides a complete, recognizable robotics task without requiring GPU drivers, model weights, ROS installation, Python at runtime, or a large dataset.

The demo can reuse:

- an openly licensed occupancy-map asset from the ROS 2 Navigation2 repository;
- the Rust [`pathfinding`](https://docs.rs/pathfinding/) crate for A* and Dijkstra;
- the Rust [`image`](https://docs.rs/image/) crate for map image loading;
- the Rust [`imageproc`](https://docs.rs/imageproc/) crate where morphology is useful for obstacle inflation;
- the Rust [`rerun`](https://docs.rs/rerun/) SDK for visualization.

The preferred initial map is Nav2's TurtleBot sandbox map:

- [`nav2_bringup/maps/tb3_sandbox.yaml`](https://github.com/ros-navigation/navigation2/blob/main/nav2_bringup/maps/tb3_sandbox.yaml)
- corresponding `tb3_sandbox.pgm`

The asset should be copied into the example with its original license and an attribution notice. The relevant Nav2 package declares Apache-2.0. The implementation PR must verify the specific image file history and preserve any file-level notices.

### 3.2 Module inputs and outputs

Module inputs:

| Resource | Semantic type | Meaning |
| --- | --- | --- |
| `map_asset` | `nav.RosMapAsset/v1` | Map YAML path plus image asset location |
| `request` | `nav.PlanRequest/v1` | Start pose, goal pose, and optional planner constraints |

Module output:

| Resource | Semantic type | Meaning |
| --- | --- | --- |
| `plan` | `nav.PathPlan/v1` | Final world-coordinate path and summary metrics |

### 3.3 Units

#### `nav.ros_map_decoder/v1`

```text
RosMapAsset -> OccupancyGrid
```

Responsibilities:

- parse the standard ROS map YAML fields;
- load PGM or PNG through `image`;
- convert pixels using `negate`, `occupied_thresh`, and `free_thresh`;
- preserve resolution and map origin;
- report malformed metadata with a Unit-local error.

This Unit is adapter code and should remain small.

#### `nav.binary_inflation/v1`

```text
OccupancyGrid -> CostMap
```

Responsibilities:

- inflate occupied cells by the configured robot radius;
- retain unknown-space policy explicitly in configuration;
- produce a binary or small integer cost map suitable for V0 planners.

Use `imageproc` morphology when it fits the grid representation. A compact custom radius mask is acceptable if it is simpler and independently tested.

#### `nav.astar/v1` and `nav.dijkstra/v1`

```text
CostMap + PlanRequest -> PathPlan
```

Both implementations expose the same input and output port contract. They should delegate graph search to the `pathfinding` crate and limit custom code to:

- mapping grid cells to neighbors;
- collision and cost lookup;
- world/grid coordinate conversion;
- assembling `PathPlan` metrics;
- optionally emitting search statistics through Debug data.

Replacing A* with Dijkstra in YAML must require no host-code change and no connection change.

#### `nav.line_of_sight_smoother/v1`

```text
CostMap + PathPlan -> PathPlan
```

Responsibilities:

- remove unnecessary intermediate waypoints using collision-checked line-of-sight shortcuts;
- retain start and goal;
- report original and smoothed path length;
- never cross inflated obstacles.

This is expected to be the only small algorithm implemented specifically for the Quickstart. Keep it simple and deterministic.

### 3.4 Resource DAG

```text
map_asset
    |
    v
RosMapDecoder
    |
    v
occupancy_grid
    |
    v
ObstacleInflation
    |
    +-----------------------------+
    |                             |
    v                             |
navigation_costmap                |
    |                             |
    +--> Planner <-- request      |
             |                    |
             v                    |
          raw_plan                |
             |                    |
             +--> PathSmoother <--+
                         |
                         v
                     final_plan
```

This graph proves:

- Module input Resources;
- Resource fan-out (`navigation_costmap`);
- Unit fan-in (`Planner` and `PathSmoother`);
- compatible Unit replacement (`AStar` / `Dijkstra`);
- optional graph restructuring by removing `PathSmoother`.

### 3.5 YAML variants

The example binary should ship with at least three Module Definitions.

#### `astar.yaml`

- `nav.astar/v1`
- smoother enabled
- exports `final_plan`

#### `dijkstra.yaml`

- `nav.dijkstra/v1`
- the same Resource bindings as A*
- smoother enabled

#### `astar_without_smoothing.yaml`

- `nav.astar/v1`
- no smoother Unit
- exports `raw_plan`

A fourth negative fixture should intentionally contain a type mismatch or cycle and be used in validation tests, not as a normal runnable configuration.

### 3.6 Rerun visualization

Rerun is implemented as an optional Debug adapter, not as code called directly by Units.

Recommended views:

#### Navigation view

- source occupancy grid;
- inflated cost map;
- start and goal points;
- raw path;
- smoothed path;
- optional expanded cells or frontier summary.

Preferred archetypes include `GridMap`, `Points2D`, `LineStrips2D`, and scalar/text summaries, subject to the pinned Rerun SDK version.

#### UnitCompose graph view

Show the compiled bipartite Unit/Resource graph using Rerun graph archetypes when practical. If graph archetypes are not sufficiently stable, log DOT or Mermaid as a text document and render a simplified graph separately.

#### Timing and metrics view

Log at least:

- Unit duration;
- expanded-node count;
- raw and final waypoint count;
- raw and final path length;
- build and validation warnings.

Use a fixed Blueprint so the first run opens with a useful layout.

### 3.7 Quickstart command shape

The implementation should converge on a command similar to:

```bash
cargo run -p navigation-planning-demo -- \
  --module examples/navigation_planning/configs/astar.yaml \
  --start 1.0,1.0 \
  --goal 8.0,7.0 \
  --spawn-rerun
```

Also support a headless mode and recording output:

```bash
cargo run -p navigation-planning-demo -- \
  --module examples/navigation_planning/configs/dijkstra.yaml \
  --save-rerun target/navigation-demo.rrd
```

Exact CLI syntax may change during implementation, but Module configuration, start/goal input, and Debug sink selection must remain separate concerns.

## 4. Demo B: nuScenes LiDAR Showcase

### 4.1 Why this is a second-stage showcase

nuScenes mini provides real autonomous-driving sensor data, calibration, ego poses, annotations, cameras, radar, and LiDAR. Rerun already maintains a comprehensive nuScenes dataset example with data download, transforms, annotations, and a multi-view Blueprint:

- [`rerun-io/rerun/examples/python/nuscenes_dataset`](https://github.com/rerun-io/rerun/tree/main/examples/python/nuscenes_dataset)
- [`rerun-io/rerun/examples/python/lidar`](https://github.com/rerun-io/rerun/tree/main/examples/python/lidar)

This allows UnitCompose to focus on the algorithm DAG and Debug integration instead of inventing a new dataset browser.

The showcase is not the V0 Quickstart because it introduces a larger download, Python preparation tooling, dataset licensing constraints, and more complex coordinate transforms.

### 4.2 Data and license policy

- Do not commit nuScenes data into the UnitCompose repository.
- Provide a preparation or download wrapper that uses the official `nuscenes-devkit` and documented nuScenes mini source.
- Keep code licensing separate from dataset licensing.
- Add a `DATA_LICENSE.md` or equivalent notice before the showcase is released.
- Document that nuScenes data is subject to the nuScenes terms and CC BY-NC-SA licensing conditions applicable to the downloaded dataset.
- Keep a very small generated or synthetic point-cloud fixture for unit tests so CI never depends on external data.

### 4.3 Preparation boundary

V0 should not implement a complete nuScenes parser in Rust.

Use a Python preparation tool based on the official devkit to export a small, explicit frame manifest:

```text
prepared_scene/
  manifest.json
  lidar/
    000000.bin
    000001.bin
  annotations/
    000000.json
  camera/
    ... original or referenced images ...
```

The manifest should contain only what the Rust showcase needs:

- frame timestamp;
- LiDAR file path and point layout;
- ego and sensor transforms;
- ground-truth 3D boxes;
- optional camera image references and intrinsics.

The Python tool is a dataset adapter, not a Python Unit runtime.

### 4.4 Initial Unit DAG

Keep the first real-data pipeline small:

#### `lidar.range_filter/v1`

```text
PointCloud -> PointCloud
```

Filter by configurable XYZ or radial bounds.

#### `lidar.voxel_downsample/v1`

```text
PointCloud -> PointCloud
```

Downsample to a configurable voxel size. Prefer a maintained point-cloud or spatial indexing crate if a suitable one is stable; otherwise implement only the minimal voxel hashing required by the demo.

#### Ground removal implementations

```text
PointCloud -> PointCloud
```

Provide two compatible implementations:

- `lidar.height_threshold_ground_removal/v1`
- `lidar.ransac_ground_removal/v1`

The height-threshold version establishes a minimal baseline. The RANSAC version may use a maintained numerical or geometry crate where practical.

#### `lidar.euclidean_cluster_detector/v1`

```text
PointCloud -> Detections3D
```

Cluster non-ground points and compute axis-aligned or yaw-free bounding boxes for the first version. The purpose is to demonstrate the framework and visualization, not to compete with modern learned detectors.

### 4.5 Resource flow

```text
raw_point_cloud
      |
      v
RangeFilter
      |
filtered_point_cloud
      |
      v
VoxelDownsample
      |
downsampled_point_cloud
      |
      v
GroundRemoval
      |
non_ground_point_cloud
      |
      v
EuclideanClusterDetector
      |
predicted_detections
```

YAML should demonstrate replacement of the two GroundRemoval implementations without changing Resource bindings.

### 4.6 Rerun visualization

Reuse the structure of Rerun's official nuScenes Blueprint where possible:

- world and ego coordinate frames;
- LiDAR point cloud;
- optional radar and camera views;
- ground-truth boxes;
- predicted boxes;
- selected intermediate point clouds;
- UnitCompose DAG;
- per-Unit timing and point-count metrics.

Resource renderers should be registered by semantic type:

| Semantic type | Rerun rendering |
| --- | --- |
| `lidar.PointCloud/v1` | `Points3D` with configured color policy |
| `perception.Detections3D/v1` | `Boxes3D` plus annotation context |
| `geometry.Transform3D/v1` | `Transform3D` |
| graph metadata | graph archetypes or text/DOT fallback |

Intermediate Resource rendering must be opt-in through Debug configuration because logging every large point cloud can dominate execution time and storage.

## 5. Debug adapter design

The demos should drive a general adapter boundary rather than embed visualization calls in Units.

Conceptual interfaces:

```rust
trait DebugSink {
    fn module_built(&mut self, graph: &DebugGraph);
    fn run_started(&mut self, event: &RunEvent);
    fn unit_finished(&mut self, event: &UnitFinishedEvent);
    fn run_finished(&mut self, event: &RunFinishedEvent);
}

trait ResourceDebugRenderer {
    fn semantic_type(&self) -> ResourceTypeName;
    fn log(&self, path: &DebugPath, value: &dyn ResourceValue) -> Result<(), DebugError>;
}
```

The concrete API may differ, but preserve these boundaries:

- Unit code has no required Rerun dependency;
- core execution continues when Debug is disabled;
- Debug failures do not silently change algorithm results;
- Resource rendering is type-specific and opt-in;
- expensive intermediate logging is configurable;
- the Rerun SDK version is pinned in the adapter crate.

Recommended crate split:

```text
crates/
  unit-compose-core/
  unit-compose-yaml/
  unit-compose-debug-rerun/
```

## 6. Proposed repository layout

```text
examples/
  navigation_planning/
    README.md
    configs/
      astar.yaml
      dijkstra.yaml
      astar_without_smoothing.yaml
    assets/
      nav2/
        tb3_sandbox.yaml
        tb3_sandbox.pgm
        NOTICE
    src/
      main.rs
      resources.rs
      map_decoder.rs
      inflation.rs
      planners.rs
      smoother.rs

showcases/
  nuscenes_lidar/
    README.md
    configs/
      height_ground.yaml
      ransac_ground.yaml
    tools/
      prepare_nuscenes.py
    src/
      main.rs
      resources.rs
      range_filter.rs
      voxel_downsample.rs
      ground_removal.rs
      clustering.rs
```

The final repository may use a Cargo workspace or another organization, but Quickstart code, optional showcase code, and core crates should remain clearly separated.

## 7. Implementation milestones

### Milestone 1 — Core definition and validation

Implement enough V0 core to:

- register Unit descriptors and factories;
- parse the V0 YAML schema;
- derive Resources and dependencies;
- validate ports, semantic types, producers, outputs, and cycles;
- produce a stable topological order;
- export a textual graph description.

Evidence:

- parser and validation tests;
- source-order permutation tests;
- negative YAML fixtures.

### Milestone 2 — Sequential execution

Implement:

- run-local Resource value storage;
- typed input/output adaptation;
- sequential Unit invocation;
- complete output-set validation;
- stop-on-first-error behavior;
- structured timing events.

Evidence:

- small synthetic Units;
- fan-out and fan-in tests;
- failure-order tests;
- independent Unit tests.

### Milestone 3 — Navigation Quickstart

Implement the four navigation Units and three YAML variants.

Evidence:

- A* and Dijkstra both produce valid paths;
- no-smoother YAML changes the compiled graph;
- path never enters inflated obstacles;
- all Quickstart tests run without Rerun or ROS.

### Milestone 4 — Rerun Debug adapter

Implement:

- graph logging;
- timing and metric logging;
- OccupancyGrid, CostMap, and PathPlan renderers;
- fixed navigation Blueprint;
- live, headless, and `.rrd` output modes.

Evidence:

- one checked-in screenshot or generated recording reference;
- Debug-disabled result equivalence;
- adapter failure behavior test.

### Milestone 5 — Host embedding example

Add a thin host wrapper. Prefer a plain Rust loop first; add ROS 2 only as an optional integration after the core example is stable.

Evidence:

- the host owns and invokes the Module;
- UnitCompose does not own the host event loop;
- the host can build a new Module and swap it between runs.

### Milestone 6 — nuScenes Showcase

Implement the preparation tool, four LiDAR Units, YAML replacement, and Rerun views.

Evidence:

- one documented nuScenes mini scene runs end to end;
- predicted and ground-truth boxes are both visible;
- height-threshold and RANSAC configurations use the same binary;
- default CI remains independent of the dataset.

## 8. Validation matrix

| Requirement | Navigation Quickstart | nuScenes Showcase |
| --- | ---: | ---: |
| YAML Unit replacement | A* / Dijkstra | two GroundRemoval Units |
| YAML graph change | smoother present/absent | optional intermediate Debug outputs |
| Resource fan-out | CostMap to Planner and Smoother | optional Debug and detector consumers |
| Resource fan-in | Planner and Smoother | detector plus frame metadata where needed |
| Stable sequential order | Required | Required |
| Large Resource handling | Moderate grid | LiDAR point cloud |
| Rerun graph and timing | Required | Required |
| Real public data | Nav2 map | nuScenes mini |
| Default CI | Yes | No |

## 9. Dependency and maintenance rules

- Pin exact or narrow compatible dependency versions during V0 implementation.
- Prefer libraries with active maintenance and permissive licenses.
- Keep optional Rerun and dataset dependencies out of `unit-compose-core`.
- Record third-party assets and licenses in example-local notices.
- Do not vendor model weights or large datasets.
- Do not make network access a requirement for unit tests.
- Reuse public algorithms through libraries when the adapter is smaller than a reimplementation.
- Keep custom algorithms intentionally small and documented as demo-quality when appropriate.

## 10. Risks and mitigations

### Rerun API evolution

**Risk:** Rerun's SDK and graph/blueprint APIs may change.

**Mitigation:** isolate all calls in `unit-compose-debug-rerun`, pin a version, and keep core Debug events independent of Rerun types.

### Visualization overhead

**Risk:** logging large intermediate Resources changes performance substantially.

**Mitigation:** make Resource rendering opt-in, sample or summarize expensive values, and report Debug overhead separately.

### Demo becoming a framework of its own

**Risk:** ROS, dataset, CLI, and visualization integration overwhelm the UnitCompose concepts.

**Mitigation:** keep the navigation Quickstart as the normative example; keep ROS and nuScenes optional adapters/showcases.

### Algorithm-quality expectations

**Risk:** a simple LiDAR clustering detector is mistaken for a production detector.

**Mitigation:** describe it as a framework showcase, compare to ground truth only for visualization, and avoid accuracy claims.

### Dataset licensing

**Risk:** users assume code and data share the MIT license.

**Mitigation:** do not redistribute nuScenes data, add explicit data-license documentation, and keep the preparation step user-triggered.

## 11. Definition of done

The demo work is complete when:

- a new contributor can run the navigation Quickstart from the repository instructions;
- the same compiled binary runs all three navigation YAML variants;
- graph and Resource validation errors are actionable;
- A* and Dijkstra are exchanged only through YAML;
- removing the smoother changes the DAG without source changes;
- Rerun displays the map, paths, Unit/Resource graph, and Unit timing;
- headless tests cover the same algorithm outputs;
- core crates do not depend on Rerun, ROS, Python, or nuScenes;
- third-party asset notices are present;
- the nuScenes Showcase has a documented preparation and run path, without becoming a default CI dependency.
