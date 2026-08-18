use proptest::prelude::*;
use unit_compose_core::{
    CompileError, ConcreteType, ParsedModule, ParsedModuleInput, ParsedUnit, PortDescriptor,
    ResolvedBinding, ResolvedModule, ResolvedModuleInput, ResolvedUnit, ResourceDescriptor,
    ResourceId, ResourceRegistry, SemanticType, UnitDescriptor, UnitId, UnitRegistry, UnitTypeName,
};

fn register(units: &mut UnitRegistry, descriptor: UnitDescriptor) {
    units
        .register::<(), (), _, _>(
            descriptor,
            |_, _| Ok(()),
            |_, _| Ok(unit_compose_core::UnitRequirements::default()),
        )
        .unwrap();
}

fn scalar() -> SemanticType {
    SemanticType::new("test.Scalar/v1").unwrap()
}

fn image() -> SemanticType {
    SemanticType::new("test.Image/v1").unwrap()
}

fn registries() -> (UnitRegistry, ResourceRegistry) {
    let mut resources = ResourceRegistry::default();
    resources
        .register(ResourceDescriptor::of::<u32>(scalar(), "value", "u32"))
        .unwrap();
    resources
        .register(ResourceDescriptor::of::<Vec<u8>>(
            image(),
            "buffer",
            "bytes",
        ))
        .unwrap();

    let mut units = UnitRegistry::default();
    register(
        &mut units,
        UnitDescriptor {
            type_name: UnitTypeName::new("test.map/v1"),
            inputs: vec![PortDescriptor::of::<u32>("in", scalar())],
            outputs: vec![PortDescriptor::of::<u32>("out", scalar())],
        },
    );
    register(
        &mut units,
        UnitDescriptor {
            type_name: UnitTypeName::new("test.join/v1"),
            inputs: vec![
                PortDescriptor::of::<u32>("left", scalar()),
                PortDescriptor::of::<u32>("right", scalar()),
            ],
            outputs: vec![PortDescriptor::of::<u32>("out", scalar())],
        },
    );
    (units, resources)
}

fn parsed(units: Vec<ParsedUnit>) -> ParsedModule {
    ParsedModule {
        schema: "unit-compose/v0alpha1".into(),
        name: "fixture".into(),
        inputs: vec![ParsedModuleInput {
            resource: ResourceId::new("source"),
            semantic_type: scalar(),
        }],
        units,
        outputs: vec![ResourceId::new("result")],
    }
}

fn map(id: &str, input: &str, output: &str) -> ParsedUnit {
    ParsedUnit {
        id: UnitId::new(id),
        unit_type: UnitTypeName::new("test.map/v1"),
        inputs: vec![("in".into(), ResourceId::new(input))],
        outputs: vec![("out".into(), ResourceId::new(output))],
    }
}

#[test]
fn source_order_permutations_normalize_to_structural_equality() {
    let (units, resources) = registries();
    let forward = parsed(vec![
        map("z_last", "middle", "result"),
        map("a_first", "source", "middle"),
    ])
    .resolve(&units, &resources)
    .unwrap()
    .compile()
    .unwrap();
    let reverse = parsed(vec![
        map("a_first", "source", "middle"),
        map("z_last", "middle", "result"),
    ])
    .resolve(&units, &resources)
    .unwrap()
    .compile()
    .unwrap();

    assert_eq!(forward, reverse);
    assert_eq!(
        forward.execution_order,
        vec![UnitId::new("a_first"), UnitId::new("z_last")]
    );
}

