# V0 implementation plan

- **Status:** Active delivery plan
- **Date:** 2026-08-04
- **Normative baseline:** [V0 architecture specification](../specification/v0-architecture.md)

## Plan Ledger

- Plan status: ACTIVE
- Session scope: v0-implementation
- Parent plan: none
- Child plans: none
- Last updated: 2026-08-04
- Current slice: Milestone 1 graph compiler
- Next action: implement and verify resolved definitions, stable graph compilation, and description export
- Blocked on: none
- Do not touch from this session: optional post-V0 showcase work

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

## 2. Internal state boundaries

Keep the public API centered on `Module`, but implement explicit internal boundaries:

```text
ParsedDefinition
  -> ResolvedModule
  -> CompiledModule
  -> Prepared Module runtime
```

Recommended responsibilities:

- `ParsedDefinition` retains YAML source locations and syntax-level values;
- `ResolvedModule` contains validated configuration, typed Unit and Resource identities, port bindings, factories, and resolved requirement inputs;
- `CompiledModule` contains the fixed graph, stable execution order, live ranges, requirements, and storage plan;
- Module runtime contains Unit private state, prepared storage contents, pending output state, failure state, and bounded run reporting.

Later phases must not depend on YAML values or unvalidated Unit configuration. The exact Rust type names are replaceable, but the ownership boundaries are required implementation guidance.

## 3. Milestone 0 — Contract and API spike

Create three synthetic typed Units:

- `FixedImageFilter` — fixed-size output;
- `BoundedPointFilter` — variable logical length with fixed maximum capacity;
- `WorkspaceHeavyPlanner` — precise scratch requirement.

Prototype:

- treat the accepted panic, reload, `run_into`, YAML, and strict allocation-trust contracts in the ADRs and V0 specification as an implementation entry gate;
- a Cargo workspace with a provisional MSRV and Linux x86_64 baseline CI;
- typed inputs;
- typed pending-output value and buffer writers;
- complete-set validation and publication;
- caller-provided workspace;
- semantic type to concrete representation registry;
- Resource descriptors as the single source of representation invariants;
- borrowed Module outputs;
- `run_into`;
- recoverable and fatal Unit failures;
- fatal poisoning when a Unit panic unwinds through the executor;
- capacity overflow;
- validated build-option constructors or named presets;
- an explicit trust contract for Unit allocation capability and declared allocation domains.

Exit criteria:

- no string lookup inside the Unit execution hot path;
- no Unit-owned output allocation in the primary API;
- individual writers cannot publish Resources independently;
- partial initialization drops safely on Unit error, validation error, and unwind panic;
- an unwind panic leaves the Module fatally poisoned, so a later run is rejected; `panic=abort` terminates the process without a cleanup or poisoning guarantee;
- a later run can reset storage and create a new value for the same logical Resource without violating per-run write-once semantics;
- grow-and-measure cannot be combined with no-run-allocation in a usable build configuration;
- strict declarations and certifications are inspectable, and the API makes their trusted, non-mechanically-provable completeness explicit;
- dependency and API spikes freeze the exact MSRV, ARM64 target and compile/run expectation, dependency versions, feature set, and lockfile policy before Milestone 1;
- the APIs remain understandable without exposing raw pointers or allocator generics.

## 4. Milestone 1 — Graph compiler

Implement:

- Unit and Resource IDs;
- registries and descriptors;
- parsed and resolved Module Definition IRs;
- required-port validation;
- semantic and concrete type validation;
- producer-consumer derivation;
- duplicate-producer diagnostics;
- stable Kahn topological ordering;
- cycle diagnostics;
- fixed Module description export for DOT and Mermaid.

Use a small UnitCompose-owned graph representation first. A graph crate may be added only if it materially improves diagnostics or maintenance.

Evidence:

- source-order permutation tests;
- fan-out and fan-in;
- multiple independent roots;
- structural equality of normalized compiled graphs under source-order permutation;
- property tests for graph normalization, stable ordering, and cycle preservation;
- graph tests use programmatically constructed resolved definitions, and graph compilation operates only on resolved identities and bindings;
- negative graph fixtures.

## 5. Milestone 2 — Typed storage kernel

Implement:

- Resource descriptors that own representation, layout, alignment, memory class, initialization, reset, validation, and drop behavior;
- fixed value slots;
- fixed typed buffers;
- bounded variable-length typed buffers;
- typed pending-output writers;
- safe initialized-range tracking;
- one pending output set per Unit invocation;
- complete-set validation and group publication;
- pre-execution Module-input validation for names, semantic and concrete types, shape or capacity bounds, and prepared-plan compatibility;
- Resource live-range calculation;
- conservative same-representation slot reuse;
- workspace backing and `dyn-stack` wrapper;
- storage report and estimated peak memory.

