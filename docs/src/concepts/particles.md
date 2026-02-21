# Particle System

Ryo2D includes a CPU-simulated, GPU-rendered particle system for visual effects like fire, smoke, explosions, magic, and weather. The system uses instanced rendering to achieve 10k+ particles at 60fps.

## Quick Start

```rust
use ryo2d::prelude::*;

// Spawn a fire effect
let fire_sprite = ctx.assets.load_sprite("particle.png");
ctx.world.spawn((
    Transform::new(100.0, 100.0),
    ParticleEmitter::fire(fire_sprite),
));
```

## Built-in Presets

The particle system includes ready-to-use presets for common effects:

| Preset | Description |
|--------|-------------|
| `ParticleEmitter::fire(sprite)` | Upward orange-yellow flames with glow |
| `ParticleEmitter::smoke(sprite)` | Slow expanding gray particles |
| `ParticleEmitter::explosion(sprite, count)` | Radial burst with trails |
| `ParticleEmitter::magic(sprite)` | Glowing wandering sparkles |
| `ParticleEmitter::rain(sprite)` | Fast downward streaks |
| `ParticleEmitter::snow(sprite)` | Gentle floating descent |
| `ParticleEmitter::sparks(sprite)` | Bouncing bright particles |
| `ParticleEmitter::confetti(sprite)` | Colorful tumbling pieces |
| `ParticleEmitter::comet_trail(sprite)` | High-rate glowing trail |

Additional presets: `flame_small`, `smoke_thick`, `explosion_debris`, `magic_heal`, `dust`, `bubbles`, `exhaust`.

## Custom Emitters

Build custom emitters with the builder pattern:

```rust
let emitter = ParticleEmitter::new(sprite)
    .with_mode(EmissionMode::Continuous { rate: 50.0 })
    .with_shape(EmissionShape::Circle { radius: 10.0, hollow: false })
    .with_lifetime(1.0, 2.0)
    .with_size(4.0, 8.0)
    .with_end_size(1.0, 2.0)
    .with_color(Color::CYAN, Color::TRANSPARENT)
    .with_gravity(0.0, -20.0)
    .with_drag(0.5);

ctx.world.spawn((
    Transform::new(x, y),
    emitter,
));
```

## Emission Modes

Control how particles are spawned:

```rust
// Continuous - particles per second
EmissionMode::Continuous { rate: 30.0 }

// Single burst
EmissionMode::Burst { count: 50 }

// Repeated bursts
EmissionMode::BurstRepeating {
    count: 20,
    interval: 0.5,       // seconds between bursts
    repeats: Some(5),    // None = infinite
}
```

## Emission Shapes

Define where particles spawn:

```rust
// From a single point
EmissionShape::Point

// Along a line
EmissionShape::Line {
    start_x: -50.0, start_y: 0.0,
    end_x: 50.0, end_y: 0.0,
}

// Within a circle
EmissionShape::Circle {
    radius: 20.0,
    hollow: false,  // true = edge only
}

// Within a rectangle
EmissionShape::Rectangle {
    width: 100.0,
    height: 50.0,
}

// Directional cone with velocity
EmissionShape::Cone {
    direction: -std::f32::consts::FRAC_PI_2,  // up
    spread: 0.5,           // radians
    min_speed: 50.0,
    max_speed: 100.0,
}
```

## One-Shot Effects

For explosions and impacts that should play once:

```rust
fn explode(ctx: &mut SceneContext, x: f32, y: f32) {
    ctx.world.spawn((
        Transform::new(x, y),
        ParticleEmitter::explosion(sprite, 30),
    ));
}
```

One-shot emitters automatically disable after firing. Check `emitter.is_finished()` to clean up:

```rust
// Cleanup one-shot emitters when done
let mut to_despawn = Vec::new();
for entity in ctx.world.entities_with::<ParticleEmitter>() {
    if let Some(emitter) = ctx.world.get_component::<ParticleEmitter>(entity) {
        if emitter.is_finished() {
            to_despawn.push(entity);
        }
    }
}
for entity in to_despawn {
    ctx.world.despawn(entity);
}
```

## Particle Attractors

