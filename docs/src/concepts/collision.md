# Collision Detection

ryo2d provides two collision systems:

1. **Rapier2D Physics Collisions** — Automatic collision response and events for physics bodies
2. **Manual Collision Queries** — For game logic, triggers, and non-physics interactions

This page focuses on the **manual collision system**. For physics-based collisions with automatic response, see [Physics System](./physics.md).

## When to Use Which

| Use Case | System |
|----------|--------|
| Bouncing ball, falling objects | Rapier (automatic) |
| Platformer ground detection | Rapier collision events |
| Bullet hitting enemy (damage) | Rapier events + game logic |
| Mouse click on entity | Manual collision |
| Custom trigger zones | Manual collision or Rapier sensors |

## Collision Shapes

### AABB (Axis-Aligned Bounding Box)

A rectangle aligned to the screen axes. Fast and simple, ideal for most 2D games.

```rust
use ryo2d::prelude::*;

// Create from position and size
let box1 = AABB::from_position_size(100.0, 50.0, 64.0, 32.0);

// Create centered at a point
let box2 = AABB::centered(200.0, 100.0, 48.0, 48.0);

// Check intersection
if box1.intersects(&box2) {
    println!("Boxes are overlapping!");
}

// Get overlap amount
if let Some((overlap_x, overlap_y)) = box1.overlap(&box2) {
    println!("Overlap: ({}, {})", overlap_x, overlap_y);
}
```

### Circle

Perfect for balls, projectiles, and characters with rounded hitboxes.

```rust
let circle1 = Circle::new(100.0, 100.0, 16.0); // x, y, radius
let circle2 = Circle::new(120.0, 100.0, 16.0);

// Circle vs Circle
if circle1.intersects_circle(&circle2) {
    println!("Circles colliding!");
}

// Circle vs AABB
let wall = AABB::from_position_size(0.0, 0.0, 800.0, 32.0);
if circle1.intersects_aabb(&wall) {
    println!("Circle hit the wall!");
}
```

## The Collider Component

Attach colliders to entities for automatic collision detection:

```rust
// Box collider
ctx.world.add_component(
    wall_entity,
    Collider::box_collider(64.0, 32.0)
);

// Circle collider
ctx.world.add_component(
    ball_entity,
    Collider::circle_collider(12.0)
);
```

### Collider Properties

| Property | Description | Default |
|----------|-------------|---------|
| `offset_x`, `offset_y` | Offset from entity's Transform | 0.0, 0.0 |
| `is_trigger` | If true, detects overlap but doesn't respond physically | false |
| `layer` | Which layer this collider belongs to | 1 |
| `mask` | Which layers this collider can collide with | all (u32::MAX) |

```rust
// Collider with offset (useful when sprite origin isn't top-left)
Collider::box_collider(32.0, 32.0)
    .with_offset(-16.0, -16.0)  // Center the hitbox

// Trigger zone (detects overlaps, no physics response)
Collider::circle_collider(100.0)
    .as_trigger()

// Layer filtering (see below)
Collider::box_collider(16.0, 16.0)
    .with_layer(2)
    .with_mask(0b0101)
```

## Layer Filtering

Use layers to control which objects can collide with each other.

### Layer Bits

Each collider has a `layer` (what it *is*) and a `mask` (what it *collides with*):

```rust
const LAYER_PLAYER: u32 = 1;      // 0b0001
const LAYER_ENEMY: u32 = 2;       // 0b0010
const LAYER_PROJECTILE: u32 = 4;  // 0b0100
const LAYER_WALL: u32 = 8;        // 0b1000

// Player collides with enemies and walls
Collider::box_collider(32.0, 48.0)
    .with_layer(LAYER_PLAYER)
    .with_mask(LAYER_ENEMY | LAYER_WALL)

// Enemy projectile only hits player
Collider::circle_collider(8.0)
    .with_layer(LAYER_PROJECTILE)
    .with_mask(LAYER_PLAYER)

// Wall collides with everything
Collider::box_collider(64.0, 64.0)
    .with_layer(LAYER_WALL)
    .with_mask(u32::MAX)
```

### Checking Layer Compatibility

```rust
if collider_a.should_collide_with(&collider_b) {
    // These colliders can interact
}
```

## Detecting Collisions

Use `check_collision` to test if two colliders overlap:

```rust
use ryo2d::prelude::*;

fn update(&mut self, ctx: &mut GameContext) {
    // Get positions and colliders
    let player_pos = ctx.world.get_component::<Transform>(self.player);
    let player_col = ctx.world.get_component::<Collider>(self.player);
    
    let enemy_pos = ctx.world.get_component::<Transform>(self.enemy);
    let enemy_col = ctx.world.get_component::<Collider>(self.enemy);
    
    if let (Some(p_pos), Some(p_col), Some(e_pos), Some(e_col)) = 
        (player_pos, player_col, enemy_pos, enemy_col) 
    {
        let p_xy = (p_pos.x, p_pos.y);
        let e_xy = (e_pos.x, e_pos.y);
        
        if let Some(collision) = check_collision(&p_col, p_xy, &e_col, e_xy) {
            // collision.penetration_x, penetration_y - overlap amounts
            // collision.normal_x, normal_y - direction to separate
            self.handle_enemy_hit();
        }
    }
}
```

