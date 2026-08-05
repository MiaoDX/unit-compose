# Navigation visualization refinement

- **Status:** Done
- **Date:** 2026-08-05
- **Parent:** [V0 implementation plan](v0-implementation-plan.md)

## Plan Ledger

- Plan status: DONE
- Session scope: navigation-visualization-refinement
- Current slice: complete
- Next action: none
- Blocked on: none
- Do not touch: core execution semantics, browser/desktop UI, per-Unit animation, nuScenes

## Goal

Make the navigation example and its inspection views communicate the product
pipeline directly, then implement the optional Rerun boundary already reserved
by the V0 plan.

## Scope

1. Remove the demonstration-only `stats` Unit and `cost_stats` Resource from
   all navigation definitions, runtime evidence, registry wiring, and tests.
2. Keep the smoothed graphs at four Units and the no-smoothing graph at three
   Units while retaining real `cost_map` fan-out through planner and smoother.
3. Make DOT and Mermaid distinguish Units, Resources, Module inputs, and Module
   outputs. A terminal internal Resource must not look like a Module output.
4. Add the optional `unit-compose-debug-rerun` crate using the existing fixed
   description/run snapshot adapter boundary. Provide map, cost-map, raw-path,
   smoothed-path, graph, timing/capacity metrics, a fixed blueprint, and
   recording/spawn routes without entering strict measured execution.
5. Align runnable examples and human documentation with the shipped surface.

## Non-goals

- changing Module execution, graph compilation, storage, or allocation policy;
- browser or desktop UI owned by UnitCompose;
- per-Unit execution animation;
- making Rerun a core dependency or a V0 completion gate;
- nuScenes or other dataset integration.

## Acceptance

- the three navigation YAML definitions expose exactly one Module output;
- no `stats` Unit or `cost_stats` Resource remains in the navigation product;
- A*/Dijkstra execute four real stages and no-smoothing executes three;
- DOT and Mermaid have contract tests for distinct node classes and explicit
  Module input/output styling;
- the Rerun adapter compiles independently, records the required navigation
  entities to a file without a viewer, and has focused adapter tests;
- strict navigation product runs remain allocation-free;
- formatting, strict Clippy, workspace tests, doctests, and documentation pass.

## Stop Gate

Close only after changed-code review and documentation alignment find no
required follow-up. External interactive viewer inspection may be reported
separately, but file recording must be proven locally and deterministically.

## Outcome

- Removed the demonstration-only statistics branch. A* and Dijkstra now use
  four Units, the no-smoothing variant uses three, and each Module exposes one
  path output.
- DOT and Mermaid use distinct styles for Units, internal Resources, Module
  inputs, and Module outputs.
- Added a default-off Rerun adapter with file and external-viewer routes. Its
  focused tests inspect recorded entity paths and blueprint activation through
  the SDK memory sink.
- Changed-code review found and fixed the final-path selection invariant; no
  required reuse, quality, or efficiency follow-up remains.
- README, documentation index, implementation evidence, and dependency/license
  inventory describe the shipped surface.

## Verification

Focused adapter and navigation tests pass with and without the `rerun` feature.
All three strict navigation products retain zero measured allocator operations,
and file recording produces a nonempty `.rrd` without a viewer. Workspace
formatting, strict Clippy, tests, doctests, documentation, cross-target checks,
panic-abort checks, and the isolated allocation harness form the final closeout
gate.

Interactive `--rerun-spawn` inspection remains an external environment check:
the route is implemented, but requires a compatible `rerun` executable and is
not part of deterministic local acceptance.
