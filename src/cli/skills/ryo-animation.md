---
name: ryo-animation
description: Sprite animations (Animator, AnimationController), tweens, knockback and squash/stretch.
metadata:
  version: 2.0.0
---

# Ryo2D Animation Specialist

You help users animate games: sprite frame animation, from simple loops to character state machines, and the "juice" components that move, scale, fade and push entities.

**For complete API signatures, see the `ryo-api` skill.**

## How it runs

Every animation component here is driven by the engine once per fixed step with the fixed timestep, in `FixedSet::Juice`, before the scene's `update` in the same step. Knockback is the exception: it runs in `FixedSet::Movement`, after `update` and just before physics, since physics applies it. So:

- Slow motion (`ctx.time.game.set_speed(0.5)`) slows them; pause and hitstop freeze them.
- A flag such as `just_finished` set in a step is visible to that step's `update` (knockback's, and its messages, to the next step's).
- Start and finish are also reported as messages (`TweenStarted`, `TweenCompleted`, `KnockbackStarted`, ...), which a scene reads once with `ctx.messages::<T>()`.

| Component | Driven by the engine | Writes |
|-----------|----------------------|--------|
| `Animator`, `AnimationController`, `ValidatedAnimator` | yes | `Renderable.sprite` |
| `Tween`, `TweenSequence` | yes | `Transform` (position, rotation, scale), `Renderable` alpha and tint |
| `PropertyTween<f32>`, `<Vec2>`, `<Color>` | yes | its own value |
| `PropertyTween<T>` for other `Lerp` types | no: call `update(dt)` | its own value |
| `Knockback` | yes (in `FixedSet::Movement`) | a character's sweep, a dynamic body's `Velocity` (once), else `Transform.translation` |
| `SquashStretch` | yes | `Transform.scale` |

## Sprite animation

### Animation: frames and timing

```rust
// Uniform frame duration; loops by default
let walk = Animation::new("walk", walk_frames, 0.1);

// Per-frame timing and a loop mode
let attack = Animation::with_frame_durations("attack", attack_frames, vec![0.1, 0.15, 0.2])
  .with_loop_mode(LoopMode::Once);
```

`LoopMode`: `Once` (stops on the last frame), `Loop` (default), `PingPong`, `Times(n)`. Frames are `SpriteHandle`s.

### Animator: one animation

```rust
let coin = ctx
  .world
  .spawn((
    Transform::from_xy(100.0, 100.0),
    Renderable::sprite(coin_frames[0].clone()),
    Animator::new(Animation::new("spin", coin_frames, 0.08)),
  ))
  .id();

if let Some(mut animator) = ctx.world.get_mut::<Animator>(coin) {
  animator.pause();
  animator.play();
  animator.stop(); // back to frame 0, stopped
  animator.reset(); // back to frame 0, same play state
  animator.set_speed(2.0);
}
```

Read: `is_playing()`, `just_finished()`, `current_frame_index()`, `animation_name()`, `speed()`. `set_animation(animation)` swaps the animation and restarts it.

### AnimationController: named states

```rust
let mut controller = AnimationController::new();
controller.add_animation("idle", idle);
controller.add_animation("walk", walk);
controller.add_animation("attack", attack.with_loop_mode(LoopMode::Once));
controller.set_state("idle");

let player = ctx
  .world
  .spawn((
    Transform::from_xy(100.0, 100.0),
    Renderable::sprite(idle_frames[0].clone()),
    controller,
  ))
  .id();
```

State names are `AnimationId` (`&'static str`). Setting the current state again does not restart it.

```rust
fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
  if let Some(mut anim) = ctx.world.get_mut::<AnimationController>(self.player) {
    if ctx.input.key_pressed(KeyCode::Space) {
      // A one-shot that returns to idle when it finishes
      anim.set_state("attack");
      anim.transition_after_finish("idle");
    } else if anim.current_state() != Some("attack") {
      let moving = ctx.input.key_down(KeyCode::KeyA) || ctx.input.key_down(KeyCode::KeyD);
      anim.set_state(if moving { "walk" } else { "idle" });
    }
  }
  SceneAction::Continue
}
```

