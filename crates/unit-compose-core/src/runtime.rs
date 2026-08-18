use std::any::{Any, TypeId, type_name};
use std::cell::{Ref, RefCell};
use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use crate::{
    BuildError, BuildOptions, CapacityPolicy, DecodedConfiguration, DenseBinding, DenseGraph,
    DenseUnit, ResourceId, ResourceIndex, ResourceRegistry, ResourceRequirement, RunError, UnitId,
    UnitRegistry, UnitWorkspace,
};

pub(crate) trait RuntimeSlot {
    fn concrete_type(&self) -> TypeId;
    fn concrete_name(&self) -> &'static str;
    fn reset(&mut self);
    fn discard(&mut self);
    fn pending_complete(&self) -> bool;
    fn publish(&mut self);
    fn published(&self) -> Option<&dyn Any>;
    fn pending(&mut self) -> &mut dyn Any;
    fn prepared_capacity(&self) -> usize;
    fn capacity_policy(&self) -> CapacityPolicy;
    fn physical_capacity(&self) -> usize;
}

struct ValueSlot<T: 'static> {
    published: Option<T>,
    pending: Option<T>,
}

impl<T: 'static> RuntimeSlot for ValueSlot<T> {
    fn concrete_type(&self) -> TypeId {
        TypeId::of::<T>()
    }

    fn concrete_name(&self) -> &'static str {
        type_name::<T>()
    }

    fn reset(&mut self) {
        self.published = None;
        self.pending = None;
    }

    fn discard(&mut self) {
        self.pending = None;
    }

    fn pending_complete(&self) -> bool {
        self.pending.is_some()
    }

    fn publish(&mut self) {
        self.published = self.pending.take();
    }

    fn published(&self) -> Option<&dyn Any> {
        self.published.as_ref().map(|value| value as &dyn Any)
    }

    fn pending(&mut self) -> &mut dyn Any {
        &mut self.pending
    }

    fn prepared_capacity(&self) -> usize {
        1
    }

    fn capacity_policy(&self) -> CapacityPolicy {
        CapacityPolicy::RejectOverflow
    }

    fn physical_capacity(&self) -> usize {
        1
    }
}

struct BufferSlot<E: 'static> {
    published: Vec<E>,
    published_valid: bool,
    pending: Vec<E>,
    prepared_capacity: usize,
    policy: CapacityPolicy,
    fixed: bool,
}

impl<E: 'static> RuntimeSlot for BufferSlot<E> {
    fn concrete_type(&self) -> TypeId {
        TypeId::of::<Vec<E>>()
    }

    fn concrete_name(&self) -> &'static str {
        type_name::<Vec<E>>()
    }

    fn reset(&mut self) {
        self.published.clear();
        self.published_valid = false;
        self.pending.clear();
    }

    fn discard(&mut self) {
        self.pending.clear();
    }

    fn pending_complete(&self) -> bool {
        !self.fixed || self.pending.len() == self.prepared_capacity
    }

    fn publish(&mut self) {
        std::mem::swap(&mut self.published, &mut self.pending);
        self.pending.clear();
        self.published_valid = true;
    }

    fn published(&self) -> Option<&dyn Any> {
        self.published_valid.then_some(&self.published as &dyn Any)
    }

    fn pending(&mut self) -> &mut dyn Any {
        &mut self.pending
    }

    fn prepared_capacity(&self) -> usize {
        self.prepared_capacity
    }

    fn capacity_policy(&self) -> CapacityPolicy {
        self.policy
    }

    fn physical_capacity(&self) -> usize {
        self.pending.capacity().max(self.published.capacity())
    }
}

#[derive(Clone, Copy)]
pub(crate) struct RuntimeResourceAdapter {
    allocate: fn(usize, CapacityPolicy) -> Result<Box<dyn RuntimeSlot>, String>,
    identity: &'static str,
}

impl RuntimeResourceAdapter {
    pub(crate) fn fixed_value<T: 'static>() -> Self {
        Self {
            allocate: |capacity, _| {
                if capacity != 1 {
                    return Err(format!(
                        "fixed value {} requires capacity 1, got {capacity}",
                        type_name::<T>()
                    ));
                }
                Ok(Box::new(ValueSlot::<T> {
                    published: None,
                    pending: None,
                }))
            },
            identity: type_name::<ValueSlot<T>>(),
        }
    }

    pub(crate) fn unavailable(identity: &'static str) -> Self {
        Self {
            allocate: |_, _| Err("runtime buffer adapter is not prepared yet".to_owned()),
            identity,
        }
    }

    pub(crate) fn fixed_buffer<E: 'static>() -> Self {
        Self::buffer::<E>(true)
    }

    pub(crate) fn bounded_buffer<E: 'static>() -> Self {
        Self::buffer::<E>(false)
    }

    fn buffer<E: 'static>(fixed: bool) -> Self {
        Self {
            allocate: if fixed {
                allocate_fixed_buffer::<E>
            } else {
                allocate_bounded_buffer::<E>
            },
            identity: type_name::<BufferSlot<E>>(),
        }
    }

    pub(crate) fn allocate(
        self,
        capacity: usize,
        policy: CapacityPolicy,
    ) -> Result<Box<dyn RuntimeSlot>, String> {
        (self.allocate)(capacity, policy)
    }

    pub(crate) const fn identity(self) -> &'static str {
        self.identity
    }
}

