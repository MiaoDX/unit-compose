#!/usr/bin/env bash
set -euo pipefail

root_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
output_dir="$root_dir/target/demo-pages"
demo_dir="$output_dir/demos"
asset_dir="$output_dir/assets"

cd "$root_dir"
rm -rf "$output_dir"
mkdir -p "$demo_dir" "$asset_dir"
cp scripts/demo-report/tokens.css scripts/demo-report/site.css scripts/demo-report/site.js "$asset_dir/"

scripts/fetch-showcase-data.sh
cargo build \
    --locked \
    --all-features \
    -p navigation-planning \
    -p image-registration \
    -p point-cloud-registration \
    -p lidar-slam

navigation_bin="$root_dir/target/debug/navigation-planning"
image_bin="$root_dir/target/debug/image-registration"
point_bin="$root_dir/target/debug/point-cloud-registration"
lidar_bin="$root_dir/target/debug/lidar-slam"

run_demo() {
    local slug=$1
    local binary=$2
    local module=$3
    local run_mode=$4
    local expected_samples=$5
    shift 5
    local units=("$@")
    local page_dir="$demo_dir/$slug"

    mkdir -p "$page_dir"
    cp "$module" "$page_dir/module.yaml"
    "$binary" --module "$module" "$run_mode" | tee "$page_dir/run.txt"
    "$binary" --module "$module" --inspect mermaid > "$page_dir/static.mmd"
    "$binary" --module "$module" --timed-mermaid > "$page_dir/timed.mmd"
    "$binary" --module "$module" --rerun-save "$page_dir/recording.rrd"

    test -s "$page_dir/recording.rrd"
    for unit in "${units[@]}"; do
        grep -Fq "$unit<br/>Unit" "$page_dir/static.mmd"
        grep -Fq "$unit<br/>Unit" "$page_dir/timed.mmd"
    done
    test "$(grep -o "n=$expected_samples" "$page_dir/timed.mmd" | wc -l)" \
        -eq "${#units[@]}"
    grep -Eq "avg .*p99 .*n=$expected_samples" "$page_dir/timed.mmd"
}

navigation_a="$demo_dir/navigation/group-a"
navigation_b="$demo_dir/navigation/group-b"
run_demo \
    navigation/group-a \
    "$navigation_bin" \
    "$root_dir/examples/navigation-planning/astar.yaml" \
    --strict \
    1000 \
    decode inflate plan smooth
"$navigation_bin" \
    --module "$root_dir/examples/navigation-planning/astar.yaml" \
    --snapshot-json > "$navigation_a/snapshot.json"

run_demo \
    navigation/group-b \
    "$navigation_bin" \
    "$root_dir/examples/navigation-planning/dijkstra-no-smoothing.yaml" \
    --strict \
    1000 \
    decode inflate plan
"$navigation_bin" \
    --module "$root_dir/examples/navigation-planning/dijkstra-no-smoothing.yaml" \
    --snapshot-json > "$navigation_b/snapshot.json"

grep -Fq '"type": "nav.astar/v1"' "$navigation_a/snapshot.json"
grep -Fq '"type": "nav.dijkstra/v1"' "$navigation_b/snapshot.json"
grep -Fq '"samples": 1000' "$navigation_a/snapshot.json"
grep -Fq '"samples": 1000' "$navigation_b/snapshot.json"
grep -Fq '"smoothed": true' "$navigation_a/snapshot.json"
grep -Fq '"smoothed": false' "$navigation_b/snapshot.json"
if grep -Fq 'smooth<br/>Unit' "$navigation_b/static.mmd"; then
    echo "group B unexpectedly contains a smoother Unit" >&2
    exit 1
fi

run_demo \
    image-registration \
    "$image_bin" \
    "$root_dir/examples/image-registration/image-registration.yaml" \
    --run \
    8 \
    grayscale orb match homography warp metrics

run_demo \
    point-cloud-registration \
    "$point_bin" \
    "$root_dir/examples/point-cloud-registration/point-cloud-registration.yaml" \
    --run \
    8 \
    sample icp transform metrics

run_demo \
    lidar-slam \
    "$lidar_bin" \
    "$root_dir/examples/lidar-slam/lidar-slam.yaml" \
    --run \
    480 \
    scan_prepare slam snapshot
grep -Eq 'frames=480 .*loops=([3-9]|[1-9][0-9]+)( |$)' \
    "$demo_dir/lidar-slam/run.txt"

