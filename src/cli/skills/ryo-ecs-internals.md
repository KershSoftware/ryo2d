---
name: ryo-ecs-internals
description: Working on the hecs wrapper, adding components, and ECS patterns.
metadata:
  version: 1.0.0
---

# Ryo2D ECS Internals Specialist

You help developers work on the ECS layer.

## Resources

- **ECS Module**: `src/core/ecs/AGENTS.md`
- **World Wrapper**: `src/core/ecs/world.rs`
- **Components**: `src/core/ecs/components.rs`

## Module Structure

```
src/core/ecs/
├── mod.rs          # Public exports
├── active.rs       # Active/Inactive marker components for pooling
├── component.rs    # Component trait
├── components.rs   # Built-in components (Transform, PreviousTransform, Velocity)
├── hierarchy.rs    # Parent/child relationships
├── knockback.rs    # Knockback component (decay-based force)
├── squash_stretch.rs # SquashStretch component (impact deformation)
├── pool.rs         # Object pooling system
├── renderable.rs   # Renderable component
├── sprite_effects.rs # Per-sprite SpriteEffect
├── material.rs     # SpriteShader / Material
├── y_sort.rs       # YSort component
├── tween.rs        # Tweening system (Tween, TweenSequence)
├── property_tween.rs # Generic property tweening (PropertyTween<T>)
├── lerp.rs         # Lerp trait for generic interpolation
└── world.rs        # World wrapper
```

## Why hecs?

- Generational entity IDs (u64 with index + generation)
- Archetype storage (cache-friendly)
- Battle-tested, no proc macros

## World Wrapper

```rust
pub struct World {
    inner: hecs::World,
}

impl World {
    pub fn spawn<T: hecs::DynamicBundle>(&mut self, components: T) -> Entity;
    pub fn despawn(&mut self, entity: Entity) -> Result<(), hecs::NoSuchEntity>;
    pub fn get<T: hecs::Component>(&self, entity: Entity) -> Option<hecs::Ref<'_, T>>;
    pub fn get_mut<T: hecs::Component>(&self, entity: Entity) -> Option<hecs::RefMut<'_, T>>;
    // get_component / get_component_mut: deprecated aliases of get / get_mut
    pub fn add_component<T: hecs::Component>(&mut self, entity: Entity, component: T);
    pub fn has_component<T: hecs::Component>(&self, entity: Entity) -> bool;
    pub fn query<Q: hecs::Query>(&self) -> hecs::QueryBorrow<'_, Q>;
    pub fn query_mut<Q: hecs::Query>(&mut self) -> hecs::QueryMut<'_, Q>;
    pub fn entities_with<T: hecs::Component>(&self) -> Vec<Entity>;
    pub fn clear(&mut self);          // despawns everything, bumps generation()
}

// get/get_mut/has/insert_one/remove_one are the short forms of the *_component methods.
```

## Core Components

```rust
pub struct Transform { pub x: f32, pub y: f32, pub z: f32, pub rotation: f32, pub scale_x: f32, pub scale_y: f32, /* + pivot_x/y, offset_x/y */ }
pub struct Velocity { pub dx: f32, pub dy: f32, pub dz: f32 }
pub struct Renderable { pub sprite: SpriteHandle, pub visible: bool, pub z_order: i32, pub flip_x: bool, pub flip_y: bool, pub alpha: f32, pub tint: Color, /* + style, mode, shader, shadows */ }
pub struct Parent(pub Entity);
pub struct Children(pub Vec<Entity>);
```

## Query Patterns

hecs 0.11: iterators yield only the query item. Put `Entity` in the tuple to get IDs.

```rust
// Immutable (query() returns a borrow - call .iter())
for (entity, transform, velocity) in world.query::<(Entity, &Transform, &Velocity)>().iter() {
    // read-only access
}

// Mutable (query_mut() returns the iterator directly)
for (transform, velocity) in world.query_mut::<(&mut Transform, &Velocity)>() {
    transform.x += velocity.dx * dt;
}

// Optional components
for (transform, maybe_sprite) in world.query::<(&Transform, Option<&Renderable>)>().iter() {
    if let Some(sprite) = maybe_sprite { /* ... */ }
}
```

## Adding Components

