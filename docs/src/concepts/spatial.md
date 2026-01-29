# Spatial Partitioning

For games with many collidable entities, checking every entity against every other entity becomes expensive (O(n²) complexity). ryo2d provides **spatial partitioning** to dramatically reduce collision checks.

## The Problem

With 100 entities, brute-force collision detection requires 4,950 pair checks per frame. With 1,000 entities, that's 499,500 checks!

```
Entities:   10     100     1,000
Checks:     45   4,950   499,500  ← O(n²) growth
```

## The Solution: Spatial Hash

A **spatial hash** divides the world into a grid of cells. Each entity is placed into the cell(s) it occupies. To find potential collisions, we only check entities in the same or adjacent cells.

```
┌────┬────┬────┬────┐
│ A  │    │    │    │
├────┼────┼────┼────┤
│    │ B  │ C  │    │  ← Only B and C need checking
├────┼────┼────┼────┤
│    │    │ D  │    │  ← C and D also need checking
├────┼────┼────┼────┤
│    │    │    │ E  │  ← E is isolated, no checks needed
└────┴────┴────┴────┘
```

## Using PhysicsWorld

The `PhysicsWorld` struct handles all collision detection automatically:

```rust
use ryo2d::prelude::*;

struct MyGame {
    physics_world: PhysicsWorld,
}

impl Game for MyGame {
    fn init(&mut self, ctx: &mut GameContext) {
        // Create entities with colliders (as usual)
        let ball = ctx.world.spawn();
        ctx.world.add_component(ball, Transform::new(100.0, 100.0));
        ctx.world.add_component(ball, Collider::circle_collider(12.0));
        
        let wall = ctx.world.spawn();
        ctx.world.add_component(wall, Transform::new(0.0, 200.0));
        ctx.world.add_component(wall, Collider::box_collider(400.0, 32.0));
    }

    fn update(&mut self, ctx: &mut GameContext, _dt: f32) {
        // Step 1: Rebuild spatial hash from current positions
        self.physics_world.update(ctx.world);
        
        // Step 2: Get all collisions (broad + narrow phase combined)
        let collisions = self.physics_world.get_collisions(ctx.world);
        
        // Step 3: Handle each collision
        for collision in collisions {
            println!("Collision between {:?} and {:?}", 
                collision.entity_a, collision.entity_b);
            
            // Get minimum translation vector to separate
            let (mtv_x, mtv_y) = collision.mtv();
        }
    }
}
```

## Broad Phase vs Narrow Phase

Collision detection happens in two stages:

### Broad Phase (Fast, Approximate)
Uses the spatial hash to find **candidate pairs** - entities that *might* be colliding based on their cells.

### Narrow Phase (Accurate, Slower)
Uses actual shape intersection tests (AABB, Circle) on candidate pairs to confirm real collisions.

```
All Entities     Broad Phase      Narrow Phase
    ┌───┐         ┌───┐           ┌───┐
    │100│ ──────► │ 15│  ───────► │ 3 │  actual collisions
    └───┘ entities└───┘ candidates└───┘
```

`PhysicsWorld` handles both phases automatically.

## CollisionResult

Each detected collision provides:

| Property | Description |
|----------|-------------|
| `entity_a`, `entity_b` | The colliding entities |
| `penetration_x`, `penetration_y` | How far they overlap |
| `normal_x`, `normal_y` | Direction to separate (from A to B) |

```rust
for collision in self.physics_world.get_collisions(ctx.world) {
    // Check if a specific entity is involved
    if collision.involves(my_player) {
        // Get the other entity in the collision
        if let Some(other) = collision.other(my_player) {
            handle_player_hit(other);
        }
    }
    
    // Get separation vector (minimum translation to resolve)
    let (mtv_x, mtv_y) = collision.mtv();
}
```

## Direct SpatialHash Usage

For advanced scenarios, you can use `SpatialHash` directly:

```rust
use ryo2d::prelude::*;

let mut spatial = SpatialHash::new(64.0); // 64 pixel cells

// Insert by bounding box
spatial.insert(entity, min_x, min_y, max_x, max_y);

// Or insert using a collider
spatial.insert_collider(entity, &collider, pos_x, pos_y);

// Query a region
let nearby = spatial.query(100.0, 100.0, 200.0, 200.0);

// Get all collision candidate pairs
let pairs = spatial.get_collision_pairs();

// Remove an entity
spatial.remove(entity);

// Clear all
spatial.clear();
```

## Choosing Cell Size

The default cell size is 64 pixels. Choose a size close to your average entity size:

```rust
// For a game with small entities (bullets, particles)
PhysicsWorld::with_cell_size(32.0)

// For large entities (buildings, terrain chunks)
PhysicsWorld::with_cell_size(128.0)
```

| Cell Size | Good For |
|-----------|----------|
| 16-32 | Particles, bullets, small sprites |
| 64 (default) | Player-sized entities, most games |
| 128-256 | Large sprites, sparse worlds |

**Too small**: Many cells, entities span multiple cells, overhead increases  
**Too large**: Many entities per cell, defeats the purpose

## Performance Comparison

With spatial partitioning, collision detection scales much better:

```
Entities    Brute Force    Spatial Hash
    10           45            ~15
   100        4,950           ~150
 1,000      499,500         ~1,500
```

The improvement depends on entity distribution. Clustered entities still require more checks, but sparse worlds see dramatic gains.

## Future: Rapier2D Integration

For games needing advanced physics (joints, continuous collision, rigid body stacks), consider migrating to [Rapier2D](https://rapier.rs/):

```rust
// Future integration might look like:
use rapier2d::prelude::*;

// Rapier provides its own broad-phase (DBVT) and
// full rigid body simulation
let physics_pipeline = PhysicsPipeline::new();
```

ryo2d's simple physics are suitable for:
- Platformers
- Top-down games
- Breakout/Pong-style games
- Puzzle games

Consider Rapier2D for:
- Realistic physics simulations
- Complex stacking/piling
- Vehicles with wheels
- Rope/cloth physics

## See Also

- [Collision Detection](./collision.md) - Shapes and collision basics
- [Physics System](./physics.md) - Gravity, velocity, rigid bodies
- [ECS Architecture](./ecs.md) - Entity and component fundamentals
