# Contributing to UnitCompose

UnitCompose is design-first while the V0 executable baseline is being established. Contributions should keep the public model small and preserve a clear separation between durable concepts and replaceable implementation mechanisms.

## Before opening a change

Read:

1. [Project positioning](docs/adr/0001-project-positioning.md)
2. [Core terminology](docs/adr/0002-core-terminology.md)
3. [Configuration-driven Resource DAG for V0](docs/adr/0004-configuration-driven-resource-dag.md)
4. [V0 architecture specification](docs/specification/v0-architecture.md)

ADR-0003 and the former transactional specification files are superseded for V0.

## Change categories

### Concept or semantic change

A change that affects project positioning, the Unit/Resource/Module/Debug model, YAML behavior, validation, observable execution, reload semantics, or compatibility guarantees requires a new ADR or an explicit amendment through the accepted ADR process.

Do not rewrite an accepted ADR to hide a previous decision. Supersede it and preserve the design history.

### Implementation change

An implementation change may select Rust traits, crate boundaries, containers, graph libraries, error types, tracing libraries, or internal APIs without a new ADR when it preserves the accepted semantics.

### Unit or Resource integration

A new Unit type should document:

- its stable type name;
- input and output port contracts;
- configuration;
- semantic Resource types;
- error behavior;
- whether it retains private state;
- independent tests.

A Resource semantic type should use a stable, namespaced identity and must not rely on Rust `TypeId` as its serialized identity.

### Research update

Research documents should distinguish:

- facts supported by a linked primary source;
- interpretation by UnitCompose maintainers;
- recommendations that still require executable evidence.

## Design principles

- Keep the durable public model to Unit, Resource, Module, and Debug.
- Make the primary composition path configuration-driven.
- Derive dependencies from Resource bindings rather than YAML source order.
- Reject invalid Module Definitions before Unit business code runs.
- Do not expose a general Resource service locator to Unit code.
- Treat Debug and diagnostics as product behavior.
- Prefer the smallest executable mechanism that proves a representative workload.
- Add transaction, state, scheduling, storage, language, or plugin complexity only after a workload requires it.

## Pull requests

A pull request should explain:

- the problem being solved;
- whether semantics change;
- affected ADRs or specification requirements;
- tests or examples used as evidence;
- intentionally deferred work.
