use std::any::{TypeId, type_name};
use std::collections::{BTreeMap, BTreeSet};

use crate::{CompiledGraph, ResourceDescriptor, ResourceId, ResourceRegistry};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedInputSpec {
    pub resource: ResourceId,
    pub semantic_type: crate::SemanticType,
    concrete_type: TypeId,
    concrete_name: &'static str,
    pub maximum_capacity: usize,
    pub plan_token: u64,
}

impl PreparedInputSpec {
    #[must_use]
    pub fn of<T: 'static>(
        resource: ResourceId,
        semantic_type: crate::SemanticType,
        maximum_capacity: usize,
        plan_token: u64,
    ) -> Self {
        Self {
            resource,
            semantic_type,
            concrete_type: TypeId::of::<T>(),
            concrete_name: type_name::<T>(),
            maximum_capacity,
            plan_token,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModuleInput {
    pub resource: ResourceId,
    pub semantic_type: crate::SemanticType,
    concrete_type: TypeId,
    concrete_name: &'static str,
    pub capacity: usize,
    pub plan_token: u64,
}

impl ModuleInput {
    #[must_use]
    pub fn of<T: 'static>(
        resource: ResourceId,
        semantic_type: crate::SemanticType,
        capacity: usize,
        plan_token: u64,
    ) -> Self {
        Self {
            resource,
            semantic_type,
            concrete_type: TypeId::of::<T>(),
            concrete_name: type_name::<T>(),
            capacity,
            plan_token,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InputValidationError {
    DuplicatePrepared {
        resource: ResourceId,
    },
    Duplicate {
        resource: ResourceId,
    },
    Missing {
        resource: ResourceId,
    },
    Unknown {
        resource: ResourceId,
    },
    SemanticType {
        resource: ResourceId,
    },
    ConcreteType {
        resource: ResourceId,
        expected: &'static str,
        actual: &'static str,
    },
    Capacity {
        resource: ResourceId,
        supplied: usize,
        maximum: usize,
    },
    PreparedPlan {
        resource: ResourceId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedInputPlan {
    inputs: BTreeMap<ResourceId, PreparedInputSpec>,
}

impl PreparedInputPlan {
    pub fn new(
        inputs: impl IntoIterator<Item = PreparedInputSpec>,
    ) -> Result<Self, InputValidationError> {
        let mut prepared = BTreeMap::new();
        for input in inputs {
            let resource = input.resource.clone();
            if prepared.insert(resource.clone(), input).is_some() {
                return Err(InputValidationError::DuplicatePrepared { resource });
            }
        }
        Ok(Self { inputs: prepared })
    }

    pub fn validate(&self, supplied: &[ModuleInput]) -> Result<(), InputValidationError> {
        let supplied_by_name: BTreeMap<_, _> = supplied
            .iter()
            .map(|input| (&input.resource, input))
            .collect();
        if supplied_by_name.len() != supplied.len() {
            let mut seen = BTreeSet::new();
            let duplicate = supplied
                .iter()
                .find(|input| !seen.insert(&input.resource))
                .expect("length mismatch proves a duplicate");
            return Err(InputValidationError::Duplicate {
                resource: duplicate.resource.clone(),
            });
        }
        for resource in supplied_by_name.keys() {
            if !self.inputs.contains_key(*resource) {
                return Err(InputValidationError::Unknown {
                    resource: (*resource).clone(),
                });
            }
        }
        for (resource, expected) in &self.inputs {
            let actual =
                supplied_by_name
                    .get(resource)
                    .ok_or_else(|| InputValidationError::Missing {
                        resource: resource.clone(),
                    })?;
            if actual.semantic_type != expected.semantic_type {
                return Err(InputValidationError::SemanticType {
                    resource: resource.clone(),
                });
            }
            if actual.concrete_type != expected.concrete_type {
                return Err(InputValidationError::ConcreteType {
                    resource: resource.clone(),
                    expected: expected.concrete_name,
                    actual: actual.concrete_name,
                });
            }
            if actual.capacity > expected.maximum_capacity {
                return Err(InputValidationError::Capacity {
                    resource: resource.clone(),
                    supplied: actual.capacity,
                    maximum: expected.maximum_capacity,
                });
            }
            if actual.plan_token != expected.plan_token {
                return Err(InputValidationError::PreparedPlan {
                    resource: resource.clone(),
                });
            }
        }
        Ok(())
    }
}

/// Inclusive sequential execution interval for one logical Resource.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LiveRange {
    pub start: usize,
    pub end: usize,
}

impl LiveRange {
    #[must_use]
    pub const fn overlaps(self, other: Self) -> bool {
        self.start <= other.end && other.start <= self.end
    }
}

/// Unit-resolved quantity. Representation details deliberately remain absent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResourceRequirement {
    pub capacity: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SlotAssignment {
    pub resource: ResourceId,
    pub slot: usize,
    pub live_range: LiveRange,
    pub capacity: usize,
    pub bytes: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StorageReport {
    pub assignments: Vec<SlotAssignment>,
    pub slot_count: usize,
    pub estimated_peak_bytes: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoragePlan {
    report: StorageReport,
}

impl StoragePlan {
    #[must_use]
    pub const fn report(&self) -> &StorageReport {
        &self.report
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PlanningError {
    MissingDescriptor { resource: ResourceId },
    MissingRequirement { resource: ResourceId },
    SizeOverflow { resource: ResourceId },
    InvalidGraph { resource: ResourceId },
}

/// Calculates lifetimes from the normalized sequential schedule. Module
/// outputs remain live through the synthetic run-end step.
pub fn calculate_live_ranges(
    graph: &CompiledGraph,
) -> Result<BTreeMap<ResourceId, LiveRange>, PlanningError> {
    let positions: BTreeMap<_, _> = graph
        .execution_order
        .iter()
        .enumerate()
        .map(|(index, unit)| (unit, index))
        .collect();
    let module_outputs: BTreeSet<_> = graph.module_outputs.iter().collect();
    let run_end = graph.execution_order.len();
    let mut ranges = BTreeMap::new();

    for resource in &graph.resources {
        let start = match &resource.producer {
            crate::graph::Producer::ModuleInput => 0,
            crate::graph::Producer::Unit { unit, .. } => {
                *positions
                    .get(unit)
                    .ok_or_else(|| PlanningError::InvalidGraph {
                        resource: resource.id.clone(),
                    })?
            }
        };
        let mut end = start;
        for consumer in &resource.consumers {
            end = end.max(*positions.get(&consumer.unit).ok_or_else(|| {
                PlanningError::InvalidGraph {
                    resource: resource.id.clone(),
                }
            })?);
        }
        if module_outputs.contains(&resource.id) {
            end = run_end;
        }
        ranges.insert(resource.id.clone(), LiveRange { start, end });
    }
    Ok(ranges)
}

struct PlannedSlot<'a> {
    descriptor: &'a ResourceDescriptor,
    capacity: usize,
    bytes: usize,
    ranges: Vec<LiveRange>,
}

/// First-fit conservative assignment. Slots alias only for descriptor-equal
/// compatible representations with sufficient capacity and disjoint ranges.
pub fn plan_storage<'a>(
    graph: &CompiledGraph,
    registry: &'a ResourceRegistry,
    requirements: &BTreeMap<ResourceId, ResourceRequirement>,
) -> Result<StoragePlan, PlanningError> {
    let ranges = calculate_live_ranges(graph)?;
    let mut slots: Vec<PlannedSlot<'a>> = Vec::new();
    let mut assignments = Vec::new();

    for resource in &graph.resources {
        if matches!(resource.producer, crate::graph::Producer::ModuleInput) {
            continue;
        }
        let descriptor = registry.get(&resource.semantic_type).ok_or_else(|| {
            PlanningError::MissingDescriptor {
                resource: resource.id.clone(),
            }
        })?;
        let requirement =
            requirements
                .get(&resource.id)
                .ok_or_else(|| PlanningError::MissingRequirement {
                    resource: resource.id.clone(),
                })?;
        let range = ranges[&resource.id];
        let physical_capacity =
            if descriptor.invariants().representation == crate::StorageRepresentation::FixedValue {
                1
            } else {
                requirement.capacity
            };
        let backing_count = 2;
        let bytes = descriptor
            .invariants()
            .element_size
            .checked_mul(physical_capacity)
            .and_then(|bytes| bytes.checked_mul(backing_count))
            .ok_or_else(|| PlanningError::SizeOverflow {
                resource: resource.id.clone(),
            })?;

        let reusable = slots.iter().position(|slot| {
            slot.descriptor.compatible_with(descriptor)
                && slot.capacity >= physical_capacity
                && slot.ranges.iter().all(|assigned| !assigned.overlaps(range))
        });
        let slot = if let Some(slot) = reusable {
            slots[slot].ranges.push(range);
            slot
        } else {
            let slot = slots.len();
            slots.push(PlannedSlot {
                descriptor,
                capacity: physical_capacity,
                bytes,
                ranges: vec![range],
            });
            slot
        };
        assignments.push(SlotAssignment {
            resource: resource.id.clone(),
            slot,
            live_range: range,
            capacity: physical_capacity,
            bytes,
        });
    }

    let estimated_peak_bytes = slots.iter().try_fold(0usize, |total, slot| {
        total
            .checked_add(slot.bytes)
            .ok_or_else(|| PlanningError::SizeOverflow {
                resource: assignments[0].resource.clone(),
            })
    })?;
    Ok(StoragePlan {
        report: StorageReport {
            assignments,
            slot_count: slots.len(),
            estimated_peak_bytes,
        },
    })
}
