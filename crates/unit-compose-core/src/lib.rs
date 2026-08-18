//! UnitCompose core contracts.
//!
//! The crate contains the typed execution kernel, normalized graph compiler,
//! conservative typed storage planner, strict allocation contracts, and fixed
//! inspection model. YAML parsing and application adapters remain separate
//! crates so core behavior has no frontend or framework dependency.

mod graph;
mod inspection;
#[allow(dead_code)]
mod runtime;
mod storage;

pub use graph::{
    CompileError, CompiledGraph, CompiledResource, CompiledUnit, ConcreteType, ConfigurationError,
    ConstructedUnit, Consumer, DecodedConfiguration, DenseBinding, DenseGraph, DenseResource,
    DenseUnit, FactoryError, HandleError, InputHandle, OutputHandle, ParsedModule,
    ParsedModuleInput, ParsedUnit, PortDescriptor, Producer, RegistrationError, ResolvedBinding,
    ResolvedModule, ResolvedModuleInput, ResolvedUnit, ResourceId, ResourceIndex, UnitDescriptor,
    UnitId, UnitIndex, UnitRegistry, UnitTypeName,
};
pub use inspection::{FixedModuleDescription, UnitConfigurationSummary, UnitWorkspaceDescription};
pub use runtime::{
    InputBindingError, InputBuffer, InputValue, Module, ModuleInputs, RegistrationInvocation,
};
pub use storage::{
    InputValidationError, LiveRange, ModuleInput, PlanningError, PreparedInputPlan,
    PreparedInputSpec, ResourceRequirement, SlotAssignment, StoragePlan, StorageReport,
    calculate_live_ranges, plan_storage,
};

use std::any::{TypeId, type_name};
use std::collections::{BTreeMap, btree_map::Entry};
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::{Duration, Instant};

/// Host- and adapter-provided bounds available while resolving one Unit's
/// typed configuration.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BoundSources {
    pub host: BTreeMap<ResourceId, usize>,
    pub adapters: BTreeMap<SemanticType, usize>,
}

/// Prepared output and workspace requirements resolved from typed config.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct UnitRequirements {
    pub output_capacities: BTreeMap<String, usize>,
    pub workspace_bytes: usize,
}

/// Stable serialized identity of a Resource representation.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SemanticType(String);

impl SemanticType {
    /// Creates a namespaced and versioned semantic identity.
    pub fn new(value: impl Into<String>) -> Result<Self, DescriptorError> {
        let value = value.into();
        if value.contains('.') && value.contains("/v") {
            Ok(Self(value))
        } else {
            Err(DescriptorError::InvalidSemanticType(value))
        }
    }

    /// Returns the serialized identity.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Framework-owned physical form of a Resource value.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum StorageRepresentation {
    FixedValue,
    FixedBuffer,
    BoundedBuffer,
}

/// Complete representation authority for one semantic Resource type.
#[derive(Clone, Debug)]
pub struct ResourceDescriptor {
    semantic_type: SemanticType,
    concrete_type: TypeId,
    concrete_name: &'static str,
    element_size: usize,
    element_alignment: usize,
    representation: StorageRepresentation,
    adapter: &'static str,
    initialization: &'static str,
    reset: &'static str,
    validation: &'static str,
    drop_behavior: &'static str,
    runtime_adapter: runtime::RuntimeResourceAdapter,
}

impl ResourceDescriptor {
    /// Describes a concrete representation without exposing allocator details.
    pub fn of<T: 'static>(
        semantic_type: SemanticType,
        adapter: &'static str,
        validation: &'static str,
    ) -> Self {
        Self {
            semantic_type,
            concrete_type: TypeId::of::<T>(),
            concrete_name: type_name::<T>(),
            element_size: size_of::<T>(),
            element_alignment: align_of::<T>(),
            representation: StorageRepresentation::FixedValue,
            adapter,
            initialization: "initialize exactly one typed value",
            reset: "drop published value before the next run",
            validation,
            drop_behavior: "Rust Drop",
            runtime_adapter: runtime::RuntimeResourceAdapter::fixed_value::<T>(),
        }
    }

    /// Describes a fixed-length typed buffer. `R` is the public Resource type
    /// and `E` is the element stored by the framework adapter.
    pub fn fixed_buffer<R: 'static, E: 'static>(
        semantic_type: SemanticType,
        adapter: &'static str,
        validation: &'static str,
    ) -> Self {
        Self::buffer::<R, E>(
            semantic_type,
            adapter,
            validation,
            StorageRepresentation::FixedBuffer,
        )
    }

    /// Describes a bounded variable-length typed buffer.
    pub fn bounded_buffer<R: 'static, E: 'static>(
        semantic_type: SemanticType,
        adapter: &'static str,
        validation: &'static str,
    ) -> Self {
        Self::buffer::<R, E>(
            semantic_type,
            adapter,
            validation,
            StorageRepresentation::BoundedBuffer,
        )
    }

    fn buffer<R: 'static, E: 'static>(
        semantic_type: SemanticType,
        adapter: &'static str,
        validation: &'static str,
        representation: StorageRepresentation,
    ) -> Self {
        Self {
            semantic_type,
            concrete_type: TypeId::of::<R>(),
            concrete_name: type_name::<R>(),
            element_size: size_of::<E>(),
            element_alignment: align_of::<E>(),
            representation,
            adapter,
            initialization: "initialize elements in logical index order",
            reset: "drop initialized elements and reset logical length",
            validation,
            drop_behavior: "drop initialized elements only",
            runtime_adapter: match representation {
                StorageRepresentation::FixedBuffer => {
                    runtime::RuntimeResourceAdapter::fixed_buffer::<E>()
                }
                StorageRepresentation::BoundedBuffer => {
                    runtime::RuntimeResourceAdapter::bounded_buffer::<E>()
                }
                StorageRepresentation::FixedValue => unreachable!("buffer representation"),
            },
        }
    }

    /// Returns the stable semantic identity.
    #[must_use]
    pub fn semantic_type(&self) -> &SemanticType {
        &self.semantic_type
    }

    /// Tests the registered concrete representation.
    #[must_use]
    pub fn represents<T: 'static>(&self) -> bool {
        self.concrete_type == TypeId::of::<T>()
    }

    /// Returns the registered concrete representation identity.
    #[must_use]
    pub(crate) const fn concrete_type(&self) -> TypeId {
        self.concrete_type
    }

    /// Returns the registered concrete representation name.
    #[must_use]
    pub(crate) const fn concrete_name(&self) -> &'static str {
        self.concrete_name
    }

    /// Returns inspectable representation invariants.
    #[must_use]
    pub fn invariants(&self) -> RepresentationInvariants<'_> {
        RepresentationInvariants {
            concrete_name: self.concrete_name,
            element_size: self.element_size,
            element_alignment: self.element_alignment,
            representation: self.representation,
            adapter: self.adapter,
            initialization: self.initialization,
            reset: self.reset,
            validation: self.validation,
            drop_behavior: self.drop_behavior,
        }
    }

    #[must_use]
    pub(crate) fn compatible_with(&self, other: &Self) -> bool {
        self.concrete_type == other.concrete_type
            && self.element_size == other.element_size
            && self.element_alignment == other.element_alignment
            && self.representation == other.representation
            && self.adapter == other.adapter
            && self.initialization == other.initialization
            && self.reset == other.reset
            && self.validation == other.validation
            && self.drop_behavior == other.drop_behavior
            && self.runtime_adapter.identity() == other.runtime_adapter.identity()
    }

    #[allow(dead_code)]
    pub(crate) const fn runtime_adapter(&self) -> runtime::RuntimeResourceAdapter {
        self.runtime_adapter
    }
}

/// Read-only descriptor details used by build inspection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RepresentationInvariants<'a> {
    pub concrete_name: &'static str,
    pub element_size: usize,
    pub element_alignment: usize,
    pub representation: StorageRepresentation,
    pub adapter: &'a str,
    pub initialization: &'a str,
    pub reset: &'a str,
    pub validation: &'a str,
    pub drop_behavior: &'a str,
}

/// Semantic-to-concrete representation registry.
#[derive(Default)]
pub struct ResourceRegistry {
    descriptors: BTreeMap<SemanticType, ResourceDescriptor>,
}

