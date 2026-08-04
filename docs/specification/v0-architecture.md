# UnitCompose V0 architecture specification

- **Status:** Accepted V0 baseline
- **Date:** 2026-08-04
- **Depends on:** [ADR-0002](../adr/0002-configuration-driven-resource-dag.md), [ADR-0003](../adr/0003-framework-managed-resource-storage.md)

This document defines the observable implementation contract for UnitCompose V0.

## 1. Product boundary

UnitCompose organizes implementation inside one host-owned algorithm or functional component.

The host:

- creates a Unit Registry;
- loads a Module Definition;
- supplies build options and input bounds when required;
- constructs and owns a Module;
- supplies inputs and invokes runs;
- consumes outputs and errors;
- chooses when to replace or destroy the Module.

UnitCompose does not own the process, ROS executor, service loop, simulator clock, communication middleware, or application lifecycle.

## 2. Public concepts

### UC-V0-01 — Unit

A Unit is one scheduled computation step with required typed input ports, typed output ports, validated configuration, declared storage requirements, optional private state, and one execution method.

A Unit normally represents a boundary with independent configuration, test value, replacement value, Debug value, fan-out/fan-in value, or meaningful storage behavior. Private helper functions do not need to become Units.

### UC-V0-02 — Resource

A Resource is a Module-local logical value identified by a name and stable semantic type.

Each Resource has exactly one producer: one Module input or one Unit output. It may have zero or more read-only Unit consumers and zero or more Module-output aliases.

A Resource is immutable after successful publication. Logical identity does not imply one unique allocation.

### UC-V0-03 — Module

A Module is a validated, prepared, immutable Resource DAG. It owns Unit instances, internal identities, stable execution order, prepared storage, workspace backing, and Debug state.

A Module admits at most one active run.

### UC-V0-04 — Debug

Debug is a read-only Module capability exposing structure, diagnostics, execution events, timing, storage reports, capacity information, and optional Resource renderings. Debug never grants Unit code undeclared Resource access.

## 3. Unit Registry

The host or integration crates register all Unit and Resource types available to a binary.

Conceptually:

```rust
struct UnitDescriptor {
    type_name: UnitTypeName,
    input_ports: Vec<PortDescriptor>,
    output_ports: Vec<PortDescriptor>,
    config_decoder: ConfigDecoder,
    requirements: RequirementsFn,
    allocation_capability: AllocationCapability,
    factory: UnitFactory,
}

struct ResourceTypeDescriptor {
    semantic_type: ResourceTypeName,
    concrete_type: TypeId,
    representation: StorageRepresentation,
}
```

The concrete API is replaceable, but these behaviors are required:

- Unit type names are unique within one Registry;
- Resource semantic type names are unique within one Registry;
- one semantic type resolves to one concrete Rust representation;
- multiple semantic types may intentionally use the same Rust representation;
- Rust `TypeId` is never the serialized semantic identity;
- descriptors are inspectable before Unit business execution;
- all configuration decoders and requirement functions run before a usable Module is returned.

Stable identities use names such as `nav.astar/v1` and `lidar.PointCloud/v1`. A breaking port, configuration, semantic, or representation change requires a new versioned identity.

## 4. Module Definition

YAML is the primary V0 source format. Programmatic frontends may compile to the same logical definition.

Required fields:

```yaml
schema: unit-compose/v0alpha1
module: example
inputs: {}
units: {}
outputs: {}
```

A Module input declares a Resource semantic type:

```yaml
inputs:
  raw_points:
    type: lidar.PointCloud/v1
```

A Unit instance selects a type, provides optional configuration, and binds every required port:

```yaml
units:
  filter:
    type: lidar.crop_box_filter/v1
    config:
      min: [-40.0, -20.0, -3.0]
      max: [80.0, 20.0, 4.0]
      max_output_points: 200000
    inputs:
      points: raw_points
    outputs:
      points: filtered_points
```

