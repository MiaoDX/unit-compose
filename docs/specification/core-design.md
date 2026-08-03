# Core design specification — superseded

- **Status:** Superseded for V0
- **Superseded by:** [ADR-0004](../adr/0004-configuration-driven-resource-dag.md) and the [V0 architecture specification](v0-architecture.md)

The previous version of this file specified a broader transactional Resource runtime with publication versions, persistent updates, atomic Run commit, poisoned Modules, storage leases, and future scheduler equivalence.

That design is retained in Git history and supporting research, but it is **not** the implementation contract for the first UnitCompose version.

Use the [V0 architecture specification](v0-architecture.md) for current requirements. Advanced transaction, state, storage, and reliability semantics require new representative workloads and future ADRs before implementation.
