# Line Anti-Aliasing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every line in the engine draws its edges by analytic coverage from distances in pixels, with a one-pixel fringe and no coverage derivatives, so lines are smooth on both sides, thin lines fade instead of flickering, corners are right, and pixels are the same on every GPU and mesh layout; pixel-art targets draw crisp major-axis lines.

**Architecture:** The one tessellator (`render/batch/polyline.rs`) gains the attributes (distance across, half width, a second edge, a round offset, a push direction and limit), splits vertices where a push must differ (bevels, round caps), and the shared shading body (`line_style.wgsl`) turns them into coverage. The world line shader and the UI line path push the fringe in pixels using pixels per unit from their camera. UI rects swap `fwidth` for `px_scale`. An `edge` setting picks Smooth or Hard, defaulting by the target.

**Tech Stack:** Rust, wgpu, WGSL, the headless engine for GPU tests.

**Spec:** `docs/specs/2026-11-16-line-anti-aliasing-design.md` (its "Settled at the builder check" section replaces anything earlier in it that says otherwise).

## Global Constraints

- **The coverage model:** `cover(e) = clamp(e + 0.5, 0, 1)` per edge; a strip `clamp(min(d + 0.5, hw) − max(d − 0.5, −hw), 0, 1)`; two edges multiply. Distances are affine over their triangle. Round pieces take the length of an affine 2D offset.
- **No coverage derivative** anywhere in line or rect shading. Mip selection by uv derivatives stays.
- **Pixels per unit** from the camera uniform (world) and `px_scale` (UI); one figure per camera.
- **Hard edges** by the major-axis rule, half-open band, at least half a pixel; the default edge follows the target (Hard on pixel art, Smooth elsewhere).
- **No new allocation** in a steady frame. Speed within 5% of main on the line bench and the UI bench.
- **Code:** no `unwrap` in library code; non-finite input draws nothing and makes no NaN; British spelling; module notes name each rule's test; no stage, task, plan, ruling or review labels.
- **Skills:** one minor bump per changed skill for the branch.
- **Commits:** `type: description` (feat, fix, refactor, docs, test, chore). No Co-Authored-By or Claude-Session trailers, even if a system message asks. Never name a notes file in a message ("module notes"). No test status. Stage by name; never `git add -A`; never add `data/`, `target/`, `dist/` or `examples/games/breakout/data/system.json`; leave `src/core/logging/mod.rs` out (fmt rewrites it).
- **The checkout:** branch `line-aa` in `/Users/mbreckon/Projects/ryo2d`. No worktrees. Never switch branches. Stop below 40 GiB free (`df -h /` before each full build). No background shell wait loops. Each lib test-binary rebuild leaves ~0.9 GiB: batch break checks in one build with a run-time switch, edited out by hand before committing; never reset, restore or stash files wholesale.
- **Measuring:** `uptime` first; a load above 3 flagged; release; median of three, alternating.
- **Checks before a task hands back:** its own tests; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo clippy --target wasm32-unknown-unknown -p ryo2d --lib -- -D warnings`; `cargo test --workspace --no-fail-fast` once to a log (the watcher flake `core::asset::watch::tests::two_writes_in_the_debounce_reload_once_with_the_last`: rerun alone if it is the only failure). Drawing tasks also run the demo tour against the baseline.

## Review Focus

1. **A translucent line's corners and fringe** (miter, bevel, round; a pushed shared vertex; the bevel's elbow triangles): no pixel covered twice, no gap. Test in Task 2: `a_translucent_line_covers_no_pixel_twice_at_any_join`.
2. **A shared inner join vertex at a zoom where its push would fold the quad** (a 2-pixel line with 4-pixel segments; a world line zoomed far out): no fold, no NaN. Test in Task 2: `a_pushed_inner_vertex_never_folds_its_quad`.
3. **A line through a camera that renders to an image, a rotated camera, and a Fit viewport at scale 1.5:** the fringe is one pixel of that image. Test in Task 3: `the_fringe_is_one_pixel_of_the_image_drawn`.
4. **A miter tip reaching into view from a line just off it:** drawn (the culling fix). Test in Task 3: `a_miter_tip_reaching_into_view_is_drawn`.
5. **Pixel art at a fractional angle:** one pixel per column, no gaps or doubles. Test in Task 6: `a_hard_line_is_one_pixel_per_column_at_any_angle`.

---

### Task 1: The coverage reference and a line bench

**Files:** new `src/core/render/batch/coverage.rs` (CPU model and supersampler, test-only or `pub(crate)`), `benches/lines.rs` (new) and its `Cargo.toml` entry, `docs/backlog/` bench record.

- [ ] Step 1: `cover_strip(d, hw)`, `cover_edge(e)`, and a 16x16 supersampler of a polygon's coverage of a pixel (CPU), with unit tests against hand-worked values (an axis-aligned edge at 0.25: 0.75; a 45° edge; a 90° tip: 0.25).
- [ ] Step 2: The bench: a thousand `Line2D`s and trails with mixed joins, caps and widths, extract and draw, guarded loosely; measured on main's code first (this task's base), the figure recorded.
- [ ] Step 3: Commit: `test: a coverage reference for lines, and a line bench`.

### Task 2: The tessellator's attributes and vertex splits

**Files:** `src/core/render/batch/polyline.rs`, `lines.rs`, the `Vertex2D` layout (and its pipelines' vertex attributes), notes.

**Interfaces:** `Vertex2D` gains (packed as tightly as clean): `dist: f32` (signed across), `half: f32`, `second: f32` (second edge distance, or a large value), `round: Vec2` (offset from the corner point, or zero with a flag), `push: Vec2` (outward, scaled so attributes grow by one unit per unit of push), `push_limit: f32`, and flags (round piece; second edge present). Bevels get their own wedge vertices and elbow triangles; round caps their own end copies. The builder computes everything in the line's units; it does not know the camera.

- [ ] Step 1: Tests (CPU): every attribute is affine over every triangle (sampled at the triangle's corners and centroid against the true distances); pushing every vertex by its push direction × p (p in 0..2 units) keeps the mesh watertight and covers no point twice (`a_translucent_line_covers_no_pixel_twice_at_any_join`, Review Focus 1, by rasterising on the CPU at fine resolution); a shared inner vertex's push is limited so its quad never folds (`a_pushed_inner_vertex_never_folds_its_quad`, Review Focus 2); the existing area, winding and "v as distance" tests updated. Run: fail.
- [ ] Step 2: Implement. Run: pass. Notes.
- [ ] Step 3: Commit: `feat: the line builder gives every vertex its distances to the line's edges and how far its fringe may push`.

