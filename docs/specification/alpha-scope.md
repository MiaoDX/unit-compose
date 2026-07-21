# Alpha scope

The Alpha should test whether the core model is useful before adding language integration, aggressive reuse, or parallel scheduling.

## 1. Alpha objective

Demonstrate that a Rust-only, sequential implementation can:

- organize a realistic algorithm module into explicit Units and Resources;
- compile and validate a Plan before Unit code runs;
- execute the canonical scenario correctly;
- enforce Run-level commit and failure behavior;
- provide actionable inspection and diagnostics.

## 2. Included

### Definitions and planning

- typed Resource registration;
- inspectable Unit definitions;
- one Rust builder frontend;
- normalized Plan construction;
- Unit type and configuration resolution;
- exact predecessor binding;
- dependency and access-conflict derivation;
- stable internal IDs and declaration provenance.

### Validation

Reject before Unit execution:

- unknown Unit or Resource types;
- missing or unknown configuration;
- missing bindings;
- semantic type or schema mismatch;
- unresolved required inputs;
- ambiguous predecessors;
- multiple Create writers;
- prohibited Run Resource Update;
- multiple Persistent Updates in one Run;
- unordered read/update conflicts;
- dependency cycles;
- missing or unknown exports.

### Execution

- one active Run per Module;
- complete host input admission before refresh;
- stable sequential scheduling;
- exactly one execution per enabled Unit;
- provisional downstream publication;
- staged Persistent Update without physical in-place mutation;
- atomic Run commit of persistent successors and exports;
- explicit poisoned state after execution-phase failure;
- structured shutdown.

### Inspection

- source definition and normalized Plan;
- Unit and Resource declarations;
- each derived dependency and its reason;
- Unit execution order and timing;
- publication and commit events;
- copies, allocations, leases, and storage identity where implemented;
- failure phase and Module usability.

## 3. Canonical executable fixture

```text
external points
    |
preprocess: Observe points, Create normalized_points
    |
detector: Observe normalized_points, Create objects
    |
tracker: Observe objects, Update persistent tracks
```

Exports:

- objects;
- committed tracks.

Required cases:

1. successful Run commits both exports and persistent tracks;
2. detector failure exports nothing, commits no tracks, and poisons the Module;
3. missing input fails before Unit code and leaves the Module reusable;
4. overlapping Run fails admission before input refresh;
5. ambiguous predecessor fails Plan compilation;
6. prohibited Run Resource Update fails Plan compilation;
7. a retained export lease prevents unsafe storage reuse.

## 4. Excluded

- Python Units or embedded Python;
- parallel Scheduler;
- dynamic Unit loading;
- asynchronous Units;
- writable external Resources;
- physical in-place persistent Update;
- general device scheduling;
- distributed execution;
- retries, fallback, skip, timeout, or cancellation;
- persistent checkpoint and restore;
- stable on-disk Plan format;
- compatibility with an earlier implementation.

## 5. Suggested implementation sequence

1. Define semantic IDs, Resource types, Unit definitions, and normalized configuration.
2. Build Plan validation and provenance-rich diagnostics.
3. Implement the sequential readiness loop.
4. Implement Run Resource publications and explicit exports.
5. Implement staged Persistent Update and Run commit.
6. Add poisoned Module state and cleanup paths.
7. Add inspection snapshots and event tracing.
8. Implement the canonical fixture and negative tests.

## 6. Acceptance evidence

Alpha is complete when executable tests demonstrate:

- the same normalized Plan is produced independent of source declaration order;
- no invalid composition reaches Unit business code;
- the sequential order is stable for a Plan;
- every access reads its planned predecessor;
- successful results commit atomically;
- execution failure commits nothing framework-controlled and poisons the Module;
- admission failure is distinguishable and leaves the Module reusable;
- diagnostic output identifies Unit, Resource, access intent, reason, and source location;
- no storage is reused while a lease remains active.

Performance is measured, but no aggressive performance target should override semantic evidence in Alpha.

## 7. Decisions required before later increments

### Before Python

- supported buffer protocols and representations;
- retained immutable view behavior;
- Python exception and shutdown mapping;
- native library trust boundary.

### Before parallel scheduling

- Unit capability declaration for deterministic or implementation-defined behavior;
- Resource equality/equivalence contracts;
- effect eligibility and explicit ordering policy;
- worker pool and host-thread integration.

### Before writable external Resources

- ownership transfer or transaction adapter model;
- rollback or compensation boundary;
- cross-Module coordination;
- host observation guarantees.
