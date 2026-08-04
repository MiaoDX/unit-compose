use std::collections::BTreeMap;
use std::hint::black_box;
use std::time::{Duration, Instant};

use unit_compose_core::{
    AllocationCapability, BoundedStorage, BuildOptions, CompiledGraph, CompiledResource,
    CompiledUnit, OutputStorage, PendingOutputSet, Producer, RequirementStatus, ResourceDescriptor,
    ResourceId, ResourceRegistry, ResourceRequirement, SemanticType, Unit, UnitId, UnitTypeName,
    UnitWorkspace, ValueStorage, WorkspaceBacking, WorkspaceRequirement, plan_storage,
};

struct NoOp;

impl Unit for NoOp {
    type Input = ();
    type Storage = ValueStorage<()>;

    fn workspace_requirement(&self) -> usize {
        0
    }

    fn output_storage(&self) -> Self::Storage {
        ValueStorage::new("unit")
    }

    fn allocation_capability(&self) -> AllocationCapability {
        AllocationCapability::inspect(vec![], false)
    }

    fn requirement_status(&self) -> RequirementStatus {
        RequirementStatus::Fixed
    }

    fn run(
        &mut self,
        _input: &Self::Input,
        output: &mut <Self::Storage as OutputStorage>::Pending<'_>,
        _workspace: UnitWorkspace<'_>,
    ) -> Result<(), unit_compose_core::RunError> {
        output.write(());
        Ok(())
    }
}

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
    const FAST: usize = 10_000;
    const BUILDS: usize = 1_000;

    let build = observe(BUILDS, || {
        black_box(unit_compose_core::Module::build(
            NoOp,
            BuildOptions::development(),
        ))
        .unwrap();
    });
    let mut module = unit_compose_core::Module::build(NoOp, BuildOptions::development()).unwrap();
    let run = observe(FAST, || {
        black_box(module.run(&())).unwrap();
    });
    let writes = observe(FAST, || {
        let mut storage = BoundedStorage::new("buffer", 32);
        let mut pending = storage.begin();
        for value in 0..32_u32 {
            pending.try_push(black_box(value)).unwrap();
        }
        pending.complete();
        pending.validate_complete().unwrap();
        black_box(storage.view());
    });
    let publication = observe(FAST, || {
        let mut storage = ValueStorage::new("value");
        let mut pending = storage.begin();
        pending.write(black_box(42_u64));
        pending.validate_complete().unwrap();
        black_box(storage.view());
    });
    let workspace = observe(FAST, || {
        black_box(WorkspaceBacking::new(
            WorkspaceRequirement::for_type::<u64>(256),
        ));
    });

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

    println!("build_time: {BUILDS} iterations in {build:?}");
    println!("noop_unit: {FAST} iterations in {run:?}");
    println!("bounded_buffer_writes: {FAST} x 32 writes in {writes:?}");
    println!("pending_output_publication: {FAST} iterations in {publication:?}");
    println!("workspace_allocation: {FAST} iterations in {workspace:?}");
    println!("slot_reuse_planning: {BUILDS} x 32 resources in {slot_reuse:?}");

    assert!(build + run + writes + publication + workspace + slot_reuse > Duration::ZERO);
}