fn allocate_fixed_buffer<E: 'static>(
    capacity: usize,
    policy: CapacityPolicy,
) -> Result<Box<dyn RuntimeSlot>, String> {
    Ok(Box::new(BufferSlot::<E> {
        published: Vec::with_capacity(capacity),
        published_valid: false,
        pending: Vec::with_capacity(capacity),
        prepared_capacity: capacity,
        policy,
        fixed: true,
    }))
}

fn allocate_bounded_buffer<E: 'static>(
    capacity: usize,
    policy: CapacityPolicy,
) -> Result<Box<dyn RuntimeSlot>, String> {
    Ok(Box::new(BufferSlot::<E> {
        published: Vec::with_capacity(capacity),
        published_valid: false,
        pending: Vec::with_capacity(capacity),
        prepared_capacity: capacity,
        policy,
        fixed: false,
    }))
}

impl fmt::Debug for RuntimeResourceAdapter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RuntimeResourceAdapter")
            .field("identity", &self.identity)
            .finish()
    }
}

pub(crate) struct RuntimeStore {
    slots: Vec<RefCell<Box<dyn RuntimeSlot>>>,
}

impl RuntimeStore {
    pub(crate) fn new(slots: Vec<Box<dyn RuntimeSlot>>) -> Self {
        Self {
            slots: slots.into_iter().map(RefCell::new).collect(),
        }
    }

    pub(crate) fn reset(&self) {
        for slot in &self.slots {
            slot.borrow_mut().reset();
        }
    }

    fn output_value<T: 'static>(&self, resource: ResourceIndex) -> Result<Ref<'_, T>, RunError> {
        let slot = self.slots[resource.get()].borrow();
        Ref::filter_map(slot, |slot| slot.published()?.downcast_ref::<T>()).map_err(|_| {
            RunError::RuntimeBinding {
                message: format!(
                    "Resource index {} is unpublished or has the wrong type",
                    resource.get()
                ),
            }
        })
    }

    fn output_buffer<E: 'static>(&self, resource: ResourceIndex) -> Result<Ref<'_, [E]>, RunError> {
        let slot = self.slots[resource.get()].borrow();
        Ref::filter_map(slot, |slot| {
            slot.published()?
                .downcast_ref::<Vec<E>>()
                .map(Vec::as_slice)
        })
        .map_err(|_| RunError::RuntimeBinding {
            message: format!(
                "Resource index {} is unpublished or has the wrong buffer type",
                resource.get()
            ),
        })
    }

    fn discard(&self, bindings: &[DenseBinding]) {
        for binding in bindings {
            self.slots[binding.resource.get()].borrow_mut().discard();
        }
    }

    fn validate(&self, bindings: &[DenseBinding]) -> Result<(), RunError> {
        for binding in bindings {
            if !self.slots[binding.resource.get()]
                .borrow()
                .pending_complete()
            {
                return Err(RunError::RuntimeBinding {
                    message: format!("output port {:?} was not initialized", binding.port),
                });
            }
        }
        Ok(())
    }

    fn publish(&self, bindings: &[DenseBinding]) {
        for binding in bindings {
            self.slots[binding.resource.get()].borrow_mut().publish();
        }
    }
}

pub struct RegistrationInvocation<'a> {
    inputs: &'a [DenseBinding],
    outputs: &'a [DenseBinding],
    store: &'a RuntimeStore,
}

