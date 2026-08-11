# Registration showcases

The image and point-cloud registration packages are best-effort UnitCompose
examples backed by Kornia 0.1.14. They demonstrate configuration-defined
pipelines, per-Unit timing, and optional domain visualization. The navigation
example remains the strict-allocation reference.

## Fetch the data

From the repository root, run:

```bash
scripts/fetch-showcase-data.sh
```

The script downloads only into `target/demo-data`, verifies SHA-256 before
atomically replacing an archive, and is idempotent when all artifacts already
match. Normal builds, tests, and example binaries never access the network. A
missing dataset reports `missing showcase data; run
scripts/fetch-showcase-data.sh`.

The image input is OpenCV's Apache-2.0 `samples/data/building.jpg` at revision
`77dfa297d08fdecdc509fc01ad92a2e9ec776a57`. It is 79,718 bytes and has SHA-256
`742a1baad62ac82e91e718e77eedf7e85c2eddc4badfb8c87c6cbc86c45a8b07`:

```text
https://raw.githubusercontent.com/opencv/opencv/77dfa297d08fdecdc509fc01ad92a2e9ec776a57/samples/data/building.jpg
```

The point-cloud inputs are `cloud_bin_0.pcd` and `cloud_bin_1.pcd` extracted
from Open3D's MIT-licensed `DemoICPPointClouds.zip` release asset. The archive
is 10,829,466 bytes and has SHA-256
`7596ffc80afe992ed966f4d96b676a08d9393fd86ed8bfd672b2f6a514c6fb75`:

```text
https://github.com/isl-org/open3d_downloads/releases/download/20220201-data/DemoICPPointClouds.zip
```

Datasets, generated Mermaid files, and `.rrd` recordings remain untracked.

## Run the examples

The image Module executes `grayscale -> ORB -> match -> homography -> warp ->
metrics`. The point-cloud Module executes `bounded sample -> ICP -> transform
-> metrics` and samples at most 4,096 points deterministically.

```bash
cargo run -p image-registration --locked -- --module examples/image-registration/image-registration.yaml --run
cargo run -p point-cloud-registration --locked -- --module examples/point-cloud-registration/point-cloud-registration.yaml --run
```

With the pinned datasets, the image run should find roughly 250-320 candidate
matches, retain more than 65% as inliers, and report reprojection RMSE below
1.0 pixel. The point-cloud run should start near 0.435 nearest-neighbor RMSE and
finish below 0.03, an improvement greater than 10x. Exact timing varies by
machine; transforms and quality metrics must remain finite.

Inspect the static topology or annotate the same graph with eight completed
runs (`avg`, nearest-rank `p99`, and `n=8` for every Unit):

```bash
cargo run -p image-registration --locked -- --module examples/image-registration/image-registration.yaml --inspect mermaid
cargo run -p image-registration --locked -- --module examples/image-registration/image-registration.yaml --timed-mermaid
cargo run -p point-cloud-registration --locked -- --module examples/point-cloud-registration/point-cloud-registration.yaml --inspect mermaid
cargo run -p point-cloud-registration --locked -- --module examples/point-cloud-registration/point-cloud-registration.yaml --timed-mermaid
```

## Optional Rerun views

Rerun remains a default-off feature pinned to 0.24.1. Visualization occurs
only after a successful registration run and does not modify Module outputs.
Save a recording without a viewer, or spawn a compatible viewer:

```bash
cargo run -p image-registration --features rerun --locked -- --module examples/image-registration/image-registration.yaml --rerun-save target/image-registration.rrd
cargo run -p image-registration --features rerun --locked -- --module examples/image-registration/image-registration.yaml --rerun-spawn
cargo run -p point-cloud-registration --features rerun --locked -- --module examples/point-cloud-registration/point-cloud-registration.yaml --rerun-save target/point-cloud-registration.rrd
cargo run -p point-cloud-registration --features rerun --locked -- --module examples/point-cloud-registration/point-cloud-registration.yaml --rerun-spawn
```

The fixed image blueprint contains source/target images, keypoints, candidate
matches, green inliers, red outliers, the warped result, overlay, quality
metrics, and Unit timings. The point-cloud blueprint contains the gray target,
red seeded source, blue aligned source, bounded residual lines, transform and
capacity metrics, Unit timings, and an initial/final timeline.

## Continuous demo reports

CI runs navigation, image registration, and point-cloud registration from a
clean runner and builds a separate HTML report for each demo. Every report
contains the exact stdout metrics, static and timed Mermaid graphs, and the
Rerun recording produced by that run.

Pull requests and branch pushes upload the complete site as a 30-day GitHub
Actions artifact named `demo-report-<commit>`. Successful `main` builds also
publish the same files to the stable
[UnitCompose CI demos](https://miaodx.github.io/unit-compose/demos/) site. The
README links only to this latest successful Pages deployment; it does not rely
on expiring or authenticated Actions artifact URLs.

Generate the same report locally with:

```bash
scripts/build-demo-report.sh
```

The generated site is written to `target/demo-pages`. Open
`target/demo-pages/demos/index.html` for the report index. A local `file://`
page offers the `.rrd` recording as a download because the hosted Rerun viewer
requires an HTTP URL; serving the directory over HTTP enables the embedded
viewer behavior used by GitHub Pages.
