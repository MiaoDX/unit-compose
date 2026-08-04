# UnitCompose documentation

The default branch contains only the current design. Git history and pull-request discussion preserve earlier alternatives.

## Read in order

1. [Concept overview](concepts/overview.md) — the smallest useful mental model.
2. [Terminology](concepts/terminology.md) — canonical public and implementation terms.
3. [ADR-0001: Project positioning](adr/0001-project-positioning.md).
4. [ADR-0002: Configuration-driven Resource DAG](adr/0002-configuration-driven-resource-dag.md).
5. [ADR-0003: Framework-managed Resource storage](adr/0003-framework-managed-resource-storage.md).
6. [V0 architecture specification](specification/v0-architecture.md) — the normative behavior and acceptance evidence.
7. [V0 implementation plan](plans/v0-implementation-plan.md) — delivery order and prototype gates.

## Research

- [Storage planning and steady-state allocation](research/storage-planning-and-steady-state-allocation.md) — OpenVX, GStreamer, Holoscan, ONNX Runtime, Halide, and caller-provided workspaces.
- [Rust dependency evaluation](research/rust-dependency-evaluation.md) — recommended core, frontend, and optional-adapter dependencies.

Research explains evidence and implementation direction. It does not override accepted ADRs or the V0 specification.

## Document authority

1. The V0 specification defines observable behavior.
2. Accepted ADRs explain why durable choices were made.
3. Examples and tests provide executable evidence.
4. Plans organize delivery.
5. Research supports implementation choices.

A semantic change updates an ADR and the specification together. An implementation choice may change without an ADR when the observable contract remains intact.

## Maintenance policy

Current documents replace obsolete documents. Superseded ADRs, specifications, plans, and research notes are removed from the default branch rather than kept as redirect stubs. Historical text remains available through Git.