impl RegistrationInvocation<'_> {
    pub fn input_value<T: 'static>(&self, port: usize) -> Result<Ref<'_, T>, RunError> {
        let binding = self
            .inputs
            .get(port)
            .ok_or_else(|| RunError::RuntimeBinding {
                message: format!("input port ordinal {port} is out of range"),
            })?;
        if binding.concrete_type.id() != TypeId::of::<T>() {
            return Err(RunError::RuntimeBinding {
                message: format!(
                    "input port {:?} expects {}, adapter requested {}",
                    binding.port,
                    binding.concrete_type.name(),
                    type_name::<T>()
                ),
            });
        }
        let slot = self.store.slots[binding.resource.get()].borrow();
        if slot.concrete_type() != TypeId::of::<T>() {
            return Err(RunError::RuntimeBinding {
                message: format!(
                    "input slot for {:?} contains {}, expected {}",
                    binding.port,
                    slot.concrete_name(),
                    type_name::<T>()
                ),
            });
        }
        Ref::filter_map(slot, |slot| slot.published()?.downcast_ref::<T>()).map_err(|_| {
            RunError::RuntimeBinding {
                message: format!("input port {:?} is unpublished", binding.port),
            }
        })
    }

    pub fn write_value<T: 'static>(&self, port: usize, value: T) -> Result<(), RunError> {
        let binding = self
            .outputs
            .get(port)
            .ok_or_else(|| RunError::RuntimeBinding {
                message: format!("output port ordinal {port} is out of range"),
            })?;
        if binding.concrete_type.id() != TypeId::of::<T>() {
            return Err(RunError::RuntimeBinding {
                message: format!(
                    "output port {:?} expects {}, adapter supplied {}",
                    binding.port,
                    binding.concrete_type.name(),
                    type_name::<T>()
                ),
            });
        }
        let mut slot = self.store.slots[binding.resource.get()].borrow_mut();
        let slot_name = slot.concrete_name();
        let pending =
            slot.pending()
                .downcast_mut::<Option<T>>()
                .ok_or_else(|| RunError::RuntimeBinding {
                    message: format!(
                        "output slot for {:?} contains {slot_name}, expected {}",
                        binding.port,
                        type_name::<T>()
                    ),
                })?;
        *pending = Some(value);
        Ok(())
    }

    pub fn input_buffer<E: 'static>(&self, port: usize) -> Result<Ref<'_, [E]>, RunError> {
        let binding = self
            .inputs
            .get(port)
            .ok_or_else(|| RunError::RuntimeBinding {
                message: format!("input port ordinal {port} is out of range"),
            })?;
        if binding.concrete_type.id() != TypeId::of::<Vec<E>>() {
            return Err(RunError::RuntimeBinding {
                message: format!(
                    "input port {:?} expects {}, adapter requested {}",
                    binding.port,
                    binding.concrete_type.name(),
                    type_name::<Vec<E>>()
                ),
            });
        }
        let slot = self.store.slots[binding.resource.get()].borrow();
        Ref::filter_map(slot, |slot| {
            slot.published()?
                .downcast_ref::<Vec<E>>()
                .map(Vec::as_slice)
        })
        .map_err(|_| RunError::RuntimeBinding {
            message: format!(
                "input port {:?} is unpublished or has the wrong type",
                binding.port
            ),
        })
    }

    pub fn push_buffer<E: 'static>(&self, port: usize, value: E) -> Result<(), RunError> {
        let binding = self
            .outputs
            .get(port)
            .ok_or_else(|| RunError::RuntimeBinding {
                message: format!("output port ordinal {port} is out of range"),
            })?;
        if binding.concrete_type.id() != TypeId::of::<Vec<E>>() {
            return Err(RunError::RuntimeBinding {
                message: format!(
                    "output port {:?} expects {}, adapter supplied {}",
                    binding.port,
                    binding.concrete_type.name(),
                    type_name::<Vec<E>>()
                ),
            });
        }
        let mut slot = self.store.slots[binding.resource.get()].borrow_mut();
        let prepared = slot.prepared_capacity();
        let policy = slot.capacity_policy();
        let pending =
            slot.pending()
                .downcast_mut::<Vec<E>>()
                .ok_or_else(|| RunError::RuntimeBinding {
                    message: format!("output port {:?} is not a typed buffer slot", binding.port),
                })?;
        if pending.len() == prepared && policy == CapacityPolicy::RejectOverflow {
            return Err(RunError::RuntimeOverflow {
                port: binding.port.clone(),
                required: pending.len().saturating_add(1),
                prepared,
                policy,
            });
        }
        pending.push(value);
        Ok(())
    }
}

pub(crate) trait ObjectSafeExecutable {
    fn execute(
        &mut self,
        invocation: &RegistrationInvocation<'_>,
        workspace: UnitWorkspace<'_>,
    ) -> Result<(), RunError>;
}

pub(crate) struct ExecutableAdapter<U, E> {
    unit: U,
    execute: Arc<E>,
}

impl<U, E> ExecutableAdapter<U, E> {
    pub(crate) fn new(unit: U, execute: Arc<E>) -> Self {
        Self { unit, execute }
    }
}

impl<U, E> ObjectSafeExecutable for ExecutableAdapter<U, E>
where
    U: Send,
    E: Fn(&mut U, &RegistrationInvocation<'_>, UnitWorkspace<'_>) -> Result<(), RunError>,
{
    fn execute(
        &mut self,
        invocation: &RegistrationInvocation<'_>,
        workspace: UnitWorkspace<'_>,
    ) -> Result<(), RunError> {
        (self.execute.as_ref())(&mut self.unit, invocation, workspace)
    }
}

pub(crate) struct PreparedExecutable {
    pub(crate) unit: DenseUnit,
    pub(crate) executable: Box<dyn ObjectSafeExecutable>,
    pub(crate) workspace: Vec<u8>,
}

impl PreparedExecutable {
    pub(crate) fn run(&mut self, store: &RuntimeStore) -> Result<(), RunError> {
        store.discard(&self.unit.outputs);
        let invocation = RegistrationInvocation {
            inputs: &self.unit.inputs,
            outputs: &self.unit.outputs,
            store,
        };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.executable.execute(
                &invocation,
                UnitWorkspace {
                    bytes: &mut self.workspace,
                },
            )
        }));
        let result = match result {
            Ok(result) => result,
            Err(_) => {
                store.discard(&self.unit.outputs);
                return Err(RunError::Panic);
            }
        };
        if let Err(error) = result {
            store.discard(&self.unit.outputs);
            return Err(error);
        }
        if let Err(error) = store.validate(&self.unit.outputs) {
            store.discard(&self.unit.outputs);
            return Err(error);
        }
        store.publish(&self.unit.outputs);
        Ok(())
    }
}

