# Terminology

This document defines canonical UnitCompose terminology for V0.

## Public concepts

| Term | Definition |
| --- | --- |
| **UnitCompose** | The embeddable framework described by this repository. |
| **Unit** | One typed computation step with configuration, required input ports, output ports, optional private state, and declared output-capacity and workspace requirements. |
| **Resource** | One named logical value with a stable semantic type, produced once per run and consumed read-only after publication. |
| **Module** | One validated and prepared Resource DAG owned by a host application, with fixed compiled structure and mutable runtime state. |

These three concepts form the durable user mental model. Inspection, diagnostics, timing, storage reports, and optional visualization are read-only Module capabilities rather than another public concept.

## Composition terms

| Term | Definition |
| --- | --- |
| **Module Definition** | A source description, normally YAML, that selects Unit types, supplies configuration, binds ports to Resource names, and declares Module inputs and outputs. |
| **Unit type** | A registered implementation contract identified by a stable name such as `nav.astar/v1`. |
| **Unit instance** | One configured occurrence of a Unit type in a Module, identified by a Module-local name such as `planner`. |
| **Port** | A named, typed Unit input or output. All V0 ports are required; optional business results use an explicit Resource representation such as `Option<T>` or a domain enum. |
| **Module input** | A Resource value and any required bounds supplied by the host for one run. |
| **Module output** | A Resource selected for return to the host after a successful run. |
| **Binding** | The association between one Unit port and one Resource name. |
| **Run** | One host-triggered attempt to transform Module inputs into Module outputs. |

## Type terms

| Term | Definition |
| --- | --- |
| **Resource semantic type** | A stable serialized identity such as `lidar.PointCloud/v1`. |
| **Concrete representation** | The Rust type and storage adapter registered for one semantic type in a Unit Registry. |
| **Resource type descriptor** | Registry metadata that associates a semantic type with its concrete representation, storage behavior, and optional inspection renderer. |
| **Runtime type check** | Internal verification that an erased value or slot uses the registered concrete representation. Rust `TypeId` may support this check but is not serialized. |

Within one Registry, one semantic type maps to one concrete representation. Multiple semantic types may intentionally use the same Rust type.

## Storage terms

| Term | Definition |
| --- | --- |
| **Logical Resource identity** | The Module-local identity used for dependencies, diagnostics, and outputs, independent of physical memory. |
| **Storage requirement** | The size, alignment, representation, capacity, and memory-class requirement for a Resource output. |
| **Storage slot** | Prepared physical storage that may back one or more compatible Resources with non-overlapping live ranges. |
| **Capacity bound** | A maximum number of elements or bytes accepted without growth. |
| **Workspace requirement** | Temporary storage needed during one Unit invocation but not published as a Resource. |
| **Preparation** | The Module-build stage that resolves requirements, plans slots and workspaces, allocates storage, constructs Units, and optionally warms them up. |
| **Steady state** | Runs after successful Module build and any documented warm-up. |
| **Capacity overflow** | A structured error indicating that a bounded output or workspace requirement was exceeded. |
| **Allocation domain** | One allocator or allocation mechanism declared by a Unit or adapter as participating in the run boundary. |
| **Allocation certification** | A trusted, inspectable assertion that one declared domain is allocation-free during the run boundary when it cannot be instrumented. Its completeness is not mechanically provable for arbitrary native code. |
| **No-run-allocation guarantee** | An opt-in guarantee that steady-state `run` performs no dynamic allocator operations in every declared allocation domain, subject to complete and correct domain declarations and certifications. |

Storage, slots, workspaces, and preparation are advanced API and implementation terms, not additional public model pillars.

## Internal terms

| Term | Definition |
| --- | --- |
| **Unit Registry** | The mapping from stable Unit type names to descriptors and factories compiled into a binary. |
| **Unit descriptor** | Static metadata and functions for ports, types, configuration, storage requirements, workspace requirements, allocation capability, and construction. |
| **Compiled graph** | Internal validated representation containing identities, bindings, dependencies, stable execution order, and live ranges. |
| **Storage plan** | Internal assignment of Resource outputs and Unit workspaces to prepared physical storage. |
| **Output writer** | A typed bounded handle through which a Unit initializes one declared output. |
| **Diagnostic sink** | An optional receiver for graph metadata, execution events, storage reports, and type-specific Resource renderings. |
| **Recoverable failure** | A run failure after which the Unit explicitly guarantees its private state remains valid for another run. |
| **Fatal failure** | A run failure after which the Module rejects further runs and must be replaced. |
| **Unwind panic** | A Rust panic that returns control through stack unwinding; UnitCompose catches it at the Unit boundary, drops pending outputs, and marks the Module fatally failed. |

`Plan` may be used internally as a synonym for a compiled graph or storage plan. Introductory APIs should not require users to distinguish Module Definition, graph plan, storage plan, and Module.

## Words to avoid in the public model

| Word | Reason |
| --- | --- |
| **World** | Suggests broad arbitrary access to global data rather than declared Resource bindings. |
| **Service locator** | Unit code must not discover undeclared data dynamically. |
| **Node** as the Unit term | Conflicts with ROS nodes and graph implementation vocabulary. |
| **Component** as the Unit term | Conflicts with ROS composition and host-framework components. |
| **Runtime** as the project category | Overstates the scope of an embeddable module library. |
| **Pipeline** as the complete model | Understates fan-out, fan-in, and general DAG composition. |
| **Zero-copy** without a boundary | Hides ownership, representation, device, synchronization, and lifetime constraints. |

## Deferred vocabulary

The following concepts are outside the V0 guarantee unless a future ADR introduces them:

- framework-managed persistent Resources;
- transactional commit or rollback;
- generalized external leases;
- automatic parallel scheduling;
- asynchronous Resource lifetime;
- device-memory migration;
- checkpointing and replay;
- distributed execution.
