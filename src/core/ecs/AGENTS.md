# AGENTS.md - ECS Module

## Overview

This folder implements the Entity-Component-System (ECS) layer using the **hecs** crate. We wrap hecs with a compatibility API that provides a cleaner interface while leveraging hecs's high-performance archetype storage and built-in generational entity IDs.

## File Structure

| File | Purpose |
|------|---------|
| `mod.rs` | Public exports (`Entity` from hecs, `World` wrapper) |
| `active.rs` | `Active`/`Inactive` marker components for object pooling |
| `component.rs` | `Component` trait with blanket impl |
| `components.rs` | Built-in components: `Transform`, `Velocity` |
| `hierarchy.rs` | Parent/child relationships, `WorldTransform` |
| `pool.rs` | Object pooling: `Pool`, `PoolConfig`, `PoolExhaustedPolicy` |
| `renderable.rs` | `Renderable` component for sprites |
| `tween.rs` | Tweening system: `Tween`, `TweenSequence`, `Ease`, `TweenLoop` |
| `property_tween.rs` | Generic property tweening: `PropertyTween<T>` |
| `lerp.rs` | `Lerp` trait with impls for f32, Vec2, Color, Rect |
| `knockback.rs` | Knockback system: decay-based directional force |
| `squash_stretch.rs` | Squash/stretch system: impact deformation effects |
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

## Tweening System

The tweening system (`tween.rs`) provides smooth interpolation of Transform properties over time.

### Ease - Easing Functions

24 standard easing functions for controlling interpolation curves:

| Category | Functions |
|----------|-----------|
| Linear | `Linear` |
| Quad | `InQuad`, `OutQuad`, `InOutQuad` |
| Cubic | `InCubic`, `OutCubic`, `InOutCubic` |
| Quart | `InQuart`, `OutQuart`, `InOutQuart` |
| Sine | `InSine`, `OutSine`, `InOutSine` |
| Expo | `InExpo`, `OutExpo`, `InOutExpo` |
| Back | `InBack`, `OutBack`, `InOutBack` (overshoot) |
| Elastic | `InElastic`, `OutElastic`, `InOutElastic` |
| Bounce | `InBounce`, `OutBounce`, `InOutBounce` |

### TweenLoop - Loop Modes

- `Once` - Play once and stop (default)
- `Loop` - Repeat forever
- `PingPong` - Forward then backward, repeat
- `Times(n)` - Repeat n times

### Tween - Component

Attach to entities with `Transform` to animate properties:

```rust
// Move to position with easing
world.spawn((
    Transform::new(100.0, 100.0),
    Tween::to_position(400.0, 300.0, 2.0)
        .with_easing(Ease::OutBounce),
));

// Pulsing scale
world.spawn((
    Transform::new(200.0, 200.0),
    Tween::to_scale(1.2, 1.2, 0.3)
        .with_easing(Ease::InOutSine)
        .with_loop(TweenLoop::PingPong),
));

// Combined properties with delay
world.spawn((
    Transform::new(0.0, 0.0),
    Tween::new(1.5)
        .to_x(200.0)
        .to_y(100.0)
        .to_rotation_value(PI)
        .with_easing(Ease::OutCubic)
        .with_delay(0.5),
));
```

**Engine Integration:** The engine updates all `Tween` components automatically after the animation system, applying interpolated values to the entity's `Transform`.

### TweenCallback

Callbacks receive eased progress (0.0–1.0):

```rust
pub type TweenCallback = Arc<dyn Fn(f32) + Send + Sync>;
```

All three callbacks (`on_start`, `on_update`, `on_complete`) receive the eased progress value. `on_start` receives `0.0`, `on_complete` receives `1.0`.

```rust
Tween::to_position(100.0, 100.0, 1.0)
    .on_start(|_progress| println!("started"))
    .on_update(|progress| println!("progress: {progress}"))
    .on_complete(|_| println!("done"));
```

## Lerp Trait

The `Lerp` trait (`lerp.rs`) provides generic interpolation used by `PropertyTween<T>` and `UiTween<T>`:

```rust
pub trait Lerp: Clone + Send + Sync + 'static {
    fn lerp(start: &Self, end: &Self, t: f32) -> Self;
}
```

**Built-in impls:** `f32`, `(f32, f32)`, `Vec2`, `Color`, `Rect`

Users can implement `Lerp` for custom types to use with `PropertyTween<T>`.

## PropertyTween\<T\> - Generic Value Tweening

The `PropertyTween<T>` component (`property_tween.rs`) interpolates any `Lerp`-implementing type between start and end values. Unlike `Tween` (which auto-applies to `Transform`), `PropertyTween<T>` is a **value tween** — the user reads `.value()` each frame and applies it.

### Construction

