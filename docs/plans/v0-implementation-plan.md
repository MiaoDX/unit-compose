# V0 implementation plan

- **Status:** Proposed delivery plan
- **Date:** 2026-08-04
- **Normative baseline:** [V0 architecture specification](../specification/v0-architecture.md)

The plan proves the contract in small executable increments. A demo must not invent semantics that the synthetic kernel tests have not already established.

## 1. Recommended crate boundaries

```text
crates/
  unit-compose-core/
  unit-compose-yaml/
  unit-compose-debug/
  unit-compose-debug-rerun/     optional

examples/
  navigation-planning/
```

Initial dependency direction:

```text
unit-compose-core
  -> dyn-stack
  -> thiserror

unit-compose-yaml
  -> unit-compose-core
  -> serde
  -> saphyr

unit-compose-debug
  -> unit-compose-core

unit-compose-debug-rerun
  -> unit-compose-core
  -> rerun
```

These are implementation recommendations, not architecture guarantees. See the [dependency research](../research/rust-dependency-evaluation.md).

## 2. Milestone 0 — Contract and API spike

Create three synthetic typed Units:

- `FixedImageFilter` — fixed-size output;
- `BoundedPointFilter` — variable logical length with fixed maximum capacity;
- `WorkspaceHeavyPlanner` — precise scratch requirement.

Prototype:

- typed inputs;
- typed output value and buffer writers;
- caller-provided workspace;
- semantic type to concrete representation registry;
- borrowed Module outputs;
- `run_into`;
- recoverable and fatal Unit failures;
- capacity overflow.

Exit criteria:

- no string lookup inside the Unit execution hot path;
- no Unit-owned output allocation in the primary API;
- partial initialization drops safely;
- the APIs remain understandable without exposing raw pointers or allocator generics.

## 3. Milestone 1 — Graph compiler

Implement:

- Unit and Resource IDs;
- registries and descriptors;
- Module Definition IR;
- required-port validation;
- semantic and concrete type validation;
- producer-consumer derivation;
- duplicate-producer diagnostics;
- stable Kahn topological ordering;
- cycle diagnostics;
- DOT and Mermaid export.

Use a small UnitCompose-owned graph representation first. A graph crate may be added only if it materially improves diagnostics or maintenance.

Evidence:

- source-order permutation tests;
- fan-out and fan-in;
- multiple independent roots;
- deterministic compiled-graph fingerprint;
- negative graph fixtures.

## 4. Milestone 2 — Typed storage kernel

Implement:

- fixed value slots;
- fixed typed buffers;
- bounded variable-length typed buffers;
- typed bounded writers;
- safe initialized-range tracking;
- Resource live-range calculation;
- conservative same-representation slot reuse;
- workspace backing and `dyn-stack` wrapper;
- storage report and estimated peak memory.

Do not implement cross-type raw arena packing.

Evidence:

- `DropSpy` tests;
- zero-sized and over-aligned types;
- panic and error during partial initialization;
- incompatible slots never alias;
- output borrowers prevent another mutable run;
- Miri coverage for unsafe boundaries.

## 5. Milestone 3 — Allocation policies

Implement orthogonal build options:

- `grow-and-measure` versus `reject-overflow`;
- `best-effort` versus `no-run-allocation`.

Add:

- observed-capacity reporting;
- explicit overflow errors;
- Unit strict-capability checks;
- documented warm-up boundary;
- counting global allocator test harness plus an adapter hook for additional allocation domains;
- bounded Debug event buffer.

Evidence:

- 1,000 steady-state strict runs with zero allocate, reallocate, and deallocate calls in every declared allocation domain;
- success, recoverable failure, fatal failure, and overflow paths;
- third-party helper calls, Resource reset/drop, and registered Debug sinks included in the measured boundary;
- strict build rejects dynamic or unresolved requirements and uninstrumented allocator paths.

## 6. Milestone 4 — YAML frontend

Use a span-preserving YAML syntax tree before typed normalization.

Implement:

- supported schema identifier;
- duplicate mapping-key detection;
- unknown-field rejection;
- source spans and YAML paths;
- Unit config decoding through Serde;
- bounds supplied by config, adapters, or host build options;
- parser depth and document-size limits;
- deterministic normalization.

Initial candidates are Serde and Saphyr. Pin reviewed versions and keep YAML-specific types outside core APIs.

Evidence:

- exact paths for unknown Unit, missing port, duplicate producer, type mismatch, unresolved bound, and cycle;
- aliases and merge-key policy documented and tested;
- parser fuzzing independent of Unit execution.

## 7. Milestone 5 — Headless navigation Quickstart

Implement one host binary with:

- ROS map decoder;
- binary obstacle inflation;
- compatible A* and Dijkstra Units;
- line-of-sight smoother;
- three YAML variants: A*, Dijkstra, and A* without smoothing.

The Quickstart must prove:

- implementation replacement;
- graph restructuring;
- fan-out from the cost map;
- bounded path and search workspace;
- strict headless runs after warm-up;
- host-owned Module lifecycle.

Prefer maintained libraries when their APIs support prepared storage. A third-party algorithm that allocates internally may run in best-effort mode first; strict support requires a measured adapter or a small allocation-controlled implementation.

## 8. Milestone 6 — Debug and Rerun adapter

Core Debug first provides text, DOT, Mermaid, timing, capacity, and storage reports without Rerun.

The optional Rerun adapter adds:

- map and cost-map views;
- raw and smoothed paths;
- Unit/Resource graph;
- timing and capacity metrics;
- fixed blueprint;
- live and recording modes.

Resource rendering is opt-in. Strict mode either disables it or uses an explicitly bounded implementation.

Evidence:

- disabling Debug does not change algorithm results;
- adapter failure follows documented policy;
- storage and timing reports identify their own overhead.

## 9. Milestone 7 — Hardening

Add:

- property tests for graph normalization and storage assignment;
- Miri for unsafe storage code;
- allocation tests on supported platforms;
- benchmarks for build time, no-op Unit overhead, bounded buffer writes, workspace allocation, and slot reuse;
- one ARM64 CI target;
- dependency license and supply-chain review;
- API documentation and runnable examples.

Benchmark before adding:

- cross-type raw packing;
- alternative arenas;
- custom allocators;
- graph libraries;
- small-vector or slab optimizations.

## 10. Optional showcase after V0

A nuScenes LiDAR showcase may follow the V0 Quickstart. It must remain outside default CI and must not drive core dependencies.

Potential Units:

- range filter;
- voxel downsample;
- two compatible ground-removal implementations;
- Euclidean clustering.

The showcase is useful for large bounded Resources and 3D visualization, but dataset preparation, licensing, and algorithm quality are separate from the V0 definition of done.

## 11. Definition of done

V0 is done when:

- the synthetic kernel proves the complete architecture contract;
- YAML diagnostics are actionable;
- the navigation Quickstart runs three graphs from one binary;
- the primary Unit API uses framework-provided outputs and workspace;
- strict steady-state no-allocation is automatically verified;
- Debug exposes graph, timing, capacity, and storage information;
- core crates remain independent of ROS, Rerun, datasets, and application frameworks;
- the implementation satisfies every acceptance item in the V0 specification.
