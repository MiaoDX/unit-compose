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
- Resources are immutable after successful publication.
- Intermediate Resources are run-local.

### Semantic and concrete types

A stable serialized Resource semantic type is separate from Rust `TypeId`. Within one Unit Registry, a semantic type resolves to one concrete Rust representation and storage adapter. Construction rejects semantic or concrete representation mismatches.

### Validation

Before Unit business execution, Module construction rejects unsupported schema versions, duplicate names or producers, unknown Unit types, invalid configuration, unknown or missing ports, unknown Resources, type mismatches, unresolved storage requirements, and cycles.

Diagnostics identify the Module, Unit instance and type, port, Resource, semantic type, and source path when available.

### Stable sequential execution

V0 executes each Unit at most once in a stable topological order. When multiple Units are ready and unordered, canonical Unit identity determines the tie-break, not YAML source order.

A Unit publishes its complete output set only after it returns success and all outputs pass validation. The first Unit error stops later launches.

### State, failure, and reload

Unit instances may retain private state across runs. The framework does not roll back that state or external effects.

A Unit error is either recoverable or fatal. Recoverable means the Unit explicitly guarantees that another run is valid. Fatal is the default and makes the Module reject further runs.

The Module graph is immutable. Reconfiguration builds and prepares a new Module and swaps it between runs. Failed construction leaves the current Module available.

### Debug

Debug is read-only. It exposes graph structure, bindings, execution order, diagnostics, timing, failures, and storage information without granting Unit code undeclared Resource access.

## Consequences

### Benefits

- The same binary can load multiple algorithm compositions.
- Compatible Unit implementations can be exchanged through configuration.
- Static validation provides value before optimization or parallelism.
- Fan-out and fan-in express general DAGs without hidden dependencies.
- Stable execution and identities support reproducible tests and diagnostics.
- The model leaves room for future executors without making concurrency a V0 promise.

### Costs

- New Unit implementations require a new binary.
- Sequential V0 does not exploit independent graph branches.
- Unit private state and external effects remain outside rollback guarantees.
- Required ports favor explicit result types over dynamically absent outputs.

## Deferred

V0 does not guarantee automatic parallel or asynchronous execution, managed persistent Resources, transactions, dynamic native plugins, Python Unit authoring, in-place graph mutation, checkpointing, replay, or distributed execution.
