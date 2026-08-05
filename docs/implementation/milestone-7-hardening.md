# Milestone 7 hardening and V0 release readiness

Status: implemented; local deterministic gates passed on 2026-08-05.

## Regression evidence

Existing milestone suites remain the primary owners of graph normalization,
typed storage, pending-output failure safety, strict allocation, YAML
resolution, navigation execution, inspection, and reload semantics. Hardening
adds only material gaps:

- a malformed YAML corpus alongside the bounded arbitrary-input property test;
- 24 alternating successful/failed reload cycles that prove failed candidates
  retain the active Module and successful candidates atomically replace it;
- an ignored std-only benchmark harness for build time, no-op Unit execution,
  bounded writes, complete pending-output publication, workspace allocation,
  and compatible slot reuse.

The benchmark is observational. It prints iteration counts and wall-clock
durations, asserts only that the operations ran, and neither gates correctness
nor justifies an optimization without representative repeated measurements.
One local debug-profile smoke run observed: 1,000 builds in 129 microseconds,
10,000 no-op runs in 2.85 milliseconds, 10,000 sets of 32 bounded writes in
4.88 milliseconds, 10,000 pending publications in 131 microseconds, 10,000
workspace allocations in 697 microseconds, and 1,000 32-Resource slot plans in
44.5 milliseconds. These environment-specific observations are not baselines.

## Platform, panic, and unsafe boundaries

The supported V0 matrix is Linux x86_64 and `aarch64-unknown-linux-gnu`, with
Rust 1.85.1 and a committed lockfile. CI performs locked all-target/all-feature
checks on both, and the native ARM64 runner executes the full tests, isolated
allocation conformance, and all three strict navigation products.

The supported execution profile uses unwind semantics. A separate
`RUSTFLAGS="-C panic=abort" cargo check --workspace --lib --bins --all-features --locked`
proves the workspace compiles under abort semantics; it does not claim cleanup
or poisoning after an abort because the process terminates. Test targets are
excluded because stable Cargo rejects abort-mode test harnesses without the
nightly-only `-Zpanic_abort_tests` option.

An exact production search finds no `unsafe` blocks, functions, traits, or
implementations in `crates/*/src` or `examples/navigation-planning/src`. The
only workspace unsafe code is the test-only global allocator implementation in
`crates/unit-compose-core/allocation-test-harness/src/lib.rs`, where the three
`GlobalAlloc` calls delegate directly to `System` and counters are scoped by a
thread-local enable flag. The harness explicitly permits unsafe code and runs
single-threaded in CI. `dyn-stack` owns dependency-internal unsafe code for
aligned workspace backing.

Miri was not run locally: the pinned 1.85.1 toolchain has no `miri` component,
and this milestone does not install or change global toolchains. The exact
probe `cargo miri --version` reports that the `miri` component providing
`cargo-miri` is unavailable for `1.85.1-x86_64-unknown-linux-gnu`. The
fallback evidence is the production unsafe search, ordinary tests of pending
drop/unwind behavior, over-aligned workspace tests, and isolation of the only
workspace unsafe implementation to the allocator harness. Dependency-owned
unsafe remains the residual Miri/audit boundary.

## Release boundary

The workspace remains non-publishing (`publish = false`) and core remains free
of ROS, Rerun, datasets, and application-framework dependencies. V0 provides a
buildable workspace, runnable navigation products, API docs, inspection views,
CI, and the lockfile-derived [dependency/license inventory](dependency-license-inventory.md).
Crate publication, release archives, and the nuScenes showcase remain outside
V0. The optional Rerun adapter was implemented later as a default-off example
feature and remains outside the original V0 completion gate.

No terminology inconsistency was found across README, CONTRIBUTING, concepts,
ADRs, the specification, or current API docs. The stale core crate overview was
updated to describe the complete V0 surface, and README/CONTRIBUTING now expose
the runnable product and verification routes.

## Local proof commands

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-targets --all-features --locked
cargo test --doc --workspace --all-features --locked
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps --locked
cargo check --workspace --all-targets --all-features --target aarch64-unknown-linux-gnu --locked
cargo test -p unit-compose-allocation-test-harness --locked -- --test-threads=1
cargo test -p unit-compose-core --test hardening_benchmarks --locked -- --ignored --nocapture
```
