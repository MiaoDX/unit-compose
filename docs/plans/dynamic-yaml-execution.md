# Dynamic YAML execution

## Plan Ledger

- **Status:** DONE
- **Current slice:** complete through Phase 6
- **Next action:** none; implementation and required acceptance evidence are complete
- **No-touch boundary:** no unsafe code, new crate/runtime dependency,
  compatibility layer, or second runtime owner without re-approval

## Goal

Make YAML definitions control the executable Unit composition, bindings, and
stable execution order of a prepared Module. A host binary registers the Unit
implementations it supports; YAML may instantiate, configure, connect, add,
remove, or exchange those registered implementations without source changes.

This plan completes the V0 behavior already specified in
`docs/specification/v0-architecture.md`. It does not turn YAML into a native
code or plugin loader.

## Pre-implementation gap

Before this plan, the parser, typed configuration decoder, graph compiler,
topological ordering, requirement resolver, and storage planner were real and
configuration-driven, but the execution path was not:

- `UnitRegistry` contains port metadata but no executable factory;
- `FrontendRegistry` owns configuration decoding and requirements separately;
- `StoragePlan` reports assignments but does not allocate or operate runtime
  Resource slots;
- `Module<U>` owns and executes one statically selected Unit;
- showcases construct one domain-specific composite Unit and manually execute
  stages, with selected YAML differences mapped through `match` and `if`;
- several showcases reject every composition except a fixed hard-coded pipeline.

The result was a validated YAML graph beside a different, hand-written
execution graph. The completed implementation replaced every listed gap.

## Target contract

"Dynamic YAML execution" means all of the following:

1. YAML selects only Unit types already registered in the host binary.
2. Every YAML Unit instance is constructed through its registered factory.
3. The compiled graph's stable topological order is the sole Unit schedule.
4. Compiled dense handles, not Resource names, are used on the run path.
5. Inputs are read-only, outputs remain pending until complete validation, and
   workspace access is bounded to the current Unit.
6. The prepared graph and storage plan remain fixed for one Module instance.
7. Reconfiguration builds and warms a candidate Module, then the host swaps it
   between runs; there is no in-place graph mutation.
8. Development growth and strict reject-overflow/no-run-allocation modes keep
   their existing observable contracts.
9. Unit authors use a typed public authoring contract. Registration adapts it at
   build time into a private object-safe executable interface; Unit code never
   receives `Any`, Resource names, slot indexes, or storage internals.

Adding another registered Unit type may require new Unit implementation and
registration code. It must not require editing the YAML loader, graph compiler,
generic executor, or a central type-name `match`.

## Target data flow

```text
YAML source
    |
    v
parse + normalize + typed config decode
    |
    v
resolve descriptors, typed config and factory identities + validate bindings
    |
    v
compile Resource DAG + stable topological order
    |
    v
resolve bounds, output capacities, and Unit workspaces
    |
    v
compute live ranges + plan and allocate typed Resource slots and workspaces
    |
    v
construct one executable instance per YAML Unit
    |
    v
optional documented warm-up
    |
    v
Prepared Module
    |
    +-> validate host inputs
    +-> reset prepared run state
    +-> execute prepared Units in compiled order
    |      +-> read declared inputs
    |      +-> write one pending output set
    |      +-> validate and publish, or discard on failure/panic
    +-> return borrowed Module output views
```

## Architecture

### 1. Executable Unit registration

Extend the core-owned, frontend-neutral registry into the one canonical
registration path that binds these facts under one
versioned `UnitTypeName`:

- static input and output port descriptors;
- expected typed configuration identity;
- configuration requirement resolution;
- allocation capability;
- executable factory.

The YAML crate remains a frontend projection over that registration: it decodes
YAML into the registered typed configuration and preserves source spans. It
must not own a second independently keyed descriptor or requirement registry.
Core execution must not depend on a YAML AST or on `serde` values.

