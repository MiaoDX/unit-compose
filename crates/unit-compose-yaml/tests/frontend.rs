use std::collections::BTreeMap;

use serde::Deserialize;
use unit_compose_core::{
    ConcreteType, PortDescriptor, ResourceDescriptor, ResourceId, ResourceRegistry, SemanticType,
    UnitDescriptor, UnitRegistry, UnitTypeName,
};
use unit_compose_yaml::{
    BoundSources, DiagnosticKind, ParseLimits, UnitRequirements, load, register_unit,
};

#[derive(Debug, Deserialize, Eq, PartialEq)]
struct Config {
    capacity: usize,
}

fn data() -> SemanticType {
    SemanticType::new("test.Data/v1").unwrap()
}

fn registries() -> (UnitRegistry, ResourceRegistry) {
    let mut resources = ResourceRegistry::default();
    resources
        .register(ResourceDescriptor::bounded_buffer::<u8>(
            data(),
            "test",
            "bounded bytes",
        ))
        .unwrap();
    let mut units = UnitRegistry::default();
    for name in ["test.map/v1", "test.other/v1"] {
        register_unit::<Config, _>(
            &mut units,
            UnitDescriptor {
                type_name: UnitTypeName::new(name),
                inputs: vec![PortDescriptor::of::<Vec<u8>>("in", data())],
                outputs: vec![PortDescriptor::of::<Vec<u8>>("out", data())],
            },
            |config, _| {
                Ok(UnitRequirements {
                    output_capacities: BTreeMap::from([("out".to_owned(), config.capacity)]),
                    workspace_bytes: config.capacity * 2,
                })
            },
        )
        .unwrap();
    }
    (units, resources)
}

fn bounds() -> BoundSources {
    BoundSources {
        host: BTreeMap::from([(ResourceId::new("raw"), 8)]),
        adapters: BTreeMap::new(),
    }
}

const VALID: &str = r#"
schema: unit-compose/v0alpha1
module: example
inputs:
  raw:
    type: test.Data/v1
units:
  filter:
    type: test.map/v1
    config:
      capacity: 4
    inputs:
      in: raw
    outputs:
      out: filtered
outputs:
  result: filtered
"#;

#[test]
fn decodes_typed_config_and_resolves_requirements_before_compile() {
    let (units, resources) = registries();
    let resolved = load(VALID, ParseLimits::default(), &units, &resources, &bounds()).unwrap();
    assert_eq!(
        resolved.config::<Config>(&unit_compose_core::UnitId::new("filter")),
        Some(&Config { capacity: 4 })
    );
    assert_eq!(resolved.requirements()[&ResourceId::new("raw")].capacity, 8);
    assert_eq!(
        resolved.requirements()[&ResourceId::new("filtered")].capacity,
        4
    );
    assert_eq!(resolved.workspace_bytes()[&"filter".into()], 8);
    let compiled = resolved.compile().unwrap();
    assert_eq!(compiled.graph.execution_order, vec!["filter".into()]);
}

#[test]
fn rejects_duplicate_keys_unknown_fields_aliases_and_merge_keys() {
    let (units, resources) = registries();
    for (source, kind, path) in [
        (
            VALID.replacen("module: example", "module: example\nmodule: again", 1),
            DiagnosticKind::DuplicateKey,
            "$.module",
        ),
        (
            VALID.replacen("module: example", "module: example\nsurprise: true", 1),
            DiagnosticKind::UnknownField,
            "$.surprise",
        ),
        (
            VALID.replacen("capacity: 4", "capacity: &cap 4\n      other: *cap", 1),
            DiagnosticKind::Alias,
            "$.units.filter.config.other",
        ),
        (
            VALID.replacen("capacity: 4", "<<: {}\n      capacity: 4", 1),
            DiagnosticKind::MergeKey,
            "$.units.filter.config.<<",
        ),
    ] {
        let error = load(
            &source,
            ParseLimits::default(),
            &units,
            &resources,
            &bounds(),
        )
        .err()
        .unwrap();
        assert_eq!(error.kind, kind, "{error}");
        assert_eq!(error.path, path);
        assert!(error.span.start_line > 0 && error.span.start_column > 0);
    }
}

