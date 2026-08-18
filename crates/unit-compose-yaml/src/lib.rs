//! Bounded, span-preserving YAML frontend for UnitCompose V0 definitions.
//!
//! Saphyr events are converted into a private lossless syntax tree before
//! schema validation. Public results contain only core identities, validated
//! typed configuration, and resolved numeric requirements.

mod syntax;

use std::any::Any;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::de::DeserializeOwned;
use unit_compose_core::{
    CompileError, CompiledGraph, ConfigurationError, DecodedConfiguration, ParsedModule,
    ParsedModuleInput, ParsedUnit, ResolvedModule, ResourceId, ResourceRegistry,
    ResourceRequirement, SemanticType, UnitDescriptor, UnitId, UnitRegistry, UnitTypeName,
};

pub use unit_compose_core::{BoundSources, RegistrationError, UnitRequirements};

use syntax::Node;

pub const SUPPORTED_SCHEMA: &str = "unit-compose/v0alpha1";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParseLimits {
    pub max_document_bytes: usize,
    pub max_depth: usize,
}

impl Default for ParseLimits {
    fn default() -> Self {
        Self {
            max_document_bytes: 1024 * 1024,
            max_depth: 64,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceSpan {
    pub start_line: usize,
    pub start_column: usize,
    pub start_offset: usize,
    pub end_line: usize,
    pub end_column: usize,
    pub end_offset: usize,
}

impl SourceSpan {
    const fn point(line: usize, column: usize, offset: usize) -> Self {
        Self {
            start_line: line,
            start_column: column,
            start_offset: offset,
            end_line: line,
            end_column: column,
            end_offset: offset,
        }
    }

    const fn through(self, end: Self) -> Self {
        Self {
            end_line: end.end_line,
            end_column: end.end_column,
            end_offset: end.end_offset,
            ..self
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiagnosticKind {
    Syntax,
    DocumentTooLarge,
    DepthLimit,
    Alias,
    MergeKey,
    DuplicateKey,
    UnknownField,
    MissingField,
    InvalidField,
    UnsupportedSchema,
    UnknownUnit,
    MissingPort,
    DuplicateProducer,
    TypeMismatch,
    UnresolvedBound,
    Cycle,
    Graph,
    Config,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Diagnostic {
    pub kind: DiagnosticKind,
    pub path: String,
    pub span: SourceSpan,
    pub message: String,
}

impl Diagnostic {
    fn new(
        kind: DiagnosticKind,
        path: impl Into<String>,
        span: SourceSpan,
        message: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            path: path.into(),
            span,
            message: message.into(),
        }
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} at {}:{} ({}): {}",
            self.path,
            self.span.start_line,
            self.span.start_column,
            self.span.start_offset,
            self.message
        )
    }
}

impl std::error::Error for Diagnostic {}

pub fn register_unit<T, F>(
    registry: &mut UnitRegistry,
    descriptor: UnitDescriptor,
    requirements: F,
) -> Result<(), RegistrationError>
where
    T: DeserializeOwned + Any + Send + Sync + 'static,
    F: Fn(&T, &BoundSources) -> Result<UnitRequirements, String> + 'static,
{
    registry.register::<T, Node, _, _>(
        descriptor,
        |node, path| {
            let value = node
                .to_json()
                .map_err(|message| ConfigurationError::Invalid {
                    path: path.to_owned(),
                    message: message.to_owned(),
                })?;
            let mut ignored = None;
            let decoded: T = serde_ignored::deserialize(value, |field| {
                ignored.get_or_insert_with(|| field.to_string());
            })
            .map_err(|error| ConfigurationError::Invalid {
                path: path.to_owned(),
                message: error.to_string(),
            })?;
            if let Some(field) = ignored {
                return Err(ConfigurationError::UnknownField {
                    path: format!("{path}.{field}"),
                    field,
                });
            }
            Ok(decoded)
        },
        requirements,
    )
}

pub struct ResolvedDefinition {
    module: ResolvedModule,
    configs: BTreeMap<UnitId, DecodedConfiguration>,
    requirements: BTreeMap<ResourceId, ResourceRequirement>,
    workspace_bytes: BTreeMap<UnitId, usize>,
    spans: SpanIndex,
}

impl ResolvedDefinition {
    #[must_use]
    pub const fn module(&self) -> &ResolvedModule {
        &self.module
    }

    pub fn config<T: Any>(&self, unit: &UnitId) -> Option<&T> {
        self.configs.get(unit)?.downcast_ref()
    }

    #[must_use]
    pub const fn requirements(&self) -> &BTreeMap<ResourceId, ResourceRequirement> {
        &self.requirements
    }

    #[must_use]
    pub const fn workspace_bytes(&self) -> &BTreeMap<UnitId, usize> {
        &self.workspace_bytes
    }

    pub fn compile(self) -> Result<CompiledDefinition, Diagnostic> {
        let Self {
            module,
            configs,
            requirements,
            workspace_bytes,
            spans,
        } = self;
        let graph = module.compile().map_err(|error| spans.graph_error(error))?;
        Ok(CompiledDefinition {
            graph,
            configs,
            requirements,
            workspace_bytes,
        })
    }
}

pub struct CompiledDefinition {
    pub graph: CompiledGraph,
    configs: BTreeMap<UnitId, DecodedConfiguration>,
    pub requirements: BTreeMap<ResourceId, ResourceRequirement>,
    pub workspace_bytes: BTreeMap<UnitId, usize>,
}

impl CompiledDefinition {
    pub fn config<T: Any>(&self, unit: &UnitId) -> Option<&T> {
        self.configs.get(unit)?.downcast_ref()
    }

    #[must_use]
    pub fn into_executable_definition(self) -> unit_compose_core::ExecutableDefinition {
        unit_compose_core::ExecutableDefinition::new(
            self.graph,
            self.configs,
            self.requirements,
            self.workspace_bytes,
        )
    }
}

pub fn load(
    source: &str,
    limits: ParseLimits,
    units: &UnitRegistry,
    resources: &ResourceRegistry,
    bounds: &BoundSources,
) -> Result<ResolvedDefinition, Diagnostic> {
    let root = syntax::parse(source, limits)?;
    let parsed = normalize(&root)?;
    let mut configs = BTreeMap::new();
    let mut requirements = BTreeMap::new();
    let mut workspace_bytes = BTreeMap::new();
    let module = parsed
        .module
        .resolve(units, resources)
        .map_err(|error| parsed.spans.graph_error(error))?;

    for input in &module.inputs {
        let capacity = bounds
            .host
            .get(&input.resource)
            .or_else(|| bounds.adapters.get(&input.semantic_type))
            .copied()
            .ok_or_else(|| parsed.spans.unresolved_bound(&input.resource))?;
        requirements.insert(input.resource.clone(), ResourceRequirement { capacity });
    }
    for unit in &module.units {
        let config_node = &parsed.config_nodes[&unit.id];
        let config_path = format!("$.units.{}.config", unit.id.as_str());
        let config = units
            .decode(&unit.unit_type, config_node, &config_path)
            .map_err(|error| configuration_diagnostic(error, config_node, &parsed.spans))?;
        let resolved = units
            .resolve_requirements(&config, bounds, &config_path)
            .map_err(|error| configuration_diagnostic(error, config_node, &parsed.spans))?;
        for output in &unit.outputs {
            let capacity = resolved
                .output_capacities
                .get(&output.port)
                .or_else(|| bounds.host.get(&output.resource))
                .or_else(|| bounds.adapters.get(&output.semantic_type))
                .copied()
                .ok_or_else(|| parsed.spans.unresolved_bound(&output.resource))?;
            requirements.insert(output.resource.clone(), ResourceRequirement { capacity });
        }
        workspace_bytes.insert(unit.id.clone(), resolved.workspace_bytes);
        configs.insert(unit.id.clone(), config);
    }
    Ok(ResolvedDefinition {
        module,
        configs,
        requirements,
        workspace_bytes,
        spans: parsed.spans,
    })
}

fn configuration_diagnostic(
    error: ConfigurationError,
    node: &Node,
    spans: &SpanIndex,
) -> Diagnostic {
    match error {
        ConfigurationError::UnknownField { path, field } => Diagnostic::new(
            DiagnosticKind::UnknownField,
            path,
            node.span_at(&field),
            format!("unknown config field {field:?}"),
        ),
        ConfigurationError::UnresolvedRequirement { path, message } => {
            spans.at(DiagnosticKind::UnresolvedBound, path, message)
        }
        ConfigurationError::Invalid { path, message } => {
            Diagnostic::new(DiagnosticKind::Config, path, node.span, message)
        }
        ConfigurationError::SourceType { .. } | ConfigurationError::ConfigurationType { .. } => {
            Diagnostic::new(
                DiagnosticKind::Config,
                "$".to_owned(),
                node.span,
                format!("{error:?}"),
            )
        }
    }
}

struct Normalized {
    module: ParsedModule,
    config_nodes: BTreeMap<UnitId, Node>,
    spans: SpanIndex,
}

#[derive(Default)]
struct SpanIndex {
    paths: BTreeMap<String, SourceSpan>,
    producers: BTreeMap<ResourceId, String>,
}

impl SpanIndex {
    fn at(&self, kind: DiagnosticKind, path: String, message: impl Into<String>) -> Diagnostic {
        let mut ancestor = path.as_str();
        let span = loop {
            if let Some(span) = self.paths.get(ancestor) {
                break *span;
            }
            let Some(index) = ancestor.rfind('.') else {
                break SourceSpan::point(1, 1, 0);
            };
            ancestor = &ancestor[..index];
        };
        Diagnostic::new(kind, path, span, message)
    }

    fn unresolved_bound(&self, resource: &ResourceId) -> Diagnostic {
        let path = self
            .producers
            .get(resource)
            .cloned()
            .unwrap_or_else(|| "$".to_owned());
        self.at(
            DiagnosticKind::UnresolvedBound,
            path,
            format!(
                "no configuration, adapter, or host bound for {}",
                resource.as_str()
            ),
        )
    }

    fn graph_error(&self, error: CompileError) -> Diagnostic {
        let (kind, path) = match &error {
            CompileError::UnknownUnitType { unit, .. } => (
                DiagnosticKind::UnknownUnit,
                format!("$.units.{}.type", unit.as_str()),
            ),
            CompileError::MissingPort {
                unit, port, input, ..
            } => (
                DiagnosticKind::MissingPort,
                format!(
                    "$.units.{}.{}.{}",
                    unit.as_str(),
                    if *input { "inputs" } else { "outputs" },
                    port
                ),
            ),
            CompileError::DuplicateProducer { resource, .. } => (
                DiagnosticKind::DuplicateProducer,
                self.producers
                    .get(resource)
                    .cloned()
                    .unwrap_or_else(|| "$".to_owned()),
            ),
            CompileError::SemanticTypeMismatch { unit, port, .. }
            | CompileError::ConcreteBindingMismatch { unit, port, .. }
            | CompileError::ConcreteTypeMismatch { unit, port, .. } => (
                DiagnosticKind::TypeMismatch,
                format!("$.units.{}.inputs.{}", unit.as_str(), port),
            ),
            CompileError::Cycle { path } => (
                DiagnosticKind::Cycle,
                path.first().map_or_else(
                    || "$".to_owned(),
                    |unit| format!("$.units.{}", unit.as_str()),
                ),
            ),
            CompileError::UnknownResource { unit, port, .. } => (
                DiagnosticKind::Graph,
                format!("$.units.{}.inputs.{}", unit.as_str(), port),
            ),
            CompileError::UnknownModuleOutput { resource } => (
                DiagnosticKind::Graph,
                format!("$.outputs.{}", resource.as_str()),
            ),
            _ => (DiagnosticKind::Graph, "$".to_owned()),
        };
        self.at(kind, path, error.to_string())
    }
}

fn normalize(root: &Node) -> Result<Normalized, Diagnostic> {
    let root_entries = expect_mapping(root, "$", "Module Definition")?;
    reject_unknown(
        root_entries,
        "$",
        &["schema", "module", "inputs", "units", "outputs"],
    )?;
    let mut spans = SpanIndex::default();
    spans.paths.insert("$".to_owned(), root.span);
    let schema = required_string(root_entries, "$", "schema", &mut spans)?;
    if schema != SUPPORTED_SCHEMA {
        return Err(spans.at(
            DiagnosticKind::UnsupportedSchema,
            "$.schema".to_owned(),
            format!("unsupported schema {schema:?}; expected {SUPPORTED_SCHEMA}"),
        ));
    }
    let name = required_string(root_entries, "$", "module", &mut spans)?;
    let inputs_node = required(root_entries, "$", "inputs", &mut spans)?;
    let units_node = required(root_entries, "$", "units", &mut spans)?;
    let outputs_node = required(root_entries, "$", "outputs", &mut spans)?;
    let inputs = normalize_inputs(inputs_node, &mut spans)?;
    let (units, config_nodes) = normalize_units(units_node, &mut spans)?;
    let outputs = normalize_outputs(outputs_node, &mut spans)?;
    Ok(Normalized {
        module: ParsedModule {
            schema,
            name,
            inputs,
            units,
            outputs,
        },
        config_nodes,
        spans,
    })
}

fn normalize_inputs(
    node: &Node,
    spans: &mut SpanIndex,
) -> Result<Vec<ParsedModuleInput>, Diagnostic> {
    let entries = expect_mapping(node, "$.inputs", "inputs")?;
    let mut inputs = Vec::with_capacity(entries.len());
    for (key, value) in entries {
        let resource = expect_string(key, "$.inputs", "input name")?;
        let path = format!("$.inputs.{resource}");
        spans.paths.insert(path.clone(), value.span);
        spans
            .producers
            .insert(ResourceId::new(&resource), path.clone());
        let fields = expect_mapping(value, &path, "input declaration")?;
        reject_unknown(fields, &path, &["type"])?;
        let semantic = required_string(fields, &path, "type", spans)?;
        inputs.push(ParsedModuleInput {
            resource: ResourceId::new(resource),
            semantic_type: semantic_type(semantic, spans, format!("{path}.type"))?,
        });
    }
    inputs.sort_by(|left, right| left.resource.cmp(&right.resource));
    Ok(inputs)
}

fn normalize_units(
    node: &Node,
    spans: &mut SpanIndex,
) -> Result<(Vec<ParsedUnit>, BTreeMap<UnitId, Node>), Diagnostic> {
    let entries = expect_mapping(node, "$.units", "units")?;
    let mut units = Vec::with_capacity(entries.len());
    let mut configs = BTreeMap::new();
    for (key, value) in entries {
        let id_text = expect_string(key, "$.units", "Unit name")?;
        let id = UnitId::new(&id_text);
        let path = format!("$.units.{id_text}");
        spans.paths.insert(path.clone(), value.span);
        let fields = expect_mapping(value, &path, "Unit declaration")?;
        reject_unknown(fields, &path, &["type", "config", "inputs", "outputs"])?;
        let unit_type = required_string(fields, &path, "type", spans)?;
        let inputs = normalize_bindings(
            required(fields, &path, "inputs", spans)?,
            &format!("{path}.inputs"),
            spans,
            false,
        )?;
        let outputs = normalize_bindings(
            required(fields, &path, "outputs", spans)?,
            &format!("{path}.outputs"),
            spans,
            true,
        )?;
        let config = optional(fields, "config").cloned().unwrap_or_else(|| Node {
            span: value.span,
            kind: syntax::NodeKind::Mapping(Vec::new()),
        });
        spans.paths.insert(format!("{path}.config"), config.span);
        configs.insert(id.clone(), config);
        units.push(ParsedUnit {
            id,
            unit_type: UnitTypeName::new(unit_type),
            inputs,
            outputs,
        });
    }
    units.sort_by(|left, right| left.id.cmp(&right.id));
    Ok((units, configs))
}

fn normalize_bindings(
    node: &Node,
    path: &str,
    spans: &mut SpanIndex,
    producer: bool,
) -> Result<Vec<(String, ResourceId)>, Diagnostic> {
    let entries = expect_mapping(node, path, "port bindings")?;
    let mut bindings = Vec::with_capacity(entries.len());
    for (key, value) in entries {
        let port = expect_string(key, path, "port name")?;
        let resource = expect_string(value, &format!("{path}.{port}"), "Resource name")?;
        let child_path = format!("{path}.{port}");
        spans.paths.insert(child_path.clone(), value.span);
        if producer {
            spans
                .producers
                .insert(ResourceId::new(&resource), child_path);
        }
        bindings.push((port, ResourceId::new(resource)));
    }
    bindings.sort();
    Ok(bindings)
}

fn normalize_outputs(node: &Node, spans: &mut SpanIndex) -> Result<Vec<ResourceId>, Diagnostic> {
    let entries = expect_mapping(node, "$.outputs", "outputs")?;
    let mut outputs = Vec::with_capacity(entries.len());
    for (key, value) in entries {
        let name = expect_string(key, "$.outputs", "output name")?;
        let path = format!("$.outputs.{name}");
        spans.paths.insert(path.clone(), value.span);
        outputs.push(ResourceId::new(expect_string(
            value,
            &path,
            "Resource name",
        )?));
    }
    outputs.sort();
    Ok(outputs)
}

fn semantic_type(
    value: String,
    spans: &SpanIndex,
    path: String,
) -> Result<SemanticType, Diagnostic> {
    SemanticType::new(value)
        .map_err(|error| spans.at(DiagnosticKind::InvalidField, path, error.to_string()))
}

fn reject_unknown(
    entries: &[(Node, Node)],
    path: &str,
    allowed: &[&str],
) -> Result<(), Diagnostic> {
    let allowed: BTreeSet<_> = allowed.iter().copied().collect();
    for (key, _) in entries {
        let name = expect_string(key, path, "field name")?;
        if !allowed.contains(name.as_str()) {
            return Err(Diagnostic::new(
                DiagnosticKind::UnknownField,
                format!("{path}.{name}"),
                key.span,
                format!("unknown field {name:?}"),
            ));
        }
    }
    Ok(())
}

fn required<'a>(
    entries: &'a [(Node, Node)],
    path: &str,
    field: &str,
    spans: &mut SpanIndex,
) -> Result<&'a Node, Diagnostic> {
    let value = optional(entries, field).ok_or_else(|| {
        Diagnostic::new(
            DiagnosticKind::MissingField,
            format!("{path}.{field}"),
            entries
                .first()
                .map_or(SourceSpan::point(1, 1, 0), |(_, value)| value.span),
            format!("missing required field {field:?}"),
        )
    })?;
    spans.paths.insert(format!("{path}.{field}"), value.span);
    Ok(value)
}

fn required_string(
    entries: &[(Node, Node)],
    path: &str,
    field: &str,
    spans: &mut SpanIndex,
) -> Result<String, Diagnostic> {
    let value = required(entries, path, field, spans)?;
    expect_string(value, &format!("{path}.{field}"), field)
}

fn optional<'a>(entries: &'a [(Node, Node)], field: &str) -> Option<&'a Node> {
    entries
        .iter()
        .find(|(key, _)| key.as_string().as_deref() == Some(field))
        .map(|(_, value)| value)
}

fn expect_mapping<'a>(
    node: &'a Node,
    path: &str,
    label: &str,
) -> Result<&'a [(Node, Node)], Diagnostic> {
    node.mapping().ok_or_else(|| {
        Diagnostic::new(
            DiagnosticKind::InvalidField,
            path,
            node.span,
            format!("{label} must be a mapping"),
        )
    })
}

fn expect_string(node: &Node, path: &str, label: &str) -> Result<String, Diagnostic> {
    node.as_string().ok_or_else(|| {
        Diagnostic::new(
            DiagnosticKind::InvalidField,
            path,
            node.span,
            format!("{label} must be a string"),
        )
    })
}
