# AGENTS.md - Graphics Module

The renderers the render passes call (`WgpuRenderer`, `SpriteGpuRenderer`, the compositor, camera images), the screen mapping, cameras and render targets, sprites, sprite animation, tiles and parallax. The frame itself (extraction, draw order, placement, passes) is described in `src/core/render/AGENTS.md`.

## Rendering

```
World (queried each frame):
  Renderable / CompositeTile ─→ EntityDrawer ────────┐
  Scene::render(gpu) ─→ BatchCollector ──────────────┼→ DrawList (z-sorted, world space)
  Trail2D / Line2D ─→ TrailRenderer ─────────────────┘        │
Cameras (Camera2d + CameraView) ─→ plan_cameras ──→ per camera: its sprites (layers, bounds)
                                                     → its image → cameras pass → scene image
UI:
  Scene::ui(ui, ctx) ─→ UiContext ─→ UI buffer ─→ composited last
```

The whole world draws on the GPU. The only CPU drawing left is the UI (`UiContext` draws `Sprite`s into the UI buffer).

**Draw order:** entities, `Scene::render` sprites (particles, parallax) and trails/lines sort together by `z_order`. Equal z: entity, then `render` sprite, then trail/line; within a kind, in submission order. Lighting, the transition fade, post-processing and the UI follow; the UI is composited last, untouched by effects. Parallax layers need a negative z to sit behind entities at z 0.

## Resources

| Resource | What | In scenes |
|----------|------|-----------|
| `ScreenMapping` | the one mapping between the window and screen space (presentation, physical size, scale factor) | `ctx.screen_size()`; systems take `Res<ScreenMapping>` |
| `RenderTargets` | the render targets games asked for (sizes); the GPU textures follow at the next frame | `ctx.render_targets()` |
| `Gpu` | `Arc<Device>` and `Arc<Queue>`, inserted once the GPU is up | systems take `Res<Gpu>` |

Cameras are entities, not a resource (`camera2d.rs`, `camera_mut.rs`): `Camera2d` (zoom, projection, viewport, order, target, clear) with a `Transform`, `CameraShake`, and the `CameraView` the engine computes each frame (`camera_view_system`, `PostUpdateSet::Camera`, from the interpolated transform, so an `Interpolated` camera moves smoothly). The main camera (`MainCamera`) is spawned with the world over the design area (`MainCameraDesign`); `ctx.camera()` is a `CameraMut` over it.

Renderers are engine-owned, not resources. `GpuRenderContext` is the only GPU access a scene gets: `Scene::render(&mut self, gpu: &mut GpuRenderContext)` runs once per frame at present time for the visible scenes, with `gpu.batch`, `gpu.camera()` (the main camera's `CameraView`) and a read-only `gpu.world`. `SpriteInstance`s are world-space quads (centre and size in world units); something fixed on screen is placed with `gpu.camera().screen_to_world`. Positions read from `gpu.world` are true (uninterpolated) `GlobalTransform`s.

## Sprites and Anchors

- `Renderable::anchor` (default `Anchor::Center`) picks the point of a single sprite that sits on the entity's position; rotation and scale happen about it. Things laid out as a grid or from a corner (backgrounds, tiles, panels) take `.with_anchor(Anchor::TopLeft)`; things that move, rotate or scale keep the default.
- Tiled runs (`Renderable::tiled`) and composite parts ignore the anchor: they are laid out from the entity's position (the first tile's top-left), as are `GroupedCollider`s (see `src/core/physics/AGENTS.md`).
- Colliders are centred on the origin, so a centred sprite and its collider line up with no offset maths.

## Camera Uniforms and Shader Time

Every camera has one uniform buffer (`camera_gpu.rs`: `CameraGlobals { view_proj, resolution, game_time, real_time, snap }`, 96 bytes), bound as group 0 by sprites, trails, lights and shadows, so all of them move, zoom, turn and shake together. `resolution` is the camera's image size; `snap` turns on pixel snapping of axis-aligned sprites (pixel art). Sprite effects and custom sprite shaders (`in.time`) animate on `game_time`, so they freeze under pause and slow under slow motion; post-process effects, lighting and trails use real time.

