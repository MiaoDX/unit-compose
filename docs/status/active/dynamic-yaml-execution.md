# Dynamic YAML execution

- **Status:** ACTIVE
- **Source plan:** `docs/plans/dynamic-yaml-execution.md`
- **Control plane:** root session
- **Project-status writer:** none
- **Latest intent:** execute the approved plan via `intuitive-flow`
- **Current slice:** Phase 3 canonical builder and sequential DAG executor
- **Last proven evidence:** full workspace tests and clippy pass; navigation A*, Dijkstra, and no-smoothing YAML variants execute registered decoder/inflation/planner/smoother implementations in compiled order, report actual Unit timings, remain allocation-free under the existing probe, and produce distinct expected paths and inspection snapshots
- **Completed batches:** Phase 0; Phase 1; Phase 2A safe typed slots; Phase 2B planner-backed compatible reuse; Phase 3A typed host input and poison boundary; Phase 3B canonical executable builder; Phase 3C dynamic public `Module` timing/reporting and navigation vertical slice
- **Next slice:** migrate image registration and point-cloud registration to the dynamic `Module`, then LiDAR; delete the temporary renamed composite owner and its old tests/harness before closing the sole-Module gate
- **Next proof:** dynamic executor linear/branch/fan-out/fan-in, input/output, failure, poison/recovery, timing, and allocation tests
- **Stop condition:** all six phase stop gates and V0 section 18 acceptance evidence pass, including required product/manual validation
- **No-touch scope:** no unsafe code, new crate/runtime dependency, compatibility layer, second runtime owner, plugins, parallelism, or device/external storage without re-approval
- **Parked work:** the old generic owner is temporarily named `CompositeModule` only while three remaining showcases and legacy core harnesses migrate; it must be deleted, not retained as compatibility surface