Read: `current_state() -> Option<AnimationId>`, `just_finished()` (true for the step a `Once`/`Times` animation completes). Control: `pause`, `play`, `reset`, `set_speed`.

### ValidatedAnimator: enforced transitions

An `AnimationController` behind a `StateMachine` that only allows declared transitions (`with_animation`, `with_transition`, `with_bidirectional`, `with_transition_from_any`; `set_animation(id) -> bool`, `force_animation`). On an entity the engine drives it like `Animator`, putting its frame on the `Renderable`; one kept outside the world needs `update(dt)` and `current_sprite()` by hand.

### Facing

Flip instead of drawing left/right variants:

```rust
if let Some(mut renderable) = ctx.world.get_mut::<Renderable>(self.player) {
  if dx < 0.0 {
    renderable.flip_x = true;
  } else if dx > 0.0 {
    renderable.flip_x = false;
  }
}
```

## Tween: transform and colour

`Tween` animates `Transform` translation, rotation (radians) and scale, and `Renderable` alpha and tint. Start values are captured when the tween starts.

```rust
use std::f32::consts::PI;

// Move with a bounce
ctx.world.spawn((
  Transform::from_xy(100.0, 100.0),
  Renderable::sprite(sprite.clone()),
  Tween::to_position(400.0, 300.0, 2.0).with_easing(Ease::OutBounce),
));

// Pulse forever
ctx.world.spawn((
  Transform::from_xy(300.0, 300.0),
  Renderable::sprite(sprite.clone()),
  Tween::to_scale(1.2, 1.2, 0.3)
    .with_easing(Ease::InOutSine)
    .with_loop(TweenLoop::PingPong),
));

// Several targets, after a delay
ctx.world.spawn((
  Transform::IDENTITY,
  Renderable::sprite(sprite.clone()),
  Tween::new(1.5)
    .to_x(200.0)
    .to_y(100.0)
    .to_rotation_value(PI / 4.0)
    .with_easing(Ease::OutCubic)
    .with_delay(0.5),
));

// Fade out (needs a Renderable; works without a Transform too)
ctx.world.entity_mut(ghost).insert(Tween::fade_out(0.4));
```

Constructors: `new(duration)`, `to_position(x, y, d)`, `to_rotation(radians, d)`, `to_scale(sx, sy, d)`, `to_uniform_scale(s, d)`, `fade_to(alpha, d)`, `fade_in(d)`, `fade_out(d)`, `tint_to(color, d)`. Builders: `to_x`, `to_y`, `to_rotation_value`, `to_scale_x`, `to_scale_y`, `to_alpha`, `to_tint`, `with_easing`, `with_delay`, `with_loop`, `with_speed`. Control: `pause`, `resume`, `stop`, `restart`, `set_speed`; read `progress()`, `is_finished()`.

Callbacks receive the eased progress and must be `Send + Sync`:

```rust
Tween::to_position(100.0, 100.0, 1.0)
  .on_start(|_| game_info!("started"))
  .on_update(|t| game_trace!("at {t}"))
  .on_complete(|_| game_info!("done"));
```

### Completion

Read the messages; `finished` is `false` for the end of one loop iteration and `true` when the tween is done (`Once`, or the last of `Times(n)`):

```rust
for done in ctx.messages::<TweenCompleted>() {
  if done.entity == self.door && done.finished {
    self.door_open = true;
  }
}
```

### Easing and loops

| Family | Variants |
|--------|----------|
| Linear | `Linear` (default) |
| Quad, Cubic, Quart | `InQuad`, `OutQuad`, `InOutQuad`, ... |
| Sine, Expo | `InSine`, `OutSine`, `InOutSine`, `InExpo`, ... |
| Back (overshoot) | `InBack`, `OutBack`, `InOutBack` |
| Elastic | `InElastic`, `OutElastic`, `InOutElastic` |
| Bounce | `InBounce`, `OutBounce`, `InOutBounce` |

`Ease::apply(t)` evaluates a curve. `TweenLoop`: `Once` (default), `Loop`, `PingPong`, `Times(n)`.