#[test]
fn rejects_unknown_config_fields_through_serde() {
    let source = VALID.replacen("capacity: 4", "capacity: 4\n      typo: true", 1);
    let (units, resources) = registries();
    let error = load(
        &source,
        ParseLimits::default(),
        &units,
        &resources,
        &bounds(),
    )
    .err()
    .unwrap();
    assert_eq!(error.kind, DiagnosticKind::UnknownField);
    assert_eq!(error.path, "$.units.filter.config.typo");
    assert!(error.message.contains("typo"));
    assert_eq!(error.span.start_line, 12);
}

#[test]
fn enforces_size_and_depth_limits_before_normalization() {
    let (units, resources) = registries();
    let size = load(
        VALID,
        ParseLimits {
            max_document_bytes: 8,
            max_depth: 64,
        },
        &units,
        &resources,
        &bounds(),
    )
    .err()
    .unwrap();
    assert_eq!(size.kind, DiagnosticKind::DocumentTooLarge);

    let depth = load(
        VALID,
        ParseLimits {
            max_document_bytes: usize::MAX,
            max_depth: 2,
        },
        &units,
        &resources,
        &bounds(),
    )
    .err()
    .unwrap();
    assert_eq!(depth.kind, DiagnosticKind::DepthLimit);
    assert!(depth.span.start_line > 0);
}

#[test]
fn maps_resolution_and_graph_failures_to_actionable_paths_and_spans() {
    let (units, resources) = registries();
    let cases = [
        (
            VALID.replace("test.map/v1", "test.missing/v1"),
            DiagnosticKind::UnknownUnit,
            "$.units.filter.type",
        ),
        (
            VALID.replace("    inputs:\n      in: raw", "    inputs: {}"),
            DiagnosticKind::MissingPort,
            "$.units.filter.inputs.in",
        ),
        (
            VALID.replace(
                "outputs:\n  result: filtered",
                "outputs:\n  result: missing",
            ),
            DiagnosticKind::Graph,
            "$.outputs.missing",
        ),
    ];
    for (source, kind, path) in cases {
        let error = load(
            &source,
            ParseLimits::default(),
            &units,
            &resources,
            &bounds(),
        )
        .and_then(|resolved| resolved.compile())
        .err()
        .unwrap();
        assert_eq!(error.kind, kind, "{error}");
        assert_eq!(error.path, path);
        assert!(error.span.start_line > 0 && error.span.start_column > 0);
    }
}

