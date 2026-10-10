---
name: ryo-physics
description: Rapier2D physics integration - colliders, rigid bodies, events.
metadata:
  version: 1.0.0
---

# Ryo2D Physics Specialist

You help users integrate Rapier2D physics into their games, including rigid bodies, colliders, and collision events.

**For complete API signatures, see the `ryo-api` skill.**

## RigidBody Types

```rust
// Dynamic - affected by forces and gravity
let player = ctx.world.spawn((
    Transform::new(100.0, 100.0),
    RigidBody::new(),
    Collider::circle_collider(16.0),
));

// Kinematic - moved by code, not physics (platforms)
let platform = ctx.world.spawn((
    Transform::new(200.0, 400.0),
    RigidBody::kinematic(),
    Collider::box_collider(100.0, 20.0),
));

// Walls/floors - there is no static body type; use a kinematic body that never moves.
// A Collider without a RigidBody is never registered with the physics world.
let wall = ctx.world.spawn((
    Transform::new(0.0, 500.0),
    RigidBody::kinematic(),
    Collider::box_collider(800.0, 50.0),
));
```

## Collider Shapes

```rust
Collider::circle_collider(16.0)             // Circle with radius
Collider::box_collider(32.0, 64.0)          // Axis-aligned box (width, height)
Collider::circle_collider(32.0).as_trigger() // Trigger (sensor) - detects but no physics response
```

## Collision Events

```rust
fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    // Objects that just started touching
    let started: Vec<_> = ctx.events.read::<CollisionStarted>().copied().collect();
    for event in started {
        if let Some(other) = event.other(self.player) {
            self.handle_player_collision(other, ctx);
        }
    }

    // Objects that stopped touching
    for event in ctx.events.read::<CollisionStopped>() {
        // Exit triggers, reset states
    }

    SceneAction::Continue
}
```

## CollisionStarted / CollisionStopped API

Both events have the same shape (read via `ctx.events.read::<T>()`):

```rust
event.entity_a             // First entity
event.entity_b             // Second entity
event.involves(entity)     // Check if entity is in this collision
event.other(entity)        // Option<Entity>: the other entity (None if not involved)
```

The lower-level `PhysicsCollisionEvent` (`entity_a`, `entity_b`, `is_sensor`, `is_started()`, `is_stopped()`) comes from `RapierPhysicsWorld::get_collision_events()`.

## Common Patterns

### Damage/Death Triggers

```rust
let spikes = ctx.world.spawn((
    Transform::new(300.0, 480.0),
    RigidBody::kinematic(),
    Collider::box_collider(100.0, 20.0).as_trigger(),
    Hazard { damage: 1 },
));

for event in ctx.events.read::<CollisionStarted>() {
    if let Some(other) = event.other(self.player) {
        if let Some(hazard) = ctx.world.get::<Hazard>(other) {
            self.health -= hazard.damage;
        }
    }
}
```

### Collectibles

```rust
let coin = ctx.world.spawn((
    Transform::new(x, y),
    Renderable::sprite(coin_sprite),
    RigidBody::kinematic(),
    Collider::circle_collider(12.0).as_trigger(),
    Coin { value: 10 },
));

let mut to_despawn = Vec::new();
for event in ctx.events.read::<CollisionStarted>() {
    if let Some(other) = event.other(self.player) {
        if let Some(coin) = ctx.world.get::<Coin>(other) {
            self.score += coin.value;
            to_despawn.push(other);
        }
    }
}
for entity in to_despawn {
    let _ = ctx.world.despawn(entity);
}
```

### Velocity Control

```rust
// Modify velocity directly for jumps, knockback, etc.
if let Some(mut vel) = ctx.world.get_mut::<Velocity>(entity) {
    vel.dy = -300.0;  // Jump!
}
```

## Tips

- Use **triggers** (`as_trigger()`) for triggers, pickups, detection zones - they don't push objects
- Read **CollisionStarted** events for one-time events (damage, pickups)
- Read **CollisionStopped** events for exit events (leaving trigger zones)
- **Kinematic** bodies are great for moving platforms
- Despawn collected items in a separate loop to avoid iterator invalidation
- Events last one fixed update - read them in the update they arrive
- Collision events come from the most recent physics step, which runs after your scene's `update`, so a contact shows up in the next update together with the positions physics produced. Each is delivered once, even while physics is paused (`is_physics_paused`)

