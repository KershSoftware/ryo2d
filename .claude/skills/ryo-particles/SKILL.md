---
name: ryo-particles
description: Particle system - emitters, attractors, collisions, presets for visual effects.
metadata:
  version: 2.0.0
---

# Ryo2D Particle System Specialist

You help users create particle effects (fire, smoke, explosions, magic, weather) with emitters, attractors and collisions.

**For complete API signatures, see the `ryo-api` skill.**

## Driving the system

Particles are scene-driven: the engine never runs them. The scene owns a `ParticleSystem`, steps it in `update` and draws it in `render`:

```rust
struct Level {
  particles: ParticleSystem,
  spark: SpriteHandle,
}

impl Scene for Level {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    self.spark = ctx.assets.load_sprite("particle.png").expect("particle sprite");
    ctx.world.spawn((Transform::from_xy(400.0, 500.0), ParticleEmitter::fire(self.spark.clone())));
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    // Spawns, simulates, applies attractors and collisions
    self.particles.update(ctx.world, dt);
    SceneAction::Continue
  }

  fn render(&mut self, gpu: &mut GpuRenderContext) {
    // Add the particles to the batch (their sprites upload when first drawn)
    self.particles.collect_for_render(gpu.world, gpu.camera, gpu.batch);
  }
}
```

- `ParticleSystem::new()` is randomly seeded; `ParticleSystem::with_seed(n)` gives the same particles for the same updates (headless tests, replays).
- `update(world, dt)` runs at the fixed rate with the `dt` `update` receives, so slow motion and pause apply.
- Particles sort by the emitter's `z_order` (`with_z_order`, default 0) together with entities, other sprites and trails; the UI draws last.
- `particle_count()`, `clear()` and `clear_emitter(entity)` manage live particles; call `clear()` when a scene resets the world.

## Emitters

An emitter is an entity with `Transform` + `ParticleEmitter`. It emits at its `GlobalTransform`, so it can be a child of a moving entity. An emitter spawned this step is placed at once (inserting a `Transform` places the entity), so a burst spawned and fired in the same `update` starts at the emitter. An emitter moved by editing its `Transform` emits from its previous position until the step's propagation.

### Presets

| Preset | Use |
|--------|-----|
| `fire(sprite)`, `flame_small(sprite)` | Torches, bonfires |
| `smoke(sprite)`, `smoke_thick(sprite)` | Ambient smoke |
| `explosion(sprite, count)`, `explosion_debris(sprite, count)` | One-shot bursts |
| `magic(sprite)`, `magic_heal(sprite)` | Sparkles |
| `rain(sprite)`, `snow(sprite)` | Weather |
| `sparks(sprite)`, `dust(sprite)` | Impacts |
| `confetti(sprite)`, `bubbles(sprite)` | Celebration, underwater |
| `comet_trail(sprite)`, `exhaust(sprite)` | Projectile trails, engines |

### Custom emitters

```rust
ParticleEmitter::new(sprite)
  .with_mode(EmissionMode::Continuous { rate: 30.0 })
  .with_shape(EmissionShape::Circle { radius: 10.0, hollow: false })
  .with_lifetime(1.0, 2.0)
  .with_size(4.0, 8.0)
  .with_end_size(1.0, 2.0)
  .with_color(Color::CYAN, Color::TRANSPARENT)
  .with_speed(20.0, 60.0)
  .with_gravity(0.0, -20.0)
  .with_drag(0.5)
  .with_max_particles(300)
```

Also `with_rotation(min, max)`, `with_angular_velocity(min, max)`, `with_effect(SpriteEffect)`, `with_layer(bits)`, `with_collision(CollisionConfig)`, and `one_shot()` (turns any mode into a single burst). Ranges whose min is above the max sample the min.

```rust
EmissionMode::Continuous { rate: 30.0 } // particles per second
EmissionMode::Burst { count: 50 } // one burst
EmissionMode::BurstRepeating { count: 20, interval: 0.5, repeats: Some(5) }

EmissionShape::Point
EmissionShape::Line { start_x: -50.0, start_y: 0.0, end_x: 50.0, end_y: 0.0 }
EmissionShape::Circle { radius: 20.0, hollow: true } // hollow = edge only
EmissionShape::Rectangle { width: 800.0, height: 10.0 }
EmissionShape::Cone { direction: -std::f32::consts::FRAC_PI_2, spread: 0.6, min_speed: 50.0, max_speed: 120.0 }
```