impl ResourceRegistry {
    /// Registers the sole representation authority for a semantic type.
    pub fn register(&mut self, descriptor: ResourceDescriptor) -> Result<(), DescriptorError> {
        let key = descriptor.semantic_type.clone();
        match self.descriptors.entry(key) {
            Entry::Vacant(entry) => {
                entry.insert(descriptor);
                Ok(())
            }
            Entry::Occupied(entry) => Err(DescriptorError::DuplicateSemanticType(
                entry.key().as_str().to_owned(),
            )),
        }
    }

    /// Resolves a descriptor outside the execution hot path.
    #[must_use]
    pub fn get(&self, semantic_type: &SemanticType) -> Option<&ResourceDescriptor> {
        self.descriptors.get(semantic_type)
    }
}

/// Descriptor validation failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DescriptorError {
    InvalidSemanticType(String),
    DuplicateSemanticType(String),
}

impl fmt::Display for DescriptorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for DescriptorError {}

/// Framework-owned output capacity behavior.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CapacityPolicy {
    GrowAndMeasure,
    RejectOverflow,
}

/// Steady-state allocation claim.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AllocationGuarantee {
    BestEffort,
    NoRunAllocation,
}

/// Validated construction options.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BuildOptions {
    capacity_policy: CapacityPolicy,
    allocation_guarantee: AllocationGuarantee,
}

impl BuildOptions {
    /// Friendly development behavior with no allocation claim.
    #[must_use]
    pub const fn development() -> Self {
        Self {
            capacity_policy: CapacityPolicy::GrowAndMeasure,
            allocation_guarantee: AllocationGuarantee::BestEffort,
        }
    }

    /// Strict bounded behavior. Capability still depends on allocation-domain validation.
    #[must_use]
    pub const fn strict() -> Self {
        Self {
            capacity_policy: CapacityPolicy::RejectOverflow,
            allocation_guarantee: AllocationGuarantee::NoRunAllocation,
        }
    }

    /// Rejects combinations that cannot uphold their advertised guarantee.
    pub const fn try_new(
        capacity_policy: CapacityPolicy,
        allocation_guarantee: AllocationGuarantee,
    ) -> Result<Self, BuildOptionError> {
        if matches!(capacity_policy, CapacityPolicy::GrowAndMeasure)
            && matches!(allocation_guarantee, AllocationGuarantee::NoRunAllocation)
        {
            Err(BuildOptionError::GrowthWithNoRunAllocation)
        } else {
            Ok(Self {
                capacity_policy,
                allocation_guarantee,
            })
        }
    }

    #[must_use]
    pub const fn capacity_policy(self) -> CapacityPolicy {
        self.capacity_policy
    }

    #[must_use]
    pub const fn allocation_guarantee(self) -> AllocationGuarantee {
        self.allocation_guarantee
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BuildOptionError {
    GrowthWithNoRunAllocation,
}

/// How one declared allocation domain supports a strict claim.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AllocationEvidence {
    Instrumented,
    /// A trusted assertion that cannot be mechanically proven by the framework.
    Certified {
        source: String,
    },
    Unsupported,
}

/// Whether preparation has a finite allocation requirement.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RequirementStatus {
    Fixed,
    Bounded,
    Dynamic,
    Unresolved,
}

/// Allocation operation totals observed by one domain probe.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AllocationOperations {
    pub allocations: usize,
    pub reallocations: usize,
    pub deallocations: usize,
}

impl AllocationOperations {
    #[must_use]
    pub const fn is_zero(self) -> bool {
        self.allocations == 0 && self.reallocations == 0 && self.deallocations == 0
    }
}

/// Adapter hook whose scope must cover exactly one `Module::run_profiled` call.
pub trait AllocationDomainProbe {
    fn domain(&self) -> &str;
    fn begin(&mut self);
    fn finish(&mut self) -> AllocationOperations;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AllocationDomain {
    pub name: String,
    pub evidence: AllocationEvidence,
}

/// Inspectable strict-allocation capability. Completeness of declarations and
/// certifications remains a trusted integrator responsibility.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AllocationCapability {
    strict_capable: bool,
    declarations_complete_by_trusted_assertion: bool,
    domains: Vec<AllocationDomain>,
}

impl AllocationCapability {
    pub fn inspect(
        domains: Vec<AllocationDomain>,
        declarations_complete_by_trusted_assertion: bool,
    ) -> Self {
        let strict_capable = declarations_complete_by_trusted_assertion
            && !domains.is_empty()
            && domains.iter().all(|domain| {
                !domain.name.trim().is_empty()
                    && match &domain.evidence {
                        AllocationEvidence::Instrumented => true,
                        AllocationEvidence::Certified { source } => !source.trim().is_empty(),
                        AllocationEvidence::Unsupported => false,
                    }
            });
        Self {
            strict_capable,
            declarations_complete_by_trusted_assertion,
            domains,
        }
    }

    #[must_use]
    pub const fn strict_capable(&self) -> bool {
        self.strict_capable
    }

    #[must_use]
    pub const fn declarations_are_trusted(&self) -> bool {
        self.declarations_complete_by_trusted_assertion
    }

    #[must_use]
    pub fn domains(&self) -> &[AllocationDomain] {
        &self.domains
    }
}

/// Scratch bytes prepared by the caller Module for one invocation.
pub struct UnitWorkspace<'a> {
    pub(crate) bytes: &'a mut [u8],
}

impl UnitWorkspace<'_> {
    #[must_use]
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    pub fn bytes(&mut self) -> &mut [u8] {
        self.bytes
    }
}

/// Failure disposition selected explicitly by a Unit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FailureDisposition {
    Recoverable,
    Fatal,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnitFailure {
    pub disposition: FailureDisposition,
    pub message: &'static str,
}

impl UnitFailure {
    #[must_use]
    pub const fn recoverable(message: &'static str) -> Self {
        Self {
            disposition: FailureDisposition::Recoverable,
            message,
        }
    }

    #[must_use]
    pub const fn fatal(message: &'static str) -> Self {
        Self {
            disposition: FailureDisposition::Fatal,
            message,
        }
    }
}

