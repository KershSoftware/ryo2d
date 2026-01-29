# ECS Architecture

ryo2d uses a simple **Entity-Component-System** (ECS) architecture for managing game objects.

## Core Concepts

### Entities

An **Entity** is just a unique ID (a number). It has no data or behavior on its own.

```rust
let player = ctx.world.spawn();  // Returns Entity
let enemy = ctx.world.spawn();   // Another unique Entity
```

### Components

**Components** are data attached to entities. ryo2d provides several built-in components:

| Component | Purpose |
|-----------|---------|
| `Transform` | Position (x, y), rotation, scale |
| `Velocity` | Movement speed (dx, dy) |
| `SpriteRenderer` | Visual representation, z-order |

```rust
// Attach components to an entity
ctx.world.add_component(player, Transform::new(100.0, 200.0));
ctx.world.add_component(player, Velocity::zero());
ctx.world.add_component(player, SpriteRenderer::new(sprite));
```

### Systems

**Systems** process entities with specific component combinations. The engine runs these automatically:

- **Movement System** - Applies `Velocity` to `Transform` each frame
- **Render System** - Draws entities with `Transform` + `SpriteRenderer`

## Querying Components

Get a component from an entity:

```rust
// Immutable access
if let Some(transform) = ctx.world.get_component::<Transform>(entity) {
    println!("Position: ({}, {})", transform.x, transform.y);
}

// Mutable access
if let Some(mut velocity) = ctx.world.get_component_mut::<Velocity>(entity) {
    velocity.dx = 100.0;
}
```

## Custom Components

Any type that is `'static + Send + Sync` can be a component:

```rust
struct Health {
    current: i32,
    max: i32,
}

ctx.world.add_component(player, Health { current: 100, max: 100 });
```
