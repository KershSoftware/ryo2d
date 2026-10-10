# AGENTS.md - Particle System

## Integration with Engine

The engine never runs particles. The scene owns a `ParticleSystem` and drives it itself (see `examples/demos/particles/main.rs`):

1. In `update`: `self.particle_system.update(ctx.world, ctx.events, dt)`. This spawns, simulates, applies attractors, and refreshes the emitter particle counts that `is_finished()` reads.
2. In `render_gpu(ctx: &mut GpuRenderContext)`: first `ctx.upload_sprite(&sprite)` for each particle sprite, then `self.particle_system.collect_for_render(ctx.world, ctx.camera, ctx.batch)`.

Particles go into the GPU sprite batch, so they draw with GPU sprites, before trails and UI.

## Gotchas

- **One-shot cleanup** - nothing despawns finished emitters. Check `is_finished()` and despawn.
- **Layer masks need emitter layers** - an attractor's `with_layer_mask(mask)` only affects particles whose emitter set `with_layer(bits)` with a shared bit. Emitters default to layer 0, which only unmasked attractors (mask 0) affect.
- **Collisions are not implemented** - `with_collision()` is stored, but `ParticleSystem::update` does no collision detection and never emits `ParticleCollision` (placeholder in `mod.rs`).