#[test]
fn compiled_names_and_ports_resolve_once_into_dense_typed_handles() {
    let (units, resources) = registries();
    let graph = parsed(vec![
        map("z_last", "middle", "result"),
        map("a_first", "source", "middle"),
    ])
    .resolve(&units, &resources)
    .unwrap()
    .compile()
    .unwrap();
    let dense = graph.into_dense(0x51a7).unwrap();

    assert_eq!(
        dense
            .execution_order
            .iter()
            .map(|index| dense.units[index.get()].id.as_str())
            .collect::<Vec<_>>(),
        ["a_first", "z_last"]
    );
    let last = &dense.units[dense.execution_order[1].get()];
    assert_eq!(last.inputs[0].port, "in");
    assert_eq!(
        dense.resources[last.inputs[0].resource.get()].id,
        ResourceId::new("middle")
    );

    let input = dense
        .input_handle::<u32>(&ResourceId::new("source"))
        .unwrap();
    let output = dense
        .output_handle::<u32>(&ResourceId::new("result"))
        .unwrap();
    assert_eq!(input.plan_token(), 0x51a7);
    assert_eq!(output.plan_token(), 0x51a7);
    assert_ne!(input.resource(), output.resource());
    assert!(matches!(
        dense.input_handle::<i32>(&ResourceId::new("source")),
        Err(unit_compose_core::HandleError::ConcreteType { .. })
    ));
    assert!(matches!(
        dense.input_handle::<u32>(&ResourceId::new("result")),
        Err(unit_compose_core::HandleError::NotModuleInput { .. })
    ));
    assert!(matches!(
        dense.output_handle::<u32>(&ResourceId::new("middle")),
        Err(unit_compose_core::HandleError::NotModuleOutput { .. })
    ));
}

#[test]
fn fan_out_fan_in_and_independent_roots_are_derived() {
    let (units, resources) = registries();
    let definition = ParsedModule {
        schema: "unit-compose/v0alpha1".into(),
        name: "branches".into(),
        inputs: vec![
            ParsedModuleInput {
                resource: ResourceId::new("source"),
                semantic_type: scalar(),
            },
            ParsedModuleInput {
                resource: ResourceId::new("other"),
                semantic_type: scalar(),
            },
        ],
        units: vec![
            map("right", "source", "right_value"),
            ParsedUnit {
                id: UnitId::new("join"),
                unit_type: UnitTypeName::new("test.join/v1"),
                inputs: vec![
                    ("right".into(), ResourceId::new("right_value")),
                    ("left".into(), ResourceId::new("left_value")),
                ],
                outputs: vec![("out".into(), ResourceId::new("result"))],
            },
            map("independent", "other", "unused"),
            map("left", "source", "left_value"),
        ],
        outputs: vec![ResourceId::new("result")],
    };
    let graph = definition
        .resolve(&units, &resources)
        .unwrap()
        .compile()
        .unwrap();

    assert_eq!(
        graph.execution_order,
        ["independent", "left", "right", "join"]
            .map(UnitId::new)
            .to_vec()
    );
    let source = graph
        .resources
        .iter()
        .find(|resource| resource.id == ResourceId::new("source"))
        .unwrap();
    assert_eq!(source.consumers.len(), 2);
    let join = graph
        .units
        .iter()
        .find(|unit| unit.id == UnitId::new("join"))
        .unwrap();
    assert_eq!(join.dependencies, ["left", "right"].map(UnitId::new));
}

#[test]
fn descriptions_are_stable_and_escape_identifiers() {
    let (units, resources) = registries();
    let graph = parsed(vec![map("unit one", "source", "result")])
        .resolve(&units, &resources)
        .unwrap()
        .compile()
        .unwrap();
    let description = &graph;

    let text = description.to_text();
    assert!(text.contains("execution: unit one"));
    assert!(text.contains("producer: unit one.out"));
    assert!(text.contains("consumers: [unit one.in]"));
    let dot = description.to_dot();
    assert!(dot.contains("\"unit one\""));
    assert!(dot.contains("resource_726573756c74"));
    assert!(dot.contains("class=\"unit\""));
    assert!(dot.contains("shape=parallelogram,style=bold,class=\"resource module-input\""));
    assert!(dot.contains("shape=doubleoctagon,style=bold,class=\"resource module-output\""));

    let mermaid = description.to_mermaid();
    assert!(mermaid.contains("unit_756e6974206f6e65"));
    assert!(mermaid.contains("resource_726573756c74"));
    assert!(mermaid.contains("<br/>Unit<br/>test.map/v1\"]:::unit"));
    assert!(mermaid.contains("<br/>Module input<br/>test.Scalar/v1\"/]:::moduleInput"));
    assert!(mermaid.contains("<br/>Module output<br/>test.Scalar/v1\"}}:::moduleOutput"));
}

