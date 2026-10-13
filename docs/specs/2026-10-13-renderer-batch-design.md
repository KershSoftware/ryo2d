# Renderer Batch (2b): One 2D Batch, Blend and Filter Modes, Polylines - Design

## Overview

Sub-project 2b of the professional-grade overhaul (`docs/research/2026-10-11-engine-audit.md`, Part 1 §3) follows the renderer core (`docs/specs/2026-10-12-renderer-core-design.md`, 2a). 2a gave the renderer a camera matrix, one texture registry, the resolution model, cameras as entities and render targets. 2b puts everything the world draws through **one 2D batch**, gives drawables **blend and filter modes**, rebuilds trails and lines as **joined polylines**, adds **mipmaps**, makes **parallax per camera**, adds a public **debug draw**, shares WGSL through **includes**, and stops **creating bind groups every frame**.

**Why:** after 2a, sprites (`graphics/sprite_gpu.rs`, 1.8k lines) and trails and lines (`trail/renderer.rs`, 2.9k lines) are still two GPU stacks with their own vertex formats, pipelines, buffers, camera bind groups and (for textured trails) texture cache, interleaved by z through trail "groups". Everything blends as straight alpha; sampling is one global filter set by the presentation, with no mipmaps, so a zoomed-out resolution-independent game aliases. Trails have no joins: corners gap or overlap, and translucent trails darken where segments overlap; a 65,536-vertex cap is shared with the debug overlays. Parallax is laid out for the main camera only, so a split screen shows the second player a background placed for the first. The camera uniform struct is pasted into 6 shaders and a fullscreen vertex shader into about 17. Bind groups are still created every frame by the compositor, the post-process chain, the transition fade and lighting, and lighting rewrites its shared light and shadow buffers once per lit camera.

**Owner decisions (2026-10-13):**
- **Approach:** one batch with two streams (instanced quads and indexed triangles), one spec, one plan, one branch.
- **Lighting:** stays its own renderer (2c moves it onto the HDR target); 2b only removes its per-frame bind groups and per-camera buffer rewrites.
- **Trails and lines:** rebuilt as joined polylines, not ported (this pulls "trails as ribbons", which the 2a spec left to the feature modules, into 2b).
- **Blend modes:** Alpha (default), Premultiplied, Additive, Multiply and Screen.
- **Filtering:** nearest or linear per drawable, with mipmaps on atlas pages and standalone textures.
- **Parallax:** layers become components.
- **Debug draw:** a small public immediate-mode API.
- **Breaking changes:** breaking APIs, demos and games is acceptable; networking stays out of scope beyond what compiles.

**Out of scope:** HDR, an `Rgba16Float` scene target and tonemapping, and moving lighting onto the batch (2c); a GPU UI, text, and the scissor stack's first user (sub-project 4); web asset loading over fetch (sub-project 5); the tilemap renderer, the shadow rewrite, skeleton sprite binding (feature modules); the remaining 2a minors (god rays relative to a main camera that does not cover the window, fog through transparent overlay cameras, device loss returning `Ok`, a parented main camera converting in parent space, a camera whose viewport changes size every frame recreating its image); mipmaps on render targets; blend modes for lights, shadows and composite tiles.

## The batch

A new module `src/core/render/batch/` owns everything the world draws. It replaces the world-drawing halves of `SpriteGpuRenderer` and `TrailRenderer`; the atlas, the texture registry, custom-shader compilation and the camera uniforms stay where 2a put them and are used by the batch.

- **Two streams**, each written into one GPU buffer once per frame, growing by doubling (no fixed cap):
  - **Quads:** the 128-byte instance from 2a (world-space centre, size, rotation, UV rect, tint, effect parameters, UV bounds), remapped into its atlas slot. Entity sprites, shadows, tiles, composite parts, particles, `Scene::render` sprites and parallax tiles.
  - **Triangles:** `Vertex2D { position: [f32; 2] (world), uv: [f32; 2], color: [f32; 4], params: [f32; 4] (style and effect), across: [f32; 2] (perpendicular, for displacement effects) }` (48 bytes) with `u32` indices. Trails, lines and debug drawing.
