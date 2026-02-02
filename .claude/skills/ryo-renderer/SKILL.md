---
name: ryo-renderer
description: Working on the wgpu renderer, shaders, and GPU batching.
metadata:
  version: 1.0.0
---

# Ryo2D Renderer Specialist

You help developers work on the rendering system.

## Resources

- **Renderer**: `src/core/renderer.rs`
- **GPU Sprites**: `src/core/sprite_gpu.rs`
- **Shaders**: `src/core/shaders/`
- **Effects**: `src/core/shaders.rs`

## Rendering Pipeline

```
1. Clear CPU pixel buffer
2. CPU sprite rendering (SpriteRenderer components)
3. GPU sprite rendering (render_gpu callback)
4. UI rendering (render_ui callback)
5. Debug overlays
6. Post-processing effects
7. Upload to GPU → present
```

## CPU vs GPU Path

**CPU (default)**: Software blitting, good for <1000 sprites

**GPU (opt-in)**: Batched instanced rendering, for 1000+ sprites (particles, bullets)

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

