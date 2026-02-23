# AGENTS.md - Skeletal Animation Module

## Overview

Provides bone-based skeletal animation with hierarchical transforms, keyframe animation, crossfade blending, and state machine control.

## Module Structure

| Module | Purpose |
|--------|---------|
| `mod.rs` | Public exports, `Skeleton` ECS component |
| `bone.rs` | Core types: `BoneDef`, `BoneTransform`, `Pose`, `SkeletonDef`, world transform computation |
| `animation.rs` | `SkeletalAnimation` (per-bone keyframe tracks), `SkeletalAnimator` (playback) |
| `blend.rs` | `SkeletalBlender` - crossfade between two poses over a duration |
| `controller.rs` | `SkeletalController` - FSM + animation map + blending for state-driven animation |
| `render.rs` | `skeleton_debug_lines()` - generates `Line2D` renderables for bone visualization |

## Key Types

### SkeletonDef (shared, Arc-wrapped)
Immutable bone hierarchy template. Built with `SkeletonDef::builder()`:
```rust
let def = Arc::new(
    SkeletonDef::builder()
        .bone("hips", None, Vec2::ZERO, 0.0, 10.0)
        .bone("spine", Some("hips"), Vec2::new(0.0, -30.0), 0.0, 30.0)
        .build(),
);
```

### Skeleton (ECS component)
Per-entity instance wrapping a shared `SkeletonDef` with mutable pose and cached world transforms.

### BoneTransform
Local-space transform with `position`, `rotation`, `scale`. Supports `lerp()` for blending.

### Pose
Collection of per-bone `BoneTransform`s. Supports `lerp()` for crossfading between poses.

### SkeletalAnimation
Defines per-bone keyframe tracks with easing. Built with builder pattern:
```rust
let anim = Arc::new(
    SkeletalAnimation::builder("walk", 1.0, def.clone())
        .loop_mode(LoopMode::Loop)
        .track("spine", |t| {
            t.at(0.0, BoneTransform::rotation(0.0), Ease::InOutSine)
             .at(0.5, BoneTransform::rotation(0.1), Ease::InOutSine)
             .at(1.0, BoneTransform::rotation(0.0), Ease::InOutSine)
        })
        .build(),
);
```

### SkeletalAnimator
Simple playback component. Advances time, samples animation into a pose. Use for entities that play a single animation.
- `sample(base_pose) -> Pose` - sample current animation at current time

### SkeletalBlender
Crossfades between a frozen "from" pose snapshot and a live "to" pose:
- `start_blend(from_pose, duration)` - begin crossfade
- `update(dt)` - advance blend weight
- `apply(target_pose) -> Pose` - output blended pose
- `blend_progress() -> f32` - current blend weight (0.0 = from, 1.0 = to)

### SkeletalController
State machine for skeletal animations. Wraps `StateMachine<&'static str>` + animation map + `SkeletalAnimator` + `SkeletalBlender`. Caches a rest pose at construction to avoid per-frame allocation.
- `set_state(id)` - transition with crossfade (validates via FSM)
- `set_state_then(id, next)` - play animation, then auto-transition
- `force_state(id)` - bypass FSM rules
- `sample() -> Pose` - get current blended pose
- `blend_progress() -> f32` - current blend weight

## Coordinate System

Y-down: negative Y = up on screen, positive rotation = clockwise.

## System Integration

`SkeletonSystem` (in `src/core/systems/skeleton.rs`) processes all skeleton entities each frame. Entities with `SkeletalController` are handled exclusively by the controller path; standalone `SkeletalAnimator` entities use the animator path. Having both on one entity is unsupported.

## Prelude Exports

All public types are exported via `ryo2d::prelude`:
`BoneDef`, `BoneTransform`, `BoneWorldTransform`, `Pose`, `Skeleton`, `SkeletonDef`, `SkeletalAnimation`, `SkeletalAnimator`, `SkeletalBlender`, `SkeletalController`, `skeleton_debug_lines`
