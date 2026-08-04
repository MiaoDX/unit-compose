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

### Units declare requirements before execution

A Unit descriptor declares or computes from validated configuration and input bounds:

- each output's concrete representation, size, alignment, capacity, and memory class;
- the Unit's scratch workspace requirement;
- whether the Unit supports the strict no-run-allocation guarantee.

Requirements may be fixed, bounded, or dynamic. Strict execution accepts only fixed or bounded requirements.

### Module construction prepares storage

Module build includes a preparation stage that resolves requirements, computes Resource live ranges, plans compatible output slots and workspaces, allocates storage, constructs Units, and may execute documented warm-up.

Preparation is an advanced lifecycle stage, not a new public model pillar.

### Unit execution uses provided storage

The primary Unit execution API receives:

- read-only input views;
- typed writable output handles;
- a bounded scratch workspace.

A Unit publishes all outputs only after successful completion and validation. It must not retain input, output, or workspace borrows beyond the invocation.

Returning newly allocated output payloads is not the primary V0 path. Convenience adapters may copy or own values outside strict execution.

### Typed storage first

V0 prioritizes typed value slots, fixed typed buffers, and bounded variable-length typed buffers. Initial slot reuse is conservative: same compatible representation, type, alignment, capacity, memory class, and non-overlapping live range.

Cross-type raw byte packing, globally optimal packing, device memory, and asynchronous lifetime tracking are deferred.

### Capacity and allocation policies

Host build options separate capacity behavior from the allocation guarantee:

- a development capacity policy may grow framework-managed buffers and record observed peaks;
- a production capacity policy rejects overflow instead of growing;
- the default allocation guarantee is best effort;
- an opt-in no-run-allocation guarantee requires no dynamic allocator operations in the prepared Module's declared allocation domains during steady-state `Module::run`.

Strict no-run-allocation includes framework code, Unit code, registered Debug sinks, and participating third-party calls within the run boundary. The initial CPU profile must at least instrument the Rust global allocator. A custom native, device, or adapter allocator must be instrumented or explicitly certified; otherwise the Unit or adapter is ineligible for the strict guarantee. Module construction, declared warm-up, and host work outside the call are excluded.

The guarantee is enforced by build-time requirement checks and automated allocator instrumentation. Prepared Resource representations must also reset and drop without allocator activity on the run path. Capacity overflow returns a structured error and never falls back to allocation.

### Scratch workspace

Scratch workspace is distinct from Resource storage. The implementation may use a mature caller-provided stack-workspace library behind a UnitCompose wrapper. This ADR does not mandate a particular crate.

### Module output lifetime

The allocation-friendly output API returns views borrowing prepared Module storage, preventing another mutable run while those views exist. A `run_into` API may use host-provided outputs.

An owned convenience result may allocate or copy and is outside the strict guarantee.

### Debug behavior

Debug reports requirements, slot assignments, estimated peak memory, observed capacity peaks, and allocation-profile violations.

Strict execution uses disabled or bounded Debug recording. Resource visualization that retains or copies large values is not part of the strict path unless an adapter explicitly proves compliance.

## Consequences

### Benefits

- Unit authors receive typed output and workspace APIs instead of implementing ad hoc allocation.
- Steady-state latency and memory behavior become measurable.
- The framework can reuse compatible storage across non-overlapping live ranges.
- Development can discover realistic bounds before production rejects growth.
- The public Unit/Resource/Module/Debug model remains unchanged.

### Costs

- Unit descriptors and adapters become more detailed.
- Variable-size algorithms need explicit upper bounds for strict execution.
- Output lifetime becomes visible in host APIs.
- Third-party algorithms must expose workspace hooks, pre-size private state, or be excluded from strict mode.
- Storage safety requires careful initialization, drop, panic, and alignment testing.

## Deferred

- managed persistent Resources;
- cross-language and external-buffer leases;
- GPU, pinned host, unified, or remote memory planning;
- parallel-executor storage reuse;
- asynchronous output lifetime;
- cross-type global packing;
- allocator-aware versions of every Rust collection;
- automatic bound inference for arbitrary Unit code.
