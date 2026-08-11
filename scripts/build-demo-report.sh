#!/usr/bin/env bash
set -euo pipefail

root_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
output_dir="$root_dir/target/demo-pages"
demo_dir="$output_dir/demos"

cd "$root_dir"
rm -rf "$output_dir"
mkdir -p "$demo_dir"

scripts/fetch-showcase-data.sh
cargo build \
    --locked \
    --all-features \
    -p navigation-planning \
    -p image-registration \
    -p point-cloud-registration

navigation_bin="$root_dir/target/debug/navigation-planning"
image_bin="$root_dir/target/debug/image-registration"
point_bin="$root_dir/target/debug/point-cloud-registration"

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

run_demo \
    navigation \
    "$navigation_bin" \
    "$root_dir/examples/navigation-planning/astar.yaml" \
    --strict \
    1000 \
    decode inflate plan smooth

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

commit=$(git -C "$root_dir" rev-parse --short=12 HEAD)
generated_at=$(date -u +'%Y-%m-%d %H:%M:%S UTC')
compiler=$(rustc --version)

html_escape_file() {
    sed -e 's/&/\&amp;/g' -e 's/</\&lt;/g' -e 's/>/\&gt;/g' "$1"
}

write_styles() {
    cat <<'EOF'
<style>
  :root {
    color-scheme: light;
    --ink: #182026;
    --muted: #59636b;
    --line: #d9dee2;
    --paper: #ffffff;
    --wash: #f5f7f8;
    --green: #147d64;
    --blue: #2764a8;
    --red: #b83243;
  }
  * { box-sizing: border-box; }
  body {
    margin: 0;
    background: var(--paper);
    color: var(--ink);
    font-family: ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
    letter-spacing: 0;
  }
  a { color: var(--blue); }
  header { border-bottom: 1px solid var(--line); background: var(--wash); }
  header > div, main, footer > div { width: min(1180px, calc(100% - 32px)); margin: 0 auto; }
  header > div { padding: 28px 0 24px; }
  h1 { margin: 0 0 8px; font-size: 28px; line-height: 1.2; }
  h2 { margin: 0 0 14px; font-size: 19px; line-height: 1.3; }
  p { line-height: 1.6; }
  .lede, .meta { color: var(--muted); }
  .meta { margin: 0; font: 13px/1.6 ui-monospace, SFMono-Regular, Consolas, monospace; }
  main { padding: 28px 0 48px; }
  .toolbar { display: flex; flex-wrap: wrap; gap: 10px; margin: 0 0 28px; }
  .command {
    display: inline-flex;
    min-height: 36px;
    align-items: center;
    border: 1px solid var(--line);
    border-radius: 6px;
    padding: 7px 11px;
    background: var(--paper);
    text-decoration: none;
    font-size: 14px;
    font-weight: 600;
  }
  .command.primary { border-color: var(--green); background: var(--green); color: #fff; }
  .section { border-top: 1px solid var(--line); padding: 24px 0 28px; }
  .graph-grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 20px; }
  .graph-panel { min-width: 0; }
  .graph-frame {
    min-height: 320px;
    overflow: auto;
    border: 1px solid var(--line);
    border-radius: 6px;
    padding: 14px;
    background: var(--paper);
  }
  .mermaid { min-width: 560px; text-align: center; }
  pre.output {
    margin: 0;
    overflow: auto;
    border-left: 4px solid var(--green);
    padding: 14px 16px;
    background: #101820;
    color: #e9f0f3;
    font: 13px/1.6 ui-monospace, SFMono-Regular, Consolas, monospace;
    white-space: pre-wrap;
  }
  .viewer {
    display: block;
    width: 100%;
    height: 680px;
    border: 1px solid var(--line);
    border-radius: 6px;
    background: #111820;
  }
  .local-note { display: none; padding: 18px; border: 1px solid var(--line); background: var(--wash); }
  .demo-list { border-top: 1px solid var(--line); }
  .demo-row {
    display: grid;
    grid-template-columns: minmax(190px, 0.8fr) minmax(0, 2fr) auto;
    gap: 24px;
    align-items: center;
    padding: 22px 0;
    border-bottom: 1px solid var(--line);
  }
  .demo-row h2, .demo-row p { margin: 0; }
  .status { color: var(--green); font-weight: 700; font-size: 13px; }
  footer { border-top: 1px solid var(--line); color: var(--muted); }
  footer > div { padding: 18px 0 28px; font-size: 13px; }
  @media (max-width: 760px) {
    .graph-grid, .demo-row { grid-template-columns: 1fr; }
    .graph-frame { min-height: 260px; }
    .viewer { height: 520px; }
  }
</style>
EOF
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
EOF
        write_styles
        cat <<EOF
  <script defer src="https://cdn.jsdelivr.net/npm/mermaid@11.12.0/dist/mermaid.min.js"></script>
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
    <a class="command primary" id="open-rerun" href="recording.rrd">Open interactive Rerun</a>
    <a class="command" href="recording.rrd">Download .rrd</a>
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
    <iframe class="viewer" id="rerun-viewer" title="Rerun visualization" loading="lazy" allow="fullscreen"></iframe>
    <p class="local-note" id="local-note">This downloaded report cannot serve the recording to the Web Viewer. Use the download link above and open the file with Rerun 0.24.1.</p>
  </section>
</main>
<footer><div>Generated by <code>scripts/build-demo-report.sh</code>. No generated report files are committed to the repository.</div></footer>
<script>
  window.addEventListener("DOMContentLoaded", () => {
    if (window.mermaid) {
      mermaid.initialize({ startOnLoad: true, theme: "neutral", securityLevel: "strict" });
    }
    const viewer = document.getElementById("rerun-viewer");
    const open = document.getElementById("open-rerun");
    if (location.protocol === "http:" || location.protocol === "https:") {
      const recording = new URL("recording.rrd", location.href).href;
      const url = "https://app.rerun.io/version/0.24.1/?url=" + encodeURIComponent(recording);
      viewer.src = url;
      open.href = url;
    } else {
      viewer.remove();
      document.getElementById("local-note").style.display = "block";
      open.textContent = "Download recording";
    }
  });
</script>
</body>
</html>
EOF
    } > "$page_dir/index.html"
}

