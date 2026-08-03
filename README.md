# UnitCompose

![UnitCompose architecture](docs/assets/unit-compose-architecture.svg)

**UnitCompose** is an embeddable, configuration-driven framework for decomposing an algorithm or functional module into typed **Units** connected through named **Resources**.

A host program compiles the Unit implementations it supports, then loads a YAML Module Definition to select Unit types, configure instances, bind Resources, validate the resulting DAG, and execute it. The same binary can load different algorithm compositions without rewriting the host or hard-coding the graph in source code.

UnitCompose targets the layer *inside* a larger component. A ROS node, service, simulator, offline tool, or another framework owns the Module and decides when to run it. UnitCompose does not take over the application lifecycle or communication system.

## Core model

UnitCompose keeps the public model intentionally small:

- **Unit** — one independently understandable and testable computation step with declared input and output ports;
- **Resource** — a named, typed value produced by a Module input or one Unit and consumed by one or more Units;
- **Module** — a validated static Resource DAG created from a Module Definition;
- **Debug** — read-only inspection, visualization, timing, and error information for a Module and its runs.

Registries, graph compilation, value storage, and sequential execution are implementation mechanisms rather than additional concepts that ordinary users must learn.

## Configuration-driven composition

The binary registers available Unit implementations. YAML selects which implementations to instantiate and how Resources connect them:

```yaml
schema: unit-compose/v0alpha1
module: navigation_planning

inputs:
  map_asset: { type: nav.RosMapAsset/v1 }
  request:   { type: nav.PlanRequest/v1 }

units:
  map_decoder:
    type: nav.ros_map_decoder/v1
    inputs:  { asset: map_asset }
    outputs: { grid: occupancy_grid }

  inflation:
    type: nav.binary_inflation/v1
    config: { robot_radius_m: 0.22 }
    inputs:  { grid: occupancy_grid }
    outputs: { costmap: navigation_costmap }

  planner:
    type: nav.astar/v1
    config: { diagonal_motion: true }
    inputs:
      costmap: navigation_costmap
      request: request
    outputs: { plan: raw_plan }

  smoother:
    type: nav.line_of_sight_smoother/v1
    inputs:
      costmap: navigation_costmap
      plan: raw_plan
    outputs: { plan: final_plan }

outputs:
  plan: final_plan
```

Changing `nav.astar/v1` to another registered implementation with the same port contract, such as `nav.dijkstra/v1`, changes the algorithm without changing the host program. Removing the smoother and exporting `raw_plan` changes the DAG itself.

## V0 behavior

V0 deliberately establishes a small, useful baseline:

- Module Definitions are loaded from YAML at Module construction time;
- every required Unit input binds to exactly one Resource;
- every Resource has exactly one producer and may have multiple read-only consumers;
- Resource semantic types and Unit port contracts are validated before execution;
- cycles, unknown Unit types, invalid configuration, missing bindings, and type mismatches fail during Module construction;
- Units execute once in a stable topological order for each successful `Module::run` attempt;
- the first Unit error stops further launches and is returned with the Unit identity;
- Unit private state may persist across runs, but the framework does not roll it back;
- a Module instance has an immutable DAG; configuration reload builds a new Module and swaps it between runs;
- Debug can describe the DAG, export DOT or Mermaid, report Resource relationships, and record Unit timing.

## Intentionally deferred

The first implementation does **not** require transactional commit or rollback, managed persistent Resources, automatic parallel scheduling, asynchronous Units, a stable dynamic-plugin ABI, Python authoring, generalized zero-copy leases, buffer reuse, checkpointing, or in-place DAG mutation.

These capabilities may be added later through the existing Unit, Resource, Module, and Debug model when representative workloads justify them.

## Example direction

The first executable example is planned as a small navigation pipeline using an openly licensed Nav2 map, interchangeable A* and Dijkstra planners, and Rerun visualization. A larger optional showcase will process nuScenes mini LiDAR data and reuse Rerun's existing dataset visualization patterns.

## Documentation

Start with:

- [Documentation index](docs/README.md)
- [Concept overview](docs/concepts/overview.md)
- [Terminology](docs/concepts/terminology.md)
- [ADR-0004: Configuration-driven Resource DAG for V0](docs/adr/0004-configuration-driven-resource-dag.md)
- [V0 architecture specification](docs/specification/v0-architecture.md)

The earlier transactional execution design remains in repository history and research notes, but it is not the implementation contract for V0.

## License

This project is licensed under the [MIT License](LICENSE).
