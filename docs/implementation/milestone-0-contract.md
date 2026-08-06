# Milestone 0 implementation contract

- Status: frozen at the Milestone 0 exit gate
- MSRV and pinned toolchain: Rust 1.85.1 (the first stable release supporting Rust 2024)
- Rust edition: 2024
- Primary host: Linux x86_64 (`x86_64-unknown-linux-gnu`)
- ARM64 target: Linux GNU ARM64 (`aarch64-unknown-linux-gnu`)
- ARM64 expectation: cross-compile every workspace target in ordinary CI and run the full workspace tests on a native Linux ARM64 runner
- Dependencies: none; the spike uses only the Rust standard library
- Feature set: no Cargo features
- Lockfile: `Cargo.lock` is committed and CI uses `--locked`; this workspace is an integrated application/framework repository rather than a crates.io library release
- Publishing: workspace packages are `publish = false`
- Unsafe policy: forbidden in the core crate
- Panic contract: CI and the supported library profile use unwind semantics; `panic=abort` terminates and provides no cleanup or poisoning guarantee

The exact versions and target above are intentionally conservative project
metadata, not a promise that later milestones can silently lower the MSRV or
replace the ARM64 execution gate. Any such change must be reviewed explicitly.

## Evidence boundary

Milestone 0 proves the typed authoring and runtime contract with synthetic
Units. It does not implement YAML, a general graph compiler, live-range slot
planning, allocation instrumentation, navigation examples, ROS, Rerun, or
application lifecycle management.

The strict allocation capability is deliberately honest: descriptors expose
each declared allocation domain and whether it is instrumented, explicitly
certified by a named source, or unsupported. The framework can reject an
unsupported declared domain, but completeness of declarations and certification
remains a trusted integrator assertion and is not mechanically provable.