/// Structured bounded-capacity failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapacityError {
    pub resource: &'static str,
    pub required: usize,
    pub prepared: usize,
    pub policy: CapacityPolicy,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RunError {
    Unit(UnitFailure),
    Capacity(CapacityError),
    IncompleteOutput {
        resource: &'static str,
    },
    Panic,
    Poisoned,
    InvalidInput {
        message: &'static str,
    },
    Input(InputValidationError),
    AllocationProfileViolation {
        domain: String,
        operations: AllocationOperations,
    },
    RuntimeBinding {
        message: String,
    },
    RuntimeOverflow {
        port: String,
        required: usize,
        prepared: usize,
        policy: CapacityPolicy,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RunEventKind {
    Success,
    RecoverableFailure,
    FatalFailure,
    Overflow,
    IncompleteOutput,
    Panic,
    AllocationProfileViolation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RunEvent {
    pub kind: RunEventKind,
    pub observed_capacity: usize,
    /// Wall-clock duration of the Unit execution boundary. It is observational,
    /// platform-dependent, and not deterministic.
    pub elapsed: Duration,
    pub timing_scope: TimingScope,
    pub timing_overhead: TimingOverhead,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TimingScope {
    ModuleExecution,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TimingOverhead {
    /// Two monotonic clock reads are included around Unit execution.
    pub clock_reads: u8,
    /// Report writes are bounded but are not included in `RunEvent::elapsed`.
    pub bounded_report_write_in_elapsed: bool,
}

pub const RUN_REPORT_CAPACITY: usize = 16;
pub const UNIT_TIMING_CAPACITY: usize = 16;

/// One measured execution stage declared by a composite Unit.
///
/// The ordinal indexes the prepared graph's stable execution order, avoiding
/// string allocation inside the measured run boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnitTimingEvent {
    pub unit_ordinal: usize,
    pub kind: RunEventKind,
    pub started_after_module_start: Duration,
    pub elapsed: Duration,
    pub timing_overhead: TimingOverhead,
}

/// Framework-owned bounded recorder passed to composite Unit executors.
pub struct UnitExecutionRecorder {
    module_started: Instant,
    events: [Option<UnitTimingEvent>; UNIT_TIMING_CAPACITY],
    len: usize,
    dropped_events: usize,
    enabled: bool,
}

impl UnitExecutionRecorder {
    fn new(module_started: Instant, enabled: bool) -> Self {
        Self {
            module_started,
            events: [None; UNIT_TIMING_CAPACITY],
            len: 0,
            dropped_events: 0,
            enabled,
        }
    }

    /// Measures one declared Unit boundary without allocating.
    pub fn measure<T>(
        &mut self,
        unit_ordinal: usize,
        operation: impl FnOnce() -> Result<T, RunError>,
    ) -> Result<T, RunError> {
        if !self.enabled {
            return operation();
        }
        let started = Instant::now();
        let result = operation();
        let event = UnitTimingEvent {
            unit_ordinal,
            kind: event_kind(&result),
            started_after_module_start: started
                .checked_duration_since(self.module_started)
                .unwrap_or_default(),
            elapsed: started.elapsed(),
            timing_overhead: TimingOverhead {
                clock_reads: 2,
                bounded_report_write_in_elapsed: false,
            },
        };
        if self.len < UNIT_TIMING_CAPACITY {
            self.events[self.len] = Some(event);
            self.len += 1;
        } else {
            self.dropped_events = self.dropped_events.saturating_add(1);
        }
        result
    }

    fn events(&self) -> impl Iterator<Item = &UnitTimingEvent> {
        self.events[..self.len].iter().flatten()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunReport {
    events: [Option<RunEvent>; RUN_REPORT_CAPACITY],
    len: usize,
    dropped_events: usize,
    observed_capacity_peak: usize,
    allocation_operations: AllocationOperations,
    unit_timings: [Option<UnitTimingEvent>; UNIT_TIMING_CAPACITY],
    unit_timing_len: usize,
    dropped_unit_timings: usize,
}

impl Default for RunReport {
    fn default() -> Self {
        Self {
            events: [None; RUN_REPORT_CAPACITY],
            len: 0,
            dropped_events: 0,
            observed_capacity_peak: 0,
            allocation_operations: AllocationOperations::default(),
            unit_timings: [None; UNIT_TIMING_CAPACITY],
            unit_timing_len: 0,
            dropped_unit_timings: 0,
        }
    }
}

impl RunReport {
    fn reset(&mut self) {
        *self = Self::default();
    }

    fn push(&mut self, event: RunEvent) {
        self.observed_capacity_peak = self.observed_capacity_peak.max(event.observed_capacity);
        if self.len < RUN_REPORT_CAPACITY {
            self.events[self.len] = Some(event);
            self.len += 1;
        } else {
            self.dropped_events += 1;
        }
    }

    pub fn events(&self) -> impl Iterator<Item = &RunEvent> {
        self.events[..self.len].iter().flatten()
    }

    #[must_use]
    pub const fn dropped_events(&self) -> usize {
        self.dropped_events
    }

    #[must_use]
    pub const fn observed_capacity_peak(&self) -> usize {
        self.observed_capacity_peak
    }

    #[must_use]
    pub const fn allocation_operations(&self) -> AllocationOperations {
        self.allocation_operations
    }

    pub fn unit_timings(&self) -> impl Iterator<Item = &UnitTimingEvent> {
        self.unit_timings[..self.unit_timing_len].iter().flatten()
    }

    #[must_use]
    pub const fn dropped_unit_timings(&self) -> usize {
        self.dropped_unit_timings
    }

    /// Takes an owned bounded snapshot without exposing later mutable runs.
    #[must_use]
    pub fn snapshot(&self) -> RunReportSnapshot {
        self.clone()
    }
}

pub type RunReportSnapshot = RunReport;

/// A sink participates in the measured run boundary and therefore must obey
/// the Module's declared allocation policy.
pub trait DiagnosticSink {
    fn record(&mut self, event: RunEvent);
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedModuleDescription {
    pub options: BuildOptions,
    pub requirement_status: RequirementStatus,
    pub allocation_capability: AllocationCapability,
    /// Construction and explicitly declared warm-up are outside the run boundary.
    pub warm_up_is_measured: bool,
}

/// A prepared output set controls group publication.
pub trait OutputStorage {
    type View<'a>
    where
        Self: 'a;
    type Pending<'a>: PendingOutputSet
    where
        Self: 'a;

    fn begin(&mut self) -> Self::Pending<'_>;
    fn view(&self) -> Self::View<'_>;
    fn discard(&mut self);
    fn observed_capacity(&self) -> usize;
    fn set_capacity_policy(&mut self, _policy: CapacityPolicy) {}
}

/// Pending writers can only be validated as a complete group by the executor.
pub trait PendingOutputSet {
    fn validate_complete(&self) -> Result<(), RunError>;
}

/// Writer for one typed fixed value.
pub struct ValueWriter<'a, T> {
    slot: &'a mut Option<T>,
    resource: &'static str,
    written: bool,
}

impl<T> ValueWriter<'_, T> {
    pub fn write(&mut self, value: T) {
        *self.slot = Some(value);
        self.written = true;
    }
}

impl<T> PendingOutputSet for ValueWriter<'_, T> {
    fn validate_complete(&self) -> Result<(), RunError> {
        if self.written {
            Ok(())
        } else {
            Err(RunError::IncompleteOutput {
                resource: self.resource,
            })
        }
    }
}

/// Prepared storage for a fixed typed value.
pub struct ValueStorage<T> {
    value: Option<T>,
    resource: &'static str,
}

impl<T> ValueStorage<T> {
    #[must_use]
    pub const fn new(resource: &'static str) -> Self {
        Self {
            value: None,
            resource,
        }
    }
}

impl<T> OutputStorage for ValueStorage<T> {
    type View<'a>
        = &'a T
    where
        T: 'a;
    type Pending<'a>
        = ValueWriter<'a, T>
    where
        T: 'a;

    fn begin(&mut self) -> Self::Pending<'_> {
        self.value = None;
        ValueWriter {
            slot: &mut self.value,
            resource: self.resource,
            written: false,
        }
    }

    fn view(&self) -> Self::View<'_> {
        self.value.as_ref().expect("validated output")
    }

    fn discard(&mut self) {
        self.value = None;
    }
    fn observed_capacity(&self) -> usize {
        usize::from(self.value.is_some())
    }
}

/// One atomic pending group containing two independently typed values.
pub struct PairStorage<A, B> {
    first: Option<A>,
    second: Option<B>,
    names: (&'static str, &'static str),
}

impl<A, B> PairStorage<A, B> {
    #[must_use]
    pub const fn new(first: &'static str, second: &'static str) -> Self {
        Self {
            first: None,
            second: None,
            names: (first, second),
        }
    }
}

/// Writers in this set have no publication operation; only the Module can
/// validate and expose the pair after both members are complete.
pub struct PairWriter<'a, A, B> {
    pub first: ValueWriter<'a, A>,
    pub second: ValueWriter<'a, B>,
}

impl<A, B> PendingOutputSet for PairWriter<'_, A, B> {
    fn validate_complete(&self) -> Result<(), RunError> {
        self.first.validate_complete()?;
        self.second.validate_complete()
    }
}

impl<A, B> OutputStorage for PairStorage<A, B> {
    type View<'a>
        = (&'a A, &'a B)
    where
        A: 'a,
        B: 'a;
    type Pending<'a>
        = PairWriter<'a, A, B>
    where
        A: 'a,
        B: 'a;

    fn begin(&mut self) -> Self::Pending<'_> {
        self.first = None;
        self.second = None;
        PairWriter {
            first: ValueWriter {
                slot: &mut self.first,
                resource: self.names.0,
                written: false,
            },
            second: ValueWriter {
                slot: &mut self.second,
                resource: self.names.1,
                written: false,
            },
        }
    }

    fn view(&self) -> Self::View<'_> {
        (
            self.first.as_ref().expect("validated output group"),
            self.second.as_ref().expect("validated output group"),
        )
    }

    fn discard(&mut self) {
        self.first = None;
        self.second = None;
    }
    fn observed_capacity(&self) -> usize {
        usize::from(self.first.is_some()) + usize::from(self.second.is_some())
    }
}

/// Fallible writer over a framework-prepared bounded buffer.
pub struct BoundedBufferWriter<'a, T> {
    values: &'a mut Vec<T>,
    resource: &'static str,
    prepared: usize,
    completed: bool,
    policy: CapacityPolicy,
}

