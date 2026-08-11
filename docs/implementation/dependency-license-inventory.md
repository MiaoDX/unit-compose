# Dependency and license inventory

Status: reviewed for the UnitCompose workspace on 2026-08-11.

## Method and boundary

`Cargo.lock` is the exact supply-chain inventory. Every registry package below
has a crates.io index source and SHA-256 checksum in that lockfile. License
expressions come from the structured package metadata returned by
`cargo metadata --locked --format-version 1`; Cargo reads these values from
each locked package's normalized manifest. Workspace packages are MIT licensed,
version `0.0.0`, and `publish = false`.

The inventory contains 503 locked registry packages.
There are no packages with a missing license expression and no git or other
non-registry external sources. `cargo-deny`, `cargo-license`, and `cargo-audit`
were not installed, so this is a deterministic metadata/license review rather
than a vulnerability advisory scan or legal opinion.

Direct third-party runtime dependencies are `saphyr 0.0.11`,
`saphyr-parser 0.0.11`, `serde 1.0.219`,
`serde_ignored 0.1.12`, and `serde_json 1.0.142`. The default-off Rerun adapter
additionally depends on exact `re_sdk 0.24.1` and `re_types 0.24.1`. The only
direct third-party test dependency is `proptest 1.7.0`; all remaining entries
are transitive.

The two non-publishing registration showcases additionally pin the Apache-2.0
Kornia component crates at `0.1.14`: `kornia-3d`, `kornia-algebra`,
`kornia-image`, `kornia-imgproc`, `kornia-io`, and `kornia-tensor`. Their
notable transitive numerical and I/O packages include MIT `faer 0.20.1`,
MIT/Apache-2.0 `kiddo 5.3.3`, BSD-3-Clause `nalgebra 0.32.6`, and permissive
`wide`/`safe_arch`, PNG, TIFF, JPEG, and WebP stacks. `jpeg-encoder 0.6.1`
declares `(MIT OR Apache-2.0) AND IJG`. Showcase Rerun features retain exact
`re_sdk 0.24.1` and `re_types 0.24.1` pins.

## Locked registry packages

