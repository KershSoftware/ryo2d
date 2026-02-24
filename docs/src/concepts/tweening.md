# Tweening System

Ryo2D provides two tweening components for smooth interpolation over time:

- **`Tween`** — Animates Transform properties (position, rotation, scale) automatically
- **`PropertyTween<T>`** — Interpolates any `Lerp`-implementing value; you read `.value()` each frame

Both share the same easing functions, loop modes, and callback system.

## Tween — Transform Animation

Attach a `Tween` to any entity with a `Transform`. The engine applies interpolated values automatically each frame.

```rust
// Move to position with bounce
ctx.world.spawn((
    Transform::new(100.0, 100.0),
    Tween::to_position(400.0, 300.0, 2.0)
        .with_easing(Ease::OutBounce),
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

## PropertyTween\<T\> — Generic Value Tweening

`PropertyTween<T>` interpolates any type that implements the `Lerp` trait. Unlike `Tween`, it does **not** auto-apply to any component — you read `.value()` in your update loop and use it however you want.

```rust
use ryo2d::prelude::*;

// Tween a float (health bar, opacity, etc.)
let entity = ctx.world.spawn((
    PropertyTween::new(0.0f32, 100.0, 2.0)
        .with_easing(Ease::OutQuad),
));

// Tween a Vec2 (custom position, offset, etc.)
let entity = ctx.world.spawn((
    PropertyTween::new(Vec2::ZERO, Vec2::new(400.0, 300.0), 1.5)
        .with_loop(TweenLoop::PingPong),
));

// Tween a Color (background, tint, etc.)
let entity = ctx.world.spawn((
    PropertyTween::new(Color::RED, Color::BLUE, 1.0),
));
```

### Reading Values

```rust
fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    if let Some(tween) = ctx.world.get_component::<PropertyTween<f32>>(self.health_entity) {
        let health = tween.value();  // Current interpolated f32
        // Use health to update UI, scale a bar, etc.
    }
    SceneAction::Continue
}
```

### Runtime Control

```rust
if let Some(mut tween) = ctx.world.get_component_mut::<PropertyTween<f32>>(entity) {
    tween.pause();
    tween.resume();
    tween.stop();       // Freezes at current value
    tween.restart();    // Replays from start
    tween.retarget(50.0); // Change destination mid-flight
}
```

### Automatically Driven Types

The engine automatically updates `PropertyTween<f32>`, `PropertyTween<Vec2>`, and `PropertyTween<Color>` each frame. For custom types, call `tween.update(dt)` manually in your update loop.

## Easing Functions

24 standard easing functions control interpolation curves:

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

## Loop Modes

- `TweenLoop::Once` — Play once, stop (default)
- `TweenLoop::Loop` — Repeat forever
- `TweenLoop::PingPong` — Forward then backward, repeat
- `TweenLoop::Times(n)` — Repeat n times

## Callbacks

All tween callbacks receive the eased progress value (0.0–1.0):

```rust
Tween::to_position(100.0, 100.0, 1.0)
    .on_start(|_progress| println!("started"))        // receives 0.0
    .on_update(|progress| println!("at {progress}"))   // receives eased 0.0-1.0
    .on_complete(|_progress| println!("done"));        // receives 1.0

PropertyTween::new(0.0f32, 100.0, 1.0)
    .on_update(|progress| {
        // Use progress for custom effects
        println!("eased progress: {progress}");
    });
```

## TweenSequence

Chain multiple tweens to play in order:

```rust
ctx.world.spawn((
    Transform::new(0.0, 0.0),
    TweenSequence::new()
        .then(Tween::to_position(200.0, 0.0, 1.0))
        .then(Tween::to_position(200.0, 200.0, 1.0))
        .then(Tween::to_position(0.0, 0.0, 1.0))
        .with_loop(),
));
```

## Events

Both `Tween` and `PropertyTween<T>` emit the same events:

| Event | When |
|-------|------|
| `TweenStarted { entity }` | Tween begins playing |
| `TweenCompleted { entity, finished }` | Tween finishes (or loop iteration completes) |

```rust
for event in ctx.events.read::<TweenCompleted>() {
    if event.finished {
        ctx.world.despawn(event.entity);
    }
}
```

## Lerp Trait

Implement `Lerp` for custom types to use with `PropertyTween<T>`:

```rust
use ryo2d::prelude::*;

#[derive(Clone)]
struct Temperature {
    celsius: f32,
}

impl Lerp for Temperature {
    fn lerp(start: &Self, end: &Self, t: f32) -> Self {
        Temperature {
            celsius: f32::lerp(&start.celsius, &end.celsius, t),
        }
    }
}

// Now you can tween it (call .update(dt) manually for custom types)
let mut tween = PropertyTween::new(
    Temperature { celsius: 20.0 },
    Temperature { celsius: 100.0 },
    5.0,
);
```

Built-in `Lerp` impls: `f32`, `(f32, f32)`, `Vec2`, `Color`, `Rect`

## When to Use Which

| Use Case | Component |
|----------|-----------|
| Move/rotate/scale an entity | `Tween` |
| Sequence of movements | `TweenSequence` |
| Animate a float (health, opacity) | `PropertyTween<f32>` |
| Animate a position without Transform | `PropertyTween<Vec2>` |
| Color transitions | `PropertyTween<Color>` |
| Custom interpolatable type | `PropertyTween<T>` with manual `update(dt)` |

## See Also

- [Animation System](./animation.md) — Sprite frame animations
- [ECS Architecture](./ecs.md) — Understanding components and entities
