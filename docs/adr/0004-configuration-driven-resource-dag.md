# ADR-0004: Configuration-driven Resource DAG for V0

- **Status:** Accepted
- **Date:** 2026-08-03
- **Supersedes for V0:** ADR-0003 Alpha execution model

## Context

The initial design attempted to specify transactional Resource publication, Run-level atomic commit, rollback boundaries, persistent state, leases, storage reuse, parallel scheduler equivalence, Python views, and device synchronization before an executable framework existed.

Those topics are individually valuable, but together they obscure the first product problem:

> Help developers split one algorithm or functional module into clear Units with explicit data contracts, assemble those Units through configuration, inspect the resulting DAG, and embed the Module inside an existing host.

Representative workloads include perception and planning modules inside ROS nodes, services, simulators, and offline tools. The host should be compiled once while YAML selects among registered algorithm implementations and compositions.

The project needs a smaller implementation slice that is useful on its own and keeps durable concepts available for later growth.

## Decision

### Public model

V0 keeps four durable public concepts:

1. **Unit** — one typed computation step;
2. **Resource** — one named, typed value connecting producers and consumers;
3. **Module** — one validated, instantiated static Resource DAG;
4. **Debug** — one read-only inspection and observability surface.

Plan compilation, scheduling, registries, and value storage remain implementation mechanisms.

### Configuration-driven assembly

- Unit implementations are compiled into a binary and registered by stable Unit type name.
- A YAML Module Definition chooses Unit types, supplies instance configuration, binds ports to Resource names, and declares Module inputs and outputs.
- The same binary may construct different Modules from different YAML files.
- V0 does not load previously unknown native implementations from dynamic libraries.

### Resource model

- Every Resource has a stable semantic type.
- Every Resource has exactly one producer: a Module input or one Unit output.
- A Resource may have multiple read-only consumers.
- A Unit receives only the Resources bound to its declared input ports.
- A Unit publishes its complete output set only after that Unit returns success.
- Intermediate Resources are run-local in V0.

### Graph and execution

- Dependencies are derived only from Resource producer-consumer relationships.
- YAML source order is not an execution dependency.
- V0 rejects cycles, unknown Unit types, invalid configuration, unknown ports, missing bindings, duplicate producers, and Resource type mismatches before Unit code runs.
- V0 executes Units sequentially in a stable topological order.
- The first Unit error stops further Unit launches and returns a structured execution error.
- V0 does not claim rollback of Unit private state or external effects.

### State and reload

- Unit instances may hold private state across runs.
- A Module's graph is immutable after construction.
- Configuration reload constructs a new Module and swaps it between runs.
- V0 does not migrate private Unit state across reload.

### Debug

Debug is read-only and may expose graph descriptions, DOT or Mermaid export, Unit timing, failures, Resource relationships, and optional type-specific renderers. Debug does not grant Unit code undeclared Resource access.

## Consequences

### Benefits

- The first implementation directly addresses algorithm decomposition and configuration-driven experimentation.
- The public model remains small and durable.
- YAML can replace compatible filters, detectors, planners, or other Units without recompiling the host.
- Static validation provides immediate value before parallel scheduling or memory optimization exists.
- Sequential execution is straightforward to implement, test, and embed.
- Resource fan-out and fan-in naturally express non-linear DAGs.
- Advanced capabilities can later extend Resource or execution policy without replacing the core concepts.

### Costs

- V0 provides no transactional commit or rollback.
- Unit private state is opaque to the framework.
- Reload resets Unit state unless the application performs its own migration.
- Newly implemented Unit types still require a new binary until a plugin system is designed.
- V0 does not exploit graph parallelism automatically.
- Resource storage and zero-copy behavior are implementation details with no generalized guarantee.

## Deferred decisions

A later ADR is required before adding any of the following as guaranteed behavior:

- managed persistent Resources;
- transaction, prepare/commit, or rollback semantics;
- reusable Modules after arbitrary execution failure;
- automatic parallel or asynchronous execution;
- in-place DAG mutation;
- stable native plugin ABI;
- Python Unit authoring;
- generalized storage leases, pooling, or cross-language zero-copy;
- checkpoint, state migration, or recovery.

## Validation

The V0 decision is successful when executable examples demonstrate:

- one binary loading multiple YAML graphs;
- replacement of compatible Unit implementations by configuration;
- typed Resource validation;
- fan-out and fan-in;
- cycle and binding diagnostics;
- stable sequential execution;
- independent Unit tests;
- graph and timing visualization through Debug;
- embedding without UnitCompose owning the host lifecycle.
