---
name: Ryo2D Physics Specialist
description: Rapier2D physics integration - colliders, rigid bodies, events.
version: 1.0.0
trigger:
  - "physics"
  - "collision"
  - "rigid body"
  - "collider"
---

# Ryo2D Physics Specialist

You help users integrate Rapier2D physics into their games, including rigid bodies, colliders, and collision events.

## Resources

- **Physics Module**: `src/core/rapier_physics.rs`
- **Collision Module**: `src/core/collision.rs`
- **Physics Config**: `src/core/physics.rs`

## RigidBody Types

```rust
// Dynamic - affected by forces and gravity
let player = ctx.world.spawn((
    Transform::new(100.0, 100.0),
    RigidBody::dynamic(),
    Collider::circle(16.0),
));

// Kinematic - moved by code, not physics (platforms)
let platform = ctx.world.spawn((
    Transform::new(200.0, 400.0),
    RigidBody::kinematic(),
    Collider::aabb(100.0, 20.0),
));

// Static - never moves (walls) - implicit if no RigidBody
let wall = ctx.world.spawn((
    Transform::new(0.0, 500.0),
    Collider::aabb(800.0, 50.0),
));
```

## Collider Shapes

```rust
Collider::circle(16.0)              // Circle with radius
Collider::aabb(32.0, 64.0)          // Axis-aligned box (width, height)
Collider::circle(32.0).as_sensor()  // Sensor - detects but no physics response
```

## Collision Events

```rust
fn update(&mut self, ctx: &mut GameContext, dt: f32) {
    // Objects that just started touching
    for event in ctx.collisions_started() {
        if event.involves(self.player) {
            let other = event.other_entity(self.player);
            self.handle_player_collision(other, ctx);
        }
    }

    // Objects that stopped touching
    for event in ctx.collisions_stopped() {
        // Exit triggers, reset states
    }
}
```

## PhysicsCollisionEvent API

```rust
event.entity1              // First entity
event.entity2              // Second entity
event.involves(entity)     // Check if entity is in this collision
event.other_entity(entity) // Get the other entity
event.is_started()         // True if just started touching
event.is_stopped()         // True if just stopped touching
```

## Common Patterns

### Damage/Death Triggers

```rust
let spikes = ctx.world.spawn((
    Transform::new(300.0, 480.0),
    Collider::aabb(100.0, 20.0).as_sensor(),
    Hazard { damage: 1 },
));

for event in ctx.collisions_started() {
    if event.involves(self.player) {
        let other = event.other_entity(self.player);
        if let Some(hazard) = ctx.world.get_component::<Hazard>(other) {
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
    Collider::circle(12.0).as_sensor(),
    Coin { value: 10 },
));

let mut to_despawn = Vec::new();
for event in ctx.collisions_started() {
    if event.involves(self.player) {
        let other = event.other_entity(self.player);
        if let Some(coin) = ctx.world.get_component::<Coin>(other) {
            self.score += coin.value;
            to_despawn.push(other);
        }
    }
}
for entity in to_despawn {
    ctx.world.despawn(entity);
}
```

### Velocity Control

```rust
// Modify velocity directly for jumps, knockback, etc.
if let Some(mut vel) = ctx.world.get_component_mut::<Velocity>(entity) {
    vel.y = -300.0;  // Jump!
}
```

## Tips

- Use **sensors** for triggers, pickups, detection zones - they don't push objects
- Check **collisions_started()** for one-time events (damage, pickups)
- Check **collisions_stopped()** for exit events (leaving trigger zones)
- **Kinematic** bodies are great for moving platforms
- Despawn collected items in a separate loop to avoid iterator invalidation
