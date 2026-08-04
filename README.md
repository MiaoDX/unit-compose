# UnitCompose

![UnitCompose architecture](docs/assets/unit-compose-architecture.svg)

**UnitCompose** is an embeddable, configuration-driven framework for decomposing one algorithm or functional module into typed **Units** connected through named **Resources**.

A host program compiles the Unit implementations it supports, loads a YAML Module Definition, validates the resulting Resource DAG, prepares framework-managed output storage and scratch workspace, and executes the Module. The same binary can load different algorithm compositions without rewriting the host or hard-coding the graph in source code.

UnitCompose targets the layer *inside* a larger component. A ROS node, service, simulator, offline tool, or another framework owns the Module and decides when to build, run, replace, and destroy it. UnitCompose does not take over the application lifecycle or communication system.

## Core model

UnitCompose keeps the public model intentionally small:

- **Unit** — one independently understandable and testable computation step with declared input and output ports;
- **Resource** — one named, typed logical value produced by a Module input or Unit output and consumed read-only by zero or more Units;
- **Module** — one validated, prepared, immutable Resource DAG owned by a host;
- **Debug** — read-only inspection, diagnostics, timing, storage reports, and optional Resource visualization.

Registries, compiled graphs, storage slots, workspace stacks, and the sequential executor are implementation mechanisms rather than additional concepts that ordinary users must learn.

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
    config: { diagonal_motion: true, max_expanded_nodes: 200000 }
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

Changing `nav.astar/v1` to `nav.dijkstra/v1` changes the algorithm without changing host code or Resource bindings. Removing the smoother and exporting `raw_plan` changes the DAG itself.

## Prepared storage

Resource identity is separate from physical storage. A Unit declares its output storage and scratch workspace requirements before execution. Module construction then:

1. validates the graph and configuration;
2. resolves Resource semantic types to concrete Rust representations;
3. computes stable execution order and Resource live ranges;
4. plans compatible output slots and Unit workspaces;
5. allocates and optionally warms up the prepared Module.

During `run`, a Unit reads declared inputs and writes into framework-provided outputs and scratch space. The default managed mode prioritizes a friendly development path. An opt-in strict profile requires fixed or bounded capacities and verifies that steady-state runs perform no dynamic allocator operations in every declared allocation domain.

## V0 behavior

V0 establishes the following baseline:

- every required Unit input binds to exactly one Resource;
- every Resource has exactly one producer and may have multiple read-only consumers;
- semantic Resource types, concrete runtime representations, port contracts, configuration, bounds, cycles, and duplicate producers are validated before Unit business code runs;
- Units execute once in a stable topological order;
- a Unit publishes its complete output set only after successful output validation;
- output and scratch storage can be prepared by the framework rather than allocated inside `Unit::run`;
- compatible typed storage may be reused when Resource live ranges do not overlap;
- strict capacity overflow fails explicitly instead of growing a buffer;
- Unit private state may persist across runs, but the framework does not roll it back;
- configuration reload builds and prepares a new Module, then swaps it between runs;
- Debug can describe the DAG, execution order, storage plan, timing, failures, and selected Resource values.

## Intentionally deferred

V0 does not guarantee transactional commit or rollback, framework-managed persistent Resources, automatic parallel or asynchronous execution, dynamic native plugins, Python Unit authoring, generalized cross-language zero-copy leases, GPU memory planning, cross-type optimal memory packing, checkpointing, distributed execution, or in-place DAG mutation.

## Documentation

Start with:

- [Documentation index](docs/README.md)
- [Concept overview](docs/concepts/overview.md)
- [Terminology](docs/concepts/terminology.md)
- [ADR-0002: Configuration-driven Resource DAG](docs/adr/0002-configuration-driven-resource-dag.md)
- [ADR-0003: Framework-managed Resource storage](docs/adr/0003-framework-managed-resource-storage.md)
- [V0 architecture specification](docs/specification/v0-architecture.md)
- [V0 implementation plan](docs/plans/v0-implementation-plan.md)

## License

This project is licensed under the [MIT License](LICENSE).
