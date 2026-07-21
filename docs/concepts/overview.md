# Concept overview

UnitCompose organizes the internal computation of one host-level component.

The public model is intentionally small:

```text
Module Definition --compile--> Plan --instantiate--> Module --run--> Result
                                           |
                                           +-- Units
                                           `-- Resources
```

## Module

A **Module** is a live, embeddable instance that owns Unit instances, framework-managed Resources, persistent state, and execution status.

A host application decides when to create, run, inspect, and shut down a Module. A Module is not a process, service, ROS node, or application by itself, although any of those may contain one or more Modules.

## Unit

A **Unit** is the smallest scheduled computation inside a Module.

A Unit:

- has inspectable configuration and Resource access declarations;
- may hold private algorithm state and prepared handles;
- executes at most once in a normal Alpha run;
- receives access only to the Resources it declared;
- cannot decide dynamically which predecessor value an access should read.

## Resource

A **Resource** is a typed logical value or state item used by Units.

Examples include:

- host input for the current run;
- an intermediate point cloud or tensor;
- persistent tracking state;
- a read-only model parameter;
- an explicitly exported result.

Logical Resource identity is separate from physical storage. A Resource may be backed by owned memory, borrowed host memory, a device buffer, or another compatible representation.

## Plan

A **Plan** is the validated, language-neutral description of:

- Unit instances and normalized configuration;
- Resources, types, schemas, and lifetimes;
- Unit-to-Resource bindings and access intents;
- predecessor value selection;
- derived dependencies and conflicts;
- explicit ordering policies and exports.

Source declaration order does not silently become execution order.

## Scheduler

A **Scheduler** executes ready Units according to the Plan. It is an advanced and mostly internal concept.

Alpha uses a stable sequential scheduler as the reference. A future parallel scheduler must preserve the same successful-run Resource semantics.

## Run

A **Run** is one complete processing attempt for one request, frame, or tick.

A run either:

- commits all framework-controlled persistent updates and exposes all required exports; or
- commits none of them and returns a structured failure.

If Unit code has begun and the run fails, the Alpha Module becomes poisoned and cannot be reused. This avoids pretending that arbitrary Unit-private state can be rolled back.

## Why this boundary

The model separates four responsibilities:

| Responsibility | Concept |
| --- | --- |
| Business computation | Unit |
| Typed data and state | Resource |
| Static composition and constraints | Plan |
| Live ownership and execution | Module |

Everything else should remain an internal mechanism until a user-visible semantic boundary requires a name.
