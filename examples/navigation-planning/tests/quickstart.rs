use std::path::{Path, PathBuf};

use navigation_planning::{
    ExecutionEvidence, GridPoint, MAX_CELLS, NavigationHost, RosOccupancyGrid, build_from_path,
    build_from_source, demo_grid,
};
use unit_compose_allocation_test_harness::GlobalProbe;
use unit_compose_core::{
    AllocationOperations, FixedModuleDescription, InputValidationError, ModuleInput, ResourceId,
    RunError, RunEventKind, RunReportSnapshot, SemanticType, TimingScope,
};
use unit_compose_debug::{
    AdapterController, AdapterDescriptor, AdapterExecution, AdapterFailurePolicy, AdapterOutcome,
    BoundedRunSink, InspectionAdapter,
};

fn definition(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(name)
}

fn expected_occupied_after_inflation(input: &RosOccupancyGrid, radius: usize) -> usize {
    (0..input.height)
        .flat_map(|y| (0..input.width).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            input.data.iter().enumerate().any(|(index, occupancy)| {
                (*occupancy < 0 || *occupancy >= 50)
                    && x.abs_diff(index % input.width) <= radius
                    && y.abs_diff(index / input.width) <= radius
            })
        })
        .count()
}

fn evidence_delta(after: ExecutionEvidence, before: ExecutionEvidence) -> ExecutionEvidence {
    ExecutionEvidence {
        decoder: after.decoder - before.decoder,
        inflation: after.inflation - before.inflation,
        planner: after.planner - before.planner,
        stats: after.stats - before.stats,
        smoother: after.smoother - before.smoother,
        occupied_cost_map_cells: after.occupied_cost_map_cells,
    }
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
    let supplied = prepared.supplied_input::<RosOccupancyGrid>(input.data.len());
    prepared
        .run_checked_profiled(&[supplied], input, &mut [&mut probe])
        .unwrap()
}