- **Batch key.** Every draw-list item carries `BatchKey { texture: TextureId, shader: BatchShader, blend: BlendMode, filter: Filter, scissor: Option<PixelRect> }`. `BatchShader` is `Sprite`, `SpriteEffect`, `Custom(source key)`, `Line` or `LineTextured`. `scissor` is `None` for every 2b drawable; it is in the key so sub-project 4's UI needs no batch change.
- **Draw list.** Items still sort once per frame by (z, kind, submission), with each item's `RenderLayers`, world bounds, the render target it shows, `lit`, and a new `only_camera: Option<Entity>` (parallax tiles and debug shapes built for one camera).
- **Per camera.** Each camera filters the sorted items (its layers, its view's bounds, not its own target, `only_camera`), then merges consecutive items whose resolved keys are equal into draws. Quads keep 2a's per-camera ranges of the instance buffer. Triangles are world-space and uploaded once; each camera draws the index ranges of the items it sees (debug shapes and other per-camera geometry are separate items).
- **Lit and unlit.** Every item has `lit`. Lit items draw in the `world` pass, unlit items in `unlit_world` after lighting. Any drawable can opt out of lighting, not only trails and lines.
- **Pipelines** are cached per `(BatchShader, BlendMode, stream, target format)`, created on first use and validated with 2a's non-blocking check (`Checked<P>`); a custom shader gets a pipeline per blend mode it is drawn with.
- **Bind groups** are created with their resource: one per camera (its uniform buffer, as in 2a) and one per `(texture page, sampler)`, made when the page or texture is. None per frame.
- The `world` and `unlit_world` passes replay the camera's draws. `TrailRenderer` (public in 2a) is removed, with the sprite renderer's world path, the trail groups and the trail texture cache.

```
gather (world space) -> DrawList (key, layers, bounds, lit, only_camera) -> sort once
  -> per camera: filter -> merge equal keys -> draws (quad ranges, index ranges)
  -> world pass: lit draws -> lighting -> unlit_world: unlit draws
```

## Blend and filter modes

- `pub enum BlendMode { Alpha, Premultiplied, Additive, Multiply, Screen }` (`Default` = `Alpha`). Every batch shader outputs premultiplied colour, so each mode is one fixed blend state:

  | Mode | Colour blend | Alpha blend | Texture colour |
  |---|---|---|---|
  | `Alpha` | `One`, `OneMinusSrcAlpha` | `One`, `OneMinusSrcAlpha` | straight; the shader premultiplies |
  | `Premultiplied` | `One`, `OneMinusSrcAlpha` | `One`, `OneMinusSrcAlpha` | already premultiplied |
  | `Additive` | `One`, `One` | `Zero`, `One` | straight |
  | `Multiply` | `Dst`, `OneMinusSrcAlpha` | `Zero`, `One` | straight |
  | `Screen` | `OneMinusDst`, `One` | `Zero`, `One` | straight |

  `Alpha` gives the same result as 2a's straight-alpha blending; the demo tour confirms it byte for byte.
- `pub enum Filter { Default, Nearest, Linear }` (the derived default is `Filter::Default`). `Filter::Default` follows the presentation: nearest for pixel art, linear for resolution-independent. Linear (and `Default` when it resolves to linear) samples through mipmaps; nearest never does.
- **Where they are set**, on the drawable:
  - `Renderable` gains `pub blend: BlendMode`, `pub filter: Filter`, `pub lighting: bool` (default `true`) with `with_blend`, `with_filter`, `with_lighting`.
  - `Trail2D` and `Line2D` gain `with_blend` and `with_filter` (`with_lighting` exists).
  - `ParticleEmitter` gains `blend`, `filter` and `lighting` fields with `with_blend`, `with_filter`, `with_lighting`.
  - `GpuRenderContext::add_sprite_instance(sprite, instance, z, has_effects)` becomes `add_sprite(&SpriteHandle, SpriteInstance, SpriteDraw)` with `#[derive(Clone, Copy)] pub struct SpriteDraw { pub z: i32, pub blend: BlendMode, pub filter: Filter, pub lit: bool, pub effects: bool }` and a hand-written `Default` (z 0, `Alpha`, `Filter::Default`, lit, no effects); `add_custom_sprite` takes a `SpriteDraw` in place of its `z_order`.
- `Renderable::render_target(target)` defaults to `BlendMode::Premultiplied`: a target cleared to transparent holds premultiplied colour (2a's dark translucent edges).
- Drop, blob and contact shadows draw with `Alpha` whatever their entity's blend.
- Custom sprite shaders keep returning straight colour from `sprite()`; the engine's wrapper premultiplies it.
- The `Material` component stays the shader choice only. Blend and filter live on the drawable, one place each.

## Textures

- **Mipmaps.** Atlas pages get 4 mip levels (1, ½, ¼, ⅛). Each slot's extruded border grows from 1 to 8 pixels and slots start on 8-pixel boundaries, so the smallest level still has a 1-pixel border and neighbours never bleed. Standalone textures (over 256 px) get a full mip chain. Render targets stay single-level.
- **Mip generation** happens on the CPU when a texture is first uploaded or re-uploaded (hot reload): each level averages 2x2 texels of the level above in linear light (pages keep 2a's raw sRGB bytes) and is written into the slot's place in every level. About 1.33x upload bytes, once per texture.
- **Samplers.** Two, created once: nearest (no mipmaps) and linear with linear mipmap filtering. Every texture page has one bind group per sampler.
- **Tiling inside the atlas.** A tiled texture (textured trails and lines with `TextureMode::Tile`) wraps its UV inside the slot rectangle in the shader and samples with `textureSampleGrad`, passing the gradients of the unwrapped UV so the mip choice does not jump at the seams. The trail renderer's private texture cache (2a, Task 7) goes. Parallax keeps one quad per visible tile and needs no wrap.
- **Named engine textures** (the shadow ellipse) are resolved once and their `TextureId` kept on the entity drawer; no name is hashed per draw item.

## Trails and lines

- **One polyline builder** (`render/batch/polyline.rs`) turns points into triangles on the triangle stream. Inputs: points, width (per point, from taper and end width), colour (per point, from end colour, fade and age), caps, joins, closed. `Trail2D` (recorded world points), `Line2D` (points relative to its entity's `GlobalTransform` translation), debug drawing and skeleton debug lines use it. `Trail2D` keeps its recording (minimum distance, emit interval, lifetime, fade distance).
- **Joins:** `pub enum LineJoin { Miter, Bevel, Round }` (`Default` = `Miter`). A miter longer than 4x the line's width falls back to a bevel (SVG's default miter limit). `LineJoin::None` is removed. Trails get joins (default `Miter`); `Trail2D::with_join` is added.
- **Caps:** `LineCap { Butt, Round, Square }`, unchanged.
- **No overlap:** the strip shares its edge vertices and adds one wedge per join, so no pixel is covered twice; a translucent trail is evenly translucent at its corners.
- **Per-vertex data:** `uv.x` runs along the line (0..1 when stretched, distance / tile length when tiled), `uv.y` across it (0..1). Style (`Solid`, `Soft`, `Glow`) and effect (`Lightning`, `Noise`, `UvScroll`, `Ribbon`) travel in `params`, and `across` carries the perpendicular for displacement, as in 2a's trail shader, which becomes the batch's line shader on the shared includes.
- **Degenerate input:** non-finite points are skipped, consecutive coincident points collapse, a line with fewer than two usable points draws nothing, and a zero or negative width draws nothing.
- **Batching:** each trail or line is one draw-list item with its own key; consecutive equal keys merge. 2a's grouping by (lit, z, layers) goes.

## Parallax

- `pub struct ParallaxLayer` (component) replaces `GpuParallaxBackground`, `GpuParallaxBuilder` and `GpuParallaxLayer`: `sprite: SpriteHandle`, `scroll: Vec2` (0 stays put, 1 moves with the world), `auto_scroll: Vec2` (pixels per second), `offset: Vec2`, `tile: TileMode`, `scale: f32`, `tint: Color`, `z_order: i32`, `blend`, `filter`, `lighting`, and `scrolled: Vec2` (the auto-scroll so far, kept by the engine); constructor `new(&SpriteHandle)` and a builder method per field. It takes `RenderLayers` like any drawable and needs no `Transform`.
- At extraction each layer is laid out for every camera on its layers, from that camera's `CameraView` (2a's layout: relative to the camera's centre on both axes, sized by the layer scale times the camera's scale, turned with the camera), as quads tagged `only_camera`.
- `advance_parallax` (`PostUpdate`, game time) adds `auto_scroll * delta` to `scrolled` each frame, so auto-scroll is smooth at any refresh rate, stops with the game clock, and a speed change does not jump.
- Scene cleanup despawns layers with the world. `TileMode` keeps its variants.

## Debug drawing

- `pub struct DebugDraw` (resource; `ctx.debug_draw() -> Mut<'_, DebugDraw>`, `ResMut<DebugDraw>` in systems): `line(a, b, color)`, `polyline(&[Vec2], color)`, `rect(center, size, color)`, `circle(center, radius, color)`, `arrow(from, to, color)`, `cross(at, size, color)`, all in world space. `width(px)` and `layers(RenderLayers)` return a `DebugPen` with the same methods. Defaults: 1 screen pixel wide, `RenderLayers::default()`.
- Shapes added during `FixedUpdate` (including `Scene::update`) are kept until the next fixed step begins, so a frame that runs several steps draws the last step's shapes and a frame without a step draws the previous step's (no flicker). Shapes added anywhere else are drawn by the next frame and then cleared.
- Shapes are unlit, drawn after everything else in the world (z `i32::MAX`), and built per camera so their width is in that camera's screen pixels at any zoom.
- The collider, velocity and origin overlays and the skeleton debug lines are `DebugDraw` users; their special path through the trail renderer goes.

## Bind groups, lighting buffers and shared WGSL

- **No bind group is created per frame** in the game frame. Each is created with its resource and rebuilt only when that resource is (resize, a new effect, a new camera, a new page):
  - **Compositor:** one per (source image: scene, lit, post-process A, B) x (letterboxed, identity), plus the UI's.
  - **Post-process chain:** each effect's bind groups for its possible input images, and its parameter buffer.
  - **Transition:** its copy texture's bind group.
  - **Lighting:** a camera's `LightTargets` holds its light, shadow and compose bind groups.
  - **Not counted:** the profiler and logger dev windows.
- **Lighting buffers:** light instances and shadow geometry are built and uploaded once per frame, before the camera passes; each lit camera only draws them. The per-camera rewrite of shared buffers and the per-camera shadow geometry go.
- **Shared WGSL:** `src/core/shaders/include/` holds `camera.wgsl` (the camera uniform struct and binding), `color.wgsl` (sRGB/linear, premultiply), `sample.wgsl` (slot-masked and wrapped sampling) and `fullscreen.wgsl` (the fullscreen triangle). Rust composes includes and body into one module source (no preprocessor crate). Batch, lighting, trail, post-process, composite, transition and custom game shaders all use them.
- **Allocations:** extraction keeps its scratch vectors on their owners across frames (entity collection, the batch collector's textures, camera uniforms, occluders, light instances, parallax tiles).
- **Pixel-art snapping** snaps an axis-aligned quad's top-left corner once and derives the other corners from its size, instead of snapping each corner, so a fractional-size quad keeps its width as it moves (2a minor 5).

## Public API changes

**Added:**
- `BlendMode`, `Filter`.
- `Renderable::{blend, filter, lighting}` and their builders; `Trail2D::{with_blend, with_filter, with_join}`; `Line2D::{with_blend, with_filter}`; `ParticleEmitter::{blend, filter, lighting}` and their builders.
- `SpriteDraw`, `GpuRenderContext::add_sprite`.
- `ParallaxLayer`.
- `DebugDraw`, `DebugPen`, `SceneContext::debug_draw`.

**Changed:**
- `GpuRenderContext::add_custom_sprite` takes a `SpriteDraw`.
- `LineJoin` loses `None`.
- `Renderable::render_target` defaults to `Premultiplied`.

**Removed:**
- `GpuRenderContext::add_sprite_instance`.
- `GpuParallaxBackground`, `GpuParallaxBuilder`, `GpuParallaxLayer`.
- `TrailRenderer`.

## Migration (in place, every step green)

1. Shared WGSL includes and composition; a test parses every composed shader with naga. No pixel changes.
2. Bind groups cached everywhere; a counting helper and the zero-per-frame test; lighting builds and uploads once per frame. No pixel changes.
3. The batch with its quad stream: sprites, shadows, tiles, composite parts, particles and `Scene::render` sprites move onto it; shaders output premultiplied colour under `Alpha`. No pixel changes.
4. Blend and filter modes on the drawables and `SpriteDraw`; render-target sprites default to `Premultiplied`.
5. Atlas mipmaps (8-pixel borders and alignment, CPU mips, trilinear sampler). No pixel changes at scale 1.
6. The triangle stream and the polyline builder: trails and lines with joins and wrapped tiling; the trail renderer is removed. The tour baseline is refreshed for trail and line pages, each checked by eye.
7. `DebugDraw`; the overlays and skeleton lines move onto it.
8. `ParallaxLayer`; demos and games migrated.
9. Documentation.
10. Verification and review.

## Testing

- **Unit:**
  - The blend-state table, and `Filter::Default` resolution per presentation.
  - Batch merging (equal keys merge, a key change or a trail splits, layers and `only_camera` filter).
  - Polylines: miter, bevel fallback past the limit, round; caps; closed lines; degenerate input; the triangles' total area equals the strip's area (no overlap).
  - Mip averaging in linear light; slot padding and alignment; the wrap maths.
  - Debug shape lifetimes across frames with and without fixed steps.
  - Parallax layout for two cameras.
  - Every composed shader parses.
- **Pixel (headless):**
  - Additive over a dark background brightens, multiply darkens, screen lightens and stays below additive.
  - A sprite showing a transparent-cleared render target has no dark edge.
  - Nearest and linear sampling differ on a magnified sprite.
  - A checkerboard drawn at ⅛ scale with linear filtering averages to grey (mipmaps), and stays crisp with nearest.
  - A translucent trail's corner is no darker than its segments.
  - A tiled textured line repeats its texture without a seam.
  - Two split-screen cameras show a parallax layer at their own offsets.
  - A debug line is 1 pixel wide in a zoomed-out camera.
  - A sprite with lighting off keeps full brightness in a dark lit scene.
- **Bind groups:** after warm-up frames, a scene with sprites, trails, particles, lighting with shadows, a post-process effect, a transition fade and two cameras creates no bind group in a frame.
- **Regression:** every 2a test, every demo-tour page byte-identical apart from the refreshed trail and line pages, the five games at frame 60, the web demo in Chromium, WebKit and Firefox, and the 10k-sprite headless frame within 20% of 0.96 ms with no more draws.

## Documentation

Rustdoc on every new public type. The `ryo-api` skill gains blend and filter modes, `SpriteDraw`, `ParallaxLayer` and `DebugDraw`, and loses the removed types; `ryo-renderer` describes the batch, its keys, streams, pipeline and bind-group caches, mipmaps and the includes; `ryo-game-dev` gets recipes (additive particles, a debug overlay, parallax for split screen); `ryo-particles` and `ryo-lighting` follow; both copies of each skill stay verbatim. `src/core/render/AGENTS.md`, `src/core/graphics/AGENTS.md`, `src/core/shaders/AGENTS.md` and the trail module's notes are updated, and `docs/architecture/overview.md` gets the batch.

## Design decisions

- **Two streams, not one:** sprites stay instanced (128 bytes each, no CPU expansion), and line geometry gets a vertex format that fits it. Expanding sprites into triangles would roughly double the bytes per sprite and add CPU work, against the goal of no slowdown.
- **Premultiplied output everywhere:** every blend mode becomes one fixed blend state, and render targets and camera images compose correctly without special cases.
- **Blend and filter on the drawable, not on `Material`:** one place to set each, and trails, lines, particles and `Scene::render` sprites need no material component.
- **Mipmaps by padding the atlas rather than leaving it single-level:** the atlas keeps its few-draws batching, and zoomed-out games stop aliasing; the cost is atlas space.
- **Parallax as components:** a layer is laid out per camera from data the engine owns, with no per-frame game call, and goes with the world on a scene switch.
- **Lighting stays a separate renderer** until 2c moves it to the HDR target, so it is restructured once.

## Risks

- 8-pixel borders cost atlas space; sprite-heavy games may evict sooner. The atlas tests and the 10k-sprite benchmark watch it.
- Building mipmaps on the CPU adds upload time when many textures first appear (a hitch on the first frame of a texture-heavy scene).
- Custom shaders compile a pipeline per blend mode they are drawn with; creation is lazy.
- Trails look different at their corners (intended); the refreshed tour pages are checked by eye.
- Premultiplied output must match 2a's straight-alpha results byte for byte; step 3 proves it before anything else changes.