## Collision Response

The `Collision` struct provides information for resolving overlaps:

```rust
if let Some(collision) = check_collision(&a_col, a_pos, &b_col, b_pos) {
    // Get the minimum translation vector to separate entities
    let (mtv_x, mtv_y) = collision.mtv();
    
    // Move entity A out of entity B
    if let Some(mut transform) = ctx.world.get_component_mut::<Transform>(entity_a) {
        transform.x += mtv_x;
        transform.y += mtv_y;
    }
}
```

### Bounce Response

For bouncing objects, reverse velocity based on collision normal:

```rust
if let Some(collision) = check_collision(&ball_col, ball_pos, &wall_col, wall_pos) {
    if let Some(mut vel) = ctx.world.get_component_mut::<Velocity>(ball) {
        // Determine which axis has less penetration (that's the surface we hit)
        if collision.penetration_x.abs() < collision.penetration_y.abs() {
            vel.dx = -vel.dx * 0.8; // Bounce horizontally, lose some energy
        } else {
            vel.dy = -vel.dy * 0.8; // Bounce vertically
        }
    }
}
```

## Complete Example: Ball vs Walls

```rust
struct BouncingBall {
    ball: Entity,
    walls: Vec<Entity>,
}

impl Game for BouncingBall {
    fn init(&mut self, ctx: &mut GameContext) {
        // Create ball with physics
        self.ball = ctx.world.spawn();
        ctx.world.add_component(self.ball, Transform::new(100.0, 100.0));
        ctx.world.add_component(self.ball, Velocity { dx: 150.0, dy: 200.0 });
        ctx.world.add_component(self.ball, Collider::circle_collider(12.0));
        ctx.world.add_component(self.ball, RigidBody::new());
        
        // Create walls (floor, ceiling, left, right)
        let wall_positions = [
            (0.0, 580.0, 800.0, 20.0),   // Floor
            (0.0, 0.0, 800.0, 20.0),     // Ceiling
            (0.0, 0.0, 20.0, 600.0),     // Left
            (780.0, 0.0, 20.0, 600.0),   // Right
        ];
        
        for (x, y, w, h) in wall_positions {
            let wall = ctx.world.spawn();
            ctx.world.add_component(wall, Transform::new(x, y));
            ctx.world.add_component(wall, Collider::box_collider(w, h));
            ctx.world.add_component(wall, RigidBody::new().kinematic());
            self.walls.push(wall);
        }
    }

    fn update(&mut self, ctx: &mut GameContext) {
        // Get ball data
        let ball_pos = ctx.world.get_component::<Transform>(self.ball)
            .map(|t| (t.x, t.y));
        let ball_col = ctx.world.get_component::<Collider>(self.ball);
        let ball_vel = ctx.world.get_component::<Velocity>(self.ball)
            .map(|v| (v.dx, v.dy));
        
        if let (Some(b_pos), Some(b_col), Some(b_vel)) = (ball_pos, ball_col, ball_vel) {
            for wall in &self.walls {
                let wall_pos = ctx.world.get_component::<Transform>(*wall)
                    .map(|t| (t.x, t.y));
                let wall_col = ctx.world.get_component::<Collider>(*wall);
                
                if let (Some(w_pos), Some(w_col)) = (wall_pos, wall_col) {
                    if let Some(collision) = check_collision(&b_col, b_pos, &w_col, w_pos) {
                        // Bounce response
                        if let Some(mut vel) = ctx.world.get_component_mut::<Velocity>(self.ball) {
                            if collision.penetration_x.abs() < collision.penetration_y.abs() {
                                vel.dx = -vel.dx;
                            } else {
                                vel.dy = -vel.dy;
                            }
                        }
                        
                        // Separate ball from wall
                        let (mtv_x, mtv_y) = collision.mtv();
                        if let Some(mut t) = ctx.world.get_component_mut::<Transform>(self.ball) {
                            t.x += mtv_x;
                            t.y += mtv_y;
                        }
                    }
                }
            }
        }
    }
}
```

## Performance Tips

1. **Use layer filtering** to avoid unnecessary collision checks
2. **Check velocity direction** before responding (prevents double-bounces)
3. **AABB is faster than Circle** for most cases - use it when possible
4. **Spatial partitioning** (coming in Phase 3.3) will optimize many-entity scenarios

## See Also

- [ECS Architecture](./ecs.md) - Entity and component basics
- [Physics System](./physics.md) - Gravity, velocity, and rigid bodies
- [API Reference](../api/ryo2d/index.html) - Full collision API
