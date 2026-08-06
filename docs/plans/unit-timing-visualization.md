# Unit timing visualization

- **Status:** Done
- **Date:** 2026-08-06
- **Parent:** [V0 implementation plan](v0-implementation-plan.md)

## Goal

Make bounded per-Unit timing a framework-owned run observation, then project
the same snapshot into Rerun and an annotated Mermaid graph.

## Scope

1. Add an allocation-free, fixed-capacity Unit timing recorder keyed by stable
   execution-order ordinal.
2. Record the real navigation `decode`, `inflate`, `plan`, and optional
   `smooth` boundaries without changing algorithm results.
3. Keep Mermaid canonical for fixed topology and add a run-annotated renderer.
4. Replace the static Rerun graph panel with per-Unit timing series.
5. Record ten bounded timing samples on explicit Rerun routes while retaining
   one navigation frame.
6. Summarize 100 post-warm-up strict runs as average and nearest-rank p99 timing
   on the Mermaid graph.

## Non-goals

- parallel scheduling or a generic graph executor;
- unbounded trace retention or benchmark-grade statistical analysis;
- planner frontier animation;
- timing asynchronous device work.

## Verification

Focused tests prove ordered timing events, disabled-report behavior, annotated
Mermaid output, allocation-free strict runs, and nonempty Rerun recording.
Workspace formatting, tests, and strict Clippy remain the completion gate.
