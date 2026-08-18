# Dynamic YAML execution

- **Status:** ACTIVE
- **Source plan:** `docs/plans/dynamic-yaml-execution.md`
- **Control plane:** root session
- **Project-status writer:** none
- **Latest intent:** execute the approved plan via `intuitive-flow`
- **Current slice:** Phase 3 canonical builder and sequential DAG executor
- **Last proven evidence:** LiDAR all-target tests and clippy pass; registered scan preparation, stateful Slamwich, and snapshot Units preserve ordered repeated-run state, recover after rejected frames, and produce complete episode reports through the compiled YAML graph
- **Completed batches:** Phase 0; Phase 1; Phase 2 safe storage/reuse; Phase 3A-B dynamic input/builder; Phase 3C dynamic timing/reporting and navigation migration; Phase 5A image/point-cloud migrations; Phase 5B stateful LiDAR migration
- **Next slice:** delete the temporary renamed composite owner and legacy core/debug harnesses, add dynamic `run_into` and contextual errors, and close the sole-Module Phase 3 gate
- **Next proof:** dynamic executor linear/branch/fan-out/fan-in, input/output, failure, poison/recovery, timing, and allocation tests
- **Stop condition:** all six phase stop gates and V0 section 18 acceptance evidence pass, including required product/manual validation
- **No-touch scope:** no unsafe code, new crate/runtime dependency, compatibility layer, second runtime owner, plugins, parallelism, or device/external storage without re-approval
- **Parked work:** the old generic owner is temporarily named `CompositeModule` only while three remaining showcases and legacy core harnesses migrate; it must be deleted, not retained as compatibility surface
