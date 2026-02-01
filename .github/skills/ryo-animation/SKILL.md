---
name: ryo-animation
description: Sprite animations, Animator component, AnimationController state machines.
metadata:
  version: 1.0.0
---

# Ryo2D Animation Specialist

You help users create sprite animations, from simple loops to complex character state machines.

## Resources

- **Animation Module**: `src/core/animation.rs`
- **Architecture**: `src/core/AGENTS.md` (Animation System section)

## Three Animation APIs

| API | Use Case |
|-----|----------|
| `Animation` | Define frame sequences with timing |
| `Animator` | Simple looping animations (coins, flames) |
| `AnimationController` | State machines (character walk/run/attack) |

## Animation - Frame Sequences

```rust
// Uniform frame duration
let walk_anim = Animation::new(
    vec![frame1, frame2, frame3, frame4],
    0.1,  // 100ms per frame
    LoopMode::Loop,
);

// Variable timing
let attack_anim = Animation::with_durations(
    vec![frame1, frame2, frame3],
    vec![0.1, 0.15, 0.2],
    LoopMode::Once,
);
```

LoopMode: `Once`, `Loop`, `PingPong`, `Times(n)`

## Animator - Simple Looping

For entities that just play a looping animation:

```rust
let coin = ctx.world.spawn((
    Transform::new(100.0, 100.0),
    Renderable::sprite(coin_frames[0].clone()),
    Animator::new(coin_animation),  // Engine auto-updates
));

// Control playback
if let Some(mut animator) = ctx.world.get_component_mut::<Animator>(coin) {
    animator.play();
    animator.pause();
    animator.stop();          // Reset to frame 0
    animator.set_speed(2.0);  // 2x speed
}
```

## AnimationController - State Machine

For characters with multiple animation states:

```rust
let mut controller = AnimationController::new();
controller.add("idle", idle_animation);
controller.add("walk", walk_animation);
controller.add("jump", jump_animation);
controller.add("attack", attack_animation);
controller.set_state("idle");

let player = ctx.world.spawn((
    Transform::new(100.0, 100.0),
    Renderable::sprite(idle_frames[0].clone()),
    controller,
));
```

### State Transitions

```rust
fn update(&mut self, ctx: &mut GameContext, dt: f32) {
    if let Some(mut ctrl) = ctx.world.get_component_mut::<AnimationController>(self.player) {
        if ctx.input.key_down(KeyCode::KeyD) {
            ctrl.set_state("walk");
        } else {
            ctrl.set_state("idle");
        }

        // One-shot that returns to previous state
        if ctx.input.key_pressed(KeyCode::Space) {
            ctrl.set_state_then("attack", "idle");
        }
    }
}
```

### Query State

```rust
if let Some(ctrl) = ctx.world.get_component::<AnimationController>(self.player) {
    ctrl.current_state()  // "idle", "walk", etc.
    ctrl.is_finished()    // True if LoopMode::Once completed
}
```

## Common Patterns

### Character State Machine

```rust
// Priority: attack > jump > walk > idle
if self.is_attacking {
    if ctrl.current_state() != "attack" {
        ctrl.set_state_then("attack", ctrl.current_state());
    }
    if ctrl.is_finished() {
        self.is_attacking = false;
    }
} else if !self.is_grounded {
    ctrl.set_state("jump");
} else if self.is_moving {
    ctrl.set_state("walk");
} else {
    ctrl.set_state("idle");
}
```

### Directional Flip

```rust
if let Some(mut renderable) = ctx.world.get_component_mut::<Renderable>(self.player) {
    if dx < 0.0 { renderable.flip_x = true; }   // Face left
    else if dx > 0.0 { renderable.flip_x = false; }  // Face right
}
```

## Tips

- Use `Animator` for decorative animations (coins, flames, water)
- Use `AnimationController` for player/enemy characters with states
- `set_state_then()` is perfect for attack/hurt animations that return to idle
- Flip sprites with `renderable.flip_x` rather than separate left/right animations
- The engine auto-syncs animation frames to `Renderable`

