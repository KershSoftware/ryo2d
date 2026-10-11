# AGENTS.md - Graphics Module

## Rendering

```
World:  Renderable / CompositeTile → EntityDrawer ─┐
        render_gpu() → BatchCollector ─────────────┼→ DrawList (z-sorted) → SpriteGpuRenderer → GPU
        Trail2D / Line2D → TrailRenderer ──────────┘
UI:     Sprite.draw() / draw_scaled() → UI buffer → composited last
```

The whole world draws on the GPU. The only CPU drawing left is the UI (`UiContext` draws `Sprite`s into the UI buffer).

The renderers are drawn by the engine's `RenderPipeline` (`core/render/`, see its AGENTS.md). Scenes can toggle built-in passes or insert their own through `ctx.render_pipeline`. The graphics module holds the renderers the passes call; it has no render entry points of its own besides `WgpuRenderer::render_frame`, which runs the pipeline.

**Draw order**: entities, `render_gpu()` sprites (incl. particles and parallax) and trails/lines sort together by `z_order`. Equal z: entity, then `render_gpu` sprite, then trail/line; within a kind, in submission order. Lighting, the transition fade, post-processing and the UI follow; the UI is composited last, untouched by effects. Parallax layers need a negative z to sit behind entities at z 0.

## GPU Sprite Batching

- `BatchCollector` keeps each `render_gpu` sprite in submission order (texture id shared per texture) and pushes them into the `DrawList`. There are no per-texture batches: `SpriteGpuRenderer::prepare_world` merges **consecutive** sprites on one atlas page and pipeline into one draw, so many small textures still cost few draws.
- **Synchronization**: all instance data is written to the instance buffer **before** the render pass begins (`upload_world`), so the GPU never reads a buffer still being written. Each draw binds `instance_buffer.slice(offset..end)`.

## Texture Atlas and Shading

- Sprite images are packed into shared atlas pages (2048x2048, 1-px extruded border so scaling never bleeds neighbours). Images over 256 px get their own texture. Games keep their texture ids and normalized UVs; UVs outside a sprite's own rect are transparent (there is no UV wrapping). Unused entries are evicted and re-uploaded automatically: when every page is full, the least recently used page that nothing drew from this frame is cleared in one go (scattered single evictions rarely free a large enough rect, since the bucketed allocator reclaims space a bucket at a time); only if every page was drawn from this frame are single entries evicted, least recently used first.
- Tint and effects are computed in sRGB space and quantised like the old CPU path, so opaque tinted pixels look as before. Translucent overlaps blend in linear space and may look slightly different. Outline, glow and directional outline draw outside the sprite (`DirectionalOutline` works on the GPU).
- Blob and contact shadows are one tinted ellipse texture; drop shadows are the sprite's silhouette.
- Debug overlays (colliders, velocities, origins) are unlit lines on top. `DebugStats::draw_calls` counts the world draws.

## Parallax

- A builder-driven system (`GpuParallaxBackground`), not ECS components: layers are one cohesive visual, and render-time tiling is cheaper than spawning duplicate entities.
- Rendered from `render_gpu()` at each layer's `z_order`; `render_gpu` sprites no longer sit behind entities automatically, so it sits behind them only if the layers' z is below theirs (the examples use -100..-20).

## Camera Shake

- Trauma-based. Offset = smooth noise × `max_offset` × `intensity_curve(trauma)`, where the default curve `Ease::InQuad` gives trauma².
- The engine calls `update_shake()` with **unscaled** dt, so shake keeps working during slow-mo/hitstop.
- The offset is applied automatically in `world_to_screen()`. `screen_to_world()` does NOT remove it.

## Notes for AI Agents

1. **Camera is separate from rendering**: it only handles coordinate transforms.
2. **Animation updates automatically**: `systems/animation.rs` updates `Animator`/`AnimationController` each frame.
