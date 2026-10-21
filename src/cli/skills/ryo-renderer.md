---
name: ryo-renderer
description: Working on the wgpu renderer, shaders, and GPU batching.
metadata:
  version: 2.0.3
---

# Ryo2D Renderer Specialist

You help developers work on the rendering system.

## Resources

- **Extraction**: `src/core/engine/extract.rs` (`present_frame`), `src/core/render/entities.rs`, `draw_list.rs`
- **Render Pipeline**: `src/core/render/` (pass-based architecture)
- **Pipeline**: `src/core/render/pipeline.rs`
- **Pass Trait**: `src/core/render/pass.rs`
- **Built-in Passes**: `src/core/render/passes/`
- **The 2D batch**: `src/core/render/batch/` (`key.rs` keys and states, `plan.rs` per-camera draws, `pipelines.rs`, `gpu.rs` `Batch2D`, `polyline.rs` and `lines.rs` trails and lines), `src/core/render/debug_draw.rs`
- **Textures**: `src/core/graphics/sprite_gpu.rs` (textures, game shader sources, `BatchCollector`, `SpriteDraw`), the atlas (`atlas.rs`) and the texture registry (`texture_registry.rs`); bind groups through `graphics/bind_groups.rs`
- **Cameras**: `src/core/graphics/{camera2d,camera_mut,camera_gpu,camera_images,render_targets,render_layers}.rs`
- **Screen and window**: `src/core/graphics/screen.rs` (`ScreenMapping`), `compositor.rs`
- **Shaders**: `src/core/shaders/`
- **Text and the UI**: `src/core/render/text.rs` (lays glyphs, bitmap cells and icons into a draw list, shared by world text and the UI), `src/core/render/ui.rs` (the UI camera, `UiSpace`, and what each UI command becomes), `src/core/render/ui_renderer/` (the UI's own renderer: its instances, one pipeline, texture runs), `src/core/graphics/glyph_atlas.rs` (the glyph pages); the text itself (fonts, layout, glyph bitmaps) is `src/core/text/`, which the renderer sees only as `GlyphImage`s
- **Lighting**: `src/core/lighting/`
- **Trails and lines**: `src/core/trail/` (components)
- **Parallax**: `src/core/graphics/parallax.rs` (`ParallaxLayer`)

## Extraction

The world is read fresh every frame; there is no render cache to invalidate. Everything is gathered once, in world space, and sorted once; each camera then keeps what it sees. `present_frame` (after `PostUpdate`) does:

1. Scenes' `Scene::render(gpu)` hooks fill the `BatchCollector` (particles, many-sprite effects), in world space; lights are gathered at their positions.
2. The render targets' GPU textures follow `RenderTargets`; the cameras are planned (`plan_cameras`): texture cameras, then window cameras, each by `order`. The first window camera draws straight into the scene image when it covers the window and clears; every other window camera gets an image of its own, sized to its viewport (`CameraImages`); a texture camera draws straight into its target, or, when its viewport covers only part of it, into an image of its own placed in that part after its passes.
3. Each camera gets a uniform buffer (`CameraUniforms`, one `CameraGlobals` each: `view_proj`, `resolution` = its image size, `game_time`, `real_time`, `snap`), bound as group 0 by the batch, lights and shadows alike.
4. The `Scene::render` sprites join the frame's `DrawList`; each `ParallaxLayer` is laid out for every camera on its layers from that camera's view (items for that camera alone, `only_camera`).
5. One query over the renderables (plus `CompositeTile`) adds the entities, skipping quads no camera on the entity's layers can see. `Disabled` entities are skipped by the query itself.
6. World text: `Text2d`s whose text, style or fonts changed are laid out again, and each camera that sees a block gets its glyphs (made at the size that camera sees) as glyph items for that camera alone.
7. The UI: unless its image is kept, the UI renderer builds the frame's UI commands (rects, sprites, lines, text) into instances of its own in painter's order; it does not use the frame's draw list or the batch's cameras.
6. Trails and lines become joined polylines on the batch's triangle stream, one item each; `DebugDraw`'s shapes (and the overlays) are built for each camera in its own screen pixels.
7. `Batch2D::prepare` makes the textures resident, lays each camera's lit and unlit items into draws (on its layers, inside its `CameraView::bounds`, not showing its own target) and uploads the quads and triangles once.
8. The render pipeline runs; `world` and `unlit_world` replay the camera's draws.

Placement rules:

- Entities with `PreviousGlobalTransform` (the `Interpolated` marker on them or an ancestor) draw at `render_global(global, previous, time.fixed.overstep_fraction())`; everything else draws at `GlobalTransform`. Composite tiles, lights, occluders and lines on an interpolated entity are blended the same way, and so is a camera's own transform when its `CameraView` is computed.
- A single sprite is placed by `Renderable::anchor` (default `Anchor::Center`); rotation and scale happen about the anchor. Tiled runs and composite parts are laid out from the entity's position (the first tile's top-left).
- `YSort` sorts by the sprite's feet (`foot_offset` of the way down from its top edge, wherever the anchor is).
- Instances carry world-space quads (centre, size, rotation); the camera's `view_proj` takes them to its image. In pixel-art mode the sprite vertex shader rounds an axis-aligned quad's top-left and size to whole target pixels once (`snap`), so it keeps its width as it moves. A drawable's `Filter::Default` samples nearest there and linear (mipmapped) otherwise.
- Sprite effects and a custom sprite shader's `in.time` run on game time; lighting, trails, transitions and post-process effects (including a custom effect's `in.time`) use real time.