1. Define in `components.rs`:
```rust
#[derive(Debug, Clone)]
pub struct Health {
    pub current: f32,
    pub max: f32,
}
```

2. Export in `ecs/mod.rs`
3. Add to prelude in `lib.rs` if public

Components must be `Send + Sync + 'static`.

## Engine Integration

Systems run each frame:

```rust
// Update animators (skipping Inactive pooled entities)
for entity in world.entities_with::<Animator>() {
    if world.has::<Inactive>(entity) { continue; }
    if let Some(mut animator) = world.get_mut::<Animator>(entity) {
        animator.update(dt);
    }
}

// Apply velocities
for (transform, velocity) in world.query_mut::<(&mut Transform, &Velocity)>() {
    transform.x += velocity.dx * dt;
}
```

## Performance

- Entities with same components share an archetype
- Iteration is contiguous in memory
- Use batch queries over individual `get()` calls

```rust
// Good - single query
for (t, v) in world.query_mut::<(&mut Transform, &Velocity)>() {
    t.x += v.dx;
}

// Avoid - per-entity lookups
for entity in world.entities_with::<Velocity>() {   // allocates a Vec
    if let Some(mut t) = world.get_mut::<Transform>(entity) { /* ... */ }
}
```

## Tips

- Use `spawn()` with component tuples
- Prefer `query_mut()` for batch processing
- Generational entities prevent use-after-free
- Keep components small (data only)
- Despawn entities after iteration (collect IDs first, then despawn)

## Object Pooling System

The pooling system (`pool.rs` and `active.rs`) provides pre-allocated entity pools for high-frequency spawning.

### Active/Inactive Markers

```rust
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Active;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Inactive;
```

Engine systems automatically skip `Inactive` entities in:
- CPU rendering
- Physics registration/simulation
- Animation systems (Animator, AnimationController)
- Tween updates

### Pool Implementation

```rust
pub struct Pool<B: DynamicBundle + Clone> {
    available: Vec<Entity>,           // Ready for reuse
    active: Vec<Entity>,              // Currently in use
    config: PoolConfig,
    factory: Box<dyn Fn() -> B + Send + Sync>,  // Creates component bundles
    world_generation: u64,            // For scene-change detection
    stats: PoolStats,
}

impl<B: DynamicBundle + Clone + Send + Sync + 'static> Pool<B> {
    fn new<F: Fn() -> B + Send + Sync + 'static>(world: &mut World, config: PoolConfig, factory: F) -> Self;
    fn acquire(&mut self, world: &mut World) -> Option<Entity>;
    fn release(&mut self, world: &mut World, entity: Entity) -> bool;
    fn release_all(&mut self, world: &mut World);
    fn active_iter(&self) -> impl Iterator<Item = &Entity>;
    fn stats(&self) -> &PoolStats;
}

// PoolConfig::default().with_capacity(n).with_max_capacity(n)
//     .with_expand_batch_size(n).with_exhausted_policy(policy)
```

**Key implementation detail:** When spawning entities for the pool, we spawn the user's bundle first, then add `Inactive` separately with `insert_one()`. This avoids nested tuple issues with hecs's `DynamicBundle`:

```rust
// Correct - spawn bundle, then add Inactive
let entity = world.spawn(bundle);
let _ = world.insert_one(entity, Inactive);

// Wrong - nested tuple ((Transform, ...), Inactive)
let entity = world.spawn((bundle, Inactive));
```

### World Generation Tracking

Pools track `world.generation()` to detect scene clears:

```rust
fn check_generation(&mut self, world: &World) {
    if world.generation() != self.world_generation {
        // World was cleared - invalidate pool
        self.available.clear();
        self.active.clear();
        self.world_generation = world.generation();
    }
}
```

### Exhaustion Policies

```rust
pub enum PoolExhaustedPolicy {
    Expand,         // Spawn more (default)
    ReturnNone,     // Return None
    RecycleOldest,  // Force-release oldest active
}
```

### Stats Tracking

```rust
pub struct PoolStats {
    pub total_spawned: usize,
    pub current_available: usize,
    pub current_active: usize,
    pub peak_active: usize,
    pub acquires: usize,
    pub releases: usize,
    pub expansions: usize,
}

