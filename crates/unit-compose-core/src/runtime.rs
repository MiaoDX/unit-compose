use std::any::{Any, TypeId, type_name};
use std::cell::{Ref, RefCell};
use std::collections::BTreeMap;
use std::fmt;
use std::ops::Deref;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::{
    BuildError, BuildOptions, CapacityPolicy, DecodedConfiguration, DenseBinding, DenseGraph,
    DenseResource, DenseUnit, ExecutableDefinition, FailureDisposition, ResourceId, ResourceIndex,
    ResourceRegistry, ResourceRequirement, RunError, RunErrorContext, StoragePlan, UnitId,
    UnitIndex, UnitRegistry, UnitWorkspace, plan_storage,
};

static NEXT_PLAN_TOKEN: AtomicU64 = AtomicU64::new(1);

struct BoundInput<'a> {
    resource: ResourceIndex,
    plan_token: u64,
    concrete_type: TypeId,
    concrete_name: &'static str,
    value: &'a dyn Any,
}

#[derive(Default)]
pub struct ModuleInputs<'a> {
    bindings: Vec<BoundInput<'a>>,
}

/// Prepared dynamic Module built from a compiled frontend-neutral definition.
pub struct Module {
    runtime: PreparedRuntime,
    options: BuildOptions,
    allocation_capability: crate::AllocationCapability,
    report: crate::RunReport,
    reporting_enabled: bool,
}

/// Host-owned output storage whose contents are valid only after successful publication and copy.
pub struct HostOutput<T> {
    value: T,
    valid: bool,
}

impl<T> HostOutput<T> {
    #[must_use]
    pub const fn new(value: T) -> Self {
        Self {
            value,
            valid: false,
        }
    }

    #[must_use]
    pub const fn is_valid(&self) -> bool {
        self.valid
    }

    #[must_use]
    pub fn get(&self) -> Option<&T> {
        self.valid.then_some(&self.value)
    }

    /// Returns storage regardless of validity. After failure it may be partially mutated.
    #[must_use]
    pub const fn raw(&self) -> &T {
        &self.value
    }
}

impl Module {
    pub fn build(
        definition: ExecutableDefinition,
        units: &UnitRegistry,
        resources: &ResourceRegistry,
        options: BuildOptions,
    ) -> Result<Self, BuildError> {
        PreparedRuntime::build_definition(definition, units, resources, options).map(|runtime| {
            let allocation_capability = runtime.allocation_capability.clone();
            Self {
                runtime,
                options,
                allocation_capability,
                report: crate::RunReport::default(),
                reporting_enabled: true,
            }
        })
    }

    pub fn input_handle<T: 'static>(
        &self,
        resource: &ResourceId,
    ) -> Result<crate::InputHandle<T>, crate::HandleError> {
        self.runtime.input_handle(resource)
    }

    pub fn output_handle<T: 'static>(
        &self,
        resource: &ResourceId,
    ) -> Result<crate::OutputHandle<T>, crate::HandleError> {
        self.runtime.output_handle(resource)
    }

    pub fn run(&mut self, inputs: &ModuleInputs<'_>) -> Result<(), RunError> {
        self.execute(inputs, &mut [], None, false)
    }

    pub fn warm_up(&mut self, inputs: &ModuleInputs<'_>) -> Result<(), RunError> {
        self.execute(inputs, &mut [], None, false)
    }

    pub fn run_profiled(
        &mut self,
        inputs: &ModuleInputs<'_>,
        probes: &mut [&mut dyn crate::AllocationDomainProbe],
        sink: Option<&mut dyn crate::DiagnosticSink>,
    ) -> Result<(), RunError> {
        self.execute(inputs, probes, sink, true)
    }

    fn execute(
        &mut self,
        inputs: &ModuleInputs<'_>,
        probes: &mut [&mut dyn crate::AllocationDomainProbe],
        mut sink: Option<&mut dyn crate::DiagnosticSink>,
        validate_probes: bool,
    ) -> Result<(), RunError> {
        self.report.reset();
        self.runtime.store.invalidate_publications();
        inputs
            .validate(&self.runtime.graph)
            .map_err(|error| module_error(&self.runtime.graph.module, error))?;
        if validate_probes {
            for domain in self
                .allocation_capability
                .domains()
                .iter()
                .filter(|domain| matches!(domain.evidence, crate::AllocationEvidence::Instrumented))
            {
                if !probes.iter().any(|probe| probe.domain() == domain.name) {
                    return Err(RunError::AllocationProfileViolation {
                        domain: domain.name.clone(),
                        operations: crate::AllocationOperations::default(),
                    });
                }
            }
            for probe in probes.iter() {
                if !self
                    .allocation_capability
                    .domains()
                    .iter()
                    .any(|domain| domain.name == probe.domain())
                {
                    return Err(RunError::AllocationProfileViolation {
                        domain: probe.domain().to_owned(),
                        operations: crate::AllocationOperations::default(),
                    });
                }
            }
            for (index, probe) in probes.iter().enumerate() {
                if probes[..index]
                    .iter()
                    .any(|previous| previous.domain() == probe.domain())
                {
                    return Err(RunError::AllocationProfileViolation {
                        domain: probe.domain().to_owned(),
                        operations: crate::AllocationOperations::default(),
                    });
                }
            }
        }
        for probe in probes.iter_mut() {
            probe.begin();
        }
        let started = std::time::Instant::now();
        let mut timings = crate::UnitExecutionRecorder::new(started, self.reporting_enabled);
        let result = self.runtime.run_with_inputs_timed(inputs, &mut timings);
        let elapsed = started.elapsed();
        if self.reporting_enabled {
            for (target, event) in self
                .report
                .unit_timings
                .iter_mut()
                .zip(timings.events().copied())
            {
                *target = Some(event);
                self.report.unit_timing_len += 1;
            }
            self.report.dropped_unit_timings = timings.dropped_events;
        }
        let observed_capacity = match &result {
            Err(failure) => match failure.cause.root_cause() {
                RunError::Capacity(error) => error.prepared,
                RunError::RuntimeOverflow { prepared, .. } => *prepared,
                _ => self.runtime.store.observed_capacity(),
            },
            Ok(()) => self.runtime.store.observed_capacity(),
        };
        let event = crate::RunEvent {
            kind: match &result {
                Ok(()) => crate::RunEventKind::Success,
                Err(failure) => crate::error_event_kind(&failure.cause),
            },
            observed_capacity,
            elapsed,
            timing_scope: crate::TimingScope::ModuleExecution,
            timing_overhead: crate::TimingOverhead {
                clock_reads: timings.module_clock_reads(),
                bounded_report_write_in_elapsed: timings.len != 0,
            },
        };
        if self.reporting_enabled {
            self.report.push(event);
        }
        if let Some(sink) = sink.as_mut() {
            sink.record(event);
        }
        let mut violation = None;
        for (index, probe) in probes.iter_mut().enumerate() {
            let operations = probe.finish();
            if self.reporting_enabled {
                self.report.allocation_operations.allocations += operations.allocations;
                self.report.allocation_operations.reallocations += operations.reallocations;
                self.report.allocation_operations.deallocations += operations.deallocations;
            }
            if violation.is_none() && !operations.is_zero() {
                violation = Some((index, operations));
            }
        }
        if result.is_ok()
            && let Some((index, operations)) = violation
        {
            let violation_event = crate::RunEvent {
                kind: crate::RunEventKind::AllocationProfileViolation,
                ..event
            };
            if self.reporting_enabled {
                self.report.replace_last(violation_event);
            }
            if let Some(sink) = sink.as_mut() {
                sink.correct_last(violation_event);
            }
            let domain = probes[index].domain().to_owned();
            self.runtime.store.invalidate_publications();
            return Err(RunError::AllocationProfileViolation { domain, operations });
        }
        result.map_err(|failure| failure.contextualize(&self.runtime.graph))
    }

    #[must_use]
    pub const fn report(&self) -> &crate::RunReport {
        &self.report
    }

    #[must_use]
    pub const fn options(&self) -> BuildOptions {
        self.options
    }

    pub fn set_reporting_enabled(&mut self, enabled: bool) {
        self.reporting_enabled = enabled;
    }

    /// Borrows a published output from this Module.
    ///
    /// The borrow prevents another mutable run until the view is dropped:
    ///
    /// ```compile_fail
    /// use unit_compose_core::{Module, ModuleInputs, OutputHandle};
    ///
    /// fn rerun_while_borrowed(
    ///     module: &mut Module,
    ///     inputs: &ModuleInputs<'_>,
    ///     handle: &OutputHandle<u32>,
    /// ) {
    ///     let output = module.output(handle).unwrap();
    ///     module.run(inputs).unwrap();
    ///     drop(output);
    /// }
    /// ```
    pub fn output<T: 'static>(
        &self,
        handle: &crate::OutputHandle<T>,
    ) -> Result<Ref<'_, T>, RunError> {
        self.runtime.output(handle)
    }

    /// Runs and copies one completely published Resource into caller-owned storage.
    /// The target is invalid from entry until both execution and `copy` succeed.
    pub fn run_into<T: 'static, O>(
        &mut self,
        inputs: &ModuleInputs<'_>,
        handle: &crate::OutputHandle<T>,
        target: &mut HostOutput<O>,
        copy: impl FnOnce(&T, &mut O) -> Result<(), RunError>,
    ) -> Result<(), RunError> {
        target.valid = false;
        self.run(inputs)?;
        let output = self.output(handle)?;
        copy(&output, &mut target.value)?;
        target.valid = true;
        Ok(())
    }
}

