use std::collections::BTreeMap;
use std::hint::black_box;
use std::time::{Duration, Instant};

use unit_compose_core::{
    CompiledGraph, CompiledResource, CompiledUnit, Producer, ResourceDescriptor, ResourceId,
    ResourceRegistry, ResourceRequirement, SemanticType, UnitId, UnitTypeName, plan_storage,
};

fn observe(iterations: usize, mut operation: impl FnMut()) -> Duration {
    let start = Instant::now();
    for _ in 0..iterations {
        operation();
    }
    start.elapsed()
}

#[test]
#[ignore = "informational benchmark; run with --ignored --nocapture"]
fn hardening_benchmark_observations() {
    const BUILDS: usize = 1_000;
    let kind = SemanticType::new("bench.Value/v1").unwrap();
    let units: Vec<_> = (0..32)
        .map(|index| CompiledUnit {
            id: UnitId::new(format!("u{index}")),
            unit_type: UnitTypeName::new("bench.noop/v1"),
            inputs: vec![],
            outputs: vec![],
            dependencies: vec![],
        })
        .collect();
    let resources: Vec<_> = (0..32)
        .map(|index| CompiledResource {
            id: ResourceId::new(format!("r{index}")),
            semantic_type: kind.clone(),
            concrete_type: unit_compose_core::ConcreteType::of::<u64>(),
            concrete_name: std::any::type_name::<u64>(),
            producer: Producer::Unit {
                unit: UnitId::new(format!("u{index}")),
                port: "out".into(),
            },
            consumers: vec![],
        })
        .collect();
    let graph = CompiledGraph {
        schema: "unit-compose/v0alpha1".into(),
        module: "benchmark".into(),
        execution_order: units.iter().map(|unit| unit.id.clone()).collect(),
        units,
        resources,
        module_outputs: vec![],
    };
    let mut registry = ResourceRegistry::default();
    registry
        .register(ResourceDescriptor::of::<u64>(kind, "value", "fixed"))
        .unwrap();
    let requirements: BTreeMap<_, _> = (0..32)
        .map(|index| {
            (
                ResourceId::new(format!("r{index}")),
                ResourceRequirement { capacity: 1 },
            )
        })
        .collect();
    let slot_reuse = observe(BUILDS, || {
        let plan = plan_storage(&graph, &registry, &requirements).unwrap();
        assert_eq!(plan.report().slot_count, 1);
        black_box(plan);
    });

    println!("slot_reuse_planning: {BUILDS} x 32 resources in {slot_reuse:?}");

    assert!(slot_reuse > Duration::ZERO);
}