#[test]
fn terminal_internal_resources_are_not_rendered_as_module_outputs() {
    let (units, resources) = registries();
    let graph = parsed(vec![
        map("published", "source", "result"),
        map("observed", "source", "internal_terminal"),
    ])
    .resolve(&units, &resources)
    .unwrap()
    .compile()
    .unwrap();
    let description = &graph;
    let internal_node = "resource_696e7465726e616c5f7465726d696e616c";

    let dot = description.to_dot();
    assert!(dot.contains(&format!(
        "\"{internal_node}\" [shape=ellipse,style=solid,class=\"resource\",label=\"internal_terminal\\nResource\\ntest.Scalar/v1\"]"
    )));
    assert!(!dot.contains(&format!("\"{internal_node}\" [shape=doubleoctagon")));

    let mermaid = description.to_mermaid();
    assert!(mermaid.contains(&format!(
        "{internal_node}([\"internal_terminal<br/>Resource<br/>test.Scalar/v1\"]):::resource"
    )));
    assert!(!mermaid.contains(&format!("{internal_node}{{{{")));
}

#[test]
fn duplicate_registration_preserves_the_original_descriptor() {
    let (mut units, mut resources) = registries();
    let unit_name = UnitTypeName::new("test.map/v1");
    let original_unit = units.get(&unit_name).unwrap().clone();
    assert!(matches!(
        units.register::<(), (), _, _>(
            UnitDescriptor {
                type_name: unit_name.clone(),
                inputs: vec![],
                outputs: vec![],
            },
            |_, _| Ok(()),
            |_, _| Ok(unit_compose_core::UnitRequirements::default()),
        ),
        Err(unit_compose_core::RegistrationError::DuplicateUnitType { .. })
    ));
    assert_eq!(units.get(&unit_name), Some(&original_unit));

    assert!(resources.get(&scalar()).unwrap().represents::<u32>());
    assert!(
        resources
            .register(ResourceDescriptor::of::<i32>(scalar(), "other", "i32"))
            .is_err()
    );
    assert!(resources.get(&scalar()).unwrap().represents::<u32>());
}

#[test]
fn canonical_registration_owns_typed_configuration_and_requirements() {
    #[derive(Debug, Eq, PartialEq)]
    struct Source(usize);
    #[derive(Debug, Eq, PartialEq)]
    struct Config(usize);

    let mut units = UnitRegistry::default();
    let unit_type = UnitTypeName::new("test.configured/v1");
    units
        .register::<Config, Source, _, _>(
            UnitDescriptor {
                type_name: unit_type.clone(),
                inputs: vec![],
                outputs: vec![],
            },
            |source, _| Ok(Config(source.0)),
            |config, _| {
                Ok(unit_compose_core::UnitRequirements {
                    output_capacities: Default::default(),
                    workspace_bytes: config.0,
                })
            },
        )
        .unwrap();

    let decoded = units
        .decode(&unit_type, &Source(17), "$.units.configured.config")
        .unwrap();
    assert_eq!(decoded.downcast_ref::<Config>(), Some(&Config(17)));
    assert_eq!(decoded.concrete_type(), ConcreteType::of::<Config>());
    assert_eq!(
        units
            .resolve_requirements(
                &decoded,
                &unit_compose_core::BoundSources::default(),
                "$.units.configured.config",
            )
            .unwrap()
            .workspace_bytes,
        17
    );

    assert!(matches!(
        units.decode(&unit_type, &17_usize, "$.units.configured.config"),
        Err(unit_compose_core::ConfigurationError::SourceType { .. })
    ));
}