Unit authors implement a static typed multi-port contract with associated
`Inputs<'a>` and `Outputs<'a>` views plus bounded `UnitWorkspace<'a>`. Those
views may be named structs or tuples but expose only declared typed ports. The
registration adapter turns that public authoring contract into a private
object-safe executable interface at build time.

The factory receives validated typed configuration plus compiled, plan-scoped
port handles and constructs the adapter. Erasure and `TypeId` reconciliation
are registration/build concerns only. Registration must reject
descriptor/decoder/config/factory type mismatches before Module construction;
the run path must not downcast with an infallible `expect` assumption.

### 2. Typed runtime Resource storage

Turn storage descriptors from inspectable strings into executable typed storage
adapters. Each adapter owns allocation, reset, pending-output construction,
validation, publication, discard, and drop behavior for its representation.

Start with the V0 typed-slot model already selected by ADR-0003:

- fixed typed value;
- fixed typed buffer;
- bounded typed buffer with logical length;
- conservative reuse only for representation-compatible slots with
  non-overlapping live ranges.

Preserve workspace `unsafe_code = "forbid"` and design the runtime store in safe
Rust. Do not introduce a heterogeneous raw arena. If safe construction cannot
satisfy the aliasing and allocation contracts, stop before the runtime-store
phase and request separate approval for a narrowly isolated unsafe boundary,
its safety invariants, lint scope, and Miri coverage.

Logical Resource identity and publication state remain separate from the
physical slot assignment so safely reused storage never merges Resource
semantics.

Preparation must prove before Unit construction that every invocation's live
input slots, pending output slots, and workspace are mutually disjoint. Handles
carry prepared-plan provenance and validated representation identity. Module
inputs become run-scoped read-only bindings before reset/execution, and no
input, output, or workspace borrow may escape object-safe dispatch.

### 3. Prepared DAG executor

Replace the synthetic single-Unit execution owner with a prepared Module that
owns:

- immutable compiled graph and dense Resource/Unit identities;
- executable Unit instances arranged in compiled execution order;
- compiled input/output handles per Unit;
- prepared Resource slots and per-Resource publication state;
- prepared Unit workspaces;
- failure, capacity, timing, and bounded diagnostic state.

Use one object-safe dispatch per Unit invocation. Type and semantic checks occur
during build. The run path may use dense typed handles and encapsulated checked
or proven-safe slot access, but performs no string lookup.

On each Unit invocation the executor creates a pending output set. It publishes
all declared outputs only after the Unit returns success and every output
validates without changing logical publication state. Publication is then one
infallible group state transition. A pending-set guard owns every initialized
unpublished value and discards the entire set on error or unwind; no later Unit
runs.

Module run errors carry Module, Unit, port, Resource, cause, and failure
disposition context when applicable. Input-validation failures leave the Module
reusable, recoverable Unit failures permit a later run, and fatal failures or
unwind poison it. Strict capability is the conjunction of every executed Unit,
Resource adapter, reporting sink, and declared allocation domain, not a flag on
the executor alone.

### 4. Host inputs, outputs, and reload

Module inputs are validated against the compiled input plan before reset or
business execution. Module outputs borrow published prepared storage so a
second mutable run cannot start while an output view is retained. V0 also
requires `run_into` or an equivalent host-owned output API with atomic logical
publication and documented invalid caller storage after failure.

After Module preparation, the host resolves names once into typed, plan-scoped
`InputHandle<T>` and `OutputHandle<T>` values. A reusable input carrier binds
borrowed values through input handles. The borrowed output view retrieves
published values through output handles, and `run_into` binds host-owned output
storage through the same prepared identities. Handles validate prepared-plan
provenance and representation identity; the run path performs no Resource-name
lookup. Unit authors never receive this heterogeneous host carrier or `Any`.

Reload stays host-owned:

```text
read changed YAML -> build candidate -> optional warm-up -> activate between runs
        failure ----------------------------------------> retain current Module
```

