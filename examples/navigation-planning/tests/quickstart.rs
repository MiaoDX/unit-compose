use std::path::{Path, PathBuf};

use navigation_planning::{
    GridPoint, MAX_CELLS, NavigationHost, RosOccupancyGrid, build_from_path, build_from_source,
    demo_grid,
};
use unit_compose_allocation_test_harness::GlobalProbe;
use unit_compose_core::{
    AllocationOperations, InputValidationError, ModuleInput, ResourceId, RunError, RunEventKind,
    SemanticType,
};

fn definition(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(name)
}

fn planner_type(graph: &unit_compose_core::CompiledGraph) -> &str {
    graph
        .units
        .iter()
        .find(|unit| unit.id.as_str() == "plan")
        .unwrap()
        .unit_type
        .as_str()
}

fn run_strict(
    prepared: &mut navigation_planning::PreparedNavigation,
    input: &RosOccupancyGrid,
) -> Vec<GridPoint> {
    prepared.warm_up(input).unwrap();
    let mut probe = GlobalProbe;
    prepared
        .module
        .run_profiled(input, &mut [&mut probe], None)
        .unwrap()
        .to_vec()
}

#[test]
fn deterministic_algorithms_and_yaml_variants_execute_end_to_end() {
    let input = demo_grid();
    let mut astar = build_from_path(&definition("astar.yaml")).unwrap();
    let mut dijkstra = build_from_path(&definition("dijkstra.yaml")).unwrap();
    let mut raw = build_from_path(&definition("astar-no-smoothing.yaml")).unwrap();

    let astar_path = run_strict(&mut astar, &input);
    let dijkstra_path = run_strict(&mut dijkstra, &input);
    let raw_path = run_strict(&mut raw, &input);
    assert_eq!(astar_path.first(), Some(&input.start));
    assert_eq!(astar_path.last(), Some(&input.goal));
    assert_eq!(dijkstra_path, astar_path);
    assert!(raw_path.len() > astar_path.len());
    assert_eq!(raw_path.first(), Some(&input.start));
    assert_eq!(raw_path.last(), Some(&input.goal));
}

#[test]
fn graphs_prove_replacement_restructure_and_cost_map_fan_out() {
    let astar = build_from_path(&definition("astar.yaml")).unwrap();
    let dijkstra = build_from_path(&definition("dijkstra.yaml")).unwrap();
    let raw = build_from_path(&definition("astar-no-smoothing.yaml")).unwrap();
    assert_eq!(planner_type(&astar.graph), "nav.astar/v1");
    assert_eq!(planner_type(&dijkstra.graph), "nav.dijkstra/v1");
    assert_eq!(astar.graph.units.len(), 5);
    assert_eq!(raw.graph.units.len(), 4);
    assert!(
        raw.graph
            .units
            .iter()
            .all(|unit| unit.id.as_str() != "smooth")
    );
    for graph in [&astar.graph, &dijkstra.graph, &raw.graph] {
        let cost_map = graph
            .resources
            .iter()
            .find(|resource| resource.id.as_str() == "cost_map")
            .unwrap();
        let consumers: Vec<_> = cost_map
            .consumers
            .iter()
            .map(|consumer| consumer.unit.as_str())
            .collect();
        assert!(consumers.contains(&"plan"));
        assert!(consumers.contains(&"stats"));
    }
}

#[test]
fn measured_runs_are_allocation_free_after_explicit_warm_up() {
    for name in ["astar.yaml", "dijkstra.yaml", "astar-no-smoothing.yaml"] {
        let input = demo_grid();
        let mut prepared = build_from_path(&definition(name)).unwrap();
        prepared.warm_up(&input).unwrap();
        let mut probe = GlobalProbe;
        for _ in 0..1_000 {
            let path = prepared
                .module
                .run_profiled(&input, &mut [&mut probe], None)
                .unwrap();
            assert!(!path.is_empty());
            assert_eq!(
                prepared.module.report().allocation_operations(),
                AllocationOperations::default()
            );
        }
    }
}

#[test]
fn bounded_map_search_and_path_overflow_are_recoverable() {
    let mut prepared = build_from_path(&definition("astar-no-smoothing.yaml")).unwrap();
    let oversized = RosOccupancyGrid {
        width: MAX_CELLS + 1,
        height: 1,
        data: vec![0; MAX_CELLS + 1],
        start: GridPoint { x: 0, y: 0 },
        goal: GridPoint { x: 1, y: 0 },
    };
    assert!(matches!(
        prepared.module.warm_up(&oversized),
        Err(RunError::InvalidInput { .. })
    ));

    let source = std::fs::read_to_string(definition("astar-no-smoothing.yaml"))
        .unwrap()
        .replace("max_path: 64", "max_path: 4");
    let mut short = build_from_source(&source).unwrap();
    assert!(matches!(
        short.module.warm_up(&demo_grid()),
        Err(RunError::Capacity(_))
    ));
    assert_eq!(
        short.module.report().events().next().unwrap().kind,
        RunEventKind::Overflow
    );

    let source = std::fs::read_to_string(definition("astar.yaml"))
        .unwrap()
        .replace("max_expansions: 256", "max_expansions: 2");
    let mut shallow = build_from_source(&source).unwrap();
    assert!(matches!(
        shallow.module.warm_up(&demo_grid()),
        Err(RunError::Capacity(ref error)) if error.resource == "search_workspace"
    ));
}

