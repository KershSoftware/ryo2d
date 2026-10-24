# AGENTS.md - Graphics Module

The renderers the render passes call (`WgpuRenderer`, `SpriteGpuRenderer` (textures and game shader sources), the compositor, camera images), the screen mapping, cameras and render targets, sprites, sprite animation, tiles and parallax. The frame itself (extraction, draw order, placement, passes) and the 2D batch that draws the world are described in `src/core/render/AGENTS.md`.

## Rendering

```
World (queried each frame):
  Renderable / CompositeTile ─→ EntityDrawer ────────┐
  Scene::render(gpu) ─→ BatchCollector ──────────────┤
  ParallaxLayer ─→ laid out per camera ──────────────┼→ DrawList (z-sorted, world space)
  Trail2D / Line2D ─→ polylines (triangles) ─────────┤        │
  Text2d ─→ glyph quads per camera (glyph pages) ────┤        │
  DebugDraw ─→ polylines per camera ─────────────────┘        │
Cameras (Camera2d + CameraView) ─→ plan_cameras ──→ per camera: Batch2D draws (layers, bounds)
                                                     → its image → cameras pass → scene image
UI:
  Scene::ui(ui, ctx) ─→ UiContext ─→ UiCommands ─→ UI camera (batch; glyphs from the same pages) ─→ UI image ─→ composited last
```

Everything draws on the GPU, the UI included: `UiContext` records commands, and the batch draws them through the UI camera into the compositor's UI image (`render/AGENTS.md`). Text is laid out by the text module (`src/core/text/AGENTS.md`); this module keeps the glyph pages its bitmaps are placed in.

**Draw order:** entities, world text, `Scene::render` sprites (particles, parallax) and trails/lines sort together by `z_order`. Equal z: entity, then world text, then `render` sprite, then trail/line; within a kind, in submission order. Lighting, the transition fade, post-processing and the UI follow; the UI is composited last, untouched by effects. Parallax layers need a negative z to sit behind entities at z 0.

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
- Tiled runs (`Renderable::tiled`) and composite parts ignore the anchor: they are laid out from the entity's position (the first tile's top-left), and `build_with_collider` returns a rect `Collider` already offset to cover them (see `src/core/physics/AGENTS.md`).
- Colliders are centred on the origin, so a centred sprite and its collider line up with no offset maths.

## Camera Uniforms and Shader Time

Every camera has one uniform buffer (`camera_gpu.rs`: `CameraGlobals { view_proj, resolution, game_time, real_time, snap }`, 96 bytes), bound as group 0 by sprites, trails, lights and shadows, so all of them move, zoom, turn and shake together. `resolution` is the camera's image size; `snap` turns on pixel snapping of axis-aligned sprites (pixel art). Sprite effects and custom sprite shaders (`in.time`) animate on `game_time`, so they freeze under pause and slow under slow motion; post-process effects, lighting and trails use real time.

## GPU Sprite Batching

- `BatchCollector` keeps each `Scene::render` sprite in submission order (texture id shared per texture) with its `SpriteDraw` (z, blend, filter, lit, effects) and pushes them into the `DrawList`. There are no per-texture batches: the batch (`render/batch`) merges **consecutive** drawables with equal draw states (page, shader, blend, sampling) into one draw, so many small textures still cost few draws.
- Sprites pass their `Handle<Image>`; its texture is registered and uploaded the first time a frame draws it, uploaded again when the image's version moves (a large new version over frames: Texture Atlas and Shading, Reloads), and dropped when the image is freed (`sync_images`, from `AssetEvent::Removed`). Clones of a handle share one texture; two images are two textures. Textures uploaded by name go once no frame has uploaded or drawn them for 600 frames.
- `SpriteGpuRenderer` keeps the textures (registry, atlas, names) and compiles game sprite shaders to WGSL (`shade_custom`); the batch makes their pipelines. It makes the frame's textures resident in `prepare_textures` before the batch lays the frame out.

## Texture Atlas and Shading

