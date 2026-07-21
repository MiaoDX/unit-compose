# Dataflow and determinism

This document covers systems and models that expose dependencies, progress, cyclic computation, and deterministic parallel execution.

## 1. Timely Dataflow

Primary references:

- [Timely Dataflow repository](https://github.com/TimelyDataflow/timely-dataflow)
- [Core concepts](https://timelydataflow.github.io/timely-dataflow/chapter_1/chapter_1.html)
- [Progress tracking](https://timelydataflow.github.io/timely-dataflow/chapter_5/chapter_5_2.html)
- [Timely Dataflow: A Model](https://research.google/pubs/timely-dataflow-a-model/)

Timely attaches logical timestamps to data and tracks progress so operators can reason about what input may still arrive. It supports expressive cyclic and distributed computation.

### Relevance

- explicit component independence enables flexible execution;
- logical ordering is separated from physical timing;
- progress information is a first-class coordination mechanism;
- cycles require stronger semantics than a simple topological schedule.

### Difference

UnitCompose Alpha has one bounded host-invoked Run, exactly-once Unit execution, and no streaming inputs or cyclic execution. Timely is therefore much more general than needed.

Recommendation: **semantic reference for future cyclic, streaming, or overlapping-run requirements; no Alpha dependency**.

## 2. Differential Dataflow

Primary references:

- [Differential Dataflow repository](https://github.com/TimelyDataflow/differential-dataflow)
- [Differential Dataflow paper](https://www.microsoft.com/en-us/research/?p=163907)

Differential Dataflow represents changing collections as differences and reuses prior work across nested iteration.

### Relevance

- persistent computation state can be updated incrementally;
- time/version structure can distinguish reasons for change;
- iterative computation and changing inputs can share one model.

### Difference

UnitCompose Resources are arbitrary typed values, not multiset collections with algebraic differences. Unit execution is stateful and imperative rather than a declarative collection program.

Recommendation: **comparative reference for later incremental recomputation; no direct dependency**.

## 3. Hydroflow

Primary references:

- [Hydroflow repository](https://github.com/hydro-project/hydroflow)
- [Hydroflow crate documentation](https://docs.rs/hydroflow/latest/hydroflow/)
- [Hydro dataflow programming](https://hydro.run/docs/hydro/reference/dataflow-programming)

Hydroflow is a low-level Rust dataflow runtime with a custom surface syntax and support for asynchronous streams.

### Relevance

- Rust-native composition and operator execution;
- internal readiness-driven scheduling;
- feedback and streaming capabilities;
- a practical implementation to inspect when UnitCompose reaches beyond bounded Runs.

### Difference

The public programming model is stream/operator-oriented and explicitly dataflow-shaped. UnitCompose is Resource-oriented, host-invoked, and intentionally avoids making dependency structures the primary user vocabulary.

Recommendation: **comparative implementation reference; reconsider only if streaming or cyclic Units become a demonstrated requirement**.

## 4. Lingua Franca

Primary references:

- [Lingua Franca repository](https://github.com/lf-lang/lingua-franca)
- [Introduction and reactor semantics](https://www.lf-lang.org/docs/)
- [High-Performance Deterministic Concurrency using Lingua Franca](https://arxiv.org/abs/2301.02444)

Lingua Franca composes stateful reactors with declared ports, reactions, logical time, causality analysis, and deterministic concurrent execution.

### Strong overlap

- stateful components with explicit input/output access;
- composition separated from target-language business code;
- dependencies determined before execution;
- concurrency extracted without changing deterministic logical behavior;
- causality loops rejected;
- polyglot authoring.

### Difference

Lingua Franca is a coordination language and code generator with event tags, timers, logical time, and distributed execution. UnitCompose is a library embedded in a host call path and does not define logical time in Alpha.

### Key lesson

Deterministic parallelism requires stronger rules than “no data races.” The semantic predecessor and order of observations must be fixed independently of thread timing. This directly supports UnitCompose's Plan-bound predecessor and successful-run equivalence rules.

Recommendation: **strong semantic reference for scheduler design**.

## 5. Kahn Process Networks

Kahn Process Networks model deterministic sequential processes communicating through channels with blocking reads and conceptually unbounded buffering.

### Relevance

KPNs illustrate that scheduler-independent determinism comes from constraints on communication semantics, not merely from a deterministic scheduler implementation.

### Difference

UnitCompose does not use stream channels or blocking reads, and Alpha Runs are bounded. KPN assumptions such as unbounded channels would be unacceptable for predictable module memory behavior.

Recommendation: **theoretical reference only**.

## 6. Calvin and deterministic transaction scheduling

Primary reference:

- [Calvin: Fast Distributed Transactions for Partitioned Database Systems](https://cs.yale.edu/homes/dna/papers/calvin-sigmod12.pdf)

The Calvin database work separates transaction sequencing from parallel execution: a deterministic order is established first, then execution can exploit concurrency while preserving that order.

### Relevance

This supports a UnitCompose principle:

> The Plan determines semantic predecessor and required order; the Scheduler decides only how to realize the remaining safe concurrency.

### Difference

Calvin targets distributed database transactions with read/write sets, replication, and locking. UnitCompose has typed Resources, arbitrary Unit code, and a poisoned Module boundary rather than transaction replay.

Recommendation: **conceptual reference for Plan-first scheduling**.

## 7. Lessons for UnitCompose

### Adopt now

- dependencies and predecessor identity are semantic, not timing outcomes;
- the sequential schedule is a reference execution, not the source of undeclared order;
- causality cycles should fail Plan compilation in Alpha;
- parallelism is permitted only after semantic order is fixed;
- successful equivalence should be defined over Resource values, not thread traces.

### Defer until a real workload requires them

- logical time;
- watermarks or frontiers;
- feedback cycles;
- streaming operators;
- overlapping Runs;
- distributed progress tracking;
- incremental collection algebra.
