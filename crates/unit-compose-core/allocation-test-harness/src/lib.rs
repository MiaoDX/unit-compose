//! Isolated conformance harness for the Rust global allocation domain.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use unit_compose_core::{AllocationDomainProbe, AllocationOperations};

thread_local! {
    static ACTIVE: Cell<bool> = const { Cell::new(false) };
    static ALLOCS: Cell<usize> = const { Cell::new(0) };
    static REALLOCS: Cell<usize> = const { Cell::new(0) };
    static DEALLOCS: Cell<usize> = const { Cell::new(0) };
}

struct CountingAllocator;

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ACTIVE.with(|active| {
            if active.get() {
                ALLOCS.set(ALLOCS.get() + 1);
            }
        });
        // SAFETY: forwarding the allocator contract unchanged to System.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        ACTIVE.with(|active| {
            if active.get() {
                DEALLOCS.set(DEALLOCS.get() + 1);
            }
        });
        // SAFETY: `ptr` and `layout` are those supplied by the caller.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        ACTIVE.with(|active| {
            if active.get() {
                REALLOCS.set(REALLOCS.get() + 1);
            }
        });
        // SAFETY: forwarding the allocator contract unchanged to System.
        unsafe { System.realloc(ptr, layout, size) }
    }
}

/// Scoped probe for the harness-owned Rust global allocator.
#[derive(Default)]
pub struct GlobalProbe;

impl AllocationDomainProbe for GlobalProbe {
    fn domain(&self) -> &str {
        "rust-global"
    }

    fn begin(&mut self) {
        ALLOCS.set(0);
        REALLOCS.set(0);
        DEALLOCS.set(0);
        ACTIVE.set(true);
    }