Core does not watch files, choose reload timing, migrate Unit private state, or
replace an active Module in place.

## Pre-implementation gates

Before Phase 1:

1. capture a meaningful current composite-path latency and allocation baseline;
   the current ignored elapsed-time-nonzero benchmark is not an acceptance
   baseline;
2. record the public error envelope and fatal/recoverable poisoning behavior
   required by V0 before executor work begins; and
3. treat V0 specification section 18 as the normative acceptance matrix. The
   criteria in this plan supplement it and do not replace it.

### Pre-implementation evidence

Captured on 2026-08-18 in the repository's debug test profile with
`composite_execution_baseline` in the navigation quickstart target. Each sample
is 1,000 post-warm-up A* composite runs over the same demo grid, with the
existing global allocator probe enabled:

| Sample | Median | p95 | Allocation operations |
| --- | ---: | ---: | ---: |
| 1 | 266,426 ns | 280,988 ns | 0 |
| 2 | 269,403 ns | 281,681 ns | 0 |
| 3 | 262,598 ns | 280,638 ns | 0 |

The comparison baseline is the median of sample medians: **266,426 ns**. The
post-migration matched median must not exceed 293,069 ns (10 percent) without
explicit acceptance. Run the same ignored test command three times for the
post-change comparison.

The normative public failure envelope is fixed before executor work:

- build errors retain the parse/schema/registry/configuration/graph/type/bounds/
  requirements/storage/allocation/construction/warm-up cause and source location
  when available;
- run errors retain Module identity, Unit instance and type, port and Resource
  when applicable, a structured cause, recoverable/fatal disposition, and the
  available bounded trace;
- capacity errors retain Unit plus Resource/workspace identity, required and
  prepared capacities, and active policy;
- input-validation failure is reusable, explicit recoverable Unit failure is
  reusable, fatal Unit failure and unwind poison the Module, and `panic=abort`
  has no executor cleanup or poisoning guarantee.

V0 section 18 remains the authoritative 21-item matrix. Execution tracks it by
item number; Phase 0 establishes the matrix, Phases 1-5 add evidence, and Phase
6 must reconcile every item to an executable test or example before completion.

### Post-migration performance evidence

The renamed `dynamic_execution_benchmark` ran the same 1,000 post-warm-up A*
runs with strict allocation probing. Three samples measured 255,571 ns,
270,200 ns, and 268,388 ns median latency, with p95 values of 267,470 ns,
281,764 ns, and 278,908 ns. The median of medians is **268,388 ns**, a 0.74%
increase over the 266,426 ns baseline and below the 293,069 ns acceptance
ceiling. Every sample observed zero allocate, reallocate, and deallocate calls.

## Implementation phases

### Phase 1: Canonical registration and executable conformance fixture

Extend the core registry with the executable factory, typed configuration
identity, and Resource adapter identity. Make the YAML frontend consume the
same registration. Compile names and ports into dense `UnitIndex`,
`ResourceIndex`, input handles, and output handles.

Before changing showcases, define a small in-repo fixture vocabulary with
registered source/map/join/fail Units and scalar/bounded Resources. Use it to
lock registration, factory, binding, execution, contextual error, and failure
disposition interfaces. Registration and Module build reject all cross-registry
type drift without a run-time downcast assertion.

**Stop gate:** Do not proceed if the interface requires YAML knowledge in core,
string lookup on every run, domain-specific executor branches, or more than one
registration owner. A newly registered compatible Unit must be selectable by
type name without editing generic build or execution code.

### Phase 2: Runtime Resource store

#### Phase 2A: Correctness before reuse

Add safe executable adapters around the existing typed storage implementations
and allocate one physical slot per logical Resource. Implement reset, pending
output, validate-all-then-publish, discard, drop, and capacity policy. Prove the
preparation disjointness and handle-provenance invariants before constructing
Units.

#### Phase 2B: Conservative compatible reuse

