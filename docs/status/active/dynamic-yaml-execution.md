# Dynamic YAML execution

- **Status:** ACTIVE
- **Source plan:** `docs/plans/dynamic-yaml-execution.md`
- **Control plane:** root session
- **Project-status writer:** none
- **Latest intent:** execute the approved plan via `intuitive-flow`
- **Current slice:** Phase 1 canonical registration and executable conformance fixture
- **Last proven evidence:** three 1,000-run navigation composite samples; median-of-medians 266,426 ns; every sample reports zero allocate/reallocate/deallocate operations; `cargo fmt --all -- --check` passes
- **Completed batches:** Phase 0 baseline, V0 public failure envelope, and 21-item normative acceptance matrix recorded in the source plan
- **Next slice:** extend the core registry with typed configuration, Resource adapter, requirements, and executable factory identities; remove independent YAML ownership
- **Next proof:** focused core registration tests plus YAML frontend tests
- **Stop condition:** all six phase stop gates and V0 section 18 acceptance evidence pass, including required product/manual validation
- **No-touch scope:** no unsafe code, new crate/runtime dependency, compatibility layer, second runtime owner, plugins, parallelism, or device/external storage without re-approval
- **Parked work:** none
