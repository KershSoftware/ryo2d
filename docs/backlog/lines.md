# Lines: speed

## Measured

What is measured: a thousand lines and trails drawn together, by `cargo bench --bench lines` (release), on the code before the line anti-aliasing work (`docs/specs/2026-11-16-line-anti-aliasing-design.md`, Acceptance, Speed). The work is held to within 5% of these figures, and allocates nothing more in a steady frame.

Machine: Apple M5 Pro, 18 cores, shared with other work.

The scene: 1920 x 1080 headless (a GPU), unlit, camera over the view. 500 `Line2D`s and 500 `Trail2D`s in a grid of 40 x 25 cells of 48 x 43.2 px, each of five to seven points that turn at every point, with joins (miter, bevel, round), caps (butt, round, square) and widths (0.5, 1, 2, 3, 6, 10 px) mixed, every fourth translucent. None moves. The bench warms 60 frames, then measures 300; each figure is the median of the frames of one run, and the figure recorded is the median of three runs.

What each figure covers:

- `lines_render`: the render step's CPU time as a whole (`HeadlessEngine::render_time`): the lines and trails built into triangles, then the draw (placing, uploading, encoding and submitting).
- `lines_prepare`: `Batch2D::prepare` within it (`HeadlessEngine::batch_prepare_time`).
- `lines_extract`: the render step less prepare. This is where the lines and trails are built and the frame recorded and submitted; the entity drawer's own timer (`entity_draw_time`) does not cover the lines, which are built after it.

### The record

One-minute load before each run: 10.08 for all three (the clock had not moved between them). That is above 3 and so flagged: the machine was shared, and nothing waited for a quiet one. The spread of the runs is under 1%.

Medians in microseconds:

| Bench | Run 1 | Run 2 | Run 3 | Median of runs |
|-------|-------|-------|-------|----------------|
| `lines_render` | 716.0 | 712.6 | 718.9 | 716.0 |
| `lines_prepare` | 317.0 | 314.0 | 317.8 | 317.0 |
| `lines_extract` | 396.0 | 397.6 | 399.0 | 397.6 |

The guards are loose, for regressions of another order: `lines_extract` under 8,000 µs and `lines_render` under 16,000 µs.