Enable compatible slot reuse without changing logical handles or publication
state only after Phase 2A passes. Inclusive live ranges must prevent one
invocation's inputs and pending outputs from sharing a physical slot.

**Stop gate:** Fixed and bounded Resources pass success, overflow, partial write,
Unit error, validation error, unwind, reset, reuse, and drop tests with no
unpublished observation or double drop. The workspace still forbids unsafe
code; needing unsafe stops the phase for a separate reviewed decision.

### Phase 3: Canonical builder and sequential DAG executor

Add the one canonical frontend-neutral resolved-definition-to-Module builder,
then make YAML use it. Construct every Unit through its factory, execute exactly
the compiled stable order, route Resources through compiled handles, stop on
first failure, and expose borrowed outputs plus `run_into`. Integrate bounded
timing and allocation probes at actual Unit boundaries and aggregate strict
capability across all runtime participants.

**Stop gate:** A fan-out/fan-in YAML graph executes correctly and produces the
same result regardless of YAML declaration order. The generic path has a
recorded latency comparison against the pre-implementation composite baseline
and no unexplained run allocation regression before showcase migration starts.

### Phase 4: Navigation vertical slice

Split the navigation composite into independently registered decoder,
inflation, A*/Dijkstra, and smoother executable Units. Run `astar.yaml`,
`dijkstra.yaml`, and `astar-no-smoothing.yaml` through the generic Module.

Delete navigation type-name dispatch, smoothing-presence branching in the
composition owner, manual stage timing, and graph-shape simulation once the
generic executor replaces them.

**Stop gate:** Swapping planner type and adding/removing smoothing changes the
executed composition and result without source changes or a central type-name
branch.

### Phase 5: Remaining showcase migration and stale-path deletion

#### Phase 5A: Stateless registration showcases

Migrate image registration and point-cloud registration to real registered
executable Units where their YAML declares multiple Units. Delete their
fixed-pipeline validators, composite execution owners, and duplicated
per-example build pipelines after their generic paths pass.

#### Phase 5B: Stateful LiDAR gate

Migrate LiDAR SLAM separately because Unit private state, recovery disposition,
and warm-up have a larger failure surface. Delete its synthetic composition
path only after repeated-run and replacement behavior passes.

Keep one canonical YAML-to-Module builder used by examples and host adapters.
Delete documentation wording that refers to synthetic behavior once each
showcase migration is complete.

**Stop gate:** No showcase reports a multi-Unit YAML graph while executing a
single domain composite Unit, and the generic executor has no domain-specific
knowledge.

### Phase 6: Strictness and acceptance closure

Run the complete normative V0 section 18 acceptance matrix, close inspection
and diagnostic parity, measure build/run overhead against the captured
baseline, and update README/architecture/status documentation to distinguish
dynamic composition, host-owned replacement, and plugins.

**Stop gate:** The acceptance criteria below pass in development and strict
modes, including 1,000 post-warm-up zero-allocation runs.

## Acceptance criteria

V0 specification section 18 is normative. The following criteria highlight the
dynamic-execution mapping and add plan-specific deletion and performance gates:

- One binary executes at least three distinct YAML Module Definitions.
- Two compatible Unit implementations are exchanged only through YAML.
- Adding or removing one Unit changes the executed DAG without source changes.
- Fan-out and fan-in execute correctly through framework Resource storage.
- YAML declaration order does not change dependencies, stable order, or result.
- Every executable YAML Unit is produced by its registered factory.
- The generic run path performs no Resource-name lookup or domain type-name
  dispatch.
- Fixed and bounded outputs use framework-provided pending storage.
- Multiple outputs publish atomically; failure and unwind expose none of them.
- Capacity overflow in reject-overflow mode never grows storage.
- Compatible storage slots are reused only across non-overlapping live ranges.
- Recoverable errors permit a later run; fatal errors and unwind poison the
  Module.
