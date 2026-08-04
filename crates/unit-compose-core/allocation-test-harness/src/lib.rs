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

    use unit_compose_core::{
        AllocationCapability, AllocationDomain, AllocationDomainProbe, AllocationEvidence,
        AllocationOperations, BoundedPointFilter, BuildError, BuildOptions, DiagnosticSink,
        FailureDisposition, FixedImageFilter, ImageInput, Module, Point, PointInput,
        RequirementStatus, RunError, RunEvent, Unit, UnitWorkspace, ValueStorage, ValueWriter,
    };

    use super::GlobalProbe;

    #[derive(Default)]
    struct Sink {
        calls: usize,
    }

    impl DiagnosticSink for Sink {
        fn record(&mut self, _: RunEvent) {
            self.calls += 1;
        }
    }

    fn strict_capability() -> AllocationCapability {
        AllocationCapability::inspect(
            vec![AllocationDomain {
                name: "rust-global".into(),
                evidence: AllocationEvidence::Instrumented,
            }],
            true,
        )
    }

    struct AllocatingUnit;

    impl Unit for AllocatingUnit {
        type Input = ();
        type Storage = ValueStorage<usize>;

        fn workspace_requirement(&self) -> usize {
            0
        }
        fn output_storage(&self) -> Self::Storage {
            ValueStorage::new("result")
        }
        fn allocation_capability(&self) -> AllocationCapability {
            strict_capability()
        }
        fn run(
            &mut self,
            _: &(),
            output: &mut ValueWriter<'_, usize>,
            _: UnitWorkspace<'_>,
        ) -> Result<(), RunError> {
            let values = vec![1_u8; std::hint::black_box(32)];
            output.write(values.len());
            Ok(())
        }
    }

    struct DynamicUnit;

    impl Unit for DynamicUnit {
        type Input = ();
        type Storage = ValueStorage<()>;
        fn workspace_requirement(&self) -> usize {
            0
        }
        fn output_storage(&self) -> Self::Storage {
            ValueStorage::new("dynamic")
        }
        fn allocation_capability(&self) -> AllocationCapability {
            strict_capability()
        }
        fn requirement_status(&self) -> RequirementStatus {
            RequirementStatus::Dynamic
        }
        fn run(
            &mut self,
            _: &(),
            output: &mut ValueWriter<'_, ()>,
            _: UnitWorkspace<'_>,
        ) -> Result<(), RunError> {
            output.write(());
            Ok(())
        }
    }

    #[test]
    fn isolated_strict_allocation_conformance() {
        let mut module = Module::build(
            FixedImageFilter {
                fail: None,
                panic: false,
            },
            BuildOptions::strict(),
        )
        .unwrap();
        let input = ImageInput {
            pixels: [1, 2, 3, 4],
        };
        let mut probe = GlobalProbe;
        let mut sink = Sink::default();

        // Warm-up is deliberately outside the measured boundary.
        let _ = module.warm_up(&input).unwrap();
        for _ in 0..1_000 {
            let _ = module
                .run_profiled(&input, &mut [&mut probe], Some(&mut sink))
                .unwrap();
            assert_eq!(
                module.report().allocation_operations(),
                AllocationOperations::default()
            );
        }
        assert_eq!(sink.calls, 1_000);

        for disposition in [FailureDisposition::Recoverable, FailureDisposition::Fatal] {
            let mut failing = Module::build(
                FixedImageFilter {
                    fail: Some(disposition),
                    panic: false,
                },
                BuildOptions::strict(),
            )
            .unwrap();
            assert!(matches!(
                failing.run_profiled(&input, &mut [&mut probe], Some(&mut sink)),
                Err(RunError::Unit(_))
            ));
            assert_eq!(
                failing.report().allocation_operations(),
                AllocationOperations::default()
            );
        }

        let mut bounded =
            Module::build(BoundedPointFilter { maximum: 1 }, BuildOptions::strict()).unwrap();
        let overflowing = PointInput {
            points: vec![Point(1, 1), Point(2, 2)],
        };
        assert!(matches!(
            bounded.run_profiled(&overflowing, &mut [&mut probe], Some(&mut sink)),
            Err(RunError::Capacity(_))
        ));
        assert_eq!(
            bounded.report().allocation_operations(),
            AllocationOperations::default()
        );
        assert_eq!(bounded.report().observed_capacity_peak(), 1);

        let mut allocating = Module::build(AllocatingUnit, BuildOptions::strict()).unwrap();
        assert!(matches!(
            allocating.run_profiled(&(), &mut [&mut probe], Some(&mut sink)),
            Err(RunError::AllocationProfileViolation { .. })
        ));

        struct WrongProbe;
        impl AllocationDomainProbe for WrongProbe {
            fn domain(&self) -> &str {
                "uninstrumented-adapter"
            }
            fn begin(&mut self) {}
            fn finish(&mut self) -> AllocationOperations {
                AllocationOperations::default()
            }
        }
        let mut wrong = WrongProbe;
        assert!(matches!(
            module.run_profiled(&input, &mut [&mut wrong], None),
            Err(RunError::AllocationProfileViolation { .. })
        ));

        assert!(matches!(
            Module::build(DynamicUnit, BuildOptions::strict()),
            Err(BuildError::StrictRequirementUnavailable(
                RequirementStatus::Dynamic
            ))
        ));
    }
}