Attractors create force fields that influence particle motion:

```rust
// Gravity well that pulls particles
ctx.world.spawn((
    Transform::new(mouse_x, mouse_y),
    ParticleAttractor::point(200.0, 100.0),  // strength, radius
));

// Vortex that swirls particles
ctx.world.spawn((
    Transform::new(400.0, 300.0),
    ParticleAttractor::vortex(150.0, 80.0),
));

// Wind zone
ctx.world.spawn((
    Transform::new(0.0, 0.0),
    ParticleAttractor::area(800.0, 600.0, 50.0, 0.0),  // w, h, fx, fy
));

// Turbulence for chaotic motion
ctx.world.spawn((
    Transform::new(400.0, 300.0),
    ParticleAttractor::turbulence(30.0, 50.0, 1.0),  // strength, scale, speed
));
```

### Attractor Types

| Type | Description |
|------|-------------|
| `Point` | Attract/repel toward a point (negative strength = repel) |
| `Vortex` | Swirl particles around the attractor |
| `Area` | Apply uniform force within a rectangular region |
| `Turbulence` | Noise-based chaotic motion |
| `Line` | Attract toward a line segment |

## Particle Collisions

Enable particles to interact with the world:

```rust
// Bouncing sparks
ParticleEmitter::sparks(sprite)
    .with_collision(CollisionConfig::bouncy(2.0, 0.7))  // radius, restitution

// Particles that stick on contact
ParticleEmitter::new(sprite)
    .with_collision(CollisionConfig::sticky(2.0))

// Particles that die on contact
ParticleEmitter::new(sprite)
    .with_collision(CollisionConfig::deadly(2.0))
```

### Collision Responses

| Response | Description |
|----------|-------------|
| `Bounce` | Reflect off surfaces with restitution |
| `Stick` | Stop moving on contact |
| `Die` | Remove particle on contact |
| `Event` | Emit `ParticleCollision` event for custom handling |

## Particle Trails

Add motion trails to particles:

```rust
ParticleEmitter::new(sprite)
    .with_trail(TrailConfig::new(3.0)      // width
        .with_max_points(10)
        .with_fade(true)
        .with_color(Color::YELLOW))
```

## Visual Effects

Combine with sprite effects for enhanced visuals:

```rust
ParticleEmitter::magic(sprite)
    .with_effect(SpriteEffect::glow_with_intensity(
        Color::new(100, 150, 255, 255),
        6.0,    // radius
        1.0,    // intensity
    ))
```

## Complete Example

```rust
use ryo2d::prelude::*;

struct ParticleDemo {
    particle_sprite: SpriteHandle,
}

impl Scene for ParticleDemo {
    fn on_enter(&mut self, ctx: &mut SceneContext) {
        self.particle_sprite = ctx.assets.load_sprite("particle.png");

        // Fire effect
        ctx.world.spawn((
            Transform::new(200.0, 400.0),
            ParticleEmitter::fire(self.particle_sprite.clone()),
        ));

        // Gravity well attractor
        ctx.world.spawn((
            Transform::new(400.0, 300.0),
            ParticleAttractor::point(150.0, 200.0),
        ));
    }

    fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
        // Spawn explosion on click
        if ctx.input.mouse_just_pressed(MouseButton::Left) {
            let (mx, my) = ctx.input.mouse_position();
            ctx.world.spawn((
                Transform::new(mx, my),
                ParticleEmitter::explosion(self.particle_sprite.clone(), 40),
            ));
        }
        SceneAction::Continue
    }
}
```

## Performance Tips

- **Max Particles**: Keep `max_particles` reasonable (100-500 is typical)
- **Drag**: Use drag to naturally limit particle lifetime
- **Trails**: Use sparingly - trails add vertex overhead
- **Attractors**: Use `with_layer_mask()` to limit which particles are affected
- **Collisions**: Only enable when needed - spatial queries are expensive
- **One-Shots**: Despawn finished one-shot emitters to free resources

## Next Steps

- [Sprites & Rendering](./sprites.md) - Particle sprites and effects
- [Animation System](./animation.md) - Animated particle sprites
- [Physics System](./physics.md) - Particle collision surfaces