- Borrowed outputs prevent a second mutable run of that Module.
- `run_into` supports host-owned output storage and preserves atomic logical
  publication while documenting invalid caller storage after failure.
- Run errors carry Module, Unit, port, Resource, cause, and disposition context
  when applicable.
- Failed build or warm-up leaves the current host Module runnable.
- After warm-up, 1,000 strict runs show zero allocate, reallocate, and
  deallocate operations in every declared allocation domain.
- Inspection reports the same graph, bindings, requirements, storage, and Unit
  timing boundaries that the runtime actually executes.

### V0 section 18 evidence matrix

| Item | Executable evidence |
| ---: | --- |
| 1 | `deterministic_algorithms_and_yaml_variants_execute_end_to_end` loads the three navigation definitions in one binary. |
| 2 | The same test exchanges registered A* and Dijkstra factories through YAML. |
| 3 | `graphs_prove_exact_stages_inspection_outputs_and_real_cost_map_fan_out` proves the 4/4/3-Unit DAGs, including optional smoothing. |
| 4 | `object_safe_fixture_executes_stable_fan_in_and_discards_failure_output` and the image/point-cloud YAML tests execute fan-out and fan-in. |
| 5 | `source_order_permutations_normalize_to_structural_equality` and `normalization_is_independent_of_mapping_and_unit_source_order`. |
| 6 | YAML frontend rejection and source-path tests cover aliases, merge keys, unknown Units, ports, producers, types, bounds, and cycles. |
| 7 | Registration/type tests prove canonical typed configuration, semantic/concrete agreement, and descriptor-owned representation invariants. |
| 8 | Runtime slot tests exercise framework-provided fixed and bounded pending outputs. |
| 9 | `isolated_dynamic_strict_allocation_conformance` executes a declared 64-byte Unit workspace. |
| 10 | `fixed_buffer_group_validation_and_unwind_drop_all_pending_values` proves grouped validation and publication. |
| 11 | The same test proves Unit-error, validation-error, and unwind cleanup/drop behavior. |
| 12 | Storage-kernel planner/property tests prove reuse only for compatible disjoint live ranges. |
| 13 | Runtime and navigation overflow tests prove structured reject-overflow errors without growth. |
| 14 | `BuildOptions` and strict capability tests reject unusable strict combinations. |
| 15 | Allocation harness and navigation tests prove 1,000 post-warm-up strict zero-operation runs. |
| 16 | Allocation harness tests cover success, recovery, poison, overflow reporting, reset/drop, unsupported/certified domains, and a detected violation. |
| 17 | The `Module::output` compile-fail doctest proves borrow exclusion; `dynamic_run_into_tracks_host_storage_validity` proves host storage validity. |
| 18 | `dynamic_failure_disposition_controls_poisoning` proves recoverable, fatal, unwind, and poisoned behavior. |
| 19 | Registered fixture tests plus focused image, point-cloud, navigation, and LiDAR algorithm tests exercise Units independently of host binaries. |
| 20 | Description, timing, bounded-reporting, and inspection product tests export the executed graph, requirements, storage plan, and Unit timings. |
| 21 | Navigation reload/replacement tests prove successful swap, failed candidate retention, and old borrowed output lifetime. |

### Product and manual evidence

All three strict navigation definitions ran successfully: A* and Dijkstra
executed four Units and returned three path points; A* without smoothing
executed three Units and returned 38 points. Mermaid inspection and timed
Mermaid output showed the compiled bindings and actual Unit timing boundaries.
The image run reported 283 matches, 200 inliers, 0.852115 pixel RMSE, and a
0.7067 inlier ratio. Point-cloud registration processed 4,096 points in 37
iterations and reduced RMSE from 0.435053 to 0.023408. The 480-frame LiDAR run
completed successfully.

