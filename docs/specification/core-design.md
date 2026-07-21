# UnitCompose core design specification

- **Status:** Proposed Alpha baseline
- **Date:** 2026-07-21
- **Review target:** Product model, domain boundaries, and observable semantics

This document defines the conceptual contract for UnitCompose. Concrete Rust traits, Python decorators, serialization formats, crate boundaries, and dependency choices remain replaceable until implementation evidence justifies them.

## 1. Review contract

Status labels have precise meanings:

| Status | Meaning |
| --- | --- |
| **Foundational** | Stable project direction. |
| **Alpha** | Normative behavior for the first implementation. |
| **Open** | A material decision that must be resolved before the affected implementation slice. |
| **Deferred** | Intentionally outside Alpha. |

Accepted ADRs and this specification are the source of truth for conceptual and observable behavior. Research documents are supporting evidence, not contracts.

## 2. Problem

Long-lived algorithm modules tend to accumulate:

- business logic inside large application classes;
- hidden data dependencies and execution-order assumptions;
- shared mutable state with unclear ownership;
- inconsistent initialization, error, profiling, and debug behavior;
- expensive source changes when composition changes;
- APIs that are difficult to inspect or generate from metadata.

UnitCompose provides a small embedded framework that separates data, computation, composition, and execution while keeping those concepts inside one host-level module.

## 3. Goals

### 3.1 Product goals

- Make algorithm modules easier to organize, test, inspect, and evolve.
- Embed inside ROS, services, simulation applications, and other host frameworks.
- Keep the public model small: Module, Unit, Resource, Plan.
- Make Rust and Python Unit authoring follow one semantic model.
- Validate a composition before Unit business code runs.
- Permit safe in-process exchange of large arrays, tensors, and device buffers where ownership and representation allow it.
- Allow scheduling and storage policies to evolve without rewriting Unit business logic.

### 3.2 Engineering goals

- Declare every Unit's Resource access explicitly.
- Select every observed or updated predecessor during Plan compilation, not at runtime.
- Derive dependencies and access conflicts from declarations.
- Separate logical Resource semantics from physical allocation and reuse.
- Give sequential and parallel schedulers one successful-run contract.
- Return diagnostics that identify the Unit, Resource, access intent, and declaration source involved.

### 3.3 AI-assisted development goals

- Expose machine-readable Unit, Resource, configuration, and Plan declarations.
- Generate scaffolding, schemas, tests, examples, and documentation from shared metadata.
- Explain why a Unit ran, waited, conflicted, or failed.
- Prefer explicit contracts over constructor side effects and implicit naming.

## 4. Non-goals for Alpha

- An application framework or ROS replacement.
- Distributed or transparent remote execution.
- A complete ECS or entity model.
- Dynamic or conditional Plan mutation after Module creation.
- Multiple overlapping Runs in one Module.
- Asynchronous Units.
- General GPU scheduling.
- Automatic optimization of arbitrary Unit compositions.
- A first-class C++ Unit SDK.
- Sandboxing hostile native or Python code.
- Universal zero-copy.
- Stable serialized Plan compatibility.
- Checkpointing or recovery of a poisoned Module.

## 5. Core model

```text
Module Definition
      |
      | compile and validate
      v
     Plan
      |
      | instantiate
      v
    Module ---- Scheduler
      |             |
      +-- Units <---+
      |
      `-- Resources
