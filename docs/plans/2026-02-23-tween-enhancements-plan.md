# Tween Enhancements Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add generic property tweening (`PropertyTween<T>`) and callback progress arguments to complete the tween system.

**Architecture:** New `PropertyTween<T: Lerp>` ECS component with dedicated system, independent from existing `Tween`. Breaking change to `TweenCallback` signature from `Fn()` to `Fn(f32)` for progress parameter.

**Tech Stack:** Rust, hecs ECS, existing `Lerp` trait from `src/core/ui/tween.rs`, existing `Ease`/`TweenLoop` enums.

---

## Task 1: Change TweenCallback to Fn(f32)

This is the breaking change. Do it first so all subsequent code uses the new signature.

**Files:**
- Modify: `src/core/ecs/tween.rs`
- Modify: `src/core/ui/tween.rs`
- Modify: `examples/demos/tweening/main.rs`
- Modify: `tests/integration/ui_tween.rs`

**Step 1: Update ECS TweenCallback type and builder signatures**

In `src/core/ecs/tween.rs`:

Line 271 — change the type alias:
```rust
// Before:
pub type TweenCallback = Arc<dyn Fn() + Send + Sync>;
// After:
pub type TweenCallback = Arc<dyn Fn(f32) + Send + Sync>;
```

Lines 575-607 — change all three builder method trait bounds from `Fn()` to `Fn(f32)`:
```rust
// on_start (line 578):
F: Fn(f32) + Send + Sync + 'static,

// on_update (line 589):
F: Fn(f32) + Send + Sync + 'static,

// on_complete (line 601):
F: Fn(f32) + Send + Sync + 'static,
```

**Step 2: Update ECS callback invocation sites to pass progress**

In `src/core/ecs/tween.rs`, in `Tween::update()`:

Lines 740-742 — on_start after delay (progress is 0.0 at start):
```rust
if let Some(ref cb) = self.on_start {
  cb(0.0);
}
```

Lines 751-753 — on_start without delay (progress is 0.0 at start):
```rust
if let Some(ref cb) = self.on_start {
  cb(0.0);
}
```

Lines 793-795 — on_update (pass the `eased` variable computed on line 773):
```rust
if let Some(ref cb) = self.on_update {
  cb(eased);
}
```

Lines 826-830 — on_complete (progress is 1.0 when complete):
```rust
if self.just_finished
  && let Some(ref cb) = self.on_complete
{
  cb(1.0);
}
```

**Step 3: Update UiTweenCallback type and builder signatures**

In `src/core/ui/tween.rs`:

Line 104 — change the type alias:
```rust
// Before:
pub type UiTweenCallback = Arc<dyn Fn() + Send + Sync>;
// After:
pub type UiTweenCallback = Arc<dyn Fn(f32) + Send + Sync>;
```

Lines 186-192 — on_start builder bound:
```rust
F: Fn(f32) + Send + Sync + 'static,
```

Lines 198-204 — on_complete builder bound:
```rust
F: Fn(f32) + Send + Sync + 'static,
```

**Step 4: Update UiTween callback invocation sites**

In `src/core/ui/tween.rs`:

Lines 227-229 — on_start after delay:
```rust
if let Some(ref cb) = self.on_start {
  cb(0.0);
}
```

Lines 237-239 — on_start without delay:
```rust
if let Some(ref cb) = self.on_start {
  cb(0.0);
}
```

Lines 267, 273, 280, 291 — on_complete invocations. Each is `cb();` inside a match arm for loop modes. Pass `1.0` for all (completion of a cycle):
```rust
// Each cb() call becomes:
cb(1.0);
```

Also update the doc comment example on line 120:
```rust
///     .on_complete(|_| println!("Done!"));
```

**Step 5: Fix all existing callback closures**

Every `|| { ... }` callback in the codebase needs to become `|_| { ... }`.

In `src/core/ecs/tween.rs` tests — search for `.on_start(`, `.on_update(`, `.on_complete(` and add `|_|` parameter. Lines ~1424, 1444, 1464, 1484, 1506, 1542.

In `src/core/ui/tween.rs` tests — lines ~605, 623, 747, 770.