The four saved recordings were nonempty (`navigation.rrd`,
`image-registration.rrd`, `point-cloud-registration.rrd`, and
`lidar-slam.rrd`) and passed `rerun rrd verify --check-footers false`; these
save routes produce loadable streaming RRDs without footer manifests. Manual
screenshots showed coherent navigation, registration, point-cloud alignment,
and reconstructed-room LiDAR scenes with metrics and Unit timings. Under Xvfb,
Rerun's Vulkan renderer displayed a 40,000-pixel surface-size notification and
panicked during teardown after each screenshot was saved; the recordings loaded
correctly and the notification did not obscure the primary scenes.

## Verification

Required test layers:

1. registration unit tests for duplicate names and config/factory type drift;
2. graph-to-dense-handle tests for bindings and stable order;
3. Resource adapter tests for initialization, reset, capacity, publication,
   discard, panic, reuse, and drop;
4. executor integration tests for linear, branch, fan-out/fan-in, empty-input,
   failure, and poison/recovery graphs;
5. YAML frontend tests retaining actionable source diagnostics;
6. navigation end-to-end tests proving actual algorithm and topology changes;
7. host-style replacement tests retaining the current Module on candidate
   failure;
8. allocator-instrumented strict tests and hardening benchmarks;
9. Miri or equivalent focused checks only if a separately approved unsafe
   storage boundary is introduced.

Baseline commands:

```bash
cargo test --workspace
cargo test -p unit-compose-core --test graph_compiler
cargo test -p unit-compose-core --test storage_kernel
cargo test -p unit-compose-yaml --test frontend
cargo test -p navigation-planning --test quickstart
```

Add focused dynamic-executor and runtime-storage test targets as their modules
are introduced. A benchmark result is evidence, not a replacement for behavior
or allocator assertions.

## Failure modes and controls

| Failure mode | Control |
| --- | --- |
| Decoder and factory disagree on config type | One canonical registration identity and build-time `TypeId` check |
| Factory requests an undeclared or wrongly typed port | Compiled typed binding validation before construction |
| Input/output slots alias during one invocation | Preparation proves disjoint live inputs, pending outputs, and workspace before Unit construction |
| Partial output leaks after error or panic | Validate every output before one infallible publication transition; pending-set guard discards the group |
| Reused slot double-drops or exposes stale data | Logical publication state plus adapter-owned reset/drop tests |
| Dynamic dispatch introduces run allocation | Prebuilt trait objects and handles plus allocation probes |
| One participant invalidates strict mode | Aggregate capability across Units, adapters, sinks, and allocation domains at build time |
| Reload destroys a good active Module | Build/warm candidate fully before host activation |
| Inspection differs from execution | Module description generated from the exact prepared runtime identities |
| Migration preserves two runtime owners indefinitely | Phase stop gates require deletion of each migrated composite path |

## Non-goals

- dynamic native shared-library loading or a stable plugin ABI;
- Python, WASM, or cross-language Unit authoring;
- in-place mutation of an active graph or storage plan;
- automatic Unit private-state migration across reload;
- framework-owned file watching or reload scheduling;
- parallel or asynchronous Unit execution;
- managed persistent Resources or writable shared Resources;
- heterogeneous raw storage packing or global optimal packing;
- GPU, pinned, unified, remote, or cross-process memory planning;
- automatic capacity inference from arbitrary Unit code.

## Migration and compatibility

Replace the current generic `Module<U>` directly with the real dynamic
`Module`. Change all in-repo callers as part of the owning migration phases.
There is no public compatibility wrapper, alias, parallel `PreparedModule`, or
deprecation window. Temporary internal scaffolding must remain private and be
deleted by its phase stop gate. The composite showcase Units are implementation
scaffolding, not a second supported execution architecture.

## Resolved approval decisions

Approved on 2026-08-18:

1. **Dynamic host I/O:** typed plan-scoped input/output handles, a reusable
   input carrier, heterogeneous borrowed output access, and V0 `run_into`.
2. **Typed Unit authoring:** public typed multi-port input/output views adapted
   at build time into a private object-safe executor.
