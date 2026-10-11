---
name: ryo-renderer
description: Working on the wgpu renderer, shaders, and GPU batching.
metadata:
  version: 1.0.0
---

# Ryo2D Renderer Specialist

You help developers work on the rendering system.

## Resources

- **Render Pipeline**: `src/core/render/` (pass-based architecture)
- **Pipeline**: `src/core/render/pipeline.rs`
- **Pass Trait**: `src/core/render/pass.rs`
- **Built-in Passes**: `src/core/render/passes/`
- **GPU Sprites**: `src/core/graphics/sprite_gpu.rs`
- **Shaders**: `src/core/shaders/`
- **Lighting**: `src/core/lighting/`
- **Trails**: `src/core/trail/`

## Rendering Pipeline

Every frame runs through the engine's `RenderPipeline` (`src/core/render/`), built with `RenderPipeline::default_2d()`. One command encoder, one submit per frame. Passes run in list order:

```
clear         clear the game texture
raycast       GPU raycaster (replaces the 2D world passes while raycasting; clears to black)
world         entities, render_gpu() sprites (particles, parallax) and trails/lines, one z-sorted list (lit by lighting)
lighting      render_lights + compose, if enabled (the lit texture is persistent)
unlit_world   trails and lines with `with_lighting(false)`, by z, full brightness on the lit scene
transition    scene transition fade over the whole frame (not the UI)
post_process  bloom, CRT, vignette, etc.
ui            upload the UI layer (only if the scene drew UI)
composite     letterboxed present to the window/offscreen target; the UI is blended last, untouched by effects
capture       frame capture, when requested
```

Gotchas:
- The UI is only composited if the `ui` pass uploaded it this frame. A requested capture is only read back if the `capture` pass ran; otherwise it reports an error.
- Every game-size texture follows `resize_buffer` (ResizableStretch).
- Passes are thin: GPU pipelines live in the subsystem renderers (lighting, trails, post-process, ...), and the passes just call them.

### RenderPass Trait

```rust
pub trait RenderPass: Send + Sync {
    fn id(&self) -> &str;                                   // unique
    fn should_run(&self, _frame: &FrameInfo) -> bool { true } // only asked while enabled
    fn execute(&mut self, ctx: &mut RenderContext<'_>);
}
```

`RenderContext` exposes `device()`, `queue()`, `encoder()`, `frame()`, `game_size()`, `current_view()` and `encoder_and_current_view()` (the encoder plus the view holding the latest image - draw on it to add to the frame). All of these are in the prelude, with `pass_ids`, `RenderPipeline`, `RenderPipelineError` and `FrameInfo`.

### Configuring the pipeline from a scene

`ctx.render_pipeline` (on `SceneContext`) is the engine's pipeline, so changes persist across scenes. Ids and anchors are checked: duplicates or unknown ids return `RenderPipelineError`.

```rust
// Toggle a built-in pass (fails only if the game removed it)
let _ = ctx.render_pipeline.set_enabled(pass_ids::POST_PROCESS, false);

// Passes persist across scenes, so add a game pass only if it is missing
// (scene re-entry would otherwise hit DuplicatePass)
if !ctx.render_pipeline.contains("flash") {
    let _ = ctx.render_pipeline.insert_before(pass_ids::COMPOSITE, Flash);
}

// Alternative: remove it in on_exit
ctx.render_pipeline.remove("flash");
```

To draw on the frame, insert the pass before `composite` (or after `post_process`). `composite` copies the frame to the window, so a pass placed after it (including `push` at the end) never reaches the screen. `RenderContext::format()` gives the image's texture format, and `ryo2d::wgpu` re-exports the wgpu types a pass needs.

A game pass draws on the latest image:

```rust
fn execute(&mut self, ctx: &mut RenderContext<'_>) {
    let (encoder, view) = ctx.encoder_and_current_view();
    let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor { /* attach `view` */ });
}
```

Custom shaders and a higher-level effects API for passes are coming; for now a game pass records raw wgpu work.

## Draw Order

Everything in the world draws on the GPU in one z-sorted list: entities (`Renderable`, `CompositeTile`, shadows), `render_gpu()` sprites (particles, parallax) and `Trail2D`/`Line2D` all sort by `z_order`. Equal z draws entity, then `render_gpu` sprite, then trail/line; within a kind, in submission order. Consecutive sprites on one atlas page merge into one draw call, so 10,000 sprites cost a handful of draws. The UI is a separate layer on top.

Particles, parallax and trails default to z 0: give them an explicit z to sit behind or in front of entities. Only the UI uses CPU drawing now (`Sprite::draw` / `draw_scaled`).

## Background Rendering

Games render parallax and background layers via the GPU path:

```rust
fn render_gpu(&mut self, gpu: &mut GpuRenderContext) {
    self.background.render(gpu);
}
```

Give parallax layers a negative z (e.g. -100..-20) so they sit behind entities at z 0 and above.

## GPU Batching

Games use `render_gpu()`:

```rust
fn render_gpu(&mut self, gpu: &mut GpuRenderContext) {
    gpu.upload_sprite(&self.particle_sprite);

    for particle in &self.particles {
        let instance = SpriteInstance::new()
            .with_transform(particle.x, particle.y, particle.rotation, 1.0, 1.0);
        gpu.batch.add(&self.particle_sprite.id(), false, instance, particle.z_order);
    }
}
```

Sprites are batched by texture and effect type.

## WGSL Shaders

Located in `src/core/shaders/`:

| File | Purpose |
|------|---------|
| `core/sprite.wgsl` | Sprite rendering, including per-sprite effects |
| `core/light.wgsl`, `shadow.wgsl`, `raymarch_shadow.wgsl` | Lighting and shadows |
| `core/trail.wgsl`, `trail_textured.wgsl` | Trails |
| `post-process/*.wgsl` | Screen-wide effects (one file per effect) |

Basic structure:

```wgsl
@vertex
fn vs_main(vertex: VertexInput, instance: InstanceInput) -> VertexOutput {
    // Transform vertex
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

## Post-Processing

9 built-in effects applied via `ctx.effects`:

```rust
ctx.effects.set_fade(0.5);
ctx.effects.set_vignette(0.3);
ctx.effects.enable_crt();
ctx.effects.set_chromatic_aberration(0.005);
ctx.effects.set_god_rays(0.8, vec![(0.5, 0.3)]);
ctx.effects.set_ambient_occlusion(0.3, 16.0);
ctx.effects.set_bloom(1.0);  // multi-pass: extract -> blur -> composite
ctx.effects.flash(Color::RED, Duration::from_millis(100));
ctx.effects.set_sepia(0.5);
```

## Synchronization

Critical: Upload instance data BEFORE starting render pass:

```rust
// Pre-upload all data
for (key, instances) in batch_collector.iter() {
    queue.write_buffer(&instance_buffer, offset, &instances);
}

// Then render
render_pass.draw_indexed(...);
```

## Tips

- Trails, lines and particles sort with entities by z: give them one that matches what they should sit behind or in front of
- Keep shaders simple for mobile compatibility
- Instance buffers are more efficient than individual draws