A Module output aliases an existing Resource:

```yaml
outputs:
  points: filtered_points
```

Resource bindings are the only dependency source. YAML source order never creates a dependency. V0 has no `depends_on` field.

V0 does not prescribe one generic YAML shape language. Bounds may come from Unit configuration, Resource type adapters, Module-input declarations supported by an adapter, or host build options. A strict build fails if required bounds remain unresolved.

## 5. Port and Resource rules

### UC-V0-05 — Required ports

All V0 input and output ports are required. An optional business result uses an explicit Resource value such as `Option<T>` or a domain result enum.

### UC-V0-06 — One producer

A Resource name has exactly one producer. Duplicate Module inputs, duplicate Unit outputs, or a Module input and Unit output using the same Resource name are invalid.

### UC-V0-07 — Multiple readers

A Resource may bind to multiple Unit inputs. Consumers receive read-only logical access.

### UC-V0-08 — Complete output publication

A Unit execution either successfully initializes its complete declared output set or returns an error. Output representation, length, capacity, and initialization state are validated before any downstream Unit executes.

Partial output is never published.

### UC-V0-09 — Run-local intermediates

Module inputs and Unit outputs are run-local. V0 does not manage persistent Resource state between runs. Stateful algorithms use Unit private state.

## 6. Storage and workspace requirements

Every Unit output resolves before execution to a storage requirement containing enough information to prepare compatible physical storage. The concrete representation may vary, but requirements distinguish:

- fixed value or fixed-size buffer;
- bounded variable-length buffer;
- dynamic storage that may grow.

Each Unit also resolves a scratch workspace requirement. Workspace is temporary to one Unit invocation and is not a Resource.

Requirements may depend only on validated configuration, registered Resource descriptors, supplied input bounds, and other deterministic build information. Requirement calculation must not execute Unit business logic.

Strict no-run-allocation accepts only fixed or bounded requirements.

## 7. Module build and preparation

Module construction performs, in order:

1. parse YAML and retain source locations;
2. validate schema and duplicate keys;
3. normalize names and configuration;
4. resolve Unit and Resource type descriptors;
5. validate required ports and bindings;
6. derive producers, consumers, dependencies, and cycles;
7. compute stable topological order;
8. resolve input bounds, output requirements, and workspace requirements;
9. compute Resource live ranges;
10. assign compatible Resource slots and Unit workspaces;
11. allocate prepared backing storage;
12. construct Unit instances;
13. perform optional documented warm-up;
14. return an immutable ready Module and storage report.

All known definition, configuration, graph, type, bound, and planning errors occur before Unit business execution.

If Unit construction or warm-up fails, already constructed Units and initialized storage are dropped safely and no Module is returned.

## 8. Storage planning

### UC-V0-10 — Identity separation

Storage assignment never changes Resource identity, semantic type, producer, consumer, or observable value.

### UC-V0-11 — Conservative V0 reuse

V0 may reuse one slot for multiple Resources only when their live ranges do not overlap and their concrete representation, element type, alignment, capacity, memory class, initialization, and drop requirements are compatible.

Cross-type raw packing is not required.

### UC-V0-12 — Capacity behavior

Build options select a capacity policy:

- **grow and measure** may grow framework-owned storage and records observed peaks;
- **reject overflow** returns a structured capacity error and never grows prepared storage.

A convenience implementation may expose named development and production presets, but the above behavior is normative.

### UC-V0-13 — Allocation guarantee

Build options select an allocation guarantee:

- **best effort** makes no universal claim about Unit or third-party allocator use;
- **no run allocation** guarantees that steady-state `Module::run` invokes no dynamic allocator allocate, reallocate, or deallocate operation in every allocation domain declared by the prepared Module.