## Rendering Pipeline

Every frame runs through the engine's `RenderPipeline` (`src/core/render/`), built with `RenderPipeline::default_2d()`. One command encoder, one submit per frame. The camera passes run first, once per camera in render order, each on that camera's image; then the frame passes run once.

The world is drawn in linear HDR: every camera image, the scene images and render targets are `Rgba16Float` (`SCENE_FORMAT`), linear light premultiplied by alpha, with values that may exceed 1. The `tonemap` pass applies the frame's `Look` and writes the `Rgba8UnormSrgb` display image; display effects, the transition, the UI and the composite work on that. Every `Color` you give the engine is sRGB and is decoded once (`Color::to_linear`) where it reaches the GPU.

```
per camera (PassScope::Camera):
clear         the camera's image, per CameraClear (Background, Color, or transparent for None)
world         its lit batch draws: entities, Scene::render sprites, parallax, trails, lines, one z-sorted list
lighting      lights and shadows into its light buffer, composed with its image (lit in place for an image of its own)
unlit_world   its unlit batch draws (anything `with_lighting(false)`, debug shapes), by z, full brightness

per frame (PassScope::Frame):
cameras       window cameras' own images into their viewports, in order (None blends over); clears the scene image first when no camera drew straight into it
hdr_effects   bloom, god rays, ambient occlusion and HDR custom effects, on the scene image (linear, may exceed 1)
tonemap       the look: exposure, auto-exposure, curve, grading and LUT; scene image -> display image (Rgba8UnormSrgb)
transition    scene transition fade over the whole frame (not the UI)
post_process  display effects: fade, tint, vignette, flash, CRT, chromatic aberration and display custom effects, on the display image
ui            the UI renderer's draws (rects, sprites, lines, text) into the UI image (only if the scene drew UI)
composite     to the window/offscreen target through ScreenMapping (letterbox for pixel art); the UI image is blended last in sRGB (gamma) space, untouched by effects
capture       frame capture, when requested
```

Gotchas:
- The UI is only composited if the `ui` pass drew it this frame. A requested capture is only read back if the `capture` pass ran; otherwise it reports an error.
- Scene images (game, lit, HDR and display effect buffers) have the render size: the window's physical size, or the pixel-art target. A window camera's own image is its viewport's part of it.
- Per-camera GPU state (uniforms, bind groups, images, light targets) exists only for live cameras and goes with them.
- Passes are thin: GPU pipelines live in the subsystem renderers (the batch, lighting, post-process, ...), and the passes just call them.

### RenderPass Trait

```rust
pub trait RenderPass: Send + Sync {
    fn id(&self) -> &str;                                   // unique
    fn should_run(&self, _frame: &FrameInfo) -> bool { true } // only asked while enabled
    fn scope(&self) -> PassScope { PassScope::Frame }        // or PassScope::Camera: once per camera
    fn execute(&mut self, ctx: &mut RenderContext<'_>);
}
```

