# Research landscape

- **Research date:** 2026-07-21
- **Purpose:** Identify prior art, reusable implementation ideas, and design traps relevant to UnitCompose.

## 1. Overall finding

No single reviewed project combines the full UnitCompose target:

- an embeddable in-process Module;
- explicit Unit-to-Resource access contracts;
- validated static composition;
- persistent Resource state;
- Run-level atomic commit;
- poisoned-state recovery boundary for arbitrary Unit-private state;
- sequential/parallel successful-run equivalence;
- Rust and Python authoring with conditional zero-copy host export.

Nearly every individual idea has substantial prior art. The defensible project identity is the narrow combination and adaptation of those ideas for clean, stateful algorithm modules—not the invention of Resources, dependency scheduling, deterministic execution, or commit protocols themselves.

## 2. Where UnitCompose fits

UnitCompose is closest to a **resource-oriented local compute framework** with a validated execution plan and transactional Run boundary.

It is not primarily:

- a workflow orchestrator;
- a distributed dataflow engine;
- an ECS;
- an incremental query database;
- a reactive timing language;
- an ML compiler;
- a ROS executor.

## 3. Comparison matrix

| Family / project | Strong overlap | Important difference | Role for UnitCompose |
| --- | --- | --- | --- |
| [Bevy ECS](https://github.com/bevyengine/bevy) | World/Resource/System access declarations, conflict-aware scheduling, sequential and parallel executors | Entity-centric model; in-place mutable state; no Run-level transaction | Semantic and implementation reference; possible scheduling prototype |
| [Flecs](https://github.com/SanderMertens/flecs) | Declared read/write access, staging, deferred operations, sync points, pipelines | Deferred ECS commands are not full rollback; direct component writes can be immediate | Strong reference for access analysis and publication boundaries |
| [Salsa](https://github.com/salsa-rs/salsa) | Revisions, persistent database, dependency tracking, cached values, deterministic functions | On-demand pure incremental queries, not scheduled stateful Units | Reference for revision identity and logical/physical separation |
| [Timely Dataflow](https://github.com/TimelyDataflow/timely-dataflow) | Explicit dependencies, cyclic computation, logical time, progress tracking | Stream/distributed event model is much broader and heavier | Reference for progress and later cyclic/streaming needs |
| [Differential Dataflow](https://github.com/TimelyDataflow/differential-dataflow) | Incremental state across changing inputs and iterative computation | Collection-difference model, not per-Run Module state | Comparative reference for future incremental execution |
| [Hydroflow](https://github.com/hydro-project/hydroflow) | Rust dataflow runtime, operator composition, cyclic and asynchronous flows | Stream operators and event-driven execution; public graph syntax | Comparative reference; direct dependency unlikely for Alpha |
| [Lingua Franca](https://github.com/lf-lang/lingua-franca) | Stateful components, declared inputs/outputs, deterministic concurrency, explicit causality | Logical-time reactive language and code generation | Strong semantic reference for deterministic parallelism |
| [Temporal](https://github.com/temporalio/temporal) | Deterministic core logic, effect isolation, failure phases, replayable state | Durable distributed workflows and event history | Reference for separating deterministic logic from effects |
| [Apache Flink](https://github.com/apache/flink) | Consistent state, checkpoints, precommit/commit split, transactional sinks | Distributed streaming and replay; much larger failure model | Strong reference for commit boundary and external effects |
| [Apache Beam](https://github.com/apache/beam) | Portable logical plan, runner independence, stateful processing | Distributed data processing, windows, watermarks | Comparative reference for frontend/runner separation |
| [Ray](https://github.com/ray-project/ray) | Stateful actors, task scheduling, resource constraints | Distributed cluster runtime and remote object store | Out of scope for core implementation |
| [Dask](https://github.com/dask/dask) | Task collections and parallel execution | Python analytics focus, lazy task collections | Comparative only |
| [Dagster](https://github.com/dagster-io/dagster), [Prefect](https://github.com/PrefectHQ/prefect), [Airflow](https://github.com/apache/airflow) | Declarative dependencies, observability, retries | Long-running workflow orchestration, not in-process algorithm code | Diagnostics and tooling inspiration only |
| [ONNX Runtime](https://github.com/microsoft/onnxruntime) | Embeddable execution, typed tensors, memory planning, provider abstraction | Model inference and operator kernels, not arbitrary stateful Units | Reference for allocator/provider and zero-copy integration |
| [JAX](https://github.com/jax-ml/jax), [XLA](https://github.com/openxla/xla), [TVM](https://github.com/apache/tvm) | Functional computation, compilation, memory planning, device execution | Functional tensor programs and compiler IR | Future optimization and device-execution reference |
| [ROS 2](https://github.com/ros2) | Host integration, components, callbacks, executors | Inter-component middleware and callback dispatch | Primary embedding environment; terminology must avoid collision |
| [GStreamer](https://github.com/GStreamer/gstreamer) | Embeddable media elements, pads, state transitions, buffer ownership | Streaming pipeline and media-specific timing | Reference for modular integration and buffer lifetime |

## 4. Ideas already validated by the community

The following directions are well established and should be treated as engineering choices rather than novelty claims:

- declare read/write access before execution;
- derive conflict-free parallelism from access declarations;
- separate source definitions from a normalized execution representation;
- use stable logical identity independent of physical storage;
- stage changes until a synchronization or commit boundary;
- isolate external effects from deterministic or replayable logic;
- use logical ordering rather than thread timing as the semantic basis;
- attach provenance and explanation to derived dependencies;
- expose host-owned buffers through explicit lifetime contracts.

## 5. UnitCompose-specific combination

The potentially distinctive combination is:

1. **Module-scale embedding:** the framework organizes one algorithm component inside a larger host rather than owning the application.
2. **Resource-oriented Unit contracts:** access declarations serve validation, scheduling, inspection, and cross-language safety.
3. **Run-level Resource commit:** downstream provisional visibility exists within a Run, while persistent successors and exports commit together.
4. **Poisoned-state boundary:** arbitrary Unit-private state is not falsely claimed to roll back; execution failure invalidates the live Module.
5. **Small public model:** Module, Unit, Resource, and Plan are enough for ordinary use.
6. **Cross-language and large-buffer focus:** Rust/Python consistency and conditional zero-copy are part of the product boundary.

## 6. Prior-art risk

Prior-art risk is **medium** for a publication or patent claim and **low** for building a useful open-source framework.

A novelty claim based only on names or isolated concepts would be weak. Stronger technical claims would need:

- a precise formal Run semantics;
- demonstrated interaction between provisional publication and atomic commit;
- explicit treatment of private Unit state and poisoned recovery;
- a verified scheduler-equivalence contract;
- a concrete cross-language lease implementation;
- evidence that the combination solves workloads not handled cleanly by existing embedded systems.

## 7. Research categories used in this repository

- [Resource-oriented systems](resource-oriented-systems.md)
- [Dataflow and determinism](dataflow-and-determinism.md)
- [Transactions and failure](transactions-and-failure.md)
- [Implementation options](implementation-options.md)
