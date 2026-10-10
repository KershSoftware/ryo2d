# AGENTS.md - Particle System

## Integration with Engine

The engine never runs particles. The scene owns a `ParticleSystem` and drives it itself (see `examples/demos/particles/main.rs`):

1. In `update`: `self.particle_system.update(ctx.world, ctx.events, dt)`. This spawns, simulates, applies attractors, and refreshes the emitter particle counts that `is_finished()` reads.
2. In `render_gpu(ctx: &mut GpuRenderContext)`: first `ctx.upload_sprite(&sprite)` for each particle sprite, then `self.particle_system.collect_for_render(ctx.world, ctx.camera, ctx.batch)`.

Particles go into the GPU sprite batch, so they draw with GPU sprites, before trails and UI.

## Gotchas

- **One-shot cleanup** - nothing despawns finished emitters. Check `is_finished()` and despawn.
- **Layer masks need emitter layers** - an attractor's `with_layer_mask(mask)` only affects particles whose emitter set `with_layer(bits)` with a shared bit. Emitters default to layer 0, which only unmasked attractors (mask 0) affect.
- **Disabled or `Inactive` emitters** - their live particles lose gravity and collisions (the per-emitter lookup only holds active, enabled emitters).
- **Randomness** - spawn variation comes from the system's `StdRng`: `new()`/`default()` seed it from entropy, `with_seed(n)` makes it reproducible for a given engine version (headless frame comparisons, replays).

## Collisions

`ParticleEmitter::with_collision(CollisionConfig)` makes `ParticleSystem::update` collide that emitter's particles with the world (all in `mod.rs`, config types in `collision.rs`):

- **Colliders** - entities with `Transform` + `Collider`, gathered once per update and only when some emitter has a collision config. Geometry comes from `Collider::world_aabb` (same bounds as physics and debug draw): `Box` uses that AABB, `Circle` a circle at its centre with radius half its width. Trigger colliders and `Inactive` entities are skipped. No `RigidBody` is needed.
- **Filtering** - a collider counts only if `collider.layer & config.collision_mask != 0`. The collider on a particle's own emitter entity is always ignored (particles spawn inside it). Each particle is a circle of `config.radius`. `Bounce` resolves every overlapping collider in turn (so a particle sliding on a floor still hits a wall gathered after it) but sends at most one event per update (a corner hit is one event); `Die`, `Stick` and `Event` stop at the first contact. Each collider is resolved once per update, so a particle squeezed between nearly opposing colliders can end an update overlapping one of them; the next update corrects it, and a gap narrower than the particle can't be resolved at all.
- **Responses** - `Die` removes the particle. `Bounce` reflects velocity along the contact normal with the restitution, then pushes the particle just outside the surface. `Stick` zeroes velocity and sets the private `stuck` flag: no more forces or movement, but it still ages, fades and dies. `Event` passes through.
- **Events** - opt-in with `CollisionConfig::with_events()`: `Die` and `Stick` emit one `ParticleCollision`. `Bounce` emits only on contact entry (the particle touched no collider last update) and only when the impact speed along the normal is at least `MIN_BOUNCE_EVENT_SPEED` (20 px/s, also in the prelude). A particle resting on a surface stays touching, so it emits nothing more, whatever the gravity or dt. `CollisionResponse::Event` always emits on entry. Both entry checks use the private `touching` flag (not-touching to touching), which is per particle, not per collider: moving straight from one collider into an overlapping or adjacent one (or sliding along a floor into a wall) doesn't notify again. Events carry the emitter and collider entities, the particle position, the surface normal and the velocity before the response.
- **Cost** - brute force (every colliding particle against every gathered collider), which is fine for hundreds of particles against dozens of colliders.
- **Tunnelling** - collisions are checked at particle positions once per update, so very fast particles can pass through thin colliders. Use thicker colliders or lower speeds.