    fn finish(&mut self) -> AllocationOperations {
        ACTIVE.set(false);
        AllocationOperations {
            allocations: ALLOCS.get(),
            reallocations: REALLOCS.get(),
            deallocations: DEALLOCS.get(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use unit_compose_core::{
        AllocationCapability, AllocationDomain, AllocationEvidence, AllocationOperations,
        BuildOptions, ConcreteType, ExecutableDefinition, HostOutput, Module, ModuleInputs,
        ParsedModule, ParsedUnit, PortDescriptor, ResourceDescriptor, ResourceId, ResourceRegistry,
        ResourceRequirement, RunError, SemanticType, UnitDescriptor, UnitFailure, UnitId,
        UnitRegistry, UnitRequirements, UnitTypeName,
    };

    use super::GlobalProbe;

    #[derive(Clone)]
    struct Config {
        allocate: bool,
        fail: bool,
    }

    struct SourceUnit {
        allocate: bool,
        fail: bool,
    }

    fn prepared(allocate: bool, fail: bool) -> Module {
        let scalar = SemanticType::new("test.Scalar/v1").unwrap();
        let kind = UnitTypeName::new("test.source/v1");
        let mut resources = ResourceRegistry::default();
        resources
            .register(ResourceDescriptor::of::<u32>(
                scalar.clone(),
                "fixed scalar",
                "initialized",
            ))
            .unwrap();
        let mut units = UnitRegistry::default();
        units
            .register::<Config, Config, _, _>(
                UnitDescriptor {
                    type_name: kind.clone(),
                    inputs: vec![],
                    outputs: vec![PortDescriptor {
                        name: "out".to_owned(),
                        semantic_type: scalar,
                        concrete_type: ConcreteType::of::<u32>(),
                    }],
                },
                |source, _| Ok(source.clone()),
                |_, _| {
                    Ok(UnitRequirements {
                        output_capacities: BTreeMap::from([("out".to_owned(), 1)]),
                        workspace_bytes: 0,
                    })
                },
            )
            .unwrap();
        units
            .set_allocation_capability(
                &kind,
                AllocationCapability::inspect(
                    vec![AllocationDomain {
                        name: "rust-global".to_owned(),
                        evidence: AllocationEvidence::Instrumented,
                    }],
                    true,
                ),
            )
            .unwrap();
        units
            .register_factory::<Config, SourceUnit, _>(&kind, |config| {
                Ok(SourceUnit {
                    allocate: config.allocate,
                    fail: config.fail,
                })
            })
            .unwrap();
        units
            .register_executor::<SourceUnit, _>(&kind, |unit, invocation, _| {
                if unit.allocate {
                    let values = vec![1_u8; std::hint::black_box(32)];
                    invocation.write_value(0, values.len() as u32)?;
                } else {
                    invocation.write_value(0, 32_u32)?;
                }
                if unit.fail {
                    Err(RunError::Unit(UnitFailure::recoverable("expected failure")))
                } else {
                    Ok(())
                }
            })
            .unwrap();
        let parsed = ParsedModule {
            schema: "unit-compose/v0alpha1".to_owned(),
            name: "allocation-fixture".to_owned(),
            inputs: vec![],
            units: vec![ParsedUnit {
                id: UnitId::new("source"),
                unit_type: kind.clone(),
                inputs: vec![],
                outputs: vec![("out".to_owned(), ResourceId::new("result"))],
            }],
            outputs: vec![ResourceId::new("result")],
        };
        let graph = parsed
            .resolve(&units, &resources)
            .unwrap()
            .compile()
            .unwrap();
        let config = units
            .decode(&kind, &Config { allocate, fail }, "$.config")
            .unwrap();
        Module::build(
            ExecutableDefinition::new(
                graph,
                BTreeMap::from([(UnitId::new("source"), config)]),
                BTreeMap::from([(
                    ResourceId::new("result"),
                    ResourceRequirement { capacity: 1 },
                )]),
                BTreeMap::new(),
            ),
            &units,
            &resources,
            BuildOptions::strict(),
        )
        .unwrap()
    }

    #[test]
    fn isolated_dynamic_strict_allocation_conformance() {
        let inputs = ModuleInputs::default();
        let mut module = prepared(false, false);
        module.warm_up(&inputs).unwrap();
        let mut probe = GlobalProbe;
        for _ in 0..1_000 {
            module
                .run_profiled(&inputs, &mut [&mut probe], None)
                .unwrap();
            assert_eq!(
                module.report().allocation_operations(),
                AllocationOperations::default()
            );
        }

        let mut allocating = prepared(true, false);
        assert!(matches!(
            allocating.run_profiled(&inputs, &mut [&mut probe], None),
            Err(RunError::AllocationProfileViolation { .. })
        ));
    }

    #[test]
    fn dynamic_run_into_tracks_host_storage_validity() {
        let inputs = ModuleInputs::default();
        let mut module = prepared(false, false);
        let output = module
            .output_handle::<u32>(&ResourceId::new("result"))
            .unwrap();
        let mut target = HostOutput::new(0_u32);
        module
            .run_into(&inputs, &output, &mut target, |value, target| {
                *target = *value;
                Ok(())
            })
            .unwrap();
        assert_eq!(target.get(), Some(&32));

        let mut failing = prepared(false, true);
        let failed_output = failing
            .output_handle::<u32>(&ResourceId::new("result"))
            .unwrap();
        let failure = failing
            .run_into(&inputs, &failed_output, &mut target, |value, target| {
                *target = *value;
                Ok(())
            })
            .unwrap_err();
        let context = failure.context().unwrap();
        assert_eq!(context.module, "allocation-fixture");
        assert_eq!(context.unit.as_ref().unwrap().as_str(), "source");
        assert_eq!(
            context.unit_type.as_ref().unwrap().as_str(),
            "test.source/v1"
        );
        assert_eq!(
            context.disposition,
            Some(unit_compose_core::FailureDisposition::Recoverable)
        );
        assert!(matches!(failure.root_cause(), RunError::Unit(_)));
        assert!(!target.is_valid());

        module
            .run_into(&inputs, &output, &mut target, |value, target| {
                *target = *value + 1;
                Err(RunError::RuntimeBinding {
                    message: "copy failed".to_owned(),
                })
            })
            .unwrap_err();
        assert!(!target.is_valid());
        assert_eq!(*target.raw(), 33);
    }
}
