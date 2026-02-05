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

The engine uses a **pass-based render pipeline** where each phase is a `RenderPass`. Passes declare dependencies and execute in topological order.

```
Built-in Passes (in dependency order):
┌─────────────────────────────────────────────────────────────┐
│ clear      │ Clear pixel buffer to background color        │
│ parallax   │ GPU-rendered parallax background layers       │
│ cpu_sprite │ Software sprite rendering (Renderable)        │
│ upload     │ Upload CPU pixel buffer to GPU texture        │
│ gpu_sprite │ Batched GPU sprite rendering (render_gpu)     │
│ trail      │ Motion trail rendering                        │
│ lighting   │ 2D lighting with shadows                      │
│ ui         │ User interface overlay (render_ui)            │
│ effects    │ Post-processing (CRT, vignette, etc.)         │
│ present    │ Final output to swapchain                     │
└─────────────────────────────────────────────────────────────┘
```

### RenderPass Trait

```rust
pub trait RenderPass: Send + Sync {
    fn id(&self) -> &'static str;
    fn dependencies(&self) -> &[&'static str] { &[] }
    fn execute(&mut self, ctx: &mut RenderContext);
    fn should_execute(&self, ctx: &RenderContext) -> bool { true }
    fn is_enabled(&self) -> bool { true }
    fn set_enabled(&mut self, enabled: bool) {}
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
// Disable a pass at runtime
pipeline.set_enabled("lighting", false);

// Add custom pass (inserted by dependency order)
pipeline.add_pass(MyCustomPass::new());

// Remove a pass
pipeline.remove_pass("trail");
```

## CPU vs GPU Path

**CPU (default)**: Software blitting, good for <1000 sprites

**GPU (opt-in)**: Batched instanced rendering, for 1000+ sprites (particles, bullets)

## Background Rendering

Games use `render_background()` for parallax and background layers:

```rust
fn render_background(&mut self, frame: &mut [u8], width: u32, height: u32, camera: &Camera) {
    // Draw directly to frame buffer before entity rendering
    self.background.render(frame, width, height, camera);
}
```

Called after frame clear but before entity rendering - ensures backgrounds are behind all sprites.

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
| `sprite.wgsl` | Basic sprite rendering |
| `sprite_effects.wgsl` | Per-sprite effects |
| `post_process.wgsl` | Screen-wide effects |

Basic structure:

```wgsl
@vertex
fn vs_main(vert: VertexInput, inst: InstanceInput) -> VertexOutput {
    // Transform vertex
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(sprite_texture, sprite_sampler, in.tex_coord);
}
```

## Adding Effects

1. Add to `SpriteEffect` enum in `ecs/components.rs`
2. Update shader in `shaders/sprite_effects.wgsl`
3. Pass uniforms from Rust

## Post-Processing

```rust
ctx.effects.set_effect(PostProcessEffect::ScreenShake { intensity: 0.5 });
ctx.effects.fade_to_black(1.0);
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

