# Terminology

This document defines canonical UnitCompose terminology.

## Core terms

| Term | Definition |
| --- | --- |
| **UnitCompose** | The project and embeddable framework described by this repository. |
| **Module Definition** | A source representation that declares Units, Resources, bindings, configuration, policies, and exports. It may be authored through Rust builders, Python, or a declarative format. |
| **Module** | A validated and instantiated runtime object owned by a host application. It holds Unit instances, framework-managed Resource state, and lifecycle status. |
| **Unit** | The smallest scheduled computation. A Unit has inspectable configuration, declared Resource access, lifecycle behavior, and optional private state. |
| **Resource** | A typed logical value or state item accessed by Units and backed by owned or borrowed storage. |
| **Plan** | The normalized, validated, language-neutral semantic description compiled from a Module Definition. |
| **Scheduler** | The policy and mechanism that executes ready Units while respecting Plan dependencies and Resource access constraints. |
| **Run** | One complete execution attempt for one request, frame, or tick. |

Only **Module**, **Unit**, **Resource**, and **Plan** are required for the basic user mental model.

## Resource access terms

| Term | Meaning |
| --- | --- |
| **Observe** | Read a specific predecessor value without producing a successor. |
| **Create** | Produce a new value without reading an earlier value of the same Resource. |
| **Update** | Read a selected predecessor and produce its successor. Alpha permits this only for persistent Resources and does not perform physical in-place update. |
| **Export** | Make a committed Resource value visible to the host as part of a successful Run result. |

## Internal semantic terms

These terms are allowed in execution and implementation documents, but are not separate public model pillars.

| Term | Meaning |
| --- | --- |
| **Publication** | A particular produced value of a Resource. A publication may be provisional inside a Run or committed after Run success. |
| **Commit** | The boundary at which staged persistent successors and exports become host-visible and reusable by later Runs. |
| **Lease** | A bounded right to access physical storage under declared mutability and lifetime constraints. |
| **Storage** | Physical memory or device backing used for one or more Resource publications. |
| **ResourceStore** | An internal Module-owned service that tracks Resource identity, publications, leases, and storage associations. It is not exposed as a general Unit service locator. |
| **BoundPlan** | An optional implementation term for a Plan after Unit implementations, Resource representations, and scheduling metadata have been resolved. It is not required in public APIs. |
| **Poisoned Module** | A Module that cannot safely run again after Unit execution, publication, or commit failure. |

## Words to avoid in the public model

| Word | Reason |
| --- | --- |
| **Executor Unit** | A Unit is executed by a Scheduler; it is not itself an executor. The term also conflicts with ROS executor terminology. |
| **World** | Suggests a global ECS container and broad arbitrary access. UnitCompose exposes declared Resource views instead. |
| **Runtime** as the project category | Too broad to describe the project and easily confused with language, process, or accelerator runtimes. |
| **Component** as the core computation term | Conflicts with ROS composition and with many host-framework object models. |
| **Pipeline** as the whole model | Suggests a linear sequence and understates persistent state and branching dependencies. |
| **Task** or **Job** as the Unit term | Suggests one-shot, remote, queued, or asynchronous work. |
| **Graph** and **Node** in user-facing APIs | Internal dependency analysis may use these structures, but the project should present Units, Resources, bindings, and dependencies. |

## Legacy mapping

| Earlier term | Canonical term |
| --- | --- |
| Compute Module V2 | UnitCompose |
| Compute Module | Module |
| Executor Unit | Unit |
| World | Internal ResourceStore |
| Logical Plan | Plan |
| Execution Plan | Internal BoundPlan, when needed |
| Process Cycle | Run |

Legacy terminology may be mentioned in migration notes or historical discussions, but new specifications and APIs should use the canonical terms.