impl<'a> ModuleInputs<'a> {
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            bindings: Vec::with_capacity(capacity),
        }
    }

    pub fn clear(&mut self) {
        self.bindings.clear();
    }

    pub fn bind<T: 'static>(
        &mut self,
        handle: &crate::InputHandle<T>,
        value: &'a T,
    ) -> Result<(), InputBindingError> {
        if self
            .bindings
            .iter()
            .any(|binding| binding.resource == handle.resource())
        {
            return Err(InputBindingError::Duplicate {
                resource: handle.resource(),
            });
        }
        self.bindings.push(BoundInput {
            resource: handle.resource(),
            plan_token: handle.plan_token(),
            concrete_type: TypeId::of::<T>(),
            concrete_name: type_name::<T>(),
            value,
        });
        Ok(())
    }

    fn validate(&self, graph: &DenseGraph) -> Result<(), RunError> {
        if self
            .bindings
            .iter()
            .any(|binding| binding.plan_token != graph.plan_token())
        {
            return Err(RunError::RuntimeBinding {
                message: "input handle belongs to a different graph plan".into(),
            });
        }
        let required = graph.module_inputs();
        if self.bindings.len() != required.len()
            || self
                .bindings
                .iter()
                .any(|binding| !required.contains(&binding.resource))
        {
            return Err(RunError::RuntimeBinding {
                message: "module inputs do not exactly match required inputs".into(),
            });
        }
        for binding in &self.bindings {
            let expected = graph.resources[binding.resource.get()].concrete_type;
            if expected.id() != binding.concrete_type {
                return Err(RunError::RuntimeBinding {
                    message: format!(
                        "input resource {} expects {}, supplied {}",
                        binding.resource.get(),
                        expected.name(),
                        binding.concrete_name
                    ),
                });
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InputBindingError {
    Duplicate { resource: ResourceIndex },
}

pub enum InputValue<'a, T: 'static> {
    Borrowed(&'a T),
    Stored(Ref<'a, T>),
}

impl<T: 'static> Deref for InputValue<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        match self {
            Self::Borrowed(value) => value,
            Self::Stored(value) => value,
        }
    }
}

pub enum InputBuffer<'a, E: 'static> {
    Borrowed(&'a [E]),
    Stored(Ref<'a, [E]>),
}

impl<E: 'static> Deref for InputBuffer<'_, E> {
    type Target = [E];

    fn deref(&self) -> &Self::Target {
        match self {
            Self::Borrowed(value) => value,
            Self::Stored(value) => value,
        }
    }
}

pub(crate) trait RuntimeSlot {
    fn concrete_type(&self) -> TypeId;
    fn concrete_name(&self) -> &'static str;
    fn reset(&mut self);
    fn discard(&mut self);
    fn pending_complete(&self, prepared_capacity: usize) -> bool;
    fn publish(&mut self);
    fn published(&self) -> Option<&dyn Any>;
    fn pending(&mut self) -> &mut dyn Any;
    fn published_capacity(&self) -> usize;
    #[cfg(test)]
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

    fn pending_complete(&self, _prepared_capacity: usize) -> bool {
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

    fn published_capacity(&self) -> usize {
        usize::from(self.published.is_some())
    }

    #[cfg(test)]
    fn physical_capacity(&self) -> usize {
        1
    }
}

struct BufferSlot<E: 'static> {
    published: Vec<E>,
    published_valid: bool,
    pending: Vec<E>,
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

    fn pending_complete(&self, prepared_capacity: usize) -> bool {
        !self.fixed || self.pending.len() == prepared_capacity
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

    fn published_capacity(&self) -> usize {
        if self.published_valid {
            self.published.len()
        } else {
            0
        }
    }

    #[cfg(test)]
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
    _policy: CapacityPolicy,
) -> Result<Box<dyn RuntimeSlot>, String> {
    let (published, pending) = allocate_buffer_backings::<E>(capacity)?;
    Ok(Box::new(BufferSlot::<E> {
        published,
        published_valid: false,
        pending,
        fixed: true,
    }))
}

fn allocate_bounded_buffer<E: 'static>(
    capacity: usize,
    _policy: CapacityPolicy,
) -> Result<Box<dyn RuntimeSlot>, String> {
    let (published, pending) = allocate_buffer_backings::<E>(capacity)?;
    Ok(Box::new(BufferSlot::<E> {
        published,
        published_valid: false,
        pending,
        fixed: false,
    }))
}

fn allocate_buffer_backings<E>(capacity: usize) -> Result<(Vec<E>, Vec<E>), String> {
    let reserve = |label| -> Result<Vec<E>, String> {
        let mut buffer = Vec::new();
        buffer.try_reserve_exact(capacity).map_err(|_| {
            format!(
                "failed to reserve {label} buffer for {} elements of {}",
                capacity,
                type_name::<E>()
            )
        })?;
        Ok(buffer)
    };
    Ok((reserve("published")?, reserve("pending")?))
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
    resource_slots: Vec<usize>,
    resource_capacities: Vec<usize>,
    resource_policies: Vec<CapacityPolicy>,
    published: RefCell<Vec<bool>>,
    owners: RefCell<Vec<Option<ResourceIndex>>>,
}

const NO_RUNTIME_SLOT: usize = usize::MAX;

impl RuntimeStore {
    pub(crate) fn new(
        slots: Vec<Box<dyn RuntimeSlot>>,
        resource_slots: Vec<usize>,
        resource_capacities: Vec<usize>,
        resource_policies: Vec<CapacityPolicy>,
    ) -> Self {
        let resource_count = resource_slots.len();
        let slot_count = slots.len();
        Self {
            slots: slots.into_iter().map(RefCell::new).collect(),
            resource_slots,
            resource_capacities,
            resource_policies,
            published: RefCell::new(vec![false; resource_count]),
            owners: RefCell::new(vec![None; slot_count]),
        }
    }

    pub(crate) fn reset(&self) {
        for slot in &self.slots {
            slot.borrow_mut().reset();
        }
        self.published.borrow_mut().fill(false);
        self.owners.borrow_mut().fill(None);
    }

    fn invalidate_publications(&self) {
        self.published.borrow_mut().fill(false);
        self.owners.borrow_mut().fill(None);
    }

    fn output_value<T: 'static>(&self, resource: ResourceIndex) -> Result<Ref<'_, T>, RunError> {
        self.require_published(resource)?;
        let slot = self.slots[self.slot(resource)].borrow();
        Ref::filter_map(slot, |slot| slot.published()?.downcast_ref::<T>()).map_err(|_| {
            RunError::RuntimeBinding {
                message: format!(
                    "Resource index {} is unpublished or has the wrong type",
                    resource.get()
                ),
            }
        })
    }

    #[cfg(test)]
    fn output_buffer<E: 'static>(&self, resource: ResourceIndex) -> Result<Ref<'_, [E]>, RunError> {
        self.require_published(resource)?;
        let slot = self.slots[self.slot(resource)].borrow();
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
            self.slots[self.slot(binding.resource)]
                .borrow_mut()
                .discard();
        }
    }

    fn validate(&self, bindings: &[DenseBinding]) -> Result<(), RunError> {
        for binding in bindings {
            if !self.slots[self.slot(binding.resource)]
                .borrow()
                .pending_complete(self.prepared_capacity(binding.resource))
            {
                return Err(RunError::RuntimeBinding {
                    message: format!("output port {:?} was not initialized", binding.port),
                });
            }
        }
        Ok(())
    }

    fn publish(&self, bindings: &[DenseBinding]) {
        let mut published = self.published.borrow_mut();
        let mut owners = self.owners.borrow_mut();
        for binding in bindings {
            let slot_index = self.slot(binding.resource);
            self.slots[slot_index].borrow_mut().publish();
            if let Some(previous) = owners[slot_index].replace(binding.resource) {
                published[previous.get()] = false;
            }
            published[binding.resource.get()] = true;
        }
    }

    fn slot(&self, resource: ResourceIndex) -> usize {
        self.resource_slots[resource.get()]
    }

    fn prepared_capacity(&self, resource: ResourceIndex) -> usize {
        self.resource_capacities[resource.get()]
    }

    fn capacity_policy(&self, resource: ResourceIndex) -> CapacityPolicy {
        self.resource_policies[resource.get()]
    }

    fn observed_capacity(&self) -> usize {
        let published = self.published.borrow();
        let owners = self.owners.borrow();
        self.resource_slots
            .iter()
            .enumerate()
            .filter(|(resource, slot)| {
                **slot != NO_RUNTIME_SLOT
                    && published[*resource]
                    && owners[**slot].is_some_and(|owner| owner.get() == *resource)
            })
            .map(|(_, slot)| self.slots[*slot].borrow().published_capacity())
            .sum()
    }

    fn require_published(&self, resource: ResourceIndex) -> Result<(), RunError> {
        let slot = self.slot(resource);
        if slot == NO_RUNTIME_SLOT {
            return Err(RunError::RuntimeBinding {
                message: format!("Resource index {} has no runtime slot", resource.get()),
            });
        }
        let published = self.published.borrow();
        let owners = self.owners.borrow();
        if published[resource.get()] && owners[slot] == Some(resource) {
            Ok(())
        } else {
            Err(RunError::RuntimeBinding {
                message: format!("Resource index {} is unpublished", resource.get()),
            })
        }
    }
}

pub struct RegistrationInvocation<'a> {
    inputs: &'a [DenseBinding],
    outputs: &'a [DenseBinding],
    store: &'a RuntimeStore,
    module_inputs: &'a ModuleInputs<'a>,
}

impl RegistrationInvocation<'_> {
    pub fn input_value<T: 'static>(&self, port: usize) -> Result<InputValue<'_, T>, RunError> {
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
        if let Some(bound) = self
            .module_inputs
            .bindings
            .iter()
            .find(|bound| bound.resource == binding.resource)
        {
            return bound
                .value
                .downcast_ref::<T>()
                .map(InputValue::Borrowed)
                .ok_or_else(|| RunError::RuntimeBinding {
                    message: format!(
                        "input port {:?} is bound as {}, expected {}",
                        binding.port,
                        bound.concrete_name,
                        type_name::<T>()
                    ),
                });
        }
        self.store.require_published(binding.resource)?;
        let slot = self.store.slots[self.store.slot(binding.resource)].borrow();
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
        Ref::filter_map(slot, |slot| slot.published()?.downcast_ref::<T>())
            .map(InputValue::Stored)
            .map_err(|_| RunError::RuntimeBinding {
                message: format!("input port {:?} is unpublished", binding.port),
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
        let mut slot = self.store.slots[self.store.slot(binding.resource)].borrow_mut();
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
        if pending.is_some() {
            return Err(RunError::RuntimeBinding {
                message: format!("output port {:?} was written more than once", binding.port),
            });
        }
        *pending = Some(value);
        Ok(())
    }

    pub fn input_buffer<E: 'static>(&self, port: usize) -> Result<InputBuffer<'_, E>, RunError> {
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
        if let Some(bound) = self
            .module_inputs
            .bindings
            .iter()
            .find(|bound| bound.resource == binding.resource)
        {
            return bound
                .value
                .downcast_ref::<Vec<E>>()
                .map(|value| InputBuffer::Borrowed(value.as_slice()))
                .ok_or_else(|| RunError::RuntimeBinding {
                    message: format!(
                        "input port {:?} is bound as {}, expected {}",
                        binding.port,
                        bound.concrete_name,
                        type_name::<Vec<E>>()
                    ),
                });
        }
        self.store.require_published(binding.resource)?;
        let slot = self.store.slots[self.store.slot(binding.resource)].borrow();
        Ref::filter_map(slot, |slot| {
            slot.published()?
                .downcast_ref::<Vec<E>>()
                .map(Vec::as_slice)
        })
        .map(InputBuffer::Stored)
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
        let prepared = self.store.prepared_capacity(binding.resource);
        let policy = self.store.capacity_policy(binding.resource);
        let mut slot = self.store.slots[self.store.slot(binding.resource)].borrow_mut();
        let pending =
            slot.pending()
                .downcast_mut::<Vec<E>>()
                .ok_or_else(|| RunError::RuntimeBinding {
                    message: format!("output port {:?} is not a typed buffer slot", binding.port),
                })?;
        if pending.len() == prepared && policy == CapacityPolicy::RejectOverflow {
            return Err(RunError::RuntimeOverflow {
                port_ordinal: port,
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
    pub(crate) fn run(
        &mut self,
        store: &RuntimeStore,
        module_inputs: &ModuleInputs<'_>,
    ) -> Result<(), RunError> {
        store.discard(&self.unit.outputs);
        let invocation = RegistrationInvocation {
            inputs: &self.unit.inputs,
            outputs: &self.unit.outputs,
            store,
            module_inputs,
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
    poisoned: bool,
    allocation_capability: crate::AllocationCapability,
}

pub(crate) struct RuntimeBuildContext<'a> {
    requirements: &'a BTreeMap<ResourceId, ResourceRequirement>,
    storage_plan: Option<&'a StoragePlan>,
    workspace_bytes: &'a BTreeMap<UnitId, usize>,
    units: &'a UnitRegistry,
    resources: &'a ResourceRegistry,
    options: BuildOptions,
}

struct AllocatedSlots {
    slots: Vec<Box<dyn RuntimeSlot>>,
    resource_slots: Vec<usize>,
    resource_capacities: Vec<usize>,
    resource_policies: Vec<CapacityPolicy>,
}

impl PreparedRuntime {
    pub(crate) fn build_definition(
        definition: ExecutableDefinition,
        units: &UnitRegistry,
        resources: &ResourceRegistry,
        options: BuildOptions,
    ) -> Result<Self, BuildError> {
        let ExecutableDefinition {
            graph,
            configurations,
            requirements,
            workspace_bytes,
        } = definition;
        let allocation_capability = units
            .allocation_capability(&graph)
            .map_err(BuildError::Factory)?;
        if options.allocation_guarantee == crate::AllocationGuarantee::NoRunAllocation
            && !allocation_capability.strict_capable()
        {
            return Err(BuildError::StrictCapabilityUnavailable(
                allocation_capability,
            ));
        }
        let storage_plan =
            plan_storage(&graph, resources, &requirements).map_err(BuildError::StoragePlanning)?;
        let token = NEXT_PLAN_TOKEN.fetch_add(1, Ordering::Relaxed);
        let dense = graph
            .into_dense(token)
            .map_err(|error| BuildError::RuntimePreparation {
                message: error.to_string(),
            })?;
        let mut runtime = Self::build(
            dense,
            configurations,
            RuntimeBuildContext {
                requirements: &requirements,
                storage_plan: Some(&storage_plan),
                workspace_bytes: &workspace_bytes,
                units,
                resources,
                options,
            },
        )?;
        runtime.allocation_capability = allocation_capability;
        Ok(runtime)
    }

    pub(crate) fn build(
        graph: DenseGraph,
        mut configurations: BTreeMap<UnitId, DecodedConfiguration>,
        context: RuntimeBuildContext<'_>,
    ) -> Result<Self, BuildError> {
        validate_unit_descriptors(&graph, context.units)?;
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
        let AllocatedSlots {
            slots,
            resource_slots,
            resource_capacities,
            resource_policies,
        } = allocate_slots(
            &graph,
            context.requirements,
            context.storage_plan,
            context.resources,
            context.options.capacity_policy(),
        )?;
        for unit in &graph.units {
            let output_slots = unit
                .outputs
                .iter()
                .map(|binding| resource_slots[binding.resource.get()])
                .collect::<std::collections::BTreeSet<_>>();
            if output_slots.len() != unit.outputs.len()
                || unit
                    .inputs
                    .iter()
                    .map(|input| resource_slots[input.resource.get()])
                    .any(|slot| slot != NO_RUNTIME_SLOT && output_slots.contains(&slot))
            {
                return Err(BuildError::RuntimePreparation {
                    message: format!(
                        "Unit {} has aliased live input and pending output slots",
                        unit.id.as_str()
                    ),
                });
            }
        }
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
                let workspace = context
                    .workspace_bytes
                    .get(&unit.id)
                    .copied()
                    .unwrap_or_default();
                context
                    .units
                    .prepare_executable(&configuration, unit, workspace)
                    .map_err(BuildError::Factory)
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            graph,
            store: RuntimeStore::new(
                slots,
                resource_slots,
                resource_capacities,
                resource_policies,
            ),
            units: prepared_units,
            poisoned: false,
            allocation_capability: crate::AllocationCapability::inspect(Vec::new(), false),
        })
    }

    #[cfg(test)]
    pub(crate) fn run(&mut self) -> Result<(), RunError> {
        let inputs = ModuleInputs::with_capacity(self.graph.module_inputs().len());
        self.run_with_inputs(&inputs)
    }

    #[cfg(test)]
    pub(crate) fn run_with_inputs(&mut self, inputs: &ModuleInputs<'_>) -> Result<(), RunError> {
        let mut timings = crate::UnitExecutionRecorder::new(std::time::Instant::now(), false);
        let result = self.run_with_inputs_timed(inputs, &mut timings);
        result.map_err(|failure| failure.contextualize(&self.graph))
    }

    fn run_with_inputs_timed(
        &mut self,
        inputs: &ModuleInputs<'_>,
        timings: &mut crate::UnitExecutionRecorder,
    ) -> Result<(), RuntimeFailure> {
        if self.poisoned {
            return Err(RuntimeFailure::module(RunError::Poisoned));
        }
        inputs
            .validate(&self.graph)
            .map_err(RuntimeFailure::module)?;
        self.store.reset();
        for (unit_ordinal, unit) in self.graph.execution_order.iter().copied().enumerate() {
            let result = timings.measure(unit_ordinal, || {
                self.units[unit.get()].run(&self.store, inputs)
            });
            if let Err(error) = result {
                self.poisoned = matches!(
                    error,
                    RunError::Panic
                        | RunError::Unit(crate::UnitFailure {
                            disposition: FailureDisposition::Fatal,
                            ..
                        })
                );
                self.store.invalidate_publications();
                return Err(RuntimeFailure::unit(unit, error));
            }
        }
        Ok(())
    }

    pub(crate) fn input_handle<T: 'static>(
        &self,
        resource: &ResourceId,
    ) -> Result<crate::InputHandle<T>, crate::HandleError> {
        self.graph.input_handle(resource)
    }

    pub(crate) fn output_handle<T: 'static>(
        &self,
        resource: &ResourceId,
    ) -> Result<crate::OutputHandle<T>, crate::HandleError> {
        self.graph.output_handle(resource)
    }

    pub(crate) fn output<T: 'static>(
        &self,
        handle: &crate::OutputHandle<T>,
    ) -> Result<Ref<'_, T>, RunError> {
        if handle.plan_token() != self.graph.plan_token() {
            return Err(RunError::RuntimeBinding {
                message: "output handle belongs to a different graph plan".into(),
            });
        }
        self.output_value(handle.resource())
    }

    pub(crate) fn output_value<T: 'static>(
        &self,
        resource: ResourceIndex,
    ) -> Result<Ref<'_, T>, RunError> {
        self.store.output_value(resource)
    }

    #[cfg(test)]
    pub(crate) fn output_buffer<E: 'static>(
        &self,
        resource: ResourceIndex,
    ) -> Result<Ref<'_, [E]>, RunError> {
        self.store.output_buffer(resource)
    }
}