commit=$(git -C "$root_dir" rev-parse --short=12 HEAD)
generated_at=$(date -u +'%Y-%m-%d %H:%M:%S UTC')
compiler=$(rustc --version)

html_escape_file() {
    sed -e 's/&/\&amp;/g' -e 's/</\&lt;/g' -e 's/>/\&gt;/g' "$1"
}

json_script_file() {
    sed -e 's/</\\u003c/g' -e 's/>/\\u003e/g' -e 's/&/\\u0026/g' "$1"
}

write_demo_page() {
    local slug=$1
    local title=$2
    local description=$3
    local pipeline=$4
    local page_dir="$demo_dir/$slug"

    {
        cat <<EOF
<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>$title | UnitCompose CI demos</title>
  <link rel="stylesheet" href="../../assets/tokens.css">
  <link rel="stylesheet" href="../../assets/site.css">
  <script defer src="https://cdn.jsdelivr.net/npm/mermaid@11.12.0/dist/mermaid.min.js"></script>
  <script defer src="../../assets/site.js"></script>
</head>
<body>
<header><div>
  <p class="meta"><a href="../">UnitCompose CI demos</a> / $slug</p>
  <h1>$title</h1>
  <p class="lede">$description</p>
  <p class="meta">commit $commit | $compiler | generated $generated_at</p>
</div></header>
<main>
  <nav class="toolbar" aria-label="Report artifacts">
    <a class="command primary" data-open-recording="recording.rrd" href="recording.rrd">Open interactive Rerun</a>
    <a class="command" href="recording.rrd">Download .rrd</a>
    <a class="command" href="module.yaml">Module YAML</a>
    <a class="command" href="static.mmd">Static Mermaid</a>
    <a class="command" href="timed.mmd">Timed Mermaid</a>
    <a class="command" href="run.txt">Run output</a>
  </nav>
  <section class="section">
    <h2>Latest result</h2>
    <p class="meta">$pipeline</p>
    <pre class="output">
EOF
        html_escape_file "$page_dir/run.txt"
        cat <<'EOF'
</pre>
  </section>
  <section class="section">
    <h2>Module structure and measured execution</h2>
    <div class="graph-grid">
      <div class="graph-panel"><p class="meta">Prepared topology</p><div class="graph-frame"><pre class="mermaid">
EOF
        html_escape_file "$page_dir/static.mmd"
        cat <<'EOF'
</pre></div></div>
      <div class="graph-panel"><p class="meta">Completed-run timing</p><div class="graph-frame"><pre class="mermaid">
EOF
        html_escape_file "$page_dir/timed.mmd"
        cat <<'EOF'
</pre></div></div>
    </div>
  </section>
  <section class="section">
    <h2>Interactive recording</h2>
    <p class="lede">The fixed Rerun 0.24.1 blueprint shows domain outputs, quality metrics, and Unit timings from this CI run.</p>
    <iframe class="viewer" data-recording="recording.rrd" title="Rerun visualization" loading="lazy" allow="fullscreen"></iframe>
    <p class="local-note" data-local-note="recording.rrd">This downloaded report cannot serve the recording to the Web Viewer. Use the download link above and open the file with Rerun 0.24.1.</p>
  </section>
</main>
<footer><div>Generated by <code>scripts/build-demo-report.sh</code>. No generated report files are committed to the repository.</div></footer>
</body>
</html>
EOF
    } > "$page_dir/index.html"
}