impl<T> BoundedBufferWriter<'_, T> {
    pub fn try_push(&mut self, value: T) -> Result<(), CapacityError> {
        if self.values.len() == self.prepared && self.policy == CapacityPolicy::RejectOverflow {
            return Err(CapacityError {
                resource: self.resource,
                required: self.values.len() + 1,
                prepared: self.prepared,
                policy: self.policy,
            });
        }
        self.values.push(value);
        Ok(())
    }

    pub fn complete(&mut self) {
        self.completed = true;
    }
}

impl<T> PendingOutputSet for BoundedBufferWriter<'_, T> {
    fn validate_complete(&self) -> Result<(), RunError> {
        if self.completed {
            Ok(())
        } else {
            Err(RunError::IncompleteOutput {
                resource: self.resource,
            })
        }
    }
}

/// Prepared storage whose capacity never grows during `begin` or `try_push`.
pub struct BoundedStorage<T> {
    values: Vec<T>,
    resource: &'static str,
    capacity: usize,
    policy: CapacityPolicy,
}

/// Prepared fixed-length typed buffer. Initialization is tracked by the
/// vector's length, so only successfully initialized elements are dropped.
pub struct FixedBufferStorage<T> {
    values: Vec<T>,
    resource: &'static str,
    length: usize,
}

impl<T> FixedBufferStorage<T> {
    #[must_use]
    pub fn new(resource: &'static str, length: usize) -> Self {
        Self {
            values: Vec::with_capacity(length),
            resource,
            length,
        }
    }
}

pub struct FixedBufferWriter<'a, T> {
    values: &'a mut Vec<T>,
    resource: &'static str,
    required: usize,
}

impl<T> FixedBufferWriter<'_, T> {
    pub fn try_push(&mut self, value: T) -> Result<(), CapacityError> {
        if self.values.len() == self.required {
            return Err(CapacityError {
                resource: self.resource,
                required: self.values.len() + 1,
                prepared: self.required,
                policy: CapacityPolicy::RejectOverflow,
            });
        }
        self.values.push(value);
        Ok(())
    }
}

impl<T> PendingOutputSet for FixedBufferWriter<'_, T> {
    fn validate_complete(&self) -> Result<(), RunError> {
        if self.values.len() == self.required {
            Ok(())
        } else {
            Err(RunError::IncompleteOutput {
                resource: self.resource,
            })
        }
    }
}

impl<T> OutputStorage for FixedBufferStorage<T> {
    type View<'a>
        = &'a [T]
    where
        T: 'a;
    type Pending<'a>
        = FixedBufferWriter<'a, T>
    where
        T: 'a;

    fn begin(&mut self) -> Self::Pending<'_> {
        self.values.clear();
        FixedBufferWriter {
            values: &mut self.values,
            resource: self.resource,
            required: self.length,
        }
    }

    fn view(&self) -> Self::View<'_> {
        &self.values
    }
    fn discard(&mut self) {
        self.values.clear();
    }
    fn observed_capacity(&self) -> usize {
        self.values.len()
    }
}

impl<T> BoundedStorage<T> {
    #[must_use]
    pub fn new(resource: &'static str, capacity: usize) -> Self {
        Self {
            values: Vec::with_capacity(capacity),
            resource,
            capacity,
            policy: CapacityPolicy::RejectOverflow,
        }
    }
}

impl<T> OutputStorage for BoundedStorage<T> {
    type View<'a>
        = &'a [T]
    where
        T: 'a;
    type Pending<'a>
        = BoundedBufferWriter<'a, T>
    where
        T: 'a;

    fn begin(&mut self) -> Self::Pending<'_> {
        self.values.clear();
        BoundedBufferWriter {
            values: &mut self.values,
            resource: self.resource,
            prepared: self.capacity,
            completed: false,
            policy: self.policy,
        }
    }

    fn view(&self) -> Self::View<'_> {
        &self.values
    }

    fn discard(&mut self) {
        self.values.clear();
    }
    fn observed_capacity(&self) -> usize {
        self.values.len()
    }
    fn set_capacity_policy(&mut self, policy: CapacityPolicy) {
        self.policy = policy;
    }
}

/// Typed Unit contract; inputs and output writers require no Resource lookup.
pub trait Unit {
    type Input;
    type Storage: OutputStorage;

    fn workspace_requirement(&self) -> usize;
    fn output_storage(&self) -> Self::Storage;
    fn allocation_capability(&self) -> AllocationCapability;
    fn requirement_status(&self) -> RequirementStatus {
        RequirementStatus::Bounded
    }
    /// Validates host inputs before storage reset or Unit business logic.
    fn validate_input(&self, _input: &Self::Input) -> Result<(), RunError> {
        Ok(())
    }
    fn run(
        &mut self,
        input: &Self::Input,
        outputs: &mut <Self::Storage as OutputStorage>::Pending<'_>,
        workspace: UnitWorkspace<'_>,
    ) -> Result<(), RunError>;

    /// Runs with an optional framework-owned recorder for declared internal
    /// Unit boundaries. Atomic Units use the default implementation.
    fn run_with_unit_timing(
        &mut self,
        input: &Self::Input,
        outputs: &mut <Self::Storage as OutputStorage>::Pending<'_>,
        workspace: UnitWorkspace<'_>,
        _recorder: &mut UnitExecutionRecorder,
    ) -> Result<(), RunError> {
        self.run(input, outputs, workspace)
    }
}

/// Prepared synthetic Module with host-owned lifecycle.
#[doc(hidden)]
pub struct CompositeModule<U: Unit> {
    unit: U,
    storage: U::Storage,
    workspace: Vec<u8>,
    poisoned: bool,
    options: BuildOptions,
    description: PreparedModuleDescription,
    report: RunReport,
    reporting_enabled: bool,
}

impl<U: Unit> CompositeModule<U> {
    pub fn build(unit: U, options: BuildOptions) -> Result<Self, BuildError> {
        let capability = unit.allocation_capability();
        let requirement_status = unit.requirement_status();
        if options.allocation_guarantee == AllocationGuarantee::NoRunAllocation
            && matches!(
                requirement_status,
                RequirementStatus::Dynamic | RequirementStatus::Unresolved
            )
        {
            return Err(BuildError::StrictRequirementUnavailable(requirement_status));
        }
        if options.allocation_guarantee == AllocationGuarantee::NoRunAllocation
            && !capability.strict_capable()
        {
            return Err(BuildError::StrictCapabilityUnavailable(capability));
        }
        let workspace = vec![0; unit.workspace_requirement()];
        let mut storage = unit.output_storage();
        storage.set_capacity_policy(options.capacity_policy);
        Ok(Self {
            unit,
            storage,
            workspace,
            poisoned: false,
            options,
            description: PreparedModuleDescription {
                options,
                requirement_status,
                allocation_capability: capability,
                warm_up_is_measured: false,
            },
            report: RunReport::default(),
            reporting_enabled: true,
        })
    }

