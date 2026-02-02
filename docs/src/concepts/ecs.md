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
| `Transform` | Position (x, y, z), rotation, scale |
| `Velocity` | Movement speed (dx, dy, dz) |
| `Renderable` | Visual representation with sprite, z-order, and optional shader |

```rust
// Spawn an entity with components
let player = ctx.world.spawn((
    Transform::new(100.0, 200.0),
    Velocity::zero(),
    Renderable::sprite(sprite_handle),
));
```

### Systems

**Systems** process entities with specific component combinations. The engine runs these automatically:

- **Movement System** - Applies `Velocity` to `Transform` each frame
- **Render System** - Draws entities with `Transform` + `Renderable`

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

## Transform Pivot Points

By default, sprites rotate and scale around their center. You can change this with **pivot points**:

```rust
// Rotate around top-center (for pendulums, hanging objects)
Transform::new(x, y)
    .with_pivot_top_center()
    .with_rotation(angle)

// Rotate around left edge (for door hinges)
Transform::new(x, y)
    .with_pivot(0.0, 0.5)
    .with_rotation(angle)

// Custom pivot (normalized 0.0-1.0)
Transform::new(x, y)
    .with_pivot(0.25, 0.75)
```

**Common pivot values:**

| Pivot | Value | Use Case |
|-------|-------|----------|
| Center | `(0.5, 0.5)` | Default - spin around center |
| Top-center | `(0.5, 0.0)` | Pendulums, hanging lamps |
| Bottom-center | `(0.5, 1.0)` | Trees swaying, standing objects |
| Left-center | `(0.0, 0.5)` | Door hinges, book pages |
| Top-left | `(0.0, 0.0)` | Traditional top-left origin |