Runtime control on the component: `set_enabled(bool)`, `reset()` (lets a burst fire again), `burst(count)`, `is_finished()`.

### One-shots

Nothing despawns finished emitters; check `is_finished()`:

```rust
let boom = ctx
  .world
  .spawn((Transform::from_xy(x, y), ParticleEmitter::explosion(self.spark.clone(), 50)))
  .id();
self.bursts.push(boom);

// later, after self.particles.update(..)
self.bursts.retain(|&entity| {
  let done = ctx
    .world
    .get::<ParticleEmitter>(entity)
    .is_none_or(|emitter| emitter.is_finished());
  if done {
    ctx.world.despawn(entity);
  }
  !done
});
```

### Disabled emitters

An emitter on a `Disabled` entity (or with `enabled = false`) stops emitting, and its live particles lose gravity and collisions. A `Disabled` emitter's particles are also not drawn.

## Attractors

An attractor is an entity with `Transform` + `ParticleAttractor`; forces from several attractors add up.

```rust
ctx.world.spawn((Transform::from_xy(x, y), ParticleAttractor::point(200.0, 100.0))); // strength, radius; negative repels
ParticleAttractor::gravity_well(300.0, 150.0)
ParticleAttractor::vortex(150.0, 80.0)
ParticleAttractor::spiral(150.0, 80.0, 40.0) // strength, radius, inward pull
ParticleAttractor::line(-100.0, 0.0, 100.0, 0.0, 120.0, 60.0)
ParticleAttractor::area(800.0, 600.0, 50.0, 0.0) // width, height, force x, force y
ParticleAttractor::turbulence(30.0, 50.0, 1.0) // strength, scale, speed
```

`with_layer_mask(mask)` limits an attractor to particles whose emitter has a shared bit in `with_layer(bits)`. Emitters default to layer 0, which only unmasked attractors affect. `with_enabled(false)` switches one off.

Following the mouse:

```rust
let (mx, my) = ctx.input.mouse_position();
let target = ctx.camera.screen_to_world(mx, my);
if let Some(mut transform) = ctx.world.get_mut::<Transform>(self.attractor) {
  transform.translation = Vec2::new(target.0, target.1);
}
```

## Collisions

`with_collision(CollisionConfig)` makes an emitter's particles collide with entities that have `Collider` (no `RigidBody` needed). Geometry is the collider as physics simulates it: centred on the entity's origin plus `offset`, unscaled (`Collider::world_bounds`). Trigger colliders, `Disabled` entities and the emitter's own collider are skipped. Each particle is a circle of the config's radius.

```rust
.with_collision(CollisionConfig::bouncy(2.0, 0.7)) // radius, restitution
.with_collision(CollisionConfig::sticky(2.0)) // frozen until its lifetime ends
.with_collision(CollisionConfig::die_on_hit(2.0).with_events())
.with_collision(
  CollisionConfig::new(2.0)
    .with_mask(0b10) // matched against Collider::layer
    .with_response(CollisionResponse::Event),
)
```

| Response | Behaviour | `ParticleCollision` messages |
|----------|-----------|------------------------------|
| `Die` | Particle removed | Once, with `with_events()` |
| `CollisionResponse::bounce(r)` | Reflects with restitution | With `with_events()`, on contact entry at an impact speed of at least `MIN_BOUNCE_EVENT_SPEED` (20 px/s) |
| `Stick` | Stops; still ages and fades | Once, with `with_events()` |
| `Event` | Passes through | Always, on contact entry |

Contact entry is tracked per particle, not per collider: sliding from a floor into a wall does not notify again. Collisions are checked at particle positions once per update, so very fast particles can pass through thin colliders.

`ParticleCollision` is a message; read it after `update`:

```rust
self.particles.update(ctx.world, dt);
for hit in ctx.messages::<ParticleCollision>() {
  // hit.emitter_entity, hit.collider_entity
  // hit.position_x / position_y: the particle at contact
  // hit.normal_x / normal_y: the surface normal, pointing at the particle
  // hit.velocity_x / velocity_y: the velocity before the response
}
```

## Visual effects

```rust
.with_effect(SpriteEffect::glow_with_intensity(Color::new(100, 150, 255, 255), 6.0, 1.0))
```

## Tips

- Try a preset before building a custom emitter.
- Keep `max_particles` in the hundreds per emitter.
- Drag shortens how far particles travel without changing their lifetime.
- Collisions are brute force (every colliding particle against every collider): fine for hundreds of particles against dozens of colliders.
