# ADR-0003: Alpha execution model

- **Status:** Superseded for V0 by [ADR-0004](0004-configuration-driven-resource-dag.md)
- **Date:** 2026-07-21
- **Superseded:** 2026-08-03

## Historical decision

This ADR selected a transactional Alpha baseline with one active Run, exact predecessor publication binding, staged persistent updates, Run-level atomic commit, read-only external Resources, and a poisoned Module after execution failure.

The decision was internally consistent, but it required the first implementation to solve Resource versioning, commit protocols, rollback boundaries, leases, persistent state, and future scheduler equivalence before demonstrating the project's primary value as an algorithm-composition framework.

## Superseding decision

[ADR-0004](0004-configuration-driven-resource-dag.md) narrows V0 to a YAML-driven typed Resource DAG with stable sequential execution and read-only Debug. V0 does not claim transaction, rollback, managed persistent Resource, or poisoned Module semantics.

The original ADR remains part of the design history and may inform later reliability work. Its semantics are not current requirements unless a future ADR explicitly reintroduces them.
