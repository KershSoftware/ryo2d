# AGENTS.md - Trail & Line Rendering System

## Integration with Engine

- `record_trail_points` (`systems/trail.rs`, `FixedSet::Trails`, after propagation) calls `trail.update(x, y, dt)` every fixed step for each `Trail2D` with its entity's `GlobalTransform` translation and the fixed timestep. Recording and aging therefore follow game time (paused, slowed, stopped by hitstop). Manual `update()` calls are only needed for custom positions.
- A `Trail2D` without a `Transform` is still drawn (its points are absolute world positions) but never records or ages. A `Line2D` needs a `Transform`: its points are offsets from the entity's world translation (rotation and scale are not applied), and on an interpolated entity it draws at the blended position, with the sprite.
- **Drawing:** this module only holds the components. Each frame the engine turns every trail and line into a joined polyline on the 2D batch's triangle stream (`render/batch/lines.rs` ports the per-point rules: width by taper, colour by fade or age, `u` by texture mode; `polyline.rs` builds the triangles; `shaders/core/line.wgsl` draws them). Each trail or line is one draw-list item, so it sorts by `z_order` with entities and `Scene::render` sprites (equal z: after entities and sprites) and neighbours with equal blend (and texture) share a draw call. There is no vertex limit.
- **Joins and caps:** `LineJoin::{Miter (default), Bevel, Round}` on both `Trail2D` and `Line2D` (a miter longer than four times the width is bevelled); `LineCap::{Butt, Round, Square}`. Every corner gets one wedge on its outside, so translucent corners are no darker than their segments.
- **Blend and filter:** `with_blend(BlendMode)` on both (`Additive` for lasers and glows); `Trail2D::with_filter(Filter)` for a textured trail. A tiled texture repeats inside its atlas slot (`sample_wrapped`).
- **Colour:** line colours are taken as linear (as 2a's trail renderer did), so a line looks lighter than a sprite of the same `Color`.
- **Lighting:** trails/lines are drawn before lighting, so they are lit like sprites (dark in unlit areas). `with_lighting(false)` opts out: the item is drawn by the `unlit_world` pass onto the lit scene after lighting and before post-processing. Regression tests in `tests/e2e/headless_engine.rs`: `lines_render_when_lighting_is_on`, `unlit_lines_stay_bright_in_the_dark`, `unlit_lines_render_without_lighting`.

## Laser Fade-Out Effects

- `set_enabled(false)` stops recording **and hides** the trail. For fade-outs, use `set_recording(false)`: aging and lifetime removal continue while it still renders.
- Points are only removed by age when a lifetime is set (`with_lifetime`, or the `fading_laser` preset).
- **Static fade** (whole line fades): `Trail2D::fading_laser(color, width, secs)` + `populate_line(x1, y1, x2, y2, n)` + `set_recording(false)`, spawned with a `Transform`.
- **Moving tracer** (recommended; the beam visibly sweeps away): spawn an invisible fast-moving entity with a `Trail2D` that has a lifetime. On reaching the target, call `set_recording(false)`. Despawn when `point_count() == 0`.

## Shader

`shaders/core/line.wgsl` (composed with the camera, colour and sampling includes) has `fs_main` and `fs_textured`. Style (Solid, Soft, Glow) and `TrailEffect` (Lightning, Noise, UvScroll, Ribbon) travel in each vertex's `params` (`render/batch/lines.rs`, `build_effect_params`); displacement moves a vertex along its `across` perpendicular. Its vertex input matches `Vertex2D` (`render/batch/polyline.rs`, 72 bytes). Solid edges anti-alias by pixel coverage, so a one-pixel line stays solid.

## Tips for AI Agents

1. **Trail vs Line**: `Trail2D` for moving objects (auto-records), `Line2D` for static/explicit shapes.
2. **Fade modes**: position fade (`.with_fade(true)`) stretches on fast objects. `.with_fade_distance(px)` fades consistently regardless of speed. Time fade (`.with_lifetime()`) suits slow/stationary sources.