write_navigation_page() {
    local page_dir="$demo_dir/navigation"
    {
        cat <<EOF
<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Navigation YAML A/B | UnitCompose CI demos</title>
  <link rel="stylesheet" href="../../assets/tokens.css">
  <link rel="stylesheet" href="../../assets/site.css">
  <script defer src="https://cdn.jsdelivr.net/npm/mermaid@11.12.0/dist/mermaid.min.js"></script>
  <script defer src="../../assets/site.js"></script>
</head>
<body>
<header><div>
  <p class="meta"><a href="../">UnitCompose CI demos</a> / navigation</p>
  <h1>Navigation YAML A/B</h1>
  <p class="lede">One compiled binary, two YAML Modules, two independently measured behaviors.</p>
  <p class="meta">commit $commit | $compiler | generated $generated_at</p>
</div></header>
<main>
  <section class="proof-strip" aria-label="YAML comparison groups">
    <div class="proof-group a"><strong>Group A: A* + smoothing</strong><p class="meta">Inflation radius 1 | 4 Units | final path condensed by line of sight</p></div>
    <div class="proof-vs">VS</div>
    <div class="proof-group b"><strong>Group B: Dijkstra, no smoothing</strong><p class="meta">Inflation radius 0 | 3 Units | raw path published directly</p></div>
  </section>
  <section class="section">
    <h2>Measured comparison</h2>
    <table class="metrics">
      <thead><tr><th>Metric</th><th class="metric-a">Group A</th><th class="metric-b">Group B</th><th>Scope</th></tr></thead>
      <tbody id="comparison-metrics"></tbody>
    </table>
  </section>
  <section class="section" id="navigation-maps">
    <h2>Map and final path</h2>
    <div class="split-grid">
      <div class="map-panel"><h3 class="a-label">Group A</h3><div class="map-shell"><canvas id="map-a" aria-label="Group A navigation map"></canvas></div></div>
      <div class="map-panel"><h3 class="b-label">Group B</h3><div class="map-shell"><canvas id="map-b" aria-label="Group B navigation map"></canvas></div></div>
    </div>
    <div class="legend"><span>Dark: obstacle</span><span>Gray: inflated cost</span><span>Green: start</span><span>Red: goal</span></div>
  </section>
  <section class="section">
    <h2>True path overlay</h2>
    <div class="map-shell"><canvas id="map-overlay" aria-label="Group A and B final path overlay"></canvas></div>
    <div class="legend"><span class="a-label"><i class="swatch"></i>Group A</span><span class="b-label"><i class="swatch"></i>Group B</span><span>Shared binary occupancy map</span></div>
  </section>
  <section class="section">
    <h2>Prepared topology</h2>
    <div class="graph-grid">
      <div class="graph-panel"><h3 class="a-label">Group A</h3><div class="graph-frame"><pre class="mermaid">
EOF
        html_escape_file "$navigation_a/static.mmd"
        cat <<'EOF'
</pre></div></div>
      <div class="graph-panel"><h3 class="b-label">Group B</h3><div class="graph-frame"><pre class="mermaid">
EOF
        html_escape_file "$navigation_b/static.mmd"
        cat <<'EOF'
</pre></div></div>
    </div>
  </section>
  <section class="section">
    <h2>Measured Unit timing</h2>
    <div class="graph-grid">
      <div class="graph-panel"><h3 class="a-label">Group A</h3><div class="graph-frame"><pre class="mermaid">
EOF
        html_escape_file "$navigation_a/timed.mmd"
        cat <<'EOF'
</pre></div></div>
      <div class="graph-panel"><h3 class="b-label">Group B</h3><div class="graph-frame"><pre class="mermaid">
EOF
        html_escape_file "$navigation_b/timed.mmd"
        cat <<'EOF'
</pre></div></div>
    </div>
  </section>
  <section class="section">
    <h2>Independent Rerun recordings</h2>
    <p class="lede">Each recording has its own event timeline because the final paths contain different point counts.</p>
    <div class="recording-grid">
      <div class="recording-panel">
        <h3 class="a-label">Group A recording</h3>
        <iframe class="viewer" data-recording="group-a/recording.rrd" title="Group A Rerun visualization" loading="lazy" allow="fullscreen"></iframe>
        <p class="local-note" data-local-note="group-a/recording.rrd">Open the downloaded Group A recording with Rerun 0.24.1.</p>
        <div class="artifact-bar"><a class="command primary" data-open-recording="group-a/recording.rrd" href="group-a/recording.rrd">Open Rerun</a><a class="command" href="group-a/recording.rrd">Download .rrd</a></div>
      </div>
      <div class="recording-panel">
        <h3 class="b-label">Group B recording</h3>
        <iframe class="viewer" data-recording="group-b/recording.rrd" title="Group B Rerun visualization" loading="lazy" allow="fullscreen"></iframe>
        <p class="local-note" data-local-note="group-b/recording.rrd">Open the downloaded Group B recording with Rerun 0.24.1.</p>
        <div class="artifact-bar"><a class="command primary" data-open-recording="group-b/recording.rrd" href="group-b/recording.rrd">Open Rerun</a><a class="command" href="group-b/recording.rrd">Download .rrd</a></div>
      </div>
    </div>
  </section>
  <section class="section">
    <h2>Exact artifacts</h2>
    <div class="split-grid">
      <div><h3 class="a-label">Group A</h3><div class="artifact-bar"><a class="command" href="group-a/module.yaml">YAML</a><a class="command" href="group-a/snapshot.json">Snapshot</a><a class="command" href="group-a/run.txt">Run</a><a class="command" href="group-a/static.mmd">Static DAG</a><a class="command" href="group-a/timed.mmd">Timed DAG</a></div></div>
      <div><h3 class="b-label">Group B</h3><div class="artifact-bar"><a class="command" href="group-b/module.yaml">YAML</a><a class="command" href="group-b/snapshot.json">Snapshot</a><a class="command" href="group-b/run.txt">Run</a><a class="command" href="group-b/static.mmd">Static DAG</a><a class="command" href="group-b/timed.mmd">Timed DAG</a></div></div>
    </div>
  </section>
</main>
<footer><div>Both groups were loaded by the same <code>target/debug/navigation-planning</code> binary. YAML selects the compiled topology and Unit types.</div></footer>
<script id="snapshot-a" type="application/json">
EOF
        json_script_file "$navigation_a/snapshot.json"
        cat <<'EOF'
</script>
<script id="snapshot-b" type="application/json">
EOF
        json_script_file "$navigation_b/snapshot.json"
        cat <<'EOF'
</script>
</body>
</html>
EOF
    } > "$page_dir/index.html"
}

