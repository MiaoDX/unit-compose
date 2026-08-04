# Rust dependency evaluation

- **Status:** Current implementation recommendation
- **Research date:** 2026-08-04
- **Target:** Rust-first sequential V0 with framework-managed typed storage and an opt-in no-run-allocation guarantee.

Dependency count is not the objective. A dependency is justified when it materially improves the public or Unit-author API, reduces unsafe code, or provides proven behavior that would otherwise be costly to maintain. Dependencies that merely replace small internal data structures are deferred.

## 1. Recommended dependency boundary

### Core candidates

```toml
dyn-stack = "0.13"
thiserror = "2"
```

### YAML frontend candidates

```toml
serde = { version = "1", features = ["derive"] }
saphyr = "<reviewed and pinned 0.x version>"
```

### Optional Resource adapters

```toml
bytes = { version = "1", optional = true }
ndarray = { version = "0.17", optional = true }
```

Versions are examples current at the research date. The implementation should pin and review actual releases.

## 2. `dyn-stack`: recommend for scratch workspace

`dyn-stack` provides:

- `StackReq` for precise size and alignment calculation;
- `MemStack` over caller-provided uninitialized bytes;
- nested typed temporary arrays;
- reuse when a nested allocation scope ends.

Primary sources:

- [`dyn-stack` crate](https://docs.rs/dyn-stack/latest/dyn_stack/)
- [`MemStack`](https://docs.rs/dyn-stack/latest/dyn_stack/struct.MemStack.html)

The `faer` ecosystem demonstrates the same API pattern through scratch-requirement functions and no-heap `apply` contracts:

- [`faer` scratch requirements](https://docs.rs/faer/latest/faer/perm/fn.permute_rows_in_place_scratch.html)
- [`faer-precond` design contract](https://docs.rs/faer-precond/latest/faer_precond/)

Recommendation:

- use `dyn-stack` for temporary Unit workspace;
- expose a UnitCompose `WorkspaceRequirement` and `UnitWorkspace` wrapper rather than making `dyn-stack` types the stable public contract;
- allow an advanced escape hatch to access the underlying `MemStack` for compatible numerical libraries;
- do not use it for published Resource storage.

## 3. `thiserror`: recommend for structured errors

`thiserror` derives standard `Error` implementations without imposing an application-wide error abstraction.

Primary source:

- [`thiserror` derive documentation](https://docs.rs/thiserror/latest/thiserror/derive.Error.html)

Recommendation:

- use it for build, graph, capacity, storage, and run error enums;
- keep hot-path errors structured with interned IDs and numeric capacity data;
- delay human-readable formatting until diagnostics are rendered;
- do not use heap-allocated strings as the only machine-readable error representation.

The dependency improves maintenance and does not define UnitCompose semantics.

## 4. Serde and Saphyr: recommend in the YAML crate

Serde provides mature derived configuration decoding and strict unknown-field controls.

Primary sources:

- [Serde derive](https://serde.rs/derive.html)
- [Serde attributes, including `deny_unknown_fields`](https://serde.rs/attributes.html)

Saphyr's `MarkedYaml` retains source spans on syntax-tree nodes.

Primary source:

- [`saphyr::MarkedYaml`](https://docs.rs/saphyr/latest/saphyr/struct.MarkedYaml.html)

Recommendation:

- parse YAML into a span-preserving syntax tree;
- perform duplicate-key, schema, alias, merge-key, depth, and size checks before typed normalization;
- decode each Unit's configuration through Serde after Unit type resolution;
- isolate Saphyr types inside `unit-compose-yaml`;
- pin a reviewed version because Saphyr remains a 0.x dependency;
- do not deserialize the complete document directly into a plain map that may lose duplicate-key provenance.

## 5. `bytes`: optional byte-Resource adapter

`BytesMut` can reserve capacity, expose spare capacity, split, reclaim, and freeze into shared immutable `Bytes`; cloning a frozen view can share the allocation.

Primary source:

- [`bytes::BytesMut`](https://docs.rs/bytes/latest/bytes/struct.BytesMut.html)

Good uses:

- encoded network packets;
- compressed image payloads;
- serialized messages;
- opaque binary assets.

Cautions:

- it is byte-oriented, not a universal typed storage abstraction;
- `reserve` or growth is incompatible with strict runs;
- retained `Bytes` clones extend storage lifetime and may block slot reuse;
- freezing and sharing should be an adapter-specific output policy.

## 6. `ndarray`: optional image and tensor views

`ArrayView` and `ArrayViewMut` provide borrowed multidimensional access over existing storage.

Primary sources:

- [`ndarray::ArrayView`](https://docs.rs/ndarray/latest/ndarray/type.ArrayView.html)
- [`ndarray::ArrayViewMut`](https://docs.rs/ndarray/latest/ndarray/type.ArrayViewMut.html)

Recommendation:

- provide optional adapters that construct views over prepared typed slices;
- keep shape and stride validation in the Resource representation;
- do not require every Unit or Resource to depend on `ndarray`.

## 7. Resource storage: UnitCompose-owned typed kernel

No evaluated crate directly models all of:

- stable logical Resource identity;
- single producer and multiple readers;
- complete-output publication;
- DAG-derived non-LIFO live ranges;
- borrowed Module outputs;
- partial initialization and drop;
- same-representation slot reuse;
- structured capacity overflow.

Therefore UnitCompose should own a small typed storage kernel using standard Rust facilities such as:

```rust
MaybeUninit<T>
Vec<T>
Box<[MaybeUninit<T>]>
Layout
TypeId
```

The Unit-facing API should be typed:

```rust
trait Unit {
    fn run(
        &mut self,
        inputs: Inputs<'_>,
        outputs: Outputs<'_>,
        workspace: UnitWorkspace<'_>,
    ) -> Result<(), UnitFailure>;
}
```

Bounded output writers should resemble familiar `Vec` operations while replacing implicit growth with fallible operations such as `try_push` and `try_extend`.

## 8. Dependencies not recommended initially

### `bumpalo` and typed arenas

Good for many objects sharing one lifetime, but Resource live ranges are not generally LIFO or common-lifetime. A growable arena can allocate during a run. Consider only inside specialized Units.

### `allocator-api2`

Allocator-aware collection types can be useful, but allocator generics may spread through Unit APIs and third-party code. Revisit only when real Units need interchangeable allocation domains.

### `petgraph`

Graph algorithms are small for the V0 static DAG, and UnitCompose needs exact stable ordering and diagnostics. Implement a compact graph representation and Kahn ordering first.

### `slotmap`, generational arenas, and slabs

The prepared graph is immutable and can use dense newtype indices over `Vec`. Dynamic deletion and stale-handle protection are not yet needed.

### `indexmap`

Stable order should derive from canonical Unit identity, not insertion order. A standard sorted structure or explicit sort is sufficient.

### `smallvec`, `arrayvec`, and `heapless`

These help specific small or compile-time-bounded collections but do not solve runtime graph-derived capacity. Add only after profiling a concrete internal structure.

### Arrow buffers

Strong for Arrow columnar data and interoperability, but they import columnar layout concepts that do not fit every Resource. Use a future Arrow adapter.

### Complete execution frameworks

Holoscan, GStreamer, Bevy, Flecs, OpenVX, and ONNX Runtime are valuable references or integration targets. Importing one would also import lifecycle, object, scheduling, and failure semantics that conflict with UnitCompose's narrower model.

## 9. API-stability policy

- Wrap 0.x dependencies behind UnitCompose-owned types.
- Do not expose parser ASTs, allocator types, or storage-planner internals in stable introductory APIs.
- Optional data-view adapters may expose their ecosystem-native view types by feature.
- A dependency replacement that preserves observable UnitCompose behavior does not require an ADR.
- Record exact versions, licenses, MSRV, unsafe surface, and feature flags before implementation.

## 10. Prototype and benchmark gates

Before finalizing dependencies:

1. implement fixed, bounded, and workspace-heavy synthetic Units;
2. compare `dyn-stack` with a minimal internal workspace wrapper;
3. run Miri on output initialization and workspace adapters;
4. measure no-op Unit overhead and workspace allocation overhead;
5. test 1,000 strict runs with global-allocator instrumentation and adapter hooks for any additional allocation domains;
6. measure code size and compile time;
7. verify Saphyr source spans and duplicate-key behavior;
8. evaluate whether `bytes` or `ndarray` meaningfully improves the first examples.

Add a dependency when the experiment shows clearer API, less unsafe code, or material performance value. Do not reject a mature dependency solely to minimize the crate count.
