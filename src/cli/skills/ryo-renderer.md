---
name: ryo-renderer
description: Working on the wgpu renderer, shaders, and GPU batching.
metadata:
  version: 2.0.0
---

# Ryo2D Renderer Specialist

You help developers work on the rendering system.

## Resources

- **Extraction**: `src/core/engine/extract.rs` (`present_frame`), `src/core/render/entities.rs`, `draw_list.rs`
- **Render Pipeline**: `src/core/render/` (pass-based architecture)
- **Pipeline**: `src/core/render/pipeline.rs`
- **Pass Trait**: `src/core/render/pass.rs`
- **Built-in Passes**: `src/core/render/passes/`
- **GPU Sprites**: `src/core/graphics/sprite_gpu.rs`, the atlas (`atlas.rs`) and the texture registry (`texture_registry.rs`)
- **Cameras**: `src/core/graphics/{camera2d,camera_mut,camera_gpu,camera_images,render_targets,render_layers}.rs`
- **Screen and window**: `src/core/graphics/screen.rs` (`ScreenMapping`), `compositor.rs`
- **Shaders**: `src/core/shaders/`
- **Lighting**: `src/core/lighting/`
- **Trails**: `src/core/trail/`

## Extraction

The world is read fresh every frame; there is no render cache to invalidate. Everything is gathered once, in world space, and sorted once; each camera then keeps what it sees. `present_frame` (after `PostUpdate`) does:

1. Scenes' `Scene::render(gpu)` hooks fill the `BatchCollector` (particles, parallax, many-sprite effects), in world space.
2. Trails (world-space points), lines (at their entity) and debug overlays go into the trail renderer, grouped by lighting mode, z and `RenderLayers`; lights are gathered at their positions.
3. The render targets' GPU textures follow `RenderTargets`; the cameras are planned (`plan_cameras`): texture cameras, then window cameras, each by `order`. The first window camera draws straight into the scene image when it covers the window and clears; every other window camera gets an image of its own, sized to its viewport (`CameraImages`); a texture camera draws straight into its target, or, when its viewport covers only part of it, into an image of its own placed in that part after its passes.
4. Each camera gets a uniform buffer (`CameraUniforms`, one `CameraGlobals` each: `view_proj`, `resolution` = its image size, `game_time`, `real_time`, `snap`), bound as group 0 by sprites, trails, lights and shadows alike.
5. One query over the renderables (plus `CompositeTile`) builds the frame's `DrawList`, skipping quads no camera on the entity's layers can see. `Disabled` entities are skipped by the query itself.
6. `SpriteGpuRenderer::prepare_world` lays each camera's sprites out in a range of the instance buffer of its own (on its layers, inside its `CameraView::bounds`, not showing its own target) and uploads them once.
7. The render pipeline runs.

Placement rules:

- Entities with `PreviousGlobalTransform` (the `Interpolated` marker on them or an ancestor) draw at `render_global(global, previous, time.fixed.overstep_fraction())`; everything else draws at `GlobalTransform`. Composite tiles, lights, occluders and lines on an interpolated entity are blended the same way, and so is a camera's own transform when its `CameraView` is computed.
- A single sprite is placed by `Renderable::anchor` (default `Anchor::Center`); rotation and scale happen about the anchor. Tiled runs and composite parts are laid out from the entity's position (the first tile's top-left).
- `YSort` sorts by the sprite's feet (`foot_offset` of the way down from its top edge, wherever the anchor is).
- Instances carry world-space quads (centre, size, rotation); the camera's `view_proj` takes them to its image. In pixel-art mode the sprite vertex shader snaps axis-aligned quads to whole target pixels (`snap`); sampling is nearest there and linear otherwise.
- Sprite effects and a custom sprite shader's `in.time` run on game time; lighting, trails, transitions and post-process effects (including a custom effect's `in.time`) use real time.

## Rendering Pipeline

Every frame runs through the engine's `RenderPipeline` (`src/core/render/`), built with `RenderPipeline::default_2d()`. One command encoder, one submit per frame. The camera passes run first, once per camera in render order, each on that camera's image; then the frame passes run once:

