# Dynamic YAML execution

- **Status:** ACTIVE
- **Source plan:** `docs/plans/dynamic-yaml-execution.md`
- **Control plane:** root session
- **Project-status writer:** none
- **Latest intent:** execute the approved plan via `intuitive-flow`
- **Current slice:** Phase 3 canonical builder and sequential DAG executor
- **Last proven evidence:** image-registration all-target tests and clippy pass; its six YAML Units now execute registered implementations with typed grayscale, ORB feature, candidate match, geometry, warp, and result Resources, including compiled grayscale fan-out into warp
- **Completed batches:** Phase 0; Phase 1; Phase 2 safe storage/reuse; Phase 3A-B dynamic input/builder; Phase 3C dynamic timing/reporting and navigation migration; Phase 5A image-registration migration with fixed-pipeline validator and composite owner deleted
- **Next slice:** migrate point-cloud registration to the dynamic `Module`, then LiDAR; delete the temporary renamed composite owner and its old tests/harness before closing the sole-Module gate
- **Next proof:** dynamic executor linear/branch/fan-out/fan-in, input/output, failure, poison/recovery, timing, and allocation tests
- **Stop condition:** all six phase stop gates and V0 section 18 acceptance evidence pass, including required product/manual validation
- **No-touch scope:** no unsafe code, new crate/runtime dependency, compatibility layer, second runtime owner, plugins, parallelism, or device/external storage without re-approval
- **Parked work:** the old generic owner is temporarily named `CompositeModule` only while three remaining showcases and legacy core harnesses migrate; it must be deleted, not retained as compatibility surface
