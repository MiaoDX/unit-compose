# Concept overview

UnitCompose organizes the internal implementation of one host-level algorithm or functional component.

```text
compiled Unit implementations       YAML Module Definition
              \                         /
               \                       /
                validate, resolve, compile, prepare
                           |
                           v
                         Module
              /---------------------------\
           Units                       Resources
             |                            |
             |                    prepared storage
             |                    and workspaces
             +----------------------------+
                           |
                           v
                stable sequential runs
                           |
                           v
                inspection and run reports
```

A ROS node, service, simulator, command-line tool, or another host owns the Module. The host supplies inputs, invokes the Module, consumes outputs, and decides when a newly built Module should replace the current one.

## Unit

A **Unit** is the smallest computation step that is independently understandable, configurable, testable, replaceable, or useful to inspect.

A Unit type declares before construction:

- a stable implementation type name;
- a configuration decoder;
- named required input ports and their Resource semantic types;
- named output ports and their Resource semantic types;
- output size or capacity requirements;
- scratch workspace requirements;
- a factory for creating the Unit instance;
- whether it supports strict steady-state no-allocation execution.

Representation invariants such as concrete Rust type, element layout, storage adapter, initialization, reset, and drop behavior belong to the Resource type descriptor rather than being repeated by each producing Unit.

A Unit instance may keep private state such as tracker history, prepared lookup tables, a pre-sized heap, or a model handle. The framework does not inspect, migrate, or roll back that private state.

During execution, a Unit receives read-only input views, writable pending-output handles, and a bounded workspace. It should not discover undeclared Resources or allocate output payloads behind the framework.

## Resource

A **Resource** is a named, typed logical value in a Module.

A Resource has exactly one producer, either a Module input or a Unit output, and may be consumed by any number of Unit inputs.

For each run, a Resource value is write-once. After its producer succeeds and the framework validates and publishes the complete output set, that value is read-only for the remainder of the run. A later run resets run-local state and produces a new value for the same logical Resource.

Logical Resource identity is separate from physical storage. Two Resources remain distinct even when the implementation safely reuses one compatible storage slot because their live ranges do not overlap. Conversely, one Resource may be represented by host-owned, framework-owned, shared, or adapter-provided storage without changing its semantic identity.

Intermediate Resources are run-local. V0 does not manage persistent Resource state across runs; stateful algorithms use Unit private state.

## Module

A **Module** is a validated and prepared Resource DAG owned by a host.

Its compiled structure is fixed for the Module lifetime:

- normalized configuration;
- Unit and Resource identities;
- port bindings and dependencies;
- stable execution order;
- resolved requirements and storage plan.

Its runtime state is intentionally mutable:

- Unit private state;
- prepared Resource and workspace contents;
- per-run publication and failure state;
- bounded timing, capacity, and diagnostic records.

A useful internal decomposition is a fixed compiled description plus mutable runtime state. This is an implementation boundary, not an additional public concept.

Module construction performs:

1. parse and schema validation;
2. Unit type, Resource type, and configuration resolution into a validated intermediate representation;
3. Resource producer, consumer, semantic type, and concrete representation validation;
4. stable dependency compilation;
5. capacity and workspace requirement resolution;
6. storage and workspace planning;
7. allocation, Unit construction, and optional warm-up.

The DAG and storage plan do not change during a Module instance's lifetime. A different definition produces a different Module.

Preparation is a lifecycle stage, not a fourth public concept. Ordinary users may call one `build` API that performs resolution, compilation, and preparation internally. Advanced tooling may inspect the compiled graph and storage report before allocation.

## Managed storage

UnitCompose manages two different categories of runtime memory:

- **Resource output storage** survives from a producer's successful completion until the last consumer or Module-output borrower releases it;
- **Scratch workspace** is temporary memory used only during one Unit invocation.

Resource storage is not a stack because Resource live ranges can overlap and are not necessarily LIFO. Scratch workspace commonly is stack-like and may use a caller-provided workspace implementation.

V0 prioritizes typed storage:

- one prepared value slot for a fixed value;
- a fixed-size typed buffer;
- a bounded variable-length typed buffer with a separate logical length.