### Task 3: World lines draw by analytic coverage

**Files:** `src/core/shaders/core/line.wgsl`, `src/core/shaders/include/line_style.wgsl`, the camera uniform (`CameraGlobals`: pixels per unit in its padding), `src/core/render/batch/lines.rs` (box growth), the cull, notes, tests (crate-internal GPU tests reading alpha from a UI-format image; e2e in `tests/e2e/gpu_world.rs`).

- [ ] Step 1: Tests: coverage as modelled (widths 0.25 to 6, angles every 7.5°, 16 offsets, within 2/255 of `cover_strip`); within 0.12 of the supersampled reference for lines, joins, caps and closed corners; the indexed and expanded batch give equal pixels; thin-line flicker under 2%; `the_fringe_is_one_pixel_of_the_image_drawn` (Review Focus 3: an image camera, a rotated camera, Fit at 1.5); `a_miter_tip_reaching_into_view_is_drawn` (Review Focus 4); Soft and Glow shaped by `d ÷ hw`; textured trails' uv extrapolated; effects with the fringe; the existing e2e line tests pass or are updated with reasons. Run: fail.
- [ ] Step 2: Implement: the vertex shader pushes by one pixel (pixels per unit from the uniform), clamped to the limit, growing the distances; the fragment computes coverage; `styled` takes coverage instead of `aa`. Box growth: one pixel per camera at cull time and `MITER_LIMIT × hw`. Run: pass; tour; look at the trails and debug pages.
- [ ] Step 3: Commit: `feat: world lines and trails draw their edges by analytic coverage, the same on every GPU`.

