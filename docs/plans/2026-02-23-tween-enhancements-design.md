# Tween Enhancements Design

**Goal:** Complete the two remaining tween system gaps — generic property tweening and callback arguments.

**Architecture:** Add a new `PropertyTween<T>` generic ECS component with its own system, independent from the existing `Tween`. Change `TweenCallback` signature from `Fn()` to `Fn(f32)` to pass progress.

---

## Deliverable 1: PropertyTween\<T\>

### Component

A new generic ECS component that interpolates any `Lerp`-implementing type between start and end values. Independent from existing `Tween` (which continues handling Transform/Renderable).

```rust
pub struct PropertyTween<T: Lerp + Send + Sync + 'static> {
    start: T,
    end: T,
    duration: f32,
    elapsed: f32,
    delay: f32,
    easing: Ease,
    loop_mode: TweenLoop,
    speed: f32,
    playing: bool,
    started: bool,
    just_started: bool,
    just_finished: bool,
    ping_pong_reverse: bool,
    loop_count: u32,
    on_start: Option<TweenCallback>,
    on_update: Option<TweenCallback>,
    on_complete: Option<TweenCallback>,
}
```

### API

```rust
// Construction
PropertyTween::new(0.0f32, 100.0, 1.0)
PropertyTween::new(Vec2::ZERO, Vec2::new(10.0, 5.0), 2.0)
PropertyTween::new(Color::RED, Color::BLUE, 0.5)

// Builder (mirrors Tween API)
.with_easing(Ease::OutQuad)
.with_delay(0.5)
.with_loop(TweenLoop::PingPong)
.with_speed(2.0)
.on_start(|_| { ... })
.on_update(|progress| { ... })
.on_complete(|_| { ... })

// Reading current state
.value() -> T           // Current interpolated value
.progress() -> f32      // Raw 0.0-1.0 progress

// Runtime control
.pause(), .resume(), .stop(), .restart()
.retarget(new_end)      // Change destination mid-flight
```

### System

`PropertyTweenSystem` runs each frame alongside existing `TweenSystem`. It:
- Advances elapsed time (respecting delay, speed, loop mode)
- Computes interpolated value via `Lerp::lerp`
- Emits `TweenStarted`/`TweenCompleted` events (reuses existing event types)

### Lerp Trait

Already exists in `src/core/ui/tween.rs` with impls for `f32`, `(f32, f32)`, `Color`, `Rect`. We:
- Move the `Lerp` trait + impls to a shared location (re-export from prelude)
- Add `Vec2` impl

### What This Does NOT Do

Does not auto-apply values to any component. The user queries `PropertyTween<f32>` in their update loop and reads `.value()`.

---

## Deliverable 2: Callback Progress Argument

### Breaking Change

```rust
// Before
pub type TweenCallback = Arc<dyn Fn() + Send + Sync>;

// After
pub type TweenCallback = Arc<dyn Fn(f32) + Send + Sync>;
```

### Impact

- All 3 callback invocation sites in `Tween::update()` pass eased progress
- All existing callbacks in demos/tests get `|_|` prefix
- Both `Tween` and `PropertyTween<T>` use the same callback type

---

## Decisions

- **Value tween pattern** over getter/setter closures — user reads `.value()` and applies it themselves
- **Progress only** in callbacks (not entity) — entity can be captured in closure if needed
- **Breaking change** to callback signature — engine is pre-1.0, fix is trivial (`||` -> `|_|`)
- **Standalone component** — no coupling to existing Tween, no risk of regression
