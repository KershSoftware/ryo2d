# Skeletal Animation

Ryo2D includes a skeletal animation system for bone-based animation. Define bone hierarchies, animate individual bones with keyframe tracks, crossfade between animations, and drive it all with an FSM controller.

This is separate from the [sprite animation system](./animation.md) — use sprite animation for frame-by-frame sprite sheets, and skeletal animation for procedural bone hierarchies (characters, creatures, ragdolls).

## Architecture Overview

| Type | Purpose |
|------|---------|
| `SkeletonDef` | Immutable bone hierarchy template (shared via `Arc`) |
| `Skeleton` | ECS component — per-entity pose and world transforms |
| `SkeletalAnimator` | Simple single-animation playback |
| `SkeletalController` | FSM-driven multi-animation state machine with crossfade |
| `SkeletalBlender` | Crossfade blending between poses |

## Defining a Skeleton

Use `SkeletonDef::builder()` to define the bone hierarchy. Bones reference their parent by name, and the builder ensures parents are always defined before children.

```rust
use ryo2d::prelude::*;
use std::sync::Arc;

let def = Arc::new(
    SkeletonDef::builder()
        // Root bone (no parent)
        .bone("hips", None, Vec2::ZERO, 0.0, 10.0)
        // Spine chain
        .bone("spine", Some("hips"), Vec2::new(0.0, -30.0), 0.0, 30.0)
        .bone("chest", Some("spine"), Vec2::new(0.0, -25.0), 0.0, 25.0)
        .bone("neck", Some("chest"), Vec2::new(0.0, -20.0), 0.0, 10.0)
        .bone("head", Some("neck"), Vec2::new(0.0, -15.0), 0.0, 15.0)
        // Left arm
        .bone("l_upper_arm", Some("chest"), Vec2::new(-15.0, -5.0), 0.3, 25.0)
        .bone("l_forearm", Some("l_upper_arm"), Vec2::new(0.0, 25.0), 0.0, 20.0)
        .bone("l_hand", Some("l_forearm"), Vec2::new(0.0, 20.0), 0.0, 10.0)
        // Right arm
        .bone("r_upper_arm", Some("chest"), Vec2::new(15.0, -5.0), -0.3, 25.0)
        .bone("r_forearm", Some("r_upper_arm"), Vec2::new(0.0, 25.0), 0.0, 20.0)
        .bone("r_hand", Some("r_forearm"), Vec2::new(0.0, 20.0), 0.0, 10.0)
        // Legs
        .bone("l_thigh", Some("hips"), Vec2::new(-10.0, 10.0), 0.0, 30.0)
        .bone("l_shin", Some("l_thigh"), Vec2::new(0.0, 30.0), 0.0, 28.0)
        .bone("l_foot", Some("l_shin"), Vec2::new(0.0, 28.0), 0.0, 12.0)
        .bone("r_thigh", Some("hips"), Vec2::new(10.0, 10.0), 0.0, 30.0)
        .bone("r_shin", Some("r_thigh"), Vec2::new(0.0, 30.0), 0.0, 28.0)
        .bone("r_foot", Some("r_shin"), Vec2::new(0.0, 28.0), 0.0, 12.0)
        .build(),
);
```

Each `.bone()` call takes:
- **name** — unique bone name
- **parent** — `None` for root, `Some("parent_name")` for children
- **position** — local offset from parent (Y-down: negative Y = up on screen)
- **rotation** — rest rotation in radians (positive = clockwise)
- **length** — bone length (used for debug rendering)

`SkeletonDef` is wrapped in `Arc` so multiple entities can share the same skeleton template.

## Bone Transforms

`BoneTransform` represents a bone's local-space transform (relative to its parent):

```rust
// Full transform
BoneTransform::new(Vec2::new(0.0, -30.0), 0.5)

// Rotation only (position stays at zero)
BoneTransform::rotation(0.3)

// Position only (no rotation)
BoneTransform::position(Vec2::new(5.0, 0.0))

// Identity (no change)
BoneTransform::IDENTITY
```

A `Pose` is a collection of `BoneTransform` values — one per bone. You can blend between poses:

```rust
let blended = pose_a.lerp(&pose_b, 0.5); // 50% blend
```

Rotation interpolation uses shortest-path angle interpolation automatically.

## Creating Animations

Use `SkeletalAnimation::builder()` to define per-bone keyframe tracks:

```rust
let idle = Arc::new(
    SkeletalAnimation::builder("idle", 2.0, def.clone())
        .loop_mode(LoopMode::Loop)
        .track("spine", |t| {
            t.at(0.0, BoneTransform::rotation(0.0), Ease::InOutSine)
             .at(1.0, BoneTransform::rotation(0.03), Ease::InOutSine)
             .at(2.0, BoneTransform::rotation(0.0), Ease::InOutSine)
        })
        .track("chest", |t| {
            t.at(0.0, BoneTransform::rotation(0.0), Ease::InOutSine)
             .at(1.0, BoneTransform::rotation(0.02), Ease::InOutSine)
             .at(2.0, BoneTransform::rotation(0.0), Ease::InOutSine)
        })
        .build(),
);
```

Key points:
- **duration** — total animation length in seconds
- **loop_mode** — `Loop`, `Once`, `PingPong`, or `Times(n)`
- **tracks** — only bones that move need tracks; others keep their rest pose
- **keyframes** — each `.at(time, transform, easing)` adds a keyframe
- **easing** — any `Ease` variant (Linear, InOutSine, InOutQuad, etc.)

