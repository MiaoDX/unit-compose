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
        let result = self.executable.execute(
            &invocation,
            UnitWorkspace {
                bytes: &mut self.workspace,
            },
        );
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
}
