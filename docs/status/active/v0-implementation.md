# V0 Implementation Active Capsule

- Status: ACTIVE
- Source plan: `docs/plans/v0-implementation-plan.md`
- Control plane: root Codex session for `v0-implementation`
- Project-status writer: not adopted; no project status surface exists
- Latest user intent: execute the complete V0 plan through `intuitive-flow`
- Current slice: Milestone 4 YAML frontend
- Blocker: none
- Last proven evidence: Milestone 3 commits `92a2e4c`, `f11fa6a`, and `7f2a458`; fmt, strict clippy, 40 workspace tests including the isolated 1,000-run allocator harness, doctests, and locked ARM64 check pass
- Completed slices: Milestone 0 typed execution kernel; Milestone 1 normalized graph compiler; Milestone 2 typed storage kernel; Milestone 3 orthogonal capacity/allocation policies, inspectable allocation-domain evidence, strict measurement, warm-up boundary, and bounded run reports
- Next slice: implement a span-preserving, resource-bounded YAML frontend with strict schema validation, Serde Unit config decoding, deterministic typed normalization, and resolved graph handoff
- Next proof: path/span diagnostics for schema and graph failures, alias/merge rejection, parser depth/document limits, deterministic normalization, parser fuzz/property coverage, and workspace gates
- Stop condition: Milestone 4 evidence passes, or the YAML schema/public configuration contract requires a product decision not fixed by the V0 specification
- No-touch scope: navigation Quickstart algorithms and reload lifecycle, debug/report adapters beyond existing core events, hardening-only campaigns, and optional Rerun adapter except interfaces required by Milestone 4
- Parked work: Milestones 5 through 7; native ARM64 CI result is pending external CI execution; Miri execution awaits a compatible toolchain; optional Rerun adapter remains non-gating