#[test]
fn reports_duplicate_producer_type_mismatch_and_cycle_at_yaml_paths() {
    let duplicate = r#"
schema: unit-compose/v0alpha1
module: duplicate
inputs:
  raw: { type: test.Data/v1 }
units:
  a: { type: test.map/v1, config: { capacity: 4 }, inputs: { in: raw }, outputs: { out: shared } }
  b: { type: test.map/v1, config: { capacity: 4 }, inputs: { in: raw }, outputs: { out: shared } }
outputs: { result: shared }
"#;
    let cycle = r#"
schema: unit-compose/v0alpha1
module: cycle
inputs: {}
units:
  a: { type: test.map/v1, config: { capacity: 4 }, inputs: { in: b_out }, outputs: { out: a_out } }
  b: { type: test.map/v1, config: { capacity: 4 }, inputs: { in: a_out }, outputs: { out: b_out } }
outputs: { result: a_out }
"#;
    let (units, resources) = registries();
    for (source, kind, path) in [
        (
            duplicate,
            DiagnosticKind::DuplicateProducer,
            "$.units.b.outputs.out",
        ),
        (cycle, DiagnosticKind::Cycle, "$.units.a"),
    ] {
        let error = load(
            source,
            ParseLimits::default(),
            &units,
            &resources,
            &bounds(),
        )
        .and_then(|resolved| resolved.compile())
        .err()
        .unwrap();
        assert_eq!(error.kind, kind, "{error}");
        assert_eq!(error.path, path);
        assert!(error.span.start_line > 0);
    }

    let mut mismatched_units = UnitRegistry::default();
    register_unit::<Config, _>(
        &mut mismatched_units,
        UnitDescriptor {
            type_name: UnitTypeName::new("test.map/v1"),
            inputs: vec![PortDescriptor {
                name: "in".to_owned(),
                semantic_type: data(),
                concrete_type: ConcreteType::of::<u16>(),
            }],
            outputs: vec![PortDescriptor::of::<Vec<u8>>("out", data())],
        },
        |config, _| {
            Ok(UnitRequirements {
                output_capacities: BTreeMap::from([("out".to_owned(), config.capacity)]),
                workspace_bytes: 0,
            })
        },
    )
    .unwrap();
    let mismatch = load(
        VALID,
        ParseLimits::default(),
        &mismatched_units,
        &resources,
        &bounds(),
    )
    .err()
    .unwrap();
    assert_eq!(mismatch.kind, DiagnosticKind::TypeMismatch);
    assert_eq!(mismatch.path, "$.units.filter.inputs.in");
    assert!(mismatch.span.start_line > 0);

    let other = SemanticType::new("test.Other/v1").unwrap();
    let mut semantic_resources = ResourceRegistry::default();
    for semantic in [data(), other.clone()] {
        semantic_resources
            .register(ResourceDescriptor::bounded_buffer::<u8>(
                semantic, "test", "bounded",
            ))
            .unwrap();
    }
    let mut semantic_units = UnitRegistry::default();
    register_unit::<Config, _>(
        &mut semantic_units,
        UnitDescriptor {
            type_name: UnitTypeName::new("test.source/v1"),
            inputs: vec![],
            outputs: vec![PortDescriptor::of::<Vec<u8>>("out", other)],
        },
        config_requirements,
    )
    .unwrap();
    register_unit::<Config, _>(
        &mut semantic_units,
        UnitDescriptor {
            type_name: UnitTypeName::new("test.map/v1"),
            inputs: vec![PortDescriptor::of::<Vec<u8>>("in", data())],
            outputs: vec![PortDescriptor::of::<Vec<u8>>("out", data())],
        },
        config_requirements,
    )
    .unwrap();
    let semantic_source = r#"
schema: unit-compose/v0alpha1
module: mismatch
inputs: {}
units:
  source: { type: test.source/v1, config: { capacity: 4 }, inputs: {}, outputs: { out: value } }
  sink: { type: test.map/v1, config: { capacity: 4 }, inputs: { in: value }, outputs: { out: result } }
outputs: { result: result }
"#;
    let semantic_error = load(
        semantic_source,
        ParseLimits::default(),
        &semantic_units,
        &semantic_resources,
        &BoundSources::default(),
    )
    .unwrap()
    .compile()
    .err()
    .unwrap();
    assert_eq!(semantic_error.kind, DiagnosticKind::TypeMismatch);
    assert_eq!(semantic_error.path, "$.units.sink.inputs.in");
    assert_eq!(semantic_error.span.start_line, 7);
    assert!(semantic_error.span.start_column > 0);
}

#[test]
fn reports_unresolved_bounds_at_the_producing_path() {
    let (units, resources) = registries();
    let error = load(
        VALID,
        ParseLimits::default(),
        &units,
        &resources,
        &BoundSources::default(),
    )
    .err()
    .unwrap();
    assert_eq!(error.kind, DiagnosticKind::UnresolvedBound);
    assert_eq!(error.path, "$.inputs.raw");
    assert!(error.span.start_line > 0);
}

#[test]
fn normalization_is_independent_of_mapping_and_unit_source_order() {
    let permuted = r#"
outputs: { result: filtered }
units:
  filter:
    outputs: { out: filtered }
    inputs: { in: raw }
    config: { capacity: 4 }
    type: test.map/v1
inputs: { raw: { type: test.Data/v1 } }
module: example
schema: unit-compose/v0alpha1
"#;
    let (units, resources) = registries();
    let first = load(VALID, ParseLimits::default(), &units, &resources, &bounds())
        .unwrap()
        .compile()
        .unwrap();
    let second = load(
        permuted,
        ParseLimits::default(),
        &units,
        &resources,
        &bounds(),
    )
    .unwrap()
    .compile()
    .unwrap();
    assert_eq!(first.graph, second.graph);
    assert_eq!(first.requirements, second.requirements);
    assert_eq!(first.workspace_bytes, second.workspace_bytes);
}

fn config_requirements(config: &Config, _: &BoundSources) -> Result<UnitRequirements, String> {
    Ok(UnitRequirements {
        output_capacities: BTreeMap::from([("out".to_owned(), config.capacity)]),
        workspace_bytes: 0,
    })
}