The guarantee covers framework code, Unit code, registered Debug sinks, Resource reset and drop paths, and called third-party code within the run boundary. The initial CPU profile must at least instrument the Rust global allocator. Custom native, device, or adapter allocators must be instrumented or explicitly certified; an uninstrumented allocation path makes the Unit or adapter ineligible for the guarantee.

The no-run-allocation guarantee requires reject-overflow capacity behavior, fixed or bounded requirements, strict-capable Units, allocation-free prepared Resource reset/drop behavior, compatible input/output APIs, bounded Debug behavior, and successful allocator-instrumented acceptance tests.

The guarantee begins after successful build and documented warm-up. It excludes host activity outside `Module::run`.

A build rejects incompatible option combinations.

## 9. Unit execution API

The primary conceptual execution interface is:

```rust
trait Unit {
    fn run(
        &mut self,
        inputs: UnitInputs<'_>,
        outputs: UnitOutputs<'_>,
        workspace: UnitWorkspace<'_>,
    ) -> Result<(), UnitFailure>;
}
```

The concrete typed authoring API may use associated types or generated adapters, but it must preserve:

- read-only access to declared inputs only;
- writable access to declared outputs only;
- bounded access to declared workspace only;
- no string-based Resource lookup on the compiled hot path;
- no retained input, output, or workspace borrow after return;
- complete-output publication only after success.

A Unit may use pre-sized private state prepared during construction or warm-up. Such state must not grow during strict runs.

## 10. Stable execution

### UC-V0-14 — Topological order

Dependencies derive from Resource producer-consumer relations. When several Units are ready and unordered, canonical Unit instance identity provides the stable tie-break.

### UC-V0-15 — Sequential reference

V0 executes Units sequentially. Each Unit executes at most once per run.

A run:

```text
validate inputs and bounds
        |
reset prepared run state
        |
execute Units in stable order
        |
validate and publish complete outputs
        |
return Module outputs
```

## 11. Failure

### UC-V0-16 — Stop on first failure

On the first Unit or framework error:

- no later Unit is launched;
- incomplete output from the failing Unit is discarded;
- initialized values are dropped safely;
- the run returns a structured error with Module, Unit, Resource, capacity, and source context when applicable;
- Unit private state and external effects are not rolled back.

### UC-V0-17 — Failure disposition

A Unit failure is recoverable or fatal.

- **Recoverable** means the Unit explicitly guarantees that its private state remains valid for a later run.
- **Fatal** is the default and makes the Module reject later runs.

Input-validation failures before Unit execution leave the Module reusable.

Expected algorithm outcomes such as no path, no match, or no detection should normally be Resource values rather than Unit failures.

## 12. Module inputs and outputs

Before the first Unit runs, the supplied inputs are checked for required names, unknown names, semantic type, concrete representation, shape or capacity bounds, and compatibility with the prepared plan.

The allocation-friendly output API returns views borrowing prepared Module storage. Their lifetime prevents another mutable run while retained.

A host may use `run_into` or an equivalent API to provide output storage.

An owned convenience result may clone or allocate and is outside the no-run-allocation guarantee.

Intermediate Resources are not implicit business outputs. A different Module Definition may expose one explicitly for inspection.

## 13. Unit private state and reload

Unit instances may retain trackers, caches, model handles, pre-sized containers, and prepared tables between runs.

V0 provides no automatic snapshot, rollback, migration, serialization, or checkpointing of Unit private state.

The DAG and storage plan are immutable for a Module instance. Reload builds and prepares a new Module beside the old one, swaps between runs after success, and retains the old Module if the new build fails.

## 14. Debug contract

Debug should provide structured access to:

- Module and schema identity;
- Unit instances, types, ports, and normalized configuration summaries;
- Resource names, types, concrete representations, producers, and consumers;
- stable dependencies and execution order;
- output and workspace requirements;
- storage-slot assignments, live ranges, estimated peak memory, and observed capacity peaks;
- validation warnings and errors;
- per-run Unit timing, completion, failure, and overflow events;
- allocation-guarantee validation results.

