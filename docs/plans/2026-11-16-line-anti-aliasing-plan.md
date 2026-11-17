# Line Anti-Aliasing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every line in the engine draws its edges by analytic coverage from distances in pixels, with a one-pixel fringe and no coverage derivatives, so lines are smooth on both sides, thin lines fade instead of flickering, corners are right, and pixels are the same on every GPU and mesh layout; pixel-art targets draw crisp major-axis lines.

**Architecture:** (Revised 2026-11-16: lines are built on the GPU from their points; see the design's "Building on the GPU".) The one tessellator (`render/batch/polyline.rs`) gains the attributes (distance across, half width, a second edge, a round offset, a push direction and limit), splits vertices where a push must differ (bevels, round caps), and the shared shading body (`line_style.wgsl`) turns them into coverage. The world line shader and the UI line path push the fringe in pixels using pixels per unit from their camera. UI rects swap `fwidth` for `px_scale`. An `edge` setting picks Smooth or Hard, defaulting by the target.

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

### Task 3: The reference in pieces (CPU only)

**Files:** `src/core/render/batch/polyline.rs` (and `coverage.rs`), notes.

- [ ] Step 1: Restructure the builder into self-contained pieces in a fixed triangle order, in f32, with no packing: segment (2 triangles), miter/bevel join (4), round join (18, circumscribed), square cap (2), round cap (16); a per-corner "neighbourhood" function deciding miter or bevel, shared or split, straight-on, U-turn; the point window each piece reads; the point filter as a copy step; a split inner corner gives the segment a vertex at the corner point (no T-junction). Drop the f16/halving/angle packing. Tests: the existing CPU tests (attributes affine, coverage from pushed attributes, no double cover, no fold) kept green on the pieces, plus round pieces at a half width of 150 px, plus the T-junction gone. No GPU change (drawing still uses the reference's triangles until Task 4).
- [ ] Step 2: Commit: `refactor: the line builder makes self-contained pieces, the reference for building lines on the GPU`.

### Task 4: Lines built on the GPU (world)

**Files:** `src/core/render/batch/{lines.rs,gpu.rs,pipelines.rs}`, new `src/core/shaders/include/line_pieces.wgsl`, `line.wgsl`, the camera uniform (pixels per unit in `CameraGlobals`), the cull, notes.

- [ ] Step 1: Tests: GPU vertex outputs equal the reference (a test-only compute entry calling the same piece-vertex function, triangle for triangle, f32 tolerance); the additive-count watertightness draw (no pixel twice, none missed, many sub-pixel offsets, translucent lines of every join and cap); `a_miter_tip_reaching_into_view_is_drawn`; `the_fringe_is_one_pixel_of_the_image_drawn`. Fold in the review of Task 2's fix round (bevels, k blend, limits). Run: fail.
- [ ] Step 2: Implement: point, line-table and triangle-code buffers; draws by code ranges, no vertex or index buffer for lines; the include with `@invariant` position and one corner function; the push by pixels per unit; culling growth; a minimal Solid coverage fragment. Run: pass; the line bench (CPU) against base.
- [ ] Step 3: Commit: `feat: lines are built on the GPU from their points`.

### Task 5: World coverage and styles complete

**Files:** `line.wgsl`, `line_style.wgsl`, tests (crate-internal alpha reads; e2e).

- [ ] Step 1: Tests: the model within 2/255 (widths 0.25–6, angles every 7.5°, 16 offsets); the supersampled truth within 0.12 (outside the recorded inner-corner zone); thin lines under 2%; translucent corners; Soft and Glow by d ÷ hw; textured trails' uv extrapolated; effects with the fringe; the e2e line tests pass or are updated with reasons. Run: fail.
- [ ] Step 2: Implement; tour; look at the trails and debug pages.
- [ ] Step 3: Commit: `feat: world lines and trails draw their edges by analytic coverage, the same on every GPU`.

### Task 6: UI lines on the pieces

**Files:** `src/core/render/ui_renderer/{build.rs,instance.rs,gpu.rs,gpu_tests.rs}`, `ui.wgsl`, `src/core/render/ui.rs`, notes.

- [ ] Step 1: Tests: UI lines equal world lines pixel for pixel at UI scales 1, 1.25 and 2 (replacing the shared-vertex proofs); the clip collapse and build cull keep a line whose fringe or miter tip reaches in; the retained-UI oracles pass. Run: fail.
- [ ] Step 2: Implement: a line instance per two triangles with its points inline, the same include, the push by `px_scale`. Run: pass; the UI bench; tour.
- [ ] Step 3: Commit: `feat: UI lines are built on the GPU by the same pieces as world lines`.

### Task 7: UI rects without derivatives

As before: rects take their edge width from `px_scale`; the grep test (no coverage derivative in line or rect shading). Commit: `refactor: UI rects take their edge width from the UI's scale, not from derivatives`.

### Task 8: Hard edges for pixel art

As before (the major-axis rule; `LineEdge`; the default by target; Review Focus 5). Commit: `feat: lines on a pixel-art target draw crisp by default, and any line can choose its edge`.

### Task 9: Retire the CPU path, speed and docs

**Files:** `polyline.rs` (reference to `cfg(test)`, with `coverage.rs`), remove line vertex/index buffers and the UI's per-triangle line build, `benches/lines.rs` (a profile scope around the line build; GPU timestamp queries, native only; a work-was-done bound; widths decoupled from joins), skills and notes, the backlog.

- [ ] Step 1: Retire; compare against main in one sitting (CPU and GPU time within ~15% on the lines-only bench); record.
- [ ] Step 2: Docs: how lines are built and anti-aliased, the edge setting, pixel art, the limits (inner corners, the cover model); remove the accepted `fwidth` class and the backlog's pointers. Every Rust example compiles.
- [ ] Step 3: Commit: `docs: lines are built on the GPU and anti-aliased by analytic coverage, and pixel-art lines are crisp`.

### Task 10: The visual review

As before: before-and-after captures (main against the branch) at 1x and 2x, side by side, for the owner's hand-play. No commit.
