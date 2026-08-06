# ADR-0003: Framework-managed Resource storage

- **Status:** Accepted
- **Date:** 2026-08-04
- **Research:** [Storage planning and steady-state allocation](../research/storage-planning-and-steady-state-allocation.md)

## Context

Images, grids, point clouds, paths, detections, tensors, and algorithm workspaces often dominate the allocation and latency behavior of an algorithm module. If each Unit allocates its own outputs and temporary containers during every run, the framework cannot provide predictable memory use, reuse non-overlapping buffers, or verify a steady-state allocation budget.

A single general arena is not sufficient by itself. Resource values outlive one Unit invocation and have DAG-derived live ranges, while scratch workspace is temporary and commonly stack-like. Module outputs may also be retained by the host.

## Decision

### Logical Resource identity is separate from physical storage

Resource names and semantic types define dependency and diagnostic identity. Physical storage is an implementation assignment. Compatible Resources may reuse a slot when their live ranges do not overlap, while one Resource may use host-owned or adapter-provided storage without changing its identity.

Each run produces a new value for a logical Resource. That value is write-once within the run and becomes read-only after successful publication. Resetting prepared storage before a later run does not mutate a previously published Resource value; it prepares storage for the next run-local value.

### Resource descriptors own representation invariants

A Resource type descriptor owns properties that do not vary by producing Unit instance:

- semantic type and concrete Rust type;
- storage representation and adapter;
- element layout and alignment;
- memory class;
- initialization, reset, validation, and drop behavior.

A Unit descriptor declares port semantic types and computes only the size or capacity needed for each output from validated configuration and input bounds. Unit requirements do not repeat or override Resource representation invariants.

Requirements may be fixed, bounded, or dynamic. Strict execution accepts only fixed or bounded requirements.

### Module construction prepares storage

Module build resolves parsing and configuration into a validated intermediate representation before graph compilation and storage planning. Later stages operate on resolved Unit and Resource identities, typed bindings, and requirements rather than YAML values or unvalidated configuration.

Preparation then computes Resource live ranges, plans compatible output slots and workspaces, allocates storage, constructs Units, and may execute documented warm-up.

Preparation is an advanced lifecycle stage, not a new public model pillar.

### Unit execution uses pending outputs

The primary Unit execution API receives:

- read-only input views;
- typed writable handles for one pending output set;
- a bounded scratch workspace.

A Unit does not publish outputs individually. After `Unit::run` returns success, the framework validates representation, initialized ranges, logical lengths, and capacities for every declared output, then publishes the set as one group.

If execution or validation fails, initialized but unpublished values are dropped safely and no downstream Unit observes the set. When panic unwinding is enabled, the same pending-output guard drops initialized values before the executor marks the Module fatally failed. This publication boundary does not roll back Unit private state, external effects, or writes already made to host-provided storage.

A Unit must not retain input, output, or workspace borrows beyond the invocation. Returning newly allocated output payloads is not the primary V0 path. Convenience adapters may copy or own values outside strict execution.

### Typed storage first

V0 prioritizes typed value slots, fixed typed buffers, and bounded variable-length typed buffers. Initial slot reuse is conservative: same compatible representation, type, alignment, capacity, memory class, initialization and drop behavior, and non-overlapping live range.

Cross-type raw byte packing, globally optimal packing, device memory, and asynchronous lifetime tracking are deferred.

### Fixed structure and mutable runtime state

The compiled graph, resolved requirements, execution order, and storage plan are fixed for one Module instance.

Prepared storage contents, Unit private state, pending publication state, observed capacity peaks, failure state, and bounded diagnostics are runtime state and may change across runs. Implementations should keep the fixed compiled description separate from mutable runtime state even when the public API exposes a single `Module` type.

### Capacity and allocation policies

Host build options separate capacity behavior from the allocation guarantee:

- a development capacity policy may grow framework-managed buffers and record observed peaks;
- a production capacity policy rejects overflow instead of growing;
- the default allocation guarantee is best effort;
- an opt-in no-run-allocation guarantee requires no dynamic allocator operations in the prepared Module's declared allocation domains during steady-state `Module::run`.

