# AGENTS.md - Game Systems

## Overview

This folder contains game systems extracted from `engine.rs` for modularity and testability. Each system handles a specific concern and operates on the ECS World independently.

## Module Structure

| Module | Purpose |
|--------|---------|
| `mod.rs` | Public exports for all systems |
| `animation.rs` | Update Animator and AnimationController components |
| `interpolation.rs` | Snapshot transforms for render interpolation |
| `tween.rs` | Update Tween and TweenSequence components |
| `knockback.rs` | Apply knockback forces to entities |
| `squash_stretch.rs` | Apply squash/stretch scale effects |
| `trail.rs` | Update Trail2D motion trails |
| `skeleton.rs` | Update skeletal animation (animators, controllers, poses, world transforms) |
| `physics.rs` | Sync ECS with Rapier physics world |

## System Pattern

All systems follow a consistent stateless pattern:

```rust
pub struct MySystem;

impl MySystem {
  pub fn update(world: &mut World, dt: f32) {
    for entity in world.entities_with::<MyComponent>() {
      // Skip inactive pooled entities
      if world.has::<Inactive>(entity) {
        continue;
      }
      // Process entity...
    }
  }
}
```

Key conventions:
- **Stateless**: Systems don't hold state; they operate on World data
- **Skip Inactive**: Always check for `Inactive` component (object pooling)
- **Delta time**: Receive scaled dt for time effects (slow-mo, hitstop)

## InterpolationSystem

Manages render interpolation for smooth motion between fixed timestep updates.

**Components handled:**
- `PreviousTransform` - Stores previous frame's transform for lerping
- `Renderable` - Checks `interpolate` flag to determine if entity uses interpolation

**Behavior:**
1. Auto-add `PreviousTransform` to entities with `renderable.interpolate = true`
2. Snapshot current transforms before fixed timestep updates
3. Handle skip flag for teleports (prevents visual sliding)

**Usage:**

```rust
// Enable interpolation on an entity
Renderable::sprite(sprite).interpolated()

// Skip interpolation after teleporting (prevents slide from old position)
InterpolationSystem::skip_interpolation(&mut world, entity);
```

**When to use:**
- Fast-moving entities (balls, bullets, projectiles)
- Player-controlled entities for responsive feel
- Scrolling backgrounds for smooth motion
- Any entity where fixed timestep causes visible stuttering

**When NOT to use:**
- Static entities (walls, UI elements)
- Short-lived particles (overhead not justified)
- Entities that teleport frequently

```rust
InterpolationSystem::snapshot(&mut world);  // Called by engine before updates
```

## AnimationSystem

Updates sprite animations each frame.

**Components handled:**
- `Animator` - Simple looping animations
- `AnimationController` - State machine animations

**Behavior:**
1. Update animation timers with dt
2. Advance to next frame when timer expires
3. Sync current sprite to entity's `Renderable` component

```rust
AnimationSystem::update(&mut world, dt);
```

## TweenSystem

Updates property interpolation for animations.

**Components handled:**
- `Tween` - Single property animation
- `TweenSequence` - Chained tween animations

**Properties tweened:**
- Transform: position, scale, rotation
- Renderable: alpha, tint color

**Behavior:**
1. Capture start values on first frame (for color tweens)
2. Update tween progress with dt
3. Apply interpolated values to Transform/Renderable
4. Advance TweenSequence when current tween finishes

```rust
TweenSystem::update(&mut world, dt);
```

## KnockbackSystem

Applies force-based movement for hit reactions.

**Components handled:**
- `Knockback` - Direction, force, and decay settings

**Events emitted:**
- `KnockbackStarted` - First frame of knockback
- `KnockbackFinished` - Knockback strength reached zero

**Behavior:**
1. Update knockback strength (decays over time)
2. Calculate current velocity from direction × force × strength
3. Update `Velocity` component (for physics systems)
4. Apply position change directly to `Transform`

```rust
KnockbackSystem::update(&mut world, &mut events, dt);
```

## SquashStretchSystem

Applies scale deformation for impact feedback.

**Components handled:**
- `SquashStretch` - Intensity, axis, and decay settings

**Events emitted:**
- `SquashStretchStarted` - Effect activated
- `SquashStretchFinished` - Effect completed

**Behavior:**
1. Capture base scale from Transform on first update
2. Calculate current scale based on strength and intensity
3. Apply scale to Transform (preserves volume if configured)
4. Decay strength back to normal

```rust
SquashStretchSystem::update(&mut world, &mut events, dt);
```

