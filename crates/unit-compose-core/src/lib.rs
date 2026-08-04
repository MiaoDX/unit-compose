//! UnitCompose core contracts.
//!
//! The crate contains the typed Milestone 0 execution kernel and the fixed,
//! storage-independent Milestone 1 graph compiler. YAML frontends, storage-slot
//! planning, and application adapters live outside these APIs.

mod graph;

pub use graph::{
    CompileError, CompiledGraph, ConcreteType, ModuleDescription, ParsedModule, ParsedModuleInput,
    ParsedUnit, PortDescriptor, ResolvedBinding, ResolvedModule, ResolvedModuleInput, ResolvedUnit,
    ResourceId, UnitDescriptor, UnitId, UnitRegistry, UnitTypeName,
};

use std::any::{TypeId, type_name};
use std::collections::{BTreeMap, btree_map::Entry};
use std::fmt;
use std::marker::PhantomData;
use std::panic::{AssertUnwindSafe, catch_unwind};

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

/// Storage class is a representation invariant, never a Unit choice.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryClass {
    /// Ordinary process memory.
    Host,
}

/// Complete representation authority for one semantic Resource type.
#[derive(Clone, Debug)]
pub struct ResourceDescriptor {
    semantic_type: SemanticType,
    concrete_type: TypeId,
    concrete_name: &'static str,
    element_size: usize,
    element_alignment: usize,
    memory_class: MemoryClass,
    adapter: &'static str,
    reset: &'static str,
    validation: &'static str,
    drop_behavior: &'static str,
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
            memory_class: MemoryClass::Host,
            adapter,
            reset: "drop published value before the next run",
            validation,
            drop_behavior: "Rust Drop",
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
            memory_class: self.memory_class,
            adapter: self.adapter,
            reset: self.reset,
            validation: self.validation,
            drop_behavior: self.drop_behavior,
        }
    }
}

/// Read-only descriptor details used by build inspection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RepresentationInvariants<'a> {
    pub concrete_name: &'static str,
    pub element_size: usize,
    pub element_alignment: usize,
    pub memory_class: MemoryClass,
    pub adapter: &'a str,
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
            && domains
                .iter()
                .all(|domain| !matches!(domain.evidence, AllocationEvidence::Unsupported));
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
    bytes: &'a mut [u8],
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
    IncompleteOutput { resource: &'static str },
    Panic,
    Poisoned,
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
}

/// Fallible writer over a framework-prepared bounded buffer.
pub struct BoundedBufferWriter<'a, T> {
    values: &'a mut Vec<T>,
    resource: &'static str,
    prepared: usize,
    completed: bool,
}

impl<T> BoundedBufferWriter<'_, T> {
    pub fn try_push(&mut self, value: T) -> Result<(), CapacityError> {
        if self.values.len() == self.prepared {
            return Err(CapacityError {
                resource: self.resource,
                required: self.values.len() + 1,
                prepared: self.prepared,
                policy: CapacityPolicy::RejectOverflow,
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
}

impl<T> BoundedStorage<T> {
    #[must_use]
    pub fn new(resource: &'static str, capacity: usize) -> Self {
        Self {
            values: Vec::with_capacity(capacity),
            resource,
            capacity,
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
        }
    }

    fn view(&self) -> Self::View<'_> {
        &self.values
    }

    fn discard(&mut self) {
        self.values.clear();
    }
}

/// Typed Unit contract; inputs and output writers require no Resource lookup.
pub trait Unit {
    type Input;
    type Storage: OutputStorage;

    fn workspace_requirement(&self) -> usize;
    fn output_storage(&self) -> Self::Storage;
    fn allocation_capability(&self) -> AllocationCapability;
    fn run(
        &mut self,
        input: &Self::Input,
        outputs: &mut <Self::Storage as OutputStorage>::Pending<'_>,
        workspace: UnitWorkspace<'_>,
    ) -> Result<(), RunError>;
}

/// Prepared synthetic Module with host-owned lifecycle.
pub struct Module<U: Unit> {
    unit: U,
    storage: U::Storage,
    workspace: Vec<u8>,
    poisoned: bool,
    options: BuildOptions,
}

impl<U: Unit> Module<U> {
    pub fn build(unit: U, options: BuildOptions) -> Result<Self, BuildError> {
        let capability = unit.allocation_capability();
        if options.allocation_guarantee == AllocationGuarantee::NoRunAllocation
            && !capability.strict_capable()
        {
            return Err(BuildError::StrictCapabilityUnavailable(capability));
        }
        let workspace = vec![0; unit.workspace_requirement()];
        let storage = unit.output_storage();
        Ok(Self {
            unit,
            storage,
            workspace,
            poisoned: false,
            options,
        })
    }

    pub fn run(
        &mut self,
        input: &U::Input,
    ) -> Result<<U::Storage as OutputStorage>::View<'_>, RunError> {
        if self.poisoned {
            return Err(RunError::Poisoned);
        }
        let mut pending = self.storage.begin();
        let result = catch_unwind(AssertUnwindSafe(|| {
            self.unit.run(
                input,
                &mut pending,
                UnitWorkspace {
                    bytes: &mut self.workspace,
                },
            )
        }));
        match result {
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
                    return Err(error);
                }
                drop(pending);
                Ok(self.storage.view())
            }
        }
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
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BuildError {
    StrictCapabilityUnavailable(AllocationCapability),
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

/// Synthetic fixed-size image input.
pub struct ImageInput {
    pub pixels: [u8; 4],
}

#[derive(Clone, Debug, Eq, PartialEq)]
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

/// Marker used to document that typed inputs borrow host data.
pub struct BorrowedInput<'a, T> {
    value: &'a T,
    marker: PhantomData<&'a T>,
}

impl<'a, T> BorrowedInput<'a, T> {
    #[must_use]
    pub const fn new(value: &'a T) -> Self {
        Self {
            value,
            marker: PhantomData,
        }
    }
    #[must_use]
    pub const fn get(&self) -> &'a T {
        self.value
    }
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

    fn fixed() -> Module<FixedImageFilter> {
        Module::build(
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
        let mut module = Module::build(WritesPartialGroup, BuildOptions::development()).unwrap();
        assert_eq!(
            module.run(&()),
            Err(RunError::IncompleteOutput { resource: "second" })
        );
        assert!(module.storage.first.is_none());
        assert!(module.storage.second.is_none());
    }

    #[test]
    fn module_preserves_structured_capacity_overflow() {
        let mut module =
            Module::build(BoundedPointFilter { maximum: 1 }, BuildOptions::strict()).unwrap();
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
        let mut module = Module::build(
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
        let mut incomplete = Module::build(
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
        let mut panics = Module::build(
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
        let mut module = Module::build(
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
    fn fatal_failure_and_unwind_poison_module() {
        let pixels = [1, 2, 3, 4];
        let mut fatal = Module::build(
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

        let mut panics = Module::build(
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
        let mut filter =
            Module::build(BoundedPointFilter { maximum: 2 }, BuildOptions::strict()).unwrap();
        assert_eq!(
            filter
                .run(&PointInput {
                    points: vec![Point(-1, 0), Point(2, 3)]
                })
                .unwrap(),
            &[Point(2, 3)]
        );

        let mut planner = Module::build(
            WorkspaceHeavyPlanner {
                workspace_bytes: 128,
            },
            BuildOptions::strict(),
        )
        .unwrap();
        assert_eq!(planner.run(&PlannerInput { seed: 2 }).unwrap(), &Plan(256));
    }
}
