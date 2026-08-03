# Concept overview

UnitCompose organizes the internal implementation of one host-level algorithm or functional component.

```text
compiled Unit implementations
           |
           v
       Unit Registry       YAML Module Definition
              \             /
               \           /
                validate and build
                       |
                       v
                     Module
                 /      |      \
              Units  Resources  Debug
                       |
                       v
               stable DAG execution
```

A ROS node, service, simulator, command-line tool, or another host owns the Module. The host supplies Module inputs, calls the Module, consumes Module outputs, and decides when a new configuration should replace the current Module.

## Unit

A **Unit** is the smallest computation step that is independently understandable, configurable, testable, replaceable, or useful to inspect.

A Unit type declares before construction:

- a stable implementation type name;
- a configuration type;
- named input ports and their Resource semantic types;
- named output ports and their Resource semantic types;
- a factory that creates a Unit instance from validated configuration.

A Unit instance may keep private state, such as a tracker history or prepared model handle. V0 does not inspect, persist, migrate, or roll back that private state.

## Resource

A **Resource** is a named, typed value in a Module.

A Resource is produced by exactly one of:

- a Module input; or
- one Unit output port.

It may be consumed by any number of Unit input ports and may be exposed as a Module output. Resources are immutable after publication in V0. A fan-out therefore shares one logical value with several read-only consumers rather than granting arbitrary shared mutation.

Intermediate Resources do not need an explicit top-level YAML declaration. An output binding creates the Resource name, the producer port provides its type, and consumers are checked against that type.

## Module

A **Module** is a validated, instantiated Resource DAG.

Construction resolves Unit types through the registry, validates configuration, derives dependencies from Resource producer-consumer relationships, rejects invalid graphs, and computes a stable topological order.

The DAG is immutable for the lifetime of a Module instance. A different YAML definition creates a different Module instance.

## Debug

**Debug** is the read-only inspection surface of a Module. It is not a service locator and does not let Unit code bypass Resource declarations.

Debug should support:

- describing Units, ports, Resources, producers, and consumers;
- exporting the graph as DOT or Mermaid;
- reporting the computed execution order;
- recording Unit start, finish, duration, and failure;
- rendering selected Resource values through optional type-specific adapters.

A Rerun integration can implement one Debug sink without making Rerun a dependency of Unit code or the core Resource model.

## Module Definition

A **Module Definition** is normally YAML. It chooses Unit implementations, provides instance configuration, binds Unit ports to Resource names, and selects Module outputs.

The binary still determines the set of available implementations. Changing YAML can select a different registered Unit or graph, but loading a previously unknown implementation requires a future plugin mechanism or a new binary.

## Execution

A V0 run follows a deliberately small contract:

1. validate the supplied Module inputs;
2. place them in the run-local Resource value store;
3. execute each Unit once in stable topological order;
4. insert each successful Unit's complete output set into the store;
5. stop on the first Unit error;
6. return the declared Module outputs on success.

The absence of a successful return value is not transactional rollback. Unit private state or external effects that occurred before an error are not reversed.

## Reload

Configuration changes use build-new-and-swap:

1. parse and validate a new Module Definition;
2. instantiate a new Module beside the current one;
3. replace the current Module between runs only after construction succeeds;
4. shut down the old Module.

V0 does not mutate an active graph in place and does not migrate private Unit state.
