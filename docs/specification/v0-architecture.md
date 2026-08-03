# UnitCompose V0 architecture specification

- **Status:** Accepted V0 baseline
- **Date:** 2026-08-03
- **Depends on:** [ADR-0004](../adr/0004-configuration-driven-resource-dag.md)

This document defines the smallest implementation contract for UnitCompose V0. It deliberately excludes transactional Resource semantics, managed persistent state, automatic parallel execution, dynamic native plugins, and generalized memory-lifetime optimization.

## 1. Product boundary

UnitCompose organizes the implementation inside one host-level algorithm or functional component.

A host application:

- creates a Unit Registry;
- loads a YAML Module Definition;
- constructs and owns a Module;
- supplies inputs and invokes the Module;
- consumes outputs and errors;
- chooses when to replace or destroy the Module.

UnitCompose does not own the process, ROS executor, service loop, simulator clock, communication middleware, or application lifecycle.

## 2. Public concepts

### UC-V0-01 — Unit

A Unit is one scheduled computation step. A Unit type exposes before construction:

- a stable Unit type name;
- a configuration decoder;
- named input ports;
- named output ports;
- one Resource semantic type for each port;
- a factory for creating a Unit instance.

A Unit instance may hold private state. The framework does not inspect or roll back that state in V0.

A Unit should normally represent a step with independent configuration, test value, replacement value, Debug value, or a meaningful fan-out/fan-in boundary. Private helper functions do not need to become Units.

### UC-V0-02 — Resource

A Resource is a Module-local logical value identified by a Resource name and semantic type.

Each Resource has exactly one producer:

- one Module input; or
- one Unit output port.

A Resource may have zero or more Unit input consumers and zero or more Module output aliases. A Resource is immutable after it becomes available to consumers.

### UC-V0-03 — Module

A Module is a validated, instantiated static Resource DAG. It owns Unit instances, internal Resource identities, stable execution order, and Debug state.

The DAG does not change during a Module instance's lifetime.

### UC-V0-04 — Debug

Debug is a read-only Module capability. It may expose graph structure, validation diagnostics, execution events, timing, and optional Resource renderings. Debug does not let Unit code discover or modify undeclared Resources.

## 3. Unit Registry

The host or an integration crate registers all Unit types available to a binary.

Conceptually:

```rust
struct UnitDescriptor {
    type_name: UnitTypeName,
    input_ports: Vec<PortDescriptor>,
    output_ports: Vec<PortDescriptor>,
    config_decoder: ConfigDecoder,
    factory: UnitFactory,
}
```

The concrete Rust API is replaceable, but the following behaviors are required:

- Unit type names are unique within one registry;
- configuration is validated before Unit construction completes;
- descriptors are inspectable without running Unit business logic;
- the registry can report the available Unit contracts;
- Rust `TypeId` is not the serialized semantic type identity.

A type name such as `nav.astar/v1` or `lidar.crop_box_filter/v1` should be stable enough for YAML and diagnostics.

## 4. Module Definition

V0 uses YAML as the primary source format. Equivalent programmatic frontends may be added later, but they must compile to the same logical definition.

Required top-level fields:

```yaml
schema: unit-compose/v0alpha1
module: example
inputs: {}
units: {}
outputs: {}
```

### Module inputs

A Module input declares a Resource name and semantic type:

```yaml
inputs:
  raw_points:
    type: lidar.PointCloud/v1
```

### Unit instances

A Unit instance declares a registered type, optional configuration, input bindings, and output bindings:

```yaml
units:
  filter:
    type: lidar.crop_box_filter/v1
    config:
      min: [-40.0, -20.0, -3.0]
      max: [80.0, 20.0, 4.0]
    inputs:
      points: raw_points
    outputs:
      points: filtered_points
```

An output binding creates or names a Resource. The output port contract determines the Resource semantic type. A consumer binding must match that type.

### Module outputs

A Module output aliases an existing Resource:

```yaml
outputs:
  points: filtered_points
```

### Dependency source

Resource bindings are the only dependency source in V0. YAML list or mapping order does not create a dependency, and V0 has no separate `depends_on` field.

## 5. Resource rules

### UC-V0-05 — One producer

A Resource name must have exactly one producer. Duplicate Module inputs, duplicate Unit outputs, or a Module input and Unit output using the same Resource name are invalid.

### UC-V0-06 — Multiple readers

A Resource may bind to multiple Unit input ports. Consumers receive read-only logical access. Implementations may share ownership, reference-count storage, or copy values, but those choices do not change Resource identity.

### UC-V0-07 — Complete Unit outputs

A Unit execution either returns its complete declared output set or returns an error. Output validation occurs before any downstream Unit executes.

V0 does not define cross-Unit atomicity or rollback. It only prevents downstream use of an incomplete output set from one failed Unit.

### UC-V0-08 — Run-local intermediate values

Module inputs and Unit outputs are run-local values in V0. The value store may release them after the run when no returned output or Debug adapter retains ownership.

Persistent framework-managed Resource lifetimes are deferred. Unit private state is the V0 mechanism for stateful algorithms such as trackers.

## 6. Module construction and validation

Construction must reject the definition before Unit business execution when it contains:

- an unsupported schema identifier;
- a duplicate Unit instance name;
- an unknown Unit type;
- invalid Unit configuration;
- an unknown input or output port;
- a missing required input binding;
- an output binding for an undeclared port;
- a Resource with zero or multiple producers;
- a Resource semantic type mismatch;
- a Module output referencing an unknown Resource;
- a dependency cycle.

