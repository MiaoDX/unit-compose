//! UnitCompose core contracts.
//!
//! The crate contains the typed execution kernel, normalized graph compiler,
//! conservative typed storage planner, strict allocation contracts, and fixed
//! inspection model. YAML parsing and application adapters remain separate
//! crates so core behavior has no frontend or framework dependency.

mod graph;
mod inspection;
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
    HostOutput, InputBindingError, InputBuffer, InputValue, Module, ModuleInputs,
    RegistrationInvocation,
};
pub use storage::{
    InputValidationError, LiveRange, ModuleInput, PlanningError, PreparedInputPlan,
    PreparedInputSpec, ResourceRequirement, SlotAssignment, StoragePlan, StorageReport,
    calculate_live_ranges, plan_storage,
};

use std::any::{TypeId, type_name};
use std::collections::{BTreeMap, btree_map::Entry};
use std::fmt;
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunErrorContext {
    pub module: String,
    pub unit: Option<UnitId>,
    pub unit_type: Option<UnitTypeName>,
    pub port: Option<String>,
    pub resource: Option<ResourceId>,
    pub disposition: Option<FailureDisposition>,
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
    Execution {
        context: Box<RunErrorContext>,
        cause: Box<RunError>,
    },
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

impl RunError {
    #[must_use]
    pub fn root_cause(&self) -> &Self {
        match self {
            Self::Execution { cause, .. } => cause.root_cause(),
            error => error,
        }
    }

    #[must_use]
    pub fn context(&self) -> Option<&RunErrorContext> {
        match self {
            Self::Execution { context, .. } => Some(context.as_ref()),
            _ => None,
        }
    }
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

/// One measured Unit execution boundary in compiled order.
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

/// Framework-owned bounded recorder used by the dynamic executor.
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

fn event_kind<T>(result: &Result<T, RunError>) -> RunEventKind {
    match result {
        Ok(_) => RunEventKind::Success,
        Err(error) => error_event_kind(error),
    }
}

fn error_event_kind(error: &RunError) -> RunEventKind {
    match error {
        RunError::Execution { cause, .. } => error_event_kind(cause),
        RunError::Unit(UnitFailure {
            disposition: FailureDisposition::Recoverable,
            ..
        }) => RunEventKind::RecoverableFailure,
        RunError::Unit(UnitFailure {
            disposition: FailureDisposition::Fatal,
            ..
        }) => RunEventKind::FatalFailure,
        RunError::Capacity(_) | RunError::RuntimeOverflow { .. } => RunEventKind::Overflow,
        RunError::IncompleteOutput { .. } => RunEventKind::IncompleteOutput,
        RunError::Panic => RunEventKind::Panic,
        RunError::AllocationProfileViolation { .. } => RunEventKind::AllocationProfileViolation,
        RunError::Poisoned
        | RunError::InvalidInput { .. }
        | RunError::Input(_)
        | RunError::RuntimeBinding { .. } => RunEventKind::RecoverableFailure,
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