- Every texture has one `TextureId` in the `TextureRegistry` (`texture_registry.rs`): sprites by handle, named pixel textures, and render targets (`TextureSource::Target`, never uploaded, evicted or packed).
- Sprite images are packed into shared atlas pages (2048x2048, up to 8). Each slot starts on an 8-pixel grid with an 8-pixel extruded border, so no level of the pages' four mip levels bleeds a neighbour. Images over 256 px get their own texture with a full mip chain. Mip levels are made on the CPU, averaging 2x2 texels in linear light weighted by alpha. The image loader makes them on its thread with the bordered first level (`PreparedUpload`, `Image::prepare_upload`): on the workers for every image, and under `load_now` for one over 256 px, which keeps its levels below the first (`Image::mips`), so a texture freed and drawn again does not make them. The rest is the renderer's once it takes it (the first time it uploads that version), freed once written; an image made or changed in code (`data_mut` drops what was made) has its levels made when it is uploaded.
- Two samplers, made once: nearest (level 0 only) and linear (trilinear). Every page, standalone texture and render target has a bind group per sampler, made with the page; a drawable's `Filter` resolves for the presentation (`Default`: nearest for pixel art, linear otherwise) and picks one. Games keep their texture ids and normalized UVs; UVs outside a sprite's own rect are transparent (there is no UV wrapping). When every page is full, the least recently used page that nothing drew from this frame is cleared in one go (scattered single evictions rarely free a large enough rect, since the bucketed allocator reclaims space a bucket at a time); only if every page was drawn from this frame are single entries evicted, least recently used first. Every 60 frames, entries unused for 300 frames are freed and empty pages at the end are released. Textures uploaded by id (`render` sprites, particles, parallax) that are neither uploaded nor drawn for 600 frames are dropped with their pixels; entity sprites follow their own 600-frame rule.
- **Reloads.** A new version of an image that has a texture, its upload made ahead and more than a frame's budget (`UPLOAD_BUDGET`, 2 MiB: a 2048 px image's 22.7 MB take 11 frames), is written into a new texture of its own a band of rows a frame, level by level, with what the frame's other image writes leave of the budget (`TextureAtlas::write_spreads`, at the end of `prepare_textures`), while the old texture draws. It starts at the start of a drawn frame (`sync_images`, from `AssetEvent::Modified`; from the draw for an event missed), and the new texture, once whole, takes the old one's place at the start of the next drawn frame, before anything draws, so no frame draws a partly written texture and all of a frame shows one version. Until then sprites draw at the old size (`ShownImage::size`) and the UI's kept image follows the version the texture shows (`StampSources::showing`). A newer version landing meanwhile starts again; freeing the image, or the standalone cap evicting it, drops both textures at once, and one evicted is uploaded at once when next drawn. A first upload, one within the budget and one made in code are written at once. The UI lays an image it draws at its natural size out at its new size from the frame it lands, so a resized large reload draws its old pixels in the new rect until the swap.
- Tint and effects are computed in sRGB space and quantised to 8 bits, as a CPU blit would; translucent overlaps blend in linear space. Outline, glow and directional outline draw outside the sprite.
- Blob and contact shadows are one tinted ellipse texture; drop shadows are the sprite's silhouette.
- Render targets are atlas pages of their own (`SlotPage::Target`): a `SCENE_FORMAT` (`Rgba16Float`) texture cameras draw into, sampled by sprites as linear premultiplied colour (`PageKind::Linear`, the sprite shader's `*_linear` entries), so a sprite showing a target draws through the ordinary sprite path and keeps light above 1. What a camera draws into a target is premultiplied, so `Renderable::render_target` defaults to `BlendMode::Premultiplied`.
- **Emission** is the instance's last transform lane (`SpriteInstance::with_emission` / `emission()`, stored as `emission - 1` so a zeroed instance has no glow); `SpriteDraw::emission` (default 1) multiplies the instance's. Emission above 1 makes a sprite glow past white and feed bloom.
- **Glyph pages** (`glyph_atlas.rs`, `SlotPage::Glyph`) sit beside the sprite atlas: `Mask` (`R8Unorm` coverage), `Color` (`Rgba8Unorm`, sRGB bytes like the sprite pages) and `Msdf` (`Rgba8Unorm`, linear distances), each 1024x1024, up to 8 per format, no mips, every glyph inside a 1 px transparent border. A glyph is rasterized and placed on first use (`SpriteGpuRenderer::glyph`); when a format is full the least recently used page is cleared whole, never one holding a glyph drawn this frame; a format full of this frame's glyphs is warned once per run (a sustained overflow runs out every frame). A glyph bigger than a page is refused, warned once and remembered, so it is never rasterized again (the text emitter caps glyph images well under a page, `src/core/text/AGENTS.md`). Bind groups (nearest and linear) are made with the page. At the start of each drawn frame (`sync_fonts`, before any text is drawn) the glyphs of a face freed or changed since are forgotten (their room deallocated, which the bucketed allocator takes back a bucket at a time) and the pages they were on change generation; a page left empty is freed with its texture, leaving a hole the next page of its format takes, so other pages keep their indices.
- Debug overlays (colliders, velocities, origins) are `DebugDraw` shapes, one screen pixel wide on top. `DebugStats::draw_calls` counts the world draws of every camera.

## Parallax

- `ParallaxLayer` is a component (`parallax.rs`): spawn one entity per layer. At extraction each layer is laid out for every camera on its `RenderLayers` from that camera's own view (`layout`, at most `MAX_PARALLAX_TILES` quads), and its quads join the draw list as sprites seen by that camera alone (`only_camera`), so split screen and minimaps each get their own parallax.
- `scroll` (per axis) is how far it moves with the camera (0 fixed on screen, 1 in the world); `auto_scroll` advances `scrolled` on game time (`advance_parallax`, `PostUpdateSet::Extract`); `offset` places it on screen; `tile` repeats it horizontally.
- Layers draw at their `z_order` among the entities; they sit behind them only if their z is below the entities' (the examples use -100..-20).

## Camera Shake

- Trauma-based (`CameraShake`, a component on the camera). Offset = smooth noise × `max_offset` × `intensity_curve(trauma)`, where the default curve `Ease::InQuad` gives trauma².
- The shake system (`PostUpdateSet::Camera`) advances it on `time.real.delta()`, so shake keeps working through pause, slow motion and hitstop.
- The offset goes into the camera's `view_proj` (in screen units, after zoom), so everything in the world shakes and the UI does not. `CameraView::world_to_screen` includes it; `screen_to_world` removes it.
- `SceneCleanup::CAMERA` resets the main camera's position, zoom, rotation, projection and shake (its target is kept).

## Notes for AI Agents

1. **Cameras are data**: `Camera2d` and its `Transform` say what a camera sees; the engine computes `CameraView` each frame and the renderer reads the views, never the other way round.
2. **Animation updates automatically**: `animate_sprites` (`FixedSet::Juice`) advances every `Animator` / `AnimationController` / `ValidatedAnimator` by the fixed timestep and swaps the entity's `Renderable.sprite` only when the frame changed.