Do not implement cross-type raw arena packing.

Evidence:

- `DropSpy` tests for successful publication, Unit error, validation error, and unwind panic;
- multi-output tests where a later output fails after an earlier output is initialized;
- zero-sized and over-aligned types;
- incompatible slots never alias;
- Resource representation invariants cannot be overridden by a Unit requirement;
- output borrowers prevent another mutable run;
- `run_into` preserves logical complete-publication behavior while tests prove that caller storage is invalid and may be partially mutated after Unit error, validation error, or unwind;
- an unwind panic after Unit private-state mutation drops pending values and leaves the Module fatally poisoned;
- invalid Module inputs are rejected before pending state is reset or Unit business code executes, and the Module remains reusable;
- property tests cover live-range calculation and conservative storage assignment, including Module outputs that remain live through the end of a run;
- Miri coverage for unsafe boundaries.

## 6. Milestone 3 — Allocation policies

Implement orthogonal behavior with validated construction:

- `grow-and-measure` versus `reject-overflow`;
- `best-effort` versus `no-run-allocation`;
- named presets or constructors that prevent incompatible combinations.

Add:

- observed-capacity reporting;
- explicit overflow errors;
- Unit strict-capability checks backed by declared allocation domains;
- an explicit trusted certification boundary for allocation domains that cannot be instrumented;
- documented warm-up boundary;
- a scoped counting global allocator harness plus adapter hooks for additional allocation domains;
- bounded run-report event buffer.

Evidence:

- 1,000 steady-state strict runs with zero allocate, reallocate, and deallocate calls in every declared allocation domain;
- success, recoverable failure, fatal failure, and overflow paths;
- third-party helper calls, Resource reset/drop, pending-output cleanup, and registered diagnostic sinks included in the measured boundary;
- strict build rejects dynamic or unresolved requirements and declared allocation domains that are neither instrumented nor certified;
- a deliberately allocating Unit and a deliberately uninstrumented adapter are rejected or produce an allocation-profile violation;
- Module descriptions expose each declared domain, its instrumentation or certification status, and the certification source;
- documentation and tests do not claim to prove that arbitrary native code declared every allocator it may call;
- allocator tests run in an isolated process or otherwise prove that unrelated test threads cannot create false positives or false negatives;
- invalid option combinations are rejected before preparation or cannot be represented by the public API.

## 7. Milestone 4 — YAML frontend

Use a span-preserving YAML syntax tree before typed normalization.

Implement:

- supported schema identifier;
- duplicate mapping-key detection;
- unknown-field rejection;
- source spans and YAML paths;
- Unit config decoding through Serde;
- conversion from parsed definition to resolved Module IR;
- bounds supplied by config, adapters, or host build options;
- parser depth and document-size limits;
- explicit rejection of YAML aliases and merge keys;
- deterministic normalization.

Initial candidates are Serde and Saphyr. Pin reviewed versions and keep YAML-specific types outside core APIs.

Evidence:

- exact paths for unknown Unit, missing port, duplicate producer, type mismatch, unresolved bound, and cycle;
- aliases and merge keys are rejected with actionable source paths;
- parser fuzzing independent of Unit execution;
- no YAML node or unvalidated config value reaches graph compilation or storage planning.

## 8. Milestone 5 — Headless navigation Quickstart

Implement one host binary with:

- ROS map decoder;
- binary obstacle inflation;
- compatible A* and Dijkstra Units;
- line-of-sight smoother;
- three YAML variants: A*, Dijkstra, and A* without smoothing;
- a host-owned reload path that builds and warms a replacement Module beside the active one and designates it active only between runs.

The Quickstart must prove:

- implementation replacement;
- graph restructuring;
- fan-out from the cost map;
- bounded path and search workspace;
- strict headless runs after warm-up;
- host-owned Module lifecycle;
- integration coverage for runtime rejection of missing, unknown, type-incompatible, or out-of-bound Module inputs before the first Unit executes;
- successful reload changes the active graph, while failed construction or warm-up leaves the old Module runnable;
- retained borrowed outputs keep the old Module alive and prevent its mutation or storage reuse without blocking activation of a different prepared Module.

Prefer maintained libraries when their APIs support prepared storage. A third-party algorithm that allocates internally may run in best-effort mode first; strict support requires a measured adapter or a small allocation-controlled implementation.

## 9. Milestone 6 — Inspection, reports, and Rerun adapter

Core diagnostics first provide two structures:

- a fixed Module description for text, DOT, Mermaid, requirements, capacity, and storage-plan reporting;
- a bounded per-run report for timing, completion, failure, overflow, and allocation-profile events.

The optional Rerun adapter adds:

- map and cost-map views;
- raw and smoothed paths;
- Unit/Resource graph;
- timing and capacity metrics;
- fixed blueprint;
- live and recording modes.

