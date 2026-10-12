# AGENTS.md - Graphics Module

The renderers the render passes call (`WgpuRenderer`, `SpriteGpuRenderer`, the compositor), sprites, the camera, sprite animation, tiles and parallax. The frame itself (extraction, draw order, placement, passes) is described in `src/core/render/AGENTS.md`.

## Rendering

```
World (queried each frame):
  Renderable / CompositeTile ─→ EntityDrawer ────────┐
  Scene::render(gpu) ─→ BatchCollector ──────────────┼→ DrawList (z-sorted) → SpriteGpuRenderer → GPU
  Trail2D / Line2D ─→ TrailRenderer ─────────────────┘
UI:
  Scene::ui(ui, ctx) ─→ UiContext ─→ UI buffer ─→ composited last
```

The whole world draws on the GPU. The only CPU drawing left is the UI (`UiContext` draws `Sprite`s into the UI buffer).

**Draw order:** entities, `Scene::render` sprites (particles, parallax) and trails/lines sort together by `z_order`. Equal z: entity, then `render` sprite, then trail/line; within a kind, in submission order. Lighting, the transition fade, post-processing and the UI follow; the UI is composited last, untouched by effects. Parallax layers need a negative z to sit behind entities at z 0.

## Resources

| Resource | What | In scenes |
|----------|------|-----------|
| `Camera` | position, zoom, viewport, trauma shake | `ctx.camera` (taken out of the world during callbacks) |
| `Gpu` | `Arc<Device>` and `Arc<Queue>`, inserted once the GPU is up | systems take `Res<Gpu>` |

Renderers are engine-owned, not resources. `GpuRenderContext` is the only GPU access a scene gets: `Scene::render(&mut self, gpu: &mut GpuRenderContext)` runs once per frame at present time for the visible scenes, with `gpu.batch`, `gpu.camera` and a read-only `gpu.world`. `SpriteInstance` positions are screen pixels: convert with `gpu.camera.world_to_screen`. Positions read from `gpu.world` are true (uninterpolated) `GlobalTransform`s.

## Sprites and Anchors

- `Renderable::anchor` (default `Anchor::Center`) picks the point of a single sprite that sits on the entity's position; rotation and scale happen about it. Things laid out as a grid or from a corner (backgrounds, tiles, panels) take `.with_anchor(Anchor::TopLeft)`; things that move, rotate or scale keep the default.
- Tiled runs (`Renderable::tiled`) and composite parts ignore the anchor: they are laid out from the entity's position (the first tile's top-left), as are `GroupedCollider`s (see `src/core/physics/AGENTS.md`).
- Colliders are centred on the origin, so a centred sprite and its collider line up with no offset maths.

## Shader Time

Sprite globals carry game elapsed time in `x` (`time.game.elapsed()`), which sprite effects and custom sprite shaders (`in.time`) animate on, so they freeze under pause and slow under slow motion; `w` is unused. Post-process effects, lighting and trails use real time.

## GPU Sprite Batching

- `BatchCollector` keeps each `Scene::render` sprite in submission order (texture id shared per texture) and pushes them into the `DrawList`. There are no per-texture batches: `SpriteGpuRenderer::prepare_world` merges **consecutive** sprites on one atlas page and pipeline into one draw, so many small textures still cost few draws.
- Sprites pass their `SpriteHandle`; its texture is registered and uploaded the first time a frame draws it, and dropped once no frame has drawn it for 600 frames. Two handles are two textures, whatever their names.
- **Synchronization**: all instance data is written to the instance buffer **before** the render pass begins (`prepare_world`), so the GPU never reads a buffer still being written. Each draw binds `instance_buffer.slice(offset..end)`.

## Texture Atlas and Shading

- Sprite images are packed into shared atlas pages (2048x2048, up to 8, 1-px extruded border so scaling never bleeds neighbours). Images over 256 px get their own texture. Games keep their texture ids and normalized UVs; UVs outside a sprite's own rect are transparent (there is no UV wrapping). When every page is full, the least recently used page that nothing drew from this frame is cleared in one go (scattered single evictions rarely free a large enough rect, since the bucketed allocator reclaims space a bucket at a time); only if every page was drawn from this frame are single entries evicted, least recently used first. Every 60 frames, entries unused for 300 frames are freed and empty pages at the end are released. Textures uploaded by id (`render` sprites, particles, parallax) that are neither uploaded nor drawn for 600 frames are dropped with their pixels; entity sprites follow their own 600-frame rule.
- Tint and effects are computed in sRGB space and quantised to 8 bits, as a CPU blit would; translucent overlaps blend in linear space. Outline, glow and directional outline draw outside the sprite.
- Blob and contact shadows are one tinted ellipse texture; drop shadows are the sprite's silhouette.
- Debug overlays (colliders, velocities, origins) are unlit lines on top. `DebugStats::draw_calls` counts the world draws.

## Parallax

- A builder-driven object (`GpuParallaxBackground`), not ECS components: layers are one cohesive visual, and render-time tiling is cheaper than spawning duplicate entities. The scene owns it, calls `update(dt)` and draws it with `render(gpu)` from `Scene::render`.
- Layers draw at their `z_order` among the entities; they sit behind them only if their z is below the entities' (the examples use -100..-20).

## Camera Shake

- Trauma-based. Offset = smooth noise × `max_offset` × `intensity_curve(trauma)`, where the default curve `Ease::InQuad` gives trauma².
- `camera_shake_system` (`PostUpdateSet::Camera`) calls `update_shake(time.real.delta())`, so shake keeps working through pause, slow motion and hitstop.
- The offset is applied in `world_to_screen()`. `screen_to_world()` does NOT remove it.
- `SceneCleanup::CAMERA` resets position, zoom and shake (the viewport is kept).

## Notes for AI Agents

1. **Camera is separate from rendering**: it only handles coordinate transforms; the renderer reads the `Camera` resource each frame.
2. **Animation updates automatically**: `animate_sprites` (`FixedSet::Juice`) advances every `Animator` / `AnimationController` / `ValidatedAnimator` by the fixed timestep and swaps the entity's `Renderable.sprite` only when the frame changed.
