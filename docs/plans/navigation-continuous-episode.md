# Continuous Navigation Episode Visualization

## Plan Ledger

- **Status:** Preflight ready; implementation deferred to a new context
- **Date:** 2026-08-06
- **Parent:** [V0 implementation plan](v0-implementation-plan.md)
- **Related:** [Unit timing visualization](unit-timing-visualization.md)
- **Planning loop:** Round 1 completed with entropy and docs-grounded grill
  scouts; both converged on the same scope and stop gates.
- **Cadence decision:** Approved full pipeline per leg for this slice.
- **Next action:** Execute this plan through `intuitive-flow` in a new context.

## Goal

Replace the fixed-input timing demonstration with one continuous,
deterministic navigation episode: one prepared navigation runtime executes
1,000 linked navigation legs on the fixed map, while Rerun shows the current
route and robot motion over time without overlaying 1,000 paths. Mermaid
summarizes the resulting Unit timing distribution.

## Recommended Cadence

For this slice, keep the existing full pipeline per leg:

```text
leg 0: decode -> inflate -> plan -> smooth
leg 1: decode -> inflate -> plan -> smooth
...
leg 999: decode -> inflate -> plan -> smooth
```

The `PreparedNavigation` runtime is built once and warmed once. The
full-pipeline choice keeps the current Module graph and execution contract
truthful, and avoids introducing map identity, cache invalidation, or a new
prepared-costmap API into the visualization refactor.

Under this cadence every represented Unit has `n=1000` in Mermaid. The timing
values remain wall-clock observations; avg is the arithmetic mean and p99 is
the nearest-rank percentile.

## Scope

1. Add a deterministic 1,000-leg itinerary for the existing 48 x 40 map.
2. Guarantee `start[i + 1] == goal[i]` and validate every endpoint against the
   inflated free-space component before execution.
3. Use explicit short, medium, and long route-distance buckets with fixed,
   asserted counts. No runtime-random sampling.
4. Run one prepared runtime through the itinerary and preserve the current
   strict allocation guarantees and algorithm results.
5. Extend Rerun recording with one monotonic episode timeline. Log the map
   once, then update stable entities for the current start, goal, raw path,
   smoothed path, and robot pose.
6. Animate pose progression along each current path. Keep only a bounded
   recent pose trail or bounded visit-density overview; never create one path
   entity per leg.
7. Record per-leg Unit timing and run metrics on the same episode timeline.
8. Aggregate the 1,000 snapshots for Mermaid and annotate each Unit with
   `avg`, nearest-rank `p99`, and truthful `n=1000`.
9. Update the navigation README/milestone documentation and add focused
   workload, timing, Rerun entity, and recording-structure tests.

## Non-goals

- A generic graph scheduler or lifecycle-aware executor.
- A map-version cache or a separate prepared-costmap public API.
- Rebuilding a costmap, SLAM, localization, ROS runtime, or planner library.
- Retaining 1,000 independent path entities or rendering all historical paths
  simultaneously.
- Planner frontier animation or unbounded trajectory/history retention.
- Benchmark-grade claims, hardware timing, asynchronous device timing, or a
  Rerun SDK upgrade.

## Rerun Contract

- `navigation/map` is timeless and emitted once.
- `navigation/raw_path`, `navigation/smoothed_path`, `navigation/start`,
  `navigation/goal`, and `navigation/robot_pose` use stable entity paths and
  temporal samples keyed by `episode_tick`.
- A path is written at the beginning of a leg and remains the latest value while
  the pose advances; the next leg replaces it at a later tick.
- The pose advances along the current path. The trail is bounded to a fixed
  maximum (default 64 samples) or replaced by a bounded visit-density layer.
- Timing series use leg/Unit identity as dimensions and preserve the existing
  elapsed and start-offset meanings.
- The file route is the deterministic acceptance route. Viewer spawn remains a
  separate manual gate when a compatible `rerun` executable is available.

## Mermaid Contract

