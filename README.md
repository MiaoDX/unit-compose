# UnitCompose

![UnitCompose architecture](docs/assets/unit-compose-architecture.svg)

## From monolithic algorithms to composable systems

**UnitCompose** is an embeddable Rust framework for building performance-critical algorithms from typed, testable, and replaceable **Units**.

Connect Units through named **Resources**, compose them into configuration-driven DAGs, and prepare storage before execution. Algorithm teams can evolve implementations and compositions independently, while host applications retain control of lifecycle, communication, threading, and deployment. The same binary can load different algorithm compositions without rewriting the host or hard-coding the graph in source code.

UnitCompose is built on lessons from 1,000 days of engineering across 10+ algorithm teams, 1,000+ engineers, and software shipped at million-product scale. It is designed for autonomous driving and embedded intelligence, and for any system that needs predictable performance without sacrificing a decoupled development experience.

UnitCompose targets the layer *inside* a larger component. A ROS node, service, simulator, offline tool, or another framework owns the Module and decides when to build, run, replace, and destroy it. UnitCompose does not take over the application lifecycle or communication system.

## Inspection at a glance

<table>
  <tr>
    <th>Prepared Module</th>
    <th>Measured over 1,000 runs</th>
  </tr>
  <tr>
    <td><a href="docs/assets/navigation-module.svg"><img src="docs/assets/navigation-module.svg" alt="Prepared navigation Module Resource DAG" width="100%"></a></td>
    <td><a href="docs/assets/navigation-module-timed.svg"><img src="docs/assets/navigation-module-timed.svg" alt="Navigation Module Resource DAG with Unit timing summaries" width="100%"></a></td>
  </tr>
  <tr>
    <th colspan="2">Interactive navigation episode</th>
  </tr>
  <tr>
    <td colspan="2"><a href="https://app.rerun.io/version/0.24.1/?url=https%3A%2F%2Fraw.githubusercontent.com%2FMiaoDX%2Funit-compose%2Fmain%2Fdocs%2Fassets%2Fnavigation-astar.rrd"><img src="docs/assets/navigation-rerun-preview.png" alt="Rerun navigation recording with map, paths, Unit timings, and run metrics" width="100%"></a></td>
  </tr>
</table>

[View the latest CI-generated reports for all four demos](https://miaodx.github.io/unit-compose/demos/).
Each successful `main` build publishes fresh Module graphs, measured timings,
run metrics, and interactive Rerun recordings. Pull requests retain the same
report as a downloadable GitHub Actions artifact.

The navigation report is also a YAML-only A/B comparison: one compiled binary
loads an A* + smoothing Module and a Dijkstra Module without smoothing. The
combined page compares their maps, true paths, topology, storage, and timing,
while retaining an independent Rerun recording for each execution.

The prepared graph is available before execution; the measured view adds timing
from completed runs. Click the Rerun preview to inspect the recorded episode.

## Core model

UnitCompose has three public concepts:

- **Unit**: one independently testable computation step with declared ports;
- **Resource**: one named, typed value connecting Module inputs and Unit outputs;
- **Module**: one validated Resource DAG, prepared once and run by its host.

The host registers available Unit implementations. YAML selects the implementations
and connections, while Module construction validates the graph and prepares its
storage. Inspection, timing, diagnostics, and visualization remain read-only views
of that Module.

## Explore

- [Registration showcase guide](docs/implementation/registration-showcases.md)
  contains local commands, verified datasets, expected results, and recording details.
- [LiDAR SLAM showcase guide](docs/implementation/lidar-slam-showcase.md)
  covers the offline 480-frame figure-eight episode, verified loop closures,
  and bounded outputs.
- [Documentation index](docs/README.md) routes to the concept model, terminology,
  architecture contract, implementation evidence, and design decisions.
- [V0 architecture specification](docs/specification/v0-architecture.md) is the
  normative source for supported behavior and boundaries.

## License

This project is licensed under the [MIT License](LICENSE).
