---
name: ryo-particles
description: Particle system - emitters, attractors, collisions, presets for visual effects.
metadata:
  version: 1.0.0
---

# Ryo2D Particle System Specialist

You help users create particle effects including fire, smoke, explosions, magic, weather, and custom effects using emitters, attractors, and collisions.

**For complete API signatures, see the `ryo-api` skill.**

## Quick Start

```rust
// Fire effect
let sprite = ctx.assets.load_sprite("particle.png");
ctx.world.spawn((
    Transform::new(100.0, 100.0),
    ParticleEmitter::fire(sprite),
));
```

## Built-in Presets

| Preset | Use Case |
|--------|----------|
| `fire(sprite)` | Torches, bonfires |
| `smoke(sprite)` | Ambient smoke |
| `explosion(sprite, count)` | One-shot bursts |
| `magic(sprite)` | Sparkle effects |
| `rain(sprite)` | Weather |
| `snow(sprite)` | Weather |
| `sparks(sprite)` | Impact effects |
| `confetti(sprite)` | Celebrations |
| `comet_trail(sprite)` | Projectile trails |

Additional: `flame_small`, `smoke_thick`, `explosion_debris`, `magic_heal`, `dust`, `bubbles`, `exhaust`.

## Custom Emitters

```rust
ParticleEmitter::new(sprite)
    .with_mode(EmissionMode::Continuous { rate: 30.0 })
    .with_shape(EmissionShape::Circle { radius: 10.0, hollow: false })
    .with_lifetime(1.0, 2.0)
    .with_size(4.0, 8.0)
    .with_end_size(1.0, 2.0)
    .with_color(Color::CYAN, Color::TRANSPARENT)
    .with_gravity(0.0, -20.0)
    .with_drag(0.5)
```

## Emission Modes

```rust
EmissionMode::Continuous { rate: 30.0 }           // Particles per second
EmissionMode::Burst { count: 50 }                  // Single burst
EmissionMode::BurstRepeating { count: 20, interval: 0.5, repeats: Some(5) }
```

## Emission Shapes

```rust
EmissionShape::Point                               // Single point
EmissionShape::Line { start_x, start_y, end_x, end_y }
EmissionShape::Circle { radius, hollow }           // hollow = edge only
EmissionShape::Rectangle { width, height }
EmissionShape::Cone { direction, spread, min_speed, max_speed }
```

## Attractors

```rust
// Gravity well (negative strength = repel)
ctx.world.spawn((
    Transform::new(x, y),
    ParticleAttractor::point(200.0, 100.0),  // strength, radius
));

// Vortex (swirling)
ParticleAttractor::vortex(150.0, 80.0)

// Wind zone
ParticleAttractor::area(800.0, 600.0, 50.0, 0.0)  // w, h, fx, fy

// Turbulence (chaotic noise)
ParticleAttractor::turbulence(30.0, 50.0, 1.0)  // strength, scale, speed
```

## Collisions

```rust
// Bouncing particles
.with_collision(CollisionConfig::bouncy(2.0, 0.7))  // radius, restitution

// Stick on contact
.with_collision(CollisionConfig::sticky(2.0))

// Die on contact
.with_collision(CollisionConfig::deadly(2.0))
```

## Particle Trails

```rust
.with_trail(TrailConfig::new(3.0)
    .with_max_points(10)
    .with_fade(true)
    .with_color(Color::YELLOW))
```

## One-Shot Effects

```rust
fn explode(ctx: &mut GameContext, x: f32, y: f32) {
    ctx.world.spawn((
        Transform::new(x, y),
        ParticleEmitter::explosion(sprite, 50),
    ));
}

// Cleanup when done
if emitter.is_finished() {
    ctx.world.despawn(entity);
}
```

## Visual Effects

```rust
.with_effect(SpriteEffect::glow_with_intensity(
    Color::new(100, 150, 255, 255),
    6.0,   // radius
    1.0,   // intensity
))
```

## Common Patterns

### Mouse-Following Attractor

```rust
fn update(&mut self, ctx: &mut GameContext, dt: f32) {
    let (mx, my) = ctx.input.mouse_position();
    if let Some(mut t) = ctx.world.get_component_mut::<Transform>(self.attractor) {
        t.x = mx;
        t.y = my;
    }
}
```

### Weather System

```rust
// Rain across screen
ctx.world.spawn((
    Transform::new(400.0, 0.0),
    ParticleEmitter::rain(drop_sprite),
));
```

### Hit Effect

```rust
fn on_hit(ctx: &mut GameContext, x: f32, y: f32) {
    ctx.world.spawn((
        Transform::new(x, y),
        ParticleEmitter::sparks(spark_sprite)
            .with_collision(CollisionConfig::bouncy(2.0, 0.6)),
    ));
}
```

## Tips

- **Presets First**: Try presets before building custom emitters
- **Transform Required**: Emitters and attractors need `Transform` component
- **One-Shots**: Check `is_finished()` to despawn completed burst emitters
- **Performance**: Keep `max_particles` reasonable (100-500 typical)
- **Layer Masks**: Give emitters a layer with `with_layer(0b10)` and attractors a matching `with_layer_mask(0b10)`; a masked attractor ignores particles from emitters without a shared layer bit (the default layer is 0)
- **Drag**: Use drag to naturally limit particle lifetime
- **Trails**: Use sparingly - trails add vertex overhead