3. **Compatibility:** the dynamic type is the final public `Module`; replace
   `Module<U>` and its callers directly with no backward-compatibility surface.
4. **Safety default:** preserve safe-only storage under workspace
   `unsafe_code = "forbid"`; unsafe remains a separately reviewed future stop
   gate only if implementation evidence proves it necessary.

## Approval gate

The whole plan and its phased stop gates are approved for execution under the
preflight below. Phase 1 is the starting point, not a narrowed replacement for
the remaining approved scope.

## Execution preflight

Preflight status: COMPLETE

Task source: approved plan plus user direction to make YAML control actual
runtime behavior with no backward-compatibility surface

Canonical source: `docs/plans/dynamic-yaml-execution.md`

Route: durable `$intuitive-flow`

Goal: Replace the synthetic single-Unit runtime with the safe, typed,
registry-driven dynamic Module described by this complete six-phase plan, then
prove that YAML changes the executed DAG and all V0 acceptance evidence passes.

Scope:

- execute Phases 1 through 6 in order, including both Phase 2 and Phase 5
  subphases;
- establish the pre-change composite latency/allocation baseline before
  changing runtime behavior;
- implement the canonical registry, typed authoring adapter, safe Resource
  store, dynamic builder/executor, host I/O handles, borrowed outputs, and
  `run_into`;
- migrate navigation, image registration, point-cloud registration, and LiDAR
  SLAM to the generic executor;
- delete the old generic `Module<U>`, duplicate registry/build owners,
  composite execution paths, fixed-pipeline simulation, and manual timing
  ownership as their replacements land; and
- close strict-mode, inspection, documentation, and V0 section 18 evidence.

Non-goals: native plugins or shared-library loading; compatibility wrappers or
deprecation aliases; a second public Module type; in-place reload; framework
file watching; parallel or async execution; persistent/shared writable
Resources; raw heterogeneous arenas; GPU/remote storage; automatic capacity
inference; unrelated cleanup.

Entity budget:

- reuse=existing graph compiler and stable ordering, `ValueStorage` and
  `BoundedStorage`, storage planner, Resource descriptors, diagnostics and
  allocation harness, YAML parser/source spans, example YAML/hosts, and current
  behavior tests;
- remove/merge=`FrontendRegistry` as an independent owner, `Module<U>`, domain
  composite execution owners, fixed-pipeline validators, duplicate example
  builders, graph-shape simulation, and manual stage timing superseded by the
  generic executor;
- new=core-owned executable registration fields, private object-safe Unit
  adapter, plan-scoped typed host handles/carriers, safe runtime Resource store,
  dynamic Module builder/executor, and focused runtime-storage/dynamic-executor
  tests and benchmark evidence; each is required by the accepted V0 contract;
- expansion triggers=stop for re-approval before adding unsafe code, a new
  crate or runtime dependency, a compatibility layer, a second runtime owner,
  plugins, parallelism, device/external storage, or any public contract that
  differs from the resolved decisions above.

Context:

- must-read=this plan; `docs/specification/v0-architecture.md`;
  `docs/adr/0002-configuration-driven-resource-dag.md`;
  `docs/adr/0003-framework-managed-resource-storage.md`;
  `crates/unit-compose-core/src/{lib.rs,graph.rs,storage.rs}`;
  `crates/unit-compose-yaml/src/lib.rs`; the four example `src/lib.rs` files
  and their YAML definitions;
- useful=`docs/concepts/{overview.md,terminology.md}`; existing core/YAML tests;
  navigation quickstart tests; implementation showcase docs; allocation test
  harness;
- avoid-unless-needed=research notes, git history, generated demo assets,
  Rerun binary assets, and unrelated crates or documentation.

Acceptance:

- SUCCESS=all six phase stop gates and every item in V0 specification section
  18 pass; one binary executes at least three materially different YAML DAGs;
  exchanging, adding, or removing a registered Unit changes actual execution
  without generic-code edits; fan-out/fan-in, atomic publication, typed host
  I/O, borrowed outputs, `run_into`, contextual errors, poison/recovery,
  replacement, inspection parity, and 1,000-run strict allocation behavior are
  executable evidence; all four showcases run through the generic executor;
  stale synthetic owners are absent; and docs describe only the real runtime;
- SUCCESS performance=compare three matched pre/post composite benchmark
  samples in the same environment; no unexplained median steady-state latency
  regression greater than 10 percent and no strict run allocation regression;
  a larger regression requires explicit acceptance before completion;
- BLOCKED_NEEDS_DECISION=any need for unsafe code, scope expansion, a new
  crate/runtime dependency, a different public contract, or acceptance of a
  performance regression over the gate;
- BLOCKED_NEEDS_LOCAL_VALIDATION=required showcase data, saved Rerun artifact,
  or product-run validation cannot be exercised in the execution environment;
- INTERMEDIATE_ONLY=none; phase commits are checkpoints, not completion;
- No regressions=YAML diagnostics and source paths, stable graph ordering,
  bounds/capacity policy, allocator guarantees, failure semantics, inspection
  formats, host-owned replacement, example results, and workspace-wide tests.

Verification:

- deterministic=`cargo fmt --all -- --check`;
  `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`;
  `cargo test --workspace --all-targets --all-features --locked`;
  `cargo test --doc --workspace --all-features --locked`;
  focused `graph_compiler`, `storage_kernel`, `frontend`, new runtime-storage,
  new dynamic-executor, navigation quickstart, strict allocator, panic/drop,
  replacement, and compile-fail borrow tests;
- integration=`cargo test -p unit-compose-core --test hardening_benchmarks --
  --ignored --nocapture` before and after runtime replacement with the new
  composite baseline assertions; V0 section 18 conformance matrix; stale-symbol
  searches proving no reachable `Module<U>`, duplicate frontend registry,
  fixed-pipeline validator, composite executor, or central Unit type-name
  dispatch remains;
- product-run=run navigation with `cargo run -p navigation-planning --locked --
  --module examples/navigation-planning/astar.yaml --strict`, repeated for
  `dijkstra.yaml` and `astar-no-smoothing.yaml`; run
  `scripts/fetch-showcase-data.sh`; run
  `cargo run -p image-registration --locked -- --module
  examples/image-registration/image-registration.yaml --run`; run
  `cargo run -p point-cloud-registration --locked -- --module
  examples/point-cloud-registration/point-cloud-registration.yaml --run`; run
  `cargo run -p lidar-slam --bin lidar-slam --locked -- --module
  examples/lidar-slam/lidar-slam.yaml --run`; and run navigation `--inspect
  mermaid` plus `--timed-mermaid` after migration;
- local-live-manual=save `target/navigation.rrd`, `target/image-registration.rrd`,
  `target/point-cloud-registration.rrd`, and `target/lidar-slam.rrd` using each
  binary's `cargo run -p <package> --features rerun --locked -- --module
  <yaml> --rerun-save <artifact>` route (`--bin lidar-slam` for LiDAR); verify
  each artifact is nonempty and opens with coherent graph/results/timing
  content; if data fetch, Rerun build, or visual inspection is unavailable, report
  `BLOCKED_NEEDS_LOCAL_VALIDATION` rather than success;
- optional=additional sanitizer/Miri checks for safe code; Miri becomes required
  only if a separately approved unsafe boundary is introduced.

Execution: main=root session owns the durable goal, phase ordering, stop-gate
judgment, atomic commits, worktree safety, final acceptance, and complete or
blocked status

Worker: none by default; shared core architecture stays under the root session

Worker-goal: none

To execute: `/goal execute docs/plans/dynamic-yaml-execution.md with intuitive-flow`

Optional tracking: none

Approval: invoking the `To execute` command in the new context approves this
preflight; edits request revision.
