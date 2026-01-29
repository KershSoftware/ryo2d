# Physics System

Ferrum2D includes a physics system for simulating gravity, forces, and physical movement.

## Core Components

### Velocity

Already built into the engine - represents movement speed:

```rust
ctx.world.add_component(entity, Velocity { dx: 100.0, dy: 0.0 });
```

The engine automatically applies velocity to transform each frame.

### Acceleration

Apply continuous forces to an entity:

```rust
use ferrum2d::prelude::*;

// Constant acceleration (e.g., thrust, wind)
ctx.world.add_component(entity, Acceleration { ax: 0.0, ay: 50.0 });
```

Acceleration is integrated into velocity automatically by the physics system.

### RigidBody

Controls how physics affects an entity:

```rust
// Default rigid body (affected by gravity)
ctx.world.add_component(entity, RigidBody::new());

// Builder pattern for configuration
ctx.world.add_component(entity, 
    RigidBody::new()
        .with_mass(2.0)        // Heavier objects
        .with_gravity_scale(0.5) // Half gravity
        .with_drag(0.1)        // Air resistance
);

// Kinematic body (not affected by physics, but can affect others)
ctx.world.add_component(platform, 
    RigidBody::new().kinematic()
);
```

| Property | Description | Default |
|----------|-------------|---------|
| `mass` | Mass of the object (for future force calculations) | 1.0 |
| `gravity_scale` | Multiplier for gravity (0 = no gravity, 1 = normal) | 1.0 |
| `drag` | Air resistance (0 = none, higher = more slowdown) | 0.0 |
| `is_kinematic` | If true, unaffected by forces/gravity | false |

## Physics Configuration

Configure global physics settings via `PhysicsConfig`:

```rust
// Default: gravity of 980 pixels/second² (similar to 9.8 m/s² scaled)
let config = PhysicsConfig::default();

// Custom gravity
let config = PhysicsConfig {
    gravity_x: 0.0,
    gravity_y: 500.0,  // Gentler gravity
};

// Zero gravity (space game)
let config = PhysicsConfig::zero_gravity();
```

## How the Physics System Works

Each physics update (fixed timestep), the engine:

1. **Applies gravity** to entities with `RigidBody` (scaled by `gravity_scale`)
2. **Integrates acceleration** into velocity
3. **Applies drag** to velocity (reduces speed over time)
4. **Clears acceleration** for next frame

```
Gravity → Velocity
Acceleration → Velocity → clear Acceleration
Drag → Velocity (reduced)
Velocity → Transform (in movement system)
```

## Example: Falling Objects

```rust
struct FallingDemo {
    objects: Vec<Entity>,
}

impl Game for FallingDemo {
    fn init(&mut self, ctx: &mut GameContext) {
        // Create several falling objects with different properties
        for i in 0..5 {
            let entity = ctx.world.spawn();
            ctx.world.add_component(entity, 
                Transform::new(100.0 + i as f32 * 80.0, 50.0)
            );
            ctx.world.add_component(entity, Velocity::zero());
            ctx.world.add_component(entity, 
                RigidBody::new()
                    .with_gravity_scale(0.5 + i as f32 * 0.25) // Varying gravity
                    .with_drag(i as f32 * 0.02) // Varying drag
            );
            self.objects.push(entity);
        }
    }
}
```

## Example: Jumping

```rust
fn update(&mut self, ctx: &mut GameContext) {
    if ctx.input.key_pressed(KeyCode::Space) {
        if let Some(mut vel) = ctx.world.get_component_mut::<Velocity>(self.player) {
            vel.dy = -400.0; // Jump upward (negative Y)
        }
    }
}
```

## Example: Thrust-Based Movement

```rust
fn update(&mut self, ctx: &mut GameContext) {
    // Spaceship-style controls with acceleration
    let thrust = 200.0;
    let mut ax = 0.0;
    let mut ay = 0.0;
    
    if ctx.input.key_down(KeyCode::KeyW) { ay -= thrust; }
    if ctx.input.key_down(KeyCode::KeyS) { ay += thrust; }
    if ctx.input.key_down(KeyCode::KeyA) { ax -= thrust; }
    if ctx.input.key_down(KeyCode::KeyD) { ax += thrust; }
    
    if let Some(mut accel) = ctx.world.get_component_mut::<Acceleration>(self.ship) {
        accel.ax = ax;
        accel.ay = ay;
    }
}
```

## Kinematic Bodies

Use kinematic rigid bodies for objects that move but aren't affected by physics:

```rust
// Moving platform - moves on a path, not affected by gravity
ctx.world.add_component(platform, RigidBody::new().kinematic());
ctx.world.add_component(platform, Velocity { dx: 50.0, dy: 0.0 });

// Player paddle in a breakout game
ctx.world.add_component(paddle, RigidBody::new().kinematic());
```

## Combining with Collision

Physics and collision work together for realistic behavior:

```rust
// Ball with physics and collision
ctx.world.add_component(ball, Transform::new(100.0, 100.0));
ctx.world.add_component(ball, Velocity::zero());
ctx.world.add_component(ball, RigidBody::new()); // Affected by gravity
ctx.world.add_component(ball, Collider::circle_collider(12.0));

// Floor with collision but no physics response
ctx.world.add_component(floor, Transform::new(0.0, 550.0));
ctx.world.add_component(floor, Collider::box_collider(800.0, 50.0));
ctx.world.add_component(floor, RigidBody::new().kinematic());
```

## See Also

- [Collision Detection](./collision.md) - Shapes and collision response
- [ECS Architecture](./ecs.md) - Component and entity basics
- [Game Loop](./game-loop.md) - Fixed timestep physics updates