In `examples/demos/tweening/main.rs` — lines ~493-501 (three callbacks with `move ||` become `move |_|`).

In `tests/integration/ui_tween.rs` — lines ~260, 283.

**Step 6: Run tests**

```bash
cargo test --all-features 2>&1 | tail -20
```

Expected: All 2091+ tests pass, 0 failures.

**Step 7: Run clippy**

```bash
cargo clippy --all-targets --all-features 2>&1 | grep "^error"
```

Expected: No errors.

**Step 8: Commit**

```bash
git add src/core/ecs/tween.rs src/core/ui/tween.rs examples/demos/tweening/main.rs tests/integration/ui_tween.rs
git commit -m "feat(tween): add progress parameter to tween callbacks

BREAKING: TweenCallback signature changed from Fn() to Fn(f32).
Existing callbacks need |_| prefix: .on_complete(|| {}) -> .on_complete(|_| {})"
```

---

## Task 2: Move Lerp trait to shared location and add Vec2 impl

The `Lerp` trait currently lives in `src/core/ui/tween.rs`. `PropertyTween<T>` needs it too, so it needs to be accessible from the ECS module.

**Files:**
- Modify: `src/core/ui/tween.rs` (remove Lerp trait + impls, re-export)
- Create: `src/core/ecs/lerp.rs`
- Modify: `src/core/ecs/mod.rs` (add module, export)
- Modify: `src/lib.rs` (update prelude export path)

**Step 1: Create src/core/ecs/lerp.rs with the Lerp trait**

Move the trait and impls from `src/core/ui/tween.rs` lines 43-87, add a `Vec2` impl:

```rust
use crate::core::ui::Color;
use glam::Vec2;

/// Trait for types that can be linearly interpolated.
///
/// Used by `PropertyTween<T>` and `UiTween<T>` for generic value animation.
pub trait Lerp: Clone + Send + Sync + 'static {
  fn lerp(start: &Self, end: &Self, t: f32) -> Self;
}

impl Lerp for f32 {
  fn lerp(start: &Self, end: &Self, t: f32) -> Self {
    start + (end - start) * t
  }
}

impl Lerp for (f32, f32) {
  fn lerp(start: &Self, end: &Self, t: f32) -> Self {
    (
      f32::lerp(&start.0, &end.0, t),
      f32::lerp(&start.1, &end.1, t),
    )
  }
}

impl Lerp for Vec2 {
  fn lerp(start: &Self, end: &Self, t: f32) -> Self {
    Vec2::new(
      f32::lerp(&start.x, &end.x, t),
      f32::lerp(&start.y, &end.y, t),
    )
  }
}

impl Lerp for Color {
  fn lerp(start: &Self, end: &Self, t: f32) -> Self {
    Color {
      r: (start.r as f32 + (end.r as f32 - start.r as f32) * t).round() as u8,
      g: (start.g as f32 + (end.g as f32 - start.g as f32) * t).round() as u8,
      b: (start.b as f32 + (end.b as f32 - start.b as f32) * t).round() as u8,
      a: (start.a as f32 + (end.a as f32 - start.a as f32) * t).round() as u8,
    }
  }
}
```

Note: Check the exact `Color` struct field names — they may be `r, g, b, a` or something else. Match whatever `src/core/ui/tween.rs` lines 67-76 use.

**Step 2: Update src/core/ecs/mod.rs**

Add the new module and export:
```rust
mod lerp;
pub use lerp::Lerp;
```

**Step 3: Update src/core/ui/tween.rs**

Remove the `Lerp` trait definition and all impls (lines 43-87). Replace with:
```rust
use crate::core::ecs::Lerp;
```

The rest of `UiTween<T>` should still work since it uses `T: Lerp`.

**Step 4: Update src/lib.rs prelude**

The `Lerp` export currently comes from `crate::core::ui`. Move it to come from `crate::core::ecs` instead. In the ECS exports block (lines 69-75), add `Lerp`. Remove it from the UI exports block (lines 196-199).

**Step 5: Run tests**

```bash
cargo test --all-features 2>&1 | tail -20
```