#[test]
fn invalid_named_inputs_are_rejected_before_execution_and_module_stays_runnable() {
    let input = demo_grid();
    let mut prepared = build_from_path(&definition("astar.yaml")).unwrap();
    prepared.warm_up(&input).unwrap();
    let valid = prepared.supplied_input::<RosOccupancyGrid>(input.data.len());
    let wrong_semantic = SemanticType::new("nav.Wrong/v1").unwrap();
    let cases = [
        (vec![], "missing"),
        (
            vec![
                valid.clone(),
                ModuleInput::of::<RosOccupancyGrid>(
                    ResourceId::new("extra"),
                    wrong_semantic.clone(),
                    1,
                    0,
                ),
            ],
            "unknown",
        ),
        (
            vec![ModuleInput::of::<RosOccupancyGrid>(
                ResourceId::new("occupancy_grid"),
                wrong_semantic,
                input.data.len(),
                0x4e41_5635,
            )],
            "semantic",
        ),
        (
            vec![prepared.supplied_input::<Vec<i8>>(input.data.len())],
            "concrete",
        ),
        (
            vec![prepared.supplied_input::<RosOccupancyGrid>(MAX_CELLS + 1)],
            "capacity",
        ),
    ];
    for (supplied, label) in cases {
        let error = prepared
            .module
            .run_checked(&prepared.input_plan, &supplied, &input)
            .unwrap_err();
        match (label, error) {
            ("missing", RunError::Input(InputValidationError::Missing { .. }))
            | ("unknown", RunError::Input(InputValidationError::Unknown { .. }))
            | ("semantic", RunError::Input(InputValidationError::SemanticType { .. }))
            | ("concrete", RunError::Input(InputValidationError::ConcreteType { .. }))
            | ("capacity", RunError::Input(InputValidationError::Capacity { .. })) => {}
            (_, other) => panic!("unexpected {label} result: {other:?}"),
        }
    }
    let mut probe = GlobalProbe;
    assert!(
        !prepared
            .module
            .run_profiled(&input, &mut [&mut probe], None)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn reload_is_atomic_and_changes_graph_and_result() {
    let input = demo_grid();
    let mut initial = build_from_path(&definition("astar.yaml")).unwrap();
    initial.warm_up(&input).unwrap();
    let mut host = NavigationHost::new(initial);
    let old_points = {
        let mut probe = GlobalProbe;
        host.active_mut()
            .module
            .run_profiled(&input, &mut [&mut probe], None)
            .unwrap()
            .len()
    };
    let old = host
        .reload(&definition("astar-no-smoothing.yaml"), &input)
        .unwrap();
    assert_eq!(old.graph.module, "navigation-astar");
    assert_eq!(host.active().graph.module, "navigation-astar-no-smoothing");
    let new_points = {
        let mut probe = GlobalProbe;
        host.active_mut()
            .module
            .run_profiled(&input, &mut [&mut probe], None)
            .unwrap()
            .len()
    };
    assert!(new_points > old_points);
}

#[test]
fn failed_construction_or_warm_up_preserves_old_runnable_module() {
    let input = demo_grid();
    let mut active = build_from_path(&definition("astar.yaml")).unwrap();
    active.warm_up(&input).unwrap();
    let mut host = NavigationHost::new(active);
    assert!(build_from_source("not: a module").is_err());

    let mut blocked = demo_grid();
    let start = usize::from(blocked.start.y) * blocked.width + usize::from(blocked.start.x);
    blocked.data[start] = 100;
    assert!(host.reload(&definition("dijkstra.yaml"), &blocked).is_err());
    assert_eq!(host.active().graph.module, "navigation-astar");
    let mut probe = GlobalProbe;
    assert!(
        !host
            .active_mut()
            .module
            .run_profiled(&input, &mut [&mut probe], None)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn borrowed_old_output_can_coexist_with_running_a_different_prepared_module() {
    let input = demo_grid();
    let mut old = build_from_path(&definition("astar.yaml")).unwrap();
    let mut candidate = build_from_path(&definition("dijkstra.yaml")).unwrap();
    old.warm_up(&input).unwrap();
    candidate.warm_up(&input).unwrap();
    let mut old_probe = GlobalProbe;
    let retained = old
        .module
        .run_profiled(&input, &mut [&mut old_probe], None)
        .unwrap();
    let retained_first = retained[0];
    let mut candidate_probe = GlobalProbe;
    let candidate_path = candidate
        .module
        .run_profiled(&input, &mut [&mut candidate_probe], None)
        .unwrap();
    assert_eq!(retained[0], retained_first);
    assert_eq!(candidate_path.first(), Some(&retained_first));
    // A mutable rerun of `old.module` here cannot compile while `retained` is live;
    // core's `Module::run` compile-fail doctest is the executable compile-time proof.
}
