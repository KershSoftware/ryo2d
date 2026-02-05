# AGENTS.md - Particle System

## Overview

This module provides a CPU-simulated, GPU-rendered particle system for visual effects like fire, smoke, explosions, magic, weather, and more. Particles are rendered via the existing `SpriteGpuRenderer` with instanced batching for 10k+ particles at 60fps.

## Module Structure

| File | Purpose |
|------|---------|
| `mod.rs` | Module exports, `ParticleSystem` main system |
| `particle.rs` | `Particle` component - individual particle state |
| `emitter.rs` | `ParticleEmitter` component, `EmissionMode`, `EmissionShape`, `TrailConfig` |
| `attractor.rs` | `ParticleAttractor` component, `AttractorType` for force fields |
| `collision.rs` | `CollisionConfig`, `CollisionResponse`, `ParticleCollision` event |
| `presets.rs` | Built-in emitter presets (fire, smoke, magic, etc.) |

## Core Components

### ParticleEmitter

Spawns particles according to configurable mode, shape, and properties. Attach to an entity with `Transform`.

**Presets:**
| Preset | Use Case | Characteristics |
|--------|----------|-----------------|
| `ParticleEmitter::fire(sprite)` | Torches, bonfires | Upward orange-yellow with glow |
| `ParticleEmitter::flame_small(sprite)` | Candles, small flames | Compact, short-lived |
| `ParticleEmitter::smoke(sprite)` | Ambient smoke | Slow expanding gray |
| `ParticleEmitter::smoke_thick(sprite)` | Fire/destruction smoke | Dense, dark |
| `ParticleEmitter::explosion(sprite, count)` | One-shot bursts | Radial with trails |
| `ParticleEmitter::explosion_debris(sprite, count)` | Physical debris | Bouncing chunks |
| `ParticleEmitter::magic(sprite)` | Sparkle effects | Glowing wandering |
| `ParticleEmitter::magic_heal(sprite)` | Healing/buffs | Rising green sparkles |
| `ParticleEmitter::rain(sprite)` | Weather rain | Fast downward streaks |
| `ParticleEmitter::snow(sprite)` | Weather snow | Gentle floating |
| `ParticleEmitter::sparks(sprite)` | Impact sparks | Bouncing bright particles |
| `ParticleEmitter::dust(sprite)` | Dust puffs | Expanding clouds |
| `ParticleEmitter::confetti(sprite)` | Celebrations | Multi-colored tumbling |
| `ParticleEmitter::bubbles(sprite)` | Underwater | Rising translucent |
| `ParticleEmitter::comet_trail(sprite)` | Projectile trails | High-rate glow |
| `ParticleEmitter::exhaust(sprite)` | Engine exhaust | Cone-shaped |

**Key Methods:**
```rust
ParticleEmitter::new(sprite)           // Create custom emitter
  .with_mode(EmissionMode::...)        // Continuous, Burst, BurstRepeating
  .with_shape(EmissionShape::...)      // Point, Line, Circle, Rectangle, Cone
  .with_lifetime(min, max)             // Particle lifetime range
  .with_size(min, max)                 // Starting size range
  .with_end_size(min, max)             // Ending size range
  .with_color(start, end)              // Color gradient over lifetime
  .with_speed(min, max)                // Initial speed range
  .with_rotation(min, max)             // Initial rotation range
  .with_angular_velocity(min, max)     // Spin rate range
  .with_gravity(x, y)                  // Applied gravity
  .with_drag(drag)                     // Velocity damping (0-1)
  .with_z_order(z)                     // Render layer
  .with_effect(SpriteEffect::...)      // Glow, dissolve, etc.
  .with_trail(TrailConfig::...)        // Motion trails
  .with_collision(CollisionConfig::...)// Collision response
  .with_max_particles(max)             // Particle limit
  .one_shot()                          // Convert to single burst

emitter.set_enabled(enabled);          // Enable/disable
emitter.reset();                       // Reset burst state
emitter.burst(count);                  // Trigger manual burst
emitter.is_finished()                  // Check if one-shot complete
```

### EmissionMode

| Mode | Description |
|------|-------------|
| `Continuous { rate }` | Emit `rate` particles per second |
| `Burst { count }` | One-shot burst of `count` particles |
| `BurstRepeating { count, interval, repeats }` | Repeated bursts |

### EmissionShape

| Shape | Description |
|-------|-------------|
| `Point` | Emit from emitter position |
| `Line { start_x, start_y, end_x, end_y }` | Along a line segment |
| `Circle { radius, hollow }` | Within or on edge of circle |
| `Rectangle { width, height }` | Within rectangle bounds |
| `Cone { direction, spread, min_speed, max_speed }` | Directional cone |

### ParticleAttractor

Force fields that influence particle motion. Attach to an entity with `Transform`.

