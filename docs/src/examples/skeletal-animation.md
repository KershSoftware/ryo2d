# Skeletal Animation Examples

This guide shows practical examples of using Ryo2D's skeletal animation system, from a minimal skeleton to a full character with an FSM controller.

## Example 1: Minimal Skeleton with Idle Breathing

The simplest possible skeletal animation — a two-bone spine that gently rocks:

```rust
use ryo2d::prelude::*;
use std::sync::Arc;

fn spawn_simple_skeleton(world: &mut World) -> Entity {
    // Define a minimal skeleton: root + spine
    let def = Arc::new(
        SkeletonDef::builder()
            .bone("root", None, Vec2::ZERO, 0.0, 10.0)
            .bone("spine", Some("root"), Vec2::new(0.0, -30.0), 0.0, 30.0)
            .build(),
    );

    // Create a breathing animation on the spine bone
    let breathe = Arc::new(
        SkeletalAnimation::builder("breathe", 2.0, def.clone())
            .loop_mode(LoopMode::Loop)
            .track("spine", |t| {
                t.at(0.0, BoneTransform::rotation(0.0), Ease::InOutSine)
                 .at(1.0, BoneTransform::rotation(0.05), Ease::InOutSine)
                 .at(2.0, BoneTransform::rotation(0.0), Ease::InOutSine)
            })
            .build(),
    );

    // Spawn with simple animator (no FSM needed)
    world.spawn((
        Transform::new(200.0, 300.0),
        Skeleton::new(def),
        SkeletalAnimator::new(breathe),
    ))
}
```

The engine automatically updates the animation each frame. No manual tick calls needed.

## Example 2: Stick Figure with Idle and Walk

A humanoid skeleton with two animation states controlled by an FSM:

```rust
use ryo2d::prelude::*;
use std::sync::Arc;

fn create_humanoid() -> (Arc<SkeletonDef>, Arc<SkeletalAnimation>, Arc<SkeletalAnimation>) {
    let def = Arc::new(
        SkeletonDef::builder()
            .bone("hips", None, Vec2::ZERO, 0.0, 10.0)
            .bone("spine", Some("hips"), Vec2::new(0.0, -30.0), 0.0, 30.0)
            .bone("chest", Some("spine"), Vec2::new(0.0, -25.0), 0.0, 25.0)
            .bone("head", Some("chest"), Vec2::new(0.0, -20.0), 0.0, 15.0)
            // Arms
            .bone("l_arm", Some("chest"), Vec2::new(-15.0, -5.0), 0.3, 25.0)
            .bone("l_forearm", Some("l_arm"), Vec2::new(0.0, 25.0), 0.0, 20.0)
            .bone("r_arm", Some("chest"), Vec2::new(15.0, -5.0), -0.3, 25.0)
            .bone("r_forearm", Some("r_arm"), Vec2::new(0.0, 25.0), 0.0, 20.0)
            // Legs
            .bone("l_thigh", Some("hips"), Vec2::new(-10.0, 10.0), 0.0, 30.0)
            .bone("l_shin", Some("l_thigh"), Vec2::new(0.0, 30.0), 0.0, 28.0)
            .bone("r_thigh", Some("hips"), Vec2::new(10.0, 10.0), 0.0, 30.0)
            .bone("r_shin", Some("r_thigh"), Vec2::new(0.0, 30.0), 0.0, 28.0)
            .build(),
    );

    // Idle: subtle spine breathing
    let idle = Arc::new(
        SkeletalAnimation::builder("idle", 2.0, def.clone())
            .loop_mode(LoopMode::Loop)
            .track("spine", |t| {
                t.at(0.0, BoneTransform::rotation(0.0), Ease::InOutSine)
                 .at(1.0, BoneTransform::rotation(0.03), Ease::InOutSine)
                 .at(2.0, BoneTransform::rotation(0.0), Ease::InOutSine)
            })
            .build(),
    );

    // Walk: alternating leg and arm swings
    let walk = Arc::new(
        SkeletalAnimation::builder("walk", 1.0, def.clone())
            .loop_mode(LoopMode::Loop)
            // Left leg swings forward, then back
            .track("l_thigh", |t| {
                t.at(0.0, BoneTransform::rotation(-0.4), Ease::InOutSine)
                 .at(0.5, BoneTransform::rotation(0.4), Ease::InOutSine)
                 .at(1.0, BoneTransform::rotation(-0.4), Ease::InOutSine)
            })
            // Right leg opposite phase
            .track("r_thigh", |t| {
                t.at(0.0, BoneTransform::rotation(0.4), Ease::InOutSine)
                 .at(0.5, BoneTransform::rotation(-0.4), Ease::InOutSine)
                 .at(1.0, BoneTransform::rotation(0.4), Ease::InOutSine)
            })
            // Arms swing opposite to legs
            .track("l_arm", |t| {
                t.at(0.0, BoneTransform::rotation(0.5), Ease::InOutSine)
                 .at(0.5, BoneTransform::rotation(-0.3), Ease::InOutSine)
                 .at(1.0, BoneTransform::rotation(0.5), Ease::InOutSine)
            })
            .track("r_arm", |t| {
                t.at(0.0, BoneTransform::rotation(-0.3), Ease::InOutSine)
                 .at(0.5, BoneTransform::rotation(0.5), Ease::InOutSine)
                 .at(1.0, BoneTransform::rotation(-0.3), Ease::InOutSine)
            })
            .build(),
    );

    (def, idle, walk)
}

fn spawn_character(world: &mut World) -> Entity {
    let (def, idle, walk) = create_humanoid();

    let controller = SkeletalController::new(idle, "idle", def.clone())
        .with_animation(walk, "walk")
        .with_bidirectional("idle", "walk")
        .with_default_blend_duration(0.15);

    world.spawn((
        Transform::new(400.0, 400.0),
        Skeleton::new(def),
        controller,
    ))
}

// In your scene update:
fn update_character(world: &mut World, entity: Entity, input: &Input) {
    if let Some(mut ctrl) = world.get_mut::<SkeletalController>(entity) {
        if input.key(KeyCode::KeyD) || input.key(KeyCode::KeyA) {
            ctrl.set_state("walk");
        } else {
            ctrl.set_state("idle");
        }
    }
}
```

