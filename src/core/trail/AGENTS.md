# AGENTS.md - Trail & Line Rendering System

## Integration with Engine

- `TrailSystem` (`systems/trail.rs`) calls `trail.update()` for every `Trail2D` using its entity's `Transform`, after game logic. Manual `update()` calls are only needed for custom positions.
- **`Transform` is required** for both `Trail2D` and `Line2D`. Without one, the engine neither ages nor renders them (`engine.rs` collects only entities with both components).
- `Trail2D` points are absolute world positions (the `Transform` only feeds recording). `Line2D` points are offsets from the entity's `Transform`.
- **Render order:** GPU sprites → trails/lines → post-process effects → UI. Trails/lines are not z-sorted (`Line2D::z_order` is unused by the renderer).
- **Lighting:** trails/lines are drawn after the CPU content and before lighting, so they are lit like sprites (dark in unlit areas). `with_lighting(false)` on a `Trail2D`/`Line2D` opts out: `TrailRenderer` builds those into a separate geometry group (swapped in around the shared geometry code) that `render_unlit` draws onto the lit scene after lighting and before post-processing; without lighting both groups draw together. Regression tests in `tests/e2e/headless_engine.rs`: `lines_render_when_lighting_is_on`, `unlit_lines_stay_bright_in_the_dark`, `unlit_lines_render_without_lighting`.

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
