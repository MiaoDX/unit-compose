# ADR-0002: Core terminology

- **Status:** Accepted
- **Date:** 2026-07-21

## Context

The earlier vocabulary mixed project, live instance, execution mechanism, global storage, and dependency-structure terms. A greenfield project should reduce ambiguity without inventing a large new ontology.

The framework is expected to embed in ROS and other systems, so terms such as Executor, Component, Node, and Runtime create collisions or misleading expectations.

## Decision

The basic public model uses four core terms:

- **Module** — the live embeddable instance;
- **Unit** — the smallest scheduled computation;
- **Resource** — a typed logical value or state item;
- **Plan** — the validated semantic composition.

Two supporting terms are retained:

- **Scheduler** — the mostly internal execution policy;
- **Run** — one complete host-triggered execution attempt.

Resource access uses:

- Observe;
- Create;
- Update;
- Export.

Internal implementation documents may use Publication, Commit, Lease, Storage, ResourceStore, and BoundPlan when the distinction is required. These are not additional public model pillars.

The following mappings are adopted:

| Earlier term | Canonical term |
| --- | --- |
| Compute Module V2 | UnitCompose |
| Compute Module | Module |
| Executor Unit | Unit |
| World | Internal ResourceStore |
| Logical Plan | Plan |
| Execution Plan | Internal BoundPlan, if needed |
| Process Cycle | Run |

Public APIs and introductory documentation avoid Graph and Node. Internal algorithms may use those data structures.

## Consequences

- Documentation and code should use one term for one semantic role.
- The project does not create separate public names for logical Resource identity, value publication, physical storage, and lease unless users directly manipulate them.
- World is not exposed as a service locator to Unit code.
- Scheduler is not named Executor, avoiding conflict with ROS executors.
- A later terminology change requires a superseding ADR.
