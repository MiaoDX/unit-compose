# Navigation visualization refinement

- **Status:** Active delivery plan
- **Date:** 2026-08-05
- **Parent:** [V0 implementation plan](v0-implementation-plan.md)

## Plan Ledger

- Plan status: ACTIVE
- Session scope: navigation-visualization-refinement
- Current slice: optional Rerun adapter
- Next action: implement file recording and live viewer routes at the post-run adapter boundary
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