```rust
// Tween a float from 0 to 100 over 1 second
PropertyTween::new(0.0f32, 100.0, 1.0)

// Tween a Vec2
PropertyTween::new(Vec2::ZERO, Vec2::new(10.0, 5.0), 2.0)

// Tween a Color
PropertyTween::new(Color::RED, Color::BLUE, 0.5)
```

### Builder API (mirrors Tween)

```rust
PropertyTween::new(0.0f32, 100.0, 1.0)
    .with_easing(Ease::OutQuad)
    .with_delay(0.5)
    .with_loop(TweenLoop::PingPong)
    .with_speed(2.0)
    .on_start(|_| println!("started"))
    .on_update(|progress| println!("{progress}"))
    .on_complete(|_| println!("done"))
```

### Reading Values

```rust
if let Some(tween) = ctx.world.get_component::<PropertyTween<f32>>(entity) {
    let current = tween.value();    // Current interpolated value
    let progress = tween.progress(); // Raw 0.0-1.0 progress
}
```

### Runtime Control

```rust
.pause(), .resume(), .stop(), .restart()
.retarget(new_end)  // Change destination mid-flight
```

### Engine Integration

The `PropertyTweenSystem` automatically drives `PropertyTween<f32>`, `PropertyTween<Vec2>`, and `PropertyTween<Color>` each frame. It emits `TweenStarted` and `TweenCompleted` events (same types as `Tween`). For custom `Lerp` types, call `tween.update(dt)` manually.

## Knockback System

The knockback system (`knockback.rs`) applies decaying directional force to entities, useful for hit reactions, explosions, and physics effects.

### Knockback - Component

Attach to entities to apply directional force that decays over time:

```rust
// Simple knockback - pushes entity right at 300 px/sec
world.add_component(entity, Knockback::new(Vec2::new(1.0, 0.0), 300.0));

// From angle (radians) - explosion pushback
world.add_component(entity, Knockback::from_angle(angle_to_player, 500.0));

// Custom decay rate (default 4.0 = ~0.25s full decay)
world.add_component(entity,
    Knockback::new(direction, 400.0)
        .with_decay_rate(8.0)  // Fast decay
);

// Non-accumulating (replaces existing knockback)
world.add_component(entity,
    Knockback::new(direction, 400.0)
        .with_accumulate(false)
);
```

### Decay Algorithm

Knockback uses strength-based decay with squared intensity for a punchy feel:

```rust
// Each frame:
self.strength = (self.strength - self.decay_rate * dt).max(0.0);
let intensity = self.strength * self.strength;  // Squared curve
let velocity = (direction_x * force * intensity, direction_y * force * intensity);
```

- `strength` starts at 1.0 and decays to 0.0
- `decay_rate` controls how fast (4.0 = ~0.25s, 8.0 = ~0.125s, 2.0 = ~0.5s)
- Squared intensity gives strong initial push that tapers off smoothly

### Accumulation Behavior

When `accumulate` is true (default), applying knockback to an entity with existing knockback:
1. **Blends directions** - Weighted average based on force magnitudes
2. **Adds forces** - New force adds to existing
3. **Resets strength** - Back to 1.0 for full effect

When `accumulate` is false, new knockback completely replaces existing.

### Convenience Methods

Use context methods for common patterns:

```rust
// Basic knockback (accumulates, default decay 4.0)
ctx.knockback(entity, Vec2::new(1.0, 0.0), 300.0);

// With custom decay rate
ctx.knockback_with_decay(entity, direction, 500.0, 8.0);
```

### Events

The system emits events for feedback:

| Event | When |
|-------|------|
| `KnockbackStarted { entity }` | Knockback becomes active |
| `KnockbackFinished { entity }` | Knockback strength reaches zero |

```rust
// Flash entity on knockback hit
for event in ctx.events.read::<KnockbackStarted>() {
    if let Some(mut renderable) = ctx.world.get_component_mut::<Renderable>(event.entity) {
        renderable.tint = Color::WHITE;  // Flash white
    }
}
```

### Engine Integration

The engine updates all `Knockback` components automatically, applying displacement directly to the entity's `Transform`. Works with both simple entities (Transform only) and physics entities (with Velocity component).

## Squash/Stretch System

The squash/stretch system (`squash_stretch.rs`) applies scale deformation that recovers over time, perfect for impact effects, landing animations, and bouncy feel.

### SquashStretch - Component

Attach to entities to apply scale deformation on impact:

```rust
// Quick squash on landing (compresses Y, stretches X)
ctx.squash(entity, 0.3);  // 30% deformation

// Horizontal squash for wall hits
ctx.squash_x(entity, 0.25);

// Custom decay rate
ctx.squash_with_decay(entity, 0.4, 12.0);  // Very snappy recovery

// Full control via component
world.add_component(entity,
    SquashStretch::new(0.3)
        .with_axis(SquashAxis::Y)
        .with_decay_rate(8.0)
        .with_preserve_volume(true)
);
```