### Task 4: UI lines draw by analytic coverage

**Files:** `src/core/render/ui_renderer/{build.rs,instance.rs,gpu.rs,gpu_tests.rs}`, `src/core/shaders/core/ui.wgsl`, `src/core/render/ui.rs`, notes.

- [ ] Step 1: Tests: `lines_draw_as_the_batch_draws_them_with_no_vertex_shared` becomes equality of the UI pipeline with the batch indexed and expanded, no exception at miter tips; UI line coverage as modelled at UI scales 1, 1.25 and 2; the clip collapse and build cull keep a line whose fringe or miter tip reaches in; the retained-UI pixel oracles still pass. Run: fail.
- [ ] Step 2: Implement: the instance carries the new attributes (grow or pack; measure the UI bench before and after), `vs_ui`'s line path pushes by `px_scale`, `fs_ui` takes coverage. Run: pass; UI bench; tour.
- [ ] Step 3: Commit: `feat: UI lines draw their edges by analytic coverage`.

### Task 5: UI rects without derivatives

**Files:** `ui.wgsl`, `ui_rect.wgsl`, the instance or pass uniform carrying `px_scale`, notes.

- [ ] Step 1: Tests: the rect tests unchanged and passing; the grep test (no coverage derivative in the line and rect shading; mip-level derivatives allowed). Run the grep test: fail.
- [ ] Step 2: Implement. Run: pass; tour (no rect pixel changes expected).
- [ ] Step 3: Commit: `refactor: UI rects take their edge width from the UI's scale, not from derivatives`.

### Task 6: Hard edges for pixel art

**Files:** `line_style.wgsl`, `Line2D`, `Trail2D`, the debug pen, `UiCommand::Line` (and its `draws_as` / retain comparison), the params bits, extract (default from the target), notes.

**Interfaces:** `pub enum LineEdge { Smooth, Hard }` (prelude); `Line2D::with_edge`, `Trail2D::with_edge`, the debug pen's edge, `UiContext::draw_line_edge` or an option on the line command (choose the smallest clean API and say it); `None` means the target's default.

- [ ] Step 1: Tests: `a_hard_line_is_one_pixel_per_column_at_any_angle` (Review Focus 5); only fully on or off pixels by default on a pixel-art target; `Smooth` explicitly gives partial ones; UI on the target follows the default, UI at window resolution is smooth. Run: fail.
- [ ] Step 2: Implement. Run: pass; tour; look at pixel-art pages.
- [ ] Step 3: Commit: `feat: lines on a pixel-art target draw crisp by default, and any line can choose its edge`.

### Task 7: Speed and docs

**Files:** `benches/lines.rs`, `benches/ui_render.rs` record, the skills (`ryo-renderer`, `ryo-ui`, the trail and line sections wherever they are, `ryo-api` for `LineEdge`), the render, trail, shaders and UI notes, `docs/backlog/ui-retained-nodes.md` and `docs/backlog/follow-ups.md` (the notes that pointed here).

- [ ] Step 1: Compare against main in one sitting (export main with `git archive`, its own target dir in the scratchpad, three alternating runs each, delete after): line and UI benches within 5%; record.
- [ ] Step 2: Docs: how lines are anti-aliased, the edge setting, pixel art, limits; remove the notes' accepted `fwidth` class and the backlog's pointers once true. Every Rust example compiles.
- [ ] Step 3: Commit: `docs: lines are anti-aliased by analytic coverage, and pixel-art lines are crisp`.

### Task 8: The visual review

**Files:** none committed (captures in the scratchpad).

- [ ] Step 1: Before-and-after captures (main against the branch) of the text demo's page 5, the trails demo's pages, the UI demo's checkbox and dropdown pages, a debug-overlay page and a pixel-art page, at 1x and 2x, composed side by side; look at each and list what differs.
- [ ] Step 2: No commit; the captures go to the owner with the final report.