Strict no-run-allocation includes framework code, Unit code, registered diagnostic sinks, and participating third-party calls within the run boundary. Unit and adapter descriptors declare every allocation domain they use. The initial CPU profile must at least instrument the Rust global allocator. A custom native, device, or adapter allocator must be instrumented or explicitly certified; otherwise the Unit or adapter is ineligible for the strict guarantee. Module construction, declared warm-up, and host work outside the call are excluded.

Certification is an explicit trusted assertion by the Unit, adapter, or host integrator that the declared domain is allocation-free during the run boundary. The certification source and covered domain are inspectable in the prepared Module description. Instrumentation can verify observed operations in declared domains, but neither instrumentation nor conformance tests can mechanically prove that arbitrary native code declared every allocator it may call. The strict guarantee therefore depends on the completeness and correctness of these trusted declarations.

The guarantee is enforced by build-time requirement checks, rejection of declared domains that are neither instrumented nor certified, automated allocator instrumentation, and negative conformance tests for known violations. Prepared Resource representations must also reset and drop without allocator activity on the run path. Capacity overflow returns a structured error and never falls back to allocation.

Public build APIs should use named presets, validated constructors, or otherwise prevent incompatible option combinations such as grow-and-measure with no-run-allocation.

### Scratch workspace

Scratch workspace is distinct from Resource storage. The implementation may use a mature caller-provided stack-workspace library behind a UnitCompose wrapper. This ADR does not mandate a particular crate.

### Module output lifetime

The allocation-friendly output API returns views borrowing prepared Module storage, preventing another mutable run, destruction, or storage reuse for that Module while those views exist. The host may still make a different prepared Module active while retaining the borrowed old Module.

A `run_into` API may use host-provided outputs. It preserves atomic logical Resource publication: Module outputs are valid only after the complete set succeeds and passes validation. It does not provide byte-level rollback for caller memory. On Unit error, validation error, or unwind, caller-provided storage may be partially initialized or mutated and must be treated as invalid until a later successful call.

An owned convenience result may allocate or copy and is outside the strict guarantee.

### Inspection and reporting

Read-only inspection separates fixed Module description from per-run reporting.

The Module description includes requirements, slot assignments, live ranges, and estimated peak memory. A run report includes observed capacity peaks, timing, failures, and allocation-profile violations.

Strict execution uses disabled or bounded run reporting. Resource visualization that retains or copies large values is not part of the strict path unless an adapter explicitly proves compliance.

## Consequences

### Benefits

- Unit authors receive typed output and workspace APIs instead of implementing ad hoc allocation.
- Resource representation has one source of truth, while Unit requirements describe only required size or capacity.
- Complete output publication is centralized instead of distributed across individual writers.
- Fixed compiled state and mutable runtime state have explicit ownership boundaries.
- Steady-state latency and memory behavior become measurable.
- The framework can reuse compatible storage across non-overlapping live ranges.
- Development can discover realistic bounds before production rejects growth.
- The public Unit/Resource/Module model remains unchanged.

### Costs

- Resource descriptors and storage adapters become more detailed.
- The implementation needs a pending-output state that tracks initialization before publication.
- Variable-size algorithms need explicit upper bounds for strict execution.
- Output lifetime becomes visible in host APIs.
- Third-party algorithms must expose workspace hooks, pre-size private state, or be excluded from strict mode.
- Strict certification relies on trusted declarations whose completeness cannot be proven mechanically for arbitrary native code.
- `run_into` callers must treat output storage as invalid after failure because byte-level rollback is not provided.
- Storage safety requires careful initialization, drop, error, panic, and alignment testing.

## Deferred

- managed persistent Resources;
- cross-language and external-buffer leases;
- GPU, pinned host, unified, or remote memory planning;
- parallel-executor storage reuse;
- asynchronous output lifetime;
- cross-type global packing;
- allocator-aware versions of every Rust collection;
- automatic bound inference for arbitrary Unit code.