`RenderContext` exposes `device()`, `queue()`, `encoder()`, `frame()`, `render_size()`, `screen_size()`, `format()`, `current_view()` and `encoder_and_current_view()` (the encoder plus the view holding the latest image - in a camera pass, the camera's image - draw on it to add to the frame), and `camera()` / `camera_entity()` (the camera whose passes are running; `None` in a frame pass). All of these are in the prelude, with `pass_ids`, `PassScope`, `RenderPipeline`, `RenderPipelineError` and `FrameInfo`.

### Configuring the pipeline from a scene

`ctx.render_pipeline()` returns the engine's `RenderPipeline` resource (`Mut<RenderPipeline>`), so changes persist across scenes. Ids and anchors are checked: duplicates or unknown ids return `RenderPipelineError`.

```rust
let mut pipeline = ctx.render_pipeline();

// Toggle a built-in pass (fails only if the game removed it)
let _ = pipeline.set_enabled(pass_ids::POST_PROCESS, false);

// Passes persist across scenes, so add a game pass only if it is missing
// (scene re-entry would otherwise hit DuplicatePass)
if !pipeline.contains("flash") {
  let _ = pipeline.insert_before(pass_ids::COMPOSITE, Flash);
}

// Alternative: remove it in on_exit
ctx.render_pipeline().remove("flash");
```

To draw on the frame, insert the pass before `composite` (or after `post_process`). `composite` copies the frame to the window, so a pass placed after it (including `push` at the end) never reaches the screen. `ryo2d::wgpu` re-exports the wgpu types a pass needs.

Build a pass's pipelines for `RenderContext::format()`, the format of `current_view()`: a pass in a camera or before `tonemap` sees `Rgba16Float` linear light that may exceed 1 (write premultiplied, linear colour); a pass after `tonemap` (for instance after `post_process`) sees the `Rgba8UnormSrgb` display image, 0 to 1, where the hardware encodes sRGB for you. Place a pass relative to `tonemap` to choose which.

A game pass draws on the latest image:

```rust
fn execute(&mut self, ctx: &mut RenderContext<'_>) {
    let (encoder, view) = ctx.encoder_and_current_view();
    let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor { /* attach `view` */ });
}
```

For screen-wide effects written in WGSL, use a custom post-process effect (see Custom Shaders); a game pass is for raw wgpu work.

## Draw Order

Everything in the world draws on the GPU in one z-sorted list, which each camera filters by `RenderLayers` and its view: entities (`Renderable`, `CompositeTile`, shadows), world text (`Text2d`), `Scene::render` sprites (particles), parallax layers and `Trail2D`/`Line2D` all sort by `z_order`. Equal z draws entity, then world text, then sprite (`Scene::render`, parallax), then trail/line; within a kind, in submission order. Consecutive drawables with equal draw states merge into one draw call, so 10,000 sprites cost a handful of draws. The UI is a separate layer on top: its own camera, image and composite step (below).

Particles, parallax and trails default to z 0: give them an explicit z to sit behind or in front of entities; `Scene::render` sprites do not sit behind entities automatically. `z_order` picks the layer; `YSort` orders entities by y within their layer, after the layer's plain sprites; world text, sprites, particles and trails at the same z draw after the layer's entities. Debug shapes (`DebugDraw`, the overlays, `draw_skeleton`) are unlit on top at `i32::MAX`; the transition fade covers the world but not the UI. Nothing in the frame is drawn on the CPU (`Sprite::draw` is a CPU blit the dev windows use for their own pixel buffers).

## The 2D Batch

- **Streams.** Instanced quads (`SpriteInstance`, 128 bytes, remapped into its atlas slot) and indexed triangles (`Vertex2D`, 72 bytes). Each is written once per frame into a buffer that doubles when needed.
- **Keys.** Every item resolves to a `DrawState`: page (atlas page, standalone texture or render target; none for untextured lines), `BatchShader` (`Sprite`, `SpriteEffect`, `Custom(source)`, `Line`, `LineTextured`, `TextMask`, `TextColor`, `TextMsdf`), `BlendMode`, `Sampling`. `plan_draws` merges consecutive equal states per camera (triangles also need adjacent index ranges).
- **Blend modes.** Every batch shader outputs premultiplied colour, so each `BlendMode` is one fixed blend state (`pipelines::blend_state`): Alpha and Premultiplied `(One, OneMinusSrcAlpha)`; Additive `(One, One)`; Multiply `(Dst, OneMinusSrcAlpha)`; Screen `(OneMinusDst, One)`; the last three keep the destination's alpha. `Premultiplied` uses the shader entries that take a texture's colour as it is (`fs_*_premultiplied`).
- **Pipelines** are made on first use per `(BatchShader, BlendMode, Stream, format)`; a custom shader gets one per blend it is drawn with, checked without blocking.
- **Bind groups are made with their resource**, never per frame: one per camera, one per texture page and sampler. Make every bind group through `graphics::bind_groups::create` (a guard test fails on any other `create_bind_group`); `steady_frames_create_no_bind_groups` counts them. Frame-resource passes (compositor, post-process, lighting) keep a `BindGroupCache` keyed by the resources they bind.
- **Polylines.** `Triangles::polyline` builds trails, lines and debug shapes: a strip sharing its edge vertices, one wedge per corner on the outside (miter, bevel or round, fanned from the corner point), caps at open ends. `v` runs 0..1 across, `across` is the perpendicular for displacement effects. `line.wgsl` styles them (Solid with pixel-coverage edges, Soft, Glow) and displaces them (Lightning, Noise, Ribbon; UV scroll).

### Text and the UI Image

- **World glyphs are batch items.** Text glyphs are placed in a glyph page (`R8Unorm` coverage, `Rgba8Unorm` colour or MSDF) and drawn by `TextMask`, `TextColor` or `TextMsdf`; bitmap-font cells and inline icons are ordinary sprites. The instance's `tint`, `effect_params` and `effect_extra` lanes carry the glyph colour, the MSDF mode/width/softness/range and the outline or border colour (lanes in `src/core/render/AGENTS.md`).
- **The UI renderer.** The UI does not go through the batch: `render::ui_renderer` builds its commands into one instance format (rects, glyphs, sprites, line triangles) drawn by one pipeline (`ui.wgsl`) that clips in the shader, a draw only where a bound texture changes. The `ui` pass draws it through the UI camera (`UI_CAMERA`, its uniforms only) into the UI image: `Rgba8Unorm` (`UI_FORMAT`), colour gamma-encoded and premultiplied by alpha, through the `UI_GAMMA` override and `ui_out` in `color.wgsl`.
- **Gamma-space composite.** The composite shader (`fullscreen_quad.wgsl`) encodes the frame to sRGB, blends `ui + frame * (1 - ui.a)` and decodes, so UI alpha behaves as on the web (a 50% black panel over white is 128). The UI image is size `UiSpace::image`: the render size, or the window's screen rectangle with `UiResolution::Window` in pixel-art mode.
- **Batch shaders end in `ui_out(..)`** like the existing ones (custom sprite shaders get it from the generated entry points): a batch built in `UI_FORMAT` gamma-encodes as the UI pipeline does, which a test uses to prove UI lines against the line pipeline.

### Texture Atlas and Fidelity

Every texture the renderer draws has one `TextureId` in the `TextureRegistry` (sprites by handle, named pixel textures such as the shadow ellipse, and render targets). Sprite images are packed into shared atlas pages (2048x2048, four mip levels); each slot starts on an 8-pixel grid with an 8-pixel extruded border, so no level bleeds a neighbour. Images over 256 px get their own texture with a full mip chain. Mip levels are averaged on the CPU in linear light (weighted by alpha) when a texture is uploaded. Two samplers (nearest at level 0; trilinear) with a bind group per page each; fragment entries take the UV derivatives first and sample by gradient. UVs outside a sprite's own rect are transparent; only textured trails and lines repeat their texture, inside its slot (`sample_wrapped`). Unused entries are evicted and re-uploaded automatically. Render targets are pages of their own (`SlotPage::Target`): `Rgba16Float` textures that cameras draw into and sprites sample as linear premultiplied colour (the `*_linear` shader entries, `PageKind::Linear`); they are never evicted, and keep light above 1. Their contents are premultiplied, so `Renderable::render_target` blends `Premultiplied`. Atlas pages stay `Rgba8Unorm` raw sRGB bytes. Tint and effects run in sRGB space, quantised like the old CPU path, so opaque tinted pixels look as before; translucent overlaps blend in linear light, on the float image. Outline, glow and directional outline draw outside the sprite. Blob and contact shadows are a tinted ellipse; drop shadows are the sprite's silhouette. `DebugStats::draw_calls` counts the world draws of every camera.

### Resolution and the window

`ScreenMapping` (resource) is the one mapping between the window and screen space: `screen_size()` (logical pixels, or the pixel-art target), `render_size()` (scene images), `window_rect()` (where the scene lands, bars included), `window_to_screen` / `screen_to_window`, `ndc_rect()` (the compositor's rectangle). The compositor, mouse mapping and tests all use it. The UI buffer is screen-space sized and composited with nearest sampling.

### Shaders, errors and the web

Custom shader compilation never blocks the frame: errors are collected asynchronously and a failed sprite shader draws plain (with a warning). GPU setup failures are `GpuError`s (`NoAdapter`, `RequestDevice`, `CreateSurface`, `DeviceLost`). The engine builds for `wasm32` on WebGPU: `./dev.sh web <demo>` serves a demo with trunk (only demos with an `index.html` and `Trunk.toml`, such as `lighting`).

## Parallax

Parallax layers are components, one entity per layer, laid out for every camera on their layers from its own view:

```rust
ctx.world.spawn(ParallaxLayer::new(&sky).with_z_order(-100));
ctx.world.spawn(
  ParallaxLayer::new(&hills)
    .with_scroll(Vec2::splat(0.3))
    .with_tile(TileMode::InfiniteScroll)
    .with_auto_scroll(Vec2::new(20.0, 0.0))
    .with_z_order(-80),
);
```

Give parallax layers a negative z (e.g. -100..-20) so they sit behind entities at z 0 and above.

## GPU Batching

Games use `Scene::render`, called once per frame for every visible scene, bottom-up:

```rust
fn render(&mut self, gpu: &mut GpuRenderContext) {
  // A texture uploads the first time a frame draws it, and is dropped 600 frames after it was last drawn
  for particle in &self.particles {
    // Centre and size in world units
    let instance = SpriteInstance::new()
      .with_transform(particle.x, particle.y, particle.rotation, particle.size, particle.size);
    let draw = SpriteDraw { z: particle.z_order, blend: BlendMode::Additive, ..Default::default() };
    gpu.add_sprite(&self.particle_sprite, instance, draw);
  }
}
```

`gpu` also carries `world` (read-only) and `camera()` (the main camera's `CameraView`; to fix something on screen, place it with `camera().screen_to_world`). `SpriteDraw` carries z, blend, filter, lighting, whether the instance has an effect, and an emission that multiplies the instance's. Positions are world space, on render layer 0; nothing here is interpolated, so move particles in `update` and draw where they are.

## WGSL Shaders

Located in `src/core/shaders/`:

| File | Purpose |
|------|---------|
| `include/*.wgsl` | Shared, composed by `compose.rs` at compile time: `camera.wgsl`, `color.wgsl`, `sample.wgsl`, `fullscreen.wgsl`, `snap.wgsl` (pixel-art snapping), `quad.wgsl` (`quad_corner`: where a quad's corner lands, snap included; the sprite and UI vertex stages both call it), and the fragment bodies the world and UI shaders share: `tint.wgsl` (sprite tinting), `glyph.wgsl` (raster, colour and MSDF glyphs), `ui_rect.wgsl` (the UI rect) and `line_style.wgsl` (line shading) |
| `core/sprite.wgsl` | Sprites (batch quads), including per-sprite effects and premultiplied entries |
| `core/line.wgsl` | Trails, lines and debug shapes (batch triangles) |
| `core/ui.wgsl` | The UI renderer's one pipeline: `vs_ui` (quads through `quad_corner`, as `sprite.wgsl` places them, line triangles by their corners) and `fs_ui` (a switch over the instance's kind running the shared bodies, then the clip test as a scissor) |
| `core/light.wgsl`, `shadow.wgsl` | Lights (batch quads) and shadow volumes (batch triangles) |
| `core/tonemap.wgsl`, `exposure.wgsl` | The look, and auto-exposure's compute passes |
| `core/text.wgsl` | World glyphs: `fs_text_mask` (raster coverage times tint), `fs_text_color` (colour glyphs), `fs_text_msdf` (distance field: fill, outline, soft shadow and glow lanes) |
| `core/camera_image.wgsl` | A window camera's own image placed in its viewport |
| `core/fullscreen_quad.wgsl` | The final composite (and sRGB encoding when the surface is not sRGB) |
| `post-process/*.wgsl` | Screen-wide effects (one file per effect) |

Basic structure:

```wgsl
@vertex
fn vs_main(vertex: VertexInput, instance: InstanceInput) -> VertexOutput {
    // The instance's world-space quad, through the camera's globals.view_proj (group 0)
}

@fragment
fn fs_base(in: VertexOutput) -> @location(0) vec4<f32> {
    take_derivatives(in.uv);  // first, while control flow is uniform
    return finish(sample_sprite(in.uv, in.uv_bounds), in.tint, in.emission);  // tinted, times emission, linear, premultiplied
}
```

## The Look and Emission

The `tonemap` pass (`render/passes/tonemap.rs`, `core/tonemap.wgsl`) turns the linear HDR image into the display image, in this order: exposure (`Look::exposure` plus the auto-exposure EV), white balance (temperature, tint), contrast and saturation, the curve (`Tonemap`: `Soft` by default, `Clamp`, `Aces`, `AgX`), lift/gamma/gain on the display colour, then sRGB encoding and the LUT. `Tonemap::apply` in `look.rs` mirrors the shader on the CPU: change them together (`every_curve_on_the_gpu_matches_tonemap_apply` pins them). The WGSL `Look` struct names its field `auto_exposure` (`auto` is reserved). A `ColorLut` is a 3D texture that follows its sprite's hot reload. Auto-exposure (`exposure.rs`) runs two compute dispatches (a luminance histogram of the scene, then a reduction into a stored EV the tonemap binds); it adapts in real time. Games reach the look with `ctx.look()`.

Emission multiplies a drawable's light: `Renderable::with_emission(4.0)` (also on `ParticleEmitter`, `Trail2D`, `Line2D`, `SpriteInstance` and `SpriteDraw`) makes it glow past white, and bloom picks up what exceeds its luminance threshold (default 1.0). A lit sprite is still darkened by lighting; add `with_lighting(false)` to glow in the dark. The sprite shader keeps it in the instance's `transform0.w` as `emission - 1`.

## Effect Stages

Each post-process effect runs in one `EffectStage`: `Hdr` (bloom, god rays, ambient occlusion: linear light before the look, in `hdr_effects`) or `Display` (everything else: after the look, in `post_process`). Two pipelines, one per format, each with its own ping-pong images. Custom effects run in `Display` by default (`add_custom`); `add_custom_in(name, shader, EffectStage::Hdr)` puts one in the HDR stage, where `scene(uv)` can exceed 1. Bloom and god-ray thresholds are linear luminance (1.0 is white), so a threshold under 1 blooms lit surfaces and above 1 only emission and bright lights.

## Adding Effects

Per-sprite effect:

1. Add to `SpriteEffect` enum in `ecs/sprite_effects.rs`
2. Update shader in `shaders/core/sprite.wgsl`
3. Pass uniforms from Rust

Post-process effect: see `src/core/shaders/AGENTS.md` (`effects.rs`, `post-process/<name>.wgsl`, `pipeline.rs`, `manager.rs`).

## Custom Shaders

Games write WGSL; the engine wraps it with a prelude and a generated entry point (`src/core/shaders/custom.rs`). Load with `assets.load_shader("shaders/x.wgsl")` (cached by path; only IO errors are `Err`, so `match` it). `ShaderHandle::from_source(id, wgsl)` makes an inline shader and `handle.replace_source(..)` updates it. Handles with the same source share one compiled source (and one error report), so only the first compiles; the batch makes a pipeline per blend mode it is drawn with. Cloning one handle is still cheapest.

### Sprite shader

```wgsl
struct Params {          // optional; members f32/vec2/vec3/vec4<f32>, at most 32 bytes
  edge: vec3<f32>,
  amount: f32,
}

fn sprite(in: SpriteIn, params: Params) -> vec4<f32> {
  // in.uv: 0..1 across the sprite, in.size: sprite size in texels,
  // in.screen_pos: pixel position in the camera's image, in.time: game seconds (stops while paused)
  let c = sample(in.uv);  // sprite colour; transparent outside the sprite rect
  return vec4<f32>(mix(c.rgb, params.edge, params.amount), c.a);
}
```

WGSL alignment applies: a `vec3<f32>` member takes 16 bytes of alignment, so `{ a: vec3<f32>, b: vec3<f32> }` is 32 bytes. Put `f32`s after a `vec3` to pack tightly (as `edge`/`amount` above do).

Without a `Params` struct the engine supplies `struct Params { unused: f32 }`. The function works in the sprite's sRGB values (straight alpha); the engine applies the tint, rounding, linear conversion and premultiplication afterwards. Custom shaders draw within the sprite's quad. Shadows and tiled tiles draw plain.

```rust
if let Ok(handle) = ctx.assets.load_shader("shaders/dissolve.wgsl") {
    // Params are set by name; a Color is sRGB/255 (rgb into a vec3, rgba into a vec4).
    let fx = SpriteShader::custom(handle).with_param("amount", 0.4).with_param("edge", Color::rgb(255, 120, 0));
    Renderable::sprite(sprite).with_shader(fx);   // or a Material component (wins over Renderable::shader)
}
// Scene::render: gpu.add_custom_sprite(&sprite, &handle, &params, instance, SpriteDraw::default());
```

`ParamValue` is `F32`/`Vec2`/`Vec3`/`Vec4`/`Color`, with `From` for `f32`, `glam::Vec2/3/4` and `Color`. A `Color` fills a `vec3<f32>` field with rgb or a `vec4<f32>` field with rgba. A name the shader does not declare, or a value of the wrong type, is warned about and ignored; a declared param never set is zero.

### Post-process effect

```wgsl
struct Params {          // optional; at most 64 bytes
  strength: f32,
}

fn effect(in: EffectIn, params: Params) -> vec4<f32> {
  // in.uv, in.time (real seconds), in.resolution
  let c = scene(in.uv);  // latest image, LINEAR colour; return linear colour too
  // A Color param is sRGB bytes/255: decode it before mixing it with scene(uv)
  // let tint = srgb_to_linear(params.tint.rgb);
  return vec4<f32>(c.rgb * (1.0 - params.strength), c.a);
}
```

```rust
if let Ok(h) = ctx.assets.load_shader("shaders/scan.wgsl") {
  let mut fx = ctx.effects();
  fx.add_custom("scan", h)?;                           // Err(EffectError::DuplicateName)
  let _ = fx.set_custom_param("scan", "strength", 0.3); // setters return false (#[must_use]) for an unknown name
  let _ = fx.set_custom_enabled("scan", false);
  fx.remove_custom("scan");                            // has_custom("scan")
}
```

Custom effects run after the built-in effects of their stage, in insertion order (`add_custom_in` picks the stage; see Effect Stages). `effects.clear()` (part of default scene cleanup) removes them too.

### Hot reload and errors

In debug builds the engine watches the game's `assets/` folder and reloads changed files every frame, with nothing to call: editing a `.wgsl` file loaded with `assets.load_shader` updates the handle and the next frame rebuilds. (`assets.enable_hot_reload(dirs)` watches other folders.) A bad shader is logged once per version in the dev log as `<id>:<line>:<column>: <message>` with the line in your own file; a sprite then draws plain and an effect is skipped. Nothing panics.

## Post-Processing

Built-in effects on the `EffectsManager` resource, reached with `ctx.effects()` (`Mut<EffectsManager>`; bind it for several calls). Timed effects (flash) run on real time.

```rust
let mut fx = ctx.effects();
fx.set_fade(0.5);
fx.set_vignette(0.3);
fx.enable_crt();
fx.set_chromatic_aberration(0.005);
fx.set_god_rays(0.8, vec![(0.5, 0.3)]);
fx.set_ambient_occlusion(0.3, 16.0);
fx.set_bloom(1.0);  // HDR stage; multi-pass: extract -> blur -> composite
fx.flash(Color::RED, Duration::from_millis(100));
fx.set_sepia(0.5);
```

## Synchronization

Write every buffer (`queue.write_buffer`) before beginning the render pass that draws from it; the batch uploads the frame's quads and triangles in `Batch2D::prepare` (during extraction), then the `world` and `unlit_world` passes record the draws.

## Tips

- Trails, lines and particles sort with entities by z: give them one that matches what they should sit behind or in front of
- Keep shaders simple for mobile compatibility
- Instance buffers are more efficient than individual draws

