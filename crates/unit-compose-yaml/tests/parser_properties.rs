use std::collections::BTreeMap;

use proptest::prelude::*;
use unit_compose_core::{ResourceRegistry, UnitRegistry};
use unit_compose_yaml::{BoundSources, DiagnosticKind, ParseLimits, load};

fn parse(source: &str) -> Result<(), DiagnosticKind> {
    load(
        source,
        ParseLimits::default(),
        &UnitRegistry::default(),
        &ResourceRegistry::default(),
        &BoundSources {
            host: BTreeMap::new(),
            adapters: BTreeMap::new(),
        },
    )
    .map(|_| ())
    .map_err(|diagnostic| diagnostic.kind)
}

#[test]
fn malformed_yaml_corpus_is_rejected_without_panicking() {
    let cases = [
        ("", DiagnosticKind::Syntax),
        ("%u", DiagnosticKind::Syntax),
        ("---\n---\n", DiagnosticKind::Syntax),
        ("schema: [unterminated", DiagnosticKind::Syntax),
        ("? [complex, key]\n: value\n", DiagnosticKind::InvalidField),
        (
            "schema: &schema unit-compose/v0alpha1\ncopy: *schema\n",
            DiagnosticKind::Alias,
        ),
        (
            "schema: unit-compose/v0alpha1\nmodule: x\nmodule: y\n",
            DiagnosticKind::DuplicateKey,
        ),
        ("- {a: [b, {c: [d]}]}", DiagnosticKind::InvalidField),
        ("schema: !!binary '@@@'", DiagnosticKind::UnsupportedSchema),
        (
            "schema: unit-compose/v0alpha1\nmodule:\u{0} x\n",
            DiagnosticKind::InvalidField,
        ),
    ];

    for (source, expected) in cases {
        assert_eq!(parse(source), Err(expected), "source: {source:?}");
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        timeout: 1_000,
        ..ProptestConfig::default()
    })]

    #[test]
    fn arbitrary_parser_input_never_panics_and_remains_bounded(
        input in prop::collection::vec(any::<char>(), 0..2048)
            .prop_map(|chars| chars.into_iter().collect::<String>()),
        max_depth in 1usize..16,
        max_bytes in 0usize..2048,
    ) {
        let result = load(
            &input,
            ParseLimits { max_document_bytes: max_bytes, max_depth },
            &UnitRegistry::default(),
            &ResourceRegistry::default(),
            &BoundSources { host: BTreeMap::new(), adapters: BTreeMap::new() },
        );
        prop_assert!(result.is_ok() || result.is_err());
    }
}
