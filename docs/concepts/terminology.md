# Terminology

This document defines canonical UnitCompose terminology for V0.

## Public concepts

| Term | Definition |
| --- | --- |
| **UnitCompose** | The embeddable framework described by this repository. |
| **Unit** | One typed computation step with configuration, named input ports, named output ports, and optional private state. |
| **Resource** | A named value with a stable semantic type, produced by a Module input or one Unit output and consumed read-only by zero or more Units. |
| **Module** | A validated and instantiated static Resource DAG owned by a host application. |
| **Debug** | The read-only inspection, visualization, trace, and diagnostic surface for a Module and its runs. |

These four concepts form the durable user mental model.

## Composition terms

| Term | Definition |
| --- | --- |
| **Module Definition** | A source description, normally YAML, that selects Unit types, provides configuration, binds ports to Resource names, and declares Module inputs and outputs. |
| **Unit type** | A registered implementation contract identified by a stable name such as `nav.astar/v1`. |
| **Unit instance** | One configured occurrence of a Unit type in a Module, identified by a Module-local name such as `planner`. |
| **Port** | A named, typed input or output in a Unit type contract. |
| **Module input** | A Resource value supplied by the host for one run. |
| **Module output** | A Resource selected for return to the host after a successful run. |
| **Binding** | The association between one Unit port and one Resource name. |

## Implementation terms

These terms may appear in implementation and advanced documentation, but they are not additional public model pillars.

| Term | Definition |
| --- | --- |
| **Unit Registry** | The mapping from Unit type names to descriptors and factories compiled into a binary. |
| **Unit descriptor** | Static metadata for a Unit type: ports, semantic types, configuration decoder, and factory. |
| **Compiled graph** | Internal validated representation containing Unit instances, Resources, dependencies, and stable execution order. |
| **Value store** | Run-local implementation storage that maps Resource identities to values. It is not exposed to Unit code as a general service locator. |
| **Debug sink** | Optional receiver for graph metadata, execution events, and type-specific Resource renderings. |
| **Run** | One call that supplies Module inputs and attempts to produce Module outputs. |

`Plan` may be used internally as a synonym for compiled graph, but V0 APIs and introductory documentation should not require users to distinguish Module Definition, Plan, BoundPlan, and Module.

## V0 Resource vocabulary

V0 uses only producer, consumer, input, and output semantics:

```text
Module input or Unit output -> Resource -> Unit input or Module output
```

The following advanced terms are intentionally outside the V0 contract:

- Publication;
- Commit;
- Lease;
- Observe / Create / Update;
- staged successor;
- persistent or external Resource lifetime;
- transactional rollback;
- poisoned Module.

They may be reconsidered later without replacing Unit, Resource, Module, or Debug as the public model.

## Words to avoid in the public model

| Word | Reason |
| --- | --- |
| **World** | Suggests broad arbitrary access to global data rather than declared Resource bindings. |
| **Service locator** | Unit code must not discover undeclared data dynamically. |
| **Node** as the Unit term | Conflicts with ROS nodes and graph implementation vocabulary. |
| **Component** as the Unit term | Conflicts with ROS composition and host-framework components. |
| **Runtime** as the project category | Overstates the scope of an embeddable module library. |
| **Pipeline** as the complete model | Understates fan-out, fan-in, and general DAG composition. |

## Legacy mapping

| Earlier term | V0 term |
| --- | --- |
| Compute Module V2 | UnitCompose |
| Compute Module | Module |
| Executor Unit | Unit |
| World | Internal value store, where needed |
| Logical Plan / Execution Plan | Internal compiled graph |
| Process Cycle | Run |