    /// Executes one invocation and returns a view borrowing prepared storage.
    /// The borrow statically prevents another mutable run.
    ///
    /// ```compile_fail
    /// use unit_compose_core::{BuildOptions, FixedImageFilter, ImageInput, Module};
    /// let mut module = CompositeModule::build(
    ///     FixedImageFilter { fail: None, panic: false },
    ///     BuildOptions::development(),
    /// ).unwrap();
    /// let output = module.run(&ImageInput { pixels: [1, 2, 3, 4] }).unwrap();
    /// let _second = module.run(&ImageInput { pixels: [5, 6, 7, 8] });
    /// assert_eq!(output.0, [1, 2, 3, 4]);
    /// ```
    pub fn run(
        &mut self,
        input: &U::Input,
    ) -> Result<<U::Storage as OutputStorage>::View<'_>, RunError> {
        let validate_probes =
            self.options.allocation_guarantee == AllocationGuarantee::NoRunAllocation;
        self.execute(input, &mut [], None, validate_probes)
    }

    /// Executes declared warm-up outside the steady-state allocation boundary.
    pub fn warm_up(
        &mut self,
        input: &U::Input,
    ) -> Result<<U::Storage as OutputStorage>::View<'_>, RunError> {
        self.execute(input, &mut [], None, false)
    }

    pub fn run_profiled(
        &mut self,
        input: &U::Input,
        probes: &mut [&mut dyn AllocationDomainProbe],
        sink: Option<&mut dyn DiagnosticSink>,
    ) -> Result<<U::Storage as OutputStorage>::View<'_>, RunError> {
        self.execute(input, probes, sink, true)
    }

    fn execute(
        &mut self,
        input: &U::Input,
        probes: &mut [&mut dyn AllocationDomainProbe],
        mut sink: Option<&mut dyn DiagnosticSink>,
        validate_probes: bool,
    ) -> Result<<U::Storage as OutputStorage>::View<'_>, RunError> {
        self.report.reset();
        if self.poisoned {
            return Err(RunError::Poisoned);
        }
        self.unit.validate_input(input)?;
        for domain in self
            .description
            .allocation_capability
            .domains()
            .iter()
            .filter(|domain| matches!(domain.evidence, AllocationEvidence::Instrumented))
        {
            if validate_probes && !probes.iter().any(|probe| probe.domain() == domain.name) {
                return Err(RunError::AllocationProfileViolation {
                    domain: domain.name.clone(),
                    operations: AllocationOperations::default(),
                });
            }
        }
        for probe in probes.iter_mut() {
            let declared = self
                .description
                .allocation_capability
                .domains()
                .iter()
                .any(|domain| {
                    domain.name == probe.domain()
                        && matches!(domain.evidence, AllocationEvidence::Instrumented)
                });
            if !declared {
                return Err(RunError::AllocationProfileViolation {
                    domain: probe.domain().to_owned(),
                    operations: AllocationOperations::default(),
                });
            }
        }
        for probe in probes.iter_mut() {
            probe.begin();
        }
        let mut pending = self.storage.begin();
        let started = Instant::now();
        let mut unit_timings = UnitExecutionRecorder::new(started, self.reporting_enabled);
        let result = catch_unwind(AssertUnwindSafe(|| {
            self.unit.run_with_unit_timing(
                input,
                &mut pending,
                UnitWorkspace {
                    bytes: &mut self.workspace,
                },
                &mut unit_timings,
            )
        }));
        let elapsed = started.elapsed();
        if self.reporting_enabled {
            for (target, event) in self
                .report
                .unit_timings
                .iter_mut()
                .zip(unit_timings.events().copied())
            {
                *target = Some(event);
                self.report.unit_timing_len += 1;
            }
            self.report.dropped_unit_timings = unit_timings.dropped_events;
        }
        let outcome = match result {
            Err(_) => {
                drop(pending);
                self.storage.discard();
                self.poisoned = true;
                Err(RunError::Panic)
            }
            Ok(Err(error)) => {
                drop(pending);
                self.storage.discard();
                if matches!(
                    error,
                    RunError::Unit(UnitFailure {
                        disposition: FailureDisposition::Fatal,
                        ..
                    })
                ) {
                    self.poisoned = true;
                }
                Err(error)
            }
            Ok(Ok(())) => {
                if let Err(error) = pending.validate_complete() {
                    drop(pending);
                    self.storage.discard();
                    Err(error)
                } else {
                    drop(pending);
                    Ok(())
                }
            }
        };
        let observed_capacity = match &outcome {
            Err(RunError::Capacity(error)) => error.prepared,
            _ => self.storage.observed_capacity(),
        };
        let kind = event_kind(&outcome);
        let nested_clock_reads = u8::try_from(unit_timings.len)
            .unwrap_or(u8::MAX)
            .saturating_mul(2);
        let event = RunEvent {
            kind,
            observed_capacity,
            elapsed,
            timing_scope: TimingScope::ModuleExecution,
            timing_overhead: TimingOverhead {
                clock_reads: 2_u8.saturating_add(nested_clock_reads),
                bounded_report_write_in_elapsed: unit_timings.len != 0,
            },
        };
        if self.reporting_enabled {
            self.report.push(event);
        }
        if let Some(sink) = sink.as_mut() {
            sink.record(event);
        }
        let mut violation = None;
        for probe in probes.iter_mut() {
            let operations = probe.finish();
            if self.reporting_enabled {
                self.report.allocation_operations.allocations += operations.allocations;
                self.report.allocation_operations.reallocations += operations.reallocations;
                self.report.allocation_operations.deallocations += operations.deallocations;
            }
            if violation.is_none() && !operations.is_zero() {
                violation = Some((probe.domain().to_owned(), operations));
            }
        }
        if let Some((domain, operations)) = violation {
            self.storage.discard();
            if self.reporting_enabled {
                self.report.push(RunEvent {
                    kind: RunEventKind::AllocationProfileViolation,
                    observed_capacity,
                    elapsed: Duration::ZERO,
                    timing_scope: TimingScope::ModuleExecution,
                    timing_overhead: TimingOverhead {
                        clock_reads: 0,
                        bounded_report_write_in_elapsed: false,
                    },
                });
            }
            return Err(RunError::AllocationProfileViolation { domain, operations });
        }
        outcome?;
        Ok(self.storage.view())
    }

    /// Validates the complete prepared host-input set before entering the
    /// ordinary run boundary, so rejection cannot reset outputs or invoke Unit code.
    pub fn run_checked(
        &mut self,
        plan: &PreparedInputPlan,
        supplied: &[ModuleInput],
        input: &U::Input,
    ) -> Result<<U::Storage as OutputStorage>::View<'_>, RunError> {
        plan.validate(supplied).map_err(RunError::Input)?;
        self.run(input)
    }

    /// Runs and copies a completely published view into caller-owned storage.
    /// The target is invalid from entry until this method returns success.
    pub fn run_into<T>(
        &mut self,
        input: &U::Input,
        target: &mut CallerOutput<T>,
    ) -> Result<(), RunError>
    where
        for<'a> <U::Storage as OutputStorage>::View<'a>: CopyInto<T>,
    {
        target.valid = false;
        let view = self.run(input)?;
        view.copy_into(&mut target.value);
        target.valid = true;
        Ok(())
    }

    #[must_use]
    pub const fn options(&self) -> BuildOptions {
        self.options
    }

    #[must_use]
    pub const fn description(&self) -> &PreparedModuleDescription {
        &self.description
    }

    #[must_use]
    pub const fn report(&self) -> &RunReport {
        &self.report
    }

    /// Enables or disables framework-owned report writes for subsequent runs.
    /// Diagnostic sinks remain independently controlled by the caller.
    pub fn set_reporting_enabled(&mut self, enabled: bool) {
        self.reporting_enabled = enabled;
    }

    #[must_use]
    pub const fn reporting_enabled(&self) -> bool {
        self.reporting_enabled
    }

    /// Returns immutable access to the prepared Unit's persistent state.
    #[must_use]
    pub const fn unit(&self) -> &U {
        &self.unit
    }
}