pub(crate) struct PreparedRuntime {
    graph: DenseGraph,
    store: RuntimeStore,
    units: Vec<PreparedExecutable>,
}

impl PreparedRuntime {
    pub(crate) fn build(
        graph: DenseGraph,
        mut configurations: BTreeMap<UnitId, DecodedConfiguration>,
        requirements: &BTreeMap<ResourceId, ResourceRequirement>,
        workspace_bytes: &BTreeMap<UnitId, usize>,
        units: &UnitRegistry,
        resources: &ResourceRegistry,
        options: BuildOptions,
    ) -> Result<Self, BuildError> {
        for unit in &graph.units {
            let output_resources = unit
                .outputs
                .iter()
                .map(|binding| binding.resource)
                .collect::<std::collections::BTreeSet<_>>();
            if output_resources.len() != unit.outputs.len()
                || unit
                    .inputs
                    .iter()
                    .any(|input| output_resources.contains(&input.resource))
            {
                return Err(BuildError::RuntimePreparation {
                    message: format!(
                        "Unit {} does not have disjoint live inputs and pending outputs",
                        unit.id.as_str()
                    ),
                });
            }
        }
        let slots = graph
            .resources
            .iter()
            .map(|resource| {
                let descriptor = resources.get(&resource.semantic_type).ok_or_else(|| {
                    BuildError::RuntimePreparation {
                        message: format!("missing descriptor for {}", resource.id.as_str()),
                    }
                })?;
                let requirement = requirements.get(&resource.id).ok_or_else(|| {
                    BuildError::RuntimePreparation {
                        message: format!("missing requirement for {}", resource.id.as_str()),
                    }
                })?;
                descriptor
                    .runtime_adapter()
                    .allocate(requirement.capacity, options.capacity_policy())
                    .map_err(|message| BuildError::RuntimePreparation { message })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let prepared_units = graph
            .units
            .iter()
            .cloned()
            .map(|unit| {
                let configuration = configurations.remove(&unit.id).ok_or_else(|| {
                    BuildError::MissingConfiguration {
                        unit: unit.id.clone(),
                    }
                })?;
                let workspace = workspace_bytes.get(&unit.id).copied().unwrap_or_default();
                units
                    .prepare_executable(&configuration, unit, workspace)
                    .map_err(BuildError::Factory)
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            graph,
            store: RuntimeStore::new(slots),
            units: prepared_units,
        })
    }

    pub(crate) fn run(&mut self) -> Result<(), RunError> {
        self.store.reset();
        for unit in self.graph.execution_order.iter().copied() {
            self.units[unit.get()].run(&self.store)?;
        }
        Ok(())
    }

    pub(crate) fn output_value<T: 'static>(
        &self,
        resource: ResourceIndex,
    ) -> Result<Ref<'_, T>, RunError> {
        self.store.output_value(resource)
    }

