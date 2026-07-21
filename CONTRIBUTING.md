# Contributing to UnitCompose

UnitCompose is currently design-first. Contributions should preserve a clear separation between conceptual semantics and replaceable implementation choices.

## Before opening a change

Read:

1. [Project positioning](docs/adr/0001-project-positioning.md)
2. [Core terminology](docs/adr/0002-core-terminology.md)
3. [Alpha execution model](docs/adr/0003-alpha-execution-model.md)
4. [Core design specification](docs/specification/core-design.md)
5. [Alpha scope](docs/specification/alpha-scope.md)

## Change categories

### Concept or semantic change

A change that affects project positioning, core terminology, observable behavior, failure semantics, or compatibility guarantees requires a new ADR. Do not rewrite an accepted ADR to hide the previous decision.

### Implementation change

An implementation change may select data structures, libraries, crate boundaries, or internal APIs without a new ADR when it preserves the accepted semantics.

### Research update

Research documents should distinguish:

- facts supported by a linked primary source;
- interpretation or comparison by UnitCompose maintainers;
- a dependency recommendation that still requires a prototype.

## Design principles

- Keep the public model small: Module, Unit, Resource, Plan.
- Do not expose dependency-structure implementation vocabulary unless users need it.
- Prefer explicit contracts over constructor side effects or naming conventions.
- Reject invalid compositions before Unit business code runs.
- Treat diagnostics and inspectability as product behavior.
- Avoid adopting a complete framework solely to reuse one internal capability.
- Add a capability only when a representative workload proves the smaller model insufficient.

## Pull requests

A pull request should explain:

- the problem being solved;
- whether semantics change;
- affected ADRs or specification IDs;
- tests or evidence used;
- intentionally deferred follow-up work.