Animations are wrapped in `Arc` so they can be shared across controllers.

## Simple Playback with SkeletalAnimator

For entities that play a single animation (ambient effects, background characters):

```rust
let entity = world.spawn((
    Transform::new(100.0, 200.0),
    Skeleton::new(def.clone()),
    SkeletalAnimator::new(idle.clone()),
));
```

Control playback:

```rust
if let Some(mut animator) = world.get_mut::<SkeletalAnimator>(entity) {
    animator.play();
    animator.pause();
    animator.reset();          // Back to time 0
    animator.speed = 2.0;      // Double speed

    if animator.just_finished() {
        // One-shot animation completed
    }
}
```

## FSM Controller with SkeletalController

For characters with multiple animation states and crossfade transitions:

```rust
let controller = SkeletalController::new(idle.clone(), "idle", def.clone())
    // Register animations
    .with_animation(walk.clone(), "walk")
    .with_animation(attack.clone(), "attack")
    // Define allowed transitions
    .with_bidirectional("idle", "walk")
    .with_transition("idle", "attack")
    .with_transition("walk", "attack")
    .with_transition("attack", "idle")
    // Blend durations
    .with_default_blend_duration(0.15)
    .with_blend_duration("idle", "attack", 0.05);

let entity = world.spawn((
    Transform::new(100.0, 200.0),
    Skeleton::new(def.clone()),
    controller,
));
```

### Transition Methods

```rust
if let Some(mut ctrl) = world.get_mut::<SkeletalController>(entity) {
    // Validated transition (returns false if not allowed by FSM)
    ctrl.set_state("walk");

    // Play animation, then auto-transition when it finishes
    ctrl.set_state_then("attack", "idle");

    // Bypass FSM rules (use sparingly)
    ctrl.force_state("walk");

    // Query state
    let current = ctrl.current();       // "idle", "walk", etc.
    let blending = ctrl.is_blending();  // Crossfade in progress?
    let progress = ctrl.blend_progress(); // 0.0 to 1.0
}
```

### Transition Rules

| Method | Effect |
|--------|--------|
| `with_transition(from, to)` | One-way transition |
| `with_bidirectional(a, b)` | Two-way transition |
| `with_transition_from_any(to)` | Can enter from any state |

If a transition isn't registered, `set_state()` returns `false` and nothing happens. This prevents invalid state changes.

## Crossfade Blending

When transitioning between animations, the controller automatically crossfades:

1. Snapshots the current pose
2. Starts the new animation from time 0
3. Lerps from the snapshot toward the new animation over the blend duration

Custom blend durations per transition:

```rust
let controller = SkeletalController::new(idle, "idle", def)
    .with_default_blend_duration(0.15)         // Default for all transitions
    .with_blend_duration("idle", "attack", 0.05)  // Quick snap to attack
    .with_blend_duration("attack", "idle", 0.3);  // Slow ease out of attack
```

Setting a blend duration of `0.0` makes the transition instant (no crossfade).

## Debug Visualization

Render skeletons as lines and joint circles for debugging:

```rust
use ryo2d::prelude::*;

if let Some(skeleton) = world.get::<Skeleton>(entity) {
    let lines = skeleton_debug_lines(
        &skeleton.def,
        &skeleton.world_transforms,
        Color::WHITE,           // Bone color
        Color::new(0, 255, 0),  // Joint color
        2.0,                    // Line width
        4.0,                    // Joint radius
    );

    for line in lines {
        world.spawn((Transform::new(0.0, 0.0), line));
    }
}
```

This generates `Line2D` components — bone segments between parent-child joints and circles at each joint position.

## Engine Integration

The `SkeletonSystem` runs automatically each frame and handles:

1. **Animator updates** — advances playback time on `SkeletalAnimator` components
2. **Controller updates** — advances FSM state, playback, and blending on `SkeletalController` components
3. **Pose sampling** — writes the current animation pose to `Skeleton.pose`
4. **World transforms** — recomputes `Skeleton.world_transforms` from the entity's `Transform`

You don't call any of this manually — just spawn entities with the right components and the engine handles the rest.

### Component Patterns

| Pattern | Components | Use Case |
|---------|-----------|----------|
| Simple playback | `Skeleton` + `SkeletalAnimator` | Single looping animation |
| FSM-driven | `Skeleton` + `SkeletalController` | Character with multiple states |

Do not add both `SkeletalAnimator` and `SkeletalController` to the same entity.

## Coordinate System

- **Y-down** — negative Y = up on screen
- **Rotation** — positive = clockwise (radians)
- **Bone order** — parents are always processed before children (guaranteed by the builder)

## When to Use Which Animation System

| Use Case | System |
|----------|--------|
| Sprite sheet characters (pixel art) | [Sprite Animation](./animation.md) |
| Bone-based characters (stick figures, puppets) | Skeletal Animation |
| Procedural motion (ragdolls, IK) | Skeletal Animation |
| Simple decorative loops (coins, flames) | [Sprite Animation](./animation.md) |
| Characters with many animation states and blending | Skeletal Animation with `SkeletalController` |

## See Also

- [Animation System](./animation.md) — Sprite-based frame animation
- [ECS Architecture](./ecs.md) — Understanding components and entities
- [Skeletal Animation Examples](../examples/skeletal-animation.md) — Practical usage examples
