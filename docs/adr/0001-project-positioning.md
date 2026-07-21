# ADR-0001: Project positioning

- **Status:** Accepted
- **Date:** 2026-07-21

## Context

The project needs a clear identity that does not overstate novelty, imply a standalone application runtime, or bind the design to an earlier implementation.

Relevant communities already provide mature ideas for resource access, dependency scheduling, deterministic concurrency, revision management, and commit protocols. The unresolved practical problem is applying a coherent subset of those ideas inside a host-level algorithm module.

## Decision

The project is named **UnitCompose**.

UnitCompose is positioned as:

> An embeddable framework for organizing stateful computation into explicit Units, Resources, and validated Plans.

The project narrative starts from the long-standing community design space and the need for cleaner algorithm modules. Practical experience with long-lived modules may inform decisions, but the project is not presented as a renamed internal V2 implementation.

UnitCompose is not positioned as:

- an application framework;
- a ROS replacement;
- a standalone general runtime;
- a workflow orchestrator;
- a distributed execution platform;
- an ECS;
- a streaming engine;
- a novel universal computation model.

## Consequences

- README and public documentation lead with the problem and community foundations.
- Earlier implementation history is not the primary value proposition.
- Complete frameworks are treated as references or candidate dependencies, not ancestors that define compatibility.
- Features are added only when a representative embedded-module workload requires them.
- Project naming may remain stable even if implementation dependencies change.
