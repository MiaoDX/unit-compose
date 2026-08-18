# Dynamic YAML execution

- **Status:** ACTIVE
- **Source plan:** `docs/plans/dynamic-yaml-execution.md`
- **Control plane:** root session
- **Project-status writer:** none
- **Latest intent:** execute the approved plan via `intuitive-flow`
- **Current slice:** Phase 2A safe runtime Resource store correctness
- **Last proven evidence:** object-safe source/map/join/fail fixture executes stable fan-in to 26 and keeps a written failing output unpublished; graph compiler (14), core lib (22), workspace all-target check, and focused core clippy with `-D warnings` pass
- **Completed batches:** Phase 0 baseline/error/matrix; all Phase 1 slices: single registration owner, dense bindings/handles, typed factory construction, private object-safe dispatch, and fixed-value executable adapter
- **Next slice:** implement fixed/bounded buffer runtime slots, grouped validate-then-publish/discard/drop/unwind behavior, and preparation disjointness validation
- **Next proof:** runtime-storage success, overflow, partial write, Unit error, validation error, unwind, reset, and drop tests
- **Stop condition:** all six phase stop gates and V0 section 18 acceptance evidence pass, including required product/manual validation
- **No-touch scope:** no unsafe code, new crate/runtime dependency, compatibility layer, second runtime owner, plugins, parallelism, or device/external storage without re-approval
- **Parked work:** remove the private runtime module's temporary dead-code allowance when the canonical Module builder consumes it in Phase 3
