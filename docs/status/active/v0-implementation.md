# V0 Implementation Active Capsule

- Status: ACTIVE
- Source plan: `docs/plans/v0-implementation-plan.md`
- Control plane: root Codex session for `v0-implementation`
- Project-status writer: not adopted; no project status surface exists
- Latest user intent: execute the complete V0 plan through `intuitive-flow`
- Current slice: Milestone 3 allocation policies
- Blocker: none
- Last proven evidence: Milestone 2 commits `0ee2694`, `395abd6`, and `eead3fb`; fmt, strict clippy, 37 tests plus compile-fail doctest, docs, and locked ARM64 check pass; Miri component unavailable on pinned stable
- Completed slices: Milestone 0 typed execution kernel; Milestone 1 normalized graph compiler; Milestone 2 descriptor-owned typed storage, input validation, live ranges, conservative reuse, aligned workspace, and storage reports
- Next slice: implement orthogonal capacity/allocation policy construction, observed capacity, allocation-domain instrumentation/certification, strict measurement, warm-up, and bounded run events
- Next proof: isolated 1,000-run allocation harness across success/failure/overflow paths, negative allocating/unsupported-domain fixtures, strict build rejection, description evidence, and workspace gates
- Stop condition: Milestone 3 evidence passes, or allocation measurement/certification requires an external service or public-contract decision
- No-touch scope: YAML parsing, navigation Quickstart, debug/report adapters beyond bounded core events, lifecycle/reload, and optional Rerun adapter except interfaces required by Milestone 3
- Parked work: Milestones 4 through 7; native ARM64 CI result is pending external CI execution; Miri execution awaits a compatible toolchain; optional Rerun adapter remains non-gating