A Resource type descriptor defines the representation invariants required to allocate, initialize, reset, validate, and drop that storage. A Unit output requirement supplies only the fixed size, upper bound, or dynamic capacity policy derived from validated configuration and input bounds.

The first storage planner may reuse only slots with compatible representation, element type, alignment, capacity, memory class, initialization and drop behavior, and non-overlapping live ranges. Cross-type raw byte packing is deferred.

## Output publication

A Unit does not publish individual outputs directly. The framework creates a pending output set for one Unit invocation:

1. typed writers track initialized values and logical lengths;
2. the Unit writes all declared outputs or returns an error;
3. the framework validates representation, initialization, lengths, and capacities for the complete set;
4. the framework publishes the set as one group;
5. on Unit error, validation error, or unwind panic, all initialized but unpublished values are dropped safely.

This publication boundary is limited to Resource outputs. It does not roll back Unit private state or external effects.

With `panic=abort`, the process terminates without returning through the executor, so UnitCompose cannot guarantee pending-output cleanup or Module poisoning.

## Allocation modes

The default managed path may grow a framework-owned buffer during development and report the observed peak. Production configurations can reject growth. An optional strict guarantee additionally requires that, after build and warm-up, `Module::run` performs no dynamic allocator operations in every declared allocation domain used by the framework, participating Units, diagnostic sinks, and their called libraries.

Strict execution requires:

- fixed or bounded input, output, and workspace requirements;
- no capacity growth;
- allocation-safe Unit implementations, Resource reset/drop behavior, and third-party calls;
- bounded or disabled run reporting;
- borrowed Module outputs or host-provided output storage;
- allocator instrumentation or explicit trusted certification for every declared allocation domain.

Capacity overflow is a structured run error. Strict mode never silently reallocates. Build option constructors or named presets should prevent incompatible combinations such as grow-and-measure with a no-run-allocation guarantee.

Declarations, certification sources, and covered domains are inspectable. Instrumentation verifies observed operations, but arbitrary native code can omit an allocator from its declaration; the strict guarantee depends on complete and correct trusted declarations.

## Module outputs

The allocation-friendly result is a borrowed Module output view whose lifetime prevents that Module from starting another run, being destroyed, or reusing its storage while the output is retained. A host may still activate a different prepared Module while keeping the old one alive.

A `run_into` API may write into host-provided output storage. It publishes Module outputs only after the complete set succeeds, but it does not roll back caller memory: after Unit error, validation error, or unwind, caller storage may be partially mutated and is invalid.

A convenience API may return owned outputs by cloning or allocating. Such an API is explicitly outside the strict no-run-allocation path.

## Inspection and diagnostics

Read-only Module capabilities expose two kinds of information:

- **Module description** — Units, ports, Resources, producers, consumers, normalized configuration, execution order, requirements, slot assignments, and estimated peak memory;
- **Run report** — Unit timing, completion, failure, capacity events, allocation-profile results, and optional type-specific Resource renderings.

Text, DOT, Mermaid, Rerun, and other integrations consume these structures through optional renderers or adapters. Unit code does not call visualization systems directly.

## Execution

One run:

1. validates supplied inputs and their bounds;
2. resets prepared run-local slots and bounded report state;
3. executes each Unit once in stable topological order;
4. validates and publishes each Unit's pending output set;
5. stops at the first error;
6. returns borrowed or host-provided Module outputs on success.

No downstream Unit can observe incomplete output from a failed producer. Unit private state and external effects are not rolled back.

When panic unwinding is enabled, a Unit panic stops the run, drops pending outputs, fatally poisons the Module, and becomes a structured fatal run error. With `panic=abort`, the process terminates and no cleanup or poisoning guarantee applies.

Expected algorithm outcomes such as “no path” or “no detection” should normally be represented in Resource values rather than framework execution errors.

## Reload

Configuration changes use a host-owned build-new-and-swap pattern; UnitCompose does not own reload lifecycle:

1. compile and prepare a new Module beside the current one;
2. retain the current Module if the new build fails;
3. designate the new Module active only between runs;
4. keep the old Module alive and its storage unavailable for mutation or reuse until outstanding output borrows are gone.

Outstanding borrows do not prevent the host from activating the new Module. V0 does not mutate an active graph or migrate Unit private state.