| Packages | Declared license expression |
| --- | --- |
| unicode-ident 1.0.24 | (MIT OR Apache-2.0) AND Unicode-3.0 |
| adler2 2.0.1 | 0BSD OR MIT OR Apache-2.0 |
| arrow 55.2.0; arrow-arith 55.2.0; arrow-array 55.2.0; arrow-buffer 55.2.0; arrow-cast 55.2.0; arrow-data 55.2.0; arrow-ipc 55.2.0; arrow-ord 55.2.0; arrow-row 55.2.0; arrow-schema 55.2.0; arrow-select 55.2.0; arrow-string 55.2.0; flatbuffers 25.12.19; prost 0.13.5; prost-derive 0.13.5; prost-types 0.13.5; sync_wrapper 1.0.2 | Apache-2.0 |
| fnv 1.0.7 | Apache-2.0 / MIT |
| ring 0.17.14 | Apache-2.0 AND ISC |
| ryu 1.0.23 | Apache-2.0 OR BSL-1.0 |
| rustls 0.23.43; rustls-native-certs 0.8.4 | Apache-2.0 OR ISC OR MIT |
| addr2line 0.25.1; atomic-waker 1.1.2; autocfg 1.5.1; bit-set 0.8.0; bit-vec 0.8.0; equivalent 1.0.2; fastrand 2.5.0; idna_adapter 1.2.1; indexmap 2.14.0; nohash-hasher 0.2.0; ntapi 0.4.3; object 0.37.3; pin-project 1.1.13; pin-project-internal 1.1.13; pin-project-lite 0.2.17; portable-atomic 1.14.0; portable-atomic-util 0.2.7; utf8_iter 1.0.4; utf8parse 0.2.2; uuid 1.18.1; zeroize 1.9.0 | Apache-2.0 OR MIT |
| linux-raw-sys 0.12.1; linux-raw-sys 0.4.15; rustix 0.38.44; rustix 1.1.4; wasi 0.11.1+wasi-snapshot-preview1; wasip2 1.0.4+wasi-0.2.12; wit-bindgen 0.57.1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| bytecount 0.6.9 | Apache-2.0/MIT |
| zerocopy 0.8.55; zerocopy-derive 0.8.55 | BSD-2-Clause OR Apache-2.0 OR MIT |
| subtle 2.6.1 | BSD-3-Clause |
| tiny-keccak 2.0.2 | CC0-1.0 |
| rustls-webpki 0.103.13; untrusted 0.9.0 | ISC |
| async-stream 0.3.6; async-stream-impl 0.3.6; atoi 2.0.0; axum 0.8.4; axum-core 0.5.6; bytes 1.12.1; cargo_metadata 0.14.2; cargo_metadata 0.18.1; cfb 0.7.3; comfy-table 7.1.4; convert_case 0.6.0; crossterm 0.28.1; crossterm_winapi 0.9.1; crunchy 0.2.4; generic-array 0.14.7; h2 0.4.15; http-body 1.1.0; http-body-util 0.1.4; hyper 1.11.0; hyper-util 0.1.20; infer 0.16.0; jsonwebtoken 9.3.1; libm 0.2.16; lz4_flex 0.11.6; mime_guess2 2.3.1; mio 1.2.2; natord 1.0.9; ordered-float 5.3.0; peg 0.6.3; peg-macros 0.6.3; peg-runtime 0.6.3; phf 0.11.3; phf_generator 0.11.3; phf_macros 0.11.3; phf_shared 0.11.3; ply-rs 0.1.3; pulldown-cmark 0.9.6; redox_syscall 0.5.18; schannel 0.1.29; slab 0.4.12; synstructure 0.13.2; sysinfo 0.30.13; tokio 1.53.1; tokio-macros 2.7.2; tokio-stream 0.1.19; tokio-util 0.7.19; tonic 0.13.1; tonic-web 0.13.1; tower 0.5.3; tower-http 0.6.11; tower-layer 0.3.3; tower-service 0.3.3; tracing 0.1.44; tracing-attributes 0.1.31; tracing-core 0.1.36; try-lock 0.2.5; twox-hash 2.1.3; want 0.3.1; winnow 0.7.15 | MIT |
| matchit 0.8.4 | MIT AND BSD-3-Clause |
| ahash 0.8.12; anstream 0.6.21; anstyle 1.0.14; anstyle-parse 0.2.7; anstyle-query 1.1.5; anstyle-wincon 3.0.11; anyhow 1.0.104; array-init 2.1.0; async-trait 0.1.91; backtrace 0.3.76; base64 0.22.1; bitflags 2.13.1; block-buffer 0.10.4; bumpalo 3.20.3; camino 1.1.12; cargo-platform 0.1.9; cc 1.4.0; cfg-if 1.0.4; chrono 0.4.45; clang-format 0.3.0; clean-path 0.2.1; colorchoice 1.0.5; const-random 0.1.18; const-random-macro 0.1.16; core-foundation 0.10.1; core-foundation-sys 0.8.7; cpufeatures 0.2.17; crossbeam 0.8.4; crossbeam-channel 0.5.16; crossbeam-deque 0.8.7; crossbeam-epoch 0.9.20; crossbeam-queue 0.3.13; crossbeam-utils 0.8.22; crypto-common 0.1.7; deranged 0.5.8; digest 0.10.7; displaydoc 0.2.7; document-features 0.2.12; either 1.17.0; emath 0.32.3; env_filter 0.1.4; env_filter 1.0.1; env_logger 0.11.9; errno 0.3.14; find-msvc-tools 0.1.9; form_urlencoded 1.2.2; futures-channel 0.3.33; futures-core 0.3.33; futures-io 0.3.33; futures-macro 0.3.33; futures-sink 0.3.33; futures-task 0.3.33; futures-util 0.3.33; getrandom 0.2.17; getrandom 0.3.4; getrandom 0.4.3; gimli 0.32.3; glob 0.3.4; half 2.7.1; hashbrown 0.15.5; hashbrown 0.17.1; hashlink 0.12.1; http 1.5.0; httparse 1.10.1; httpdate 1.0.3; hyper-timeout 0.5.2; iana-time-zone 0.1.65; iana-time-zone-haiku 0.1.2; idna 1.1.0; is_terminal_polyfill 1.70.2; itertools 0.14.0; itoa 1.0.18; js-sys 0.3.103; lazy_static 1.5.0; libc 0.2.189; litrs 1.0.0; lock_api 0.4.14; log 0.4.33; memory-stats 1.2.0; mime 0.3.17; ndarray 0.16.1; num 0.4.3; num-bigint 0.4.8; num-complex 0.4.6; num-conv 0.1.0; num-derive 0.4.2; num-integer 0.1.46; num-iter 0.1.46; num-rational 0.4.2; num-traits 0.2.19; once_cell 1.21.4; once_cell_polyfill 1.70.2; openssl-probe 0.2.1; parking_lot 0.12.5; parking_lot_core 0.9.12; percent-encoding 2.3.2; powerfmt 0.2.0; ppv-lite86 0.2.21; prettyplease 0.2.37; proc-macro2 1.0.107; proptest 1.7.0; puffin 0.19.1; quote 1.0.47; rand 0.8.7; rand 0.9.5; rand_chacha 0.3.1; rand_chacha 0.9.0; rand_core 0.6.4; rand_core 0.9.5; rand_xorshift 0.4.0; rayon 1.12.0; rayon-core 1.13.0; re_arrow_util 0.24.1; re_auth 0.24.1; re_build_info 0.24.1; re_build_tools 0.24.1; re_byte_size 0.24.1; re_case 0.24.1; re_chunk 0.24.1; re_error 0.24.1; re_format 0.24.1; re_format_arrow 0.24.1; re_grpc_client 0.24.1; re_grpc_server 0.24.1; re_log 0.24.1; re_log_encoding 0.24.1; re_log_types 0.24.1; re_memory 0.24.1; re_protos 0.24.1; re_sdk 0.24.1; re_smart_channel 0.24.1; re_sorbet 0.24.1; re_span 0.24.1; re_string_interner 0.24.1; re_tracing 0.24.1; re_tuid 0.24.1; re_types 0.24.1; re_types_builder 0.24.1; re_types_core 0.24.1; re_uri 0.24.1; regex 1.13.1; regex-automata 0.4.18; regex-syntax 0.8.11; rust-format 0.3.4; rustc_version 0.4.1; rustls-pki-types 1.15.1; rustversion 1.0.23; saphyr 0.0.11; saphyr-parser 0.0.11; scopeguard 1.2.0; security-framework 3.7.0; security-framework-sys 2.17.0; semver 1.0.26; serde 1.0.219; serde_derive 1.0.219; serde_ignored 0.1.12; serde_json 1.0.142; serde_spanned 0.6.9; sha2 0.10.9; shlex 2.0.1; smallvec 1.15.2; socket2 0.5.10; socket2 0.6.5; stable_deref_trait 1.2.1; static_assertions 1.1.0; syn 2.0.119; syn 3.0.3; tempfile 3.27.0; thiserror 1.0.69; thiserror 2.0.19; thiserror-impl 1.0.69; thiserror-impl 2.0.19; time 0.3.44; time-core 0.1.6; time-macros 0.2.24; tokio-rustls 0.26.4; toml 0.8.23; toml_datetime 0.6.11; toml_edit 0.22.27; typenum 1.20.1; unarray 0.1.4; unicase 2.9.0; unicode-segmentation 1.13.3; unicode-width 0.2.2; unicode-xid 0.2.6; unindent 0.2.4; url 2.5.8; wasm-bindgen 0.2.126; wasm-bindgen-futures 0.4.76; wasm-bindgen-macro 0.2.126; wasm-bindgen-macro-support 0.2.126; wasm-bindgen-shared 0.2.126; wasm-streams 0.4.2; web-sys 0.3.103; web-time 1.1.0; windows 0.52.0; windows-core 0.52.0; windows-core 0.62.2; windows-implement 0.60.2; windows-interface 0.59.3; windows-link 0.2.1; windows-result 0.4.1; windows-strings 0.5.1; windows-sys 0.52.0; windows-sys 0.59.0; windows-sys 0.61.2; windows-targets 0.52.6; windows_aarch64_gnullvm 0.52.6; windows_aarch64_msvc 0.52.6; windows_i686_gnu 0.52.6; windows_i686_gnullvm 0.52.6; windows_i686_msvc 0.52.6; windows_x86_64_gnu 0.52.6; windows_x86_64_gnullvm 0.52.6; windows_x86_64_msvc 0.52.6; xshell 0.2.7; xshell-macros 0.2.7 | MIT OR Apache-2.0 |
| r-efi 5.3.0; r-efi 6.0.0 | MIT OR Apache-2.0 OR LGPL-2.1-or-later |
| miniz_oxide 0.8.9 | MIT OR Zlib OR Apache-2.0 |
| android_system_properties 0.1.5; arraydeque 0.5.1; az 1.3.0; error-chain 0.12.4; fixed 1.29.0; itertools 0.10.5; lexical-core 1.0.6; lexical-parse-float 1.0.6; lexical-parse-integer 1.0.6; lexical-util 1.0.7; lexical-write-float 1.0.6; lexical-write-integer 1.0.6; linked-hash-map 0.5.6; log-once 0.4.1; matrixmultiply 0.3.11; quick-error 1.2.3; rawpointer 0.2.1; rustc-demangle 0.1.28; rusty-fork 0.3.1; siphasher 1.0.3; skeptic 0.13.7; tonic-web-wasm-client 0.7.1; version_check 0.9.5; wait-timeout 0.2.1; winapi 0.3.9; winapi-i686-pc-windows-gnu 0.4.0; winapi-x86_64-pc-windows-gnu 0.4.0 | MIT/Apache-2.0 |
| colored 2.2.0; indent 0.1.1 | MPL-2.0 |
| icu_collections 2.1.1; icu_locale_core 2.1.1; icu_normalizer 2.1.1; icu_normalizer_data 2.1.1; icu_properties 2.1.1; icu_properties_data 2.1.2; icu_provider 2.1.1; litemap 0.8.2; potential_utf 0.1.5; tinystr 0.8.3; writeable 0.6.3; yoke 0.8.3; yoke-derive 0.8.2; zerofrom 0.1.8; zerofrom-derive 0.1.7; zerotrie 0.2.4; zerovec 0.11.6; zerovec-derive 0.11.3 | Unicode-3.0 |
| aho-corasick 1.1.5; byteorder 1.5.0; jiff 0.2.15; jiff-static 0.2.15; jiff-tzdb 0.1.8; jiff-tzdb-platform 0.1.3; memchr 2.8.3; winapi-util 0.1.11 | Unlicense OR MIT |
| same-file 1.0.6; walkdir 2.5.0 | Unlicense/MIT |
| const_format 0.2.36; const_format_proc_macros 0.2.34; foldhash 0.2.0; konst 0.2.20; konst_macro_rules 0.2.19 | Zlib |
| bytemuck 1.25.2; bytemuck_derive 1.11.0 | Zlib OR Apache-2.0 OR MIT |

Most expressions provide MIT, Apache-2.0, BSD, ISC, Unicode, Zlib, CC0, or
another permissive option. `colored 2.2.0` and `indent 0.1.1` declare
MPL-2.0; redistribution must preserve that license's file-level terms for
those upstream packages. No GPL or AGPL package appears in the lockfile.

## Validation

Regenerate and validate the inventory with:

```bash
cargo metadata --locked --format-version 1
rg -n '^source = |^checksum = ' Cargo.lock
```

The generation query groups every registry package's exact name/version pair
by license expression. Its completion check requires 503 unique registry
packages, zero missing license expressions, and zero non-registry external
sources. Residual risk remains for undisclosed upstream licensing mistakes,
withdrawn crates, and advisories unavailable without an advisory database.