## Example 3: One-Shot Attack with Auto-Return

A character that plays an attack animation and returns to idle:

```rust
use ryo2d::prelude::*;
use std::sync::Arc;

fn setup_combat_controller(
    def: Arc<SkeletonDef>,
    idle: Arc<SkeletalAnimation>,
    walk: Arc<SkeletalAnimation>,
) -> SkeletalController {
    // Attack: punch animation (one-shot)
    let attack = Arc::new(
        SkeletalAnimation::builder("punch", 0.4, def.clone())
            .loop_mode(LoopMode::Once)
            .track("chest", |t| {
                t.at(0.0, BoneTransform::rotation(0.0), Ease::OutQuad)
                 .at(0.15, BoneTransform::rotation(-0.3), Ease::OutQuad)
                 .at(0.4, BoneTransform::rotation(0.0), Ease::InOutQuad)
            })
            .track("r_arm", |t| {
                t.at(0.0, BoneTransform::rotation(-0.3), Ease::OutQuad)
                 .at(0.15, BoneTransform::rotation(-1.5), Ease::OutQuad)
                 .at(0.4, BoneTransform::rotation(-0.3), Ease::InOutQuad)
            })
            .track("r_forearm", |t| {
                t.at(0.0, BoneTransform::rotation(0.0), Ease::OutQuad)
                 .at(0.15, BoneTransform::rotation(-0.5), Ease::OutQuad)
                 .at(0.4, BoneTransform::rotation(0.0), Ease::InOutQuad)
            })
            .build(),
    );

    SkeletalController::new(idle, "idle", def.clone())
        .with_animation(walk, "walk")
        .with_animation(attack, "attack")
        // Transitions
        .with_bidirectional("idle", "walk")
        .with_transition("idle", "attack")
        .with_transition("walk", "attack")
        .with_transition("attack", "idle")
        .with_transition("attack", "walk")
        // Fast snap into attack, slow ease out
        .with_default_blend_duration(0.15)
        .with_blend_duration("idle", "attack", 0.05)
        .with_blend_duration("walk", "attack", 0.05)
        .with_blend_duration("attack", "idle", 0.2)
}

// In your scene update:
fn handle_combat_input(world: &mut World, entity: Entity, input: &Input) {
    if let Some(mut ctrl) = world.get_mut::<SkeletalController>(entity) {
        // Attack key: play punch, then auto-return to idle
        if input.key_pressed(KeyCode::Space) {
            ctrl.set_state_then("attack", "idle");
            return;
        }

        // Only change movement state if not attacking
        if ctrl.current() != "attack" {
            if input.key(KeyCode::KeyD) || input.key(KeyCode::KeyA) {
                ctrl.set_state("walk");
            } else {
                ctrl.set_state("idle");
            }
        }
    }
}
```

