use std::collections::BTreeSet;

use saphyr::{Scalar, ScalarOwned};
use saphyr_parser::{BufferedInput, Event, Parser, Span};

use crate::{Diagnostic, DiagnosticKind, ParseLimits, SourceSpan};

#[derive(Clone, Debug)]
pub(crate) struct Node {
    pub span: SourceSpan,
    pub kind: NodeKind,
}

#[derive(Clone, Debug)]
pub(crate) enum NodeKind {
    Scalar(ScalarOwned),
    Alias,
    Sequence(Vec<Node>),
    Mapping(Vec<(Node, Node)>),
}

enum Container {
    Sequence {
        start: SourceSpan,
        values: Vec<Node>,
    },
    Mapping {
        start: SourceSpan,
        values: Vec<(Node, Node)>,
        key: Option<Node>,
    },
}

pub(crate) fn parse(source: &str, limits: ParseLimits) -> Result<Node, Diagnostic> {
    if source.len() > limits.max_document_bytes {
        return Err(Diagnostic::new(
            DiagnosticKind::DocumentTooLarge,
            "$",
            SourceSpan::point(1, 1, 0),
            format!(
                "document is {} bytes; limit is {} bytes",
                source.len(),
                limits.max_document_bytes
            ),
        ));
    }
    let mut parser = Parser::new(BufferedInput::new(source.chars()));
    let mut stack = Vec::<Container>::new();
    let mut root = None;
    let mut documents = 0usize;
    while let Some(event) = parser.next_event() {
        let (event, span) = event.map_err(|error| {
            let marker = error.marker();
            Diagnostic::new(
                DiagnosticKind::Syntax,
                "$",
                SourceSpan::point(marker.line(), marker.col() + 1, marker.index()),
                error.info(),
            )
        })?;
        let source_span = SourceSpan::from_saphyr(span);
        match event {
            Event::DocumentStart(_) => documents += 1,
            Event::Alias(_) => push_node(
                &mut stack,
                &mut root,
                Node {
                    span: source_span,
                    kind: NodeKind::Alias,
                },
            )?,
            Event::Scalar(value, style, anchor, tag) => {
                let _ = anchor;
                let scalar = Scalar::parse_from_cow_and_metadata(value, style, tag.as_ref())
                    .ok_or_else(|| {
                        Diagnostic::new(
                            DiagnosticKind::Syntax,
                            "$",
                            source_span,
                            "invalid tagged scalar",
                        )
                    })?
                    .into_owned();
                push_node(
                    &mut stack,
                    &mut root,
                    Node {
                        span: source_span,
                        kind: NodeKind::Scalar(scalar),
                    },
                )?;
            }
            Event::SequenceStart(anchor, _) | Event::MappingStart(anchor, _) => {
                let _ = anchor;
                if stack.len() + 1 > limits.max_depth {
                    return Err(Diagnostic::new(
                        DiagnosticKind::DepthLimit,
                        "$",
                        source_span,
                        format!("parser depth exceeds limit {}", limits.max_depth),
                    ));
                }
                let container = if matches!(event, Event::SequenceStart(..)) {
                    Container::Sequence {
                        start: source_span,
                        values: Vec::new(),
                    }
                } else {
                    Container::Mapping {
                        start: source_span,
                        values: Vec::new(),
                        key: None,
                    }
                };
                stack.push(container);
            }
            Event::SequenceEnd | Event::MappingEnd => {
                let container = stack.pop().ok_or_else(|| {
                    Diagnostic::new(
                        DiagnosticKind::Syntax,
                        "$",
                        source_span,
                        "unexpected collection end",
                    )
                })?;
                let node = finish_container(container, source_span)?;
                push_node(&mut stack, &mut root, node)?;
            }
            Event::Nothing | Event::StreamStart | Event::StreamEnd | Event::DocumentEnd => {}
        }
    }
    if documents != 1 {
        return Err(Diagnostic::new(
            DiagnosticKind::Syntax,
            "$",
            SourceSpan::point(1, 1, 0),
            "exactly one YAML document is required",
        ));
    }
    let root = root.ok_or_else(|| {
        Diagnostic::new(
            DiagnosticKind::Syntax,
            "$",
            SourceSpan::point(1, 1, 0),
            "YAML document is empty",
        )
    })?;
    validate_mapping_keys(&root, "$".to_owned())?;
    Ok(root)
}

fn finish_container(container: Container, end: SourceSpan) -> Result<Node, Diagnostic> {
    match container {
        Container::Sequence { start, values } => Ok(Node {
            span: start.through(end),
            kind: NodeKind::Sequence(values),
        }),
        Container::Mapping { start, values, key } => {
            if let Some(key) = key {
                return Err(Diagnostic::new(
                    DiagnosticKind::Syntax,
                    "$",
                    key.span,
                    "mapping key has no value",
                ));
            }
            Ok(Node {
                span: start.through(end),
                kind: NodeKind::Mapping(values),
            })
        }
    }
}

