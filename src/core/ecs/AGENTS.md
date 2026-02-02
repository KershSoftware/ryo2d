# AGENTS.md - ECS Module

## Overview

This folder implements the Entity-Component-System (ECS) layer using the **hecs** crate. We wrap hecs with a compatibility API that provides a cleaner interface while leveraging hecs's high-performance archetype storage and built-in generational entity IDs.

## File Structure

| File | Purpose |
|------|---------|
| `mod.rs` | Public exports (`Entity` from hecs, `World` wrapper) |
| `component.rs` | `Component` trait with blanket impl |
| `components.rs` | Built-in components: `Transform`, `Velocity` |
| `hierarchy.rs` | Parent/child relationships, `WorldTransform` |
| `renderable.rs` | `Renderable` component for sprites |
| `world.rs` | `World` wrapper around `hecs::World` with compatibility API |

## Key Design Decisions

### hecs as Backend

We use [hecs](https://docs.rs/hecs) for ECS internals because:
- **Generational IDs** - Built into `hecs::Entity` (u64 with index + generation)
- **Archetype storage** - Cache-friendly iteration over entities
- **Battle-tested** - Used in production by many projects
- **No runtime dependencies** - Pure Rust, no proc macros required

### Entity Type

```rust
pub use hecs::Entity;  // Re-exported from hecs
```

`hecs::Entity` is a lightweight handle (u64) with built-in generation counter. Stale handles safely return `None` on access.

### World Wrapper

Our `World` wraps `hecs::World` to provide:
- Familiar API methods (`spawn`, `despawn`, `add_component`, `get_component`)
- Helper iterators (`entities_with`, `entities_with_two`, `for_each_mut`)
- Direct query access when needed (`query`, `query_mut`)

## Usage Patterns

### Spawning Entities

**With components (preferred):**
```rust
let entity = world.spawn((
    Transform::new(100.0, 200.0),
    Velocity::new(5.0, 0.0),
    SpriteRenderer::new(sprite_handle),
));
```

**Empty entity (use sparingly):**
```rust
let entity = world.spawn(());
world.add_component(entity, Transform::new(0.0, 0.0));
```

### Accessing Components

```rust
// Single component (returns Option<hecs::Ref<T>>)
if let Some(transform) = world.get_component::<Transform>(entity) {
    println!("Position: {:?}", transform.position);
}

// Mutable access (returns Option<hecs::RefMut<T>>)
if let Some(mut vel) = world.get_component_mut::<Velocity>(entity) {
    vel.x += 1.0;
}
```

### Iterating Entities

**All entities with a component:**
```rust
for entity in world.entities_with::<Transform>() {
    // entity is hecs::Entity
}
```

**Mutable iteration with closure:**
```rust
world.for_each_mut::<Velocity>(|entity, velocity| {
    velocity.x *= 0.99;  // Apply drag
});
```

**Direct hecs queries (advanced):**
```rust
// Immutable query
for (entity, (transform, velocity)) in world.query::<(&Transform, &Velocity)>().iter() {
    println!("{:?} at {:?}", entity, transform.position);
}

// Mutable query
for (entity, (transform, mut velocity)) in world.query_mut::<(&Transform, &mut Velocity)>() {
    velocity.y += 9.8 * dt;  // Apply gravity
}
```

### Generational Safety

Entity handles are generationally safe:
```rust
let entity = world.spawn((Transform::default(),));
world.despawn(entity);
let new_entity = world.spawn((Velocity::default(),));  // May reuse same index

// Old handle is invalid - returns None, not wrong entity's data
assert!(world.get_component::<Transform>(entity).is_none());
```

## Adding New Built-in Components

1. Add the struct to `components.rs`
2. Derive/implement `Debug`, `Clone` as appropriate
3. Add to `mod.rs` re-exports
4. Add to `lib.rs` prelude if it should be public

## Transform Pivot Points

`Transform` includes `pivot_x` and `pivot_y` fields (normalized 0.0-1.0) that define the rotation/scale origin:

| Pivot | Value | Use Case |
|-------|-------|----------|
| Center | `(0.5, 0.5)` | Default - rotate around center |
| Top-center | `(0.5, 0.0)` | Pendulums, hanging objects |
| Bottom-center | `(0.5, 1.0)` | Standing objects, trees |
| Left-center | `(0.0, 0.5)` | Door hinges |
| Top-left | `(0.0, 0.0)` | Traditional top-left origin |

**Builder methods:**
```rust
// Custom pivot
Transform::new(100.0, 200.0).with_pivot(0.5, 0.0)

// Convenience methods
Transform::new(100.0, 200.0).with_pivot_top_center()
Transform::new(100.0, 200.0).with_pivot_bottom_center()
```

**How it works:** When rendering, the engine calculates the offset from the default center (0.5, 0.5) to the custom pivot, rotates that offset by the transform's rotation, and adjusts the draw position so the pivot point stays fixed at the transform's (x, y) position.

## hecs Query Patterns

When using direct queries, remember:
- `&T` for read access, `&mut T` for write access
- Include `Entity` in the query tuple to get entity IDs: `query::<(Entity, &T)>()`
- `query()` returns a `QueryBorrow` - call `.iter()` to iterate
- `query_mut()` returns an iterator directly

## Performance Notes

- **Archetype storage**: Entities with the same component set are stored together
- **Cache-friendly**: Iteration is fast due to contiguous memory layout
- **Spawn overhead**: First spawn with a new component combination creates an archetype
- **Query overhead**: Queries are O(archetypes) to find matches, then O(entities) to iterate
