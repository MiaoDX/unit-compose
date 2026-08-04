# ADR-0002: Configuration-driven Resource DAG

- **Status:** Accepted
- **Date:** 2026-08-04

## Context

The first executable version must prove that configuration-driven decomposition is useful before adding parallel execution, persistent framework state, dynamic plugins, language bindings, or distributed behavior.

The composition contract must be deterministic, inspectable, and independent of YAML declaration order.

## Decision

### Registered Unit implementations

A binary registers every Unit type it supports under a stable namespaced type name. A Module Definition selects registered types, supplies configuration, binds ports to Resource names, and declares Module inputs and outputs.

Loading a previously unknown native implementation requires a different binary until a future plugin contract is accepted.

### Required typed ports

A Unit type declares all input and output ports before construction. Every V0 port is required.

Optional business results are represented explicitly in a Resource value, such as `Option<T>` or a domain enum, rather than by omitting a port or output at runtime.

### Resource graph

- Every Resource has one producer: a Module input or one Unit output.
- A Resource may have any number of read-only Unit consumers and Module-output aliases.
- Dependencies are derived only from Resource producer-consumer relationships.
- YAML mapping or list order is not an execution dependency.
- For one run, each Resource value is write-once and becomes read-only after successful publication.
- A later run may reset run-local storage and produce a new value for the same logical Resource.
- Intermediate Resources are run-local.

### Semantic and concrete types

A stable serialized Resource semantic type is separate from Rust `TypeId`. Within one Unit Registry, a semantic type resolves to one concrete Rust representation and storage adapter. Construction rejects semantic or concrete representation mismatches.

Representation invariants such as concrete type, element layout, storage adapter, memory class, initialization, reset, validation, and drop behavior belong to the Resource type descriptor. A producing Unit supplies only the output size or capacity requirement derived from validated configuration and input bounds.

### Validation and resolution

Before Unit business execution, Module construction rejects unsupported schema versions, duplicate names or producers, unknown Unit types, invalid configuration, unknown or missing ports, unknown Resources, type mismatches, unresolved storage requirements, and cycles.

Configuration decoding and descriptor resolution produce a validated intermediate representation before graph compilation or storage planning. Later build stages do not operate on YAML values or unvalidated Unit configuration.

Diagnostics identify the Module, Unit instance and type, port, Resource, semantic type, and source path when available.

### Stable sequential execution

V0 executes each Unit at most once in a stable topological order. When multiple Units are ready and unordered, canonical Unit identity determines the tie-break, not YAML source order.

A Unit writes into a pending output set. The framework publishes the complete set only after the Unit returns success and every output passes validation. If execution or validation fails, initialized but unpublished values are discarded safely. The first Unit error stops later launches.

This publication rule applies only to Resource outputs. It does not roll back Unit private state or external effects.

### Module structure and runtime state

A Module has a fixed compiled structure and mutable runtime state.

The fixed structure includes normalized configuration, Unit and Resource identities, bindings, dependencies, stable execution order, resolved requirements, and the storage plan.

The mutable runtime state includes Unit private state, prepared storage contents, per-run publication and failure state, and bounded diagnostics. The DAG and storage plan do not change during a Module instance's lifetime.

### State, failure, and reload

Unit instances may retain private state across runs. The framework does not roll back that state or external effects.

A Unit error is either recoverable or fatal. Recoverable means the Unit explicitly guarantees that another run is valid. Fatal is the default and makes the Module reject further runs.

Reconfiguration builds and prepares a new Module and swaps it between runs. Failed construction leaves the current Module available.

### Inspection and diagnostics

Read-only Module capabilities expose fixed Module descriptions and per-run reports. They cover graph structure, bindings, execution order, diagnostics, timing, failures, and storage information without granting Unit code undeclared Resource access.

## Consequences

### Benefits

- The same binary can load multiple algorithm compositions.
- Compatible Unit implementations can be exchanged through configuration.
- Static validation provides value before optimization or parallelism.
- Fan-out and fan-in express general DAGs without hidden dependencies.
- Stable execution and identities support reproducible tests and diagnostics.
- Per-run write-once semantics avoid ambiguity between logical Resource identity and changing runtime values.
- Separating fixed compiled structure from mutable runtime state makes implementation ownership explicit without adding public concepts.

### Costs

- New Unit implementations require a new binary.
- Sequential V0 does not exploit independent graph branches.
- Unit private state and external effects remain outside rollback guarantees.
- Required ports favor explicit result types over dynamically absent outputs.
- The build pipeline requires a validated intermediate representation between parsing and compilation.

## Deferred

V0 does not guarantee automatic parallel or asynchronous execution, managed persistent Resources, transactions, dynamic native plugins, Python Unit authoring, in-place graph mutation, checkpointing, replay, or distributed execution.
