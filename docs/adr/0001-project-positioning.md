# ADR-0001: Project positioning

- **Status:** Accepted
- **Date:** 2026-08-04

## Context

Algorithm and robotics components often grow from one entry point into a collection of filters, planners, model calls, caches, shared buffers, profiling hooks, and order assumptions. The host-level component may remain the correct deployment boundary, while its internal implementation becomes difficult to configure, validate, test, inspect, and optimize.

Many established systems already provide graph execution, buffer pools, arena allocation, operator composition, or scheduling. UnitCompose needs a narrow identity that does not imply a new general runtime.

## Decision

UnitCompose is:

> An embeddable, configuration-driven framework for composing typed Units through named Resources inside one host-owned algorithm or functional component.

The durable public model is Unit, Resource, and Module. Inspection, diagnostics, timing, storage reports, and optional visualization are read-only Module capabilities rather than a fourth domain object.

UnitCompose additionally treats predictable storage as part of the module-composition problem: Unit outputs and scratch workspace can be declared before execution, prepared by the Module, and verified under an opt-in steady-state no-allocation profile covering declared allocation domains.

UnitCompose is not positioned as:

- an application framework or ROS replacement;
- a standalone process runtime;
- a distributed workflow or streaming platform;
- an ECS;
- a model compiler;
- a universal device-memory or zero-copy layer;
- a hard-real-time operating environment;
- a novel scheduling theory.

## Consequences

- The host owns lifecycle, communication, threading, and reload timing.
- YAML changes registered algorithms and graph structure without changing host code.
- Unit and Resource contracts serve validation, execution, inspection, and storage preparation.
- Sequential execution is the V0 semantic reference.
- Allocation predictability is an explicit capability, not an implicit claim about arbitrary third-party code.
- Larger capabilities are added only when representative embedded workloads prove the smaller model insufficient.