fn push_node(
    stack: &mut [Container],
    root: &mut Option<Node>,
    node: Node,
) -> Result<(), Diagnostic> {
    match stack.last_mut() {
        Some(Container::Sequence { values, .. }) => values.push(node),
        Some(Container::Mapping { values, key, .. }) => {
            if let Some(map_key) = key.take() {
                values.push((map_key, node));
            } else {
                *key = Some(node);
            }
        }
        None if root.is_none() => *root = Some(node),
        None => {
            return Err(Diagnostic::new(
                DiagnosticKind::Syntax,
                "$",
                node.span,
                "multiple root values are not supported",
            ));
        }
    }
    Ok(())
}

fn validate_mapping_keys(node: &Node, path: String) -> Result<(), Diagnostic> {
    match &node.kind {
        NodeKind::Mapping(entries) => {
            let mut seen = BTreeSet::new();
            for (key, value) in entries {
                let key_text = key.as_string().ok_or_else(|| {
                    Diagnostic::new(
                        DiagnosticKind::InvalidField,
                        path.clone(),
                        key.span,
                        "mapping keys must be strings",
                    )
                })?;
                let child_path = format!("{path}.{key_text}");
                if key_text == "<<" {
                    return Err(Diagnostic::new(
                        DiagnosticKind::MergeKey,
                        child_path,
                        key.span,
                        "YAML merge keys are not supported",
                    ));
                }
                if !seen.insert(key_text) {
                    return Err(Diagnostic::new(
                        DiagnosticKind::DuplicateKey,
                        child_path,
                        key.span,
                        "duplicate mapping key",
                    ));
                }
                validate_mapping_keys(value, child_path)?;
            }
        }
        NodeKind::Sequence(values) => {
            for (index, value) in values.iter().enumerate() {
                validate_mapping_keys(value, format!("{path}[{index}]"))?;
            }
        }
        NodeKind::Alias => {
            return Err(Diagnostic::new(
                DiagnosticKind::Alias,
                path,
                node.span,
                "YAML aliases are not supported",
            ));
        }
        NodeKind::Scalar(_) => {}
    }
    Ok(())
}

impl Node {
    pub(crate) fn as_string(&self) -> Option<String> {
        match &self.kind {
            NodeKind::Scalar(ScalarOwned::String(value)) => Some(value.clone()),
            _ => None,
        }
    }

    pub(crate) fn mapping(&self) -> Option<&[(Node, Node)]> {
        match &self.kind {
            NodeKind::Mapping(entries) => Some(entries),
            _ => None,
        }
    }

    pub(crate) fn to_json(&self) -> Result<serde_json::Value, &'static str> {
        match &self.kind {
            NodeKind::Scalar(ScalarOwned::Null) => Ok(serde_json::Value::Null),
            NodeKind::Scalar(ScalarOwned::Boolean(value)) => Ok((*value).into()),
            NodeKind::Scalar(ScalarOwned::Integer(value)) => Ok((*value).into()),
            NodeKind::Scalar(ScalarOwned::FloatingPoint(value)) => {
                serde_json::Number::from_f64(**value).map_or(
                    Err("non-finite floating-point values are not supported"),
                    |number| Ok(serde_json::Value::Number(number)),
                )
            }
            NodeKind::Scalar(ScalarOwned::String(value)) => Ok(value.clone().into()),
            NodeKind::Alias => Err("YAML aliases are not supported"),
            NodeKind::Sequence(values) => values
                .iter()
                .map(Self::to_json)
                .collect::<Result<Vec<_>, _>>()
                .map(serde_json::Value::Array),
            NodeKind::Mapping(entries) => entries
                .iter()
                .map(|(key, value)| {
                    let key = key.as_string().ok_or("mapping keys must be strings")?;
                    Ok((key, value.to_json()?))
                })
                .collect::<Result<serde_json::Map<_, _>, _>>()
                .map(serde_json::Value::Object),
        }
    }

    pub(crate) fn span_at(&self, path: &str) -> SourceSpan {
        let mut node = self;
        for field in path.split('.') {
            let NodeKind::Mapping(entries) = &node.kind else {
                return self.span;
            };
            let Some((_, value)) = entries
                .iter()
                .find(|(key, _)| key.as_string().as_deref() == Some(field))
            else {
                return self.span;
            };
            node = value;
        }
        node.span
    }
}

impl SourceSpan {
    fn from_saphyr(span: Span) -> Self {
        Self {
            start_line: span.start.line(),
            start_column: span.start.col() + 1,
            start_offset: span.start.index(),
            end_line: span.end.line(),
            end_column: span.end.col() + 1,
            end_offset: span.end.index(),
        }
    }
}
