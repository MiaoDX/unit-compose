# Dynamic YAML execution

- **Status:** ACTIVE
- **Source plan:** `docs/plans/dynamic-yaml-execution.md`
- **Control plane:** root session
- **Project-status writer:** none
- **Latest intent:** execute the approved plan via `intuitive-flow`
- **Current slice:** Phase 3 canonical builder and sequential DAG executor
- **Last proven evidence:** core all-target tests and focused clippy pass; prepared execution accepts borrowed typed host inputs through plan-scoped handles, validates exact input membership/provenance/type before reset, and poisons after panic while remaining reusable after recoverable Unit failure
- **Completed batches:** Phase 0; Phase 1; Phase 2A safe typed slots; Phase 2B planner-backed compatible reuse with separate logical publication ownership and physical disjointness validation; Phase 3A host input carrier and prepared-runtime poison boundary
- **Next slice:** make the prepared runtime the sole public `Module`, implement the frontend-neutral builder, borrowed output views, `run_into`, contextual errors, timing, and strict probes
- **Next proof:** dynamic executor linear/branch/fan-out/fan-in, input/output, failure, poison/recovery, timing, and allocation tests
- **Stop condition:** all six phase stop gates and V0 section 18 acceptance evidence pass, including required product/manual validation
- **No-touch scope:** no unsafe code, new crate/runtime dependency, compatibility layer, second runtime owner, plugins, parallelism, or device/external storage without re-approval
- **Parked work:** remove the private runtime module's temporary dead-code allowance when the canonical Module builder consumes it in Phase 3