## GPU Sprite Batching

- `BatchCollector` keeps each `Scene::render` sprite in submission order (texture id shared per texture) and pushes them into the `DrawList`. There are no per-texture batches: `SpriteGpuRenderer::prepare_world` merges **consecutive** sprites on one atlas page and pipeline into one draw, so many small textures still cost few draws.
- Sprites pass their `SpriteHandle`; its texture is registered and uploaded the first time a frame draws it, and dropped once no frame has drawn it for 600 frames. Two handles are two textures, whatever their names.
- **Synchronization**: all instance data is written to the instance buffer **before** the render pass begins (`prepare_world`), so the GPU never reads a buffer still being written. Each camera's sprites have a range of their own; each draw binds `instance_buffer.slice(offset..end)`.

## Texture Atlas and Shading

- Every texture has one `TextureId` in the `TextureRegistry` (`texture_registry.rs`): sprites by handle, named pixel textures, and render targets (`TextureSource::Target`, never uploaded, evicted or packed).
- Sprite images are packed into shared atlas pages (2048x2048, up to 8, 1-px extruded border so scaling never bleeds neighbours). Images over 256 px get their own texture. Games keep their texture ids and normalized UVs; UVs outside a sprite's own rect are transparent (there is no UV wrapping). When every page is full, the least recently used page that nothing drew from this frame is cleared in one go (scattered single evictions rarely free a large enough rect, since the bucketed allocator reclaims space a bucket at a time); only if every page was drawn from this frame are single entries evicted, least recently used first. Every 60 frames, entries unused for 300 frames are freed and empty pages at the end are released. Textures uploaded by id (`render` sprites, particles, parallax) that are neither uploaded nor drawn for 600 frames are dropped with their pixels; entity sprites follow their own 600-frame rule.
- Tint and effects are computed in sRGB space and quantised to 8 bits, as a CPU blit would; translucent overlaps blend in linear space. Outline, glow and directional outline draw outside the sprite.
- Blob and contact shadows are one tinted ellipse texture; drop shadows are the sprite's silhouette.
- Render targets are atlas pages of their own (`SlotPage::Target`): an `Rgba8UnormSrgb` texture cameras draw into, sampled by sprites through an `Rgba8Unorm` view (raw sRGB bytes, like the pages), so a sprite showing a target draws through the ordinary sprite runs.
- Debug overlays (colliders, velocities, origins) are unlit lines on top, on render layer 0. `DebugStats::draw_calls` counts the world draws of every camera.

## Parallax

- A builder-driven object (`GpuParallaxBackground`), not ECS components: layers are one cohesive visual, and render-time tiling is cheaper than spawning duplicate entities. The scene owns it, calls `update(dt)` and draws it with `render(gpu)` from `Scene::render`.
- Layers draw at their `z_order` among the entities; they sit behind them only if their z is below the entities' (the examples use -100..-20).
- Layers follow the main camera on both axes, scaled by its zoom and turned with it; other cameras see them where the main camera put them.

## Camera Shake

- Trauma-based (`CameraShake`, a component on the camera). Offset = smooth noise × `max_offset` × `intensity_curve(trauma)`, where the default curve `Ease::InQuad` gives trauma².
- The shake system (`PostUpdateSet::Camera`) advances it on `time.real.delta()`, so shake keeps working through pause, slow motion and hitstop.
- The offset goes into the camera's `view_proj` (in screen units, after zoom), so everything in the world shakes and the UI does not. `CameraView::world_to_screen` includes it; `screen_to_world` removes it.
- `SceneCleanup::CAMERA` resets the main camera's position, zoom, rotation, projection and shake (its target is kept).

## Notes for AI Agents

1. **Cameras are data**: `Camera2d` and its `Transform` say what a camera sees; the engine computes `CameraView` each frame and the renderer reads the views, never the other way round.
2. **Animation updates automatically**: `animate_sprites` (`FixedSet::Juice`) advances every `Animator` / `AnimationController` / `ValidatedAnimator` by the fixed timestep and swaps the entity's `Renderable.sprite` only when the frame changed.
