use std::any::{Any, TypeId, type_name};
use std::collections::{BTreeMap, BTreeSet, btree_map::Entry};
use std::fmt::{self, Write};
use std::marker::PhantomData;
use std::sync::Arc;

use crate::{
    BoundSources, ResourceRegistry, RunError, SemanticType, UnitRequirements, UnitWorkspace,
    runtime::{ExecutableAdapter, PreparedExecutable, RegistrationInvocation},
};

macro_rules! string_id {
    ($name:ident) => {
        #[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
        pub struct $name(String);

        impl $name {
            #[must_use]
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self::new(value)
            }
        }
    };
}

string_id!(UnitId);
string_id!(ResourceId);
string_id!(UnitTypeName);

/// Concrete Rust representation expected by a port.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConcreteType {
    id: TypeId,
    name: &'static str,
}

impl ConcreteType {
    #[must_use]
    pub fn of<T: 'static>() -> Self {
        Self {
            id: TypeId::of::<T>(),
            name: type_name::<T>(),
        }
    }

    #[must_use]
    pub(crate) const fn id(self) -> TypeId {
        self.id
    }

    #[must_use]
    pub(crate) const fn name(self) -> &'static str {
        self.name
    }
}

/// Static contract for one required input or output port.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PortDescriptor {
    pub name: String,
    pub semantic_type: SemanticType,
    pub concrete_type: ConcreteType,
}

impl PortDescriptor {
    #[must_use]
    pub fn of<T: 'static>(name: impl Into<String>, semantic_type: SemanticType) -> Self {
        Self {
            name: name.into(),
            semantic_type,
            concrete_type: ConcreteType::of::<T>(),
        }
    }
}

/// Graph-relevant static metadata for a registered Unit type.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnitDescriptor {
    pub type_name: UnitTypeName,
    pub inputs: Vec<PortDescriptor>,
    pub outputs: Vec<PortDescriptor>,
}

type ErasedConfiguration = dyn Any + Send + Sync;
type ConfigurationDecoder =
    dyn Fn(&dyn Any, &str) -> Result<Box<ErasedConfiguration>, ConfigurationError>;
type RequirementsResolver =
    dyn Fn(&ErasedConfiguration, &BoundSources) -> Result<UnitRequirements, ConfigurationError>;
type ErasedImplementation = dyn Any + Send;
type ExecutableFactory =
    dyn Fn(&ErasedConfiguration) -> Result<Box<ErasedImplementation>, FactoryError>;
type ExecutableAdapterFactory =
    dyn Fn(Box<ErasedImplementation>, DenseUnit, usize) -> Result<PreparedExecutable, FactoryError>;

struct UnitRegistration {
    descriptor: UnitDescriptor,
    source_type: ConcreteType,
    configuration_type: ConcreteType,
    decode: Box<ConfigurationDecoder>,
    requirements: Box<RequirementsResolver>,
    factory: Option<RegisteredFactory>,
}

struct RegisteredFactory {
    configuration_type: ConcreteType,
    implementation_type: ConcreteType,
    construct: Box<ExecutableFactory>,
    adapt: Option<Box<ExecutableAdapterFactory>>,
}

/// A decoded typed Unit configuration whose concrete value remains private to
/// registration, construction, and inspection code.
pub struct DecodedConfiguration {
    unit_type: UnitTypeName,
    concrete_type: ConcreteType,
    value: Box<ErasedConfiguration>,
}

/// One implementation produced by a registered factory. The value remains
/// erased until the private executable adapter is attached during preparation.
pub struct ConstructedUnit {
    unit_type: UnitTypeName,
    concrete_type: ConcreteType,
    value: Box<ErasedImplementation>,
}

impl ConstructedUnit {
    #[must_use]
    pub fn downcast_ref<T: Any>(&self) -> Option<&T> {
        self.value.downcast_ref()
    }

    #[must_use]
    pub fn downcast_mut<T: Any>(&mut self) -> Option<&mut T> {
        self.value.downcast_mut()
    }

    #[must_use]
    pub const fn concrete_type(&self) -> ConcreteType {
        self.concrete_type
    }

    #[must_use]
    pub const fn unit_type(&self) -> &UnitTypeName {
        &self.unit_type
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FactoryError {
    UnknownUnitType {
        unit_type: UnitTypeName,
    },
    MissingFactory {
        unit_type: UnitTypeName,
    },
    ConfigurationType {
        unit_type: UnitTypeName,
        expected: &'static str,
        actual: &'static str,
    },
    Construction {
        unit_type: UnitTypeName,
        message: String,
    },
    ImplementationType {
        unit_type: UnitTypeName,
        expected: &'static str,
        actual: &'static str,
    },
    MissingExecutor {
        unit_type: UnitTypeName,
    },
    ExecutorImplementationType {
        unit_type: UnitTypeName,
        registered: &'static str,
        executor: &'static str,
    },
}

impl DecodedConfiguration {
    #[must_use]
    pub fn downcast_ref<T: Any>(&self) -> Option<&T> {
        self.value.downcast_ref()
    }

    #[must_use]
    pub const fn concrete_type(&self) -> ConcreteType {
        self.concrete_type
    }
}

/// Frontend-neutral configuration failure. Frontends add source spans while
/// preserving this path and message.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConfigurationError {
    UnknownField {
        path: String,
        field: String,
    },
    Invalid {
        path: String,
        message: String,
    },
    SourceType {
        unit_type: UnitTypeName,
        expected: &'static str,
    },
    ConfigurationType {
        unit_type: UnitTypeName,
        expected: &'static str,
        actual: &'static str,
    },
    UnresolvedRequirement {
        path: String,
        message: String,
    },
}

/// Registration failure detected before a Module can be constructed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RegistrationError {
    Compile(Box<CompileError>),
    DuplicateUnitType {
        unit_type: UnitTypeName,
    },
    UnknownUnitType {
        unit_type: UnitTypeName,
    },
    DuplicateFactory {
        unit_type: UnitTypeName,
    },
    FactoryConfigurationType {
        unit_type: UnitTypeName,
        registered: &'static str,
        factory: &'static str,
    },
    DuplicateExecutor {
        unit_type: UnitTypeName,
    },
    ExecutorImplementationType {
        unit_type: UnitTypeName,
        registered: &'static str,
        executor: &'static str,
    },
}