Resource rendering is opt-in. Strict mode either disables it or uses an explicitly bounded implementation.

The Rerun adapter is optional and does not gate the V0 definition of done. Core Module descriptions and bounded run reports remain required.

Evidence:

- disabling inspection adapters or run reporting does not change algorithm results;
- adapter failure follows documented policy;
- storage and timing reports identify their own overhead;
- fixed Module description does not depend on mutable run state.

## 10. Milestone 7 — Hardening

Add:

- cross-milestone regression and property-test suites for graph, storage, failure, and reload invariants;
- Miri for unsafe storage and pending-output code;
- allocation tests on supported platforms;
- benchmarks for build time, no-op Unit overhead, bounded buffer writes, pending-output publication, workspace allocation, and slot reuse;
- the ARM64 CI target and compile/run expectation frozen by Milestone 0;
- dependency license and supply-chain review;
- a terminology-consistency sweep across README, CONTRIBUTING, concepts, ADRs, specification, and API documentation;
- API documentation and runnable examples.

Benchmark before adding:

- cross-type raw packing;
- alternative arenas;
- custom allocators;
- graph libraries;
- small-vector or slab optimizations.

## 11. Optional showcase after V0

A nuScenes LiDAR showcase may follow the V0 Quickstart. It must remain outside default CI and must not drive core dependencies.

Potential Units:

- range filter;
- voxel downsample;
- two compatible ground-removal implementations;
- Euclidean clustering.

The showcase is useful for large bounded Resources and 3D visualization, but dataset preparation, licensing, and algorithm quality are separate from the V0 definition of done.

## 12. Test strategy and acceptance traceability

Tests live beside the milestone that establishes their invariant. Hardening expands platform coverage and regression depth; it does not postpone the first executable proof of graph or storage correctness.

```text
parse -> resolve -> compile -> prepare -> run
  |         |          |         |       |
 YAML     typed IR   stable DAG  slots   validated inputs
 spans    and bounds  and order  + work  pending outputs
                                          + failure state
```

Acceptance evidence maps to milestones as follows:

| V0 acceptance evidence | Owning milestone |
| --- | --- |
| 1-3: three definitions, implementation exchange, graph restructuring | 5 — Navigation Quickstart |
| 4-5: fan-out/fan-in and source-order independence | 1 — Graph compiler |
| 6: actionable YAML and graph errors | 1 and 4 — Graph compiler and YAML frontend |
| 7: semantic-to-concrete representation invariants | 0 and 2 — API spike and storage kernel |
| 8-11: pending outputs, workspace, group publication, error and unwind-panic drop safety | 0 and 2 — API spike and storage kernel |
| 12-14: live-range reuse, overflow, and valid build options | 2 and 3 — Storage kernel and allocation policies |
| 15-16: strict steady-state allocation evidence | 3 — Allocation policies |
| 17: borrowed outputs and `run_into` | 0 and 2 — API spike and storage kernel |
| 18: recoverable and fatal failure disposition | 0, 2, and 3 — Spike through strict failure paths |
| 19: independently testable Units | 0 and 5 — Synthetic Units and Quickstart Units |
| 20: Module description and run reports | 6 — Inspection and reports |
| 21: host-owned lifecycle | 5 — Navigation Quickstart |

Additional accepted contract behavior requires executable evidence:

- runtime Module-input validation belongs to Milestone 2, with Milestone 5 integration coverage;
- panic poisoning belongs to Milestones 0 and 2;
- reload success, failed-reload retention, between-run activation, and borrowed-old-Module retention belong to Milestone 5;
- `run_into` logical publication and invalid caller storage after failure belong to Milestones 0 and 2;
- YAML alias and merge-key rejection belongs to Milestone 4;
- allocator-domain declaration, instrumentation, certification, and violation detection belong to Milestone 3.

## 13. Delivery and distribution boundary

V0 delivers a buildable Rust workspace, library crates, runnable examples, API documentation, and CI evidence for Linux x86_64 plus the ARM64 target frozen after the API and dependency spike. The workspace records its frozen MSRV, supported target matrix, dependency licenses, unsafe surface, and feature combinations.

Publishing crates to crates.io, producing binary release archives, package-manager integration, and support for additional operating systems are not V0 definition-of-done requirements. A later release plan may add them after the public API and dependency versions stabilize.

## 14. Definition of done

V0 is done when:

