# Concept overview

UnitCompose organizes the internal implementation of one host-level algorithm or functional component.

```text
compiled Unit implementations       YAML Module Definition
              \                         /
               \                       /
                validate, compile, prepare
                           |
                           v
                         Module
              /------------|-------------\
           Units        Resources        Debug
             |              |
             |       prepared storage
             |       and workspaces
             +--------------+
                           |
                           v
                stable sequential runs
```

A ROS node, service, simulator, command-line tool, or another host owns the Module. The host supplies inputs, invokes the Module, consumes outputs, and decides when a newly built Module should replace the current one.

## Unit

A **Unit** is the smallest computation step that is independently understandable, configurable, testable, replaceable, or useful to inspect.

A Unit type declares before construction:

- a stable implementation type name;
- a configuration decoder;
- named required input ports and their Resource semantic types;
- named output ports and their Resource semantic types;
- output storage requirements;
- scratch workspace requirements;
- a factory for creating the Unit instance;
- whether it supports strict steady-state no-allocation execution.

A Unit instance may keep private state such as tracker history, prepared lookup tables, a pre-sized heap, or a model handle. The framework does not inspect, migrate, or roll back that private state.

During execution, a Unit receives read-only input views, writable output handles, and a bounded workspace. It should not discover undeclared Resources or allocate output payloads behind the framework.

## Resource

A **Resource** is a named, typed logical value in a Module.

A Resource is produced by exactly one Module input or Unit output and may be consumed by any number of Unit inputs. It becomes read-only after successful publication by its producer.

Logical Resource identity is separate from physical storage. Two Resources remain distinct even when the implementation safely reuses one compatible storage slot because their live ranges do not overlap. Conversely, one Resource may be represented by host-owned, framework-owned, shared, or adapter-provided storage without changing its semantic identity.

Intermediate Resources are run-local. V0 does not manage persistent Resource state across runs; stateful algorithms use Unit private state.

## Module

A **Module** is a validated, prepared, immutable Resource DAG.

Module construction performs:

1. parse and schema validation;
2. Unit type and configuration resolution;
3. Resource producer, consumer, semantic type, and concrete representation validation;
4. stable dependency compilation;
5. capacity and workspace requirement resolution;
6. storage and workspace planning;
7. allocation, Unit construction, and optional warm-up.

The DAG does not change during a Module instance's lifetime. A different definition produces a different Module.

Preparation is a lifecycle stage, not a fifth public concept. Ordinary users may call one `build` API that performs compilation and preparation internally. Advanced tooling may inspect the compiled graph and storage report before allocation.

## Managed storage

UnitCompose manages two different categories of runtime memory:

- **Resource output storage** survives from a producer's successful completion until the last consumer or Module-output borrower releases it.
- **Scratch workspace** is temporary memory used only during one Unit invocation.

Resource storage is not a stack because Resource live ranges can overlap and are not necessarily LIFO. Scratch workspace commonly is stack-like and may use a caller-provided workspace implementation.

V0 prioritizes typed storage:

- one prepared value slot for a fixed value;
- a fixed-size typed buffer;
- a bounded variable-length typed buffer with a separate logical length.

The first storage planner may reuse only slots with compatible representation, element type, alignment, capacity, memory class, and non-overlapping live ranges. Cross-type raw byte packing is deferred.

## Allocation modes

The default managed path may grow a framework-owned buffer during development and report the observed peak. Production configurations can reject growth. An optional strict guarantee additionally requires that, after build and warm-up, `Module::run` performs no dynamic allocator operations in every declared allocation domain used by the framework, participating Units, Debug sinks, and their called libraries.

Strict execution requires:

- fixed or bounded input, output, and workspace requirements;
- no capacity growth;
- allocation-safe Unit implementations, Resource reset/drop behavior, and third-party calls;
- bounded or disabled Debug recording;
- borrowed Module outputs or host-provided output storage;
- allocator instrumentation for every declared allocation domain.

Capacity overflow is a structured run error. Strict mode never silently reallocates.

## Module outputs

The allocation-friendly result is a borrowed Module output view whose lifetime prevents the Module from starting another run while the output is retained. A `run_into` API may write into host-provided output storage.

A convenience API may return owned outputs by cloning or allocating. Such an API is explicitly outside the strict no-run-allocation path.

## Debug

**Debug** is the read-only inspection surface of a Module. It should expose:

- Units, ports, Resources, producers, and consumers;
- stable execution order;
- output and workspace requirements;
- storage-slot assignments and estimated peak memory;
- validation diagnostics;
- Unit timing, failure, and capacity events;
- optional type-specific Resource rendering.

Unit code does not call visualization systems directly. Rerun and other integrations belong in optional adapters.

## Execution

One run:

1. validates supplied inputs and their bounds;
2. resets prepared run-local slots and bounded Debug state;
3. executes each Unit once in stable topological order;
4. validates and publishes each Unit's complete output set;
5. stops at the first error;
6. returns borrowed or host-provided Module outputs on success.

No downstream Unit can observe incomplete output from a failed producer. Unit private state and external effects are not rolled back.

Expected algorithm outcomes such as “no path” or “no detection” should normally be represented in Resource values rather than framework execution errors.

## Reload

Configuration changes use build-new-and-swap:

1. compile and prepare a new Module beside the current one;
2. retain the current Module if the new build fails;
3. swap only between runs;
4. destroy the old Module after outstanding output borrows are gone.

V0 does not mutate an active graph or migrate Unit private state.
