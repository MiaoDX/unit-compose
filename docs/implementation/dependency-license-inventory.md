# Dependency and license inventory

Status: reviewed for the UnitCompose V0 workspace on 2026-08-05.

## Method and boundary

`Cargo.lock` is the exact supply-chain inventory. Every registry package below
has a crates.io index source and SHA-256 checksum in that lockfile. License
expressions were read from each locked package's normalized `Cargo.toml` in the
local Cargo registry. Workspace packages are MIT licensed, version `0.0.0`, and
`publish = false`.

The review used `cargo metadata --locked --format-version 1` to distinguish
direct runtime and development dependencies. `cargo-deny`, `cargo-license`, and
`cargo-audit` were not installed, and no global tools were installed for this
milestone. Consequently this is a deterministic metadata/license review, not a
vulnerability advisory scan or legal opinion. CI's `--locked` commands prevent
unreviewed resolution changes; a changed lockfile requires repeating this
inventory.

Direct runtime dependencies are `dyn-stack 0.13.0`, `saphyr 0.0.11`,
`saphyr-parser 0.0.11`, `serde 1.0.219`, `serde_ignored 0.1.12`, and
`serde_json 1.0.142`. The only direct third-party test dependency is
`proptest 1.7.0`. All other entries are transitive.

## Locked registry packages

| Packages | Declared license expression |
| --- | --- |
| arraydeque 0.5.1; quick-error 1.2.3; rusty-fork 0.3.1; wait-timeout 0.2.1 | MIT / Apache-2.0 |
| autocfg 1.5.1; bit-set 0.8.0; bit-vec 0.8.0; bitflags 2.13.1; cfg-if 1.0.4; errno 0.3.14; fastrand 2.5.0; getrandom 0.3.4, 0.4.3; hashbrown 0.17.1; hashlink 0.12.1; itoa 1.0.18; lazy_static 1.5.0; libc 0.2.189; num-traits 0.2.19; once_cell 1.21.4; ppv-lite86 0.2.21; proc-macro2 1.0.107; proptest 1.7.0; quote 1.0.47; rand 0.9.5; rand_chacha 0.9.0; rand_core 0.9.5; rand_xorshift 0.4.0; regex-syntax 0.8.11; saphyr 0.0.11; saphyr-parser 0.0.11; serde 1.0.219; serde_derive 1.0.219; serde_ignored 0.1.12; serde_json 1.0.142; syn 2.0.119, 3.0.3; tempfile 3.27.0; thiserror 2.0.19; thiserror-impl 2.0.19; unarray 0.1.4 | MIT OR Apache-2.0 |
| dyn-stack 0.13.0; ordered-float 5.3.0 | MIT |
| bytemuck 1.25.2 | Zlib OR Apache-2.0 OR MIT |
| foldhash 0.2.0 | Zlib |
| fnv 1.0.7 | Apache-2.0 / MIT |
| linux-raw-sys 0.12.1; rustix 1.1.4; wasip2 1.0.4+wasi-0.2.12 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| memchr 2.8.3 | Unlicense OR MIT |
| r-efi 5.3.0, 6.0.0 | MIT OR Apache-2.0 OR LGPL-2.1-or-later |
| ryu 1.0.23 | Apache-2.0 OR BSL-1.0 |
| unicode-ident 1.0.24 | (MIT OR Apache-2.0) AND Unicode-3.0 |
| windows-link 0.2.1; windows-sys 0.61.2 | MIT OR Apache-2.0 |
| wit-bindgen 0.57.1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| zerocopy 0.8.55; zerocopy-derive 0.8.55 | BSD-2-Clause OR Apache-2.0 OR MIT |

There are no git, path-outside-workspace, or unknown registry sources in the
lockfile. No license exception is required: all expressions provide a
permissive option compatible with the workspace's MIT distribution. Optional
copyleft or source-available alternatives in multi-license expressions are not
the selected license basis.

## Validation

Regenerate the package names and sources with:

```bash
cargo metadata --locked --format-version 1
rg -n '^source = |^checksum = ' Cargo.lock
```

Residual risk remains for undisclosed upstream licensing mistakes, withdrawn
crates, and security advisories unavailable without an advisory database tool.