write_demo_page \
    navigation \
    "Navigation planning" \
    "Strict-allocation A* navigation over a deterministic 1,000-leg episode." \
    "ROS map decode -&gt; inflation -&gt; A* planning -&gt; line-of-sight smoothing"

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

{
    cat <<EOF
<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>UnitCompose CI demos</title>
EOF
    write_styles
    cat <<EOF
</head>
<body>
<header><div>
  <p class="meta">UnitCompose / continuous examples</p>
  <h1>CI-generated demo reports</h1>
  <p class="lede">Three real UnitCompose pipelines, rebuilt and executed from a clean GitHub Actions runner.</p>
  <p class="meta">commit $commit | $compiler | generated $generated_at</p>
</div></header>
<main>
  <div class="demo-list">
    <section class="demo-row">
      <div><span class="status">PASS</span><h2>Navigation planning</h2></div>
      <p>Strict allocation, deterministic 1,000-leg episode, route playback, and four measured Units.</p>
      <a class="command primary" href="navigation/">View report</a>
    </section>
    <section class="demo-row">
      <div><span class="status">PASS</span><h2>Image registration</h2></div>
      <p>ORB features, candidate matches, seeded homography, warped result, and reprojection quality.</p>
      <a class="command primary" href="image-registration/">View report</a>
    </section>
    <section class="demo-row">
      <div><span class="status">PASS</span><h2>Point-cloud registration</h2></div>
      <p>Bounded sampling, ICP alignment, residuals, coordinate frames, and RMSE improvement.</p>
      <a class="command primary" href="point-cloud-registration/">View report</a>
    </section>
  </div>
</main>
<footer><div>Each report contains the exact run output, static and timed Module graphs, and its Rerun recording.</div></footer>
</body>
</html>
EOF
} > "$demo_dir/index.html"

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
