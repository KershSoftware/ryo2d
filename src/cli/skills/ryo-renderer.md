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

Frame rendering is hard-coded in `engine.rs` / `graphics/renderer.rs` (`WgpuRenderer::render_with_gpu_sprites` and the lighting variant). Order, back to front:

```
1. Clear game texture
2. render_gpu() sprites (incl. parallax) - behind CPU content
3. CPU pixel buffer (Renderable sprites) alpha-blended on top
4. Trails and lines (so lighting applies to them)
5. Lighting (render_lights + compose) - if enabled
6. Trails and lines with `with_lighting(false)`, at full brightness on the lit scene
7. UI composited on top of the lit scene
8. Post-processing effects (bloom, CRT, vignette, etc.)
9. Present to screen with letterboxing
```

### RenderPass / RenderPipeline (not wired in)

`RenderPipeline` (`src/core/render/`, import from `ryo2d::core::render` - it isn't in the prelude) is a pass-based, topologically sorted pipeline, but **the engine never builds or executes it**. Adding or disabling passes does not change what the game renders. Only `ClearPass`, `ParallaxPass`, `UploadPass` and `LightingPass` exist; the other `pass_ids` are constants only.

### RenderPass Trait

```rust
pub trait RenderPass: Send + Sync {
    fn id(&self) -> RenderPassId;  // &'static str
    fn dependencies(&self) -> &[RenderPassId] { &[] }
    fn execute(&mut self, ctx: &mut RenderContext);
    fn should_execute(&self, _ctx: &RenderContext) -> bool { true }
    fn is_enabled(&self) -> bool { true }
    fn set_enabled(&mut self, _enabled: bool) {}
}
```

### RenderContext

Shared state passed to all passes:

```rust
pub struct RenderContext<'a> {
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    pub encoder: &'a mut wgpu::CommandEncoder,
    pub game_texture: &'a wgpu::Texture,
    pub game_texture_view: &'a wgpu::TextureView,
    pub scratch_a: Option<&'a wgpu::Texture>,  // Ping-pong buffers
    pub scratch_b: Option<&'a wgpu::Texture>,
    pub pixel_buffer: &'a mut [u8],            // CPU buffer
    pub camera: &'a Camera,
    pub sprite_gpu: &'a mut SpriteGpuRenderer,
    pub batch_collector: &'a mut BatchCollector,
    pub current_source: TextureSlot,           // Which texture has result
    // ... more fields
}
```

### Pipeline Configuration

```rust
// Disable a pass (returns false if no such pass)
pipeline.set_enabled("lighting", false);

// Add custom pass (inserted by dependency order)
pipeline.add_pass(MyCustomPass::new());

// Remove a pass
pipeline.remove_pass("trail");
```

## CPU vs GPU Path

**CPU (default)**: Software blitting, good for <1000 sprites

**GPU (opt-in)**: Batched instanced rendering, for 1000+ sprites (particles, bullets). GPU sprites always draw behind CPU-rendered entities; `z_order` only orders GPU batches among themselves.

## Background Rendering

Games render parallax and background layers via the GPU path:

```rust
fn render_gpu(&mut self, gpu: &mut GpuRenderContext) {
    self.background.render(gpu);
}
```

Parallax layers render behind all game entities via the GPU pipeline.

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

- CPU rendering is simpler and sufficient for most 2D games
- Use GPU path when CPU blitting becomes a bottleneck
- Keep shaders simple for mobile compatibility
- Instance buffers are more efficient than individual draws

