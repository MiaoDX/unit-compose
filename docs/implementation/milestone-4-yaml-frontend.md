# Milestone 4: YAML frontend

Milestone 4 adds `unit-compose-yaml`, a bounded frontend for the
`unit-compose/v0alpha1` Module Definition schema. It preserves source spans in
a private syntax tree before normalization, retains mapping entries long enough
to reject duplicate keys, rejects aliases and merge keys, and applies document
size and parser depth limits before typed work.

The dependency review selected and pinned:

- `saphyr` and `saphyr-parser` 0.0.11: MSRV 1.85.0, span-bearing parser events,
  YAML 1.2 parsing, MIT or Apache-2.0;
- `serde` 1.0.219: typed Unit configuration decoding and
  `deny_unknown_fields` support, MIT or Apache-2.0;
- `serde_ignored` 0.1.12: registry-enforced unknown config-field detection even
  for config types without `deny_unknown_fields`, MIT or Apache-2.0;
- `serde_json` 1.0.142: a frontend-private Serde value bridge after syntax and
  schema validation, MIT or Apache-2.0.

Saphyr's convenience mapping loader is deliberately not the validation
boundary because a map cannot retain duplicate entries. The frontend consumes
the event API into a private ordered mapping representation instead. Neither
that syntax representation nor `serde_json::Value` is exposed by the crate.

The host registers a typed Serde config and a deterministic requirements
resolver for every YAML-visible Unit type. Requirement resolution receives only
the typed config and `BoundSources`, whose adapter and host maps implement the
accepted config/adapter/host precedence. `load` returns `ResolvedDefinition`,
which contains the existing core `ResolvedModule`, type-erased but validated
configs, and numeric resource/workspace requirements. Graph compilation sees
only the core resolved IR. Storage planning receives only the compiled graph
and numeric `ResourceRequirement` values.

Diagnostics carry a YAML path and start/end source span. Resolution and graph
errors are mapped back to the recorded Unit, port, Resource producer, or Module
output path. Tests cover strict schema failures, bounds, graph errors,
permutation stability, and parser property testing without Unit construction or
execution.
