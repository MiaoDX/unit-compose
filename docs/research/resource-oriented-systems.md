# Resource-oriented systems

This document examines Bevy ECS, Flecs, and Salsa because they are the closest references for explicit access, scheduling, shared state, and revision identity.

## 1. Bevy ECS

Primary references:

- [Bevy repository](https://github.com/bevyengine/bevy)
- [`bevy_ecs` documentation](https://docs.rs/bevy_ecs/latest/bevy_ecs/)
- [Schedule documentation](https://docs.rs/bevy_ecs/latest/bevy_ecs/schedule/)
- [Access tracking](https://docs.rs/bevy_ecs/latest/bevy_ecs/query/struct.Access.html)

### Relevant ideas

Bevy ECS provides:

- a World that stores components and singleton Resources;
- Systems whose parameters declare read and write access;
- access compatibility and conflict reporting;
- schedules with explicit ordering and stable single-threaded or multi-threaded executors;
- deferred command application at synchronization boundaries;
- change ticks and change detection.

### Similarity to UnitCompose

The strongest overlap is the ability to inspect access before execution and use it to determine safe parallelism. Bevy also demonstrates that one computation API can support single-threaded and multi-threaded execution.

### Important differences

- Bevy's primary model is entity/component storage; UnitCompose has no entity model.
- A Bevy Resource is a type-unique singleton, while UnitCompose may have several named Resources of the same semantic type.
- Bevy systems commonly mutate World state in place. UnitCompose Alpha stages persistent successors and commits them at Run level.
- Deferred commands protect iteration and schedule visibility, but do not provide full Run rollback.
- Bevy's System abstraction and lifecycle are shaped by a game/application loop rather than a host-invoked algorithm Module.

### Reuse assessment

`bevy_ecs` is worth a focused prototype for access analysis and scheduling, but adopting it as the Alpha kernel would also import World, System, entity, change-tick, and deferred-command semantics. The prototype should answer whether reuse is smaller than implementing the required subset directly.

Current recommendation: **semantic reference; experimental dependency, not default Alpha dependency**.

## 2. Flecs

Primary references:

- [Flecs repository](https://github.com/SanderMertens/flecs)
- [Systems, staging, and sync points](https://www.flecs.dev/flecs/md_docs_2Systems.html)
- [Manual: deferred operations](https://www.flecs.dev/flecs/md_docs_2Manual.html)
- [Design guidance](https://www.flecs.dev/flecs/md_docs_2DesignWithFlecs.html)

### Relevant ideas

Flecs analyzes component reads and writes to determine scheduling and where synchronization points are needed. During staged execution, structural changes are queued and later merged so iteration and multithreaded access remain safe.

The particularly relevant pattern is:

```text
systems enqueue changes
    |
access analysis identifies next dependent read
    |
sync point merges changes
    |
downstream system observes them
```

This resembles UnitCompose provisional publication followed by a visibility boundary.

### Important differences

Flecs documentation explicitly notes that staged behavior is not always identical to immediate behavior. Direct writes to queried component storage are also not necessarily deferred. Therefore staging is not a general transaction or rollback mechanism.

UnitCompose requires a narrower but stricter rule:

- every produced Resource value has defined publication visibility;
- persistent successors and host exports do not commit until Run success;
- execution failure poisons the Module rather than exposing partially modified framework state.

### Reuse assessment

Flecs is implemented in C/C++ and brings an entity-centric model. Direct dependency is unlikely for a Rust-first Alpha, but its pipeline analysis and sync-point behavior are valuable implementation references.

Current recommendation: **strong semantic reference; no direct dependency**.

## 3. Salsa

Primary references:

- [Salsa repository](https://github.com/salsa-rs/salsa)
- [Salsa overview](https://salsa-rs.github.io/salsa/overview.html)
- [Red-green incremental algorithm](https://salsa-rs.github.io/salsa/reference/algorithm.html)

### Relevant ideas

Salsa maintains a database revision, tracks dependencies of computed values, and reuses cached results when inputs have not semantically changed. Its model clearly separates:

- externally mutated inputs;
- deterministic computation;
- stored values and dependency metadata;
- revision identity.

### Similarity to UnitCompose

Salsa is a strong reference for:

- separating logical value identity from storage;
- defining revision or publication identity;
- keeping provenance of derived values;
- distinguishing normalized semantics from execution cache state.

### Important differences

- Salsa computations are intended to be deterministic functions of inputs.
- Mutation occurs outside tracked computation.
- Evaluation is on-demand and incremental.
- Arbitrary stateful Units and externally visible effects do not fit naturally.
- Salsa has no Run-level transaction matching the UnitCompose host-call boundary.

### Reuse assessment

Using Salsa as the core would force UnitCompose toward a pure query model. Its concepts should influence Resource publication metadata and future incremental execution, but direct dependency is not recommended for Alpha.

Current recommendation: **semantic reference; revisit for incremental caching after Alpha**.

## 4. Cross-project lessons

### Adopt

- access declarations must be available before execution;
- read/write conflicts can safely constrain parallelism;
- delayed visibility needs explicit synchronization points;
- logical change identity should not equal physical allocation identity;
- derived dependencies and reasons should be inspectable.

### Do not assume

- deferred mutation equals rollback;
- exclusive access equals transactional safety;
- a general World should be exposed to Unit code;
- entity-oriented storage is required for resource-oriented scheduling;
- change detection alone defines Run commit.