fn validate_unit_descriptors(graph: &DenseGraph, units: &UnitRegistry) -> Result<(), BuildError> {
    for unit in &graph.units {
        let descriptor = units.get(&unit.unit_type).ok_or_else(|| {
            BuildError::Factory(crate::FactoryError::UnknownUnitType {
                unit_type: unit.unit_type.clone(),
            })
        })?;
        validate_ordered_port_contract(graph, unit, "input", &unit.inputs, &descriptor.inputs)?;
        validate_ordered_port_contract(graph, unit, "output", &unit.outputs, &descriptor.outputs)?;
    }
    Ok(())
}

fn validate_ordered_port_contract(
    graph: &DenseGraph,
    unit: &DenseUnit,
    direction: &str,
    bindings: &[DenseBinding],
    ports: &[crate::PortDescriptor],
) -> Result<(), BuildError> {
    if bindings.len() != ports.len() {
        return Err(BuildError::RuntimePreparation {
            message: format!(
                "Unit {} ({}) ordered {direction} port contract has {} compiled ports but build descriptor has {}",
                unit.id.as_str(),
                unit.unit_type.as_str(),
                bindings.len(),
                ports.len()
            ),
        });
    }
    for (ordinal, (binding, port)) in bindings.iter().zip(ports).enumerate() {
        let resource = &graph.resources[binding.resource.get()];
        if binding.port != port.name
            || resource.semantic_type != port.semantic_type
            || binding.concrete_type != port.concrete_type
        {
            return Err(BuildError::RuntimePreparation {
                message: format!(
                    "Unit {} ({}) ordered {direction} port contract differs at ordinal {ordinal}",
                    unit.id.as_str(),
                    unit.unit_type.as_str()
                ),
            });
        }
    }
    Ok(())
}