impl From<CompileError> for RegistrationError {
    fn from(error: CompileError) -> Self {
        Self::Compile(Box::new(error))
    }
}

/// Registry of complete Unit contracts compiled into a host binary.
#[derive(Default)]
pub struct UnitRegistry {
    registrations: BTreeMap<UnitTypeName, UnitRegistration>,
}

impl UnitRegistry {
    pub fn register<C, S, D, R>(
        &mut self,
        descriptor: UnitDescriptor,
        decode: D,
        requirements: R,
    ) -> Result<(), RegistrationError>
    where
        C: Any + Send + Sync,
        S: Any,
        D: Fn(&S, &str) -> Result<C, ConfigurationError> + 'static,
        R: Fn(&C, &BoundSources) -> Result<UnitRequirements, String> + 'static,
    {
        validate_descriptor_ports(&descriptor)?;
        let name = descriptor.type_name.clone();
        match self.registrations.entry(name) {
            Entry::Vacant(entry) => {
                let decode_unit_type = descriptor.type_name.clone();
                let requirements_unit_type = descriptor.type_name.clone();
                entry.insert(UnitRegistration {
                    descriptor,
                    source_type: ConcreteType::of::<S>(),
                    configuration_type: ConcreteType::of::<C>(),
                    decode: Box::new(move |source, path| {
                        let source = source.downcast_ref::<S>().ok_or_else(|| {
                            ConfigurationError::SourceType {
                                unit_type: decode_unit_type.clone(),
                                expected: type_name::<S>(),
                            }
                        })?;
                        decode(source, path)
                            .map(|config| Box::new(config) as Box<ErasedConfiguration>)
                    }),
                    requirements: Box::new(move |config, sources| {
                        let typed = config.downcast_ref::<C>().ok_or_else(|| {
                            ConfigurationError::ConfigurationType {
                                unit_type: requirements_unit_type.clone(),
                                expected: type_name::<C>(),
                                actual: type_name_of_any(config),
                            }
                        })?;
                        requirements(typed, sources).map_err(|message| {
                            ConfigurationError::UnresolvedRequirement {
                                path: String::new(),
                                message,
                            }
                        })
                    }),
                    factory: None,
                });
                Ok(())
            }
            Entry::Occupied(entry) => Err(RegistrationError::DuplicateUnitType {
                unit_type: entry.key().clone(),
            }),
        }
    }

    pub fn register_factory<C, U, F>(
        &mut self,
        unit_type: &UnitTypeName,
        factory: F,
    ) -> Result<(), RegistrationError>
    where
        C: Any + Send + Sync,
        U: Any + Send,
        F: Fn(&C) -> Result<U, String> + 'static,
    {
        let registration = self.registrations.get_mut(unit_type).ok_or_else(|| {
            RegistrationError::UnknownUnitType {
                unit_type: unit_type.clone(),
            }
        })?;
        if registration.factory.is_some() {
            return Err(RegistrationError::DuplicateFactory {
                unit_type: unit_type.clone(),
            });
        }
        let factory_configuration_type = ConcreteType::of::<C>();
        if registration.configuration_type != factory_configuration_type {
            return Err(RegistrationError::FactoryConfigurationType {
                unit_type: unit_type.clone(),
                registered: registration.configuration_type.name,
                factory: factory_configuration_type.name,
            });
        }
        let factory_unit_type = unit_type.clone();
        registration.factory = Some(RegisteredFactory {
            configuration_type: factory_configuration_type,
            implementation_type: ConcreteType::of::<U>(),
            construct: Box::new(move |configuration| {
                let typed = configuration.downcast_ref::<C>().ok_or_else(|| {
                    FactoryError::ConfigurationType {
                        unit_type: factory_unit_type.clone(),
                        expected: type_name::<C>(),
                        actual: type_name_of_any(configuration),
                    }
                })?;
                factory(typed)
                    .map(|unit| Box::new(unit) as Box<ErasedImplementation>)
                    .map_err(|message| FactoryError::Construction {
                        unit_type: factory_unit_type.clone(),
                        message,
                    })
            }),
            adapt: None,
        });
        Ok(())
    }

    pub fn register_executor<U, E>(
        &mut self,
        unit_type: &UnitTypeName,
        execute: E,
    ) -> Result<(), RegistrationError>
    where
        U: Any + Send,
        E: Fn(&mut U, &RegistrationInvocation<'_>, UnitWorkspace<'_>) -> Result<(), RunError>
            + Send
            + Sync
            + 'static,
    {
        let registration = self.registrations.get_mut(unit_type).ok_or_else(|| {
            RegistrationError::UnknownUnitType {
                unit_type: unit_type.clone(),
            }
        })?;
        let factory =
            registration
                .factory
                .as_mut()
                .ok_or_else(|| RegistrationError::UnknownUnitType {
                    unit_type: unit_type.clone(),
                })?;
        if factory.adapt.is_some() {
            return Err(RegistrationError::DuplicateExecutor {
                unit_type: unit_type.clone(),
            });
        }
        let executor_type = ConcreteType::of::<U>();
        if factory.implementation_type != executor_type {
            return Err(RegistrationError::ExecutorImplementationType {
                unit_type: unit_type.clone(),
                registered: factory.implementation_type.name,
                executor: executor_type.name,
            });
        }
        let adapter_unit_type = unit_type.clone();
        let execute = Arc::new(execute);
        factory.adapt = Some(Box::new(move |implementation, unit, workspace_bytes| {
            let implementation = implementation.downcast::<U>().map_err(|value| {
                FactoryError::ExecutorImplementationType {
                    unit_type: adapter_unit_type.clone(),
                    registered: type_name::<U>(),
                    executor: type_name_of_any(value.as_ref()),
                }
            })?;
            Ok(PreparedExecutable {
                unit,
                executable: Box::new(ExecutableAdapter::new(
                    *implementation,
                    Arc::clone(&execute),
                )),
                workspace: vec![0; workspace_bytes],
            })
        }));
        Ok(())
    }

