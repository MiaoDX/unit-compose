# ADR-0003: Alpha execution model

- **Status:** Accepted
- **Date:** 2026-07-21

## Context

The design must resolve failure visibility before persistent state, parallel scheduling, Python views, or buffer reuse are implemented.

A framework can stage Resource values, but it cannot automatically roll back arbitrary private state inside Rust or Python Unit objects. Writable external objects and physical in-place updates also expose partial effects that an exclusive lock cannot undo.

## Decision

Alpha adopts the following execution baseline:

1. A Module admits at most one active Run.
2. The complete host input envelope is validated before Unit code starts.
3. A stable sequential Scheduler is the executable reference.
4. Every enabled Unit executes exactly once in a normal Run.
5. Each Observe and Update is bound in the Plan to one exact predecessor.
6. Successful Unit outputs may be provisionally visible to planned downstream Units.
7. Framework-controlled persistent successors and exports commit atomically at Run success.
8. Persistent Update is staged into distinct storage; physical in-place Update is disabled.
9. External Resources are read-only.
10. If Unit code, completion fencing, publication, or commit fails, no staged framework value commits and the Module becomes poisoned.
11. Admission and input validation failures occur before Unit code and leave the Module reusable.
12. Retry, fallback, skip, cancellation, timeout, checkpoint, and recovery are deferred.
13. A future parallel Scheduler must preserve planned predecessor identity and successful committed Resource equivalence.

## Consequences

### Benefits

- Failure behavior is testable and does not overclaim rollback.
- The host never receives a partial successful result.
- Previously committed persistent Resource values are not physically corrupted by a failed Update.
- Sequential implementation can establish a clear correctness reference.
- Python and parallel increments inherit an explicit boundary.

### Costs

- Execution-phase failure requires Module reconstruction.
- Staged persistent values may require extra allocation or copying.
- Writable host-owned state is unavailable in Alpha.
- Long-lived Unit-private state cannot survive a failed Run.
- External effects remain outside atomicity unless a later explicit adapter is designed.

## Revisit conditions

A new ADR is required before adding:

- reusable Modules after execution failure;
- transactional Unit-private state hooks;
- writable external Resources;
- physical in-place persistent Update;
- durable replay or checkpoint restoration;
- a weaker partial-commit mode.