The `set_state_then("attack", "idle")` call plays the attack animation and automatically transitions to idle when it finishes.

## Example 4: Debug Visualization

Render your skeleton as colored lines for debugging and prototyping:

```rust
use ryo2d::prelude::*;

struct SkeletonDebugScene {
    character: Option<Entity>,
    debug_entities: Vec<Entity>,
}

impl SkeletonDebugScene {
    fn redraw_debug_lines(&mut self, world: &mut World) {
        // Remove old debug lines
        for entity in self.debug_entities.drain(..) {
            world.despawn(entity);
        }

        // Generate new debug lines from current skeleton state
        if let Some(character) = self.character {
            if let Some(skeleton) = world.get::<Skeleton>(character) {
                let lines = skeleton_debug_lines(
                    &skeleton.def,
                    &skeleton.world_transforms,
                    Color::WHITE,              // Bone segments
                    Color::new(0, 255, 0),     // Joint circles
                    2.0,                       // Line width
                    4.0,                       // Joint radius
                );

                // Clone data before spawning (drop the borrow)
                let lines: Vec<Line2D> = lines;
                drop(skeleton);

                for line in lines {
                    let entity = world.spawn((Transform::new(0.0, 0.0), line));
                    self.debug_entities.push(entity);
                }
            }
        }
    }
}
```

Call `redraw_debug_lines()` every frame in your scene's `render()` to see the skeleton move in real-time.

## Example 5: Multiple Scenes with Shared Skeleton

Multiple entities can share the same `SkeletonDef` and animation clips:

```rust
use ryo2d::prelude::*;
use std::sync::Arc;

fn spawn_crowd(
    world: &mut World,
    def: Arc<SkeletonDef>,
    idle: Arc<SkeletalAnimation>,
    walk: Arc<SkeletalAnimation>,
    count: usize,
) -> Vec<Entity> {
    let mut entities = Vec::new();

    for i in 0..count {
        let x = 100.0 + (i as f32) * 80.0;

        // Each entity gets its own controller instance, but shares
        // the Arc<SkeletonDef> and Arc<SkeletalAnimation> data
        let controller = SkeletalController::new(idle.clone(), "idle", def.clone())
            .with_animation(walk.clone(), "walk")
            .with_bidirectional("idle", "walk")
            .with_default_blend_duration(0.15);

        let entity = world.spawn((
            Transform::new(x, 400.0),
            Skeleton::new(def.clone()),
            controller,
        ));

        entities.push(entity);
    }

    entities
}
```

The `Arc` wrapping means the skeleton definition and animation data are only stored once in memory, no matter how many entities use them.

## Best Practices

1. **Share definitions** — wrap `SkeletonDef` and `SkeletalAnimation` in `Arc` and clone the `Arc` for each entity
2. **Only animate what moves** — bones without tracks inherit their rest pose; don't add empty tracks
3. **Use easing** — `Ease::InOutSine` looks natural for organic motion; `Ease::OutQuad` for snappy attacks
4. **Tune blend durations** — fast blends (0.05s) for combat snappiness, slow blends (0.2-0.3s) for relaxed transitions
5. **One-shot with auto-return** — use `set_state_then()` for attacks, jumps, and reactions that should return to a base state
6. **Guard transitions** — check `ctrl.current()` before changing state to avoid interrupting important animations
7. **Redraw debug lines each frame** — skeleton world transforms change every frame, so debug visualization must be regenerated

## See Also

- [Skeletal Animation Concept](../concepts/skeletal-animation.md) — Full API reference and system overview
- [Sprite Animation](../concepts/animation.md) — Frame-based sprite animation
- [ECS Architecture](../concepts/ecs.md) — Understanding components and entities
