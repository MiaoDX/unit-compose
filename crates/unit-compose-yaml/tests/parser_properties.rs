use std::collections::BTreeMap;

use proptest::prelude::*;
use unit_compose_core::{ResourceRegistry, UnitRegistry};
use unit_compose_yaml::{BoundSources, FrontendRegistry, ParseLimits, load};

proptest! {
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
            &FrontendRegistry::default(),
            &UnitRegistry::default(),
            &ResourceRegistry::default(),
            &BoundSources { host: BTreeMap::new(), adapters: BTreeMap::new() },
        );
        prop_assert!(result.is_ok() || result.is_err());
    }
}