#[test]
fn checked_execution_observes_exact_stage_deltas_and_cost_map_stats() {
    let input = demo_grid();
    let expected_occupied = expected_occupied_after_inflation(&input, 1);
    for (name, smoothing) in [
        ("astar.yaml", true),
        ("dijkstra.yaml", true),
        ("astar-no-smoothing.yaml", false),
    ] {
        let mut prepared = build_from_path(&definition(name)).unwrap();
        prepared.warm_up(&input).unwrap();
        let before = prepared.execution_evidence();
        let supplied = prepared.supplied_input::<RosOccupancyGrid>(input.data.len());
        let mut probe = GlobalProbe;
        prepared
            .run_checked_profiled(&[supplied], &input, &mut [&mut probe])
            .unwrap();
        assert_eq!(
            evidence_delta(prepared.execution_evidence(), before),
            ExecutionEvidence {
                decoder: 1,
                inflation: 1,
                planner: 1,
                stats: 1,
                smoother: usize::from(smoothing),
                occupied_cost_map_cells: Some(expected_occupied),
            }
        );
    }
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
fn inspection_reporting_and_bounded_sink_do_not_change_results() {
    let input = demo_grid();
    let mut reported = build_from_path(&definition("astar.yaml")).unwrap();
    let mut disabled = build_from_path(&definition("astar.yaml")).unwrap();
    let mut bounded = build_from_path(&definition("astar.yaml")).unwrap();
    reported.warm_up(&input).unwrap();
    disabled.warm_up(&input).unwrap();
    bounded.warm_up(&input).unwrap();
    disabled.module.set_reporting_enabled(false);

    let mut probe = GlobalProbe;
    let expected = reported
        .module
        .run_profiled(&input, &mut [&mut probe], None)
        .unwrap()
        .to_vec();
    let without_report = disabled
        .module
        .run_profiled(&input, &mut [&mut probe], None)
        .unwrap()
        .to_vec();
    let mut sink = BoundedRunSink::<1>::default();
    let with_sink = bounded
        .module
        .run_profiled(&input, &mut [&mut probe], Some(&mut sink))
        .unwrap()
        .to_vec();

    assert_eq!(without_report, expected);
    assert_eq!(with_sink, expected);
    assert_eq!(disabled.module.report().events().count(), 0);
    assert_eq!(sink.events().count(), 1);
    assert_eq!(
        bounded.module.report().allocation_operations(),
        AllocationOperations::default()
    );
    for _ in 0..100 {
        assert_eq!(
            bounded
                .module
                .run_profiled(&input, &mut [&mut probe], Some(&mut sink))
                .unwrap(),
            expected
        );
        assert_eq!(
            bounded.module.report().allocation_operations(),
            AllocationOperations::default()
        );
    }
    assert_eq!(sink.dropped_events(), 100);
}

#[test]
fn fixed_description_and_renderers_are_stable_across_runs_and_failures() {
    let input = demo_grid();
    let mut prepared = build_from_path(&definition("astar.yaml")).unwrap();
    let fixed = prepared.description.clone();
    let text = fixed.to_text();
    let dot = fixed.to_dot();
    let mermaid = fixed.to_mermaid();
    prepared.warm_up(&input).unwrap();
    let mut probe = GlobalProbe;
    prepared
        .module
        .run_profiled(&input, &mut [&mut probe], None)
        .unwrap();
    assert_eq!(prepared.description, fixed);
    assert_eq!(prepared.description.to_text(), text);
    assert_eq!(prepared.description.to_dot(), dot);
    assert_eq!(prepared.description.to_mermaid(), mermaid);
    assert!(text.contains("config plan: max_cells=256,max_expansions=256,max_path=64"));
    assert!(text.contains("requirement raw_path: capacity=64"));
    assert!(text.contains("storage raw_path: slot="));
    assert!(text.contains("storage peak: slots="));
    assert!(text.contains("allocation domain rust-global: Instrumented"));
    assert!(text.contains("description overhead:"));
    assert!(text.contains("rendering overhead:"));

    let source = std::fs::read_to_string(definition("astar-no-smoothing.yaml"))
        .unwrap()
        .replace("max_path: 64", "max_path: 4");
    let mut failing = build_from_source(&source).unwrap();
    let failed_fixed = failing.description.clone();
    assert!(matches!(
        failing.module.warm_up(&input),
        Err(RunError::Capacity(_))
    ));
    assert_eq!(failing.description, failed_fixed);
}

#[test]
fn timing_and_bounded_overflow_report_their_scope_and_overhead() {
    let input = demo_grid();
    let mut prepared = build_from_path(&definition("astar.yaml")).unwrap();
    prepared.warm_up(&input).unwrap();
    let mut probe = GlobalProbe;
    prepared
        .module
        .run_profiled(&input, &mut [&mut probe], None)
        .unwrap();
    let event = prepared.module.report().events().next().unwrap();
    assert_eq!(event.timing_scope, TimingScope::ModuleExecution);
    assert_eq!(event.timing_overhead.clock_reads, 2);
    assert!(!event.timing_overhead.bounded_report_write_in_elapsed);

    use unit_compose_core::DiagnosticSink;
    let mut sink = BoundedRunSink::<1>::default();
    for capacity in 0..4 {
        sink.record(unit_compose_core::RunEvent {
            observed_capacity: capacity,
            ..*event
        });
    }
    assert_eq!(sink.events().next().unwrap().observed_capacity, 0);
    assert_eq!(sink.dropped_events(), 3);
}

struct FailingAdapter;

impl InspectionAdapter for FailingAdapter {
    type Error = &'static str;

    fn descriptor(&self) -> AdapterDescriptor {
        AdapterDescriptor {
            name: "navigation-test",
            execution: AdapterExecution::PostRunAllocating,
            allocation_domains: &["rust-global"],
            overhead: "post-run test failure",
        }
    }

    fn fixed_description(&mut self, _: &FixedModuleDescription) -> Result<(), Self::Error> {
        Err("unavailable")
    }

    fn run_snapshot(&mut self, _: &RunReportSnapshot) -> Result<(), Self::Error> {
        Err("unavailable")
    }
}

#[test]
fn adapter_failure_disables_separately_without_corrupting_module() {
    let input = demo_grid();
    let mut prepared = build_from_path(&definition("astar.yaml")).unwrap();
    prepared.warm_up(&input).unwrap();
    let mut probe = GlobalProbe;
    let expected = prepared
        .module
        .run_profiled(&input, &mut [&mut probe], None)
        .unwrap()
        .to_vec();
    let snapshot = prepared.module.report().snapshot();
    let fixed = prepared.description.clone();
    let mut adapter = AdapterController::new(FailingAdapter, AdapterFailurePolicy::Disable);
    assert_eq!(
        adapter.run_snapshot(&snapshot).unwrap(),
        AdapterOutcome::DisabledAfterFailure
    );
    assert!(!adapter.is_enabled());
    assert_eq!(prepared.description, fixed);
    assert_eq!(
        prepared
            .module
            .run_profiled(&input, &mut [&mut probe], None)
            .unwrap(),
        expected
    );
}

#[test]
fn inspection_product_command_exercises_all_stable_renderers() {
    for (view, marker) in [
        ("text", "storage peak:"),
        ("dot", "digraph \"navigation-astar\""),
        ("mermaid", "flowchart TD"),
    ] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_navigation-planning"))
            .args([
                "--module",
                definition("astar.yaml").to_str().unwrap(),
                "--inspect",
                view,
            ])
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(String::from_utf8(output.stdout).unwrap().contains(marker));
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
    let before = prepared.execution_evidence();
    assert!(matches!(
        prepared.module.warm_up(&oversized),
        Err(RunError::InvalidInput { .. })
    ));
    assert_eq!(prepared.execution_evidence(), before);

    let invalid_length = RosOccupancyGrid {
        width: 2,
        height: 2,
        data: vec![0; 3],
        start: GridPoint { x: 0, y: 0 },
        goal: GridPoint { x: 1, y: 1 },
    };
    assert!(matches!(
        prepared.module.warm_up(&invalid_length),
        Err(RunError::InvalidInput { .. })
    ));
    assert_eq!(prepared.execution_evidence(), before);

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
    let before = prepared.execution_evidence();
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
        assert_eq!(prepared.execution_evidence(), before);
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
fn repeated_successful_and_failed_reloads_preserve_atomic_activation() {
    let input = demo_grid();
    let mut active = build_from_path(&definition("astar.yaml")).unwrap();
    active.warm_up(&input).unwrap();
    let mut host = NavigationHost::new(active);

    let mut blocked = demo_grid();
    let start = usize::from(blocked.start.y) * blocked.width + usize::from(blocked.start.x);
    blocked.data[start] = 100;
    for round in 0..24 {
        let expected_before = host.active().graph.module.clone();
        assert!(host.reload(&definition("dijkstra.yaml"), &blocked).is_err());
        assert_eq!(host.active().graph.module, expected_before);

        let candidate = if round % 2 == 0 {
            "dijkstra.yaml"
        } else {
            "astar.yaml"
        };
        let old = host.reload(&definition(candidate), &input).unwrap();
        assert_eq!(old.graph.module, expected_before);
        let expected_after = if round % 2 == 0 {
            "navigation-dijkstra"
        } else {
            "navigation-astar"
        };
        assert_eq!(host.active().graph.module, expected_after);

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
}

#[test]
fn activated_host_can_run_while_returned_old_output_remains_borrowed() {
    let input = demo_grid();
    let mut active = build_from_path(&definition("astar.yaml")).unwrap();
    let mut candidate = build_from_path(&definition("dijkstra.yaml")).unwrap();
    active.warm_up(&input).unwrap();
    candidate.warm_up(&input).unwrap();
    let mut host = NavigationHost::new(active);
    let mut old = host.activate(candidate);
    assert_eq!(host.active().graph.module, "navigation-dijkstra");
    assert_eq!(old.graph.module, "navigation-astar");

    let mut old_probe = GlobalProbe;
    let retained = old
        .module
        .run_profiled(&input, &mut [&mut old_probe], None)
        .unwrap();
    let retained_first = retained[0];
    let mut candidate_probe = GlobalProbe;
    let candidate_path = host
        .active_mut()
        .module
        .run_profiled(&input, &mut [&mut candidate_probe], None)
        .unwrap();
    assert_eq!(retained[0], retained_first);
    assert_eq!(candidate_path.first(), Some(&retained_first));
    // A mutable rerun of `old.module` here cannot compile while `retained` is live;
    // core's `Module::run` compile-fail doctest is the executable compile-time proof.
}