### TweenSequence: one after another

```rust
ctx.world.spawn((
  Transform::from_xy(100.0, 100.0),
  Renderable::sprite(sprite.clone()),
  TweenSequence::new()
    .then(Tween::to_position(200.0, 100.0, 0.5).with_easing(Ease::OutQuad))
    .then(Tween::to_position(200.0, 50.0, 0.3))
    .then(Tween::fade_out(0.2)),
));
```

`with_loop()` repeats the whole sequence; `TweenCompleted { finished: true }` is written when it ends.

## PropertyTween<T>: any value

Interpolates any `Lerp` type (`f32`, `(f32, f32)`, `Vec2`, `Color`, `Rect`, or your own) and stores the value; read it with `value()`.

```rust
let bar = ctx
  .world
  .spawn(PropertyTween::new(0.0_f32, 100.0, 2.0).with_easing(Ease::OutQuad))
  .id();

// later
if let Some(tween) = ctx.world.get::<PropertyTween<f32>>(bar) {
  self.health_shown = tween.value();
}
```

Control: `pause`, `resume`, `stop`, `restart`, `retarget(new_end)`; read `progress()`, `playing()`, `is_finished()`, `just_started()`, `just_finished()`. The engine drives `f32`, `Vec2` and `Color`; for other types call `tween.update(dt)` yourself, for example from a scene system.

## Knockback

```rust
// Push an enemy away from the player (adds a Knockback if it has none)
let dir = enemy_pos - player_pos;
Knockback::apply(ctx.world, enemy, dir, 400.0);
Knockback::apply_with_decay(ctx.world, enemy, dir, 400.0, 8.0); // faster recovery

// On a component you already hold
if let Some(mut kb) = ctx.world.get_mut::<Knockback>(enemy) {
  kb.push(Vec2::new(-1.0, 0.0), 300.0);
}
```

The direction is normalised; force is pixels per second at full strength and decays as strength squared (`DEFAULT_DECAY_RATE` 4.0, about a quarter second). `with_accumulate(true)` blends a new push into an active one. What it does depends on the target. A character (`CharacterController`) is swept by it, so walls, floors and ceilings stop it and the part running into them is dropped. A dynamic body takes each hit as one change in velocity of its own (the same speed whatever its mass) and physics slows it from there. A kinematic body without a controller and a fixed body ignore it (logged once). Anything without a body, including an entity whose `Velocity` your own code integrates, is moved through its `Transform`. The movement presets steer at their `knockback_control` (0 by default) while it lasts. Messages: `KnockbackStarted`, `KnockbackFinished`, readable in the next step's `update`. `stop()` ends it at once and drops a hit not yet given to a dynamic body. Per target in detail: `ryo-character`.

## Squash and stretch

```rust
SquashStretch::apply(ctx.world, player, 0.3); // land: squash on Y
SquashStretch::apply_x(ctx.world, player, 0.25); // wall hit: squash on X
SquashStretch::apply_on_axis(ctx.world, player, 0.3, SquashAxis::Y);
SquashStretch::apply_with_decay(ctx.world, player, 0.4, 3.0); // slow, wobbly

// Full control
ctx.world.entity_mut(player).insert(
  SquashStretch::new(0.4)
    .with_axis(SquashAxis::X)
    .with_decay_rate(6.0)
    .with_preserve_volume(true),
);
```

The effect deforms `Transform.scale` and recovers to the base scale, read from the entity when an effect starts from rest (or fixed with `with_base_scale`). Component methods: `squash`, `squash_with_decay`, `squash_on_axis`. Messages: `SquashStretchStarted`, `SquashStretchFinished`. Scaling happens about the sprite's anchor (`Anchor::Center` by default), so a character meant to squash onto the floor uses `Renderable::with_anchor(Anchor::BottomCenter)` with its origin at the feet.

## Tips

- `Animator` for decoration, `AnimationController` for characters, `Tween` for movement and fades, `PropertyTween` for numbers and colours.
- Detect completion with the messages rather than polling flags.
- A tween and squash/stretch both writing `Transform.scale` on one entity fight each other; put one on a child entity.
