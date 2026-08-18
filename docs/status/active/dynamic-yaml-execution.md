# Dynamic YAML execution

- **Status:** ACTIVE
- **Source plan:** `docs/plans/dynamic-yaml-execution.md`
- **Control plane:** root session
- **Project-status writer:** none
- **Latest intent:** execute the approved plan via `intuitive-flow`
- **Current slice:** Phase 2B conservative compatible storage reuse
- **Last proven evidence:** core all-target tests pass (24 lib, 14 graph, 7 storage); runtime tests cover fixed/bounded success, empty-vs-unpublished state, strict overflow with unchanged capacity, development growth, grouped partial validation, Unit error, unwind, reset, and exact pending/published drop counts
- **Completed batches:** Phase 0; Phase 1; Phase 2A safe one-physical-slot-per-logical-Resource runtime store with preparation disjointness checks
- **Next slice:** allocate physical slots from the conservative `StoragePlan`, map dense logical Resources to slots, and track publication ownership independently
- **Next proof:** compatible non-overlap reuse plus overlapping/incompatible non-reuse execution tests with logical publication isolation
- **Stop condition:** all six phase stop gates and V0 section 18 acceptance evidence pass, including required product/manual validation
- **No-touch scope:** no unsafe code, new crate/runtime dependency, compatibility layer, second runtime owner, plugins, parallelism, or device/external storage without re-approval
- **Parked work:** remove the private runtime module's temporary dead-code allowance when the canonical Module builder consumes it in Phase 3
