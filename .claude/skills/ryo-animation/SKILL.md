---
name: ryo-animation
description: Sprite animations, Animator component, AnimationController state machines.
metadata:
  version: 1.0.0
---

# Ryo2D Animation Specialist

You help users create sprite animations, from simple loops to complex character state machines.

**For complete API signatures, see the `ryo-api` skill.**

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
fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
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

## Tweening - Transform Animation

For animating **Transform properties** (position, rotation, scale) rather than sprite frames, use the `Tween` component.

### Quick Examples

```rust
// Move to position with bounce
ctx.world.spawn((
    Transform::new(100.0, 100.0),
    Tween::to_position(400.0, 300.0, 2.0)
        .with_easing(Ease::OutBounce),
));

// Rotate with overshoot
ctx.world.spawn((
    Transform::new(200.0, 200.0),
    Tween::to_rotation(PI, 0.5)
        .with_easing(Ease::OutBack),
));

// Pulsing scale (looping)
ctx.world.spawn((
    Transform::new(300.0, 300.0),
    Tween::to_scale(1.2, 1.2, 0.3)
        .with_easing(Ease::InOutSine)
        .with_loop(TweenLoop::PingPong),
));

// Combined with delay
ctx.world.spawn((
    Transform::new(0.0, 0.0),
    Tween::new(1.5)
        .to_x(200.0)
        .to_y(100.0)
        .to_rotation_value(PI / 4.0)
        .with_easing(Ease::OutCubic)
        .with_delay(0.5),
));
```

### Callbacks

Tween callbacks receive the eased progress (0.0–1.0):

```rust
Tween::to_position(100.0, 100.0, 1.0)
    .on_start(|_progress| println!("started"))      // receives 0.0
    .on_update(|progress| println!("at {progress}")) // receives eased 0.0-1.0
    .on_complete(|_| println!("done"));              // receives 1.0
```

### Easing Functions

| Category | Options |
|----------|---------|
| Basic | `Linear` |
| Smooth | `InQuad`, `OutQuad`, `InOutQuad` |
| Smoother | `InCubic`, `OutCubic`, `InOutCubic` |
| Dramatic | `InExpo`, `OutExpo`, `InOutExpo` |
| Overshoot | `InBack`, `OutBack`, `InOutBack` |
| Bouncy | `InBounce`, `OutBounce`, `InOutBounce` |
| Elastic | `InElastic`, `OutElastic`, `InOutElastic` |

### Loop Modes

- `TweenLoop::Once` - Play once, stop (default)
- `TweenLoop::Loop` - Repeat forever
- `TweenLoop::PingPong` - Forward, backward, repeat
- `TweenLoop::Times(n)` - Repeat n times

### Detecting Completion

```rust
if let Some(tween) = ctx.world.get_component::<Tween>(entity) {
    if tween.just_finished {
        // Tween completed this frame (or loop iteration)
    }
    if tween.is_finished() {
        // Tween fully done (Once or Times mode)
    }
}
```

## PropertyTween\<T\> — Generic Value Tweening

For animating **arbitrary values** (not just Transform), use `PropertyTween<T>`. It interpolates any `Lerp`-implementing type. You read `.value()` each frame.

### Quick Examples

```rust
// Tween a float (health, opacity, score display)
let entity = ctx.world.spawn((
    PropertyTween::new(0.0f32, 100.0, 2.0)
        .with_easing(Ease::OutQuad),
));

// Tween a Vec2 (custom position, offset)
let entity = ctx.world.spawn((
    PropertyTween::new(Vec2::ZERO, Vec2::new(400.0, 300.0), 1.5)
        .with_loop(TweenLoop::PingPong),
));

// Tween a Color
let entity = ctx.world.spawn((
    PropertyTween::new(Color::RED, Color::BLUE, 1.0),
));
```

### Reading Values

```rust
if let Some(tween) = ctx.world.get_component::<PropertyTween<f32>>(entity) {
    let value = tween.value();      // Current interpolated value
    let progress = tween.progress(); // Raw 0.0-1.0
}
```

### Runtime Control

```rust
tween.pause();
tween.resume();
tween.stop();           // Freezes at current value
tween.restart();        // Replays from start
tween.retarget(50.0);   // Change destination mid-flight
```

### Auto-Driven Types

The engine automatically updates `PropertyTween<f32>`, `PropertyTween<Vec2>`, and `PropertyTween<Color>`. For custom `Lerp` types, call `tween.update(dt)` manually.

## Tips

- Use `Animator` for decorative animations (coins, flames, water)
- Use `AnimationController` for player/enemy characters with states
- Use `Tween` for position/rotation/scale animations (UI, effects, movement)
- Use `PropertyTween<T>` for animating arbitrary values (health bars, opacity, colors)
- `set_state_then()` is perfect for attack/hurt animations that return to idle
- Flip sprites with `renderable.flip_x` rather than separate left/right animations
- The engine auto-syncs animation frames to `Renderable`
- The engine auto-updates `Tween`, `PropertyTween<f32/Vec2/Color>` each frame

