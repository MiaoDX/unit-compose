# Milestone 1 implementation contract

- Status: implemented and verified at the Milestone 1 exit gate
- Production dependencies: none; the graph compiler uses the standard library
- Test dependency: `proptest` exactly `1.7.0`, dev-only
- Graph representation: UnitCompose-owned normalized vectors and ordered maps
- Stable ordering: Kahn topological ordering with canonical `UnitId` tie-breaking
- Equality contract: `CompiledGraph` structural equality compares normalized
  identities, bindings, dependencies, Resources, Module outputs, and execution
  order; source declaration order is not retained

## Build boundaries

The programmatically constructible graph pipeline is:

```text
ParsedModule
  -> descriptor-backed resolution
ResolvedModule
  -> graph compilation
CompiledGraph
  -> fixed ModuleDescription views
```

`ParsedModule::resolve` validates registered Unit types, required and unknown
ports, duplicate port bindings, registered Resource semantic types, and the
concrete Rust representation registered for every port semantic type.

`ResolvedModule::compile` consumes only resolved identities and typed bindings.
It derives producers, consumers, and Unit dependencies; rejects duplicate Unit
identities and Resource producers, unknown Resources and Module outputs,
semantic and concrete binding mismatches, and cycles; then normalizes the graph
and computes a stable execution order. A cycle diagnostic contains a closed
canonical Unit path so the offending bindings can be located.

The resolved and compiled graph structures contain no YAML syntax value,
unvalidated Unit configuration, storage requirement, live range, slot,
workspace, runtime input, or prepared runtime state. YAML source spans may be
retained by a future frontend beside `ParsedModule`; they are not accepted by
the compiler.

## Inspection boundary

`CompiledGraph::description` exposes the fixed normalized structure and stable
text, DOT, and Mermaid exports. Renderers derive solely from the compiled graph
and do not grant access to Unit execution or Resource payloads.

## Evidence boundary

Milestone 1 tests cover source-order permutations, structural equality, fan-out,
fan-in, independent roots, descriptor and port failures, unknown Resources,
duplicate producers, semantic and concrete type mismatches, stable description
exports, randomized chain normalization and ordering, and randomized cycle
preservation.

Milestone 1 does not implement YAML parsing, configuration decoding, storage
requirements, live ranges, slot planning, allocation, runtime inputs, Unit
construction, or application lifecycle management. The Milestone 0 typed
execution and failure-safety kernel remains unchanged except for private
read-only Resource descriptor access used during resolution.
