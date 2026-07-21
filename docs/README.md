# UnitCompose documentation

This directory is the design baseline for UnitCompose.

## Start here

- [Concept overview](concepts/overview.md): the smallest useful mental model.
- [Terminology](concepts/terminology.md): canonical names and legacy mappings.
- [Core design specification](specification/core-design.md): product boundaries and normative concepts.
- [Execution semantics](specification/execution-semantics.md): resource visibility, commit, scheduling, and failure behavior.
- [Alpha scope](specification/alpha-scope.md): the first implementation increment and acceptance evidence.

## Research

- [Landscape](research/landscape.md): comparison across relevant communities.
- [Resource-oriented systems](research/resource-oriented-systems.md): Bevy ECS, Flecs, and Salsa.
- [Dataflow and determinism](research/dataflow-and-determinism.md): Timely, Differential, Hydroflow, Lingua Franca, and related models.
- [Transactions and failure](research/transactions-and-failure.md): Temporal, Flink, commit boundaries, and external effects.
- [Implementation options](research/implementation-options.md): dependency candidates and prototype recommendations.

## Architecture decisions

- [ADR-0001: Project positioning](adr/0001-project-positioning.md)
- [ADR-0002: Core terminology](adr/0002-core-terminology.md)
- [ADR-0003: Alpha execution model](adr/0003-alpha-execution-model.md)

## Authority

The documents have different roles:

1. Accepted ADRs record why important choices were made.
2. The specification defines the current conceptual and observable contract.
3. Research documents provide evidence and alternatives, but are not normative.
4. Source code and tests will define implementation details only where they do not conflict with accepted semantics.

When evidence requires a semantic change, add a new ADR and update the affected specification sections together.
