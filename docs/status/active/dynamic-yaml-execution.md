# Dynamic YAML execution

- **Status:** ACTIVE
- **Source plan:** `docs/plans/dynamic-yaml-execution.md`
- **Control plane:** root session
- **Project-status writer:** none
- **Latest intent:** execute the approved plan via `intuitive-flow`
- **Current slice:** Phase 1 canonical registration and executable conformance fixture
- **Last proven evidence:** `cargo test --workspace --all-targets` passes after canonical registry migration; focused graph compiler (12), YAML frontend (8), and parser property (2) tests pass; exact stale-symbol search finds no `FrontendRegistry` in code
- **Completed batches:** Phase 0 baseline/error/matrix; Phase 1A core-owned descriptor, typed decoder/config identity, and requirements resolver with all YAML/showcase callers migrated off the independent frontend registry
- **Next slice:** add executable factory and Resource adapter identities, dense Unit/Resource/port handles, and source/map/join/fail conformance fixture
- **Next proof:** focused registration/factory drift and graph-to-dense-handle tests
- **Stop condition:** all six phase stop gates and V0 section 18 acceptance evidence pass, including required product/manual validation
- **No-touch scope:** no unsafe code, new crate/runtime dependency, compatibility layer, second runtime owner, plugins, parallelism, or device/external storage without re-approval
- **Parked work:** none
