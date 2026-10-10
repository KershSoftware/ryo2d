# AGENTS.md - Particle System

## Integration with Engine

The engine never runs particles. The scene owns a `ParticleSystem` and drives it itself (see `examples/demos/particles/main.rs`):

1. In `update`: `self.particle_system.update(ctx.world, ctx.events, dt)`. This spawns, simulates, applies attractors, and refreshes the emitter particle counts that `is_finished()` reads.
2. In `render_gpu(ctx: &mut GpuRenderContext)`: first `ctx.upload_sprite(&sprite)` for each particle sprite, then `self.particle_system.collect_for_render(ctx.world, ctx.camera, ctx.batch)`.

Particles go into the GPU sprite batch, so they draw with GPU sprites, before trails and UI.

## Gotchas

- **One-shot cleanup** - nothing despawns finished emitters. Check `is_finished()` and despawn.
- **Attractor layer masks don't work** - `Particle.collision_layer` is always 0 and never set, so any non-zero `with_layer_mask()` stops the attractor affecting every particle. Leave masks at the default 0.
- **Collisions are not implemented** - `with_collision()` is stored, but `ParticleSystem::update` does no collision detection and never emits `ParticleCollision` (placeholder in `mod.rs`).