The timed graph aggregates only completed legs from the episode. Every timing
annotation includes `avg`, nearest-rank `p99`, and `n`. If a leg fails, the
episode stops and the output identifies the failed leg; partial aggregates are
not presented as a successful 1,000-leg result.

## Acceptance Criteria

- Exactly 1,000 deterministic legs execute for the selected fixture.
- All endpoints are valid and reachable; `start[i + 1] == goal[i]` for all
  adjacent legs.
- Short/medium/long bucket counts and the itinerary fingerprint are asserted.
- The prepared runtime is built once and warm-up remains outside measured runs.
- Existing algorithm/path semantics and strict allocation tests remain green.
- Rerun contains one static map, ordered episode ticks, stable current-state
  entities, and no unbounded per-leg entity growth.
- The bounded trail/density representation has an asserted maximum.
- Mermaid recomputation matches every rendered avg, p99, and `n=1000` value.
- The generated `.rrd` is nonempty and machine-inspected for entity cardinality,
  timeline order, and current-path replacement.
- The latest screenshot visibly shows route replacement and pose progression;
  it does not show 1,000 overlaid routes.

## Verification Ladder

1. `cargo fmt --all -- --check` and focused navigation tests for itinerary
   determinism, chaining, bucket counts, and path validity.
2. Allocation-free strict execution and full workspace tests/Clippy.
3. Save a Rerun recording and inspect it through the SDK/file reader for
   stable entity paths, ordered ticks, bounded trail data, and path replacement.
4. Render the timed Mermaid PNG and compare its annotations against the
   independently recomputed aggregates.
5. Run the viewer route manually when a compatible `rerun` binary is on PATH;
   treat this as an explicit live gate, not a CI requirement.

## Risks And Stop Gates

- **Cadence review gate:** if map preparation must be `n=1` and planning must
  be `n=1000` now, stop this plan and create a separate map-lifecycle design.
- Stop on any unreachable endpoint, broken chaining invariant, path-capacity
  overflow, allocation event, dropped timing event, timing mismatch, or
  unbounded Rerun entity growth.
- Stop if the file recording cannot prove temporal replacement without relying
  on a live viewer.

## Parked Alternatives

- Split map preparation from per-leg query execution with explicit map-version
  resources and mixed invocation denominators.
- Replace the bounded recent trail with a visit-density heatmap if visual review
  shows the trail is insufficient.
- Add a separate ADR only if map lifecycle or timeline semantics become a
  durable public contract.

## Planning Loop Disposition

- **Accepted:** one new extension plan, deterministic linked itinerary, stable
  temporal Rerun entities, bounded history, per-Unit avg/p99 aggregation.
- **Merged:** static-map reuse, route metrics, and timeline inspection are one
  visualization contract rather than separate plans.
- **Parked:** map-aware cadence split, cache invalidation, generic scheduling,
  density-vs-trail preference.
- **Rejected:** random runtime sampling, per-leg entity namespaces, full path
  overlays, frontier animation, SDK upgrade, SLAM/costmap redesign.

## Preflight Contract

**Preflight status:** DRAFT

**Task source:** Approved planning-loop recommendation plus this plan.

**Canonical source:** `docs/plans/navigation-continuous-episode.md`

**Route:** Durable `$intuitive-flow`, supervised by the main session without a
separate implementation worker.

**Goal:** Implement and prove one deterministic 1,000-leg continuous navigation
episode using the existing full-pipeline cadence, with temporal Rerun playback
and truthful Mermaid timing aggregates.

**Scope:** Execute every item in this plan: deterministic itinerary, linked
leg execution, current-state Rerun entities and pose playback, bounded history,
1,000-sample Mermaid aggregation, focused tests, product artifacts, and current
human documentation.

**Non-goals:** The complete `Non-goals` section above is binding. In particular,
do not introduce a generic scheduler, map cache/lifecycle API, costmap/SLAM
implementation, per-leg entity namespace, benchmark claim, or SDK upgrade.

**Entity budget:**

