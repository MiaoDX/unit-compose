# Contributing to UnitCompose

UnitCompose is establishing its first executable baseline. Contributions should keep the public model small, make behavior testable, and preserve a clear separation between durable contracts and replaceable implementation choices.

## Read first

1. [Concept overview](docs/concepts/overview.md)
2. [Terminology](docs/concepts/terminology.md)
3. [Configuration-driven Resource DAG](docs/adr/0002-configuration-driven-resource-dag.md)
4. [Framework-managed Resource storage](docs/adr/0003-framework-managed-resource-storage.md)
5. [V0 architecture specification](docs/specification/v0-architecture.md)
6. [V0 implementation plan](docs/plans/v0-implementation-plan.md)

## Change categories

### Semantic change

A change to the Unit/Resource/Module model, Module Definition behavior, validation, execution, failure disposition, output lifetime, inspection behavior, storage guarantees, reload semantics, or compatibility direction requires an ADR update and a corresponding specification update.

### Implementation change

Rust traits, crate boundaries, containers, graph algorithms, storage-planner heuristics, dependency versions, error enums, and tracing mechanisms may change without an ADR when they preserve the current contract.

### Unit or Resource integration

A new Unit type should document:

- stable Unit type name;
- configuration and validation;
- required input and output ports;
- semantic Resource types and concrete Rust representations;
- fixed, bounded, or dynamic output requirements;
- scratch workspace requirements;
- private state and warm-up behavior;
- recoverable and fatal errors;
- declared allocation domains and support or certification evidence for the strict no-run-allocation profile;
- independent tests.

A Resource semantic type must use a stable namespaced identity. Rust `TypeId` may verify a registered concrete representation internally but is never the serialized identity.

### Research update

Research documents should:

- cite primary specifications, official documentation, or upstream source;
- separate source-backed facts from UnitCompose recommendations;
- identify which conclusions enter V0 and which remain deferred;
- avoid retaining abandoned design directions as current guidance.

## Documentation maintenance

The default branch documents the current design. When an ADR, specification, plan, or research document is no longer applicable, update the current canonical documents and remove the obsolete file. Git history and pull-request discussion retain the design history; the repository does not keep superseded placeholder documents solely for archival purposes.

## Design principles

- Keep the durable public model to Unit, Resource, and Module; expose inspection and diagnostics as read-only Module capabilities.
- Derive dependencies only from declared Resource bindings.
- Reject invalid compositions before Unit business code runs.
- Keep logical Resource identity separate from physical storage.
- Prefer framework-provided output storage and scratch workspace.
- Make strict steady-state allocation behavior measurable rather than aspirational.
- Do not expose a general Resource service locator to Unit code.
- Treat diagnostics, storage reports, and inspectability as product behavior.
- Add parallelism, persistence, plugins, language bindings, and device-memory complexity only after representative workloads require them.

## Pull requests

A pull request should explain:

- the problem being solved;
- whether observable semantics change;
- affected ADRs or specification requirements;
- research or executable evidence;
- validation performed;
- intentionally deferred work.
