function initializeMermaid() {
  if (window.mermaid) {
    mermaid.initialize({ startOnLoad: true, theme: "neutral", securityLevel: "strict" });
  }
}

function initializeRerun() {
  document.querySelectorAll("[data-recording]").forEach((viewer) => {
    const recordingPath = viewer.dataset.recording;
    const open = document.querySelector(`[data-open-recording="${recordingPath}"]`);
    if (location.protocol === "http:" || location.protocol === "https:") {
      const recording = new URL(recordingPath, location.href).href;
      const url = "https://app.rerun.io/version/0.24.1/?url=" + encodeURIComponent(recording);
      viewer.src = url;
      if (open) open.href = url;
    } else {
      viewer.remove();
      const note = document.querySelector(`[data-local-note="${recordingPath}"]`);
      if (note) note.style.display = "block";
      if (open) open.textContent = "Download recording";
    }
  });
}

function cssColor(name) {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
}

function drawMap(canvas, snapshots, overlay) {
  const rect = canvas.getBoundingClientRect();
  const ratio = window.devicePixelRatio || 1;
  canvas.width = Math.max(1, Math.round(rect.width * ratio));
  canvas.height = Math.max(1, Math.round(rect.height * ratio));
  const context = canvas.getContext("2d");
  context.setTransform(ratio, 0, 0, ratio, 0, 0);
  context.clearRect(0, 0, rect.width, rect.height);

  const snapshot = snapshots[0];
  const scale = Math.min(rect.width / snapshot.width, rect.height / snapshot.height);
  const mapWidth = snapshot.width * scale;
  const mapHeight = snapshot.height * scale;
  const offsetX = (rect.width - mapWidth) / 2;
  const offsetY = (rect.height - mapHeight) / 2;
  context.fillStyle = cssColor("--color-paper");
  context.fillRect(offsetX, offsetY, mapWidth, mapHeight);

  const map = overlay ? snapshot.binary_map : snapshot.cost_map;
  for (let y = 0; y < snapshot.height; y += 1) {
    for (let x = 0; x < snapshot.width; x += 1) {
      const value = map[y * snapshot.width + x];
      if (value === 0) continue;
      context.fillStyle = value >= 100 ? cssColor("--color-obstacle") : cssColor("--color-cost");
      context.fillRect(offsetX + x * scale, offsetY + y * scale, Math.ceil(scale), Math.ceil(scale));
    }
  }

  const pathColors = overlay
    ? [cssColor("--color-a"), cssColor("--color-b")]
    : [snapshots[0].smoothed ? cssColor("--color-a") : cssColor("--color-b")];
  snapshots.forEach((item, index) => {
    context.strokeStyle = pathColors[index];
    context.lineWidth = overlay ? Math.max(2, scale * 0.3) : Math.max(2.5, scale * 0.35);
    context.lineJoin = "round";
    context.lineCap = "round";
    context.beginPath();
    item.final_path.forEach((point, pointIndex) => {
      const px = offsetX + (point.x + 0.5) * scale;
      const py = offsetY + (point.y + 0.5) * scale;
      if (pointIndex === 0) context.moveTo(px, py);
      else context.lineTo(px, py);
    });
    context.stroke();
  });

  [[snapshot.start, "--color-start"], [snapshot.goal, "--color-goal"]].forEach(([point, color]) => {
    context.fillStyle = cssColor(color);
    context.beginPath();
    context.arc(offsetX + (point.x + 0.5) * scale, offsetY + (point.y + 0.5) * scale, Math.max(3.5, scale * 0.45), 0, Math.PI * 2);
    context.fill();
  });
}

function formatDuration(nanoseconds) {
  if (nanoseconds < 1000) return `${Math.round(nanoseconds)} ns`;
  if (nanoseconds < 1000000) return `${(nanoseconds / 1000).toFixed(1)} us`;
  return `${(nanoseconds / 1000000).toFixed(2)} ms`;
}

function initializeNavigationComparison() {
  const aNode = document.getElementById("snapshot-a");
  const bNode = document.getElementById("snapshot-b");
  if (!aNode || !bNode) return;
  const a = JSON.parse(aNode.textContent);
  const b = JSON.parse(bNode.textContent);
  const metrics = [
    ["Compiled Units", a.units.length, b.units.length, ""],
    ["Final path points", a.final_path_metrics.points, b.final_path_metrics.points, ""],
    ["Path length", a.final_path_metrics.length.toFixed(2), b.final_path_metrics.length.toFixed(2), "cells"],
    ["Turns", a.final_path_metrics.turns, b.final_path_metrics.turns, ""],
    ["Storage slots", a.storage.slots, b.storage.slots, ""],
    ["Estimated storage", a.storage.estimated_bytes, b.storage.estimated_bytes, "bytes"],
    ["Median run", formatDuration(a.timing.median_ns), formatDuration(b.timing.median_ns), "1000 samples"],
    ["P95 run", formatDuration(a.timing.p95_ns), formatDuration(b.timing.p95_ns), "1000 samples"],
    ["Measured allocations", a.allocation_operations.allocations, b.allocation_operations.allocations, "operations"],
  ];
  document.getElementById("comparison-metrics").innerHTML = metrics.map(([label, av, bv, note]) =>
    `<tr><th scope="row">${label}</th><td class="metric-value metric-a">${av}</td><td class="metric-value metric-b">${bv}</td><td class="metric-delta">${note}</td></tr>`
  ).join("");

  const renderers = [
    [document.getElementById("map-a"), [a], false],
    [document.getElementById("map-b"), [b], false],
    [document.getElementById("map-overlay"), [a, b], true],
  ];
  const render = () => renderers.forEach(([canvas, snapshots, overlay]) => drawMap(canvas, snapshots, overlay));
  new ResizeObserver(render).observe(document.getElementById("navigation-maps"));
  render();
}

window.addEventListener("DOMContentLoaded", () => {
  initializeMermaid();
  initializeRerun();
  initializeNavigationComparison();
});
