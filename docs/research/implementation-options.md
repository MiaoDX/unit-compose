# Implementation options

- **Status:** Research recommendation, not dependency commitment
- **Target:** Rust-only sequential Alpha first

This document classifies candidate dependencies by the amount of UnitCompose semantics they would import.

## 1. Selection principle

Prefer small libraries that provide a replaceable mechanism. Be cautious with complete frameworks whose object model, lifecycle, and failure semantics would become difficult to remove.

Each dependency should be evaluated against:

- semantic fit;
- amount of unwanted model imported;
- inspectability and diagnostic support;
- determinism and stable ordering;
- host-thread and ROS integration;
- maintenance and compatibility cost;
- Python and device interoperability implications.

## 2. Recommended Alpha dependency class

These libraries solve local implementation problems without defining the UnitCompose public model.

| Capability | Candidate | Recommendation | Prototype question |
| --- | --- | --- | --- |
| Stable typed IDs | [`slotmap`](https://github.com/orlp/slotmap) | Strong candidate | Can custom key types cover Unit, Resource, publication, and storage IDs cleanly? |
| Deterministic maps/order | [`indexmap`](https://github.com/indexmap-rs/indexmap) plus ordered normalization | Strong candidate | Can Plan serialization and diagnostics remain stable across runs? |
| Internal dependency analysis | [`petgraph`](https://github.com/petgraph/petgraph) | Strong candidate, internal only | Does it provide cycle detection/topological utilities while allowing UnitCompose-native diagnostics? |
| Serialization | [`serde`](https://github.com/serde-rs/serde) | Strong candidate | Can all language-neutral Plan data remain explicit and versioned? |
| Schema generation | [`schemars`](https://github.com/GREsau/schemars) | Candidate | Is JSON Schema sufficient for Unit configuration frontends? |
| Errors | [`thiserror`](https://github.com/dtolnay/thiserror) | Strong candidate | Can internal errors map cleanly into phase/effect structured diagnostics? |
| Rich diagnostics | [`miette`](https://github.com/zkat/miette) or custom renderer | Candidate | Can declaration provenance and multiple source formats be presented consistently? |
| Structured tracing | [`tracing`](https://github.com/tokio-rs/tracing) | Strong candidate | Can Run/Unit/Resource IDs provide useful spans without defining business semantics? |
| Testing | `proptest`, snapshot testing | Strong candidate | Can normalization and invalid-composition behavior be generated systematically? |

Public APIs should say Unit, Resource, dependency, and binding even if `petgraph` uses graph/node terminology internally.

## 3. Parallel Scheduler candidates after Alpha

### Rayon

- Repository: [rayon-rs/rayon](https://github.com/rayon-rs/rayon)
- Strengths: mature work-stealing pool, scoped tasks, data-race-safe Rust APIs, custom pools.
- Risks: execution order is intentionally not semantic; nested pool behavior and host thread-local expectations require care.
- Recommendation: **leading mechanism candidate after the sequential reference is complete**.

UnitCompose would still own readiness, conflict checks, commit, and diagnostics. Rayon would execute selected ready work; it would not define the Plan semantics.

### Bevy ECS scheduler

- Repository: [bevyengine/bevy](https://github.com/bevyengine/bevy)
- Strengths: mature access tracking, conflicts, single- and multi-threaded scheduling.
- Risks: imports System/World lifecycle, type-unique Resource assumptions, deferred state semantics, and ECS terminology.
- Recommendation: **build a disposable comparison prototype, not the default plan**.

### Custom worker pool

- Strengths: exact control of readiness, failure stop, host integration, and diagnostics.
- Risks: substantial concurrency engineering, wakeup and work-stealing complexity, more code to validate.
- Recommendation: **do not build before Rayon and Bevy prototypes establish the gap**.

## 4. Python integration candidates

### PyO3

- Repository: [PyO3/pyo3](https://github.com/PyO3/pyo3)
- Role: host Python Units and expose Rust objects.
- Key questions:
  - interpreter ownership and shutdown;
  - GIL behavior under future parallel scheduling;
  - contained exception mapping;
  - lifetime of borrowed and retained views.

Recommendation: **leading Python binding candidate, deferred from Alpha**.

### Data interchange

| Representation | Reference | Use case | Limitation |
| --- | --- | --- | --- |
| Python buffer protocol / NumPy array interface | Python and NumPy standards | CPU contiguous or strided arrays | Ownership and mutability must be enforced by adapters |
| Arrow C Data Interface | [Apache Arrow](https://arrow.apache.org/docs/format/CDataInterface.html) | Columnar arrays across languages | Not a universal tensor/device format |
| DLPack | [dmlc/dlpack](https://github.com/dmlc/dlpack) | Device and tensor exchange | Stream synchronization and ownership transfer require precise handling |
| Custom typed Resource adapter | UnitCompose-owned | Domain objects and non-array values | More implementation work; semantics can be exact |

Recommendation: **Resource-type-specific adapters, not one universal zero-copy layer**.

## 5. Storage and allocation

Alpha should begin with correctness-first storage:

- owned values or immutable shared ownership;
- distinct storage for staged persistent successors;
- explicit host leases;
- no aggressive reuse while retained views exist.

Potential later candidates:

- typed arena or slab allocation;
- buffer pools by Resource representation;
- Arrow memory pools;
- device-specific allocator adapters.

Do not let allocator selection leak into Plan semantic identity.

## 6. Complete frameworks: reference versus dependency

| Project | Direct dependency recommendation | Reason |
| --- | --- | --- |
| Bevy ECS | Prototype only | Useful scheduler/access machinery but imports an ECS object model |
| Flecs | No | C/C++ entity-centric framework and different staging semantics |
| Salsa | No for Alpha | Pure on-demand incremental query assumptions conflict with arbitrary Unit state |
| Timely Dataflow | No for Alpha | Distributed streaming and progress model far exceeds bounded Run needs |
| Differential Dataflow | No | Collection-difference algebra does not match general Resources |
| Hydroflow | No for Alpha | Stream/operator model and public dataflow syntax |
| Lingua Franca | No | Coordination language and code generation; semantic reference |
| Temporal | No | Durable workflow service and replay architecture |
| Flink | No | Distributed stream processor |
| ROS 2 executor | Host integration only | UnitCompose must embed without taking over the application's callback model |

## 7. Proposed prototype matrix

### Prototype A — Minimal native kernel

Use small libraries only:

- slotmap;
- indexmap;
- petgraph internally;
- serde;
- thiserror/miette;
- tracing.

Measure code size, diagnostic quality, and ability to implement the Alpha tests.

### Prototype B — Bevy access/schedule experiment

Represent Units as Bevy Systems and named Resources through wrapper types or dynamic access metadata. Test:

- multiple named Resources of the same type;
- exact predecessor binding;
- staged Run commit;
- poisoned failure stop;
- diagnostic provenance.

Reject the approach if wrappers dominate or Bevy semantics leak into public APIs.

### Prototype C — Rayon parallel increment

After Alpha, use UnitCompose readiness to submit compatible Units into a dedicated Rayon pool. Compare successful results with the sequential reference and measure:

- launch overhead;
- host-thread behavior;
- failure stop latency;
- trace clarity;
- nested parallelism interaction with algorithm libraries.

## 8. Current recommendation

Start with **Prototype A**. Keep scheduling, Resource publication, and commit semantics owned by UnitCompose. Use focused external libraries for IDs, containers, diagnostics, and tracing.

Do not commit to a complete framework dependency until a prototype shows that it reduces total semantic and implementation complexity rather than moving it behind adapters.