#[derive(Debug)]
struct RuntimeFailure {
    unit: Option<UnitIndex>,
    cause: RunError,
}

impl RuntimeFailure {
    const fn module(cause: RunError) -> Self {
        Self { unit: None, cause }
    }

    const fn unit(unit: UnitIndex, cause: RunError) -> Self {
        Self {
            unit: Some(unit),
            cause,
        }
    }

    fn contextualize(self, graph: &DenseGraph) -> RunError {
        match self.unit {
            Some(unit) => unit_error(
                &graph.module,
                &graph.units[unit.get()],
                &graph.resources,
                self.cause,
            ),
            None => module_error(&graph.module, self.cause),
        }
    }
}

fn module_error(module: &str, cause: RunError) -> RunError {
    RunError::Execution {
        context: Box::new(RunErrorContext {
            module: module.to_owned(),
            unit: None,
            unit_type: None,
            port: None,
            resource: None,
            disposition: None,
        }),
        cause: Box::new(cause),
    }
}

fn unit_error(
    module: &str,
    unit: &DenseUnit,
    resources: &[DenseResource],
    cause: RunError,
) -> RunError {
    let disposition = match &cause {
        RunError::Unit(failure) => Some(failure.disposition),
        _ => None,
    };
    let binding = match &cause {
        RunError::RuntimeOverflow { port_ordinal, .. } => unit.outputs.get(*port_ordinal),
        _ => None,
    };
    let port = binding.map(|binding| binding.port.clone());
    let resource = binding.map(|binding| resources[binding.resource.get()].id.clone());
    RunError::Execution {
        context: Box::new(RunErrorContext {
            module: module.to_owned(),
            unit: Some(unit.id.clone()),
            unit_type: Some(unit.unit_type.clone()),
            port,
            resource,
            disposition,
        }),
        cause: Box::new(cause),
    }
}

