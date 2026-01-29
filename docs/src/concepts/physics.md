# Physics System

Ferrum2D uses [Rapier2D](https://rapier.rs/), a professional-grade 2D physics engine, for realistic physics simulation including gravity, collisions, and rigid body dynamics.

## Overview

The physics system handles:

- **Rigid body simulation** (dynamic, kinematic, and static bodies)
- **Collision detection and response** (bouncing, sliding)
- **Collision events** (callbacks when objects collide)
- **Automatic synchronization** between physics and ECS transforms

All physics simulation runs at a fixed timestep (default 60Hz) for deterministic behavior.

## Quick Start

To add physics to an entity, give it a `Transform`, `RigidBody`, and `Collider`:

```rust
use ferrum2d::prelude::*;

// Create a bouncing ball with physics
let ball = ctx.world.spawn((
    Transform::new(400.0, 100.0),
    Velocity::zero(),
    RigidBody::new()              // Dynamic body, affected by gravity
        .with_restitution(0.8),   // Bouncy
    Collider::circle(16.0),       // Circle collider, radius 16
));

// Create a static floor
let floor = ctx.world.spawn((
    Transform::new(0.0, 550.0),
    RigidBody::static_body(),     // Static body, never moves
    Collider::box_collider(800.0, 50.0),
));
```

The engine automatically:
1. Registers entities with physics components into Rapier
2. Steps the physics simulation each fixed update
3. Syncs positions back to your Transform components

## Rigid Body Types

### Dynamic Bodies

Dynamic bodies are fully simulated - they respond to gravity, forces, and collisions:

```rust
// Default: dynamic body with gravity
RigidBody::new()

// Configure mass and bounciness
RigidBody::new()
    .with_mass(2.0)           // Heavier object
    .with_restitution(0.5)    // Half of kinetic energy preserved on bounce

// Disable gravity for this body
RigidBody::new()
    .with_gravity_scale(0.0)  // Floats in space
```

### Kinematic Bodies

Kinematic bodies move according to their velocity but aren't affected by forces or gravity. Use them for moving platforms, player-controlled characters, or scripted motion:

```rust
// Moving platform
let platform = ctx.world.spawn((
    Transform::new(200.0, 400.0),
    Velocity { dx: 50.0, dy: 0.0 },  // Moves right at 50 px/sec
    RigidBody::kinematic(),           // Not affected by physics
    Collider::box_collider(128.0, 16.0),
));
```

**Important:** For kinematic bodies, you control position via `Transform` or `Velocity`. The physics engine syncs your changes into Rapier each frame.

### Static Bodies

Static bodies never move. They're perfect for walls, floors, and level geometry:

```rust
// Ground
let ground = ctx.world.spawn((
    Transform::new(0.0, 580.0),
    RigidBody::static_body(),
    Collider::box_collider(1280.0, 40.0),
));
```

Static bodies use less memory and are faster to simulate than kinematic bodies.

## Collider Shapes

Colliders define the physical shape for collision detection:

```rust
// Circle (radius)
Collider::circle(16.0)

// Box (width, height)
Collider::box_collider(64.0, 32.0)

// Offset from transform origin
Collider::box_collider(32.0, 32.0)
    .with_offset(-16.0, -16.0)  // Center the hitbox
```

### Sensors (Triggers)

Sensors detect overlaps without causing physical response - perfect for pickups, trigger zones, and detection areas:

```rust
// Pickup item - detects touch but doesn't block movement
let coin = ctx.world.spawn((
    Transform::new(300.0, 200.0),
    RigidBody::static_body(),
    Collider::circle(12.0).as_sensor(),  // Won't block, just detects
));
```

### Collision Properties

```rust
Collider::box_collider(32.0, 48.0)
    .with_friction(0.5)        // Sliding friction (0 = ice, 1 = sticky)
    .with_restitution(0.3)     // Bounciness on collider level
```

## Collision Events

When objects collide, the engine generates events you can query:

```rust
fn update(&mut self, ctx: &mut GameContext) {
    // Process all collision events this frame
    for event in ctx.collision_events.iter() {
        match event.event_type {
            CollisionEventType::Started => {
                println!("Collision started between {:?} and {:?}", 
                    event.entity_a, event.entity_b);
                
                // Check if either is a sensor
                if event.sensor_a || event.sensor_b {
                    self.handle_trigger(event.entity_a, event.entity_b, ctx);
                } else {
                    self.handle_collision(event.entity_a, event.entity_b, ctx);
                }
            }
            CollisionEventType::Stopped => {
                println!("Collision ended");
            }
        }
    }
}

fn handle_trigger(&mut self, a: Entity, b: Entity, ctx: &mut GameContext) {
    // Check if player touched a coin
    if a == self.player || b == self.player {
        let other = if a == self.player { b } else { a };
        if self.coins.contains(&other) {
            ctx.world.despawn(other).ok();
            self.score += 1;
        }
    }
}
```

### Collision Event Fields

| Field | Description |
|-------|-------------|
| `entity_a`, `entity_b` | The two entities involved |
| `event_type` | `Started` or `Stopped` |
| `sensor_a`, `sensor_b` | Whether each collider is a sensor |

## Applying Forces and Impulses

For dynamic bodies, you can apply physics forces:

```rust
fn update(&mut self, ctx: &mut GameContext) {
    // Jump: apply an instant impulse
    if ctx.input.key_pressed(KeyCode::Space) && self.grounded {
        if let Some(mut vel) = ctx.world.get_component_mut::<Velocity>(self.player) {
            vel.dy = -400.0;  // Instant upward velocity
        }
        self.grounded = false;
    }
    
    // Movement: set velocity directly
    let mut move_x = 0.0;
    if ctx.input.key_down(KeyCode::KeyA) { move_x -= 200.0; }
    if ctx.input.key_down(KeyCode::KeyD) { move_x += 200.0; }
    
    if let Some(mut vel) = ctx.world.get_component_mut::<Velocity>(self.player) {
        vel.dx = move_x;
    }
}
```

## Gravity Configuration

Configure global gravity through `PhysicsConfig`:

```rust
// In EngineBuilder
EngineBuilder::new()
    .physics_config(PhysicsConfig {
        gravity_x: 0.0,
        gravity_y: 980.0,  // Down is positive Y
    })

// Or use presets
PhysicsConfig::default()        // (0, 980) - Earth-like
PhysicsConfig::zero_gravity()   // (0, 0) - Space
```

You can also override gravity per-body:

```rust
// Floaty jump feel
RigidBody::new().with_gravity_scale(0.5)

// No gravity (flying enemy)
RigidBody::new().with_gravity_scale(0.0)

// Extra gravity (heavy object)
RigidBody::new().with_gravity_scale(2.0)
```

## Coordinate System

Rapier uses a meters-based coordinate system internally. Ferrum2D automatically converts between pixel coordinates and physics units:

```
PIXELS_PER_METER = 100.0

Your Transform: 500 pixels  →  Rapier: 5.0 meters
Rapier result: 3.2 meters   →  Your Transform: 320 pixels
```

This conversion is transparent - you always work in pixels.

## Complete Example: Breakout Physics

```rust
use ferrum2d::prelude::*;

struct BreakoutGame {
    paddle: Entity,
    ball: Entity,
    bricks: Vec<Entity>,
    score: u32,
}

impl Game for BreakoutGame {
    fn init(&mut self, ctx: &mut GameContext) {
        // Ball - bouncy, affected by physics
        self.ball = ctx.world.spawn((
            Transform::new(400.0, 300.0),
            Velocity { dx: 200.0, dy: -300.0 },
            RigidBody::new()
                .with_restitution(1.0)      // Perfect bounce
                .with_gravity_scale(0.0),   // No gravity
            Collider::circle(8.0),
            SpriteRenderer::from_color([255, 255, 255]),
        ));
        
        // Paddle - kinematic, player controlled
        self.paddle = ctx.world.spawn((
            Transform::new(350.0, 550.0),
            Velocity::zero(),
            RigidBody::kinematic(),
            Collider::box_collider(100.0, 16.0),
            SpriteRenderer::from_color([100, 200, 100]),
        ));
        
        // Walls - static
        self.spawn_walls(ctx);
        
        // Bricks - static sensors (we handle destruction in events)
        self.spawn_bricks(ctx);
    }
    
    fn update(&mut self, ctx: &mut GameContext) {
        // Paddle movement
        let mut paddle_vel = 0.0;
        if ctx.input.key_down(KeyCode::ArrowLeft) { paddle_vel = -400.0; }
        if ctx.input.key_down(KeyCode::ArrowRight) { paddle_vel = 400.0; }
        
        if let Some(mut vel) = ctx.world.get_component_mut::<Velocity>(self.paddle) {
            vel.dx = paddle_vel;
        }
        
        // Handle brick collisions
        for event in ctx.collision_events.iter() {
            if event.event_type == CollisionEventType::Started {
                self.check_brick_hit(event.entity_a, event.entity_b, ctx);
            }
        }
    }
    
    fn check_brick_hit(&mut self, a: Entity, b: Entity, ctx: &mut GameContext) {
        // Is one of them the ball?
        if a != self.ball && b != self.ball { return; }
        
        let brick = if a == self.ball { b } else { a };
        
        // Is the other a brick?
        if let Some(pos) = self.bricks.iter().position(|&b| b == brick) {
            ctx.world.despawn(brick).ok();
            self.bricks.swap_remove(pos);
            self.score += 10;
        }
    }
}
```

## Best Practices

1. **Use static bodies for level geometry** - they're faster than kinematic
2. **Use sensors for pickups and triggers** - they don't affect physics
3. **Set restitution = 1.0 for perfect bounces** - good for Pong/Breakout
4. **Disable gravity for top-down games** - use `PhysicsConfig::zero_gravity()`
5. **Keep colliders simple** - circles and boxes are fastest
6. **Process collision events each frame** - they're cleared automatically

## See Also

- [Collision Detection](./collision.md) - Manual collision checking, layers
- [ECS Architecture](./ecs.md) - Component and entity basics
- [Game Loop](./game-loop.md) - Fixed timestep physics updates