    pub fn construct(
        &self,
        configuration: &DecodedConfiguration,
    ) -> Result<ConstructedUnit, FactoryError> {
        let registration = self
            .registrations
            .get(&configuration.unit_type)
            .ok_or_else(|| FactoryError::UnknownUnitType {
                unit_type: configuration.unit_type.clone(),
            })?;
        let factory =
            registration
                .factory
                .as_ref()
                .ok_or_else(|| FactoryError::MissingFactory {
                    unit_type: configuration.unit_type.clone(),
                })?;
        if configuration.concrete_type != factory.configuration_type {
            return Err(FactoryError::ConfigurationType {
                unit_type: configuration.unit_type.clone(),
                expected: factory.configuration_type.name,
                actual: configuration.concrete_type.name,
            });
        }
        let value = (factory.construct)(configuration.value.as_ref())?;
        if value.as_ref().type_id() != factory.implementation_type.id {
            return Err(FactoryError::ImplementationType {
                unit_type: configuration.unit_type.clone(),
                expected: factory.implementation_type.name,
                actual: type_name_of_any(value.as_ref()),
            });
        }
        Ok(ConstructedUnit {
            unit_type: configuration.unit_type.clone(),
            concrete_type: factory.implementation_type,
            value,
        })
    }

    #[allow(dead_code)]
    pub(crate) fn prepare_executable(
        &self,
        configuration: &DecodedConfiguration,
        unit: DenseUnit,
        workspace_bytes: usize,
    ) -> Result<PreparedExecutable, FactoryError> {
        let registration = self
            .registrations
            .get(&configuration.unit_type)
            .ok_or_else(|| FactoryError::UnknownUnitType {
                unit_type: configuration.unit_type.clone(),
            })?;
        let factory =
            registration
                .factory
                .as_ref()
                .ok_or_else(|| FactoryError::MissingFactory {
                    unit_type: configuration.unit_type.clone(),
                })?;
        let adapt = factory
            .adapt
            .as_ref()
            .ok_or_else(|| FactoryError::MissingExecutor {
                unit_type: configuration.unit_type.clone(),
            })?;
        let constructed = self.construct(configuration)?;
        adapt(constructed.value, unit, workspace_bytes)
    }

    #[must_use]
    pub fn get(&self, name: &UnitTypeName) -> Option<&UnitDescriptor> {
        self.registrations
            .get(name)
            .map(|registration| &registration.descriptor)
    }

    pub fn decode(
        &self,
        name: &UnitTypeName,
        source: &dyn Any,
        path: &str,
    ) -> Result<DecodedConfiguration, ConfigurationError> {
        let registration =
            self.registrations
                .get(name)
                .ok_or_else(|| ConfigurationError::Invalid {
                    path: path.to_owned(),
                    message: format!("Unit type {} is not registered", name.as_str()),
                })?;
        if source.type_id() != registration.source_type.id {
            return Err(ConfigurationError::SourceType {
                unit_type: name.clone(),
                expected: registration.source_type.name,
            });
        }
        let value = (registration.decode)(source, path)?;
        if value.as_ref().type_id() != registration.configuration_type.id {
            return Err(ConfigurationError::ConfigurationType {
                unit_type: name.clone(),
                expected: registration.configuration_type.name,
                actual: type_name_of_any(value.as_ref()),
            });
        }
        Ok(DecodedConfiguration {
            unit_type: name.clone(),
            concrete_type: registration.configuration_type,
            value,
        })
    }

    pub fn resolve_requirements(
        &self,
        configuration: &DecodedConfiguration,
        sources: &BoundSources,
        path: &str,
    ) -> Result<UnitRequirements, ConfigurationError> {
        let registration = self
            .registrations
            .get(&configuration.unit_type)
            .ok_or_else(|| ConfigurationError::Invalid {
                path: path.to_owned(),
                message: format!(
                    "Unit type {} is not registered",
                    configuration.unit_type.as_str()
                ),
            })?;
        if configuration.concrete_type != registration.configuration_type {
            return Err(ConfigurationError::ConfigurationType {
                unit_type: configuration.unit_type.clone(),
                expected: registration.configuration_type.name,
                actual: configuration.concrete_type.name,
            });
        }
        (registration.requirements)(configuration.value.as_ref(), sources).map_err(|error| {
            match error {
                ConfigurationError::UnresolvedRequirement { message, .. } => {
                    ConfigurationError::UnresolvedRequirement {
                        path: path.to_owned(),
                        message,
                    }
                }
                other => other,
            }
        })
    }
}

fn type_name_of_any(_: &dyn Any) -> &'static str {
    "unregistered concrete type"
}

