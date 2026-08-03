# UnitCompose documentation

This directory records the current V0 contract, supporting research, and historical design decisions.

## Current V0 baseline

Read these documents in order:

1. [Concept overview](concepts/overview.md) — the smallest useful mental model.
2. [Terminology](concepts/terminology.md) — canonical public and implementation terms.
3. [ADR-0004: Configuration-driven Resource DAG for V0](adr/0004-configuration-driven-resource-dag.md) — why the initial scope was reduced.
4. [V0 architecture specification](specification/v0-architecture.md) — normative behavior and acceptance evidence.

## Accepted architecture decisions

- [ADR-0001: Project positioning](adr/0001-project-positioning.md)
- [ADR-0002: Core terminology](adr/0002-core-terminology.md)
- [ADR-0003: Alpha execution model](adr/0003-alpha-execution-model.md) — superseded for the V0 implementation scope.
- [ADR-0004: Configuration-driven Resource DAG for V0](adr/0004-configuration-driven-resource-dag.md)

## Research

The research documents remain useful as prior-art and future-design references. They are not the V0 implementation contract.

- [Landscape](research/landscape.md)
- [Resource-oriented systems](research/resource-oriented-systems.md)
- [Dataflow and determinism](research/dataflow-and-determinism.md)
- [Transactions and failure](research/transactions-and-failure.md)
- [Implementation options](research/implementation-options.md)

## Superseded specification files

The following paths are retained to prevent stale links from silently resolving to the wrong semantics. They point readers to the current V0 specification:

- [Former core design specification](specification/core-design.md)
- [Former execution semantics](specification/execution-semantics.md)
- [Former Alpha scope](specification/alpha-scope.md)

## Authority

1. Accepted, non-superseded ADRs explain why important choices were made.
2. The [V0 architecture specification](specification/v0-architecture.md) defines current observable behavior.
3. Examples and tests provide executable evidence for that specification.
4. Research and superseded documents provide context but are not normative.

A semantic change requires an ADR and a corresponding specification update. Implementation choices may change without an ADR when they preserve the current contract.
