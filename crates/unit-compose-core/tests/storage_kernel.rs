use std::collections::BTreeMap;
use std::mem::{align_of, size_of};

use proptest::prelude::*;
use unit_compose_core::{
    CompiledGraph, CompiledResource, CompiledUnit, Consumer, LiveRange, Producer,
    ResourceDescriptor, ResourceId, ResourceRegistry, ResourceRequirement, SemanticType,
    StorageRepresentation, UnitId, UnitTypeName, calculate_live_ranges, plan_storage,
};

fn semantic(name: &str) -> SemanticType {
    SemanticType::new(format!("test.{name}/v1")).unwrap()
}

fn graph(resources: Vec<CompiledResource>, outputs: &[&str], unit_count: usize) -> CompiledGraph {
    let execution_order: Vec<_> = (0..unit_count)
        .map(|index| UnitId::new(format!("u{index}")))
        .collect();
    CompiledGraph {
        schema: "unit-compose/v0alpha1".into(),
        module: "storage-test".into(),
        units: execution_order
            .iter()
            .map(|id| CompiledUnit {
                id: id.clone(),
                unit_type: UnitTypeName::new("test.unit/v1"),
                inputs: vec![],
                outputs: vec![],
                dependencies: vec![],
            })
            .collect(),
        resources,
        module_outputs: outputs.iter().map(|name| ResourceId::new(*name)).collect(),
        execution_order,
    }
}

fn produced(name: &str, kind: SemanticType, at: usize, consumers: &[usize]) -> CompiledResource {
    CompiledResource {
        id: ResourceId::new(name),
        semantic_type: kind,
        concrete_type: unit_compose_core::ConcreteType::of::<Vec<u32>>(),
        concrete_name: std::any::type_name::<Vec<u32>>(),
        producer: Producer::Unit {
            unit: UnitId::new(format!("u{at}")),
            port: "out".into(),
        },
        consumers: consumers
            .iter()
            .map(|index| Consumer {
                unit: UnitId::new(format!("u{index}")),
                port: "in".into(),
            })
            .collect(),
    }
}

#[test]
fn descriptor_owns_buffer_representation_including_zero_sized_and_overaligned_layouts() {
    #[repr(align(128))]
    struct Aligned;
    let fixed = ResourceDescriptor::fixed_buffer::<Vec<Aligned>, Aligned>(
        semantic("Aligned"),
        "fixed-buffer",
        "exact length",
    );
    assert_eq!(
        fixed.invariants().representation,
        StorageRepresentation::FixedBuffer
    );
    assert_eq!(fixed.invariants().element_alignment, 128);
    assert_eq!(fixed.invariants().element_size, size_of::<Aligned>());

    let zst = ResourceDescriptor::bounded_buffer::<Vec<()>, ()>(
        semantic("Zst"),
        "bounded-buffer",
        "bounded length",
    );
    assert_eq!(zst.invariants().element_size, 0);
    assert_eq!(zst.invariants().element_alignment, align_of::<()>());
    assert_eq!(
        zst.invariants().representation,
        StorageRepresentation::BoundedBuffer
    );
}

#[test]
fn module_outputs_are_live_through_run_end() {
    let graph = graph(
        vec![
            produced("temporary", semantic("Words"), 0, &[1]),
            produced("result", semantic("Words"), 1, &[]),
        ],
        &["result"],
        2,
    );
    let ranges = calculate_live_ranges(&graph).unwrap();
    assert_eq!(
        ranges[&ResourceId::new("temporary")],
        LiveRange { start: 0, end: 1 }
    );
    assert_eq!(
        ranges[&ResourceId::new("result")],
        LiveRange { start: 1, end: 2 }
    );
}