write_navigation_page

write_demo_page \
    image-registration \
    "Image registration" \
    "Kornia ORB matching, seeded homography estimation, and perspective warp." \
    "grayscale -&gt; ORB -&gt; match -&gt; homography -&gt; warp -&gt; metrics"

write_demo_page \
    point-cloud-registration \
    "Point-cloud registration" \
    "Kornia ICP over a deterministic bounded sample of the Open3D tutorial pair." \
    "bounded sample -&gt; ICP -&gt; transform -&gt; residual metrics"

write_demo_page \
    lidar-slam \
    "LiDAR SLAM" \
    "Slamwich over a deterministic 480-frame figure-eight episode with odometry drift, multiple verified loop closures, and evaluation reference." \
    "synchronized LiDAR frame -&gt; stateful planar SLAM -&gt; bounded trajectory, map, and error snapshot"

cat > "$demo_dir/index.html" <<EOF
<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>UnitCompose CI demos</title>
  <link rel="stylesheet" href="../assets/tokens.css">
  <link rel="stylesheet" href="../assets/site.css">
</head>
<body>
<header><div>
  <p class="meta">UnitCompose / continuous examples</p>
  <h1>CI-generated demo reports</h1>
  <p class="lede">Four UnitCompose pipelines, rebuilt and executed from a clean GitHub Actions runner.</p>
  <p class="meta">commit $commit | $compiler | generated $generated_at</p>
</div></header>
<main>
  <div class="demo-list">
    <section class="demo-row"><div><span class="status">PASS</span><h2>Navigation planning</h2></div><p>One binary, two YAML-selected topologies, side-by-side maps, path overlay, DAGs, and independent recordings.</p><a class="command primary" href="navigation/">View comparison</a></section>
    <section class="demo-row"><div><span class="status">PASS</span><h2>Image registration</h2></div><p>ORB features, candidate matches, seeded homography, warped result, and reprojection quality.</p><a class="command primary" href="image-registration/">View report</a></section>
    <section class="demo-row"><div><span class="status">PASS</span><h2>Point-cloud registration</h2></div><p>Bounded sampling, ICP alignment, residuals, coordinate frames, and RMSE improvement.</p><a class="command primary" href="point-cloud-registration/">View report</a></section>
    <section class="demo-row"><div><span class="status">PASS</span><h2>LiDAR SLAM</h2></div><p>Large figure-eight route, persistent Slamwich state, multiple verified loop closures, optimized pose graph, and 480 measured frames.</p><a class="command primary" href="lidar-slam/">View report</a></section>
  </div>
</main>
<footer><div>Each report contains the exact YAML, run output, static and timed Module graphs, and Rerun recording.</div></footer>
</body>
</html>
EOF

cat > "$output_dir/index.html" <<'EOF'
<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta http-equiv="refresh" content="0; url=demos/">
  <title>UnitCompose CI demos</title>
</head>
<body><p><a href="demos/">Open UnitCompose CI demos</a></p></body>
</html>
EOF

echo "demo report ready at target/demo-pages/demos/index.html"
