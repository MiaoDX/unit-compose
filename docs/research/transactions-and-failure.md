# Transactions and failure

This document examines how workflow and stream-processing systems separate deterministic state, external effects, precommit, commit, and recovery.

## 1. Temporal

Primary references:

- [Temporal repository](https://github.com/temporalio/temporal)
- [Temporal documentation](https://docs.temporal.io/)
- [Temporal architecture](https://github.com/temporalio/temporal/blob/main/docs/architecture/README.md)

Temporal persists workflow history and reconstructs workflow state through deterministic replay. Workflow code is separated from Activities that perform external, failure-prone, or non-deterministic work.

### Relevant lessons

- deterministic logic and external effects need different contracts;
- failure phase matters: a task attempt failing is not identical to the logical workflow failing;
- effectful work needs idempotency, retry, compensation, or an explicit at-most/at-least-once contract;
- durable state reconstruction requires a recorded semantic history, not only a memory snapshot.

### Difference

UnitCompose is in-process and bounded to one Module Run. Alpha does not persist event history, replay Unit code, or retry automatically.

### Application to UnitCompose

UnitCompose should not promise scheduler equivalence or rollback for arbitrary host callbacks, I/O, or native side effects. Such effects need a future explicit contract. Alpha instead limits atomicity to framework-controlled Resources and poisons the Module after execution failure.

Recommendation: **strong reference for effect isolation and failure classification; no direct dependency**.

## 2. Apache Flink

Primary references:

- [Apache Flink repository](https://github.com/apache/flink)
- [Checkpointing documentation](https://nightlies.apache.org/flink/flink-docs-stable/docs/dev/datastream/fault-tolerance/checkpointing/)
- [Data Sink and committer APIs](https://nightlies.apache.org/flink/flink-docs-stable/docs/dev/datastream/sinks/)
- [Fault tolerance overview](https://nightlies.apache.org/flink/flink-docs-stable/docs/learn-flink/fault_tolerance/)

Flink creates consistent snapshots of distributed operator state. End-to-end exactly-once requires replayable sources and transactional or idempotent sinks. Its sink API separates precommit work from the final Committer.

### Relevant lessons

- internal state consistency does not automatically make external effects exactly once;
- a precommit/commit split is useful for staged outputs;
- failure recovery semantics depend on source replay and sink behavior;
- state snapshots and transaction commits are related but distinct mechanisms.

### Difference

UnitCompose Alpha does not replay inputs or restore a failed Module. It uses Run-level atomic visibility for local Resource state and exports, followed by poisoned-state replacement after execution failure.

### Application to UnitCompose

The Flink model supports keeping these boundaries explicit:

```text
Unit output publication -> provisional
Run success              -> commit framework-controlled state and exports
external side effect     -> separate future adapter contract
```

Recommendation: **strong semantic reference for publication/commit and external adapters; no direct dependency**.

## 3. Why exclusive access is not enough

An exclusive lease proves that no other framework participant concurrently accesses a storage location. It does not prove:

- the old value can be reconstructed after partial mutation;
- the host has not observed the mutation;
- an external device or library has not retained a pointer;
- an I/O operation can be undone;
- a second Module sharing the object follows the same lock.

Therefore Alpha rejects writable external Resources and physical in-place persistent Update.

## 4. Why Unit-private state is special

A Unit may hold arbitrary Rust or Python state:

- caches;
- model handles;
- mutable containers;
- library objects;
- random generators;
- device contexts.

The framework cannot generically snapshot or roll back that state. Three possible models exist:

1. restrict private state to semantically irrelevant caches;
2. require Unit checkpoint/prepare/commit/abort hooks;
3. poison the Module after any execution-phase failure.

Alpha selects option 3 because it is small, honest, and testable. Later workloads may justify an explicit transactional Unit capability, but it should not be implicit.

## 5. Run-level atomicity in Alpha

Alpha commits atomically only what UnitCompose controls:

- staged persistent Resource successors;
- required host exports;
- associated current-publication metadata.

If a Run fails after Unit code starts:

- none of those staged values commit;
- arbitrary external effects may already have occurred;
- Unit-private state may have changed;
- the Module is poisoned and must be replaced.

This is closer to a fail-stop component boundary than a fully recoverable transaction.

## 6. Future external effect designs

Potential later models include:

### Transaction adapter

The host supplies prepare, commit, and abort operations for an external Resource.

### Ownership-consuming effect

The host transfers unique ownership into the Run and receives a new value only on success.

### Idempotent sink

The effect accepts a stable Run or publication identity and safely ignores duplicates.

### Compensating action

The Plan declares a compensating Unit or host operation. This is not rollback and must be modeled explicitly.

### Durable replay

Inputs and Unit decisions are recorded so a new Module can reconstruct state. This would substantially expand project scope toward workflow systems.

No future model should be added without a concrete workload and a clear end-to-end guarantee.
