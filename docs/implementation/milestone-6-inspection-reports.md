# Milestone 6 inspection and reports

Status: implemented without the optional Rerun adapter

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
platform-dependent, not deterministic. It currently covers the composite Unit
execution boundary rather than individual internal application stages. Two
monotonic clock reads are inside the measured allocation boundary. The bounded
report write is outside the event's elapsed duration but remains inside the
allocation-profile boundary. `RunReportSnapshot` creates an owned bounded
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
```

The integration suite proves algorithm results are identical with reporting
enabled, reporting disabled, and the bounded sink; profiled bounded-sink runs
remain allocation-free; adapter failure leaves results and Module state
runnable; fixed descriptions and all renderers are identical after success and
failure; storage and timing identify their overhead; and dropped-event counts
are deterministic. The optional Rerun crate is not implemented and does not
gate this milestone.