```
per camera (PassScope::Camera):
clear         the camera's image, per CameraClear (Background, Color, or transparent for None)
world         its entities, Scene::render sprites (particles, parallax) and lit trails/lines, one z-sorted list
lighting      lights and shadows into its light buffer, composed with its image (lit in place for an image of its own)
unlit_world   its trails and lines with `with_lighting(false)`, by z, full brightness

per frame (PassScope::Frame):
cameras       window cameras' own images into their viewports, in order (None blends over); clears the scene image first when no camera drew straight into it
transition    scene transition fade over the whole frame (not the UI)
post_process  bloom, CRT, vignette, etc.
ui            upload the UI layer (only if the scene drew UI)
composite     to the window/offscreen target through ScreenMapping (letterbox for pixel art); the UI is blended last, untouched by effects
capture       frame capture, when requested
```

Gotchas:
- The UI is only composited if the `ui` pass uploaded it this frame. A requested capture is only read back if the `capture` pass ran; otherwise it reports an error.
- Scene images (game, lit, post-process) have the render size: the window's physical size, or the pixel-art target. A window camera's own image is its viewport's part of it.
- Per-camera GPU state (uniforms, bind groups, images, light targets) exists only for live cameras and goes with them.
- Passes are thin: GPU pipelines live in the subsystem renderers (lighting, trails, post-process, ...), and the passes just call them.

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

To draw on the frame, insert the pass before `composite` (or after `post_process`). `composite` copies the frame to the window, so a pass placed after it (including `push` at the end) never reaches the screen. `RenderContext::format()` gives the image's texture format, and `ryo2d::wgpu` re-exports the wgpu types a pass needs.

A game pass draws on the latest image:

```rust
fn execute(&mut self, ctx: &mut RenderContext<'_>) {
    let (encoder, view) = ctx.encoder_and_current_view();
    let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor { /* attach `view` */ });
}
```

For screen-wide effects written in WGSL, use a custom post-process effect (see Custom Shaders); a game pass is for raw wgpu work.

## Draw Order

Everything in the world draws on the GPU in one z-sorted list, which each camera filters by `RenderLayers` and its view: entities (`Renderable`, `CompositeTile`, shadows), `Scene::render` sprites (particles, parallax) and `Trail2D`/`Line2D` all sort by `z_order`. Equal z draws entity, then `Scene::render` sprite, then trail/line; within a kind, in submission order. Consecutive sprites on one atlas page merge into one draw call, so 10,000 sprites cost a handful of draws. The UI is a separate layer on top.

Particles, parallax and trails default to z 0: give them an explicit z to sit behind or in front of entities; `Scene::render` sprites do not sit behind entities automatically. `Trail2D` and `Line2D` have `with_z_order` (default 0). `z_order` picks the layer; `YSort` orders entities by y within their layer, after the layer's plain sprites; sprites, particles and trails at the same z draw after the layer's entities. `skeleton_debug_lines` draw on top. Debug overlays are unlit lines on top; the transition fade covers the world (including trails) but not the UI. Only the UI uses CPU drawing now (`Sprite::draw` / `draw_scaled`).

### Texture Atlas and Fidelity

Every texture the renderer draws has one `TextureId` in the `TextureRegistry` (sprites by handle, named pixel textures such as the shadow ellipse, and render targets). Sprite images are packed into shared atlas pages (2048x2048, 1-px extruded border); images over 256 px get their own texture. UVs outside a sprite's own rect are transparent, so there is no UV wrapping. Unused entries are evicted and re-uploaded automatically. Render targets are pages of their own (`SlotPage::Target`): cameras draw into an sRGB view, sprites sample an `Rgba8Unorm` view like the atlas pages; they are never evicted. Tint and effects run in sRGB space, quantised like the old CPU path, so opaque tinted pixels look as before; translucent overlaps blend in linear space and may look slightly different. Outline, glow and directional outline draw outside the sprite. Blob and contact shadows are a tinted ellipse; drop shadows are the sprite's silhouette. `DebugStats::draw_calls` counts the world draws of every camera.

### Resolution and the window

