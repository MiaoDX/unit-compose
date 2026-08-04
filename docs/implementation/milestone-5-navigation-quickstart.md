# Milestone 5 headless navigation Quickstart

Status: implemented

The `navigation-planning` host loads the same bounded YAML frontend and core
runtime for three Module Definitions:

- `astar.yaml` selects `nav.astar/v1` and line-of-sight smoothing;
- `dijkstra.yaml` replaces only the compatible planner implementation;
- `astar-no-smoothing.yaml` removes the smoother Unit and publishes the raw
  planner path.

All definitions decode a bounded in-memory ROS occupancy-grid representation,
produce a binary inflated cost map, and fan that cost map out to planning and
statistics Units. The example intentionally has no ROS runtime, service,
network, debug, or visualization dependency.

The host compiles the YAML graph before constructing a composite navigation
Unit. The composite owns fixed-capacity cost-map, distance, parent, visited,
raw-path, and smoothed-path storage. A* and Dijkstra use the same prepared
scan-based search storage; Dijkstra uses a zero heuristic. This small local
implementation is the Milestone 5 allowance for strict algorithms whose
maintained library alternatives do not accept host-prepared search storage.
Both path length and search expansions have explicit reject-overflow bounds.

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
Successful checked runs expose bounded decoder, inflation, planner, statistics,
and optional smoother execution evidence; the no-smoothing graph records zero
smoother executions while its two declared `cost_map` consumers still run.

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
restructuring, and cost-map fan-out, exercise deterministic path fixtures,
measure 1,000 post-warm-up runs per variant, cover path/search/input overflow,
prove successful and failed reload behavior, and retain an old borrowed output
while a different prepared Module runs. The core `Module::run` compile-fail
doctest remains the compile-time proof that a retained view prevents mutation
of its source Module.