fn allocate_slots(
    graph: &DenseGraph,
    requirements: &BTreeMap<ResourceId, ResourceRequirement>,
    storage_plan: Option<&StoragePlan>,
    resources: &ResourceRegistry,
    policy: CapacityPolicy,
) -> Result<AllocatedSlots, BuildError> {
    for resource in &graph.resources {
        let descriptor = resources.get(&resource.semantic_type).ok_or_else(|| {
            BuildError::RuntimePreparation {
                message: format!("missing descriptor for {}", resource.id.as_str()),
            }
        })?;
        if descriptor.concrete_type() != resource.concrete_type.id() {
            return Err(BuildError::RuntimePreparation {
                message: format!(
                    "Resource {} graph type {} does not match build descriptor type {}",
                    resource.id.as_str(),
                    resource.concrete_type.name(),
                    descriptor.concrete_name(),
                ),
            });
        }
    }
    let Some(storage_plan) = storage_plan else {
        let resource_capacities = graph
            .resources
            .iter()
            .map(|resource| {
                requirements
                    .get(&resource.id)
                    .map(|requirement| requirement.capacity)
                    .ok_or_else(|| BuildError::RuntimePreparation {
                        message: format!("missing requirement for {}", resource.id.as_str()),
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let resource_policies = resource_policies(graph, resources, policy)?;
        let mut slots = Vec::new();
        let mut resource_slots = vec![NO_RUNTIME_SLOT; graph.resources.len()];
        for (resource_index, resource) in graph.resources.iter().enumerate() {
            if is_module_input(graph, resource_index) {
                continue;
            }
            let descriptor = resources.get(&resource.semantic_type).ok_or_else(|| {
                BuildError::RuntimePreparation {
                    message: format!("missing descriptor for {}", resource.id.as_str()),
                }
            })?;
            let requirement =
                requirements
                    .get(&resource.id)
                    .ok_or_else(|| BuildError::RuntimePreparation {
                        message: format!("missing requirement for {}", resource.id.as_str()),
                    })?;
            resource_slots[resource_index] = slots.len();
            slots.push(
                descriptor
                    .runtime_adapter()
                    .allocate(requirement.capacity, policy)
                    .map_err(|message| BuildError::RuntimePreparation { message })?,
            );
        }
        return Ok(AllocatedSlots {
            slots,
            resource_slots,
            resource_capacities,
            resource_policies,
        });
    };

    let report = storage_plan.report();
    let assignments = report
        .assignments
        .iter()
        .map(|assignment| (&assignment.resource, assignment))
        .collect::<BTreeMap<_, _>>();
    let stored_resource_count = graph
        .resources
        .iter()
        .enumerate()
        .filter(|(resource_index, _)| !is_module_input(graph, *resource_index))
        .count();
    if assignments.len() != report.assignments.len() || assignments.len() != stored_resource_count {
        return Err(BuildError::RuntimePreparation {
            message: "storage plan does not contain exactly one assignment per stored Resource"
                .to_owned(),
        });
    }
    let mut resource_slots = vec![NO_RUNTIME_SLOT; graph.resources.len()];
    let mut resource_capacities = vec![0; graph.resources.len()];
    let mut representatives: Vec<Option<usize>> = vec![None; report.slot_count];
    let mut capacities: Vec<usize> = vec![0; report.slot_count];
    for (resource_index, resource) in graph.resources.iter().enumerate() {
        if is_module_input(graph, resource_index) {
            continue;
        }
        let assignment =
            assignments
                .get(&resource.id)
                .ok_or_else(|| BuildError::RuntimePreparation {
                    message: format!("storage plan is missing {}", resource.id.as_str()),
                })?;
        if assignment.slot >= report.slot_count {
            return Err(BuildError::RuntimePreparation {
                message: format!(
                    "storage plan assigns {} to invalid slot {}",
                    resource.id.as_str(),
                    assignment.slot
                ),
            });
        }
        resource_slots[resource_index] = assignment.slot;
        resource_capacities[resource_index] = assignment.capacity;
        capacities[assignment.slot] = capacities[assignment.slot].max(assignment.capacity);
        if let Some(representative) = representatives[assignment.slot] {
            let left = resources
                .get(&graph.resources[representative].semantic_type)
                .ok_or_else(|| BuildError::RuntimePreparation {
                    message: "storage plan representative descriptor is missing".to_owned(),
                })?;
            let right = resources.get(&resource.semantic_type).ok_or_else(|| {
                BuildError::RuntimePreparation {
                    message: format!("missing descriptor for {}", resource.id.as_str()),
                }
            })?;
            if !left.compatible_with(right) {
                return Err(BuildError::RuntimePreparation {
                    message: format!(
                        "storage slot {} mixes incompatible Resource adapters",
                        assignment.slot
                    ),
                });
            }
        } else {
            representatives[assignment.slot] = Some(resource_index);
        }
    }
    let slots = representatives
        .into_iter()
        .enumerate()
        .map(|(slot, representative)| {
            let representative = representative.ok_or_else(|| BuildError::RuntimePreparation {
                message: format!("storage plan slot {slot} has no Resource assignment"),
            })?;
            let descriptor = resources
                .get(&graph.resources[representative].semantic_type)
                .ok_or_else(|| BuildError::RuntimePreparation {
                    message: "storage plan representative descriptor is missing".to_owned(),
                })?;
            descriptor
                .runtime_adapter()
                .allocate(capacities[slot], policy)
                .map_err(|message| BuildError::RuntimePreparation { message })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(AllocatedSlots {
        slots,
        resource_slots,
        resource_capacities,
        resource_policies: resource_policies(graph, resources, policy)?,
    })
}

fn is_module_input(graph: &DenseGraph, resource_index: usize) -> bool {
    graph
        .module_inputs()
        .iter()
        .any(|input| input.get() == resource_index)
}

fn resource_policies(
    graph: &DenseGraph,
    resources: &ResourceRegistry,
    policy: CapacityPolicy,
) -> Result<Vec<CapacityPolicy>, BuildError> {
    graph
        .resources
        .iter()
        .map(|resource| {
            let descriptor = resources.get(&resource.semantic_type).ok_or_else(|| {
                BuildError::RuntimePreparation {
                    message: format!("missing descriptor for {}", resource.id.as_str()),
                }
            })?;
            Ok(match descriptor.invariants().representation {
                crate::StorageRepresentation::FixedBuffer => CapacityPolicy::RejectOverflow,
                _ => policy,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ParsedModule, ParsedUnit, PortDescriptor, SemanticType, UnitDescriptor, UnitRequirements,
        UnitTypeName, plan_storage,
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
        assert!(matches!(
            Module::build(
                ExecutableDefinition::new(
                    graph.clone(),
                    BTreeMap::new(),
                    BTreeMap::new(),
                    BTreeMap::new(),
                ),
                &UnitRegistry::default(),
                &resources,
                BuildOptions::development(),
            ),
            Err(BuildError::Factory(
                crate::FactoryError::UnknownUnitType { .. }
            ))
        ));
        let requirements = graph
            .resources
            .iter()
            .map(|resource| (resource.id.clone(), ResourceRequirement { capacity: 1 }))
            .collect();
        let storage_plan = plan_storage(&graph, &resources, &requirements).unwrap();
        assert_eq!(storage_plan.report().slot_count, 3);
        let dense = graph.clone().into_dense(23).unwrap();
        let left = dense
            .units
            .iter()
            .find(|unit| unit.id == UnitId::new("left"))
            .unwrap()
            .clone();
        let wrong_configuration = units
            .decode(
                &UnitTypeName::new("fixture.map/v1"),
                &SourceConfig(3),
                "$.config",
            )
            .unwrap();
        assert!(matches!(
            units.prepare_executable(&wrong_configuration, left, 0),
            Err(crate::FactoryError::ConfigurationUnitType { .. })
        ));
        let mut mismatched_resources = ResourceRegistry::default();
        mismatched_resources
            .register(crate::ResourceDescriptor::of::<u64>(
                scalar(),
                "mismatched scalar",
                "u64",
            ))
            .unwrap();
        assert!(matches!(
            allocate_slots(
                &dense,
                &requirements,
                None,
                &mismatched_resources,
                CapacityPolicy::RejectOverflow,
            ),
            Err(BuildError::RuntimePreparation { .. })
        ));
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
        let mut runtime = PreparedRuntime::build_definition(
            ExecutableDefinition::new(graph, configs, requirements, BTreeMap::new()),
            &units,
            &resources,
            BuildOptions::development(),
        )
        .unwrap();
        let mut timings = crate::UnitExecutionRecorder::new(std::time::Instant::now(), true);
        runtime
            .run_with_inputs_timed(&ModuleInputs::default(), &mut timings)
            .unwrap();
        assert_eq!(
            timings
                .events()
                .map(|event| event.unit_ordinal)
                .collect::<Vec<_>>(),
            vec![0, 1, 2, 3]
        );
        let result = runtime
            .output_handle::<u32>(&ResourceId::new("result"))
            .unwrap();
        let left_value = runtime
            .graph
            .units
            .iter()
            .find(|unit| unit.id == UnitId::new("map"))
            .unwrap()
            .inputs[0]
            .resource;
        let right_value = runtime
            .graph
            .units
            .iter()
            .find(|unit| unit.id == UnitId::new("join"))
            .unwrap()
            .inputs
            .iter()
            .find(|binding| binding.port == "right")
            .unwrap()
            .resource;
        assert_eq!(runtime.store.slots.len(), 3);
        assert_eq!(
            runtime.store.slot(left_value),
            runtime.store.slot(result.resource())
        );
        assert_ne!(
            runtime.store.slot(left_value),
            runtime.store.slot(right_value)
        );
        runtime.run().unwrap();
        assert_eq!(*runtime.output(&result).unwrap(), 26);
        assert!(runtime.output_value::<u32>(left_value).is_err());
        assert_eq!(*runtime.output_value::<u32>(right_value).unwrap(), 5);

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
            outputs: vec![ResourceId::new("result"), ResourceId::new("failed_output")],
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
        let partial_output = failing_dense
            .output_handle::<u32>(&ResourceId::new("result"))
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
            RuntimeBuildContext {
                requirements: &failing_requirements,
                storage_plan: None,
                workspace_bytes: &BTreeMap::new(),
                units: &units,
                resources: &resources,
                options: BuildOptions::development(),
            },
        )
        .unwrap();
        let error = failing_runtime.run().unwrap_err();
        assert!(matches!(error, RunError::Execution { .. }));
        assert!(matches!(
            error.root_cause(),
            RunError::Unit(crate::UnitFailure { .. })
        ));
        assert!(
            failing_runtime
                .output_value::<u32>(failed_output.resource())
                .is_err()
        );
        assert!(
            failing_runtime
                .output_value::<u32>(partial_output.resource())
                .is_err()
        );
    }

    #[test]
    fn build_rejects_registry_port_order_that_differs_from_compiled_contract() {
        let mut resources = ResourceRegistry::default();
        resources
            .register(crate::ResourceDescriptor::of::<u32>(
                scalar(),
                "fixed scalar",
                "initialized",
            ))
            .unwrap();
        let mut compile_units = UnitRegistry::default();
        register::<JoinUnit, _, _>(
            &mut compile_units,
            "fixture.ordered_join/v1",
            &["z", "a"],
            &["out"],
            |_| Ok(JoinUnit),
            |_, invocation, _| {
                let z = invocation.input_value::<u32>(0)?;
                let a = invocation.input_value::<u32>(1)?;
                invocation.write_value(0, *z - *a)
            },
        );
        let graph = ParsedModule {
            schema: "unit-compose/v0alpha1".to_owned(),
            name: "ordered-contract".to_owned(),
            inputs: vec![
                crate::ParsedModuleInput {
                    resource: ResourceId::new("z_value"),
                    semantic_type: scalar(),
                },
                crate::ParsedModuleInput {
                    resource: ResourceId::new("a_value"),
                    semantic_type: scalar(),
                },
            ],
            units: vec![unit(
                "join",
                "fixture.ordered_join/v1",
                &[("a", "a_value"), ("z", "z_value")],
                "result",
            )],
            outputs: vec![ResourceId::new("result")],
        }
        .resolve(&compile_units, &resources)
        .unwrap()
        .compile()
        .unwrap();
        let configuration = compile_units
            .decode(
                &UnitTypeName::new("fixture.ordered_join/v1"),
                &SourceConfig(0),
                "$.config",
            )
            .unwrap();
        let requirements = graph
            .resources
            .iter()
            .map(|resource| (resource.id.clone(), ResourceRequirement { capacity: 1 }))
            .collect();

        let mut build_units = UnitRegistry::default();
        register::<JoinUnit, _, _>(
            &mut build_units,
            "fixture.ordered_join/v1",
            &["a", "z"],
            &["out"],
            |_| Ok(JoinUnit),
            |_, invocation, _| {
                let a = invocation.input_value::<u32>(0)?;
                let z = invocation.input_value::<u32>(1)?;
                invocation.write_value(0, *z - *a)
            },
        );

        let error = match Module::build(
            ExecutableDefinition::new(
                graph,
                BTreeMap::from([(UnitId::new("join"), configuration)]),
                requirements,
                BTreeMap::new(),
            ),
            &build_units,
            &resources,
            BuildOptions::development(),
        ) {
            Ok(_) => panic!("mismatched build registry must be rejected"),
            Err(error) => error,
        };
        assert!(matches!(
            error,
            BuildError::RuntimePreparation { message }
                if message.contains("ordered input port contract")
        ));
    }

    #[test]
    fn fixed_value_output_rejects_a_second_write() {
        let mut resources = ResourceRegistry::default();
        resources
            .register(crate::ResourceDescriptor::of::<u32>(
                scalar(),
                "fixed scalar",
                "initialized once",
            ))
            .unwrap();
        let mut units = UnitRegistry::default();
        register::<SourceUnit, _, _>(
            &mut units,
            "fixture.double_write/v1",
            &[],
            &["out"],
            |config| Ok(SourceUnit(config.0)),
            |unit, invocation, _| {
                invocation.write_value(0, unit.0)?;
                invocation.write_value(0, unit.0 + 1)
            },
        );
        let graph = ParsedModule {
            schema: "unit-compose/v0alpha1".to_owned(),
            name: "double-write".to_owned(),
            inputs: vec![],
            units: vec![unit("source", "fixture.double_write/v1", &[], "result")],
            outputs: vec![ResourceId::new("result")],
        }
        .resolve(&units, &resources)
        .unwrap()
        .compile()
        .unwrap();
        let configuration = units
            .decode(
                &UnitTypeName::new("fixture.double_write/v1"),
                &SourceConfig(3),
                "$.config",
            )
            .unwrap();
        let requirements = BTreeMap::from([(
            ResourceId::new("result"),
            ResourceRequirement { capacity: 1 },
        )]);
        let mut runtime = PreparedRuntime::build_definition(
            ExecutableDefinition::new(
                graph,
                BTreeMap::from([(UnitId::new("source"), configuration)]),
                requirements,
                BTreeMap::new(),
            ),
            &units,
            &resources,
            BuildOptions::development(),
        )
        .unwrap();

        let error = runtime.run().unwrap_err();
        assert!(matches!(
            error.root_cause(),
            RunError::RuntimeBinding { message } if message.contains("more than once")
        ));
    }

    #[test]
    fn oversized_workspace_returns_a_build_error() {
        let mut resources = ResourceRegistry::default();
        resources
            .register(crate::ResourceDescriptor::of::<u32>(
                scalar(),
                "fixed scalar",
                "workspace fixture",
            ))
            .unwrap();
        let mut units = UnitRegistry::default();
        register::<SourceUnit, _, _>(
            &mut units,
            "fixture.workspace/v1",
            &[],
            &["out"],
            |config| Ok(SourceUnit(config.0)),
            |unit, invocation, _| invocation.write_value(0, unit.0),
        );
        let graph = ParsedModule {
            schema: "unit-compose/v0alpha1".to_owned(),
            name: "oversized-workspace".to_owned(),
            inputs: vec![],
            units: vec![unit("source", "fixture.workspace/v1", &[], "result")],
            outputs: vec![ResourceId::new("result")],
        }
        .resolve(&units, &resources)
        .unwrap()
        .compile()
        .unwrap();
        let configuration = units
            .decode(
                &UnitTypeName::new("fixture.workspace/v1"),
                &SourceConfig(3),
                "$.config",
            )
            .unwrap();
        let error = match Module::build(
            ExecutableDefinition::new(
                graph,
                BTreeMap::from([(UnitId::new("source"), configuration)]),
                BTreeMap::from([(
                    ResourceId::new("result"),
                    ResourceRequirement { capacity: 1 },
                )]),
                BTreeMap::from([(UnitId::new("source"), usize::MAX)]),
            ),
            &units,
            &resources,
            BuildOptions::development(),
        ) {
            Ok(_) => panic!("oversized workspace must be rejected"),
            Err(error) => error,
        };

        assert!(matches!(
            error,
            BuildError::Factory(crate::FactoryError::WorkspaceAllocation { bytes, .. })
                if bytes == usize::MAX
        ));
    }

    #[test]
    fn oversized_buffer_capacity_returns_an_allocation_error() {
        let error = match RuntimeResourceAdapter::bounded_buffer::<u32>()
            .allocate(usize::MAX, CapacityPolicy::RejectOverflow)
        {
            Ok(_) => panic!("oversized buffer capacity must be rejected"),
            Err(error) => error,
        };

        assert!(error.contains("failed to reserve published buffer"));
    }

    #[test]
    fn module_timing_overhead_counts_dropped_unit_events() {
        let mut timings = crate::UnitExecutionRecorder::new(std::time::Instant::now(), true);
        for ordinal in 0..20 {
            timings.measure(ordinal, || Ok(())).unwrap();
        }

        assert_eq!(timings.len, crate::UNIT_TIMING_CAPACITY);
        assert_eq!(timings.dropped_events, 4);
        assert_eq!(timings.module_clock_reads(), 42);
    }

    #[test]
    fn failed_input_preflight_invalidates_previous_publications() {
        let mut resources = ResourceRegistry::default();
        resources
            .register(crate::ResourceDescriptor::of::<u32>(
                scalar(),
                "fixed scalar",
                "borrowed input and stored output",
            ))
            .unwrap();
        let mut units = UnitRegistry::default();
        register::<MapUnit, _, _>(
            &mut units,
            "fixture.echo/v1",
            &["in"],
            &["out"],
            |_| Ok(MapUnit(1)),
            |_, invocation, _| {
                let input = invocation.input_value::<u32>(0)?;
                invocation.write_value(0, *input)
            },
        );
        let graph = ParsedModule {
            schema: "unit-compose/v0alpha1".to_owned(),
            name: "input-preflight".to_owned(),
            inputs: vec![crate::ParsedModuleInput {
                resource: ResourceId::new("input"),
                semantic_type: scalar(),
            }],
            units: vec![unit(
                "echo",
                "fixture.echo/v1",
                &[("in", "input")],
                "result",
            )],
            outputs: vec![ResourceId::new("result")],
        }
        .resolve(&units, &resources)
        .unwrap()
        .compile()
        .unwrap();
        let configuration = units
            .decode(
                &UnitTypeName::new("fixture.echo/v1"),
                &SourceConfig(0),
                "$.config",
            )
            .unwrap();
        let requirements = graph
            .resources
            .iter()
            .map(|resource| (resource.id.clone(), ResourceRequirement { capacity: 1 }))
            .collect();
        let mut module = Module::build(
            ExecutableDefinition::new(
                graph,
                BTreeMap::from([(UnitId::new("echo"), configuration)]),
                requirements,
                BTreeMap::new(),
            ),
            &units,
            &resources,
            BuildOptions::development(),
        )
        .unwrap();
        let input = module
            .input_handle::<u32>(&ResourceId::new("input"))
            .unwrap();
        let output = module
            .output_handle::<u32>(&ResourceId::new("result"))
            .unwrap();
        let value = 7_u32;
        let mut valid_inputs = ModuleInputs::with_capacity(1);
        valid_inputs.bind(&input, &value).unwrap();

        module.run(&valid_inputs).unwrap();
        assert_eq!(*module.output(&output).unwrap(), value);

        let error = module.run(&ModuleInputs::default()).unwrap_err();
        assert!(matches!(
            error.root_cause(),
            RunError::RuntimeBinding { message }
                if message.contains("do not exactly match required inputs")
        ));
        assert!(module.output(&output).is_err());
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
            .register(crate::ResourceDescriptor::bounded_buffer::<u32>(
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
            RuntimeBuildContext {
                requirements: &requirements,
                storage_plan: None,
                workspace_bytes: &BTreeMap::new(),
                units: &units,
                resources: &resources,
                options: BuildOptions::strict(),
            },
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
            RuntimeBuildContext {
                requirements: &requirements,
                storage_plan: None,
                workspace_bytes: &BTreeMap::new(),
                units: &units,
                resources: &resources,
                options: BuildOptions::strict(),
            },
        )
        .unwrap();
        let error = overflow.run().unwrap_err();
        assert!(matches!(error, RunError::Execution { .. }));
        let context = error.context().unwrap();
        assert_eq!(context.port.as_deref(), Some("out"));
        assert_eq!(
            context.resource.as_ref().map(ResourceId::as_str),
            Some("result")
        );
        assert!(matches!(
            error.root_cause(),
            RunError::RuntimeOverflow {
                port_ordinal: 0,
                required: 3,
                prepared: 2,
                ..
            }
        ));
        assert!(
            overflow
                .output_buffer::<u32>(overflow_output.resource())
                .is_err()
        );
        let slot = overflow.store.slots[overflow_output.resource().get()].borrow();
        assert!(slot.pending_complete(2));
        assert_eq!(
            overflow.store.prepared_capacity(overflow_output.resource()),
            2
        );
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
            RuntimeBuildContext {
                requirements: &requirements,
                storage_plan: None,
                workspace_bytes: &BTreeMap::new(),
                units: &units,
                resources: &resources,
                options: BuildOptions::development(),
            },
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
    fn aliased_fixed_buffers_keep_logical_capacities_and_reject_growth() {
        #[derive(Clone, Copy)]
        struct BufferConfig {
            writes: usize,
        }
        struct BufferSource {
            writes: usize,
        }

        let buffer_type = SemanticType::new("fixture.FixedBuffer/v1").unwrap();
        let mut resources = ResourceRegistry::default();
        resources
            .register(crate::ResourceDescriptor::fixed_buffer::<u32>(
                buffer_type.clone(),
                "fixed u32 fixture",
                "exact length",
            ))
            .unwrap();
        let mut units = UnitRegistry::default();
        let unit_type = UnitTypeName::new("fixture.fixed_buffer_source/v1");
        units
            .register::<BufferConfig, BufferConfig, _, _>(
                UnitDescriptor {
                    type_name: unit_type.clone(),
                    inputs: vec![],
                    outputs: vec![PortDescriptor::of::<Vec<u32>>("out", buffer_type)],
                },
                |source, _| Ok(*source),
                |_, _| Ok(UnitRequirements::default()),
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

        let graph = ParsedModule {
            schema: "unit-compose/v0alpha1".to_owned(),
            name: "aliased-fixed-buffers".to_owned(),
            inputs: vec![],
            units: vec![
                unit("first", "fixture.fixed_buffer_source/v1", &[], "scratch"),
                unit("second", "fixture.fixed_buffer_source/v1", &[], "result"),
            ],
            outputs: vec![ResourceId::new("result")],
        }
        .resolve(&units, &resources)
        .unwrap()
        .compile()
        .unwrap();
        let requirements = BTreeMap::from([
            (
                ResourceId::new("scratch"),
                ResourceRequirement { capacity: 2 },
            ),
            (
                ResourceId::new("result"),
                ResourceRequirement { capacity: 3 },
            ),
        ]);
        let configurations = [("first", 2), ("second", 3)]
            .map(|(id, writes)| {
                (
                    UnitId::new(id),
                    units
                        .decode(&unit_type, &BufferConfig { writes }, "$.config")
                        .unwrap(),
                )
            })
            .into_iter()
            .collect();
        let mut runtime = PreparedRuntime::build_definition(
            ExecutableDefinition::new(graph, configurations, requirements, BTreeMap::new()),
            &units,
            &resources,
            BuildOptions::development(),
        )
        .unwrap();
        runtime.run().unwrap();
        let scratch = runtime
            .graph
            .units
            .iter()
            .find(|unit| unit.id == UnitId::new("first"))
            .unwrap()
            .outputs[0]
            .resource;
        let result = runtime
            .output_handle::<Vec<u32>>(&ResourceId::new("result"))
            .unwrap();
        assert_eq!(
            runtime.store.slot(scratch),
            runtime.store.slot(result.resource())
        );
        assert_eq!(runtime.store.prepared_capacity(scratch), 2);
        assert_eq!(runtime.store.prepared_capacity(result.resource()), 3);
        assert_eq!(
            &*runtime.output_buffer::<u32>(result.resource()).unwrap(),
            &[0, 1, 2]
        );
        assert_eq!(runtime.store.observed_capacity(), 3);

        let overflow_graph = ParsedModule {
            schema: "unit-compose/v0alpha1".to_owned(),
            name: "fixed-buffer-overflow".to_owned(),
            inputs: vec![],
            units: vec![unit(
                "source",
                "fixture.fixed_buffer_source/v1",
                &[],
                "result",
            )],
            outputs: vec![ResourceId::new("result")],
        }
        .resolve(&units, &resources)
        .unwrap()
        .compile()
        .unwrap();
        let configuration = units
            .decode(&unit_type, &BufferConfig { writes: 3 }, "$.config")
            .unwrap();
        let mut overflow = PreparedRuntime::build_definition(
            ExecutableDefinition::new(
                overflow_graph,
                BTreeMap::from([(UnitId::new("source"), configuration)]),
                BTreeMap::from([(
                    ResourceId::new("result"),
                    ResourceRequirement { capacity: 2 },
                )]),
                BTreeMap::new(),
            ),
            &units,
            &resources,
            BuildOptions::development(),
        )
        .unwrap();
        assert!(matches!(
            overflow.run().unwrap_err().root_cause(),
            RunError::RuntimeOverflow {
                required: 3,
                prepared: 2,
                policy: CapacityPolicy::RejectOverflow,
                ..
            }
        ));
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
            .register(crate::ResourceDescriptor::fixed_buffer::<DropProbe>(
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
                RuntimeBuildContext {
                    requirements: &requirements,
                    storage_plan: None,
                    workspace_bytes: &BTreeMap::new(),
                    units: &units,
                    resources: &resources,
                    options: BuildOptions::strict(),
                },
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
        let error = partial.run().unwrap_err();
        assert!(matches!(error, RunError::Execution { .. }));
        assert!(matches!(
            error.root_cause(),
            RunError::RuntimeBinding { .. }
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
        let error = errors.run().unwrap_err();
        assert!(matches!(error, RunError::Execution { .. }));
        assert!(matches!(
            error.root_cause(),
            RunError::Unit(crate::UnitFailure { .. })
        ));
        assert_eq!(error_drops.load(Ordering::SeqCst), 3);
        assert!(errors.output_buffer::<DropProbe>(complete_output).is_err());
        assert!(errors.output_buffer::<DropProbe>(partial_output).is_err());
        let error = errors.run().unwrap_err();
        assert!(matches!(error, RunError::Execution { .. }));
        assert!(matches!(
            error.root_cause(),
            RunError::Unit(crate::UnitFailure { .. })
        ));

        let panic_drops = Arc::new(AtomicUsize::new(0));
        let (mut panics, complete_output, partial_output) = build(
            "fixture.panic/v1",
            33,
            Arc::clone(&panic_drops),
            true,
            false,
            false,
        );
        let error = panics.run().unwrap_err();
        assert!(matches!(error, RunError::Execution { .. }));
        assert_eq!(error.root_cause(), &RunError::Panic);
        assert_eq!(panic_drops.load(Ordering::SeqCst), 3);
        assert!(panics.output_buffer::<DropProbe>(complete_output).is_err());
        assert!(panics.output_buffer::<DropProbe>(partial_output).is_err());
        let error = panics.run().unwrap_err();
        assert!(matches!(error, RunError::Execution { .. }));
        assert_eq!(error.root_cause(), &RunError::Poisoned);
    }
}