`ScreenMapping` (resource) is the one mapping between the window and screen space: `screen_size()` (logical pixels, or the pixel-art target), `render_size()` (scene images), `window_rect()` (where the scene lands, bars included), `window_to_screen` / `screen_to_window`, `ndc_rect()` (the compositor's rectangle). The compositor, mouse mapping and tests all use it. The UI buffer is screen-space sized and composited with nearest sampling.

### Shaders, errors and the web

Custom shader compilation never blocks the frame: errors are collected asynchronously and a failed sprite shader draws plain (with a warning). GPU setup failures are `GpuError`s (`NoAdapter`, `RequestDevice`, `CreateSurface`, `DeviceLost`). The engine builds for `wasm32` on WebGPU: `./dev.sh web <demo>` serves a demo with trunk (only demos with an `index.html` and `Trunk.toml`, such as `lighting`).

## Background Rendering

Games render parallax and background layers via the GPU path:

```rust
fn render(&mut self, gpu: &mut GpuRenderContext) {
  self.background.render(gpu);
}
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
    gpu.add_sprite_instance(&self.particle_sprite, instance, particle.z_order, false);
  }
}
```

`gpu` also carries `world` (read-only) and `camera()` (the main camera's `CameraView`; to fix something on screen, place it with `camera().screen_to_world`). Sprites are batched by texture and effect type. Positions are world space, on render layer 0; nothing here is interpolated, so move particles in `update` and draw where they are.

## WGSL Shaders

Located in `src/core/shaders/`:

| File | Purpose |
|------|---------|
| `core/sprite.wgsl` | Sprite rendering, including per-sprite effects |
| `core/light.wgsl`, `shadow.wgsl`, `raymarch_shadow.wgsl` | Lighting and shadows |
| `core/trail.wgsl`, `trail_textured.wgsl` | Trails |
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
    return textureSample(sprite_texture, sprite_sampler, in.uv) * in.tint;
}
```

## Adding Effects

Per-sprite effect:

1. Add to `SpriteEffect` enum in `ecs/sprite_effects.rs`
2. Update shader in `shaders/core/sprite.wgsl`
3. Pass uniforms from Rust

Post-process effect: see `src/core/shaders/AGENTS.md` (`effects.rs`, `post-process/<name>.wgsl`, `pipeline.rs`, `manager.rs`).

## Custom Shaders

Games write WGSL; the engine wraps it with a prelude and a generated entry point (`src/core/shaders/custom.rs`). Load with `assets.load_shader("shaders/x.wgsl")` (cached by path; only IO errors are `Err`, so `match` it). `ShaderHandle::from_source(id, wgsl)` makes an inline shader and `handle.replace_source(..)` updates it. Handles with the same source share one compiled pipeline (and one error report), so only the first compiles; cloning one handle is still cheapest.

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

Without a `Params` struct the engine supplies `struct Params { unused: f32 }`. The function works in the sprite's sRGB values (straight alpha); the engine applies the tint, rounding and linear conversion afterwards. Custom shaders draw within the sprite's quad. Shadows and tiled tiles draw plain.

```rust
if let Ok(handle) = ctx.assets.load_shader("shaders/dissolve.wgsl") {
    // Params are set by name; a Color is sRGB/255 (rgb into a vec3, rgba into a vec4).
    let fx = SpriteShader::custom(handle).with_param("amount", 0.4).with_param("edge", Color::rgb(255, 120, 0));
    Renderable::sprite(sprite).with_shader(fx);   // or a Material component (wins over Renderable::shader)
}
// Scene::render: gpu.add_custom_sprite(&sprite, &handle, &params, instance, z_order);
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

Custom effects run after the built-in effects, in insertion order. `effects.clear()` (part of default scene cleanup) removes them too.

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
fx.set_bloom(1.0);  // multi-pass: extract -> blur -> composite
fx.flash(Color::RED, Duration::from_millis(100));
fx.set_sepia(0.5);
```

## Synchronization

Write every instance buffer (`queue.write_buffer`) before beginning the render pass that draws from it; the sprite renderer uploads the whole draw list in `prepare_world` (during extraction), then the `world` pass records the draws.

## Tips

- Trails, lines and particles sort with entities by z: give them one that matches what they should sit behind or in front of
- Keep shaders simple for mobile compatibility
- Instance buffers are more efficient than individual draws