```

### UC-F01 — Separate computation and data — Foundational

Units implement behavior. Resources represent typed logical values and state. A Unit receives only the Resource access declared for that Unit.

### UC-F02 — Static composition — Foundational

A Module Definition compiles into one normalized Plan before Unit business code runs.

### UC-F03 — Validation before execution — Foundational

Unknown Unit types, invalid configuration, missing bindings, incompatible Resource types, ambiguous predecessors, conflicting writers, prohibited updates, invalid ordering, and dependency cycles fail before Unit business code runs.

### UC-F04 — Logical and physical separation — Foundational

Logical Resource identity, lifetime, access intent, and publication visibility are separate from storage allocation, memory domain, and buffer reuse.

### UC-F05 — One semantic model — Foundational

Rust, Python, and declarative frontends must compile equivalent definitions into equivalent Plans.

### UC-F06 — Inspectability — Foundational

Plans, derived dependencies, access conflicts, declaration provenance, copies, storage associations, and scheduling decisions are inspectable.

### UC-F07 — Small public vocabulary — Foundational

New public concepts require a user-visible semantic distinction that cannot be expressed clearly with Module, Unit, Resource, Plan, Scheduler, Run, and ordinary implementation terms.

## 6. Module Definition and Plan

### UC-S01 — Inspectable Unit definitions — Alpha

A Unit type exposes its semantic identity, configuration schema, Resource access declarations, lifecycle needs, and capability metadata before construction.

### UC-S02 — Plan contents — Alpha

Plan compilation resolves:

- Unit semantic types and normalized configuration;
- Resource semantic types, schema versions, and lifetimes;
- Unit-to-Resource bindings and access intents;
- the exact predecessor for each Observe and Update;
- dependencies and access conflicts;
- explicit non-data ordering;
- required host inputs and exports;
- declaration provenance.

### UC-S03 — Source order is not dependency — Alpha

Source-list order does not silently create ordering. Non-data ordering must be explicit and explained in the Plan.

### UC-S04 — Plan equivalence — Alpha

Plan equivalence compares normalized semantics:

- Unit semantic type and normalized configuration;
- Resource semantic type and schema;
- access intent and predecessor binding;
- dependencies and explicit ordering policies;
- required inputs and exports.

It excludes source-list order, generated internal IDs, declaration provenance, Unit implementation language, and physical storage choices.

Concrete implementation artifacts and versions belong to a build or binding identity, not Plan semantic equivalence.

## 7. Unit contract

### UC-S05 — Unit scope — Alpha

A Unit is the smallest scheduled computation. It may retain private algorithm state and prepared handles, but it cannot retain run-scoped access after its allowed lifetime.

### UC-S06 — Declared access only — Alpha

Unit code cannot access undeclared Resources. The Unit execution context exposes only the Unit's declared view.

### UC-S07 — One execution per Run — Alpha

Every enabled Unit in an Alpha Plan executes exactly once in a normal successful Run. Alpha performs no demand-driven pruning.

A Unit with no contribution to an export or persistent effect may be rejected or diagnosed as unused during Plan validation.

### UC-S08 — Synchronous completion — Alpha

A Unit is synchronously complete when its host work is complete and any device work required by its outputs is complete, or when it returns a completion fence that the Module waits on before dependent Units become ready.

General asynchronous Unit execution is deferred.

### UC-S09 — Private state and failure — Alpha

Unit private state is not transactionally rolled back. If Unit business code, completion fencing, publication, or commit fails, the Module becomes poisoned and cannot be run again.

This rule makes Run-level Resource commit possible without pretending that arbitrary native or Python object state is reversible.

## 8. Resource model

### UC-S10 — Resource dimensions — Alpha

A Resource declaration identifies at least:

- semantic type and schema version;
- lifetime;
- access intent through each Unit binding;
- mutability contract;
- representation requirements when relevant;
- whether the committed value is exported.

### UC-S11 — Resource lifetimes — Alpha

Alpha supports:

| Lifetime | Meaning |
| --- | --- |
| **Run** | Exists only during one Run and is discarded on failure. |
| **Persistent** | Has an initialized committed value and may receive at most one logical Update per successful Run. |
| **External** | Borrowed from the host for one Run. External Resources are read-only in Alpha. |
| **Parameter** | Read-only value initialized before Runs begin. |

Unit-private scratch storage is not a Resource unless it must participate in dependencies, inspection, or host exchange.

### UC-S12 — Access intents — Alpha

- **Observe** reads one exact predecessor publication.
- **Create** produces a new publication and exposes no predecessor to the Unit.
- **Update** reads one exact predecessor and produces its successor.

Alpha permits one Create for each Run Resource and at most one Update for each Persistent Resource per Run.

### UC-S13 — No physical in-place persistent Update — Alpha

Persistent Update produces a logically and physically separate staged successor in Alpha. The previously committed value is never mutated in place.

### UC-S14 — External Resources are read-only — Alpha

The host may provide borrowed external storage for Observe access. Writable external Resource semantics are deferred because exclusivity alone does not provide rollback or protect the host from partial effects.

### UC-S15 — Explicit exports — Alpha

Only explicitly exported Resource values appear in a successful Run result. Inspection APIs do not provide an alternate undeclared business-data export path.

## 9. Module lifecycle

### UC-S16 — Lifecycle order — Alpha

A Module follows these conceptual phases:

```text
compile -> bind -> initialize -> ready -> run* -> shutdown
                                      \-> poisoned