**Factory Methods:**
```rust
ParticleAttractor::point(strength, radius)       // Gravity well
ParticleAttractor::gravity_well(strength, radius) // Inverse-square falloff
ParticleAttractor::vortex(strength, radius)      // Rotation
ParticleAttractor::spiral(strength, radius, pull) // Rotation + inward pull
ParticleAttractor::turbulence(strength, scale, speed) // Chaotic noise
ParticleAttractor::area(w, h, fx, fy)            // Directional force field
ParticleAttractor::line(x1, y1, x2, y2, str, rad) // Line attractor

attractor.with_layer_mask(mask)        // Which particles affected
attractor.with_enabled(enabled)        // Toggle active state
```

### AttractorType

| Type | Description |
|------|-------------|
| `Point { strength, radius, falloff }` | Attract/repel toward point |
| `Line { start_x, start_y, end_x, end_y, strength, radius }` | Toward line segment |
| `Area { width, height, force_x, force_y }` | Uniform force in bounds |
| `Vortex { strength, radius, inward_pull }` | Swirling rotation |
| `Turbulence { strength, scale, speed }` | Noise-based chaos |

### CollisionConfig

```rust
CollisionConfig::bouncy(radius, restitution)  // Bounce off surfaces
CollisionConfig::sticky(radius)               // Stop on collision
CollisionConfig::deadly(radius)               // Die on collision
CollisionConfig::event(radius)                // Emit event only

config.with_mask(collision_mask)              // Collision layer filtering
```

### CollisionResponse

| Response | Description |
|----------|-------------|
| `Die` | Particle is removed |
| `Bounce { restitution }` | Reflect velocity |
| `Stick` | Zero velocity, remain |
| `Event` | Emit `ParticleCollision` event |

## Usage Patterns

### Basic Fire Effect

```rust
let fire_sprite = ctx.assets.load_sprite("particle.png");
ctx.world.spawn((
    Transform::new(100.0, 100.0),
    ParticleEmitter::fire(fire_sprite),
));
```

### One-Shot Explosion

```rust
fn explode(ctx: &mut GameContext, x: f32, y: f32) {
    let spark = ctx.assets.load_sprite("spark.png");
    ctx.world.spawn((
        Transform::new(x, y),
        ParticleEmitter::explosion(spark, 50),
    ));
}
```

### Custom Emitter with Attractor

```rust
let emitter = ctx.world.spawn((
    Transform::new(400.0, 300.0),
    ParticleEmitter::new(sprite)
        .with_mode(EmissionMode::Continuous { rate: 30.0 })
        .with_shape(EmissionShape::Circle { radius: 20.0, hollow: true })
        .with_lifetime(1.0, 2.0)
        .with_color(Color::CYAN, Color::TRANSPARENT)
        .with_trail(TrailConfig::new(2.0)),
));

// Mouse-following gravity well
let attractor = ctx.world.spawn((
    Transform::new(mouse_x, mouse_y),
    ParticleAttractor::point(200.0, 150.0),
));
```

### Bouncing Sparks

```rust
ctx.world.spawn((
    Transform::new(x, y),
    ParticleEmitter::sparks(sprite)
        .with_collision(CollisionConfig::bouncy(2.0, 0.7)),
));
```

### Weather Effects

```rust
// Rain across screen width
ctx.world.spawn((
    Transform::new(400.0, 0.0),
    ParticleEmitter::rain(drop_sprite),
));

// Gentle snow
ctx.world.spawn((
    Transform::new(400.0, 0.0),
    ParticleEmitter::snow(flake_sprite),
));
```

## Integration with Engine

The `ParticleSystem` is updated by the engine after game update and renders via `BatchCollector` during GPU render pass.

**Rendering Order:**
1. Background layers
2. Sprites (by z_order)
3. Particles (by z_order within sprite batch)
4. Trails and Lines
5. UI

## Performance Notes

- **Batching**: Particles share texture batches with regular sprites
- **Max Particles**: Default 1000 per emitter, configurable via `with_max_particles()`
- **Pool Recycling**: Dead particles are recycled, no allocation after warmup
- **Attractors**: O(n*m) where n=particles, m=attractors; use layer masks to filter
- **Collisions**: Spatial queries against world colliders; expensive with many particles
- **Trails**: Each particle with trail adds Trail2D overhead

## Tips for AI Agents

1. **Presets First**: Try presets before building custom emitters
2. **One-Shot Cleanup**: One-shot emitters auto-disable; check `is_finished()` to despawn
3. **Layer Masks**: Use `with_layer_mask()` on attractors to affect only specific particle types
4. **Performance**: Keep max_particles reasonable (100-500 typical), use drag to limit lifetime
5. **Trails**: Use sparingly - trails add significant vertex overhead
6. **Effects**: Combine with `SpriteEffect::glow_with_intensity()` for magical looks
7. **Collisions**: Only enable when needed; use `CollisionResponse::Event` for custom handling
8. **Transform Required**: Emitters and attractors need `Transform` component to position them