- **Reuse:** existing YAML commands, fixed map fixture, `PreparedNavigation`,
  strict run/reporting path, timing snapshots, stable graph description,
  `RerunAdapter`, README preview asset, and current tests.
- **Remove/merge:** replace the fixed-input 100-run timed-Mermaid workload and
  ten repeated Rerun timing samples with the canonical episode runner; do not
  retain parallel legacy workload paths.
- **New:** one private deterministic itinerary/episode representation and the
  minimum adapter data needed for pose/timeline logging. A new public API,
  command, crate, or generic executor is not authorized.
- **Expansion triggers:** stop for review before adding map-version lifecycle,
  caching, another source module solely for abstraction, a new CLI route,
  unbounded history, or any dependency.

**Context:**

- **Must read:** this plan; `README.md`;
  `docs/plans/unit-timing-visualization.md`;
  `docs/implementation/milestone-5-navigation-quickstart.md`;
  `docs/implementation/milestone-6-inspection-reports.md`;
  `examples/navigation-planning/src/main.rs`;
  `examples/navigation-planning/src/lib.rs`;
  `examples/navigation-planning/tests/quickstart.rs`;
  `crates/unit-compose-core/src/inspection.rs`;
  `crates/unit-compose-debug-rerun/src/lib.rs`.
- **Useful:** current generated Mermaid/RRD/PNG artifacts and focused timing
  tests in `crates/unit-compose-core`.
- **Avoid unless needed:** planning archives, Rerun SDK migration research,
  external costmap/SLAM libraries, and unrelated workspace crates.

**Acceptance:**

- **SUCCESS:** every acceptance criterion and deterministic, integration,
  product-run, and visual gate in this plan passes; final RRD, Mermaid source,
  and screenshots are regenerated and linked for review.
- **BLOCKED_NEEDS_DECISION:** any required expansion trigger, cadence change,
  public API/CLI change, new dependency, or inability to prove temporal
  replacement from the saved recording.
- **BLOCKED_NEEDS_LOCAL_VALIDATION:** implementation and deterministic tests
  pass but a compatible Rerun viewer cannot be used to prove current-path
  replacement, pose progression, and bounded visual history.
- **INTERMEDIATE_ONLY:** none; the next context is expected to finish the whole
  plan rather than stop after itinerary or logging scaffolding.
- **No regressions:** existing strict commands, A*/Dijkstra/no-smoothing
  results, allocation guarantees, inspection formats, Rerun file route, and
  default-off feature behavior remain intact.

**Verification:**

- **Deterministic:** `cargo fmt --all -- --check`;
  `cargo test -p navigation-planning --all-features --locked -- --test-threads=1`;
  focused assertions for itinerary fingerprint, chaining, reachability,
  distance buckets, sample counts, percentile math, stable entity cardinality,
  and bounded history.
- **Integration:**
  `cargo test --workspace --all-features --locked -- --test-threads=1`;
  `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`;
  `git diff --check`; inspect the saved recording through available Rerun SDK
  test/memory surfaces.
- **Product run:** run `--timed-mermaid` and verify four `n=1000` Unit
  annotations; run `--rerun-save target/navigation-astar.rrd` and verify a
  nonempty, machine-inspectable episode recording.
- **Local/live/manual:** open or spawn the recording with a compatible Rerun
  viewer, scrub/play the episode, capture the current navigation/timing views,
  and prove that only the current paths plus bounded history are visible.
- **Optional:** compare multiple local timing runs for observation only; do not
  promote them to benchmark claims.

**Execution:**

- **Main:** owns scope decisions, sequential implementation, artifact review,
  full verification, and final completion/block judgment.
- **Worker:** none by default; use a read-only scout only if execution reveals a
  narrow question that cannot be answered from the must-read context.
- **Worker goal:** none.

**To execute:** `/goal execute docs/plans/navigation-continuous-episode.md with intuitive-flow`

**Optional tracking:** none.

**Approval:** `LGTM`, `approve`, or an explicit request to implement this plan
approves execution in the new context; edits request revision.