## SkeletonSystem

Updates skeletal animation each frame in five phases.

**Components handled:**
- `SkeletalAnimator` - Playback time advancement
- `SkeletalController` - FSM state machine + crossfade blending
- `Skeleton` - Pose storage and world-space bone transforms

**Behavior:**
1. Advance `SkeletalAnimator` playback time
2. Advance `SkeletalController` state machines (FSM + blend weight)
3. Sample `SkeletalAnimator` animations into `Skeleton.pose`
4. Sample `SkeletalController` blended poses into `Skeleton.pose`
5. Recompute `Skeleton.world_transforms` from entity's `Transform`

```rust
SkeletonSystem::update(&mut world, dt);
```

**Note:** If an entity has both `SkeletalAnimator` and `SkeletalController`, the controller's pose wins (it runs after the animator phase).

## TrailSystem

Records entity positions for motion trail effects.

**Components handled:**
- `Trail2D` - Trail configuration and point history

**Behavior:**
1. Get entity position from Transform
2. Add point to trail if moved beyond min_distance
3. Trail handles point aging and removal internally

```rust
TrailSystem::update(&mut world, dt);
```

## PhysicsSystem

Synchronizes ECS entities with Rapier2D physics.

**Components handled:**
- `RigidBody` - Physics body type (dynamic, kinematic)
- `Collider` - Collision shape
- `GroupedCollider` - Composite tile collision
- `Transform` - Position sync
- `Velocity` - Velocity sync

**Methods:**

```rust
// Full cycle: sync entities, step, sync back
let generation = PhysicsSystem::update(
  &mut physics,
  &mut world,
  gravity,
  last_generation,
);

// Or individual steps:
let gen = PhysicsSystem::sync_entities(&mut physics, &world, last_gen);
PhysicsSystem::step(&mut physics, &world, gravity);
PhysicsSystem::sync_to_ecs(&physics, &mut world);
```

**Entity lifecycle:**
1. Auto-register entities with RigidBody + Collider + Transform
2. Auto-unregister despawned entities
3. Skip inactive (pooled) entities
4. Handle world.clear() via generation tracking

## Engine Integration

Systems are called from `Engine::update()` in this order:

```rust
fn update(&mut self, dt: f32) {
  // 0. Snapshot transforms for interpolation (before any updates)
  InterpolationSystem::snapshot(&mut self.world);

  // 1. Clear events, emit collision events
  self.events.clear();

  // 2. Animation systems
  AnimationSystem::update(&mut self.world, dt);
  SkeletonSystem::update(&mut self.world, dt);
  TweenSystem::update(&mut self.world, dt);

  // 3. Effect systems
  KnockbackSystem::update(&mut self.world, &mut self.events, dt);
  SquashStretchSystem::update(&mut self.world, &mut self.events, dt);

  // 4. Game logic (user code)
  game.update(&mut ctx, dt);

  // 5. Hierarchy propagation
  self.world.propagate_transforms();

  // 6. Trail recording (after final positions known)
  TrailSystem::update(&mut self.world, dt);

  // 7. Physics (unless paused)
  PhysicsSystem::update(...);
}
```

## Adding New Systems

1. Create `src/core/systems/mysystem.rs`:

```rust
use crate::core::ecs::{Inactive, World};

pub struct MySystem;

impl MySystem {
  pub fn update(world: &mut World, dt: f32) {
    for entity in world.entities_with::<MyComponent>() {
      if world.has::<Inactive>(entity) {
        continue;
      }
      // System logic...
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_my_system_no_crash_on_empty_world() {
    let mut world = World::default();
    MySystem::update(&mut world, 0.016);
  }

  // Add more comprehensive tests...
}
```

2. Export in `mod.rs`:

```rust
mod mysystem;
pub use mysystem::MySystem;
```

3. Call from `engine.rs`:

```rust
use super::systems::MySystem;

// In update():
MySystem::update(&mut self.world, dt);
```

## Testing

Each system has unit tests covering:
- Empty world (no crash)
- Component updates
- Event emission (if applicable)
- Inactive entity skipping
- Edge cases

Run system tests:
```bash
cargo test --lib systems::
```

## Design Rationale

**Why extract systems?**
- Reduces `engine.rs` complexity (~500 lines extracted)
- Each system is independently testable
- Clear separation of concerns
- Easier to understand system behavior
- Consistent patterns across systems

**Why stateless?**
- No hidden state to manage
- Easy to test in isolation
- Thread-safe by design
- Can be reordered if needed