```

Exact API names are implementation choices.

### UC-S17 — One active Run — Alpha

A Module admits at most one Run at a time. An overlapping call is rejected before host inputs are refreshed or Unit business code starts. Alpha does not silently queue or block it.

### UC-S18 — Host input envelope — Alpha

Before Unit business code starts, a Run validates the complete input envelope:

- all required inputs are present;
- no unknown inputs are supplied;
- semantic and representation compatibility hold;
- required leases can be acquired.

Input admission failure leaves the Module reusable.

### UC-S19 — Initialization failure — Open

The implementation must define cleanup ordering for partial Unit initialization and host destruction before the lifecycle API is frozen. No Run may begin after initialization failure.

## 10. Run and commit model

The normative details are in [execution-semantics.md](execution-semantics.md).

### UC-S20 — Run-level atomic commit — Alpha

Framework-controlled persistent updates and host exports commit as one Run-level boundary.

Before commit:

- successful Unit outputs may be provisionally visible to their planned downstream consumers;
- the host and later Runs continue to observe only previously committed persistent values;
- exports are not returned.

On success, staged persistent successors and required exports become committed together.

### UC-S21 — Failed Run — Alpha

If failure occurs after Unit code has started:

- no staged persistent successor or Run export is committed;
- no new dependent Unit is launched;
- the Module becomes poisoned;
- already-running independent work may finish only as required for safe cleanup;
- the host receives a structured failure.

Retry, fallback, skip, cancellation, timeout, and degraded execution are deferred.

## 11. Scheduling model

### UC-S22 — Sequential reference — Alpha

A stable sequential Scheduler is the reference for correctness, debugging, and deterministic tests. Its tie-break among ready, unordered Units is stable for a Plan but does not create a user-visible dependency.

### UC-S23 — Parallel eligibility — Alpha contract, later implementation

A future parallel Scheduler may run Units concurrently only when Plan dependencies and Resource access constraints permit it.

Scheduler-visible correctness may depend only on declared Resources and explicit ordering. Shared mutable state, I/O, host callbacks, and other external effects must be represented through declared contracts or mark the composition ineligible for scheduler equivalence.

### UC-S24 — Successful-run equivalence — Alpha contract, later implementation

For fixed inputs and deterministic Unit implementations:

1. each Observe and Update reads the same planned predecessor under sequential and parallel scheduling;
2. committed Resource values are equivalent under the Resource type's defined equality relation;
3. the Scheduler introduces no undeclared persistent state or external effect.

No bitwise guarantee is made for floating-point reductions, random sources, external libraries, thread assignment, or wall-clock timing unless the Resource or Unit contract explicitly provides one.

## 12. Failure model

Failures carry a phase and Module usability classification.

| Phase | May Unit code have run? | Module after failure |
| --- | --- | --- |
| Plan validation | No | No Module created |
| Binding or initialization | No Run | Not ready; cleanup required |
| Run admission or input validation | No | Reusable |
| Unit execution | Yes | Poisoned |
| Completion fence | Yes | Poisoned |
| Publication or commit | Yes | Poisoned |
| Contract violation | Possibly | Explicitly poisoned or unrecoverable |

Rust errors and contained Python exceptions during Unit processing map to the same Unit-execution failure concept. Process aborts, memory-safety violations, and failures escaping framework control have no structured recovery guarantee.

## 13. Language and interoperability direction

### UC-P01 — Rust kernel — Alpha

Rust is the implementation language for the core Module, Plan, validation, Resource, and sequential scheduling mechanisms.

### UC-P02 — Rust and Python Units — Direction

Rust and Python are intended to be first-class Unit authoring environments that produce equivalent Unit declarations and follow the same access, publication, failure, and lifetime semantics.

Python integration is not part of the first implementation increment.

### UC-P03 — Conditional zero-copy — Foundational

Zero-copy is an outcome of compatible ownership, representation, memory domain, synchronization, and lifetime. It is not a universal promise.

### UC-P04 — Borrowed view safety — Open for Python increment

Supported Python views must either:

- remain within the declared lease;
- retain immutable storage ownership and prevent reuse; or
- copy when the consumer cannot obey the contract.

Unsafe native extensions that bypass supported interfaces are outside the recovery guarantee.

## 14. Observability

The framework should expose structured information for:

- source definitions and compiled Plans;
- Unit and Resource declarations;
- dependencies, conflicts, and their provenance;
- Unit readiness, wait reasons, execution timing, and failure phase;
- Resource publication and commit history;
- storage ownership, allocation, copies, leases, and retained views;
- language-boundary and device-fence timing.

Humans and coding agents should be able to ask why a Unit ran, waited, conflicted, or failed, and which Unit produced a Resource value.

## 15. Trust boundary

- Rust Units compiled into the host are trusted native code.
- Python Units are trusted application code, while adapters still enforce supported access and lifetime contracts.
- Future native plugins are unsafe foreign-function boundaries.
- Module Definitions are input and require schema validation.
- Resource deserialization must not instantiate untrusted code implicitly.

## 16. Canonical scenario

A host supplies a point cloud. Preprocessing creates normalized points. Detection creates objects. Tracking updates persistent tracks.

```text
host input -> preprocess -> normalized points -> detector -> objects -> tracker
                                                                         |
                                                                         v
                                                                  persistent tracks
```

For a successful Run, the host receives explicitly exported objects and tracks. If detection fails, tracking does not run, no new objects or tracks commit, and the Module becomes poisoned.

Every supported frontend must compile this scenario into an equivalent Plan.

## 17. Open decisions

- Cleanup guarantees after partial initialization.
- Representation and schema identity format.
- Python retained-view enforcement and supported buffer protocols.
- Behavior when host-retained immutable exports prevent storage reuse.
- Exact diagnostic schema and stable IDs.
- Whether and when to support writable external Resources.
- Whether persistent recovery or checkpointing is needed after Alpha.

## 18. Deferred capabilities

- asynchronous Units;
- dynamic native loading;
- dynamic or conditional Plans;
- multiple in-flight Runs per Module;
- general GPU resource scheduling;
- process-isolated Python workers;
- ownership-consuming access for aggressive reuse;
- stable serialized Plan compatibility;
- persistent caching and checkpointing;
- remote execution.

Each capability requires a representative workload and evidence that the smaller model cannot meet it.