Expected: All tests pass — Lerp behaves identically, just lives in a different module.

**Step 6: Commit**

```bash
git add src/core/ecs/lerp.rs src/core/ecs/mod.rs src/core/ui/tween.rs src/lib.rs
git commit -m "refactor(ecs): move Lerp trait to shared ecs module, add Vec2 impl"
```

---

## Task 3: Implement PropertyTween\<T\> component

The core generic tween component. No system yet — just the data structure, builder API, update logic, and unit tests.

**Files:**
- Create: `src/core/ecs/property_tween.rs`
- Modify: `src/core/ecs/mod.rs` (add module, export)
- Modify: `src/lib.rs` (add prelude export)

**Step 1: Write failing tests first**

Create `src/core/ecs/property_tween.rs` with a `#[cfg(test)] mod tests` block at the bottom. Write these tests (they will fail to compile initially since PropertyTween doesn't exist yet):

```rust
#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn new_property_tween_starts_playing() {
    let tween = PropertyTween::new(0.0f32, 100.0, 1.0);
    assert_eq!(tween.value(), 0.0);
    assert!(tween.playing());
    assert!(!tween.is_finished());
  }

  #[test]
  fn value_interpolates_linearly() {
    let mut tween = PropertyTween::new(0.0f32, 100.0, 1.0);
    tween.update(0.5); // halfway
    assert!((tween.value() - 50.0).abs() < 0.01);
  }

  #[test]
  fn value_with_easing() {
    let mut tween = PropertyTween::new(0.0f32, 100.0, 1.0)
      .with_easing(Ease::InQuad);
    tween.update(0.5);
    // InQuad at t=0.5 => 0.25, so value = 25.0
    assert!((tween.value() - 25.0).abs() < 0.01);
  }

  #[test]
  fn completes_after_duration() {
    let mut tween = PropertyTween::new(0.0f32, 100.0, 1.0);
    tween.update(1.0);
    assert!(tween.is_finished());
    assert!(!tween.playing());
    assert!((tween.value() - 100.0).abs() < 0.01);
  }

  #[test]
  fn delay_postpones_start() {
    let mut tween = PropertyTween::new(0.0f32, 100.0, 1.0)
      .with_delay(0.5);
    tween.update(0.3);
    assert_eq!(tween.value(), 0.0); // still in delay
    tween.update(0.7); // 0.5 delay + 0.5 into tween
    assert!((tween.value() - 50.0).abs() < 0.01);
  }

  #[test]
  fn loop_mode_restarts() {
    let mut tween = PropertyTween::new(0.0f32, 100.0, 1.0)
      .with_loop(TweenLoop::Loop);
    tween.update(1.5); // past end, should loop
    assert!(tween.playing());
    // Value wraps — should be ~50% into second iteration
    assert!((tween.value() - 50.0).abs() < 1.0);
  }

  #[test]
  fn ping_pong_reverses() {
    let mut tween = PropertyTween::new(0.0f32, 100.0, 1.0)
      .with_loop(TweenLoop::PingPong);
    tween.update(1.0); // end of forward
    tween.update(0.5); // halfway back
    assert!((tween.value() - 50.0).abs() < 1.0);
  }

  #[test]
  fn speed_multiplier() {
    let mut tween = PropertyTween::new(0.0f32, 100.0, 1.0)
      .with_speed(2.0);
    tween.update(0.25); // 0.25 * 2.0 = 0.5 effective
    assert!((tween.value() - 50.0).abs() < 0.01);
  }

  #[test]
  fn pause_and_resume() {
    let mut tween = PropertyTween::new(0.0f32, 100.0, 1.0);
    tween.update(0.3);
    let paused_val = tween.value();
    tween.pause();
    tween.update(0.5); // should not advance
    assert!((tween.value() - paused_val).abs() < 0.001);
    tween.resume();
    tween.update(0.2);
    assert!(tween.value() > paused_val);
  }

  #[test]
  fn retarget_changes_destination() {
    let mut tween = PropertyTween::new(0.0f32, 100.0, 1.0);
    tween.update(0.5); // at 50.0
    tween.retarget(200.0); // now heading to 200 from current
    tween.update(0.5); // completes
    assert!((tween.value() - 200.0).abs() < 0.01);
  }

  #[test]
  fn progress_reports_correctly() {
    let mut tween = PropertyTween::new(0.0f32, 100.0, 1.0);
    tween.update(0.3);
    assert!((tween.progress() - 0.3).abs() < 0.01);
  }

  #[test]
  fn vec2_lerp() {
    let mut tween = PropertyTween::new(
      Vec2::ZERO,
      Vec2::new(100.0, 200.0),
      1.0,
    );
    tween.update(0.5);
    let v = tween.value();
    assert!((v.x - 50.0).abs() < 0.01);
    assert!((v.y - 100.0).abs() < 0.01);
  }

  #[test]
  fn callbacks_receive_progress() {
    let progress_at_update = Arc::new(std::sync::Mutex::new(0.0f32));
    let p = progress_at_update.clone();

    let mut tween = PropertyTween::new(0.0f32, 100.0, 1.0)
      .on_update(move |progress| {
        *p.lock().unwrap() = progress;
      });
    tween.update(0.5);
    assert!((*progress_at_update.lock().unwrap() - 0.5).abs() < 0.01);
  }

  #[test]
  fn just_started_and_just_finished_flags() {
    let mut tween = PropertyTween::new(0.0f32, 100.0, 1.0);
    tween.update(0.01);
    assert!(tween.just_started());
    tween.update(0.01);
    assert!(!tween.just_started());
    tween.update(1.0);
    assert!(tween.just_finished());
    tween.update(0.01);
    assert!(!tween.just_finished());
  }
}
```

**Step 2: Implement PropertyTween\<T\>**

Above the tests in the same file, implement the struct with:
- All fields from the design (start, end, duration, elapsed, delay, easing, loop_mode, speed, playing, started, just_started, just_finished, ping_pong_reverse, loop_count, callbacks)
- `new(start, end, duration)` constructor
- Builder methods: `with_easing`, `with_delay`, `with_loop`, `with_speed`, `on_start`, `on_update`, `on_complete`
- `update(dt: f32)` method — same timing/easing/looping logic as `Tween::update()` but computes `T::lerp(start, end, eased)` instead of applying to Transform
- `value() -> T` — returns current interpolated value
- `progress() -> f32` — returns raw progress 0.0-1.0
- `playing()`, `is_finished()`, `just_started()`, `just_finished()` getters
- `pause()`, `resume()`, `stop()`, `restart()` control methods
- `retarget(new_end: T)` — resets start to current value, resets elapsed to 0, sets new end

Reference `Tween::update()` at `src/core/ecs/tween.rs:711-832` for the timing/looping logic. Replicate the same patterns (delay handling, overshoot, loop modes, ping-pong).

**Step 3: Export from ecs module**

In `src/core/ecs/mod.rs`, add:
```rust
mod property_tween;
pub use property_tween::PropertyTween;
```

**Step 4: Add to prelude**

In `src/lib.rs`, add `PropertyTween` to the ECS exports block (lines 69-75).

**Step 5: Run tests**

```bash
cargo test --all-features -p ryo2d property_tween 2>&1 | tail -20
```

Expected: All PropertyTween tests pass.

```bash
cargo test --all-features 2>&1 | grep "^test result"
```

Expected: All tests pass.

**Step 6: Run clippy**

```bash
cargo clippy --all-targets --all-features 2>&1 | grep "^error"
```

Expected: No errors.

**Step 7: Commit**

```bash
git add src/core/ecs/property_tween.rs src/core/ecs/mod.rs src/lib.rs
git commit -m "feat(tween): add generic PropertyTween<T> component"
```

---

## Task 4: Implement PropertyTweenSystem

The system that drives PropertyTween components each frame, emitting events.

**Files:**
- Modify: `src/core/systems/tween.rs` (add PropertyTweenSystem or extend TweenSystem)
- Modify: `src/core/engine.rs` (wire up the system call)

**Step 1: Add PropertyTween system update to TweenSystem**

In `src/core/systems/tween.rs`, add a new method and call it from `update()`:

```rust
pub fn update(world: &mut World, events: &mut Events, dt: f32) {
  Self::update_tweens(world, events, dt);
  Self::update_tween_sequences(world, events, dt);
  Self::update_property_tweens(world, events, dt);  // NEW
}
```

The new method iterates all entities with any `PropertyTween<T>`:

**Challenge:** Since `PropertyTween<T>` is generic, we can't query for all T at once. We need one query per concrete T that users might use.

**Solution:** Query for the common types explicitly: `PropertyTween<f32>`, `PropertyTween<Vec2>`, `PropertyTween<Color>`. For user-defined types, they call `PropertyTween::update(dt)` manually in their scene update (the component has a public `update` method).

```rust
fn update_property_tweens(world: &mut World, events: &mut Events, dt: f32) {
  // f32 property tweens
  for (entity, tween) in world.query_mut::<(Entity, &mut PropertyTween<f32>)>() {
    if world.has_component::<Inactive>(entity) {
      continue;
    }
    tween.update(dt);
    if tween.just_started() {
      events.emit(TweenStarted { entity });
    }
    if tween.just_finished() {
      events.emit(TweenCompleted {
        entity,
        finished: !tween.playing(),
      });
    }
  }

  // Vec2 property tweens
  for (entity, tween) in world.query_mut::<(Entity, &mut PropertyTween<Vec2>)>() {
    if world.has_component::<Inactive>(entity) {
      continue;
    }
    tween.update(dt);
    if tween.just_started() {
      events.emit(TweenStarted { entity });
    }
    if tween.just_finished() {
      events.emit(TweenCompleted {
        entity,
        finished: !tween.playing(),
      });
    }
  }

  // Color property tweens
  for (entity, tween) in world.query_mut::<(Entity, &mut PropertyTween<Color>)>() {
    if world.has_component::<Inactive>(entity) {
      continue;
    }
    tween.update(dt);
    if tween.just_started() {
      events.emit(TweenStarted { entity });
    }
    if tween.just_finished() {
      events.emit(TweenCompleted {
        entity,
        finished: !tween.playing(),
      });
    }
  }
}
```

Note: Extract the inner loop body into a helper function to avoid repetition:

```rust
fn drive_property_tween<T: Lerp>(
  entity: Entity,
  tween: &mut PropertyTween<T>,
  events: &mut Events,
  dt: f32,
) {
  tween.update(dt);
  if tween.just_started() {
    events.emit(TweenStarted { entity });
  }
  if tween.just_finished() {
    events.emit(TweenCompleted {
      entity,
      finished: !tween.playing(),
    });
  }
}
```

**Step 2: Verify engine.rs call site**

The existing call at `src/core/engine.rs:1260` already calls `TweenSystem::update(...)` which now includes `update_property_tweens`. No change needed in engine.rs.

**Step 3: Run tests**

```bash
cargo test --all-features 2>&1 | grep "^test result"
```

Expected: All tests pass.

**Step 4: Commit**

```bash
git add src/core/systems/tween.rs
git commit -m "feat(tween): add PropertyTweenSystem for f32/Vec2/Color property tweens"
```

---

## Task 5: Update RESEARCH.md and final verification

**Files:**
- Modify: `RESEARCH.md`

**Step 1: Update RESEARCH.md**

Change the Tweening section status. Both remaining gaps are now implemented:
- Generic property tweening: `PropertyTween<T>`
- Callbacks with arguments: `TweenCallback` now passes `f32` progress

Mark the section as COMPLETE and remove it (consistent with previous cleanup).

Update the summary table to remove the Tweening row or mark it complete.

**Step 2: Full test suite**

```bash
cargo fmt --check
cargo clippy --all-targets --all-features 2>&1 | grep "^error"
cargo test --all-features 2>&1 | grep "^test result"
```

Expected: All clean.

**Step 3: Commit**

```bash
git add RESEARCH.md
git commit -m "docs(research): mark tweening enhancements as complete"
```

Note: RESEARCH.md is gitignored — if commit fails, skip this step (local-only file).
