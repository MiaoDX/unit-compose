# Dynamic YAML execution

- **Status:** ACTIVE
- **Source plan:** `docs/plans/dynamic-yaml-execution.md`
- **Control plane:** root session
- **Project-status writer:** none
- **Latest intent:** execute the approved plan via `intuitive-flow`
- **Current slice:** Phase 3 canonical builder and sequential DAG executor
- **Last proven evidence:** full workspace tests and clippy pass; production exposes one dynamic `Module`, strict capability aggregates all registered graph participants, the isolated harness proves 1,000 zero-allocation runs and detects allocation, and `run_into` plus contextual Unit failures preserve typed causes and host-output validity
- **Completed batches:** Phase 0; Phase 1; Phase 2 safe storage/reuse; Phase 3 dynamic inputs/builder/timing/reporting/run-into/errors/strict aggregation with legacy runtime deleted; Phase 4 navigation; Phase 5A image/point-cloud; Phase 5B stateful LiDAR
- **Next slice:** run the Phase 3 latency comparison and complete the V0 section 18 acceptance matrix, product commands, saved Rerun artifacts, manual visual validation, and final docs alignment
- **Next proof:** dynamic executor linear/branch/fan-out/fan-in, input/output, failure, poison/recovery, timing, and allocation tests
- **Stop condition:** all six phase stop gates and V0 section 18 acceptance evidence pass, including required product/manual validation
- **No-touch scope:** no unsafe code, new crate/runtime dependency, compatibility layer, second runtime owner, plugins, parallelism, or device/external storage without re-approval
- **Parked work:** none in implementation; local product/Rerun/manual proof remains mandatory before completion