fn event_kind<T>(result: &Result<T, RunError>) -> RunEventKind {
    match result {
        Ok(_) => RunEventKind::Success,
        Err(RunError::Unit(UnitFailure {
            disposition: FailureDisposition::Recoverable,
            ..
        })) => RunEventKind::RecoverableFailure,
        Err(RunError::Unit(UnitFailure {
            disposition: FailureDisposition::Fatal,
            ..
        })) => RunEventKind::FatalFailure,
        Err(RunError::Capacity(_)) => RunEventKind::Overflow,
        Err(RunError::RuntimeOverflow { .. }) => RunEventKind::Overflow,
        Err(RunError::IncompleteOutput { .. }) => RunEventKind::IncompleteOutput,
        Err(RunError::Panic) => RunEventKind::Panic,
        Err(RunError::AllocationProfileViolation { .. }) => {
            RunEventKind::AllocationProfileViolation
        }
        Err(
            RunError::Poisoned
            | RunError::InvalidInput { .. }
            | RunError::Input(_)
            | RunError::RuntimeBinding { .. },
        ) => RunEventKind::RecoverableFailure,
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BuildError {
    StrictCapabilityUnavailable(AllocationCapability),
    StrictRequirementUnavailable(RequirementStatus),
    MissingConfiguration { unit: UnitId },
    Factory(FactoryError),
    RuntimePreparation { message: String },
    StoragePlanning(PlanningError),
}

/// Frontend-neutral, fully decoded input to the canonical Module builder.
pub struct ExecutableDefinition {
    graph: CompiledGraph,
    configurations: BTreeMap<UnitId, DecodedConfiguration>,
    requirements: BTreeMap<ResourceId, ResourceRequirement>,
    workspace_bytes: BTreeMap<UnitId, usize>,
}

impl ExecutableDefinition {
    #[must_use]
    pub fn new(
        graph: CompiledGraph,
        configurations: BTreeMap<UnitId, DecodedConfiguration>,
        requirements: BTreeMap<ResourceId, ResourceRequirement>,
        workspace_bytes: BTreeMap<UnitId, usize>,
    ) -> Self {
        Self {
            graph,
            configurations,
            requirements,
            workspace_bytes,
        }
    }
}

/// Caller storage has an explicit validity bit because failures do not roll bytes back.
pub struct CallerOutput<T> {
    value: T,
    valid: bool,
}

impl<T> CallerOutput<T> {
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

    #[must_use]
    pub const fn raw(&self) -> &T {
        &self.value
    }
}

pub trait CopyInto<T> {
    fn copy_into(self, target: &mut T);
}

impl<T: Clone> CopyInto<T> for &T {
    fn copy_into(self, target: &mut T) {
        target.clone_from(self);
    }
}

impl<A: Clone, B: Clone> CopyInto<(A, B)> for (&A, &B) {
    fn copy_into(self, target: &mut (A, B)) {
        target.0.clone_from(self.0);
        target.1.clone_from(self.1);
    }
}

/// Synthetic fixed-size image input.
pub struct ImageInput {
    pub pixels: [u8; 4],
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Image(pub [u8; 4]);

pub struct FixedImageFilter {
    pub fail: Option<FailureDisposition>,
    pub panic: bool,
}

impl Unit for FixedImageFilter {
    type Input = ImageInput;
    type Storage = ValueStorage<Image>;

    fn workspace_requirement(&self) -> usize {
        0
    }
    fn output_storage(&self) -> Self::Storage {
        ValueStorage::new("filtered_image")
    }
    fn allocation_capability(&self) -> AllocationCapability {
        strict_global_allocator_capability()
    }
    fn run(
        &mut self,
        input: &Self::Input,
        output: &mut ValueWriter<'_, Image>,
        _: UnitWorkspace<'_>,
    ) -> Result<(), RunError> {
        output.write(Image(input.pixels));
        assert!(!self.panic, "synthetic unwind");
        if let Some(disposition) = self.fail {
            return Err(RunError::Unit(UnitFailure {
                disposition,
                message: "synthetic failure",
            }));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Point(pub i32, pub i32);

pub struct PointInput {
    pub points: Vec<Point>,
}

pub struct BoundedPointFilter {
    pub maximum: usize,
}

impl Unit for BoundedPointFilter {
    type Input = PointInput;
    type Storage = BoundedStorage<Point>;

    fn workspace_requirement(&self) -> usize {
        0
    }
    fn output_storage(&self) -> Self::Storage {
        BoundedStorage::new("filtered_points", self.maximum)
    }
    fn allocation_capability(&self) -> AllocationCapability {
        strict_global_allocator_capability()
    }
    fn run(
        &mut self,
        input: &Self::Input,
        output: &mut BoundedBufferWriter<'_, Point>,
        _: UnitWorkspace<'_>,
    ) -> Result<(), RunError> {
        for point in input.points.iter().copied().filter(|point| point.0 >= 0) {
            output.try_push(point).map_err(RunError::Capacity)?;
        }
        output.complete();
        Ok(())
    }
}

pub struct PlannerInput {
    pub seed: u8,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Plan(pub u64);

pub struct WorkspaceHeavyPlanner {
    pub workspace_bytes: usize,
}

impl Unit for WorkspaceHeavyPlanner {
    type Input = PlannerInput;
    type Storage = ValueStorage<Plan>;

    fn workspace_requirement(&self) -> usize {
        self.workspace_bytes
    }
    fn output_storage(&self) -> Self::Storage {
        ValueStorage::new("plan")
    }
    fn allocation_capability(&self) -> AllocationCapability {
        strict_global_allocator_capability()
    }
    fn run(
        &mut self,
        input: &Self::Input,
        output: &mut ValueWriter<'_, Plan>,
        mut workspace: UnitWorkspace<'_>,
    ) -> Result<(), RunError> {
        workspace.bytes().fill(input.seed);
        output.write(Plan(
            workspace
                .bytes()
                .iter()
                .map(|value| u64::from(*value))
                .sum(),
        ));
        Ok(())
    }
}

fn strict_global_allocator_capability() -> AllocationCapability {
    AllocationCapability::inspect(
        vec![AllocationDomain {
            name: "rust-global".into(),
            evidence: AllocationEvidence::Instrumented,
        }],
        true,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
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

    struct WritesThenFails(Arc<AtomicUsize>);

    struct WritesSuccessfully(Arc<AtomicUsize>);

    impl Unit for WritesSuccessfully {
        type Input = ();
        type Storage = ValueStorage<DropProbe>;
        fn workspace_requirement(&self) -> usize {
            0
        }
        fn output_storage(&self) -> Self::Storage {
            ValueStorage::new("probe")
        }
        fn allocation_capability(&self) -> AllocationCapability {
            strict_global_allocator_capability()
        }
        fn run(
            &mut self,
            _: &(),
            output: &mut ValueWriter<'_, DropProbe>,
            _: UnitWorkspace<'_>,
        ) -> Result<(), RunError> {
            output.write(DropProbe(Arc::clone(&self.0)));
            Ok(())
        }
    }

    impl Unit for WritesThenFails {
        type Input = ();
        type Storage = ValueStorage<DropProbe>;

        fn workspace_requirement(&self) -> usize {
            0
        }
        fn output_storage(&self) -> Self::Storage {
            ValueStorage::new("probe")
        }
        fn allocation_capability(&self) -> AllocationCapability {
            strict_global_allocator_capability()
        }
        fn run(
            &mut self,
            _: &(),
            output: &mut ValueWriter<'_, DropProbe>,
            _: UnitWorkspace<'_>,
        ) -> Result<(), RunError> {
            output.write(DropProbe(Arc::clone(&self.0)));
            Err(RunError::Unit(UnitFailure::recoverable(
                "after initialization",
            )))
        }
    }

    struct WritesPartialGroup;

    impl Unit for WritesPartialGroup {
        type Input = ();
        type Storage = PairStorage<u32, u64>;

        fn workspace_requirement(&self) -> usize {
            0
        }
        fn output_storage(&self) -> Self::Storage {
            PairStorage::new("first", "second")
        }
        fn allocation_capability(&self) -> AllocationCapability {
            strict_global_allocator_capability()
        }
        fn run(
            &mut self,
            _: &(),
            output: &mut PairWriter<'_, u32, u64>,
            _: UnitWorkspace<'_>,
        ) -> Result<(), RunError> {
            output.first.write(7);
            Ok(())
        }
    }

    struct WritesProbeThenPanics(Arc<AtomicUsize>);

    impl Unit for WritesProbeThenPanics {
        type Input = ();
        type Storage = ValueStorage<DropProbe>;

        fn workspace_requirement(&self) -> usize {
            0
        }
        fn output_storage(&self) -> Self::Storage {
            ValueStorage::new("probe")
        }
        fn allocation_capability(&self) -> AllocationCapability {
            strict_global_allocator_capability()
        }
        fn run(
            &mut self,
            _: &(),
            output: &mut ValueWriter<'_, DropProbe>,
            _: UnitWorkspace<'_>,
        ) -> Result<(), RunError> {
            output.write(DropProbe(Arc::clone(&self.0)));
            panic!("after initialization");
        }
    }

    struct WritesIncompleteProbeGroup(Arc<AtomicUsize>);

    impl Unit for WritesIncompleteProbeGroup {
        type Input = ();
        type Storage = PairStorage<DropProbe, u64>;

        fn workspace_requirement(&self) -> usize {
            0
        }
        fn output_storage(&self) -> Self::Storage {
            PairStorage::new("probe", "missing")
        }
        fn allocation_capability(&self) -> AllocationCapability {
            strict_global_allocator_capability()
        }
        fn run(
            &mut self,
            _: &(),
            output: &mut PairWriter<'_, DropProbe, u64>,
            _: UnitWorkspace<'_>,
        ) -> Result<(), RunError> {
            output.first.write(DropProbe(Arc::clone(&self.0)));
            Ok(())
        }
    }

    struct ValidatesBeforeMutation {
        runs: Arc<AtomicUsize>,
        accept: bool,
    }

    impl Unit for ValidatesBeforeMutation {
        type Input = ();
        type Storage = ValueStorage<DropProbe>;

        fn workspace_requirement(&self) -> usize {
            0
        }
        fn output_storage(&self) -> Self::Storage {
            ValueStorage::new("validated")
        }
        fn allocation_capability(&self) -> AllocationCapability {
            strict_global_allocator_capability()
        }
        fn validate_input(&self, _: &()) -> Result<(), RunError> {
            if self.accept {
                Ok(())
            } else {
                Err(RunError::InvalidInput {
                    message: "rejected",
                })
            }
        }
        fn run(
            &mut self,
            _: &(),
            output: &mut ValueWriter<'_, DropProbe>,
            _: UnitWorkspace<'_>,
        ) -> Result<(), RunError> {
            self.runs.fetch_add(1, Ordering::SeqCst);
            output.write(DropProbe(Arc::clone(&self.runs)));
            Ok(())
        }
    }

    struct MutatesThenPanics {
        mutations: usize,
        drops: Arc<AtomicUsize>,
    }

    impl Unit for MutatesThenPanics {
        type Input = ();
        type Storage = ValueStorage<DropProbe>;

        fn workspace_requirement(&self) -> usize {
            0
        }
        fn output_storage(&self) -> Self::Storage {
            ValueStorage::new("panic")
        }
        fn allocation_capability(&self) -> AllocationCapability {
            strict_global_allocator_capability()
        }
        fn run(
            &mut self,
            _: &(),
            output: &mut ValueWriter<'_, DropProbe>,
            _: UnitWorkspace<'_>,
        ) -> Result<(), RunError> {
            self.mutations += 1;
            output.write(DropProbe(Arc::clone(&self.drops)));
            panic!("after private mutation")
        }
    }

    fn fixed() -> CompositeModule<FixedImageFilter> {
        CompositeModule::build(
            FixedImageFilter {
                fail: None,
                panic: false,
            },
            BuildOptions::development(),
        )
        .unwrap()
    }

    #[test]
    fn descriptor_is_semantic_and_owns_representation_invariants() {
        let semantic = SemanticType::new("image.Gray8/v1").unwrap();
        let descriptor =
            ResourceDescriptor::of::<Image>(semantic.clone(), "fixed-value", "exactly four pixels");
        let mut registry = ResourceRegistry::default();
        registry.register(descriptor).unwrap();
        let resolved = registry.get(&semantic).unwrap();
        assert!(resolved.represents::<Image>());
        assert_eq!(resolved.invariants().element_size, 4);
        assert_eq!(resolved.invariants().adapter, "fixed-value");
    }

    #[test]
    fn build_options_reject_false_strict_claim() {
        assert_eq!(
            BuildOptions::try_new(
                CapacityPolicy::GrowAndMeasure,
                AllocationGuarantee::NoRunAllocation
            ),
            Err(BuildOptionError::GrowthWithNoRunAllocation)
        );
    }

    #[test]
    fn allocation_trust_is_inspectable_and_explicit() {
        let capability = AllocationCapability::inspect(
            vec![AllocationDomain {
                name: "native".into(),
                evidence: AllocationEvidence::Certified {
                    source: "integrator review 7".into(),
                },
            }],
            true,
        );
        assert!(capability.strict_capable());
        assert!(capability.declarations_are_trusted());
        assert!(matches!(
            capability.domains()[0].evidence,
            AllocationEvidence::Certified { .. }
        ));
        let unsupported = AllocationCapability::inspect(
            vec![AllocationDomain {
                name: "device".into(),
                evidence: AllocationEvidence::Unsupported,
            }],
            true,
        );
        assert!(!unsupported.strict_capable());
        let unnamed_certification = AllocationCapability::inspect(
            vec![AllocationDomain {
                name: "native".into(),
                evidence: AllocationEvidence::Certified { source: "".into() },
            }],
            true,
        );
        assert!(!unnamed_certification.strict_capable());
    }

    #[test]
    fn fixed_output_is_borrowed_and_storage_resets_each_run() {
        let mut module = fixed();
        let first = [1, 2, 3, 4];
        assert_eq!(
            module.run(&ImageInput { pixels: first }).unwrap(),
            &Image(first)
        );
        let second = [5, 6, 7, 8];
        assert_eq!(
            module.run(&ImageInput { pixels: second }).unwrap(),
            &Image(second)
        );
    }

    #[test]
    fn description_and_bounded_report_expose_allocation_contract() {
        let mut module = fixed();
        assert_eq!(
            module.description().requirement_status,
            RequirementStatus::Bounded
        );
        assert!(!module.description().warm_up_is_measured);
        assert_eq!(
            module.description().allocation_capability.domains()[0].name,
            "rust-global"
        );
        let _ = module.run(&ImageInput { pixels: [1; 4] }).unwrap();
        assert_eq!(module.report().observed_capacity_peak(), 1);
        assert_eq!(module.report().events().count(), 1);
        assert_eq!(module.report().dropped_events(), 0);
        let snapshot = module.report().snapshot();
        let event = snapshot.events().next().unwrap();
        assert_eq!(event.timing_scope, TimingScope::ModuleExecution);
        assert_eq!(event.timing_overhead.clock_reads, 2);
        assert!(!event.timing_overhead.bounded_report_write_in_elapsed);
        assert_eq!(snapshot.observed_capacity_peak(), 1);
        assert_eq!(
            snapshot.allocation_operations(),
            AllocationOperations::default()
        );

        module.set_reporting_enabled(false);
        assert!(!module.reporting_enabled());
        let _ = module.run(&ImageInput { pixels: [2; 4] }).unwrap();
        assert_eq!(module.report().events().count(), 0);
        assert_eq!(module.report().observed_capacity_peak(), 0);
        assert_eq!(
            module.report().allocation_operations(),
            AllocationOperations::default()
        );
    }

    #[test]
    fn bounded_writer_rejects_overflow_without_growth() {
        let mut storage = BoundedStorage::new("points", 1);
        let mut writer = storage.begin();
        writer.try_push(Point(1, 1)).unwrap();
        assert_eq!(
            writer.try_push(Point(2, 2)),
            Err(CapacityError {
                resource: "points",
                required: 2,
                prepared: 1,
                policy: CapacityPolicy::RejectOverflow
            })
        );
    }

    #[test]
    fn development_capacity_policy_grows_and_reports_observed_peak() {
        let mut module = CompositeModule::build(
            BoundedPointFilter { maximum: 1 },
            BuildOptions::development(),
        )
        .unwrap();
        let output = module
            .run(&PointInput {
                points: vec![Point(1, 1), Point(2, 2)],
            })
            .unwrap();
        assert_eq!(output.len(), 2);
        assert_eq!(module.report().observed_capacity_peak(), 2);
    }

    #[test]
    fn incomplete_output_set_is_not_published() {
        let mut storage = ValueStorage::<Image>::new("image");
        let writer = storage.begin();
        assert_eq!(
            writer.validate_complete(),
            Err(RunError::IncompleteOutput { resource: "image" })
        );
    }

    #[test]
    fn complete_group_validation_discards_initialized_siblings() {
        let mut module =
            CompositeModule::build(WritesPartialGroup, BuildOptions::development()).unwrap();
        assert_eq!(
            module.run(&()),
            Err(RunError::IncompleteOutput { resource: "second" })
        );
        assert!(module.storage.first.is_none());
        assert!(module.storage.second.is_none());
    }

    #[test]
    fn module_preserves_structured_capacity_overflow() {
        let options = BuildOptions::try_new(
            CapacityPolicy::RejectOverflow,
            AllocationGuarantee::BestEffort,
        )
        .unwrap();
        let mut module =
            CompositeModule::build(BoundedPointFilter { maximum: 1 }, options).unwrap();
        assert_eq!(
            module.run(&PointInput {
                points: vec![Point(1, 1), Point(2, 2)]
            }),
            Err(RunError::Capacity(CapacityError {
                resource: "filtered_points",
                required: 2,
                prepared: 1,
                policy: CapacityPolicy::RejectOverflow
            }))
        );
    }

    #[test]
    fn initialized_pending_value_is_dropped_immediately_on_error() {
        let drops = Arc::new(AtomicUsize::new(0));
        let mut module = CompositeModule::build(
            WritesThenFails(Arc::clone(&drops)),
            BuildOptions::development(),
        )
        .unwrap();
        assert!(matches!(module.run(&()), Err(RunError::Unit(_))));
        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn initialized_pending_values_drop_on_validation_error_and_unwind() {
        let validation_drops = Arc::new(AtomicUsize::new(0));
        let mut incomplete = CompositeModule::build(
            WritesIncompleteProbeGroup(Arc::clone(&validation_drops)),
            BuildOptions::development(),
        )
        .unwrap();
        assert!(matches!(
            incomplete.run(&()),
            Err(RunError::IncompleteOutput {
                resource: "missing"
            })
        ));
        assert_eq!(validation_drops.load(Ordering::SeqCst), 1);

        let panic_drops = Arc::new(AtomicUsize::new(0));
        let mut panics = CompositeModule::build(
            WritesProbeThenPanics(Arc::clone(&panic_drops)),
            BuildOptions::development(),
        )
        .unwrap();
        assert!(matches!(panics.run(&()), Err(RunError::Panic)));
        assert_eq!(panic_drops.load(Ordering::SeqCst), 1);
        assert!(matches!(panics.run(&()), Err(RunError::Poisoned)));
    }

    #[test]
    fn recoverable_failure_allows_another_run_and_invalidates_run_into() {
        let mut module = CompositeModule::build(
            FixedImageFilter {
                fail: Some(FailureDisposition::Recoverable),
                panic: false,
            },
            BuildOptions::development(),
        )
        .unwrap();
        let pixels = [1, 2, 3, 4];
        let mut target = CallerOutput::new(Image([9; 4]));
        assert!(matches!(
            module.run_into(&ImageInput { pixels }, &mut target),
            Err(RunError::Unit(_))
        ));
        assert!(!target.is_valid());
        assert_eq!(target.raw(), &Image([9; 4]));
        module.unit.fail = None;
        module
            .run_into(&ImageInput { pixels }, &mut target)
            .unwrap();
        assert_eq!(target.get(), Some(&Image(pixels)));
    }

    #[test]
    fn run_into_invalidates_caller_storage_on_all_failure_paths() {
        let mut incomplete =
            CompositeModule::build(WritesPartialGroup, BuildOptions::development()).unwrap();
        let mut pair_target = CallerOutput::new((9u32, 11u64));
        assert!(matches!(
            incomplete.run_into(&(), &mut pair_target),
            Err(RunError::IncompleteOutput { resource: "second" })
        ));
        assert!(!pair_target.is_valid());
        assert_eq!(pair_target.raw(), &(9, 11));

        let mut panics = CompositeModule::build(
            FixedImageFilter {
                fail: None,
                panic: true,
            },
            BuildOptions::development(),
        )
        .unwrap();
        let mut image_target = CallerOutput::new(Image([9; 4]));
        assert_eq!(
            panics.run_into(&ImageInput { pixels: [1; 4] }, &mut image_target),
            Err(RunError::Panic)
        );
        assert!(!image_target.is_valid());
        assert_eq!(image_target.raw(), &Image([9; 4]));
        assert!(matches!(
            panics.run_into(&ImageInput { pixels: [1; 4] }, &mut image_target),
            Err(RunError::Poisoned)
        ));
    }

    #[test]
    fn fatal_failure_and_unwind_poison_module() {
        let pixels = [1, 2, 3, 4];
        let mut fatal = CompositeModule::build(
            FixedImageFilter {
                fail: Some(FailureDisposition::Fatal),
                panic: false,
            },
            BuildOptions::development(),
        )
        .unwrap();
        assert!(matches!(
            fatal.run(&ImageInput { pixels }),
            Err(RunError::Unit(_))
        ));
        assert_eq!(fatal.run(&ImageInput { pixels }), Err(RunError::Poisoned));

        let mut panics = CompositeModule::build(
            FixedImageFilter {
                fail: None,
                panic: true,
            },
            BuildOptions::development(),
        )
        .unwrap();
        assert_eq!(panics.run(&ImageInput { pixels }), Err(RunError::Panic));
        assert_eq!(panics.run(&ImageInput { pixels }), Err(RunError::Poisoned));
    }

    #[test]
    fn synthetic_bounded_and_workspace_units_use_prepared_storage() {
        let options = BuildOptions::try_new(
            CapacityPolicy::RejectOverflow,
            AllocationGuarantee::BestEffort,
        )
        .unwrap();
        let mut filter =
            CompositeModule::build(BoundedPointFilter { maximum: 2 }, options).unwrap();
        assert_eq!(
            filter
                .run(&PointInput {
                    points: vec![Point(-1, 0), Point(2, 3)]
                })
                .unwrap(),
            &[Point(2, 3)]
        );

        let mut planner = CompositeModule::build(
            WorkspaceHeavyPlanner {
                workspace_bytes: 128,
            },
            options,
        )
        .unwrap();
        assert_eq!(planner.run(&PlannerInput { seed: 2 }).unwrap(), &Plan(256));
    }

    #[test]
    fn successful_publication_drops_once_on_next_reset() {
        let drops = Arc::new(AtomicUsize::new(0));
        let mut module = CompositeModule::build(
            WritesSuccessfully(Arc::clone(&drops)),
            BuildOptions::development(),
        )
        .unwrap();
        let _ = module.run(&()).unwrap();
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        let _ = module.run(&()).unwrap();
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        drop(module);
        assert_eq!(drops.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn invalid_input_precedes_reset_and_business_logic_and_is_reusable() {
        let counter = Arc::new(AtomicUsize::new(0));
        let mut module = CompositeModule::build(
            ValidatesBeforeMutation {
                runs: Arc::clone(&counter),
                accept: true,
            },
            BuildOptions::development(),
        )
        .unwrap();
        {
            let _borrow = module.run(&()).unwrap();
        }
        module.unit.accept = false;
        assert!(matches!(
            module.run(&()),
            Err(RunError::InvalidInput {
                message: "rejected"
            })
        ));
        assert_eq!(counter.load(Ordering::SeqCst), 1);
        assert!(module.storage.value.is_some());
        module.unit.accept = true;
        assert!(module.run(&()).is_ok());
    }

    #[test]
    fn prepared_input_validation_checks_full_contract_before_run() {
        let semantic = SemanticType::new("test.Input/v1").unwrap();
        let resource = ResourceId::new("source");
        let plan = PreparedInputPlan::new([PreparedInputSpec::of::<u32>(
            resource.clone(),
            semantic.clone(),
            4,
            17,
        )])
        .unwrap();
        let counter = Arc::new(AtomicUsize::new(0));
        let mut module = CompositeModule::build(
            ValidatesBeforeMutation {
                runs: Arc::clone(&counter),
                accept: true,
            },
            BuildOptions::development(),
        )
        .unwrap();
        let bad = [ModuleInput::of::<u64>(
            resource.clone(),
            semantic.clone(),
            4,
            17,
        )];
        assert!(matches!(
            module.run_checked(&plan, &bad, &()),
            Err(RunError::Input(InputValidationError::ConcreteType { .. }))
        ));
        assert_eq!(counter.load(Ordering::SeqCst), 0);
        assert!(module.storage.value.is_none());

        let good = [ModuleInput::of::<u32>(resource, semantic, 4, 17)];
        assert!(module.run_checked(&plan, &good, &()).is_ok());
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn prepared_input_plan_rejects_duplicate_names() {
        let semantic = SemanticType::new("test.Input/v1").unwrap();
        let resource = ResourceId::new("source");
        assert!(matches!(
            PreparedInputPlan::new([
                PreparedInputSpec::of::<u32>(resource.clone(), semantic.clone(), 4, 17),
                PreparedInputSpec::of::<u32>(resource.clone(), semantic, 4, 17),
            ]),
            Err(InputValidationError::DuplicatePrepared { resource: duplicate })
                if duplicate == resource
        ));
    }

    #[test]
    fn unwind_after_private_mutation_drops_pending_and_poisons() {
        let drops = Arc::new(AtomicUsize::new(0));
        let mut module = CompositeModule::build(
            MutatesThenPanics {
                mutations: 0,
                drops: Arc::clone(&drops),
            },
            BuildOptions::development(),
        )
        .unwrap();
        assert!(matches!(module.run(&()), Err(RunError::Panic)));
        assert_eq!(module.unit.mutations, 1);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        assert!(matches!(module.run(&()), Err(RunError::Poisoned)));
    }
}
