# Milestone 6 inspection and reports

Status: implemented, including the later optional Rerun adapter

Milestone 6 has two deliberately separate inspection structures.

`FixedModuleDescription` is an owned, immutable build product. It composes the
normalized `CompiledGraph`, host-provided normalized Unit configuration
summaries, Resource capacity requirements, Unit workspace requirements,
`StorageReport`, and `PreparedModuleDescription`. The latter retains capacity
policy, requirement status, and allocation-domain instrumentation or
certification evidence. Validation notices are fixed at construction. The
description contains no `RunReport`, Unit private state, prepared Resource
contents, timing sample, failure state, observed capacity, or adapter state.

The fixed description's text view includes Module/schema identity, Units,
types, ports through graph bindings, dependencies and execution order,
Resources, configurations, requirements, workspace bytes, storage slots, live
ranges, estimated peak bytes, and allocation-domain evidence. DOT and Mermaid
delegate to the existing stable compiled-graph renderers, so there is one graph
semantic owner. Description construction clones owned build data and creates
summary strings outside `Module::run`; text, DOT, and Mermaid rendering returns
allocated strings and is also outside strict execution. Both costs are named
by `DescriptionOverhead` and in the text output.

`RunReport` remains the bounded mutable per-run structure. Each event records
completion, recoverable/fatal failure, overflow, incomplete output, panic, or
allocation-profile violation; observed capacity; and the wall-clock duration
around the Module's Unit execution. Timing is observational and
platform-dependent, not deterministic. It covers the composite Unit execution
boundary plus declared internal Unit stages. Module timing reports two clock
reads plus two for each declared Unit event. Unit timing writes are inside the
Module elapsed duration; the final Module report write is outside it. All remain
inside the allocation-profile boundary. `RunReportSnapshot` creates an owned bounded
post-run value for adapters. `Module::set_reporting_enabled` disables
framework event writes without changing sink control or the fixed description.

The dependency-light `unit-compose-debug` crate defines the optional adapter
boundary. `InspectionAdapter` receives only `&FixedModuleDescription` and
`&RunReportSnapshot`; it cannot mutate a Module or replace an algorithm result.
`AdapterFailurePolicy::Report` returns a separate `AdapterError` and keeps the
adapter enabled. `AdapterFailurePolicy::Disable` records the separate error,
disables the adapter permanently, and returns `DisabledAfterFailure`. Later
calls report `Disabled`. Adapter descriptors state allocation domains,
execution placement, and overhead.

Strict measured registration rejects `PostRunAllocating` adapters. Such an
adapter may be used only after the measured run or be disabled. An adapter that
participates in the measured boundary must declare `MeasuredBounded` and use a
bounded implementation. `BoundedRunSink<N>` is the provided allocation-free
sink: it retains the first `N` events and deterministically counts every later
event as dropped. Sink calls are part of `Module::run_profiled`, so allocator
probes cover them. Resource rendering remains outside strict runs unless a
future adapter supplies and proves a separately bounded implementation.

The navigation host creates its description from the same compiled definition
and storage planner used by preparation. Inspect it from the workspace root:

```bash
cargo run -p navigation-planning -- --module examples/navigation-planning/astar.yaml --inspect text
cargo run -p navigation-planning -- --module examples/navigation-planning/astar.yaml --inspect dot
cargo run -p navigation-planning -- --module examples/navigation-planning/astar.yaml --inspect mermaid
cargo run -p navigation-planning -- --module examples/navigation-planning/astar.yaml --timed-mermaid
```

The optional `unit-compose-debug-rerun` crate implements the same
`InspectionAdapter` boundary with `PostRunAllocating` execution. It records
native image, line-strip, point, scalar, and series-style archetypes plus a
fixed blueprint. The navigation package keeps the dependency behind its
default-off `rerun` feature. Save a self-contained recording without starting
a viewer, or explicitly spawn an external viewer:

```bash
cargo run -p navigation-planning --features rerun --locked -- --module examples/navigation-planning/astar.yaml --rerun-save navigation-astar.rrd
cargo run -p navigation-planning --features rerun --locked -- --module examples/navigation-planning/astar.yaml --rerun-spawn
```

Each Rerun route records one navigation frame and ten bounded strict-run timing
samples. Ordinary strict and inspection routes retain their single-run or
non-executing behavior.

Both routes execute and measure the strict Module first. The ROS occupancy map,
amber binary clearance mask, raw path, optional smoothed path, per-Unit and run
timing, and capacity metrics are borrowed or converted only afterward. The
clearance mask has binary pass/block semantics; it is not presented as a graded
Nav2 cost field. Mermaid is the canonical fixed graph view; a timed Mermaid
projection summarizes 100 bounded strict-run snapshots as average and
nearest-rank p99 wall-clock duration per Unit. Rerun retains individual samples
for timeline analysis.
`--rerun-spawn` requires a compatible `rerun` executable on `PATH`; file output
does not require a viewer.

The integration suite proves algorithm results are identical with reporting
enabled, reporting disabled, and the bounded sink; profiled bounded-sink runs
remain allocation-free; adapter failure leaves results and Module state
runnable; fixed descriptions and all renderers are identical after success and
failure; storage and timing identify their overhead; and dropped-event counts
are deterministic. Focused Rerun tests additionally prove the adapter contract,
fixed file route, snapshot semantics, and nonempty `.rrd` output.
