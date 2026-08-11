# Kornia registration showcases

- **Status:** DONE
- **Date:** 2026-08-10
- **Completed:** 2026-08-11
- **Route after approval:** `$intuitive-flow`

## Goal

Add two optional, visually strong UnitCompose examples while keeping algorithms,
datasets, and generated recordings out of the core framework and repository:

1. image registration with Kornia ORB, seeded homography RANSAC, and warping;
2. point-cloud registration with Kornia ICP and residual evaluation.

The existing navigation example remains the strict-allocation reference. These
showcases run in best-effort mode because Kornia may allocate internally.

## Decisions

- Add two non-publishing packages: `examples/image-registration` and
  `examples/point-cloud-registration`.
- Pin stable Kornia `0.1.14` crates; keep Rerun pinned at `0.24.1`.
- Keep Units, Resources, snapshots, CLI code, and Rerun logging domain-local.
  Share no Rust helper until concrete duplication exists.
- Use one explicit `scripts/fetch-showcase-data.sh` command. Binaries never
  download data, and normal builds and tests never access the network.
- Download into `target/demo-data`, verify SHA-256 before atomic replacement,
  and keep datasets, derived outputs, and `.rrd` files untracked.
- Use deterministic synthetic fixtures in tests and tolerance-based numerical
  assertions rather than cross-platform byte equality.

## Presentation contract

Each Demo produces one complete visualization group with the same CLI surface
as the navigation example:

- `--inspect mermaid`: the canonical Unit/Resource topology;
- `--timed-mermaid`: the same topology annotated with `avg`, `p99`, and sample
  count from repeated completed runs;
- `--rerun-save <path.rrd>` and `--rerun-spawn`: a fixed domain-result view.

Mermaid explains pipeline structure and Unit timing; Rerun shows domain inputs,
intermediate values, and registration quality. Generated recordings remain
untracked and reproducible from the fetch script and documented commands.

## Execution

### Phase 0 evidence (2026-08-10)

- The requested Kornia component crates exist at `0.1.14`, are Apache-2.0,
  declare Rust 1.82, and expose ORB detection/matching, seeded homography
  RANSAC, perspective warp, ICP, point transforms, JPEG decode, and binary PCD
  decode. There is no umbrella `kornia` crate at `0.1.14`.
- Deterministic synthetic probes pass with image reprojection
  RMSE below `1e-4` and point-cloud RMSE below `1e-3` with at least 50x
  improvement. Both x86_64 and AArch64 all-feature checks pass.
- Dataset metadata is fixed: OpenCV `building.jpg` is 79,718 bytes with
  SHA-256 `742a1baad62ac82e91e718e77eedf7e85c2eddc4badfb8c87c6cbc86c45a8b07`
  under Apache-2.0; Open3D `20220201-data/DemoICPPointClouds.zip` is 10,829,466
  bytes with SHA-256
  `7596ffc80afe992ed966f4d96b676a08d9393fd86ed8bfd672b2f6a514c6fb75`
  under the download repository's MIT license.
- Kornia 0.1.14 requires `wide ^1.1.1`, whose published releases require Rust
  1.89. The approved resolution raises the workspace MSRV to 1.89 so normal
  locked verification remains authoritative. The configured mirror did not
  publish that toolchain, so Rust 1.89.0 was installed from the official
  distribution and the repository pins it exactly.

### Phase 0: feasibility gate

Before full implementation:

- compile minimal ORB, seeded homography RANSAC, warp, ICP, point transform,
  image decode, and PCD decode calls on Rust 1.89;
- verify x86_64 and AArch64 compilation and record direct/transitive licenses;
- prove one deterministic synthetic registration case per domain;
- fix empirical numerical thresholds and bounded fixture sizes in tests;
- finalize immutable data entries with URL, revision, SHA-256, byte size,
  license, attribution, and extracted path.

Preferred data:

- OpenCV `samples/data/building.jpg` at revision
  `77dfa297d08fdecdc509fc01ad92a2e9ec776a57` for image registration. Generate
  the second view with a fixed perspective transform.
- Open3D `DemoICPPointClouds.zip` release asset for point-cloud registration;
  it contains the binary PCD pair used by Kornia's ICP example. Its archive
  provenance and redistribution terms must pass the license gate before use.

Stop and revise the plan rather than hand-writing replacement algorithms if a
required Kornia API is unavailable, data terms are unclear, parsing needs a
substantial dependency, seeded behavior is unstable, or registration does not
improve the initial error.

