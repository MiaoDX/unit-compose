# Milestone 2 typed storage kernel

Status: implemented

The Resource descriptor is the sole authority for concrete type, storage
representation, element layout and alignment, memory class, adapter,
initialization, reset, validation, and drop behavior. Unit-resolved
`ResourceRequirement` values contain capacity only and therefore cannot
override those invariants.

The kernel provides fixed values, exact-length typed buffers, and bounded
variable-length typed buffers. Rust `Option` and `Vec` initialization state is
the initialized-range guard: only initialized values are dropped on success,
Unit error, complete-set validation error, or unwind. Writers cannot publish;
the Module exposes views only after the complete pending set validates.

Prepared Module inputs are validated as a complete named set for semantic type,
concrete type, capacity bound, and prepared-plan token before output reset or
Unit execution. Input rejection is recoverable. An unwind after execution has
started drops pending values and poisons the Module.

Sequential live ranges are inclusive. A Module output ends at the synthetic
run-end step. First-fit storage reuse requires descriptor-compatible typed
representations, sufficient capacity, and disjoint live ranges. The report
lists logical-to-physical assignments and sums allocated slot bytes as the
estimated peak. Cross-type raw arena packing is intentionally absent.

`WorkspaceBacking` wraps `dyn-stack` behind UnitCompose-owned requirement and
backing types, preserving typed alignment without exposing it as the Resource
storage model. Borrowed outputs prevent a mutable rerun. `run_into` uses an
explicit validity bit: caller storage is invalidated and may be mutated on
entry, and becomes logically published only after the complete run succeeds.

The focused tests contain no crate-owned unsafe code and are suitable for Miri.
The dependency-owned unsafe implementation of aligned stack backing remains
outside UnitCompose's unsafe boundary.