    pub(crate) fn output_buffer<E: 'static>(
        &self,
        resource: ResourceIndex,
    ) -> Result<Ref<'_, [E]>, RunError> {
        self.store.output_buffer(resource)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ParsedModule, ParsedUnit, PortDescriptor, SemanticType, UnitDescriptor, UnitRequirements,
        UnitTypeName,
    };

    #[derive(Clone, Copy)]
    struct SourceConfig(u32);
    struct SourceUnit(u32);
    struct MapUnit(u32);
    struct JoinUnit;
    struct FailUnit;

    fn scalar() -> SemanticType {
        SemanticType::new("fixture.Scalar/v1").unwrap()
    }

    fn descriptor(name: &str, inputs: &[&str], outputs: &[&str]) -> UnitDescriptor {
        UnitDescriptor {
            type_name: UnitTypeName::new(name),
            inputs: inputs
                .iter()
                .map(|port| PortDescriptor::of::<u32>(*port, scalar()))
                .collect(),
            outputs: outputs
                .iter()
                .map(|port| PortDescriptor::of::<u32>(*port, scalar()))
                .collect(),
        }
    }

    fn register<U, F, E>(
        registry: &mut UnitRegistry,
        name: &str,
        inputs: &[&str],
        outputs: &[&str],
        factory: F,
        execute: E,
    ) where
        U: Any + Send,
        F: Fn(&SourceConfig) -> Result<U, String> + 'static,
        E: Fn(&mut U, &RegistrationInvocation<'_>, UnitWorkspace<'_>) -> Result<(), RunError>
            + Send
            + Sync
            + 'static,
    {
        let unit_type = UnitTypeName::new(name);
        let output_names = outputs
            .iter()
            .map(|port| ((*port).to_owned(), 1))
            .collect::<BTreeMap<_, _>>();
        registry
            .register::<SourceConfig, SourceConfig, _, _>(
                descriptor(name, inputs, outputs),
                |source, _| Ok(*source),
                move |_, _| {
                    Ok(UnitRequirements {
                        output_capacities: output_names.clone(),
                        workspace_bytes: 0,
                    })
                },
            )
            .unwrap();
        registry
            .register_factory::<SourceConfig, U, _>(&unit_type, factory)
            .unwrap();
        registry
            .register_executor::<U, _>(&unit_type, execute)
            .unwrap();
    }

    fn unit(id: &str, unit_type: &str, inputs: &[(&str, &str)], output: &str) -> ParsedUnit {
        ParsedUnit {
            id: UnitId::new(id),
            unit_type: UnitTypeName::new(unit_type),
            inputs: inputs
                .iter()
                .map(|(port, resource)| ((*port).to_owned(), ResourceId::new(*resource)))
                .collect(),
            outputs: vec![("out".to_owned(), ResourceId::new(output))],
        }
    }

    #[test]
    fn object_safe_fixture_executes_stable_fan_in_and_discards_failure_output() {
        let mut resources = ResourceRegistry::default();
        resources
            .register(crate::ResourceDescriptor::of::<u32>(
                scalar(),
                "fixed scalar",
                "initialized",
            ))
            .unwrap();
        let mut units = UnitRegistry::default();
        register::<SourceUnit, _, _>(
            &mut units,
            "fixture.source/v1",
            &[],
            &["out"],
            |config| Ok(SourceUnit(config.0)),
            |unit, invocation, _| invocation.write_value(0, unit.0),
        );
        register::<MapUnit, _, _>(
            &mut units,
            "fixture.map/v1",
            &["in"],
            &["out"],
            |config| Ok(MapUnit(config.0)),
            |unit, invocation, _| {
                let input = invocation.input_value::<u32>(0)?;
                invocation.write_value(0, *input * unit.0)
            },
        );
        register::<JoinUnit, _, _>(
            &mut units,
            "fixture.join/v1",
            &["left", "right"],
            &["out"],
            |_| Ok(JoinUnit),
            |_, invocation, _| {
                let left = invocation.input_value::<u32>(0)?;
                let right = invocation.input_value::<u32>(1)?;
                invocation.write_value(0, *left + *right)
            },
        );
        register::<FailUnit, _, _>(
            &mut units,
            "fixture.fail/v1",
            &["in"],
            &["out"],
            |_| Ok(FailUnit),
            |_, invocation, _| {
                invocation.write_value(0, 99_u32)?;
                Err(RunError::Unit(crate::UnitFailure::recoverable(
                    "fixture failure",
                )))
            },
        );

        let parsed = ParsedModule {
            schema: "unit-compose/v0alpha1".to_owned(),
            name: "fixture".to_owned(),
            inputs: vec![],
            units: vec![
                unit("right", "fixture.source/v1", &[], "right_value"),
                unit(
                    "join",
                    "fixture.join/v1",
                    &[("left", "mapped"), ("right", "right_value")],
                    "result",
                ),
                unit("left", "fixture.source/v1", &[], "left_value"),
                unit("map", "fixture.map/v1", &[("in", "left_value")], "mapped"),
            ],
            outputs: vec![ResourceId::new("result")],
        };
        let graph = parsed
            .resolve(&units, &resources)
            .unwrap()
            .compile()
            .unwrap();
        let dense = graph.into_dense(17).unwrap();
        let result = dense
            .output_handle::<u32>(&ResourceId::new("result"))
            .unwrap();
        let configs = BTreeMap::from([
            ("left", ("fixture.source/v1", 3)),
            ("right", ("fixture.source/v1", 5)),
            ("map", ("fixture.map/v1", 7)),
            ("join", ("fixture.join/v1", 0)),
        ])
        .into_iter()
        .map(|(id, (kind, value))| {
            let config = units
                .decode(&UnitTypeName::new(kind), &SourceConfig(value), "$.config")
                .unwrap();
            (UnitId::new(id), config)
        })
        .collect();
        let requirements = dense
            .resources
            .iter()
            .map(|resource| (resource.id.clone(), ResourceRequirement { capacity: 1 }))
            .collect();
        let mut runtime = PreparedRuntime::build(
            dense,
            configs,
            &requirements,
            &BTreeMap::new(),
            &units,
            &resources,
            BuildOptions::development(),
        )
        .unwrap();
        runtime.run().unwrap();
        assert_eq!(*runtime.output_value::<u32>(result.resource()).unwrap(), 26);

        let failing = ParsedModule {
            schema: "unit-compose/v0alpha1".to_owned(),
            name: "failing-fixture".to_owned(),
            inputs: vec![],
            units: vec![
                unit("right", "fixture.source/v1", &[], "right_value"),
                unit(
                    "join",
                    "fixture.join/v1",
                    &[("left", "mapped"), ("right", "right_value")],
                    "result",
                ),
                unit("left", "fixture.source/v1", &[], "left_value"),
                unit("map", "fixture.map/v1", &[("in", "left_value")], "mapped"),
                unit(
                    "fail",
                    "fixture.fail/v1",
                    &[("in", "result")],
                    "failed_output",
                ),
            ],
            outputs: vec![ResourceId::new("failed_output")],
        };
        let failing_graph = failing
            .resolve(&units, &resources)
            .unwrap()
            .compile()
            .unwrap();
        let failing_dense = failing_graph.into_dense(18).unwrap();
        let failed_output = failing_dense
            .output_handle::<u32>(&ResourceId::new("failed_output"))
            .unwrap();
        let failing_configs = BTreeMap::from([
            ("left", ("fixture.source/v1", 3)),
            ("right", ("fixture.source/v1", 5)),
            ("map", ("fixture.map/v1", 7)),
            ("join", ("fixture.join/v1", 0)),
            ("fail", ("fixture.fail/v1", 0)),
        ])
        .into_iter()
        .map(|(id, (kind, value))| {
            let config = units
                .decode(&UnitTypeName::new(kind), &SourceConfig(value), "$.config")
                .unwrap();
            (UnitId::new(id), config)
        })
        .collect();
        let failing_requirements = failing_dense
            .resources
            .iter()
            .map(|resource| (resource.id.clone(), ResourceRequirement { capacity: 1 }))
            .collect();
        let mut failing_runtime = PreparedRuntime::build(
            failing_dense,
            failing_configs,
            &failing_requirements,
            &BTreeMap::new(),
            &units,
            &resources,
            BuildOptions::development(),
        )
        .unwrap();
        assert!(matches!(
            failing_runtime.run(),
            Err(RunError::Unit(crate::UnitFailure { .. }))
        ));
        assert!(
            failing_runtime
                .output_value::<u32>(failed_output.resource())
                .is_err()
        );
    }

    #[test]
    fn bounded_runtime_slot_publishes_resets_and_rejects_strict_overflow_without_growth() {
        #[derive(Clone, Copy)]
        struct BufferConfig {
            writes: usize,
            capacity: usize,
        }
        struct BufferSource {
            writes: usize,
        }

        let buffer_type = SemanticType::new("fixture.Buffer/v1").unwrap();
        let mut resources = ResourceRegistry::default();
        resources
            .register(crate::ResourceDescriptor::bounded_buffer::<Vec<u32>, u32>(
                buffer_type.clone(),
                "bounded u32 fixture",
                "bounded",
            ))
            .unwrap();
        let mut units = UnitRegistry::default();
        let unit_type = UnitTypeName::new("fixture.buffer_source/v1");
        units
            .register::<BufferConfig, BufferConfig, _, _>(
                UnitDescriptor {
                    type_name: unit_type.clone(),
                    inputs: vec![],
                    outputs: vec![PortDescriptor::of::<Vec<u32>>("out", buffer_type)],
                },
                |source, _| Ok(*source),
                |config, _| {
                    Ok(UnitRequirements {
                        output_capacities: BTreeMap::from([("out".to_owned(), config.capacity)]),
                        workspace_bytes: 0,
                    })
                },
            )
            .unwrap();
        units
            .register_factory::<BufferConfig, BufferSource, _>(&unit_type, |config| {
                Ok(BufferSource {
                    writes: config.writes,
                })
            })
            .unwrap();
        units
            .register_executor::<BufferSource, _>(&unit_type, |unit, invocation, _| {
                for value in 0..unit.writes {
                    invocation.push_buffer(0, value as u32)?;
                }
                Ok(())
            })
            .unwrap();

        let definition = ParsedModule {
            schema: "unit-compose/v0alpha1".to_owned(),
            name: "buffer".to_owned(),
            inputs: vec![],
            units: vec![unit("source", "fixture.buffer_source/v1", &[], "result")],
            outputs: vec![ResourceId::new("result")],
        };
        let graph = definition
            .resolve(&units, &resources)
            .unwrap()
            .compile()
            .unwrap();
        let dense = graph.into_dense(20).unwrap();
        let output = dense
            .output_handle::<Vec<u32>>(&ResourceId::new("result"))
            .unwrap();
        let config = BufferConfig {
            writes: 2,
            capacity: 2,
        };
        let decoded = units.decode(&unit_type, &config, "$.config").unwrap();
        let requirements = BTreeMap::from([(
            ResourceId::new("result"),
            ResourceRequirement { capacity: 2 },
        )]);
        let mut runtime = PreparedRuntime::build(
            dense,
            BTreeMap::from([(UnitId::new("source"), decoded)]),
            &requirements,
            &BTreeMap::new(),
            &units,
            &resources,
            BuildOptions::strict(),
        )
        .unwrap();
        runtime.run().unwrap();
        assert_eq!(
            &*runtime.output_buffer::<u32>(output.resource()).unwrap(),
            &[0, 1]
        );
        runtime.run().unwrap();
        assert_eq!(
            &*runtime.output_buffer::<u32>(output.resource()).unwrap(),
            &[0, 1]
        );

        let overflow_config = BufferConfig {
            writes: 3,
            capacity: 2,
        };
        let overflow_graph = ParsedModule {
            schema: "unit-compose/v0alpha1".to_owned(),
            name: "overflow".to_owned(),
            inputs: vec![],
            units: vec![unit("source", "fixture.buffer_source/v1", &[], "result")],
            outputs: vec![ResourceId::new("result")],
        }
        .resolve(&units, &resources)
        .unwrap()
        .compile()
        .unwrap()
        .into_dense(21)
        .unwrap();
        let overflow_output = overflow_graph
            .output_handle::<Vec<u32>>(&ResourceId::new("result"))
            .unwrap();
        let overflow_decoded = units
            .decode(&unit_type, &overflow_config, "$.config")
            .unwrap();
        let mut overflow = PreparedRuntime::build(
            overflow_graph,
            BTreeMap::from([(UnitId::new("source"), overflow_decoded)]),
            &requirements,
            &BTreeMap::new(),
            &units,
            &resources,
            BuildOptions::strict(),
        )
        .unwrap();
        assert!(matches!(
            overflow.run(),
            Err(RunError::RuntimeOverflow {
                required: 3,
                prepared: 2,
                ..
            })
        ));
        assert!(
            overflow
                .output_buffer::<u32>(overflow_output.resource())
                .is_err()
        );
        let slot = overflow.store.slots[overflow_output.resource().get()].borrow();
        assert!(slot.pending_complete());
        assert_eq!(slot.prepared_capacity(), 2);
        assert_eq!(slot.physical_capacity(), 2);

        let growth_config = BufferConfig {
            writes: 3,
            capacity: 2,
        };
        let growth_graph = ParsedModule {
            schema: "unit-compose/v0alpha1".to_owned(),
            name: "growth".to_owned(),
            inputs: vec![],
            units: vec![unit("source", "fixture.buffer_source/v1", &[], "result")],
            outputs: vec![ResourceId::new("result")],
        }
        .resolve(&units, &resources)
        .unwrap()
        .compile()
        .unwrap()
        .into_dense(22)
        .unwrap();
        let growth_output = growth_graph
            .output_handle::<Vec<u32>>(&ResourceId::new("result"))
            .unwrap();
        let growth_decoded = units
            .decode(&unit_type, &growth_config, "$.config")
            .unwrap();
        let mut growth = PreparedRuntime::build(
            growth_graph,
            BTreeMap::from([(UnitId::new("source"), growth_decoded)]),
            &requirements,
            &BTreeMap::new(),
            &units,
            &resources,
            BuildOptions::development(),
        )
        .unwrap();
        growth.run().unwrap();
        assert_eq!(
            &*growth
                .output_buffer::<u32>(growth_output.resource())
                .unwrap(),
            &[0, 1, 2]
        );
        let slot = growth.store.slots[growth_output.resource().get()].borrow();
        assert!(slot.physical_capacity() >= 3);
    }

    #[test]
    fn fixed_buffer_group_validation_and_unwind_drop_all_pending_values() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };

        struct DropProbe(Arc<AtomicUsize>);
        impl Drop for DropProbe {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        #[derive(Clone)]
        struct ProbeConfig {
            drops: Arc<AtomicUsize>,
            panic: bool,
            fail: bool,
            complete: bool,
        }
        struct ProbeUnit(ProbeConfig);

        let probe_type = SemanticType::new("fixture.DropProbeBuffer/v1").unwrap();
        let mut resources = ResourceRegistry::default();
        resources
            .register(crate::ResourceDescriptor::fixed_buffer::<
                Vec<DropProbe>,
                DropProbe,
            >(
                probe_type.clone(),
                "fixed drop probes",
                "exactly two probes",
            ))
            .unwrap();
        let mut units = UnitRegistry::default();
        for name in [
            "fixture.success/v1",
            "fixture.partial/v1",
            "fixture.error/v1",
            "fixture.panic/v1",
        ] {
            let unit_type = UnitTypeName::new(name);
            units
                .register::<ProbeConfig, ProbeConfig, _, _>(
                    UnitDescriptor {
                        type_name: unit_type.clone(),
                        inputs: vec![],
                        outputs: vec![
                            PortDescriptor::of::<Vec<DropProbe>>("complete", probe_type.clone()),
                            PortDescriptor::of::<Vec<DropProbe>>("partial", probe_type.clone()),
                        ],
                    },
                    |source, _| Ok(source.clone()),
                    |_, _| {
                        Ok(UnitRequirements {
                            output_capacities: BTreeMap::from([
                                ("complete".to_owned(), 2),
                                ("partial".to_owned(), 2),
                            ]),
                            workspace_bytes: 0,
                        })
                    },
                )
                .unwrap();
            units
                .register_factory::<ProbeConfig, ProbeUnit, _>(&unit_type, |config| {
                    Ok(ProbeUnit(config.clone()))
                })
                .unwrap();
            units
                .register_executor::<ProbeUnit, _>(&unit_type, |unit, invocation, _| {
                    invocation.push_buffer(0, DropProbe(Arc::clone(&unit.0.drops)))?;
                    invocation.push_buffer(0, DropProbe(Arc::clone(&unit.0.drops)))?;
                    invocation.push_buffer(1, DropProbe(Arc::clone(&unit.0.drops)))?;
                    if unit.0.complete {
                        invocation.push_buffer(1, DropProbe(Arc::clone(&unit.0.drops)))?;
                    }
                    if unit.0.panic {
                        panic!("fixture panic after pending writes");
                    }
                    if unit.0.fail {
                        return Err(RunError::Unit(crate::UnitFailure::recoverable(
                            "fixture error after pending writes",
                        )));
                    }
                    Ok(())
                })
                .unwrap();
        }

        let build = |name: &str,
                     plan_token: u64,
                     drops: Arc<AtomicUsize>,
                     panic: bool,
                     fail: bool,
                     complete: bool|
         -> (PreparedRuntime, ResourceIndex, ResourceIndex) {
            let definition = ParsedModule {
                schema: "unit-compose/v0alpha1".to_owned(),
                name: name.to_owned(),
                inputs: vec![],
                units: vec![ParsedUnit {
                    id: UnitId::new("probe"),
                    unit_type: UnitTypeName::new(name),
                    inputs: vec![],
                    outputs: vec![
                        ("complete".to_owned(), ResourceId::new("complete")),
                        ("partial".to_owned(), ResourceId::new("partial")),
                    ],
                }],
                outputs: vec![ResourceId::new("complete"), ResourceId::new("partial")],
            };
            let graph = definition
                .resolve(&units, &resources)
                .unwrap()
                .compile()
                .unwrap();
            let dense = graph.into_dense(plan_token).unwrap();
            let complete_output = dense
                .output_handle::<Vec<DropProbe>>(&ResourceId::new("complete"))
                .unwrap()
                .resource();
            let partial = dense
                .output_handle::<Vec<DropProbe>>(&ResourceId::new("partial"))
                .unwrap()
                .resource();
            let source = ProbeConfig {
                drops,
                panic,
                fail,
                complete,
            };
            let config = units
                .decode(&UnitTypeName::new(name), &source, "$.config")
                .unwrap();
            let requirements = BTreeMap::from([
                (
                    ResourceId::new("complete"),
                    ResourceRequirement { capacity: 2 },
                ),
                (
                    ResourceId::new("partial"),
                    ResourceRequirement { capacity: 2 },
                ),
            ]);
            let runtime = PreparedRuntime::build(
                dense,
                BTreeMap::from([(UnitId::new("probe"), config)]),
                &requirements,
                &BTreeMap::new(),
                &units,
                &resources,
                BuildOptions::strict(),
            )
            .unwrap();
            (runtime, complete_output, partial)
        };

        let validation_drops = Arc::new(AtomicUsize::new(0));
        let (mut partial, complete_output, partial_output) = build(
            "fixture.partial/v1",
            30,
            Arc::clone(&validation_drops),
            false,
            false,
            false,
        );
        assert!(matches!(
            partial.run(),
            Err(RunError::RuntimeBinding { .. })
        ));
        assert_eq!(validation_drops.load(Ordering::SeqCst), 3);
        assert!(partial.output_buffer::<DropProbe>(complete_output).is_err());
        assert!(partial.output_buffer::<DropProbe>(partial_output).is_err());

        let success_drops = Arc::new(AtomicUsize::new(0));
        let (mut success, complete_output, partial_output) = build(
            "fixture.success/v1",
            31,
            Arc::clone(&success_drops),
            false,
            false,
            true,
        );
        success.run().unwrap();
        assert_eq!(
            success
                .output_buffer::<DropProbe>(complete_output)
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            success
                .output_buffer::<DropProbe>(partial_output)
                .unwrap()
                .len(),
            2
        );
        drop(success);
        assert_eq!(success_drops.load(Ordering::SeqCst), 4);

        let error_drops = Arc::new(AtomicUsize::new(0));
        let (mut errors, complete_output, partial_output) = build(
            "fixture.error/v1",
            32,
            Arc::clone(&error_drops),
            false,
            true,
            false,
        );
        assert!(matches!(
            errors.run(),
            Err(RunError::Unit(crate::UnitFailure { .. }))
        ));
        assert_eq!(error_drops.load(Ordering::SeqCst), 3);
        assert!(errors.output_buffer::<DropProbe>(complete_output).is_err());
        assert!(errors.output_buffer::<DropProbe>(partial_output).is_err());

        let panic_drops = Arc::new(AtomicUsize::new(0));
        let (mut panics, complete_output, partial_output) = build(
            "fixture.panic/v1",
            33,
            Arc::clone(&panic_drops),
            true,
            false,
            false,
        );
        assert_eq!(panics.run(), Err(RunError::Panic));
        assert_eq!(panic_drops.load(Ordering::SeqCst), 3);
        assert!(panics.output_buffer::<DropProbe>(complete_output).is_err());
        assert!(panics.output_buffer::<DropProbe>(partial_output).is_err());
    }
}
