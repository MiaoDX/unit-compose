# Storage planning and steady-state allocation

- **Status:** Current research basis
- **Research date:** 2026-08-04
- **Purpose:** Identify proven patterns for graph-scoped data, buffer reuse, workspace declaration, and predictable steady-state allocation.

This document summarizes primary sources and then states UnitCompose-specific conclusions. It is not a compatibility promise with any referenced system.

## 1. Overall finding

No reviewed system is a direct drop-in implementation of UnitCompose's Resource model. The useful pattern is consistent across domains:

1. keep graph-level logical values separate from physical storage;
2. verify or prepare the graph before steady-state execution;
3. make size, capacity, alignment, and lifetime visible;
4. separate published data buffers from per-operator scratch;
5. provide an easy dynamic-development path and a bounded-production path;
6. treat retained external outputs as part of storage lifetime;
7. measure every declared allocation domain rather than infer behavior from API shape alone.

This supports framework-managed typed Resource storage plus caller-provided Unit workspace. It does not justify importing a complete vision, media, inference, or ECS framework.

## 2. OpenVX: graph-scoped virtual data

The OpenVX specification requires one writer for a graph data object and defines virtual data objects as graph-scoped intermediates. Their live content is limited to graph execution; dimensions and format may be resolved during graph verification; the implementation may avoid allocating them, create sub-objects, or place them in inaccessible memory.

Primary source:

- [OpenVX 1.3.1, Virtual Data Objects](https://registry.khronos.org/OpenVX/specs/1.3.1/html/OpenVX_Specification_1_3_1.html)

### UnitCompose implication

- A Resource should remain a logical graph identity rather than an allocation.
- Intermediate Resource visibility should be limited, because unrestricted external access extends lifetime and blocks reuse.
- Build-time verification is the right place to resolve bounds and storage requirements.
- UnitCompose should not copy OpenVX's opaque object API or vision-specific type system.

## 3. GStreamer: negotiated buffer pools

GStreamer's buffer-pool design reduces allocation overhead and supports zero-copy transfer. Its allocation query allows elements to negotiate a pool, buffer size, minimum and maximum buffer count, allocator, alignment, padding, metadata, and preallocation.

Primary source:

- [GStreamer bufferpool design](https://gstreamer.freedesktop.org/documentation/additional/design/bufferpool.html)

### UnitCompose implication

- Capacity, alignment, allocator, and external retention are first-class storage concerns.
- A Resource producer should not unilaterally allocate without considering consumers and host outputs.
- UnitCompose's static DAG permits simpler build-time planning instead of runtime capability negotiation.
- A fixed pool of identical blocks is useful for some adapters but is not a universal typed Resource representation.

## 4. NVIDIA Holoscan: prototyping versus predictable pools

Holoscan documents an `UnboundedAllocator` for easy prototyping and a `BlockMemoryPool` that preallocates equally sized blocks and reuses them across operator `compute` calls. Current documentation explicitly recommends fixed pools for predictable workloads with known requirements while noting that stream-ordered allocators can still grow or block when their pool is insufficient.

Primary source:

- [Holoscan SDK resources and allocator selection](https://docs.nvidia.com/holoscan/sdk-user-guide/4-3/components/resources)

### UnitCompose implication

- Development and production capacity policies should be separate.
- Dynamic growth is useful for discovering realistic peaks, but it is not a steady-state guarantee.
- Strict mode must reject overflow rather than silently fall back to an unbounded allocator.
- Holoscan allocators are valuable references and possible device adapters, not a core dependency for a CPU-first Rust V0.

## 5. ONNX Runtime: arenas and bounded growth

ONNX Runtime uses arena-based allocators, supports shared arenas, and exposes configuration for maximum memory, extension strategy, initial chunk sizes, and dead bytes. The documentation notes that an arena may extend when existing regions cannot satisfy a request and normally retains allocated regions.

Primary sources:

- [ONNX Runtime C API: shared allocators and arena configuration](https://onnxruntime.ai/docs/get-started/with-c.html)
- [ONNX Runtime memory consumption](https://onnxruntime.ai/docs/performance/tune-performance/memory.html)

### UnitCompose implication

- A general arena can reduce allocator overhead but does not by itself guarantee no allocation during `run`.
- Explicit maximum memory and growth policy are useful host build options.
- Graph-derived typed slot planning can be more predictable than servicing every output as an arbitrary arena request.
- A future shared allocator may reduce memory across Modules, but Module-local correctness should not depend on it.

## 6. Halide: schedule determines storage

Halide separates algorithm definition from schedule. Storage placement follows scheduling choices, and `fold_storage` can use a bounded circular buffer when producer-consumer access is monotonic. Halide reports a runtime error if the declared folded extent is too small.

Primary sources:

- [Halide overview](https://halide-lang.org/)
- [Halide `Func::fold_storage`](https://halide-lang.org/docs/class_halide_1_1_func.html)

### UnitCompose implication

- Resource live ranges and execution order can reduce peak storage.
- A declared bound should fail explicitly when insufficient.
- V0 should use conservative slot reuse and avoid exposing Halide-like scheduling complexity as a public composition language.
- More advanced circular or tiled storage can be implemented inside specialized Units or future Resource adapters.

## 7. `dyn-stack` and `faer`: precise caller-provided workspace

The Rust `dyn-stack` crate provides `StackReq` to calculate size and alignment and `MemStack` over caller-provided uninitialized bytes. Allocations are nested and reclaimed when the stack frame is dropped.

`faer` exposes scratch-requirement functions, and the `faer-precond` design contract states that `apply` performs no heap allocation: temporary memory flows through a caller-provided `MemStack`, and exact scratch requirements are computed beforehand.

Primary sources:

- [`dyn-stack` crate documentation](https://docs.rs/dyn-stack/latest/dyn_stack/)
- [`dyn_stack::MemStack`](https://docs.rs/dyn-stack/latest/dyn_stack/struct.MemStack.html)
- [`faer` scratch requirement example](https://docs.rs/faer/latest/faer/perm/fn.permute_rows_in_place_scratch.html)
- [`faer-precond` design contract](https://docs.rs/faer-precond/latest/faer_precond/)

### UnitCompose implication

- This is a strong direct model for Unit scratch workspace.
- UnitCompose should wrap the dependency so Unit identities, capacity errors, memory classes, and future alternatives remain under its API.
- `dyn-stack` should not back published Resources because Resource live ranges are DAG-derived and not necessarily LIFO.

## 8. Rust data-view adapters

The `bytes` crate provides a pre-capacity mutable byte buffer that can freeze into a cheaply cloned immutable view. `ndarray` provides borrowed read-only and writable multidimensional views over existing storage.

Primary sources:

- [`bytes::BytesMut`](https://docs.rs/bytes/latest/bytes/struct.BytesMut.html)
- [`ndarray::ArrayView`](https://docs.rs/ndarray/latest/ndarray/type.ArrayView.html)
- [`ndarray::ArrayViewMut`](https://docs.rs/ndarray/latest/ndarray/type.ArrayViewMut.html)

### UnitCompose implication

- `bytes` is a good optional representation for encoded packets, serialized messages, and compressed payloads.
- `ndarray` is a good optional view adapter for images, grids, and tensors.
- Neither should define the universal Resource storage model.
- Retained shared byte views extend lifetime and can prevent immediate slot reuse.

## 9. Design alternatives

### One allocation per Resource

Simple but misses live-range reuse and creates allocator activity proportional to graph size. Reject as the long-term model.

### One heterogeneous raw arena

Can reduce allocation count and pack different types, but pushes alignment, pointer provenance, partial initialization, panic safety, drop glue, and output-retention complexity into V0. Defer until benchmarks show typed slots are insufficient.

### General-purpose bump arena

Useful for many temporary objects sharing one lifetime. It does not model arbitrary Resource live ranges, and a growable arena can still allocate during execution. Consider only inside specialized Units.

### Fixed equal-size block pool

Predictable and simple for homogeneous payloads. It wastes memory for heterogeneous typed Resources and should remain an adapter option.

### Allocator-aware collections everywhere

Custom allocator generics can propagate through Unit APIs and third-party algorithms. Defer until representative Units demonstrate clear value.

## 10. V0 recommendation

Enter the V0 contract:

- logical Resource identity separated from physical storage;
- framework-provided typed output storage;
- declared Unit scratch workspace;
- fixed and bounded requirements;
- Module preparation and storage reports;
- conservative same-representation slot reuse;
- development growth-and-measure;
- production reject-overflow;
- opt-in measured no-run-allocation;
- borrowed outputs or host-provided output storage.

Keep as implementation choices:

- `dyn-stack` behind a workspace wrapper;
- `Vec<T>`, `Box<[MaybeUninit<T>]>`, or equivalent typed slots;
- exact slot-selection heuristic;
- `bytes` and `ndarray` adapters;
- counting-allocator implementation.

Defer:

- cross-type raw packing;
- managed persistent Resources;
- external and cross-language leases;
- GPU and pinned-memory planning;
- parallel and asynchronous reuse;
- automatic inference for arbitrary dynamic shapes;
- global storage optimization across Modules.

## 11. Prototype questions

Before committing implementation details, measure:

- Unit authoring ergonomics for fixed, bounded, and workspace-heavy Units;
- overhead of typed bounded writers;
- exact unsafe surface needed for partial initialization;
- peak memory saved by conservative slot reuse;
- allocation behavior of third-party algorithms after warm-up, including custom native allocators;
- effect of borrowed outputs on host integration;
- cost and usefulness of bounded Debug events;
- whether cross-type packing would produce material savings on navigation and LiDAR workloads.