### Phase 1: image registration

Implement a YAML-defined pipeline:

```text
image pair -> grayscale -> ORB -> match -> homography -> warp -> metrics
```

Its Mermaid group shows those Units and their Resource edges in that order,
with static and timed variants. Its Rerun blueprint shows the source and target
images, detected keypoints, candidate match lines, green inliers, red outliers,
the warped result, an alpha/checkerboard overlay, reprojection error, inlier
ratio, run metrics, and Unit timings.

### Phase 2: point-cloud registration

Implement a YAML-defined pipeline:

```text
cloud pair -> bounded sampling -> ICP -> transform -> residual metrics
```

Its Mermaid group shows those Units and their Resource edges in that order,
with static and timed variants. Its Rerun blueprint shows the target cloud in
neutral gray, the initial source in red, the aligned source in blue, coordinate
frames, bounded sampled residual/correspondence lines, final RMSE, transform,
capacity metrics, and Unit timings. Use an initial/final timeline; do not fork
Kornia or imply that an ICP iteration history is available.

### Phase 3: integration and documentation

- Add focused package tests, product commands, provenance, and expected metric
  ranges to the README/documentation index.
- Update the dependency and license inventory.
- Preserve all existing navigation commands, assets, and tests unchanged.

### Completion evidence (2026-08-11)

- The workspace pins Rust 1.89.0 and all seven verification commands below pass
  on that exact toolchain, including all-feature Clippy, panic-abort, and
  AArch64 checks. Two behavior-preserving Clippy updates keep existing graph
  and navigation tests green at the new MSRV.
- The image pipeline executes grayscale, ORB, matching, seeded homography,
  warp, and metrics in their corresponding timed Units. The pinned image
  reports 283 matches, 200 inliers, 0.852115 px reprojection RMSE, and a 0.7067
  inlier ratio.
- The point pipeline executes bounded deterministic sampling, ICP, transform,
  and metrics in their corresponding timed Units. The pinned clouds use 4,096
  samples and reduce RMSE from 0.435053 to 0.023408 in 37 iterations.
- Static and eight-run timed Mermaid products contain every planned Unit and
  Resource edge; every Unit has `avg`, `p99`, and `n=8` annotations.
- The fetch script passed verified download, checksum-failure, and idempotency
  checks. Both missing-data routes return the exact documented fetch command.
- Rerun 0.24.1 save routes produced nonempty recordings with fixed blueprints
  and all required image, cloud, coordinate-frame, residual, metric, capacity,
  timeline, and Unit-timing entities. Generated data and recordings remained
  under ignored `target/` paths.
- The locked inventory contains 503 registry packages, zero missing license
  expressions, and zero non-registry external sources. Existing V0 and
  navigation tests remain green.

## Non-goals

- Core public API or V0 semantic changes;
- strict-allocation certification for Kornia;
- generic image/point-cloud visualization infrastructure;
- LiDAR ground segmentation, clustering, learned correspondence, or V1 work;
- a Rerun upgrade, committed datasets, committed recordings, or benchmarks.

## Acceptance

- Each package builds and tests independently with default and Rerun features.
- Synthetic tests recover known transforms within Phase 0 tolerances and are
  repeatable with the same inputs and seed.
- Real-data runs report finite transforms and improve reprojection error or
  point-cloud RMSE by the recorded minimum margin.
- Each package emits parseable static and timed Mermaid output. The graphs
  contain the planned Units and Resource edges, and every executed Unit in the
  timed graph has `avg`, `p99`, and sample-count annotations.
- Missing data yields the exact fetch command; corrupt downloads fail closed;
  a second fetch is idempotent.
- Headless runs do not require Rerun. Save routes produce nonempty recordings
  with the expected image or point-cloud entities and fixed blueprint, and
  visualization happens only after a successful run without changing outputs.
- Fetching and running both products leaves the tracked worktree clean.
- Existing V0 and navigation behavior remains green.

## Verification

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-targets --all-features --locked
cargo test --doc --workspace --all-features --locked
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps --locked
RUSTFLAGS="-C panic=abort" cargo check --workspace --lib --bins --all-features --locked
cargo check --workspace --all-targets --all-features --target aarch64-unknown-linux-gnu --locked
```

Product verification additionally fetches both datasets, runs both headless
commands, snapshots and structurally checks both static and timed Mermaid
graphs, saves and structurally checks both recordings, exercises checksum
failure, and confirms `git status --short` has no generated changes.
