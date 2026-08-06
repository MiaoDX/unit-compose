# Milestone 5 headless navigation Quickstart

Status: implemented

The `navigation-planning` host loads the same bounded YAML frontend and core
runtime for three Module Definitions:

- `astar.yaml` selects `nav.astar/v1` and line-of-sight smoothing;
- `dijkstra.yaml` replaces only the compatible planner implementation;
- `astar-no-smoothing.yaml` removes the smoother Unit and publishes the raw
  planner path.

All definitions decode a bounded in-memory ROS occupancy-grid representation
and produce a binary inflated cost map. The smoothed variants fan that map out
to the planner and smoother; the no-smoothing variant sends it only to the
planner. The example intentionally has no ROS runtime, service, network, debug,
or visualization dependency.

The product fixture is a fixed 48 x 40 downsample of the Apache-2.0
[TurtleBot3 Navigation2 map](https://github.com/ROBOTIS-GIT/turtlebot3/blob/fc817ce3073af1d6032397c64504134882af5e9a/turtlebot3_navigation2/map/map.pgm).
It preserves free, occupied, and unknown occupancy states without adding a map
loader or costmap dependency. The prepared definitions bound the fixture at
1,920 cells, 1,920 search expansions, and 256 path points.

The host compiles the YAML graph before constructing a composite navigation
Unit. The composite owns fixed-capacity cost-map, distance, parent, visited,
open-set, raw-path, and smoothed-path storage. A* and Dijkstra share a
preallocated standard-library binary heap; Dijkstra uses a zero heuristic.
Stale heap entries are discarded, and the prepared open-set capacity is bounded
by the four-neighbor grid edge count. Path length, open-set entries, and search
expansions retain explicit reject-overflow bounds.

Strict Modules declare the instrumented `rust-global` allocation domain.
Construction and explicit warm-up occur outside the measured boundary. The
host and integration suite run the prepared Module through the shared scoped
allocator probe and reject allocate, reallocate, or deallocate activity.

Reload remains host-owned. `NavigationHost::reload` builds and warms a
candidate beside the active Module and calls `activate` only after both steps
succeed. A construction or warm-up error therefore leaves the active Module
unchanged and runnable. Activation occurs between runs. Borrowed outputs keep
their source Module storage borrowed and prevent its mutation or reuse, while
a separately prepared Module can be activated or run without borrowing the old
Module.

Prepared named inputs bind `occupancy_grid` to its semantic type, concrete Rust
representation, maximum cell count, and plan token. The integration suite
proves missing, unknown, semantic-type-incompatible,
concrete-type-incompatible, and over-capacity sets are rejected before the
ordinary run boundary, after which the Module remains runnable.

The product binary uses the host's checked profiled path, so validation covers
the measured run boundary rather than bypassing the prepared input plan.
Successful checked runs expose bounded decoder, inflation, planner, and
optional smoother execution evidence recorded by the composite Unit inside the
corresponding stage operations. The integration suite proves an exact one-run
delta for all real stages. Smoothed graphs contain four Units and preserve real
`cost_map` fan-out through planner and smoother; the no-smoothing graph contains
exactly three Units. Every definition publishes exactly one path Resource.

Run the product variants from the workspace root:

```bash
cargo run -p navigation-planning -- --module examples/navigation-planning/astar.yaml --strict
cargo run -p navigation-planning -- --module examples/navigation-planning/dijkstra.yaml --strict
cargo run -p navigation-planning -- --module examples/navigation-planning/astar-no-smoothing.yaml --strict
```

Focused proof is provided by:

```bash
cargo test -p navigation-planning --all-targets -- --test-threads=1
```

The tests execute all three source-only variants, assert graph replacement,
restructuring, exact stage counts, single-output publication, and real cost-map
fan-out, exercise deterministic path fixtures,
measure 1,000 post-warm-up runs per variant, cover path/search/input overflow,
prove successful and failed reload behavior, activate a candidate through the
host, and retain an output from the returned old Module while the host's new
active Module runs. The core `Module::run` compile-fail doctest remains the
compile-time proof that a retained view prevents mutation of its source Module.