Recommended renderers include text, DOT, Mermaid, and optional Rerun adapters.

Strict runs require disabled or bounded event storage. Resource rendering that allocates, retains, or copies payloads is outside strict execution unless explicitly certified.

Debug failure must not silently change a successful algorithm result. An adapter either reports a separate Debug error or disables itself according to configured policy.

## 15. Error classes

V0 requires:

### Module build error

Contains the parse, schema, registry, configuration, graph, type, bounds, requirement, storage-planning, allocation, construction, or warm-up cause and source location when available.

### Module run error

Contains at least:

- Module identity;
- Unit instance and type when applicable;
- Resource and port when applicable;
- structured framework or Unit cause;
- recoverable or fatal disposition;
- available bounded Debug trace.

### Capacity error

Contains the Unit, Resource or workspace identity, required amount, prepared capacity, and active capacity policy.

Concrete Rust enums are implementation choices.

## 16. Host embedding

UnitCompose is a normal in-process library:

```rust
struct RadarNode {
    module: Module,
}

impl RadarNode {
    fn on_frame<'a>(
        &'a mut self,
        input: RadarInputView<'_>,
    ) -> Result<RadarOutputView<'a>, HostError> {
        self.module.run(input).map_err(HostError::from)
    }
}
```

Core crates do not depend on ROS, Rerun, a simulator, dataset SDK, or service framework. Integrations belong in adapters or examples.

## 17. V0 non-goals

V0 does not guarantee:

- Run-level transaction or rollback;
- framework-managed persistent Resources;
- writable shared Resources;
- automatic parallel or asynchronous execution;
- dynamic native plugins or a stable plugin ABI;
- Python Unit authoring;
- generalized external or cross-language zero-copy leases;
- GPU, pinned host, unified, or remote memory planning;
- parallel storage reuse or asynchronous output lifetime;
- globally optimal or cross-type raw memory packing;
- checkpoint, replay, or recovery;
- in-place graph mutation;
- distributed execution.

## 18. Acceptance evidence

V0 is complete when executable tests and examples demonstrate:

1. one binary loads at least three distinct Module Definitions;
2. two compatible Unit implementations are exchanged through configuration;
3. adding or removing a Unit changes the DAG without source changes;
4. fan-out and fan-in execute correctly;
5. YAML order does not change compiled dependencies or stable order;
6. unknown Unit, missing port, duplicate producer, type mismatch, unresolved bound, and cycle errors identify the relevant source path;
7. semantic types map consistently to concrete Rust representations;
8. a Unit writes fixed and bounded outputs through framework-provided storage;
9. a workspace-heavy Unit uses declared caller-provided scratch;
10. complete-output validation prevents partial publication;
11. compatible typed slots are reused only across non-overlapping live ranges;
12. capacity overflow under reject-overflow returns a structured error without growth;
13. after warm-up, at least 1,000 strict runs show zero allocator allocate, reallocate, and deallocate calls in every declared allocation domain;
14. strict tests cover success, recoverable error, overflow, bounded Debug behavior, Resource reset/drop, and rejection of uninstrumented allocation paths;
15. borrowed Module outputs prevent unsafe slot reuse, and `run_into` supports host-owned outputs;
16. fatal Unit failure prevents later runs while recoverable failure permits them;
17. Units are independently testable;
18. Debug exports graph, timing, requirements, and storage plan;
19. a host-style example owns and invokes a Module without UnitCompose owning the host lifecycle.

## 19. Compatibility direction

Future versions may extend the model through separate ADRs:

- parallel execution with explicit effect and storage-lifetime contracts;
- managed persistent Resource lifetimes;
- allocator and memory-class adapters;
- native or language plugin factories;
- state migration and recovery;
- device synchronization and external-buffer leases.

Such capabilities must extend Unit, Resource, Module, and Debug rather than replace them.
