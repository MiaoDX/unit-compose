# Execution semantics

- **Status:** Proposed Alpha baseline
- **Depends on:** [Core design specification](core-design.md)

This document expands the semantics of one Module Run without adding new public model pillars.

## 1. Run phases

A Run proceeds conceptually through:

```text
admit inputs
    |
prepare run-scoped Resource state
    |
execute ready Units
    |
stage Unit publications
    |
commit persistent successors and exports
    |
return result
```

The implementation may combine phases, but observable behavior must remain equivalent.

## 2. Committed and provisional Resource values

A **publication** is a produced value of a Resource.

Two visibility states are required internally:

### Provisional publication

A successful Unit may publish an output provisionally inside the current Run. That value is visible only to downstream Unit accesses bound to it by the Plan.

It is not yet:

- visible to the host;
- the current persistent value for a later Run;
- eligible for storage reuse while a dependent lease exists.

### Committed publication

At successful Run commit, staged persistent successors become the current values for later Runs and exported publications become host-visible results.

This distinction resolves an important requirement: downstream Units must consume upstream outputs before the whole Run commits, while the host and future Runs must not observe a partially successful Run.

## 3. Predecessor binding

Every Observe and Update binds during Plan compilation to one exact predecessor:

- a host input;
- an initialized persistent value;
- a parameter;
- or one planned Create or Update publication.

The Scheduler never chooses a Resource version dynamically.

If more than one legal predecessor exists and the source definition does not disambiguate it, Plan compilation fails.

## 4. Access and readiness

A Unit becomes ready only when:

- all predecessor publications required by its declared accesses are available;
- explicit non-data ordering constraints are satisfied;
- the necessary read or exclusive leases can be acquired;
- the Module is not failing or poisoned.

Readers of the same immutable publication may execute together. A Create writes a new publication. An Update reads a predecessor and writes a distinct staged successor in Alpha.

## 5. Alpha legality matrix

| Resource lifetime | Observe | Create | Update |
| --- | --- | --- | --- |
| Run | After planned producer or host admission | One planned producer | Rejected; use a distinct Resource |
| Persistent | From initialized or planned predecessor | Initialization only | At most once per Run; staged, not in place |
| External | Allowed under a host/runtime read lease | Rejected | Rejected |
| Parameter | Allowed | Initialization only | Rejected |

## 6. Atomic commit boundary

Run commit includes all framework-controlled effects that UnitCompose claims to make atomic:

- staged Persistent Resource successors;
- required Run exports;
- internal publication metadata needed to make those values current.

Run commit does not make arbitrary Unit-private state or external side effects reversible. Instead, a post-execution failure poisons the Module so no later Run can observe a mixture of old framework state and mutated private Unit state.

## 7. Unit completion

A Unit completes successfully only when:

- Unit code returns success;
- all produced Resource metadata is valid;
- any required device completion fence has completed successfully;
- output publications can be registered without violating leases or representation constraints.

Only then do dependent Units become ready.

## 8. Failure behavior

### Before Unit code starts

Plan, binding, initialization, admission, or input validation failures expose no Run publications. Admission and input validation failures leave a ready Module reusable.

### After Unit code starts

When a Unit or completion boundary fails:

1. the failing Unit is recorded;
2. no new dependent Unit is launched;
3. staged persistent successors and exports are discarded logically;
4. already-running independent Units may finish only to reach a safe cleanup point;
5. the Module becomes poisoned;
6. the host receives a structured error.

The implementation must not reuse storage that may still be observed by an active native, Python, host, or device lease.

## 9. External effects

Alpha does not claim transactional semantics for arbitrary I/O, logging, host callbacks, network calls, or mutation of external objects.

A Unit with externally visible effects must either:

- treat them as diagnostic and explicitly outside result equivalence;
- represent them through a future declared effect contract;
- or be marked ineligible for parallel scheduler equivalence.

Serial ordering prevents overlap but does not provide rollback or value equivalence.

## 10. Sequential reference behavior

The sequential Scheduler:

- uses a stable tie-break for ready unordered Units;
- launches no Unit before its declared predecessors are available;
- records why each Unit became ready or remained blocked;
- is the executable reference for Alpha semantics.

The stable tie-break is not a dependency. A user who requires order must declare order.

## 11. Parallel equivalence contract

A later parallel Scheduler must preserve:

- predecessor identity for every Observe and Update;
- successful committed Resource equivalence;
- required exports and their completeness;
- no undeclared persistent or external Scheduler effect.

Failed-run timing equivalence is not required beyond the Alpha failure contract: no framework-controlled commit and a poisoned Module.

## 12. Host-retained exports

A successful Run may return immutable exported values backed by leased storage.

While a host-retained lease remains alive, the Module must not mutate or reuse that storage. A later Run may:

- allocate or select different compatible storage; or
- fail admission with a structured capacity/backpressure error if policy forbids allocation.

Alpha should prefer correctness and explicit failure over hidden copying or blocking.

## 13. Python and native views

For a supported zero-copy view:

- Observe access is read-only through the supported API;
- a retained immutable view extends storage lifetime and blocks reuse;
- a mutable view is available only for a Unit's own Create or staged Update output under an exclusive lease;
- publication is unavailable until completion and validation;
- consumers that cannot meet the contract require a copy or are rejected.

Hostile or unsafe native code can violate these rules and is outside the framework's recovery guarantee.