- the synthetic kernel proves the complete architecture contract;
- fixed compiled state and mutable runtime state have explicit internal ownership boundaries;
- parsed YAML is converted to a resolved Module IR before graph compilation and storage planning;
- Resource descriptors are the single source of representation invariants;
- pending output sets provide complete validation and group publication;
- YAML diagnostics are actionable;
- the navigation Quickstart runs three graphs from one binary;
- the primary Unit API uses framework-provided outputs and workspace;
- unwind panic during Unit execution safely drops pending values and fatally poisons the Module, while `panic=abort` is explicitly outside cleanup and poisoning guarantees;
- borrowed outputs, `run_into` logical publication and failure-state caller storage, runtime input validation, and host-owned reload behavior have executable coverage;
- strict steady-state no-allocation is automatically verified;
- strict capability is granted only when every declared allocation domain is instrumented or explicitly certified, intentional violations are detected, and the trusted completeness boundary is explicit;
- Module descriptions and run reports expose graph, timing, capacity, and storage information;
- the workspace declares its MSRV, supported V0 targets, and non-publishing distribution boundary;
- core crates remain independent of ROS, Rerun, datasets, and application frameworks;
- the implementation satisfies every acceptance item in the V0 specification.

## 15. Execution preflight

- **Status:** Approved
- **Approved:** 2026-08-04
- **Route:** durable `$intuitive-flow` with one isolated `skill-runner` worker per milestone
- **Goal:** implement the complete UnitCompose V0 contract and prove every acceptance item

### Scope

- Milestones 0 through 7 in this plan;
- Cargo workspace, core, YAML, and debug crates;
- navigation Quickstart, contract tests, CI, API documentation, and examples;
- MSRV, ARM64 target, dependency versions, feature set, and lockfile policy frozen at the Milestone 0 exit gate;
- optional Rerun adapter as a non-gating workstream.

### Non-goals

- crates.io publishing, release archives, package managers, and additional operating systems;
- the nuScenes showcase;
- parallel execution, dynamic plugins, Python authoring, GPU planning, or distributed execution;
- byte-level rollback for `run_into`;
- mechanical proof that arbitrary native code declared every allocator it may call.

### Entity budget

- reuse the accepted ADRs, V0 specification, research, and Unit/Resource/Module model;
- remove a stable graph fingerprint as a compatibility surface and use structural equality instead;
- add only the crates, tests, examples, CI files, and one compact active-run capsule required by this plan;
- require re-approval for new public API semantics, extra crates, required Rerun support, additional platforms, or other scope expansion.

### Context package

Must read:

- `README.md` and `docs/README.md`;
- ADR-0001, ADR-0002, and ADR-0003;
- concept overview and terminology;
- V0 architecture specification and this plan;
- both research documents.

Read `CONTRIBUTING.md` and the architecture diagram when relevant. Avoid historical alternatives and optional showcase material unless a current decision requires them.

### Acceptance states

- **SUCCESS:** every milestone exit criterion, the definition of done above, and all 21 specification acceptance items pass;
- **BLOCKED_NEEDS_DECISION:** implementation would change an accepted public contract, scope boundary, storage guarantee, or supported platform;
- **BLOCKED_NEEDS_LOCAL_VALIDATION:** Milestone 0 freezes ARM64 execution as required but the selected environment cannot run it;
- **INTERMEDIATE_ONLY:** none;
- **No regressions:** preserve host lifecycle ownership, deterministic DAG behavior, complete pending-output publication, explicit strict-allocation trust boundaries, and core dependency isolation.

### Verification

Required deterministic gates after the workspace exists:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --exclude unit-compose-debug-rerun --all-targets -- -D warnings
cargo test --workspace --exclude unit-compose-debug-rerun
cargo doc --workspace --exclude unit-compose-debug-rerun --no-deps
cargo +nightly miri test -p unit-compose-core
cargo test -p unit-compose-core --test strict_allocation -- --test-threads=1
cargo +nightly fuzz run yaml_frontend
```

Build or run the exact ARM64 target frozen at Milestone 0. Required integration coverage includes the three navigation definitions, reload success and failure, borrowed old-Module lifetime, `run_into` failure-state caller storage, unwind panic, and known allocation violations.

Required product runs:

```bash
cargo run -p navigation-planning -- --module examples/navigation-planning/astar.yaml --strict
cargo run -p navigation-planning -- --module examples/navigation-planning/dijkstra.yaml --strict
cargo run -p navigation-planning -- --module examples/navigation-planning/astar-no-smoothing.yaml --strict
```

The optional Rerun adapter uses `cargo check -p unit-compose-debug-rerun` plus manual inspection and does not gate core V0 success.

### Execution control

The main session owns the root goal, canonical documents, milestone entry and exit gates, worker review, and final status. Each worker owns one bounded milestone, its assigned paths, and its evidence. Normal milestones use a 30-60 minute review cadence; hardening may use 60-120 minutes. A worker stops with a structured status and handoff and never marks the root goal complete.

The durable run maintains one compact active capsule at `docs/status/active/v0-implementation.md`. It replaces stale state instead of appending history and is removed from the active namespace after final reconciliation.

Execute with:

```text
/goal execute docs/plans/v0-implementation-plan.md with intuitive-flow
```