#[test]
fn registered_factories_construct_source_map_join_and_fail_implementations() {
    #[derive(Clone, Copy)]
    struct Source(usize);
    #[derive(Clone, Copy)]
    struct Config(usize);
    struct SourceUnit(usize);
    struct MapUnit(usize);
    struct JoinUnit(usize);
    struct FailUnit;

    fn descriptor(name: &str) -> UnitDescriptor {
        UnitDescriptor {
            type_name: UnitTypeName::new(name),
            inputs: vec![],
            outputs: vec![],
        }
    }
    fn register_config(registry: &mut UnitRegistry, name: &str) {
        registry
            .register::<Config, Source, _, _>(
                descriptor(name),
                |source, _| Ok(Config(source.0)),
                |_, _| Ok(unit_compose_core::UnitRequirements::default()),
            )
            .unwrap();
    }

    let mut registry = UnitRegistry::default();
    for name in [
        "fixture.source/v1",
        "fixture.map/v1",
        "fixture.join/v1",
        "fixture.fail/v1",
    ] {
        register_config(&mut registry, name);
    }
    registry
        .register_factory::<Config, SourceUnit, _>(
            &UnitTypeName::new("fixture.source/v1"),
            |config| Ok(SourceUnit(config.0)),
        )
        .unwrap();
    registry
        .register_factory::<Config, MapUnit, _>(&UnitTypeName::new("fixture.map/v1"), |config| {
            Ok(MapUnit(config.0))
        })
        .unwrap();
    registry
        .register_factory::<Config, JoinUnit, _>(&UnitTypeName::new("fixture.join/v1"), |config| {
            Ok(JoinUnit(config.0))
        })
        .unwrap();
    registry
        .register_factory::<Config, FailUnit, _>(&UnitTypeName::new("fixture.fail/v1"), |_| {
            Ok(FailUnit)
        })
        .unwrap();

    let source_config = registry
        .decode(
            &UnitTypeName::new("fixture.source/v1"),
            &Source(3),
            "$.units.source.config",
        )
        .unwrap();
    let map_config = registry
        .decode(
            &UnitTypeName::new("fixture.map/v1"),
            &Source(5),
            "$.units.map.config",
        )
        .unwrap();
    let join_config = registry
        .decode(
            &UnitTypeName::new("fixture.join/v1"),
            &Source(7),
            "$.units.join.config",
        )
        .unwrap();
    let fail_config = registry
        .decode(
            &UnitTypeName::new("fixture.fail/v1"),
            &Source(0),
            "$.units.fail.config",
        )
        .unwrap();

    assert_eq!(
        registry
            .construct(&source_config)
            .unwrap()
            .downcast_ref::<SourceUnit>()
            .unwrap()
            .0,
        3
    );
    assert_eq!(
        registry
            .construct(&map_config)
            .unwrap()
            .downcast_ref::<MapUnit>()
            .unwrap()
            .0,
        5
    );
    assert_eq!(
        registry
            .construct(&join_config)
            .unwrap()
            .downcast_ref::<JoinUnit>()
            .unwrap()
            .0,
        7
    );
    assert!(
        registry
            .construct(&fail_config)
            .unwrap()
            .downcast_ref::<FailUnit>()
            .is_some()
    );

    assert!(matches!(
        registry.register_factory::<usize, SourceUnit, _>(
            &UnitTypeName::new("fixture.source/v1"),
            |value| Ok(SourceUnit(*value)),
        ),
        Err(unit_compose_core::RegistrationError::DuplicateFactory { .. })
    ));

    let mut mismatched = UnitRegistry::default();
    register_config(&mut mismatched, "fixture.mismatch/v1");
    assert!(matches!(
        mismatched.register_factory::<usize, SourceUnit, _>(
            &UnitTypeName::new("fixture.mismatch/v1"),
            |value| Ok(SourceUnit(*value)),
        ),
        Err(unit_compose_core::RegistrationError::FactoryConfigurationType { .. })
    ));
}

#[test]
fn negative_parsed_fixtures_report_required_ports_and_registry_failures() {
    let (units, resources) = registries();
    let mut missing = map("broken", "source", "result");
    missing.inputs.clear();
    assert!(matches!(
        parsed(vec![missing]).resolve(&units, &resources),
        Err(CompileError::MissingPort { port, input: true, .. }) if port == "in"
    ));

    let unknown = ParsedUnit {
        unit_type: UnitTypeName::new("missing.unit/v1"),
        ..map("broken", "source", "result")
    };
    assert!(matches!(
        parsed(vec![unknown]).resolve(&units, &resources),
        Err(CompileError::UnknownUnitType { .. })
    ));
}

fn binding(port: &str, resource: &str, semantic: SemanticType) -> ResolvedBinding {
    ResolvedBinding {
        port: port.into(),
        resource: ResourceId::new(resource),
        semantic_type: semantic,
        concrete_type: ConcreteType::of::<u32>(),
    }
}