/// Syntax-independent parse boundary. A YAML frontend may retain source spans
/// beside this value, but graph compilation never receives YAML or config data.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParsedModule {
    pub schema: String,
    pub name: String,
    pub inputs: Vec<ParsedModuleInput>,
    pub units: Vec<ParsedUnit>,
    pub outputs: Vec<ResourceId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParsedModuleInput {
    pub resource: ResourceId,
    pub semantic_type: SemanticType,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParsedUnit {
    pub id: UnitId,
    pub unit_type: UnitTypeName,
    pub inputs: Vec<(String, ResourceId)>,
    pub outputs: Vec<(String, ResourceId)>,
}

/// One resolved required port binding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedBinding {
    pub port: String,
    pub resource: ResourceId,
    pub semantic_type: SemanticType,
    pub concrete_type: ConcreteType,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedModuleInput {
    pub resource: ResourceId,
    pub semantic_type: SemanticType,
    pub concrete_type: ConcreteType,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedUnit {
    pub id: UnitId,
    pub unit_type: UnitTypeName,
    pub inputs: Vec<ResolvedBinding>,
    pub outputs: Vec<ResolvedBinding>,
}

/// Validated descriptor identities and typed bindings consumed by graph
/// compilation. It deliberately contains no source value or Unit config.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedModule {
    pub schema: String,
    pub name: String,
    pub inputs: Vec<ResolvedModuleInput>,
    pub units: Vec<ResolvedUnit>,
    pub outputs: Vec<ResourceId>,
}

impl ParsedModule {
    pub fn resolve(
        self,
        units: &UnitRegistry,
        resources: &ResourceRegistry,
    ) -> Result<ResolvedModule, CompileError> {
        let mut resolved_inputs = Vec::with_capacity(self.inputs.len());
        for input in self.inputs {
            let descriptor = resources.get(&input.semantic_type).ok_or_else(|| {
                CompileError::UnknownResourceType {
                    semantic_type: input.semantic_type.clone(),
                }
            })?;
            resolved_inputs.push(ResolvedModuleInput {
                resource: input.resource,
                semantic_type: input.semantic_type,
                concrete_type: ConcreteType {
                    id: descriptor.concrete_type(),
                    name: descriptor.concrete_name(),
                },
            });
        }

        let mut resolved_units = Vec::with_capacity(self.units.len());
        for unit in self.units {
            let descriptor =
                units
                    .get(&unit.unit_type)
                    .ok_or_else(|| CompileError::UnknownUnitType {
                        unit: unit.id.clone(),
                        unit_type: unit.unit_type.clone(),
                    })?;
            resolved_units.push(ResolvedUnit {
                id: unit.id.clone(),
                unit_type: unit.unit_type,
                inputs: resolve_ports(
                    &unit.id,
                    &unit.inputs,
                    &descriptor.inputs,
                    resources,
                    descriptor.type_name.clone(),
                    true,
                )?,
                outputs: resolve_ports(
                    &unit.id,
                    &unit.outputs,
                    &descriptor.outputs,
                    resources,
                    descriptor.type_name.clone(),
                    false,
                )?,
            });
        }

        Ok(ResolvedModule {
            schema: self.schema,
            name: self.name,
            inputs: resolved_inputs,
            units: resolved_units,
            outputs: self.outputs,
        })
    }
}

fn resolve_ports(
    unit: &UnitId,
    bindings: &[(String, ResourceId)],
    ports: &[PortDescriptor],
    resources: &ResourceRegistry,
    unit_type: UnitTypeName,
    input: bool,
) -> Result<Vec<ResolvedBinding>, CompileError> {
    let supplied: BTreeMap<_, _> = bindings.iter().cloned().collect();
    if supplied.len() != bindings.len() {
        return Err(CompileError::DuplicatePortBinding {
            unit: unit.clone(),
            unit_type,
        });
    }
    for port in supplied.keys() {
        if !ports.iter().any(|candidate| &candidate.name == port) {
            return Err(CompileError::UnknownPort {
                unit: unit.clone(),
                unit_type,
                port: port.clone(),
                input,
            });
        }
    }
    let mut resolved = Vec::with_capacity(ports.len());
    for port in ports {
        let resource = supplied
            .get(&port.name)
            .ok_or_else(|| CompileError::MissingPort {
                unit: unit.clone(),
                unit_type: unit_type.clone(),
                port: port.name.clone(),
                input,
            })?;
        let descriptor = resources.get(&port.semantic_type).ok_or_else(|| {
            CompileError::UnknownResourceType {
                semantic_type: port.semantic_type.clone(),
            }
        })?;
        if descriptor.concrete_type() != port.concrete_type.id {
            return Err(CompileError::ConcreteTypeMismatch {
                unit: unit.clone(),
                port: port.name.clone(),
                semantic_type: port.semantic_type.clone(),
                expected: port.concrete_type.name,
                registered: descriptor.concrete_name(),
            });
        }
        resolved.push(ResolvedBinding {
            port: port.name.clone(),
            resource: resource.clone(),
            semantic_type: port.semantic_type.clone(),
            concrete_type: port.concrete_type,
        });
    }
    resolved.sort_by(|left, right| left.port.cmp(&right.port));
    Ok(resolved)
}

fn validate_descriptor_ports(descriptor: &UnitDescriptor) -> Result<(), CompileError> {
    for ports in [&descriptor.inputs, &descriptor.outputs] {
        let names: BTreeSet<_> = ports.iter().map(|port| &port.name).collect();
        if names.len() != ports.len() {
            return Err(CompileError::DuplicateDescriptorPort {
                unit_type: descriptor.type_name.clone(),
            });
        }
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Producer {
    ModuleInput,
    Unit { unit: UnitId, port: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Consumer {
    pub unit: UnitId,
    pub port: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompiledResource {
    pub id: ResourceId,
    pub semantic_type: SemanticType,
    pub concrete_type: ConcreteType,
    pub concrete_name: &'static str,
    pub producer: Producer,
    pub consumers: Vec<Consumer>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompiledUnit {
    pub id: UnitId,
    pub unit_type: UnitTypeName,
    pub inputs: Vec<ResolvedBinding>,
    pub outputs: Vec<ResolvedBinding>,
    pub dependencies: Vec<UnitId>,
}

/// Normalized fixed graph. Equality is structural and independent of source order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompiledGraph {
    pub schema: String,
    pub module: String,
    pub units: Vec<CompiledUnit>,
    pub resources: Vec<CompiledResource>,
    pub module_outputs: Vec<ResourceId>,
    pub execution_order: Vec<UnitId>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct UnitIndex(usize);

impl UnitIndex {
    #[must_use]
    pub const fn get(self) -> usize {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ResourceIndex(usize);

impl ResourceIndex {
    #[must_use]
    pub const fn get(self) -> usize {
        self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DenseBinding {
    pub port: String,
    pub resource: ResourceIndex,
    pub concrete_type: ConcreteType,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DenseUnit {
    pub id: UnitId,
    pub unit_type: UnitTypeName,
    pub inputs: Vec<DenseBinding>,
    pub outputs: Vec<DenseBinding>,
    pub dependencies: Vec<UnitIndex>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DenseResource {
    pub id: ResourceId,
    pub semantic_type: SemanticType,
    pub concrete_type: ConcreteType,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DenseGraph {
    plan_token: u64,
    pub units: Vec<DenseUnit>,
    pub resources: Vec<DenseResource>,
    pub execution_order: Vec<UnitIndex>,
    module_inputs: BTreeSet<ResourceIndex>,
    module_outputs: BTreeSet<ResourceIndex>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HandleError {
    UnknownResource {
        resource: ResourceId,
    },
    NotModuleInput {
        resource: ResourceId,
    },
    NotModuleOutput {
        resource: ResourceId,
    },
    ConcreteType {
        resource: ResourceId,
        expected: &'static str,
        actual: &'static str,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InputHandle<T: 'static> {
    resource: ResourceIndex,
    plan_token: u64,
    marker: PhantomData<fn() -> T>,
}

impl<T: 'static> InputHandle<T> {
    #[must_use]
    pub const fn resource(&self) -> ResourceIndex {
        self.resource
    }

    #[must_use]
    pub const fn plan_token(&self) -> u64 {
        self.plan_token
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutputHandle<T: 'static> {
    resource: ResourceIndex,
    plan_token: u64,
    marker: PhantomData<fn() -> T>,
}

impl<T: 'static> OutputHandle<T> {
    #[must_use]
    pub const fn resource(&self) -> ResourceIndex {
        self.resource
    }

    #[must_use]
    pub const fn plan_token(&self) -> u64 {
        self.plan_token
    }
}

impl CompiledGraph {
    pub fn into_dense(self, plan_token: u64) -> Result<DenseGraph, CompileError> {
        let unit_indices: BTreeMap<_, _> = self
            .units
            .iter()
            .enumerate()
            .map(|(index, unit)| (unit.id.clone(), UnitIndex(index)))
            .collect();
        let resource_indices: BTreeMap<_, _> = self
            .resources
            .iter()
            .enumerate()
            .map(|(index, resource)| (resource.id.clone(), ResourceIndex(index)))
            .collect();
        let dense_binding = |binding: ResolvedBinding| {
            let resource = resource_indices
                .get(&binding.resource)
                .copied()
                .ok_or_else(|| CompileError::DenseResourceMissing {
                    resource: binding.resource.clone(),
                })?;
            Ok(DenseBinding {
                port: binding.port,
                resource,
                concrete_type: binding.concrete_type,
            })
        };
        let units = self
            .units
            .into_iter()
            .map(|unit| {
                Ok(DenseUnit {
                    id: unit.id,
                    unit_type: unit.unit_type,
                    inputs: unit
                        .inputs
                        .into_iter()
                        .map(&dense_binding)
                        .collect::<Result<_, _>>()?,
                    outputs: unit
                        .outputs
                        .into_iter()
                        .map(&dense_binding)
                        .collect::<Result<_, _>>()?,
                    dependencies: unit
                        .dependencies
                        .iter()
                        .map(|dependency| {
                            unit_indices.get(dependency).copied().ok_or_else(|| {
                                CompileError::DenseUnitMissing {
                                    unit: dependency.clone(),
                                }
                            })
                        })
                        .collect::<Result<_, _>>()?,
                })
            })
            .collect::<Result<Vec<_>, CompileError>>()?;
        let execution_order = self
            .execution_order
            .iter()
            .map(|unit| {
                unit_indices
                    .get(unit)
                    .copied()
                    .ok_or_else(|| CompileError::DenseUnitMissing { unit: unit.clone() })
            })
            .collect::<Result<_, _>>()?;
        let module_inputs = self
            .resources
            .iter()
            .enumerate()
            .filter_map(|(index, resource)| {
                matches!(resource.producer, Producer::ModuleInput).then_some(ResourceIndex(index))
            })
            .collect();
        let module_outputs = self
            .module_outputs
            .iter()
            .map(|resource| {
                resource_indices.get(resource).copied().ok_or_else(|| {
                    CompileError::DenseResourceMissing {
                        resource: resource.clone(),
                    }
                })
            })
            .collect::<Result<_, _>>()?;
        let resources = self
            .resources
            .into_iter()
            .map(|resource| DenseResource {
                id: resource.id,
                semantic_type: resource.semantic_type,
                concrete_type: resource.concrete_type,
            })
            .collect();
        Ok(DenseGraph {
            plan_token,
            units,
            resources,
            execution_order,
            module_inputs,
            module_outputs,
        })
    }
}

impl DenseGraph {
    #[must_use]
    pub(crate) const fn plan_token(&self) -> u64 {
        self.plan_token
    }

    #[must_use]
    pub(crate) fn module_inputs(&self) -> &BTreeSet<ResourceIndex> {
        &self.module_inputs
    }

    pub fn input_handle<T: 'static>(
        &self,
        resource: &ResourceId,
    ) -> Result<InputHandle<T>, HandleError> {
        let index = self.resource_index(resource)?;
        if !self.module_inputs.contains(&index) {
            return Err(HandleError::NotModuleInput {
                resource: resource.clone(),
            });
        }
        self.validate_handle_type::<T>(index)?;
        Ok(InputHandle {
            resource: index,
            plan_token: self.plan_token,
            marker: PhantomData,
        })
    }

    pub fn output_handle<T: 'static>(
        &self,
        resource: &ResourceId,
    ) -> Result<OutputHandle<T>, HandleError> {
        let index = self.resource_index(resource)?;
        if !self.module_outputs.contains(&index) {
            return Err(HandleError::NotModuleOutput {
                resource: resource.clone(),
            });
        }
        self.validate_handle_type::<T>(index)?;
        Ok(OutputHandle {
            resource: index,
            plan_token: self.plan_token,
            marker: PhantomData,
        })
    }

    fn resource_index(&self, resource: &ResourceId) -> Result<ResourceIndex, HandleError> {
        self.resources
            .iter()
            .position(|candidate| candidate.id == *resource)
            .map(ResourceIndex)
            .ok_or_else(|| HandleError::UnknownResource {
                resource: resource.clone(),
            })
    }

    fn validate_handle_type<T: 'static>(&self, index: ResourceIndex) -> Result<(), HandleError> {
        let resource = &self.resources[index.0];
        if resource.concrete_type.id != TypeId::of::<T>() {
            return Err(HandleError::ConcreteType {
                resource: resource.id.clone(),
                expected: resource.concrete_type.name,
                actual: type_name::<T>(),
            });
        }
        Ok(())
    }
}

impl ResolvedModule {
    pub fn compile(self) -> Result<CompiledGraph, CompileError> {
        let mut producers = BTreeMap::<ResourceId, (SemanticType, ConcreteType, Producer)>::new();
        for input in &self.inputs {
            insert_producer(
                &mut producers,
                input.resource.clone(),
                input.semantic_type.clone(),
                input.concrete_type,
                Producer::ModuleInput,
            )?;
        }
        let mut unit_ids = BTreeSet::new();
        for unit in &self.units {
            if !unit_ids.insert(unit.id.clone()) {
                return Err(CompileError::DuplicateUnit {
                    unit: unit.id.clone(),
                });
            }
            validate_resolved_ports(unit)?;
            for output in &unit.outputs {
                insert_producer(
                    &mut producers,
                    output.resource.clone(),
                    output.semantic_type.clone(),
                    output.concrete_type,
                    Producer::Unit {
                        unit: unit.id.clone(),
                        port: output.port.clone(),
                    },
                )?;
            }
        }

        let mut consumers = BTreeMap::<ResourceId, Vec<Consumer>>::new();
        let mut dependencies = BTreeMap::<UnitId, BTreeSet<UnitId>>::new();
        let mut outgoing = BTreeMap::<UnitId, BTreeSet<UnitId>>::new();
        for id in &unit_ids {
            dependencies.insert(id.clone(), BTreeSet::new());
            outgoing.insert(id.clone(), BTreeSet::new());
        }
        for unit in &self.units {
            for input in &unit.inputs {
                let Some((semantic, concrete, producer)) = producers.get(&input.resource) else {
                    return Err(CompileError::UnknownResource {
                        unit: unit.id.clone(),
                        port: input.port.clone(),
                        resource: input.resource.clone(),
                    });
                };
                validate_binding_type(unit, input, semantic, *concrete)?;
                consumers
                    .entry(input.resource.clone())
                    .or_default()
                    .push(Consumer {
                        unit: unit.id.clone(),
                        port: input.port.clone(),
                    });
                if let Producer::Unit { unit: producer, .. } = producer {
                    dependencies
                        .get_mut(&unit.id)
                        .expect("known unit")
                        .insert(producer.clone());
                    outgoing
                        .get_mut(producer)
                        .expect("known producer")
                        .insert(unit.id.clone());
                }
            }
        }
        for output in &self.outputs {
            if !producers.contains_key(output) {
                return Err(CompileError::UnknownModuleOutput {
                    resource: output.clone(),
                });
            }
        }

        let execution_order = stable_topological_order(&dependencies, &outgoing)?;
        let mut units: Vec<_> = self
            .units
            .into_iter()
            .map(|mut unit| {
                unit.inputs.sort_by(|a, b| a.port.cmp(&b.port));
                unit.outputs.sort_by(|a, b| a.port.cmp(&b.port));
                CompiledUnit {
                    dependencies: dependencies[&unit.id].iter().cloned().collect(),
                    id: unit.id,
                    unit_type: unit.unit_type,
                    inputs: unit.inputs,
                    outputs: unit.outputs,
                }
            })
            .collect();
        units.sort_by(|a, b| a.id.cmp(&b.id));

        let resources = producers
            .into_iter()
            .map(|(id, (semantic_type, concrete, producer))| {
                let mut resource_consumers = consumers.remove(&id).unwrap_or_default();
                resource_consumers.sort_by(|a, b| (&a.unit, &a.port).cmp(&(&b.unit, &b.port)));
                CompiledResource {
                    id,
                    semantic_type,
                    concrete_type: concrete,
                    concrete_name: concrete.name,
                    producer,
                    consumers: resource_consumers,
                }
            })
            .collect();
        let mut module_outputs = self.outputs;
        module_outputs.sort();
        if module_outputs.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(CompileError::DuplicateModuleOutput);
        }
        Ok(CompiledGraph {
            schema: self.schema,
            module: self.name,
            units,
            resources,
            module_outputs,
            execution_order,
        })
    }
}

fn validate_resolved_ports(unit: &ResolvedUnit) -> Result<(), CompileError> {
    for bindings in [&unit.inputs, &unit.outputs] {
        let ports: BTreeSet<_> = bindings.iter().map(|binding| &binding.port).collect();
        if ports.len() != bindings.len() {
            return Err(CompileError::DuplicateResolvedPort {
                unit: unit.id.clone(),
            });
        }
    }
    Ok(())
}

fn insert_producer(
    producers: &mut BTreeMap<ResourceId, (SemanticType, ConcreteType, Producer)>,
    resource: ResourceId,
    semantic: SemanticType,
    concrete: ConcreteType,
    producer: Producer,
) -> Result<(), CompileError> {
    if let Some((_, _, first)) =
        producers.insert(resource.clone(), (semantic, concrete, producer.clone()))
    {
        return Err(CompileError::DuplicateProducer {
            resource,
            first: first.clone(),
            second: producer,
        });
    }
    Ok(())
}

fn validate_binding_type(
    unit: &ResolvedUnit,
    binding: &ResolvedBinding,
    semantic: &SemanticType,
    concrete: ConcreteType,
) -> Result<(), CompileError> {
    if &binding.semantic_type != semantic {
        return Err(CompileError::SemanticTypeMismatch {
            unit: unit.id.clone(),
            port: binding.port.clone(),
            resource: binding.resource.clone(),
            expected: binding.semantic_type.clone(),
            actual: semantic.clone(),
        });
    }
    if binding.concrete_type != concrete {
        return Err(CompileError::ConcreteBindingMismatch {
            unit: unit.id.clone(),
            port: binding.port.clone(),
            resource: binding.resource.clone(),
            expected: binding.concrete_type.name,
            actual: concrete.name,
        });
    }
    Ok(())
}

fn stable_topological_order(
    dependencies: &BTreeMap<UnitId, BTreeSet<UnitId>>,
    outgoing: &BTreeMap<UnitId, BTreeSet<UnitId>>,
) -> Result<Vec<UnitId>, CompileError> {
    let mut indegree: BTreeMap<_, _> = dependencies
        .iter()
        .map(|(unit, incoming)| (unit.clone(), incoming.len()))
        .collect();
    let mut ready: BTreeSet<_> = indegree
        .iter()
        .filter(|(_, degree)| **degree == 0)
        .map(|(unit, _)| unit.clone())
        .collect();
    let mut order = Vec::with_capacity(indegree.len());
    while let Some(unit) = ready.pop_first() {
        order.push(unit.clone());
        for consumer in &outgoing[&unit] {
            let degree = indegree.get_mut(consumer).expect("known consumer");
            *degree -= 1;
            if *degree == 0 {
                ready.insert(consumer.clone());
            }
        }
    }
    if order.len() != indegree.len() {
        let remaining: BTreeSet<_> = indegree
            .into_iter()
            .filter(|(_, degree)| *degree > 0)
            .map(|(unit, _)| unit)
            .collect();
        let cycle = find_cycle(&remaining, outgoing);
        return Err(CompileError::Cycle { path: cycle });
    }
    Ok(order)
}

fn find_cycle(
    remaining: &BTreeSet<UnitId>,
    outgoing: &BTreeMap<UnitId, BTreeSet<UnitId>>,
) -> Vec<UnitId> {
    fn visit(
        unit: &UnitId,
        remaining: &BTreeSet<UnitId>,
        outgoing: &BTreeMap<UnitId, BTreeSet<UnitId>>,
        visiting: &mut Vec<UnitId>,
        visited: &mut BTreeSet<UnitId>,
    ) -> Option<Vec<UnitId>> {
        if let Some(start) = visiting.iter().position(|candidate| candidate == unit) {
            let mut cycle = visiting[start..].to_vec();
            cycle.push(unit.clone());
            return Some(cycle);
        }
        if !visited.insert(unit.clone()) {
            return None;
        }
        visiting.push(unit.clone());
        for next in &outgoing[unit] {
            if remaining.contains(next)
                && let Some(cycle) = visit(next, remaining, outgoing, visiting, visited)
            {
                return Some(cycle);
            }
        }
        visiting.pop();
        None
    }

    let mut visited = BTreeSet::new();
    for unit in remaining {
        if let Some(cycle) = visit(unit, remaining, outgoing, &mut Vec::new(), &mut visited) {
            return cycle;
        }
    }
    remaining.iter().cloned().collect()
}

impl CompiledGraph {
    #[must_use]
    pub fn to_text(&self) -> String {
        let mut output = format!("module {} ({})\n", self.module, self.schema);
        writeln!(output, "execution: {}", join_ids(&self.execution_order))
            .expect("String writes cannot fail");
        for resource in &self.resources {
            let producer = match &resource.producer {
                Producer::ModuleInput => "module input".to_owned(),
                Producer::Unit { unit, port } => format!("{}.{}", unit.as_str(), port),
            };
            let consumers = resource
                .consumers
                .iter()
                .map(|consumer| format!("{}.{}", consumer.unit.as_str(), consumer.port))
                .collect::<Vec<_>>()
                .join(", ");
            writeln!(
                output,
                "resource {}: {} [{}]; producer: {}; consumers: [{}]",
                resource.id.as_str(),
                resource.semantic_type.as_str(),
                resource.concrete_name,
                producer,
                consumers
            )
            .expect("String writes cannot fail");
        }
        output
    }

    #[must_use]
    pub fn to_dot(&self) -> String {
        let mut output = format!("digraph \"{}\" {{\n", escape(&self.module));
        for unit in &self.units {
            writeln!(
                output,
                "  \"{}\" [shape=box,style=rounded,class=\"unit\",label=\"{}\\nUnit\\n{}\"];",
                escape(unit.id.as_str()),
                escape(unit.id.as_str()),
                escape(unit.unit_type.as_str())
            )
            .expect("String writes cannot fail");
        }
        for resource in &self.resources {
            let resource_node = resource_node_id(&resource.id);
            let role = self.resource_role(resource);
            writeln!(
                output,
                "  \"{}\" [shape={},style={},class=\"{}\",label=\"{}\\n{}\\n{}\"];",
                resource_node,
                role.dot_shape(),
                role.dot_style(),
                role.css_classes(),
                escape(resource.id.as_str()),
                role.label(),
                escape(resource.semantic_type.as_str())
            )
            .expect("String writes cannot fail");
            if let Producer::Unit { unit, port } = &resource.producer {
                writeln!(
                    output,
                    "  \"{}\" -> \"{}\" [label=\"{}\"];",
                    escape(unit.as_str()),
                    resource_node,
                    escape(port)
                )
                .expect("String writes cannot fail");
            }
            for consumer in &resource.consumers {
                writeln!(
                    output,
                    "  \"{}\" -> \"{}\" [label=\"{}\"];",
                    resource_node,
                    escape(consumer.unit.as_str()),
                    escape(&consumer.port)
                )
                .expect("String writes cannot fail");
            }
        }
        output.push_str("}\n");
        output
    }

    #[must_use]
    pub fn to_mermaid(&self) -> String {
        self.to_mermaid_with_unit_annotations(&BTreeMap::new())
    }

    pub(crate) fn to_mermaid_with_unit_annotations(
        &self,
        annotations: &BTreeMap<UnitId, String>,
    ) -> String {
        let mut output = String::from("flowchart TD\n");
        for unit in &self.units {
            let annotation = annotations
                .get(&unit.id)
                .map(|value| format!("<br/>{}", escape_mermaid_label(value)))
                .unwrap_or_default();
            writeln!(
                output,
                "  {}[\"{}<br/>Unit<br/>{}{}\"]:::unit",
                mermaid_id(unit.id.as_str()),
                escape_mermaid_label(unit.id.as_str()),
                escape_mermaid_label(unit.unit_type.as_str()),
                annotation,
            )
            .expect("String writes cannot fail");
        }
        for resource in &self.resources {
            let resource_node = resource_node_id(&resource.id);
            let role = self.resource_role(resource);
            writeln!(
                output,
                "  {}{}:::{}",
                resource_node,
                role.mermaid_node(resource.id.as_str(), resource.semantic_type.as_str()),
                role.mermaid_class()
            )
            .expect("String writes cannot fail");
            if let Producer::Unit { unit, port } = &resource.producer {
                writeln!(
                    output,
                    "  {} -->|{}| {}",
                    mermaid_id(unit.as_str()),
                    escape_mermaid_label(port),
                    resource_node
                )
                .expect("String writes cannot fail");
            }
            for consumer in &resource.consumers {
                writeln!(
                    output,
                    "  {} -->|{}| {}",
                    resource_node,
                    escape_mermaid_label(&consumer.port),
                    mermaid_id(consumer.unit.as_str())
                )
                .expect("String writes cannot fail");
            }
        }
        output.push_str("  classDef unit fill:#eef2ff,stroke:#4338ca,stroke-width:1px\n");
        output.push_str("  classDef resource fill:#f8fafc,stroke:#64748b,stroke-width:1px\n");
        output.push_str("  classDef moduleInput fill:#ecfdf5,stroke:#047857,stroke-width:2px\n");
        output.push_str("  classDef moduleOutput fill:#fff7ed,stroke:#c2410c,stroke-width:3px\n");
        output.push_str(
            "  classDef moduleInputOutput fill:#fefce8,stroke:#a16207,stroke-width:3px\n",
        );
        output
    }

    fn resource_role(&self, resource: &CompiledResource) -> ResourceRole {
        match (
            matches!(&resource.producer, Producer::ModuleInput),
            self.module_outputs.binary_search(&resource.id).is_ok(),
        ) {
            (false, false) => ResourceRole::Internal,
            (true, false) => ResourceRole::ModuleInput,
            (false, true) => ResourceRole::ModuleOutput,
            (true, true) => ResourceRole::ModuleInputOutput,
        }
    }
}

#[derive(Clone, Copy)]
enum ResourceRole {
    Internal,
    ModuleInput,
    ModuleOutput,
    ModuleInputOutput,
}

impl ResourceRole {
    const fn label(self) -> &'static str {
        match self {
            Self::Internal => "Resource",
            Self::ModuleInput => "Module input",
            Self::ModuleOutput => "Module output",
            Self::ModuleInputOutput => "Module input / output",
        }
    }

    const fn css_classes(self) -> &'static str {
        match self {
            Self::Internal => "resource",
            Self::ModuleInput => "resource module-input",
            Self::ModuleOutput => "resource module-output",
            Self::ModuleInputOutput => "resource module-input module-output",
        }
    }

    const fn dot_shape(self) -> &'static str {
        match self {
            Self::Internal => "ellipse",
            Self::ModuleInput => "parallelogram",
            Self::ModuleOutput | Self::ModuleInputOutput => "doubleoctagon",
        }
    }

    const fn dot_style(self) -> &'static str {
        match self {
            Self::Internal => "solid",
            Self::ModuleInput | Self::ModuleOutput | Self::ModuleInputOutput => "bold",
        }
    }

    const fn mermaid_class(self) -> &'static str {
        match self {
            Self::Internal => "resource",
            Self::ModuleInput => "moduleInput",
            Self::ModuleOutput => "moduleOutput",
            Self::ModuleInputOutput => "moduleInputOutput",
        }
    }

    fn mermaid_node(self, id: &str, semantic_type: &str) -> String {
        let label = format!(
            "{}<br/>{}<br/>{}",
            escape_mermaid_label(id),
            self.label(),
            escape_mermaid_label(semantic_type)
        );
        match self {
            Self::Internal => format!("([\"{label}\"])"),
            Self::ModuleInput => format!("[/\"{label}\"/]"),
            Self::ModuleOutput | Self::ModuleInputOutput => format!("{{{{\"{label}\"}}}}"),
        }
    }
}

fn join_ids(ids: &[UnitId]) -> String {
    ids.iter()
        .map(UnitId::as_str)
        .collect::<Vec<_>>()
        .join(", ")
}

fn escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn escape_mermaid_label(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn mermaid_id(value: &str) -> String {
    encoded_id("unit_", value)
}

fn resource_node_id(resource: &ResourceId) -> String {
    encoded_id("resource_", resource.as_str())
}

fn encoded_id(prefix: &str, value: &str) -> String {
    let mut result = String::from(prefix);
    for byte in value.bytes() {
        write!(result, "{byte:02x}").expect("String writes cannot fail");
    }
    result
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CompileError {
    DuplicateUnitType {
        unit_type: UnitTypeName,
    },
    DuplicateDescriptorPort {
        unit_type: UnitTypeName,
    },
    UnknownUnitType {
        unit: UnitId,
        unit_type: UnitTypeName,
    },
    UnknownResourceType {
        semantic_type: SemanticType,
    },
    MissingPort {
        unit: UnitId,
        unit_type: UnitTypeName,
        port: String,
        input: bool,
    },
    UnknownPort {
        unit: UnitId,
        unit_type: UnitTypeName,
        port: String,
        input: bool,
    },
    DuplicatePortBinding {
        unit: UnitId,
        unit_type: UnitTypeName,
    },
    ConcreteTypeMismatch {
        unit: UnitId,
        port: String,
        semantic_type: SemanticType,
        expected: &'static str,
        registered: &'static str,
    },
    DuplicateUnit {
        unit: UnitId,
    },
    DuplicateResolvedPort {
        unit: UnitId,
    },
    DuplicateProducer {
        resource: ResourceId,
        first: Producer,
        second: Producer,
    },
    UnknownResource {
        unit: UnitId,
        port: String,
        resource: ResourceId,
    },
    UnknownModuleOutput {
        resource: ResourceId,
    },
    DuplicateModuleOutput,
    SemanticTypeMismatch {
        unit: UnitId,
        port: String,
        resource: ResourceId,
        expected: SemanticType,
        actual: SemanticType,
    },
    ConcreteBindingMismatch {
        unit: UnitId,
        port: String,
        resource: ResourceId,
        expected: &'static str,
        actual: &'static str,
    },
    Cycle {
        path: Vec<UnitId>,
    },
    DenseUnitMissing {
        unit: UnitId,
    },
    DenseResourceMissing {
        resource: ResourceId,
    },
}

impl fmt::Display for CompileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for CompileError {}
