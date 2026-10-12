# AGENTS.md - Trail & Line Rendering System

## Integration with Engine

- `record_trail_points` (`systems/trail.rs`, `FixedSet::Trails`, after propagation) calls `trail.update(x, y, dt)` every fixed step for each `Trail2D` with its entity's `GlobalTransform` translation and the fixed timestep. Recording and aging therefore follow game time (paused, slowed, stopped by hitstop). Manual `update()` calls are only needed for custom positions.
- A `Trail2D` without a `Transform` is still drawn (its points are absolute world positions) but never records or ages. A `Line2D` needs a `Transform`: its points are offsets from the entity's world translation (rotation and scale are not applied), and on an interpolated entity it draws at the blended position, with the sprite.
- **Render order:** trails and lines sort by `z_order` with entities and `Scene::render` sprites in the `world` pass (equal z: after entities and sprites), then lighting, the transition fade, post-process effects and the UI. `Trail2D` and `Line2D` both have `with_z_order` / `z_order` / `set_z_order` (default 0). Default z is 0, so give a trail its entity's z (or higher) to keep it in front of that entity. Geometry is grouped by (lighting mode, z) in draw order; `prepare` uploads every group once per frame and `draw_group` draws one inside the pass.
- **Vertex limit:** `MAX_TRAIL_VERTICES` vertices per frame across all groups; further trails and lines are dropped and `prepare` logs one warning for that frame.
- **Lighting:** trails/lines are drawn before lighting, so they are lit like sprites (dark in unlit areas). `with_lighting(false)` on a `Trail2D`/`Line2D` opts out: those groups are drawn by the `unlit_world` pass onto the lit scene after lighting and before post-processing; without lighting both groups draw together. Regression tests in `tests/e2e/headless_engine.rs`: `lines_render_when_lighting_is_on`, `unlit_lines_stay_bright_in_the_dark`, `unlit_lines_render_without_lighting`.

## Laser Fade-Out Effects

- `set_enabled(false)` stops recording **and hides** the trail. For fade-outs, use `set_recording(false)`: aging and lifetime removal continue while it still renders.
- Points are only removed by age when a lifetime is set (`with_lifetime`, or the `fading_laser` preset).
- **Static fade** (whole line fades): `Trail2D::fading_laser(color, width, secs)` + `populate_line(x1, y1, x2, y2, n)` + `set_recording(false)`, spawned with a `Transform`.
- **Moving tracer** (recommended; the beam visibly sweeps away): spawn an invisible fast-moving entity with a `Trail2D` that has a lifetime. On reaching the target, call `set_recording(false)`. Despawn when `point_count() == 0`.

## Shader

Two uber-shaders, `shaders/core/trail.wgsl` and `trail_textured.wgsl`, handle per-vertex style (Solid/Soft/Glow) and `TrailEffect` displacement. The WGSL vertex input must match `TrailVertex` in `renderer.rs` (56 bytes, asserted in a test there).

## Tips for AI Agents

1. **Trail vs Line**: `Trail2D` for moving objects (auto-records), `Line2D` for static/explicit shapes.
2. **Fade modes**: position fade (`.with_fade(true)`) stretches on fast objects. `.with_fade_distance(px)` fades consistently regardless of speed. Time fade (`.with_lifetime()`) suits slow/stationary sources.