### Volume Preservation

When `preserve_volume` is true (default), squashing one axis stretches the other:
- Squash Y (landing): `scale_y = 1.0 - intensity`, `scale_x = 1.0 + intensity`
- Squash X (wall hit): `scale_x = 1.0 - intensity`, `scale_y = 1.0 + intensity`

### Decay Rates

| Rate | Feel | Time to Recover |
|------|------|-----------------|
| 12.0 | Very snappy | ~0.08s |
| 8.0 | Snappy (default) | ~0.125s |
| 4.0 | Moderate | ~0.25s |
| 2.0 | Slow/cartoon | ~0.5s |

### Events

| Event | When |
|-------|------|
| `SquashStretchStarted { entity }` | Effect becomes active |
| `SquashStretchFinished { entity }` | Entity returns to base scale |

### Engine Integration

The engine updates all `SquashStretch` components automatically, applying scale changes directly to the entity's `Transform`. Base scale is captured on first update.

## Object Pooling System

The pooling system (`pool.rs` and `active.rs`) provides pre-allocated entity pools for high-frequency spawning scenarios like bullets, particles, and enemies.

### Why Object Pooling?

Allocating/deallocating entities rapidly (100+ bullets per second) can cause:
- Memory allocation overhead
- GC pressure in some scenarios
- Frame rate stutters during heavy spawning

Object pools pre-allocate entities at load time and recycle them during gameplay.

### Active/Inactive Markers

Two marker components control entity visibility:

| Component | Purpose |
|-----------|---------|
| `Active` | Entity is processed (rendered, physics, animations) |
| `Inactive` | Entity is pooled/dormant (skipped by all systems) |

The engine automatically skips `Inactive` entities in:
- Rendering (CPU and GPU paths)
- Physics registration and simulation
- Animation systems (Animator, AnimationController)
- Tween updates

### Pool Configuration

```rust
PoolConfig::default()
    .with_capacity(100)          // Pre-spawn 100 entities
    .with_max_capacity(500)      // Never exceed 500
    .with_expand_batch_size(25)  // Spawn 25 when depleted
    .with_exhausted_policy(PoolExhaustedPolicy::Expand)
```

**Exhaustion Policies:**
- `Expand` - Spawn more entities (default)
- `ReturnNone` - Return `None` from `acquire()`
- `RecycleOldest` - Force-release oldest active entity

### Pool Usage

```rust
// In scene struct
struct GameScene {
    bullet_pool: Pool<(Transform, Renderable, Collider, Bullet)>,
}

// Initialize in on_enter
fn on_enter(&mut self, ctx: &mut SceneContext) {
    let sprite = ctx.assets.load_sprite("bullet.png");
    self.bullet_pool = Pool::new(
        &mut ctx.world,
        PoolConfig::default().with_capacity(200),
        move || (
            Transform::default(),
            Renderable::sprite(sprite.clone()),
            Collider::circle(4.0).as_trigger(),
            Bullet::default(),
        )
    );
}

// Acquire bullet (no allocation!)
fn fire(&mut self, ctx: &mut SceneContext, x: f32, y: f32, vx: f32, vy: f32) {
    if let Some(entity) = self.bullet_pool.acquire(&mut ctx.world) {
        if let Some(mut t) = ctx.world.get_mut::<Transform>(entity) {
            t.x = x;
            t.y = y;
        }
        if let Some(mut b) = ctx.world.get_mut::<Bullet>(entity) {
            b.velocity_x = vx;
            b.velocity_y = vy;
        }
    }
}

// Release bullet (no deallocation!)
fn despawn_bullet(&mut self, ctx: &mut SceneContext, entity: Entity) {
    self.bullet_pool.release(&mut ctx.world, entity);
}
```

### Pool Statistics

Monitor pool performance with `pool.stats()`:

```rust
let stats = pool.stats();
println!("Total spawned: {}", stats.total_spawned);
println!("Currently active: {}", stats.current_active);
println!("Available for reuse: {}", stats.current_available);
println!("Peak active: {}", stats.peak_active);
println!("Expansions: {}", stats.expansions);
```

### Scene Safety

Pools track `world.generation()` to detect scene clears. When `world.clear()` is called during scene transitions, pools automatically invalidate their entity references on the next operation.

### Best Practices

1. **Pre-warm generously** - Over-allocate slightly to avoid runtime expansion
2. **Reset components on acquire** - Don't assume component state from previous use
3. **Match pool size to peak usage** - Monitor `peak_active` to tune capacity
4. **Use `release_all()` for cleanup** - Reset pool state on level end/restart
