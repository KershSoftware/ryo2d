# Tiled Rendering

ryo2d provides a tiled rendering system for repeating sprites and composite multi-part tiles like pipes, walls, and platforms.

## Tiled Sprites - Simple Repeating

Repeat a single sprite in one direction to fill a target size:

```rust
use ryo2d::prelude::*;

// Create a vertical ladder by tiling a rung sprite downward
let tiled = Renderable::tiled(rung_sprite, TileDirection::Down, 200.0);

ctx.world.spawn((
    Transform::new(100.0, 50.0),
    tiled,
));
```

### Tile Directions

- `TileDirection::Up` - Tiles repeat upward (negative Y)
- `TileDirection::Down` - Tiles repeat downward (positive Y)
- `TileDirection::Left` - Tiles repeat leftward (negative X)
- `TileDirection::Right` - Tiles repeat rightward (positive X)

### Properties

```rust
let tiled = Renderable::tiled(sprite, TileDirection::Right, 300.0)
    .with_z_order(5)      // Render order
    .with_visible(true);  // Visibility toggle
```

## CompositeTile - Multi-Part Tiles

Combine multiple parts (static and/or tiled) into a single entity. Useful for complex structures like pipes with caps.

```rust
let composite = CompositeTile::new()
    .add_part(TilePart::static_part(cap_sprite, 0.0, 0.0))
    .add_part(TilePart::tiled_part(body_sprite, 0.0, 24.0, TileDirection::Down, 128.0))
    .with_z_order(5);

ctx.world.spawn((
    Transform::new(x, y),
    composite,
));
```

### TilePart Types

**Static Part** - Drawn once at a fixed offset:
```rust
TilePart::static_part(sprite, offset_x, offset_y)
```

**Tiled Part** - Repeated in a direction:
```rust
TilePart::tiled_part(sprite, offset_x, offset_y, direction, target_size)
```

### Sprite Flipping

Parts support horizontal and vertical flipping:

```rust
// Flip a cap sprite vertically (e.g., for a pipe opening facing down)
TilePart::static_part(cap_sprite, 0.0, 0.0)
    .with_flip_y(true)
```

## CompositeTileBuilder - Fluent API

The builder provides a convenient API for common patterns:

```rust
// Top pipe: body tiles down, cap at bottom flipped
let (top_pipe, top_collider) = CompositeTileBuilder::new()
    .body_tiled(body_sprite, 0.0, 0.0, TileDirection::Down, body_height)
    .cap_flipped_y(cap_sprite, 0.0, body_height)
    .z_order(5)
    .build_with_collider();

// Bottom pipe: cap at top (normal), body tiles down
let (bottom_pipe, bottom_collider) = CompositeTileBuilder::new()
    .cap(cap_sprite, 0.0, 0.0)
    .body_tiled(body_sprite, 0.0, cap_height, TileDirection::Down, body_height)
    .z_order(5)
    .build_with_collider();
```

### Builder Methods

| Method | Description |
|--------|-------------|
| `cap(sprite, x, y)` | Add a static part |
| `cap_flipped_y(sprite, x, y)` | Add a vertically flipped static part |
| `cap_flipped(sprite, x, y, flip_x, flip_y)` | Add a static part with custom flip |
| `body_tiled(sprite, x, y, dir, size)` | Add a tiled part |
| `body_tiled_flipped(sprite, x, y, dir, size, flip_x, flip_y)` | Add a tiled part with flip |
| `z_order(n)` | Set render order |
| `build()` | Build just the composite |
| `build_with_collider()` | Build composite + auto-sized collider |

## GroupedCollider - Physics for Composites

Instead of creating a collider for each part, use a single `GroupedCollider` that covers the entire composite:

```rust
// Auto-calculate from composite bounds
let collider = GroupedCollider::from_composite(&composite);

// Or manually specify dimensions
let collider = GroupedCollider::new(width, height)
    .with_offset(offset_x, offset_y)
    .with_friction(0.5)
    .with_restitution(0.0);
```

The `build_with_collider()` method automatically creates a properly-sized `GroupedCollider`.

### Physics Integration

Entities with `GroupedCollider` + `RigidBody` + `Transform` are automatically registered with the physics system:

```rust
ctx.world.spawn((
    Transform::new(x, y),
    RigidBody::kinematic(),  // or RigidBody::new() for dynamic
    composite,
    collider,
));
```

### Debug Visualization

`GroupedCollider` is supported by the debug collider visualization (`show_colliders = true` in Ryo.toml).

## Example: Flappy Bird Pipes

```rust
fn spawn_pipe_pair(&mut self, ctx: &mut SceneContext, x: f32, gap_y: f32) {
    let cap_height = 24.0;

    // Top pipe: hangs from ceiling
    let top_body_height = gap_y - PIPE_GAP / 2.0 - cap_height;
    let (top_composite, top_collider) = CompositeTileBuilder::new()
        .body_tiled(body_sprite.clone(), 0.0, 0.0, TileDirection::Down, top_body_height)
        .cap_flipped_y(cap_sprite.clone(), 0.0, top_body_height)
        .z_order(5)
        .build_with_collider();

    ctx.world.spawn((
        Transform::new(x, 0.0),
        RigidBody::kinematic(),
        top_composite,
        top_collider,
        Pipe::new_top(),
    ));

    // Bottom pipe: rises from ground
    let bottom_y = gap_y + PIPE_GAP / 2.0;
    let bottom_body_height = SCREEN_HEIGHT - bottom_y - cap_height;
    let (bottom_composite, bottom_collider) = CompositeTileBuilder::new()
        .cap(cap_sprite, 0.0, 0.0)
        .body_tiled(body_sprite, 0.0, cap_height, TileDirection::Down, bottom_body_height)
        .z_order(5)
        .build_with_collider();

    ctx.world.spawn((
        Transform::new(x, bottom_y),
        RigidBody::kinematic(),
        bottom_composite,
        bottom_collider,
        Pipe::new_bottom(),
    ));
}
```