#[test]
fn conservative_planner_reuses_only_compatible_disjoint_slots() {
    let words = semantic("Words");
    let bytes = semantic("Bytes");
    let graph = graph(
        vec![
            produced("early", words.clone(), 0, &[]),
            produced("late", words.clone(), 1, &[]),
            produced("incompatible", bytes.clone(), 2, &[]),
        ],
        &[],
        3,
    );
    let mut registry = ResourceRegistry::default();
    registry
        .register(ResourceDescriptor::bounded_buffer::<Vec<u32>, u32>(
            words, "words", "bounded",
        ))
        .unwrap();
    registry
        .register(ResourceDescriptor::bounded_buffer::<Vec<u8>, u8>(
            bytes, "bytes", "bounded",
        ))
        .unwrap();
    let requirements = ["early", "late", "incompatible"]
        .map(|name| (ResourceId::new(name), ResourceRequirement { capacity: 4 }))
        .into_iter()
        .collect();
    let report = plan_storage(&graph, &registry, &requirements).unwrap();
    let assignments = &report.report().assignments;
    assert_eq!(assignments[0].slot, assignments[1].slot);
    assert_ne!(assignments[1].slot, assignments[2].slot);
    assert_eq!(report.report().slot_count, 2);
    assert_eq!(
        report.report().estimated_peak_bytes,
        4 * size_of::<u32>() + 4
    );
}

#[test]
fn unit_requirement_cannot_override_descriptor_authority() {
    let kind = semantic("Authority");
    let graph = graph(vec![produced("value", kind.clone(), 0, &[])], &[], 1);
    let mut registry = ResourceRegistry::default();
    registry
        .register(ResourceDescriptor::fixed_buffer::<Vec<u64>, u64>(
            kind,
            "authoritative-adapter",
            "exact",
        ))
        .unwrap();
    let requirements = BTreeMap::from([(
        ResourceId::new("value"),
        ResourceRequirement { capacity: 3 },
    )]);
    let report = plan_storage(&graph, &registry, &requirements).unwrap();
    assert_eq!(report.report().assignments[0].bytes, 3 * size_of::<u64>());
}

proptest! {
    #[test]
    fn live_ranges_follow_producer_consumers_and_output_rule(
        producer in 0usize..12,
        offsets in prop::collection::vec(0usize..12, 0..16),
        module_output in any::<bool>(),
    ) {
        let consumers: Vec<_> = offsets.into_iter().map(|offset| producer.max(offset)).collect();
        let unit_count = consumers.iter().copied().max().unwrap_or(producer) + 1;
        let outputs: Vec<&str> = if module_output { vec!["r"] } else { vec![] };
        let graph = graph(vec![produced("r", semantic("Property"), producer, &consumers)], &outputs, unit_count);
        let range = calculate_live_ranges(&graph).unwrap()[&ResourceId::new("r")];
        let expected_end = if module_output { unit_count } else { consumers.iter().copied().max().unwrap_or(producer) };
        prop_assert_eq!(range, LiveRange { start: producer, end: expected_end });
    }

    #[test]
    fn conservative_assignment_never_aliases_overlapping_ranges(
        starts in prop::collection::vec(0usize..16, 1..20),
    ) {
        let unit_count = starts.iter().copied().max().unwrap() + 1;
        let kind = semantic("Slots");
        let resources: Vec<_> = starts.iter().enumerate()
            .map(|(index, start)| produced(&format!("r{index}"), kind.clone(), *start, &[]))
            .collect();
        let graph = graph(resources, &[], unit_count);
        let mut registry = ResourceRegistry::default();
        registry.register(ResourceDescriptor::of::<u64>(kind, "value", "valid")).unwrap();
        let requirements = starts.iter().enumerate()
            .map(|(index, _)| (ResourceId::new(format!("r{index}")), ResourceRequirement { capacity: 1 }))
            .collect();
        let plan = plan_storage(&graph, &registry, &requirements).unwrap();
        for (index, left) in plan.report().assignments.iter().enumerate() {
            for right in &plan.report().assignments[index + 1..] {
                if left.slot == right.slot {
                    prop_assert!(!left.live_range.overlaps(right.live_range));
                }
            }
        }
    }
}