fn resolved(units: Vec<ResolvedUnit>, outputs: &[&str]) -> ResolvedModule {
    ResolvedModule {
        schema: "unit-compose/v0alpha1".into(),
        name: "resolved-fixture".into(),
        inputs: vec![ResolvedModuleInput {
            resource: ResourceId::new("source"),
            semantic_type: scalar(),
            concrete_type: ConcreteType::of::<u32>(),
        }],
        units,
        outputs: outputs
            .iter()
            .map(|value| ResourceId::new(*value))
            .collect(),
    }
}

fn resolved_map(id: &str, input: &str, output: &str) -> ResolvedUnit {
    ResolvedUnit {
        id: UnitId::new(id),
        unit_type: UnitTypeName::new("test.map/v1"),
        inputs: vec![binding("in", input, scalar())],
        outputs: vec![binding("out", output, scalar())],
    }
}

#[test]
fn negative_resolved_fixtures_report_unknown_resource_and_duplicate_producer() {
    assert!(matches!(
        resolved(vec![resolved_map("unit", "missing", "result")], &["result"]).compile(),
        Err(CompileError::UnknownResource { resource, .. }) if resource == ResourceId::new("missing")
    ));
    assert!(matches!(
        resolved(
            vec![
                resolved_map("first", "source", "result"),
                resolved_map("second", "source", "result"),
            ],
            &["result"],
        )
        .compile(),
        Err(CompileError::DuplicateProducer { resource, .. }) if resource == ResourceId::new("result")
    ));
}

#[test]
fn semantic_and_concrete_mismatches_are_rejected() {
    let mut semantic_mismatch = resolved_map("unit", "source", "result");
    semantic_mismatch.inputs[0].semantic_type = image();
    assert!(matches!(
        resolved(vec![semantic_mismatch], &["result"]).compile(),
        Err(CompileError::SemanticTypeMismatch { .. })
    ));

    let mut concrete_mismatch = resolved_map("unit", "source", "result");
    concrete_mismatch.inputs[0].concrete_type = ConcreteType::of::<i32>();
    assert!(matches!(
        resolved(vec![concrete_mismatch], &["result"]).compile(),
        Err(CompileError::ConcreteBindingMismatch { .. })
    ));
}

#[test]
fn cycle_diagnostic_names_a_closed_actionable_path() {
    let graph = resolved(
        vec![
            resolved_map("alpha", "beta_value", "alpha_value"),
            resolved_map("beta", "alpha_value", "beta_value"),
        ],
        &["alpha_value"],
    );
    let error = graph.compile().unwrap_err();
    let CompileError::Cycle { path } = error else {
        panic!("expected cycle diagnostic");
    };
    assert_eq!(path.first(), path.last());
    assert_eq!(path, ["alpha", "beta", "alpha"].map(UnitId::new));
}

proptest! {
    #[test]
    fn chain_normalization_and_order_ignore_source_permutation(keys in prop::collection::vec(any::<u32>(), 1..24)) {
        let mut canonical: Vec<_> = (0..keys.len())
            .map(|index| {
                let id = format!("unit_{index:03}");
                let input = if index == 0 {
                    "source".to_owned()
                } else {
                    format!("value_{:03}", index - 1)
                };
                let output = format!("value_{index:03}");
                resolved_map(&id, &input, &output)
            })
            .collect();
        let expected = resolved(canonical.clone(), &[&format!("value_{:03}", keys.len() - 1)]).compile().unwrap();
        canonical.sort_by_key(|unit| keys[unit.id.as_str()[5..].parse::<usize>().unwrap()]);
        let actual = resolved(canonical, &[&format!("value_{:03}", keys.len() - 1)]).compile().unwrap();
        prop_assert_eq!(actual, expected);
    }

    #[test]
    fn generated_rings_preserve_cycles(size in 2usize..24, rotation in 0usize..24) {
        let units = (0..size)
            .map(|index| {
                let current = (index + rotation) % size;
                let previous = (current + size - 1) % size;
                resolved_map(&format!("unit_{current:03}"), &format!("value_{previous:03}"), &format!("value_{current:03}"))
            })
            .collect();
        let error = resolved(units, &["value_000"]).compile().unwrap_err();
        let CompileError::Cycle { path } = error else {
            return Err(TestCaseError::fail("ring was not retained as a cycle"));
        };
        prop_assert_eq!(path.first(), path.last());
        prop_assert_eq!(path.len(), size + 1);
    }
}