Construction should diagnose, without necessarily rejecting:

- a Resource with no consumer and no Module output;
- a Unit that does not contribute to a Module output;
- an optional output that is never used;
- a graph with multiple independent roots or components.

Diagnostics should identify the Module, Unit instance, Unit type, port, Resource, semantic type, and YAML path involved.

## 7. Stable execution

### UC-V0-09 — Topological order

After validation, the implementation computes a topological order from Resource dependencies. When several Units are ready and unordered, V0 uses a stable tie-break derived from canonical Unit instance identity rather than YAML source order.

### UC-V0-10 — Sequential execution

V0 executes Units sequentially. A run conceptually performs:

```text
validate inputs
    |
create run-local value store
    |
execute Units in stable topological order
    |
collect declared Module outputs
    |
return
```

A Unit executes at most once in one run.

### UC-V0-11 — Failure

On the first Unit error:

- no further Unit is launched;
- the run returns a structured error naming the failing Unit;
- values already produced during the run are discarded unless retained only for Debug;
- Unit private state and external effects are not rolled back;
- the framework makes no automatic retry decision.

V0 does not require a poisoned Module state. A Unit or Module implementation may choose to reject a later run when its own state is no longer usable, but this is not a universal transaction guarantee.

## 8. Module inputs and outputs

Before the first Unit runs, the supplied input set must be checked for:

- all required Module inputs present;
- no unknown input names, unless an API explicitly permits them;
- semantic and concrete runtime type compatibility.

A successful run returns only declared Module outputs. Intermediate Resource values are not implicit business outputs.

For debugging, users may expose an intermediate Resource explicitly as an additional Module output in a separate YAML definition.

## 9. State and reload

### UC-V0-12 — Unit private state

Unit instances may retain private state between runs. Examples include tracker history, caches, model handles, and prepared lookup tables.

V0 does not provide:

- framework snapshots of Unit state;
- rollback after failure;
- state migration between Unit implementations;
- automatic serialization or checkpointing.

### UC-V0-13 — Immutable graph

A Module's Unit and Resource graph is immutable after construction.

### UC-V0-14 — Build-new-and-swap reload

A host may reconfigure behavior without recompiling by:

1. loading another YAML definition;
2. constructing and validating a new Module;
3. swapping to it between runs after construction succeeds;
4. shutting down the old Module.

The old Module remains available if new construction fails. Private Unit state is reset unless the host or Unit implementation explicitly performs migration outside the V0 framework contract.

## 10. Debug contract

V0 Debug should provide structured access to:

- Module name and schema version;
- Unit instances, registered types, and normalized configuration summaries;
- input and output ports;
- Resource names, semantic types, producers, and consumers;
- derived Unit dependencies;
- stable execution order;
- validation warnings and errors;
- per-run Unit start, completion, duration, and failure events.

Recommended renderers include:

- textual description;
- DOT;
- Mermaid;
- a Rerun Debug sink for graph, timing, and selected Resource values.

Resource renderers are registered by semantic type or adapter. Unit implementations must not call Rerun directly as a requirement of the framework.

## 11. Error model

V0 requires two top-level error classes:

### Module build error

Contains the validation or construction cause and source location before a usable Module is returned.

### Module run error

Contains at least:

- Module identity;
- Unit instance identity when applicable;
- Unit type;
- original Unit or framework error;
- available Debug trace for the failed run.

The concrete Rust enum hierarchy remains an implementation choice.

## 12. Host embedding

UnitCompose must function as a normal in-process library. A host should be able to own a Module directly:

```rust
struct RadarNode {
    module: Module,
}

impl RadarNode {
    fn on_frame(&mut self, input: RadarInput) -> Result<RadarOutput, HostError> {
        self.module.run(input).map_err(HostError::from)
    }
}
```

Core crates must not depend on ROS, Rerun, a service framework, or a simulator. Integrations belong in adapter crates or examples.

## 13. V0 non-goals

V0 does not guarantee:

- Run-level atomic commit;
- rollback of Unit state or external effects;
- managed persistent Resources;
- writable shared Resources;
- automatic parallel or asynchronous execution;
- dynamic native library loading or a stable plugin ABI;
- Python Unit authoring;
- generalized zero-copy, leases, or storage pooling;
- checkpoint, replay, or recovery;
- in-place graph mutation;
- distributed execution.

## 14. Acceptance evidence

V0 is complete when executable tests and examples demonstrate:

1. one binary loads at least three distinct YAML Module Definitions;
2. two registered Unit implementations with the same contract can be exchanged through YAML;
3. a Module Definition can add or remove a Unit and change the DAG without source changes;
4. fan-out and fan-in execute correctly;
5. type mismatch, missing binding, duplicate producer, unknown Unit, and cycle errors identify the relevant YAML path;
6. YAML declaration order does not change the compiled graph or execution order;
7. Unit errors stop later launches and identify the failing Unit;
8. Units can be tested without constructing a host application;
9. Debug exports the graph and Unit timing;
10. a Module is embedded in at least one host-style example without UnitCompose owning that host lifecycle.

## 15. Compatibility direction

Future versions should extend, rather than replace, the V0 model:

- parallel execution can change the internal executor while preserving Resource dependencies;
- persistent state can add Resource lifetime metadata;
- plugins can extend the registry;
- Python can provide additional Unit factories;
- storage adapters can optimize Resource representation;
- richer reliability can add explicit state or effect contracts.

Each such capability requires a representative workload and a separate ADR before it becomes guaranteed behavior.
