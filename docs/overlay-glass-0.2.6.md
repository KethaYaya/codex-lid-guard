# Glass overlay (0.2.6)

Project tabs and drawers use a dark, translucent glass appearance: a diagonal slate tint, a fine illuminated edge, inset selection, and a raised Open/Reply/Read action. Status colors, shortcut badges, layout, hit targets, and the existing opacity setting remain available.

This is a glass-style tint, without live backdrop blur. GDI paints the lighting into the existing buffers. The drawer stays cached between content changes and during slide animation; the small tab is painted through its existing path. The change introduces no additional surfaces, dependencies, polling, or animation timers. Temporary GDI pens are restored and deleted after painting. A failed gradient fill falls back to a solid tint.

## Drawing cost

Measured on this Windows machine on 2026-09-17 using optimized native render fixtures. The baseline uses the 0.2.5 painter with the same opt-in benchmark harness. Each figure is the mean of two runs of 1,000 complete drawer-and-tab repaints, ordered baseline/glass/glass/baseline. Each iteration flushes GDI. No build or other validation ran during these comparison samples.

| Scale | Sessions | Baseline | Glass | Extra per full redraw |
| --- | ---: | ---: | ---: | ---: |
| 100% | 1 | 0.519 ms | 0.866 ms | 0.346 ms |
| 100% | 3 | 0.657 ms | 0.823 ms | 0.166 ms |
| 150% | 3 | 0.633 ms | 0.840 ms | 0.207 ms |
| 200% | 3 | 0.713 ms | 1.078 ms | 0.365 ms |
| 100% | 5, shortcut hints | 0.832 ms | 1.531 ms | 0.699 ms |
| 200% | 10 | 1.404 ms | 1.932 ms | 0.527 ms |

These are cold paint costs, including text, font creation, and controls. They do not represent idle CPU usage, cached animation-frame cost, GPU utilization, or battery-life measurements. Real timing varies by device and system load. The existing cache avoids repainting the full drawer for every animation frame.

The existing native rendering fixtures cover 100%, 150%, and 200% scale, short and overflowing lists, and shortcut hints. Export them or repeat the glass benchmark with:

```powershell
$env:CODEX_OVERLAY_BENCH_ITERATIONS = '1000'
cargo test --release --locked --manifest-path native/CodexLidGuard/Cargo.toml native_grouped_render_and_hit_targets -- --nocapture
# Omit the benchmark variable for normal tests. Optional render output:
$env:CODEX_OVERLAY_RENDER_DIR = Join-Path $env:TEMP 'codex-overlay-glass-renders'
```

The default test run uses one repaint. The screenshot in the README is an opaque rendering fixture; the running overlay additionally uses the configured window opacity (82% by default).
