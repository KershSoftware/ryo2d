# Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rebuild Ryo2D's foundation on `bevy_ecs` 0.20 (exposed as `ryo2d::ecs`), a three-clock time model with one frame loop, and a 2D `Transform`/`GlobalTransform` pair over the built-in hierarchy, then migrate the engine, tests, demos and games onto it.

**Architecture:** The engine owns one `World` and six phase schedules (`First`, `PreUpdate`, `FixedUpdate`, `Update`, `PostUpdate`, `Last`). Engine systems and game systems share those schedules; scenes keep the `Scene` trait and may own schedules of their own that the engine runs inside the `Game` sets. `Time` holds real, game and fixed clocks; the fixed delta is constant and the step count varies. `Transform` is local, `GlobalTransform` is an `Affine2` written only by change-detected propagation, and interpolation blends previous and current globals at render time.

**Tech Stack:** Rust 2024 edition, `bevy_ecs = "0.20"` (standalone, single-threaded executor), `glam` 0.31 (`Affine2`, `Vec2`), existing `wgpu` 22 / `winit` 0.30 / `rapier2d` 0.22 / `kira` 0.11 unchanged.

**Spec:** `docs/specs/2026-10-11-foundation-design.md`

## Global Constraints

- Branch `foundation` in a git worktree; `main` stays green until the final merge (spec: Migration).
- `bevy_ecs` version `0.20`, pinned as `"0.20"` (spec: Risks). Game code never writes `bevy_ecs::`; the crate name appears only in `Cargo.toml`, `src/core/ecs/kernel.rs` and the prelude re-export that lets derives resolve (spec: Workspace layout).
- Dependency arrows point only toward the core: nothing in `src/` references `crates/` (spec: Workspace layout).
- Every schedule uses `SingleThreadedExecutor`; systems declare their real access (spec: Phases).
- Rotation is radians everywhere; degree helpers carry `degrees` in their name (spec: Transform).
- Default sprite anchor is `Anchor::Center` (spec: Transform).
- `Scene::update`'s `dt` is the constant fixed timestep; default fixed rate 60 Hz; `max_delta` 250 ms (spec: Time).
- Commit messages follow `type: brief description` with no co-author lines and no mention of AGENTS.md files (root `AGENTS.md`).
- LF line endings; two-space indentation per `rustfmt.toml`.
- No `unwrap`/`expect` in library code outside tests except for invariants documented at the call site.

## Review Focus

- `EngineBuilder::fixed_hz(0)` and `GameClock::set_speed(f32::NAN)` or a negative speed must not divide by zero or poison the clock: fixed rate is clamped to at least 1 Hz, speed to a finite value at least 0. Test pinned to Task 2.
- A `Transform` entity whose parent has no `Transform` (a child of a bare grouping entity) must propagate as if the parent were the identity rather than being skipped or panicking. Test pinned to Task 4.
- A message written during the scene UI pass (before the fixed loop) must be readable by systems and scenes in the next frame, not lost. Test pinned to Task 8.
- Entities spawned through `Commands` inside a scene-owned system must exist (be flushed) before that frame renders. Test pinned to Task 6.
- `Pool::acquire` after a scene cleanup despawned the pooled entities must not hand out dead entities; the pool purges them and refills per its policy. Test pinned to Task 5.

---

## Phase 0: branch and spike

### Task 0: Create the `foundation` worktree

**Files:** none in the repo.

- [ ] **Step 1: Create the worktree and branch**

Use the `superpowers:using-git-worktrees` skill. Branch name `foundation`, based on `main` at commit `ac2ad72` or later. All later tasks run inside that worktree.

- [ ] **Step 2: Verify the baseline is green**

Run: `cargo test --lib 2>&1 | tail -3`
Expected: `test result: ok. 1948 passed` (counts may differ slightly if `main` moved; all must pass).

### Task 1: Add `bevy_ecs` and the `ryo2d::ecs` export surface, prove derives resolve downstream

**Files:**
- Modify: `Cargo.toml` (add dependency)
- Create: `src/core/ecs/kernel.rs`
- Modify: `src/core/ecs/mod.rs` (add `pub mod kernel;`)
- Modify: `src/lib.rs` (add `pub mod ecs` alias and prelude lines)
- Create (throwaway, outside the repo): `<scratchpad>/derive-spike/`

**Interfaces:**
- Produces: `crate::core::ecs::kernel` re-exporting the bevy types under Ryo2D names; `ryo2d::ecs` as the public path (`pub use crate::core::ecs;` at the crate root is added in Task 9 when the old `ecs` module is replaced; for now only `kernel` is public under `core::ecs`).

- [ ] **Step 1: Add the dependency**

In `Cargo.toml` under `[dependencies]`, after `hecs`:

```toml
bevy_ecs = { version = "0.20", default-features = false, features = ["std", "backtrace"] }
```

Run: `cargo check 2>&1 | tail -3`
Expected: `Finished` (if `bevy_ecs` fails to build with these features, use `bevy_ecs = "0.20"` with default features and note it in the commit message).

- [ ] **Step 2: Write the kernel re-export module**

`src/core/ecs/kernel.rs`:

```rust
//! Ryo2D's ECS kernel: the entity-component types the engine is built on.
//!
//! Game code reaches these through `ryo2d::ecs` and the prelude. The crate
//! that implements them is a dependency detail; nothing outside this file and
//! `Cargo.toml` names it.

pub use bevy_ecs::bundle::Bundle;
pub use bevy_ecs::change_detection::{DetectChanges, DetectChangesMut, Mut, Ref};
pub use bevy_ecs::component::Component;
pub use bevy_ecs::entity::Entity;
pub use bevy_ecs::entity_disabling::Disabled;
pub use bevy_ecs::error::{BevyError as SystemError, FallbackErrorHandler};
pub use bevy_ecs::event::{EntityEvent, Event};
pub use bevy_ecs::hierarchy::{ChildOf, Children};
pub use bevy_ecs::lifecycle::RemovedComponents;
pub use bevy_ecs::message::{Message, MessageCursor, MessageReader, MessageWriter, Messages};
pub use bevy_ecs::observer::On;
pub use bevy_ecs::query::{Added, Allow, Changed, Has, Or, With, Without};
pub use bevy_ecs::resource::Resource;
pub use bevy_ecs::schedule::{
  IntoScheduleConfigs, Schedule, ScheduleLabel, SingleThreadedExecutor, SystemSet,
};
pub use bevy_ecs::system::{
  Commands, EntityCommands, IntoSystem, Query, Res, ResMut, System,
};
pub use bevy_ecs::world::{EntityWorldMut, World};
```

If any path does not exist under that name in 0.20, find it with `cargo doc -p bevy_ecs --no-deps` (open `target/doc/bevy_ecs/index.html`) and fix the path; do not drop the name from the list.

- [ ] **Step 3: Register the module and the prelude lines**

In `src/core/ecs/mod.rs` add `pub mod kernel;` as the first line of the module list.

In `src/lib.rs`, after `pub use wgpu;` add:

```rust
/// The crate behind `ryo2d::ecs`, re-exported under its own name only so the
/// derive macros in the prelude resolve from a game crate. Never name it.
#[doc(hidden)]
pub use bevy_ecs;
```

In the prelude, add (do not yet export `Entity` or `World` from the kernel; the old `ecs` module still exports hecs versions of those):

```rust
  // ECS kernel derives and system types (the rest arrives with the new World)
  pub use crate::core::ecs::kernel::{
    Component, Resource, Message, Event, SystemSet, ScheduleLabel, Query, Res, ResMut, Commands,
    With, Without, Changed, Added,
  };
```

`Component` here is the bevy derive and trait. The old `crate::core::ecs::component::Component` trait is not in the prelude, so there is no clash. If `cargo check` reports an ambiguous glob or name clash, rename the old trait to `LegacyComponent` in `src/core/ecs/component.rs` and its one user `src/core/network/replication/component.rs`.

Run: `cargo check 2>&1 | tail -3`
Expected: `Finished`.

- [ ] **Step 4: Build the derive spike**

Create `<scratchpad>/derive-spike/Cargo.toml`:

```toml
[package]
name = "derive-spike"
version = "0.0.0"
edition = "2024"

[dependencies]
ryo2d = { path = "/Users/mbreckon/Projects/ryo2d" }  # use the worktree path
```

Create `<scratchpad>/derive-spike/src/main.rs`:

```rust
use ryo2d::prelude::*;

#[derive(Component)]
struct Health(f32);

#[derive(Resource, Default)]
struct Score(u32);

#[derive(Message)]
struct Hit {
  entity: Entity,
}

fn damage(mut q: Query<&mut Health>, mut score: ResMut<Score>, mut hits: MessageReader<Hit>) {
  for mut h in &mut q {
    h.0 -= 1.0;
  }
  score.0 += hits.read().count() as u32;
}

fn main() {
  let _ = damage;
  println!("derives resolve");
}
```

`Entity` and `MessageReader` are not in the prelude yet; for the spike only, import them with `use ryo2d::core::ecs::kernel::{Entity, MessageReader};` under the prelude line.

Run: `cd <scratchpad>/derive-spike && cargo build 2>&1 | tail -5`
Expected: `Finished`. The spike proves `#[derive(Component)]`, `#[derive(Resource)]` and `#[derive(Message)]` expand to paths that resolve from a crate that depends only on `ryo2d`.

If it fails with `use of unresolved crate or module bevy_ecs`: the derive expands to a path the prelude glob does not satisfy. Fallback, applied before continuing: in `crates/ryo2d-derive` (still at `ryo2d-derive/` until Task 23) add proc macros `Component`, `Resource`, `Message`, `Event` that emit `#[derive(::ryo2d::bevy_ecs::prelude::Component)]`-style attributes with the path rewritten to `::ryo2d::bevy_ecs`, and export those from the prelude instead of bevy's derives. Record the outcome in the commit message.

- [ ] **Step 5: Delete the spike and commit**

```bash
rm -rf <scratchpad>/derive-spike
git add Cargo.toml Cargo.lock src/core/ecs/kernel.rs src/core/ecs/mod.rs src/lib.rs
git commit -m "feat(ecs): add the ECS kernel re-exports and prove derives resolve downstream"
```

---

## Phase 1: the new core modules (built beside the old ones, each with its own tests)

### Task 2: `Time` with real, game and fixed clocks

**Files:**
- Create: `src/core/time.rs`
- Modify: `src/core/mod.rs` (add `pub mod time;`)
- Test: inline `#[cfg(test)]` in `src/core/time.rs`

**Interfaces:**
- Produces:
  - `pub struct Time { pub real: RealClock, pub game: GameClock, pub fixed: FixedClock, .. }` (`Resource`)
  - `Time::new(fixed_hz: u32) -> Time`
  - `Time::begin_frame(&mut self)` (wall clock), `Time::advance(&mut self, real_delta: Duration)` (explicit)
  - `Time::delta(&self) -> f32`, `Time::frame_count(&self) -> u64`
  - `pub(crate) fn Time::try_expend_fixed_step(&mut self) -> bool`, `pub(crate) fn Time::set_in_fixed(&mut self, in_fixed: bool)`
  - `RealClock::{delta, delta_duration, elapsed}`
  - `GameClock::{delta, delta_duration, elapsed, speed, set_speed, pause, resume, is_paused, hitstop, is_hitstop, max_delta, set_max_delta}`
  - `FixedClock::{timestep, timestep_duration, set_hz, accumulator, overstep_fraction, step_count}`

- [ ] **Step 1: Write the failing tests**

`src/core/time.rs` (tests first; the implementation follows in Step 3 in the same file):

```rust
#[cfg(test)]
mod tests {
  use super::*;
  use std::time::Duration;

  fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
  }

  #[test]
  fn fixed_steps_accumulate_across_uneven_frames() {
    let mut t = Time::new(60);
    t.advance(ms(10));
    assert!(!t.try_expend_fixed_step());
    t.advance(ms(10));
    assert!(t.try_expend_fixed_step());
    assert!(!t.try_expend_fixed_step());
    assert_eq!(t.fixed.step_count(), 1);
  }

  #[test]
  fn max_delta_caps_a_stall() {
    let mut t = Time::new(60);
    t.advance(Duration::from_secs(10));
    let mut steps = 0;
    while t.try_expend_fixed_step() {
      steps += 1;
    }
    assert_eq!(steps, 15); // 250 ms at 60 Hz
    assert!((t.real.delta() - 10.0).abs() < 1e-6, "real clock is never capped");
  }

  #[test]
  fn half_speed_halves_the_step_count() {
    let mut full = Time::new(60);
    let mut half = Time::new(60);
    half.game.set_speed(0.5);
    let (mut a, mut b) = (0, 0);
    for _ in 0..120 {
      full.advance(ms(16));
      half.advance(ms(16));
      while full.try_expend_fixed_step() {
        a += 1;
      }
      while half.try_expend_fixed_step() {
        b += 1;
      }
    }
    assert!((a as f32 / b as f32 - 2.0).abs() < 0.1, "{a} vs {b}");
  }

  #[test]
  fn pause_and_hitstop_run_no_steps_and_hitstop_ends_in_real_time() {
    let mut t = Time::new(60);
    t.game.pause();
    t.advance(ms(100));
    assert!(!t.try_expend_fixed_step());
    assert_eq!(t.game.delta(), 0.0);
    t.game.resume();

    t.game.hitstop(0.05);
    assert!(t.game.is_hitstop());
    t.advance(ms(30));
    assert!(!t.try_expend_fixed_step());
    t.advance(ms(30));
    assert!(!t.game.is_hitstop(), "60 ms of real time passed");
    t.advance(ms(20));
    assert!(t.try_expend_fixed_step());
  }

  #[test]
  fn overstep_fraction_is_in_unit_range() {
    let mut t = Time::new(60);
    t.advance(ms(8));
    let a = t.fixed.overstep_fraction();
    assert!((0.0..1.0).contains(&a) && a > 0.4, "{a}");
    t.advance(ms(8));
    assert!(t.try_expend_fixed_step());
    assert!(t.fixed.overstep_fraction() < 0.05);
  }

  #[test]
  fn delta_is_the_timestep_inside_fixed_and_the_game_delta_outside() {
    let mut t = Time::new(50);
    t.advance(ms(30));
    assert!((t.delta() - 0.030).abs() < 1e-6);
    t.set_in_fixed(true);
    assert!((t.delta() - 0.020).abs() < 1e-6);
    t.set_in_fixed(false);
    assert!((t.delta() - 0.030).abs() < 1e-6);
  }

  #[test]
  fn frame_count_and_elapsed_advance() {
    let mut t = Time::new(60);
    t.advance(ms(16));
    t.advance(ms(16));
    assert_eq!(t.frame_count(), 2);
    assert!((t.real.elapsed() - 0.032).abs() < 1e-6);
    assert!((t.game.elapsed() - 0.032).abs() < 1e-6);
  }

  #[test]
  fn invalid_rates_and_speeds_are_clamped() {
    let t = Time::new(0);
    assert!((t.fixed.timestep() - 1.0).abs() < 1e-6, "0 Hz clamps to 1 Hz");
    let mut t = Time::new(60);
    t.game.set_speed(-3.0);
    assert_eq!(t.game.speed(), 0.0);
    t.game.set_speed(f32::NAN);
    assert_eq!(t.game.speed(), 1.0, "NaN keeps the previous valid speed semantics: resets to 1.0");
    t.game.set_speed(f32::INFINITY);
    assert_eq!(t.game.speed(), 1.0);
  }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Add `pub mod time;` to `src/core/mod.rs` (next to `pub mod timing;`) with an empty `src/core/time.rs` containing only the test module.

Run: `cargo test --lib core::time 2>&1 | tail -5`
Expected: compile errors (`Time` not found).

- [ ] **Step 3: Implement**

Above the test module in `src/core/time.rs`:

```rust
//! The engine's clocks.
//!
//! `real` is the wall clock. `game` is real time scaled, pausable and capped;
//! everything the player perceives as game time reads it. `fixed` accumulates
//! game time into constant steps that drive `FixedUpdate`.

use std::time::{Duration, Instant};

use crate::core::ecs::kernel::Resource;

/// Wall-clock time: never scaled, paused or capped.
#[derive(Debug, Clone, Copy, Default)]
pub struct RealClock {
  delta: Duration,
  elapsed: Duration,
}

impl RealClock {
  pub fn delta(&self) -> f32 {
    self.delta.as_secs_f32()
  }

  pub fn delta_duration(&self) -> Duration {
    self.delta
  }

  pub fn elapsed(&self) -> f32 {
    self.elapsed.as_secs_f32()
  }
}

/// Game time: real time times `speed`, zero while paused or in hitstop,
/// and never more than `max_delta` per frame.
#[derive(Debug, Clone)]
pub struct GameClock {
  delta: Duration,
  elapsed: Duration,
  speed: f32,
  paused: bool,
  hitstop: Duration,
  max_delta: Duration,
}

impl Default for GameClock {
  fn default() -> Self {
    Self {
      delta: Duration::ZERO,
      elapsed: Duration::ZERO,
      speed: 1.0,
      paused: false,
      hitstop: Duration::ZERO,
      max_delta: Duration::from_millis(250),
    }
  }
}

impl GameClock {
  pub fn delta(&self) -> f32 {
    self.delta.as_secs_f32()
  }

  pub fn delta_duration(&self) -> Duration {
    self.delta
  }

  pub fn elapsed(&self) -> f32 {
    self.elapsed.as_secs_f32()
  }

  /// Multiplier on real time: 1 is normal, 0.5 is slow-mo, 0 freezes.
  pub fn speed(&self) -> f32 {
    self.speed
  }

  /// Negative speeds clamp to 0; a non-finite speed resets to 1.
  pub fn set_speed(&mut self, speed: f32) {
    self.speed = if speed.is_finite() { speed.max(0.0) } else { 1.0 };
  }

  pub fn pause(&mut self) {
    self.paused = true;
  }

  pub fn resume(&mut self) {
    self.paused = false;
  }

  pub fn is_paused(&self) -> bool {
    self.paused
  }

  /// Freeze game time for `seconds` of real time (a longer hitstop replaces a shorter one).
  pub fn hitstop(&mut self, seconds: f32) {
    let requested = Duration::from_secs_f32(seconds.max(0.0));
    self.hitstop = self.hitstop.max(requested);
  }

  pub fn is_hitstop(&self) -> bool {
    !self.hitstop.is_zero()
  }

  pub fn max_delta(&self) -> Duration {
    self.max_delta
  }

  pub fn set_max_delta(&mut self, max_delta: Duration) {
    self.max_delta = max_delta;
  }

  fn advance(&mut self, real_delta: Duration) {
    if !self.hitstop.is_zero() {
      self.hitstop = self.hitstop.saturating_sub(real_delta);
      self.delta = Duration::ZERO;
      return;
    }
    if self.paused {
      self.delta = Duration::ZERO;
      return;
    }
    let capped = real_delta.min(self.max_delta);
    self.delta = capped.mul_f32(self.speed);
    self.elapsed += self.delta;
  }
}

/// Constant-step time for `FixedUpdate`.
#[derive(Debug, Clone)]
pub struct FixedClock {
  timestep: Duration,
  accumulator: Duration,
  steps: u64,
}

impl FixedClock {
  fn with_hz(hz: u32) -> Self {
    Self {
      timestep: Duration::from_secs_f64(1.0 / f64::from(hz.max(1))),
      accumulator: Duration::ZERO,
      steps: 0,
    }
  }

  pub fn timestep(&self) -> f32 {
    self.timestep.as_secs_f32()
  }

  pub fn timestep_duration(&self) -> Duration {
    self.timestep
  }

  /// Change the fixed rate; a rate of 0 clamps to 1 Hz.
  pub fn set_hz(&mut self, hz: u32) {
    self.timestep = Duration::from_secs_f64(1.0 / f64::from(hz.max(1)));
  }

  pub fn accumulator(&self) -> f32 {
    self.accumulator.as_secs_f32()
  }

  /// How far the next fixed step is through: the render interpolation alpha, in `[0, 1)`.
  pub fn overstep_fraction(&self) -> f32 {
    (self.accumulator.as_secs_f32() / self.timestep.as_secs_f32()).clamp(0.0, 0.999_999)
  }

  /// Fixed steps run since the engine started.
  pub fn step_count(&self) -> u64 {
    self.steps
  }
}

/// The engine's clocks, a resource and `SceneContext::time`.
#[derive(Resource, Debug, Clone)]
pub struct Time {
  pub real: RealClock,
  pub game: GameClock,
  pub fixed: FixedClock,
  frame_count: u64,
  frame_start: Option<Instant>,
  in_fixed: bool,
}

impl Time {
  pub fn new(fixed_hz: u32) -> Self {
    Self {
      real: RealClock::default(),
      game: GameClock::default(),
      fixed: FixedClock::with_hz(fixed_hz),
      frame_count: 0,
      frame_start: None,
      in_fixed: false,
    }
  }

  /// The delta the current code should integrate with: the fixed timestep
  /// inside `FixedUpdate`, the game clock's frame delta elsewhere.
  pub fn delta(&self) -> f32 {
    if self.in_fixed {
      self.fixed.timestep()
    } else {
      self.game.delta()
    }
  }

  pub fn frame_count(&self) -> u64 {
    self.frame_count
  }

  /// Advance by the wall-clock time since the previous call (zero on the first).
  pub fn begin_frame(&mut self) {
    let now = Instant::now();
    let delta = self
      .frame_start
      .map(|start| now.duration_since(start))
      .unwrap_or(Duration::ZERO);
    self.frame_start = Some(now);
    self.advance(delta);
  }

  /// Advance by an explicit real duration (headless runs and tests).
  pub fn advance(&mut self, real_delta: Duration) {
    self.real.delta = real_delta;
    self.real.elapsed += real_delta;
    self.game.advance(real_delta);
    self.fixed.accumulator += self.game.delta;
    self.frame_count += 1;
  }

  /// Consume one fixed step if the accumulator holds one.
  pub(crate) fn try_expend_fixed_step(&mut self) -> bool {
    if self.fixed.accumulator >= self.fixed.timestep {
      self.fixed.accumulator -= self.fixed.timestep;
      self.fixed.steps += 1;
      true
    } else {
      false
    }
  }

  pub(crate) fn set_in_fixed(&mut self, in_fixed: bool) {
    self.in_fixed = in_fixed;
  }

  /// The instant the current frame started, for frame pacing.
  pub fn frame_start(&self) -> Option<Instant> {
    self.frame_start
  }
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test --lib core::time 2>&1 | tail -5`
Expected: `test result: ok. 8 passed`.

- [ ] **Step 5: Commit**

```bash
git add src/core/time.rs src/core/mod.rs
git commit -m "feat(time): real, game and fixed clocks with a constant fixed step"
```

### Task 3: `Transform`, `GlobalTransform`, `PreviousGlobalTransform`, `Interpolated`, `Anchor`

**Files:**
- Create: `src/core/ecs/transform.rs`
- Modify: `src/core/ecs/mod.rs` (add `pub mod transform;`)
- Modify: `src/core/ui/types.rs` (move the `Anchor` enum out; re-export it from `transform`)
- Test: inline in `src/core/ecs/transform.rs`

**Interfaces:**
- Produces (all in `crate::core::ecs::transform`):
  - `Transform { translation: Vec2, rotation: f32, scale: Vec2 }`: `IDENTITY`, `from_xy(x, y)`, `from_translation(Vec2)`, `with_rotation(rad)`, `with_rotation_degrees(deg)`, `with_scale(Vec2)`, `with_uniform_scale(f32)`, `rotation_degrees()`, `set_rotation_degrees(deg)`, `to_affine() -> Affine2`
  - `GlobalTransform(Affine2)`: `IDENTITY`, `from_transform(&Transform)`, `affine()`, `translation()`, `rotation()`, `scale()`, `transform_point(Vec2)`, `to_transform()`, `mul_transform(&Transform) -> GlobalTransform`, `lerp(&GlobalTransform, t) -> GlobalTransform`
  - `PreviousGlobalTransform { global: GlobalTransform, skip: bool }`
  - `Interpolated` (marker)
  - `Anchor` (moved from `ui::types`; `to_pivot()` kept)

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
  use super::*;
  use std::f32::consts::{FRAC_PI_2, PI};

  fn close(a: Vec2, b: Vec2) -> bool {
    (a - b).length() < 1e-4
  }

  #[test]
  fn transform_defaults_to_identity() {
    let t = Transform::default();
    assert_eq!(t, Transform::IDENTITY);
    assert_eq!(t.scale, Vec2::ONE);
  }

  #[test]
  fn degree_helpers_convert() {
    let t = Transform::from_xy(1.0, 2.0).with_rotation_degrees(90.0);
    assert!((t.rotation - FRAC_PI_2).abs() < 1e-6);
    assert!((t.rotation_degrees() - 90.0).abs() < 1e-4);
  }

  #[test]
  fn global_decomposes_translation_rotation_scale() {
    let t = Transform::from_xy(10.0, -5.0)
      .with_rotation(0.3)
      .with_scale(Vec2::new(2.0, 3.0));
    let g = GlobalTransform::from_transform(&t);
    assert!(close(g.translation(), Vec2::new(10.0, -5.0)));
    assert!((g.rotation() - 0.3).abs() < 1e-5);
    assert!(close(g.scale(), Vec2::new(2.0, 3.0)));
    let back = g.to_transform();
    assert!(close(back.translation, t.translation));
    assert!((back.rotation - t.rotation).abs() < 1e-5);
    assert!(close(back.scale, t.scale));
  }

  #[test]
  fn child_of_a_rotated_parent_lands_where_expected() {
    let parent = GlobalTransform::from_transform(&Transform::from_xy(100.0, 100.0).with_rotation(FRAC_PI_2));
    let child = parent.mul_transform(&Transform::from_xy(10.0, 0.0));
    assert!(close(child.translation(), Vec2::new(100.0, 110.0)));
    assert!((child.rotation() - FRAC_PI_2).abs() < 1e-5);
  }

  #[test]
  fn transform_point_applies_scale_then_rotation_then_translation() {
    let g = GlobalTransform::from_transform(
      &Transform::from_xy(1.0, 1.0).with_rotation(FRAC_PI_2).with_uniform_scale(2.0),
    );
    assert!(close(g.transform_point(Vec2::new(1.0, 0.0)), Vec2::new(1.0, 3.0)));
  }

  #[test]
  fn lerp_blends_translation_and_takes_the_short_way_round() {
    let a = GlobalTransform::from_transform(&Transform::from_xy(0.0, 0.0).with_rotation(PI - 0.1));
    let b = GlobalTransform::from_transform(&Transform::from_xy(10.0, 0.0).with_rotation(-PI + 0.1));
    let mid = a.lerp(&b, 0.5);
    assert!(close(mid.translation(), Vec2::new(5.0, 0.0)));
    // The short arc passes through PI, not through 0
    assert!((mid.rotation().abs() - PI).abs() < 1e-4, "{}", mid.rotation());
  }

  #[test]
  fn anchor_pivots_are_unchanged_by_the_move() {
    assert_eq!(Anchor::default(), Anchor::Center);
    assert_eq!(Anchor::TopLeft.to_pivot(), (0.0, 0.0));
    assert_eq!(Anchor::BottomRight.to_pivot(), (1.0, 1.0));
  }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Add `pub mod transform;` to `src/core/ecs/mod.rs`. Run: `cargo test --lib core::ecs::transform 2>&1 | tail -5`
Expected: compile errors.

- [ ] **Step 3: Implement**

Move the `Anchor` enum (and its `to_pivot`, `Default`) from `src/core/ui/types.rs` into `src/core/ecs/transform.rs`, change its `Default` to `Anchor::Center`, and in `src/core/ui/types.rs` replace the definition with `pub use crate::core::ecs::transform::Anchor;`. Update `src/core/ecs/components.rs`'s `use crate::core::ui::Anchor;` to `use super::transform::Anchor;` (it is deleted in Task 9 anyway). The old `Transform::with_anchor` tests in `components.rs` that assert `Anchor::default() == TopLeft` must be updated to `Center` in this task.

Then, above the tests:

```rust
//! Where things are: `Transform` is local to the parent, `GlobalTransform`
//! is world space and written only by propagation.

use glam::{Affine2, Mat2, Vec2};
use serde::{Deserialize, Serialize};

use super::kernel::Component;

/// Which point of a sprite sits on the entity's origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Anchor {
  TopLeft,
  TopCenter,
  TopRight,
  CenterLeft,
  #[default]
  Center,
  CenterRight,
  BottomLeft,
  BottomCenter,
  BottomRight,
}

impl Anchor {
  /// Normalized pivot: (0, 0) top-left, (1, 1) bottom-right.
  pub fn to_pivot(self) -> (f32, f32) {
    match self {
      Anchor::TopLeft => (0.0, 0.0),
      Anchor::TopCenter => (0.5, 0.0),
      Anchor::TopRight => (1.0, 0.0),
      Anchor::CenterLeft => (0.0, 0.5),
      Anchor::Center => (0.5, 0.5),
      Anchor::CenterRight => (1.0, 0.5),
      Anchor::BottomLeft => (0.0, 1.0),
      Anchor::BottomCenter => (0.5, 1.0),
      Anchor::BottomRight => (1.0, 1.0),
    }
  }
}

/// Position, rotation (radians) and scale relative to the parent, or to the
/// world for a root entity.
#[derive(Component, Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[require(GlobalTransform)]
pub struct Transform {
  pub translation: Vec2,
  pub rotation: f32,
  pub scale: Vec2,
}

impl Transform {
  pub const IDENTITY: Self = Self {
    translation: Vec2::ZERO,
    rotation: 0.0,
    scale: Vec2::ONE,
  };

  pub fn from_xy(x: f32, y: f32) -> Self {
    Self::from_translation(Vec2::new(x, y))
  }

  pub fn from_translation(translation: Vec2) -> Self {
    Self {
      translation,
      ..Self::IDENTITY
    }
  }

  pub fn with_rotation(mut self, radians: f32) -> Self {
    self.rotation = radians;
    self
  }

  pub fn with_rotation_degrees(self, degrees: f32) -> Self {
    self.with_rotation(degrees.to_radians())
  }

  pub fn with_scale(mut self, scale: Vec2) -> Self {
    self.scale = scale;
    self
  }

  pub fn with_uniform_scale(self, scale: f32) -> Self {
    self.with_scale(Vec2::splat(scale))
  }

  pub fn rotation_degrees(&self) -> f32 {
    self.rotation.to_degrees()
  }

  pub fn set_rotation_degrees(&mut self, degrees: f32) {
    self.rotation = degrees.to_radians();
  }

  pub fn to_affine(&self) -> Affine2 {
    Affine2::from_scale_angle_translation(self.scale, self.rotation, self.translation)
  }
}

impl Default for Transform {
  fn default() -> Self {
    Self::IDENTITY
  }
}

/// World-space transform, computed by the engine from the hierarchy.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct GlobalTransform(Affine2);

impl Default for GlobalTransform {
  fn default() -> Self {
    Self::IDENTITY
  }
}

impl GlobalTransform {
  pub const IDENTITY: Self = Self(Affine2::IDENTITY);

  pub fn from_transform(transform: &Transform) -> Self {
    Self(transform.to_affine())
  }

  pub fn affine(&self) -> Affine2 {
    self.0
  }

  pub fn translation(&self) -> Vec2 {
    self.0.translation
  }

  /// Rotation in radians, taken from the x axis.
  pub fn rotation(&self) -> f32 {
    let x = self.0.matrix2.x_axis;
    x.y.atan2(x.x)
  }

  /// Axis lengths; negative scale is not represented (use sprite flips).
  pub fn scale(&self) -> Vec2 {
    Vec2::new(self.0.matrix2.x_axis.length(), self.0.matrix2.y_axis.length())
  }

  pub fn transform_point(&self, point: Vec2) -> Vec2 {
    self.0.transform_point2(point)
  }

  pub fn to_transform(&self) -> Transform {
    Transform {
      translation: self.translation(),
      rotation: self.rotation(),
      scale: self.scale(),
    }
  }

  /// This transform followed by a local one: a child's world transform.
  pub fn mul_transform(&self, local: &Transform) -> Self {
    Self(self.0 * local.to_affine())
  }

  /// Blend toward `other`: linear on translation and scale, shortest arc on rotation.
  pub fn lerp(&self, other: &Self, t: f32) -> Self {
    let (a, b) = (self.to_transform(), other.to_transform());
    let mut delta = b.rotation - a.rotation;
    if delta > std::f32::consts::PI {
      delta -= std::f32::consts::TAU;
    } else if delta < -std::f32::consts::PI {
      delta += std::f32::consts::TAU;
    }
    let blended = Transform {
      translation: a.translation.lerp(b.translation, t),
      rotation: a.rotation + delta * t,
      scale: a.scale.lerp(b.scale, t),
    };
    Self::from_transform(&blended)
  }
}

/// The previous fixed step's global transform, for render interpolation.
/// While `skip` is set the renderer draws the current transform instead.
#[derive(Component, Debug, Clone, Copy)]
pub struct PreviousGlobalTransform {
  pub global: GlobalTransform,
  pub skip: bool,
}

/// Opt an entity (and its descendants) into render interpolation.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct Interpolated;

// Keep `Mat2` in scope for the rotation helper if the compiler complains about an unused import
#[allow(dead_code)]
fn _mat2_used(_: Mat2) {}
```

Remove the `_mat2_used` helper and the `Mat2` import if `cargo check` does not need them.

- [ ] **Step 4: Run the tests**

Run: `cargo test --lib core::ecs::transform core::ui 2>&1 | tail -5`
Expected: all pass (the moved `Anchor` keeps the UI tests green; fix any UI test that asserted the old `TopLeft` default).

- [ ] **Step 5: Commit**

```bash
git add src/core/ecs/transform.rs src/core/ecs/mod.rs src/core/ui/types.rs src/core/ecs/components.rs
git commit -m "feat(ecs): 2D Transform and GlobalTransform with Anchor moved beside them"
```

### Task 4: Propagation and snapshot systems

**Files:**
- Create: `src/core/ecs/propagate.rs`
- Create: `src/core/ecs/interpolate.rs`
- Modify: `src/core/ecs/mod.rs` (add both modules)
- Test: inline in both files

**Interfaces:**
- Consumes: Task 3 types; `kernel::{Query, Ref, Entity, Commands, Children, ChildOf, Has, With, Without, DetectChangesMut, Schedule, World}`.
- Produces:
  - `pub fn propagate_transforms(roots: Query<..>, nodes: Query<..>, commands: Commands)` system
  - `pub fn snapshot_previous_transforms(q: Query<(&GlobalTransform, &mut PreviousGlobalTransform)>)` system
  - `pub fn render_global(global: &GlobalTransform, previous: Option<&PreviousGlobalTransform>, alpha: f32) -> GlobalTransform`

- [ ] **Step 1: Write the failing tests**

`src/core/ecs/propagate.rs` tests:

```rust
#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ecs::kernel::{Schedule, ScheduleLabel, World};
  use crate::core::ecs::transform::{Interpolated, PreviousGlobalTransform, Transform};
  use glam::Vec2;
  use std::f32::consts::FRAC_PI_2;

  #[derive(ScheduleLabel, Debug, Clone, Copy, PartialEq, Eq, Hash)]
  struct Step;

  fn schedule() -> Schedule {
    let mut s = Schedule::new(Step);
    s.add_systems(propagate_transforms);
    s
  }

  fn global(world: &World, e: Entity) -> GlobalTransform {
    *world.get::<GlobalTransform>(e).expect("GlobalTransform")
  }

  fn close(a: Vec2, b: Vec2) -> bool {
    (a - b).length() < 1e-4
  }

  #[test]
  fn child_of_rotated_parent() {
    let mut world = World::new();
    let parent = world
      .spawn(Transform::from_xy(100.0, 100.0).with_rotation(FRAC_PI_2))
      .id();
    let child = world.spawn((Transform::from_xy(10.0, 0.0), ChildOf(parent))).id();
    schedule().run(&mut world);
    assert!(close(global(&world, child).translation(), Vec2::new(100.0, 110.0)));
  }

  #[test]
  fn three_levels_compose() {
    let mut world = World::new();
    let a = world.spawn(Transform::from_xy(1.0, 0.0)).id();
    let b = world.spawn((Transform::from_xy(1.0, 0.0), ChildOf(a))).id();
    let c = world.spawn((Transform::from_xy(1.0, 0.0), ChildOf(b))).id();
    schedule().run(&mut world);
    assert!(close(global(&world, c).translation(), Vec2::new(3.0, 0.0)));
  }

  #[test]
  fn unchanged_subtrees_are_not_rewritten() {
    let mut world = World::new();
    let a = world.spawn(Transform::from_xy(1.0, 0.0)).id();
    let b = world.spawn((Transform::from_xy(1.0, 0.0), ChildOf(a))).id();
    let mut s = schedule();
    s.run(&mut world);
    world.clear_trackers();
    s.run(&mut world);
    let b_ref = world.entity(b).get_ref::<GlobalTransform>().unwrap();
    assert!(!b_ref.is_changed(), "nothing moved, so nothing was written");
    world.get_mut::<Transform>(a).unwrap().translation.x = 5.0;
    s.run(&mut world);
    assert!(close(global(&world, b).translation(), Vec2::new(6.0, 0.0)));
  }

  #[test]
  fn reparenting_recomputes() {
    let mut world = World::new();
    let a = world.spawn(Transform::from_xy(0.0, 0.0)).id();
    let b = world.spawn(Transform::from_xy(50.0, 0.0)).id();
    let c = world.spawn((Transform::from_xy(1.0, 0.0), ChildOf(a))).id();
    let mut s = schedule();
    s.run(&mut world);
    world.entity_mut(c).insert(ChildOf(b));
    s.run(&mut world);
    assert!(close(global(&world, c).translation(), Vec2::new(51.0, 0.0)));
  }

  #[test]
  fn child_of_a_transform_less_parent_treats_it_as_identity() {
    let mut world = World::new();
    let group = world.spawn_empty().id();
    let child = world.spawn((Transform::from_xy(7.0, 8.0), ChildOf(group))).id();
    schedule().run(&mut world);
    assert!(close(global(&world, child).translation(), Vec2::new(7.0, 8.0)));
  }

  #[test]
  fn interpolated_ancestors_mark_descendants() {
    let mut world = World::new();
    let ship = world.spawn((Transform::IDENTITY, Interpolated)).id();
    let turret = world.spawn((Transform::from_xy(1.0, 0.0), ChildOf(ship))).id();
    let plain = world.spawn(Transform::IDENTITY).id();
    schedule().run(&mut world);
    assert!(world.get::<PreviousGlobalTransform>(ship).is_some());
    assert!(world.get::<PreviousGlobalTransform>(turret).is_some());
    assert!(world.get::<PreviousGlobalTransform>(plain).is_none());
    assert!(
      world.get::<PreviousGlobalTransform>(turret).unwrap().skip,
      "a fresh snapshot skips so the first frame does not lerp from identity"
    );
    world.entity_mut(ship).remove::<Interpolated>();
    schedule().run(&mut world);
    assert!(world.get::<PreviousGlobalTransform>(turret).is_none());
  }

  #[test]
  fn despawning_a_parent_removes_descendants() {
    let mut world = World::new();
    let a = world.spawn(Transform::IDENTITY).id();
    let b = world.spawn((Transform::IDENTITY, ChildOf(a))).id();
    world.despawn(a);
    assert!(world.get_entity(b).is_err());
  }
}
```

`src/core/ecs/interpolate.rs` tests:

```rust
#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ecs::transform::Transform;
  use glam::Vec2;

  fn at(x: f32) -> GlobalTransform {
    GlobalTransform::from_transform(&Transform::from_xy(x, 0.0))
  }

  #[test]
  fn without_previous_the_current_transform_is_drawn() {
    let g = at(10.0);
    assert_eq!(render_global(&g, None, 0.5), g);
  }

  #[test]
  fn with_previous_the_blend_is_drawn() {
    let prev = PreviousGlobalTransform { global: at(0.0), skip: false };
    let got = render_global(&at(10.0), Some(&prev), 0.25);
    assert!((got.translation() - Vec2::new(2.5, 0.0)).length() < 1e-4);
  }

  #[test]
  fn skip_draws_current() {
    let prev = PreviousGlobalTransform { global: at(0.0), skip: true };
    assert_eq!(render_global(&at(10.0), Some(&prev), 0.25), at(10.0));
  }

  #[test]
  fn snapshot_copies_and_clears_skip() {
    use crate::core::ecs::kernel::{Schedule, ScheduleLabel, World};
    #[derive(ScheduleLabel, Debug, Clone, Copy, PartialEq, Eq, Hash)]
    struct Step;
    let mut world = World::new();
    let e = world
      .spawn((at(3.0), PreviousGlobalTransform { global: at(0.0), skip: true }))
      .id();
    let mut s = Schedule::new(Step);
    s.add_systems(snapshot_previous_transforms);
    s.run(&mut world);
    let prev = world.get::<PreviousGlobalTransform>(e).unwrap();
    assert_eq!(prev.global, at(3.0));
    assert!(!prev.skip);
  }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Add `pub mod propagate; pub mod interpolate;` to `src/core/ecs/mod.rs`. Run: `cargo test --lib core::ecs::propagate core::ecs::interpolate 2>&1 | tail -5`
Expected: compile errors.

- [ ] **Step 3: Implement propagation**

`src/core/ecs/propagate.rs`:

```rust
//! Transform propagation: local `Transform`s down the hierarchy into
//! `GlobalTransform`s, recomputing only subtrees that changed.

use super::kernel::{
  ChildOf, Children, Commands, DetectChangesMut, Entity, Has, Query, Ref, With, Without,
};
use super::transform::{GlobalTransform, Interpolated, PreviousGlobalTransform, Transform};

type RootItem<'a> = (
  Entity,
  Ref<'a, Transform>,
  &'a mut GlobalTransform,
  Option<&'a Children>,
  Has<Interpolated>,
  Has<PreviousGlobalTransform>,
);

type NodeItem<'a> = (
  Ref<'a, Transform>,
  Ref<'a, ChildOf>,
  &'a mut GlobalTransform,
  Option<&'a Children>,
  Has<Interpolated>,
  Has<PreviousGlobalTransform>,
);

/// Recompute `GlobalTransform` for every subtree whose root or any ancestor
/// changed, and keep `PreviousGlobalTransform` present on interpolated
/// subtrees (and absent elsewhere).
///
/// Roots are entities with a `Transform` and no parent. Children of a parent
/// that has no `Transform` of its own (a bare grouping entity) start from the
/// identity, as if they were roots.
pub fn propagate_transforms(
  mut roots: Query<RootItem, Without<ChildOf>>,
  mut nodes: Query<NodeItem, With<ChildOf>>,
  grouping: Query<(Entity, &Children), Without<Transform>>,
  mut commands: Commands,
) {
  // (entity, parent's global, parent changed, parent interpolated)
  let mut stack: Vec<(Entity, GlobalTransform, bool, bool)> = Vec::new();

  for (entity, transform, mut global, children, interpolated, has_previous) in roots.iter_mut() {
    let changed = transform.is_changed();
    if changed {
      global.set_if_neq(GlobalTransform::from_transform(&transform));
    }
    sync_previous(&mut commands, entity, interpolated, has_previous, *global);
    if let Some(children) = children {
      for child in children.iter() {
        stack.push((child, *global, changed, interpolated));
      }
    }
  }

  for (_, children) in grouping.iter() {
    for child in children.iter() {
      stack.push((child, GlobalTransform::IDENTITY, true, false));
    }
  }

  while let Some((entity, parent_global, parent_changed, parent_interpolated)) = stack.pop() {
    let Ok((transform, child_of, mut global, children, interpolated, has_previous)) =
      nodes.get_mut(entity)
    else {
      continue; // a child without a Transform: nothing to place, nothing below it to place
    };
    let changed = parent_changed || transform.is_changed() || child_of.is_changed();
    if changed {
      global.set_if_neq(parent_global.mul_transform(&transform));
    }
    let interpolated = interpolated || parent_interpolated;
    sync_previous(&mut commands, entity, interpolated, has_previous, *global);
    if let Some(children) = children {
      for child in children.iter() {
        stack.push((child, *global, changed, interpolated));
      }
    }
  }
}

/// Insert or remove `PreviousGlobalTransform` so that it exists exactly on
/// interpolated entities. A fresh one skips, so the first frame draws current.
fn sync_previous(
  commands: &mut Commands,
  entity: Entity,
  interpolated: bool,
  has_previous: bool,
  global: GlobalTransform,
) {
  match (interpolated, has_previous) {
    (true, false) => {
      commands
        .entity(entity)
        .insert(PreviousGlobalTransform { global, skip: true });
    }
    (false, true) => {
      commands.entity(entity).remove::<PreviousGlobalTransform>();
    }
    _ => {}
  }
}
```

Two API details to check against `target/doc/bevy_ecs` if the compiler disagrees: `Children::iter()` yields `Entity` by value in 0.20 (add `.copied()` if it yields `&Entity`), and the parent accessor is `ChildOf::parent()` (use the tuple field `child_of.0` if there is no method). The `grouping` query only matches entities that have `Children` and no `Transform`, so an ordinary parent is never pushed twice. A grouping entity nested under a transform-carrying parent is reached as a child of that parent first; `nodes.get_mut` fails for it (it has no `Transform`), so its own children are placed from the identity by the `grouping` loop, which is the documented rule: a transform-less entity does not carry its parent's transform down.

- [ ] **Step 4: Implement interpolation helpers**

`src/core/ecs/interpolate.rs`:

```rust
//! Render interpolation between fixed steps.

use super::kernel::Query;
use super::transform::{GlobalTransform, PreviousGlobalTransform};

/// Runs first in every fixed step: remember where everything was.
pub fn snapshot_previous_transforms(
  mut query: Query<(&GlobalTransform, &mut PreviousGlobalTransform)>,
) {
  for (global, mut previous) in &mut query {
    previous.global = *global;
    previous.skip = false;
  }
}

/// The transform to draw this frame.
pub fn render_global(
  global: &GlobalTransform,
  previous: Option<&PreviousGlobalTransform>,
  alpha: f32,
) -> GlobalTransform {
  match previous {
    Some(prev) if !prev.skip => prev.global.lerp(global, alpha),
    _ => *global,
  }
}
```

- [ ] **Step 5: Run the tests**

Run: `cargo test --lib core::ecs::propagate core::ecs::interpolate 2>&1 | tail -5`
Expected: `test result: ok. 11 passed`.

- [ ] **Step 6: Commit**

```bash
git add src/core/ecs/propagate.rs src/core/ecs/interpolate.rs src/core/ecs/mod.rs
git commit -m "feat(ecs): change-detected transform propagation and interpolation snapshots"
```

### Task 5: `WorldExt` helpers and pooling on `Disabled`

**Files:**
- Create: `src/core/ecs/world_ext.rs`
- Create: `src/core/ecs/pooling.rs` (replaces `pool.rs` at Task 9)
- Modify: `src/core/ecs/mod.rs`
- Test: inline in both files

**Interfaces:**
- Produces:
  - `pub trait WorldExt { fn spawn_child(&mut self, parent: Entity, bundle: impl Bundle) -> Entity; fn detach_keep_world(&mut self, entity: Entity); fn skip_interpolation(&mut self, entity: Entity); }` implemented for `World`
  - `pub struct Pool<B: Bundle>` with `new(factory: impl Fn() -> B + Send + Sync + 'static, config: PoolConfig)`, `acquire(&mut self, world: &mut World) -> Option<Entity>`, `release(&mut self, world: &mut World, entity: Entity) -> bool`, `release_all(&mut self, world: &mut World)`, `active(&self) -> &[Entity]`, `stats(&self) -> PoolStats`, `is_active(&self, entity) -> bool`
  - `PoolConfig { initial_capacity, max_capacity (0 = unlimited), expand_batch_size, exhausted_policy }`, `PoolExhaustedPolicy { Expand, ReturnNone, RecycleOldest }`, `PoolStats { total_spawned, active, available }`

- [ ] **Step 1: Write the failing tests**

`world_ext.rs`:

```rust
#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ecs::kernel::{Schedule, ScheduleLabel, World};
  use crate::core::ecs::propagate::propagate_transforms;
  use crate::core::ecs::transform::{GlobalTransform, Interpolated, PreviousGlobalTransform, Transform};
  use glam::Vec2;
  use std::f32::consts::FRAC_PI_2;

  #[derive(ScheduleLabel, Debug, Clone, Copy, PartialEq, Eq, Hash)]
  struct Step;

  fn propagate(world: &mut World) {
    let mut s = Schedule::new(Step);
    s.add_systems(propagate_transforms);
    s.run(world);
  }

  #[test]
  fn spawn_child_sets_child_of() {
    let mut world = World::new();
    let p = world.spawn(Transform::IDENTITY).id();
    let c = world.spawn_child(p, Transform::from_xy(1.0, 1.0));
    assert_eq!(world.get::<ChildOf>(c).map(|c| c.parent()), Some(p));
    assert!(world.get::<Children>(p).is_some_and(|ch| ch.iter().any(|e| e == c)));
  }

  #[test]
  fn detach_keep_world_leaves_the_world_position_unchanged() {
    let mut world = World::new();
    let p = world
      .spawn(Transform::from_xy(100.0, 0.0).with_rotation(FRAC_PI_2).with_uniform_scale(2.0))
      .id();
    let c = world.spawn_child(p, Transform::from_xy(5.0, 0.0));
    propagate(&mut world);
    let before = world.get::<GlobalTransform>(c).unwrap().translation();
    world.detach_keep_world(c);
    propagate(&mut world);
    assert!(world.get::<ChildOf>(c).is_none());
    let after = world.get::<GlobalTransform>(c).unwrap().translation();
    assert!((before - after).length() < 1e-3, "{before} vs {after}");
    assert!((before - Vec2::new(100.0, 10.0)).length() < 1e-3);
  }

  #[test]
  fn skip_interpolation_sets_the_flag() {
    let mut world = World::new();
    let e = world.spawn((Transform::IDENTITY, Interpolated)).id();
    propagate(&mut world);
    world.get_mut::<PreviousGlobalTransform>(e).unwrap().skip = false;
    world.skip_interpolation(e);
    assert!(world.get::<PreviousGlobalTransform>(e).unwrap().skip);
    // No-op on an entity without the component
    let plain = world.spawn(Transform::IDENTITY).id();
    world.skip_interpolation(plain);
  }
}
```

`pooling.rs`:

```rust
#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ecs::kernel::{Allow, Disabled, World};
  use crate::core::ecs::transform::Transform;

  fn pool(config: PoolConfig) -> Pool<Transform> {
    Pool::new(|| Transform::IDENTITY, config)
  }

  #[test]
  fn acquire_enables_and_release_disables() {
    let mut world = World::new();
    let mut p = pool(PoolConfig::default());
    let e = p.acquire(&mut world).unwrap();
    assert!(world.get::<Disabled>(e).is_none());
    assert!(p.is_active(e));
    assert!(p.release(&mut world, e));
    assert!(world.get::<Disabled>(e).is_some());
    let visible = world.query::<&Transform>().iter(&world).count();
    assert_eq!(visible, 0, "disabled entities are hidden from plain queries");
    let all = world
      .query_filtered::<&Transform, Allow<Disabled>>()
      .iter(&world)
      .count();
    assert_eq!(all, p.stats().total_spawned);
  }

  #[test]
  fn return_none_policy_stops_at_capacity() {
    let mut world = World::new();
    let mut p = pool(PoolConfig {
      initial_capacity: 2,
      max_capacity: 2,
      expand_batch_size: 1,
      exhausted_policy: PoolExhaustedPolicy::ReturnNone,
    });
    assert!(p.acquire(&mut world).is_some());
    assert!(p.acquire(&mut world).is_some());
    assert!(p.acquire(&mut world).is_none());
  }

  #[test]
  fn recycle_oldest_reuses_the_longest_active() {
    let mut world = World::new();
    let mut p = pool(PoolConfig {
      initial_capacity: 2,
      max_capacity: 2,
      expand_batch_size: 1,
      exhausted_policy: PoolExhaustedPolicy::RecycleOldest,
    });
    let first = p.acquire(&mut world).unwrap();
    let _second = p.acquire(&mut world).unwrap();
    let third = p.acquire(&mut world).unwrap();
    assert_eq!(third, first);
    assert_eq!(p.active().len(), 2);
  }

  #[test]
  fn dead_entities_are_purged_after_a_world_cleanup() {
    let mut world = World::new();
    let mut p = pool(PoolConfig {
      initial_capacity: 2,
      max_capacity: 2,
      expand_batch_size: 1,
      exhausted_policy: PoolExhaustedPolicy::ReturnNone,
    });
    let e = p.acquire(&mut world).unwrap();
    let all: Vec<Entity> = world.query_filtered::<Entity, Allow<Disabled>>().iter(&world).collect();
    for e in all {
      world.despawn(e);
    }
    let fresh = p.acquire(&mut world).expect("the pool refills after its entities died");
    assert_ne!(fresh, e);
    assert!(world.get_entity(fresh).is_ok());
    assert_eq!(p.stats().total_spawned, 2, "capacity accounting resets with the purge");
  }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Add `pub mod world_ext; pub mod pooling;` to `src/core/ecs/mod.rs`. Run: `cargo test --lib core::ecs::world_ext core::ecs::pooling 2>&1 | tail -5`
Expected: compile errors.

- [ ] **Step 3: Implement `WorldExt`**

```rust
//! Convenience methods on `World` for the hierarchy and interpolation.

use super::kernel::{Bundle, ChildOf, Entity, World};
use super::transform::{GlobalTransform, PreviousGlobalTransform, Transform};

pub trait WorldExt {
  /// Spawn `bundle` as a child of `parent`.
  fn spawn_child(&mut self, parent: Entity, bundle: impl Bundle) -> Entity;
  /// Remove the entity from its parent, keeping its world position.
  fn detach_keep_world(&mut self, entity: Entity);
  /// Draw the entity at its new transform on the next frame instead of sliding there.
  fn skip_interpolation(&mut self, entity: Entity);
}

impl WorldExt for World {
  fn spawn_child(&mut self, parent: Entity, bundle: impl Bundle) -> Entity {
    self.spawn((bundle, ChildOf(parent))).id()
  }

  fn detach_keep_world(&mut self, entity: Entity) {
    let global = self.get::<GlobalTransform>(entity).copied();
    let Ok(mut e) = self.get_entity_mut(entity) else {
      return;
    };
    if e.get::<ChildOf>().is_none() {
      return;
    }
    e.remove::<ChildOf>();
    if let Some(global) = global {
      e.insert(global.to_transform());
    }
  }

  fn skip_interpolation(&mut self, entity: Entity) {
    if let Some(mut previous) = self.get_mut::<PreviousGlobalTransform>(entity) {
      previous.skip = true;
    }
    let _ = Transform::IDENTITY; // keep the import meaningful if unused elsewhere
  }
}
```

Remove the `let _ = Transform::IDENTITY;` line and the `Transform` import if `cargo check` warns.

- [ ] **Step 4: Implement pooling**

`src/core/ecs/pooling.rs`:

```rust
//! Object pooling on the entity-disabling filter: inactive pooled entities
//! carry `Disabled` and vanish from every ordinary query.

use super::kernel::{Bundle, Disabled, Entity, World};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PoolExhaustedPolicy {
  #[default]
  Expand,
  ReturnNone,
  RecycleOldest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PoolConfig {
  pub initial_capacity: usize,
  /// 0 means unlimited
  pub max_capacity: usize,
  pub expand_batch_size: usize,
  pub exhausted_policy: PoolExhaustedPolicy,
}

impl Default for PoolConfig {
  fn default() -> Self {
    Self {
      initial_capacity: 32,
      max_capacity: 0,
      expand_batch_size: 16,
      exhausted_policy: PoolExhaustedPolicy::Expand,
    }
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PoolStats {
  pub total_spawned: usize,
  pub active: usize,
  pub available: usize,
}

pub struct Pool<B: Bundle> {
  factory: Box<dyn Fn() -> B + Send + Sync>,
  config: PoolConfig,
  available: Vec<Entity>,
  /// Oldest first
  active: Vec<Entity>,
  spawned: bool,
}

impl<B: Bundle> Pool<B> {
  pub fn new(factory: impl Fn() -> B + Send + Sync + 'static, config: PoolConfig) -> Self {
    Self {
      factory: Box::new(factory),
      config,
      available: Vec::new(),
      active: Vec::new(),
      spawned: false,
    }
  }

  fn total(&self) -> usize {
    self.available.len() + self.active.len()
  }

  /// Drop entities the world no longer has (a scene cleanup despawned them).
  fn purge_dead(&mut self, world: &World) {
    self.available.retain(|&e| world.get_entity(e).is_ok());
    self.active.retain(|&e| world.get_entity(e).is_ok());
  }

  fn expand(&mut self, world: &mut World, count: usize) {
    for _ in 0..count {
      if self.config.max_capacity != 0 && self.total() >= self.config.max_capacity {
        break;
      }
      let e = world.spawn(((self.factory)(), Disabled)).id();
      self.available.push(e);
    }
  }

  /// Take an entity out of the pool (enabled). Components keep whatever
  /// values they had; set every field you rely on.
  pub fn acquire(&mut self, world: &mut World) -> Option<Entity> {
    self.purge_dead(world);
    if !self.spawned || self.total() == 0 {
      self.spawned = true;
      self.expand(world, self.config.initial_capacity.max(1));
    }
    if self.available.is_empty() {
      match self.config.exhausted_policy {
        PoolExhaustedPolicy::Expand => self.expand(world, self.config.expand_batch_size.max(1)),
        PoolExhaustedPolicy::ReturnNone => {}
        PoolExhaustedPolicy::RecycleOldest => {
          if !self.active.is_empty() {
            let oldest = self.active.remove(0);
            self.available.push(oldest);
          }
        }
      }
    }
    let entity = self.available.pop()?;
    world.entity_mut(entity).remove::<Disabled>();
    self.active.push(entity);
    Some(entity)
  }

  /// Return an entity to the pool (disabled). False if it was not active.
  pub fn release(&mut self, world: &mut World, entity: Entity) -> bool {
    let Some(index) = self.active.iter().position(|&e| e == entity) else {
      return false;
    };
    self.active.remove(index);
    if let Ok(mut e) = world.get_entity_mut(entity) {
      e.insert(Disabled);
      self.available.push(entity);
    }
    true
  }

  pub fn release_all(&mut self, world: &mut World) {
    let active = std::mem::take(&mut self.active);
    for entity in active {
      if let Ok(mut e) = world.get_entity_mut(entity) {
        e.insert(Disabled);
        self.available.push(entity);
      }
    }
  }

  pub fn active(&self) -> &[Entity] {
    &self.active
  }

  pub fn is_active(&self, entity: Entity) -> bool {
    self.active.contains(&entity)
  }

  pub fn stats(&self) -> PoolStats {
    PoolStats {
      total_spawned: self.total(),
      active: self.active.len(),
      available: self.available.len(),
    }
  }
}
```

`RecycleOldest` is oldest-first because `active` is append-only and `release` removes by position (a fix for the audit's finding). `stats().total_spawned` reports live entities, so capacity accounting resets after a purge.

- [ ] **Step 5: Run the tests**

Run: `cargo test --lib core::ecs::world_ext core::ecs::pooling 2>&1 | tail -5`
Expected: `test result: ok. 7 passed`.

- [ ] **Step 6: Commit**

```bash
git add src/core/ecs/world_ext.rs src/core/ecs/pooling.rs src/core/ecs/mod.rs
git commit -m "feat(ecs): WorldExt hierarchy helpers and pooling on the Disabled filter"
```

### Task 6: Phases, sets, engine schedules and scene-owned schedules

**Files:**
- Create: `src/core/schedule.rs`
- Modify: `src/core/mod.rs` (add `pub mod schedule;`)
- Test: inline

**Interfaces:**
- Produces:
  - Labels: `First`, `PreUpdate`, `FixedUpdate`, `Update`, `PostUpdate`, `Last` (unit structs, `ScheduleLabel`)
  - `pub enum Phase { First, PreUpdate, FixedUpdate, Update, PostUpdate, Last }` (`Copy`)
  - Sets: `FixedSet { Snapshot, Juice, Scene, Game, Physics, Transforms, Trails }`, `UpdateSet { Game }`, `PostUpdateSet { Transforms, Camera, Extract }`
  - `pub struct EngineSchedules` with `new()`, `add_systems<M>(&mut self, phase: Phase, systems: impl IntoScheduleConfigs<ScheduleSystem, M>)`, `run(&mut self, phase: Phase, world: &mut World)`
  - `pub struct SceneSchedules` with `add_systems(phase, systems) -> Result<(), SceneScheduleError>`, `run_fixed(&mut self, world)`, `run_update(&mut self, world)`
  - `pub enum SceneScheduleError { UnsupportedPhase(Phase) }`

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ecs::kernel::{Commands, Component, Query, ResMut, Resource, World};

  #[derive(Resource, Default)]
  struct Log(Vec<&'static str>);

  #[derive(Component)]
  struct Spawned;

  fn push(label: &'static str) -> impl Fn(ResMut<Log>) {
    move |mut log: ResMut<Log>| log.0.push(label)
  }

  #[test]
  fn fixed_sets_run_in_the_declared_order() {
    let mut world = World::new();
    world.init_resource::<Log>();
    let mut s = EngineSchedules::new();
    s.add_systems(Phase::FixedUpdate, push("physics").in_set(FixedSet::Physics));
    s.add_systems(Phase::FixedUpdate, push("snapshot").in_set(FixedSet::Snapshot));
    s.add_systems(Phase::FixedUpdate, push("transforms").in_set(FixedSet::Transforms));
    s.add_systems(Phase::FixedUpdate, push("game").in_set(FixedSet::Game));
    s.run(Phase::FixedUpdate, &mut world);
    assert_eq!(world.resource::<Log>().0, ["snapshot", "game", "physics", "transforms"]);
  }

  #[test]
  fn game_systems_can_order_against_engine_sets() {
    let mut world = World::new();
    world.init_resource::<Log>();
    let mut s = EngineSchedules::new();
    s.add_systems(Phase::FixedUpdate, push("physics").in_set(FixedSet::Physics));
    s.add_systems(Phase::FixedUpdate, push("after").after(FixedSet::Physics));
    s.add_systems(Phase::FixedUpdate, push("before").before(FixedSet::Physics));
    s.run(Phase::FixedUpdate, &mut world);
    assert_eq!(world.resource::<Log>().0, ["before", "physics", "after"]);
  }

  #[test]
  fn scene_schedules_accept_only_fixed_and_update() {
    let mut scene = SceneSchedules::default();
    assert!(scene.add_systems(Phase::FixedUpdate, push("a")).is_ok());
    assert!(scene.add_systems(Phase::Update, push("b")).is_ok());
    assert!(matches!(
      scene.add_systems(Phase::Last, push("c")),
      Err(SceneScheduleError::UnsupportedPhase(Phase::Last))
    ));
  }

  #[test]
  fn scene_schedules_run_and_flush_commands() {
    let mut world = World::new();
    world.init_resource::<Log>();
    let mut scene = SceneSchedules::default();
    scene
      .add_systems(Phase::FixedUpdate, |mut commands: Commands| {
        commands.spawn(Spawned);
      })
      .unwrap();
    scene.add_systems(Phase::Update, push("u")).unwrap();
    scene.run_fixed(&mut world);
    assert_eq!(world.query::<&Spawned>().iter(&world).count(), 1, "commands flushed");
    scene.run_update(&mut world);
    assert_eq!(world.resource::<Log>().0, ["u"]);
    // An empty schedule is fine to run
    SceneSchedules::default().run_fixed(&mut world);
  }

  #[test]
  fn every_phase_runs() {
    let mut world = World::new();
    world.init_resource::<Log>();
    let mut s = EngineSchedules::new();
    for (phase, name) in [
      (Phase::First, "first"),
      (Phase::PreUpdate, "pre"),
      (Phase::FixedUpdate, "fixed"),
      (Phase::Update, "update"),
      (Phase::PostUpdate, "post"),
      (Phase::Last, "last"),
    ] {
      s.add_systems(phase, push(name));
    }
    for phase in Phase::ALL {
      s.run(phase, &mut world);
    }
    assert_eq!(world.resource::<Log>().0, ["first", "pre", "fixed", "update", "post", "last"]);
  }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Add `pub mod schedule;` to `src/core/mod.rs`. Run: `cargo test --lib core::schedule 2>&1 | tail -5`
Expected: compile errors.

- [ ] **Step 3: Implement**

```rust
//! The engine's phases and the schedules behind them.

use bevy_ecs::schedule::ScheduleSystem;

use crate::core::ecs::kernel::{
  IntoScheduleConfigs, Schedule, ScheduleLabel, SingleThreadedExecutor, SystemSet, World,
};

#[derive(ScheduleLabel, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct First;
#[derive(ScheduleLabel, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PreUpdate;
#[derive(ScheduleLabel, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FixedUpdate;
#[derive(ScheduleLabel, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Update;
#[derive(ScheduleLabel, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PostUpdate;
#[derive(ScheduleLabel, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Last;

/// Which phase a system is added to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Phase {
  First,
  PreUpdate,
  FixedUpdate,
  Update,
  PostUpdate,
  Last,
}

impl Phase {
  pub const ALL: [Phase; 6] = [
    Phase::First,
    Phase::PreUpdate,
    Phase::FixedUpdate,
    Phase::Update,
    Phase::PostUpdate,
    Phase::Last,
  ];
}

/// Order inside `FixedUpdate`.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FixedSet {
  Snapshot,
  Juice,
  Scene,
  Game,
  Physics,
  Transforms,
  Trails,
}

/// Order inside `Update`.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UpdateSet {
  Game,
}

/// Order inside `PostUpdate`.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PostUpdateSet {
  Transforms,
  Camera,
  Extract,
}

fn new_schedule(label: impl ScheduleLabel) -> Schedule {
  let mut schedule = Schedule::new(label);
  schedule.set_executor(SingleThreadedExecutor::new());
  schedule
}

/// The engine's six schedules.
pub struct EngineSchedules {
  first: Schedule,
  pre_update: Schedule,
  fixed_update: Schedule,
  update: Schedule,
  post_update: Schedule,
  last: Schedule,
}

impl Default for EngineSchedules {
  fn default() -> Self {
    Self::new()
  }
}

impl EngineSchedules {
  pub fn new() -> Self {
    let mut fixed_update = new_schedule(FixedUpdate);
    fixed_update.configure_sets(
      (
        FixedSet::Snapshot,
        FixedSet::Juice,
        FixedSet::Scene,
        FixedSet::Game,
        FixedSet::Physics,
        FixedSet::Transforms,
        FixedSet::Trails,
      )
        .chain(),
    );
    let mut update = new_schedule(Update);
    update.configure_sets(UpdateSet::Game);
    let mut post_update = new_schedule(PostUpdate);
    post_update.configure_sets(
      (
        PostUpdateSet::Transforms,
        PostUpdateSet::Camera,
        PostUpdateSet::Extract,
      )
        .chain(),
    );
    Self {
      first: new_schedule(First),
      pre_update: new_schedule(PreUpdate),
      fixed_update,
      update,
      post_update,
      last: new_schedule(Last),
    }
  }

  fn get(&mut self, phase: Phase) -> &mut Schedule {
    match phase {
      Phase::First => &mut self.first,
      Phase::PreUpdate => &mut self.pre_update,
      Phase::FixedUpdate => &mut self.fixed_update,
      Phase::Update => &mut self.update,
      Phase::PostUpdate => &mut self.post_update,
      Phase::Last => &mut self.last,
    }
  }

  pub fn add_systems<M>(
    &mut self,
    phase: Phase,
    systems: impl IntoScheduleConfigs<ScheduleSystem, M>,
  ) {
    self.get(phase).add_systems(systems);
  }

  pub fn run(&mut self, phase: Phase, world: &mut World) {
    self.get(phase).run(world);
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SceneScheduleError {
  #[error("scenes can add systems to FixedUpdate and Update only, not {0:?}")]
  UnsupportedPhase(Phase),
}

/// Schedules a scene owns; they run inside the `Game` sets and go away with the scene.
#[derive(Default)]
pub struct SceneSchedules {
  fixed: Option<Schedule>,
  update: Option<Schedule>,
}

impl SceneSchedules {
  pub fn add_systems<M>(
    &mut self,
    phase: Phase,
    systems: impl IntoScheduleConfigs<ScheduleSystem, M>,
  ) -> Result<(), SceneScheduleError> {
    let slot = match phase {
      Phase::FixedUpdate => &mut self.fixed,
      Phase::Update => &mut self.update,
      other => return Err(SceneScheduleError::UnsupportedPhase(other)),
    };
    let schedule = slot.get_or_insert_with(|| new_schedule(SceneOwned));
    schedule.add_systems(systems);
    Ok(())
  }

  pub fn run_fixed(&mut self, world: &mut World) {
    if let Some(schedule) = &mut self.fixed {
      schedule.run(world);
    }
  }

  pub fn run_update(&mut self, world: &mut World) {
    if let Some(schedule) = &mut self.update {
      schedule.run(world);
    }
  }
}

#[derive(ScheduleLabel, Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct SceneOwned;
```

If `bevy_ecs::schedule::ScheduleSystem` is not the name of the boxed system type in 0.20, open `target/doc/bevy_ecs/schedule/index.html` and use the type `Schedule::add_systems` is generic over. `Schedule::run` applies deferred commands at the end of the schedule, which is what the "commands flushed" test checks.

- [ ] **Step 4: Run the tests**

Run: `cargo test --lib core::schedule 2>&1 | tail -5`
Expected: `test result: ok. 5 passed`.

- [ ] **Step 5: Commit**

```bash
git add src/core/schedule.rs src/core/mod.rs
git commit -m "feat(schedule): engine phases, fixed sets and scene-owned schedules"
```

### Task 7: Frame-scoped `Input` resource with an event queue and consumption

**Files:**
- Modify: `src/core/input.rs`
- Test: inline (extend the existing test module)

**Interfaces:**
- Consumes: nothing new.
- Produces (on the type renamed `Input`, with `pub type InputState = Input;` kept until Task 9 removes it):
  - `#[derive(Resource)] pub struct Input`
  - `pub enum InputEvent { Key { key: KeyCode, pressed: bool }, Text(String), MouseButton { button: MouseButton, pressed: bool }, MouseMove { x: f64, y: f64 }, RawMouseMotion { dx: f64, dy: f64 }, Scroll(MouseScrollDelta), Ime(ImeEvent), GamepadConnected { id, name }, GamepadDisconnected(id), GamepadButton { id, button, pressed }, GamepadTrigger { id, button, value }, GamepadAxis { id, axis, value } }` (wrapping what the `handle_*` methods take today)
  - `Input::queue(&mut self, event: InputEvent)`
  - `Input::begin_frame(&mut self)`: clears one-shots and consumed sets, resets the fixed-step index, then applies every queued event in order
  - `Input::set_fixed_step(&mut self, index: u32)`: `just_*` read false when `index > 0`
  - `Input::consume_key(&mut self, key)`, `consume_mouse(&mut self, button)`, `consume_text(&mut self)`, `consume_gamepad_button(&mut self, button)`; existing `*_and_consume` and `consume_*_press` names become aliases of these and are removed from the docs
  - All existing read methods keep their names.

- [ ] **Step 1: Write the failing tests**

Append to the test module in `src/core/input.rs`:

```rust
  #[test]
  fn queued_events_become_visible_at_begin_frame() {
    let mut input = Input::new();
    input.queue(InputEvent::Key { key: KeyCode::Space, pressed: true });
    assert!(!input.key_pressed(KeyCode::Space), "not until the frame begins");
    input.begin_frame();
    assert!(input.key_pressed(KeyCode::Space));
    assert!(input.key_down(KeyCode::Space));
    input.begin_frame();
    assert!(!input.key_pressed(KeyCode::Space), "one-shots last one frame");
    assert!(input.key_down(KeyCode::Space), "held state persists");
  }

  #[test]
  fn just_pressed_is_visible_in_the_first_fixed_step_only() {
    let mut input = Input::new();
    input.queue(InputEvent::Key { key: KeyCode::KeyA, pressed: true });
    input.begin_frame();
    input.set_fixed_step(0);
    assert!(input.key_pressed(KeyCode::KeyA));
    input.set_fixed_step(1);
    assert!(!input.key_pressed(KeyCode::KeyA));
    assert!(input.key_down(KeyCode::KeyA));
    input.begin_frame();
    assert!(input.key_pressed(KeyCode::KeyA) == false);
  }

  #[test]
  fn consumed_input_is_hidden_for_the_rest_of_the_frame() {
    let mut input = Input::new();
    input.queue(InputEvent::Key { key: KeyCode::Escape, pressed: true });
    input.queue(InputEvent::MouseButton { button: MouseButton::Left, pressed: true });
    input.begin_frame();
    assert!(input.key_pressed(KeyCode::Escape));
    input.consume_key(KeyCode::Escape);
    assert!(!input.key_pressed(KeyCode::Escape));
    assert!(!input.key_down(KeyCode::Escape), "consumed means not pressed at all this frame");
    input.consume_mouse(MouseButton::Left);
    assert!(!input.mouse_pressed(MouseButton::Left));
    input.begin_frame();
    assert!(input.key_down(KeyCode::Escape), "the key is still physically held next frame");
  }

  #[test]
  fn text_and_release_events_round_trip() {
    let mut input = Input::new();
    input.queue(InputEvent::Text("hi".into()));
    input.queue(InputEvent::Key { key: KeyCode::KeyH, pressed: true });
    input.queue(InputEvent::Key { key: KeyCode::KeyH, pressed: false });
    input.begin_frame();
    assert_eq!(input.text_input(), Some("hi".to_string()));
    assert!(input.key_pressed(KeyCode::KeyH));
    assert!(input.key_released(KeyCode::KeyH));
    assert!(!input.key_down(KeyCode::KeyH));
  }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib core::input 2>&1 | tail -5`
Expected: compile errors (`Input`, `InputEvent`, `queue` missing).

- [ ] **Step 3: Implement**

In `src/core/input.rs`:

1. Rename `pub struct InputState` to `pub struct Input` and add `#[derive(Resource)]` (import `crate::core::ecs::kernel::Resource`). Add `pub type InputState = Input;` right after it (removed in Task 9).
2. Replace the `Cell<bool>` used for first-reader-wins text consumption with `std::sync::atomic::AtomicBool` (`load`/`store` with `Ordering::Relaxed`), so the type is `Sync`.
3. Add the event queue:

```rust
/// A window or gamepad event, applied at the next `begin_frame`.
#[derive(Debug, Clone)]
pub enum InputEvent {
  Key { key: KeyCode, pressed: bool },
  Text(String),
  MouseButton { button: MouseButton, pressed: bool },
  MouseMove { x: f64, y: f64 },
  RawMouseMotion { dx: f64, dy: f64 },
  Scroll(winit::event::MouseScrollDelta),
  ImeEnabled,
  ImeDisabled,
  ImePreedit { text: String, cursor: Option<(usize, usize)> },
  ImeCommit(String),
  GamepadConnected { id: gilrs::GamepadId, name: String },
  GamepadDisconnected(gilrs::GamepadId),
  GamepadButton { id: gilrs::GamepadId, button: gilrs::Button, pressed: bool },
  GamepadTrigger { id: gilrs::GamepadId, button: gilrs::Button, value: f32 },
  GamepadAxis { id: gilrs::GamepadId, axis: gilrs::Axis, value: f32 },
}
```

with fields `pending: Vec<InputEvent>`, `fixed_step: u32`, `consumed_keys: HashSet<KeyCode>`, `consumed_mouse: HashSet<MouseButton>`, `consumed_gamepad: HashSet<GamepadButton>` on `Input`, and:

```rust
  pub fn queue(&mut self, event: InputEvent) {
    self.pending.push(event);
  }

  /// Start a frame: forget last frame's one-shots and consumptions, then
  /// apply everything queued since the previous frame.
  pub fn begin_frame(&mut self) {
    self.clear_oneshots(); // the body of today's begin_frame
    self.consumed_keys.clear();
    self.consumed_mouse.clear();
    self.consumed_gamepad.clear();
    self.fixed_step = 0;
    let pending = std::mem::take(&mut self.pending);
    for event in pending {
      self.apply(event);
    }
  }

  fn apply(&mut self, event: InputEvent) {
    match event {
      InputEvent::Key { key, pressed } => self.handle_keyboard(key, if pressed { ElementState::Pressed } else { ElementState::Released }),
      InputEvent::Text(text) => self.handle_text_input(&text),
      InputEvent::MouseButton { button, pressed } => self.handle_mouse_button(button, if pressed { ElementState::Pressed } else { ElementState::Released }),
      InputEvent::MouseMove { x, y } => self.handle_mouse_move(x, y),
      InputEvent::RawMouseMotion { dx, dy } => self.handle_raw_mouse_motion(dx, dy),
      InputEvent::Scroll(delta) => self.handle_scroll(delta),
      InputEvent::ImeEnabled => self.handle_ime_enabled(),
      InputEvent::ImeDisabled => self.handle_ime_disabled(),
      InputEvent::ImePreedit { text, cursor } => self.handle_ime_preedit(text, cursor),
      InputEvent::ImeCommit(text) => self.handle_ime_commit(text),
      InputEvent::GamepadConnected { id, name } => self.handle_gamepad_connected(id, name),
      InputEvent::GamepadDisconnected(id) => self.handle_gamepad_disconnected(id),
      InputEvent::GamepadButton { id, button, pressed } => self.handle_gamepad_button(id, button, pressed),
      InputEvent::GamepadTrigger { id, button, value } => self.handle_gamepad_trigger(id, button, value),
      InputEvent::GamepadAxis { id, axis, value } => self.handle_gamepad_axis(id, axis, value),
    }
  }

  /// Which fixed step of the frame is running; one-shots show in step 0 only.
  pub fn set_fixed_step(&mut self, index: u32) {
    self.fixed_step = index;
  }

  pub fn consume_key(&mut self, key: KeyCode) {
    self.consumed_keys.insert(key);
  }

  pub fn consume_mouse(&mut self, button: MouseButton) {
    self.consumed_mouse.insert(button);
  }

  pub fn consume_gamepad_button(&mut self, button: GamepadButton) {
    self.consumed_gamepad.insert(button);
  }
```

4. Make the readers honour both mechanisms: `key_pressed`/`key_released`/`key_repeat` return false when `self.fixed_step > 0` or the key is in `consumed_keys`; `key_down` returns false when consumed; the same for mouse buttons and gamepad buttons. Keep the existing `blocked`/`block_pointer` checks.
5. The `handle_*` methods become private (`fn`), since the engine, the input script and the headless harness now go through `queue`. `suppress_oneshots`/`unsuppress_oneshots` are replaced by `set_fixed_step`. `consume_key_press`, `consume_mouse_press`, `consume_gamepad_button_press` become one-line forwards to the new names; `*_and_consume` stay (they are used by the demos) and call the new names.

- [ ] **Step 4: Run the tests**

Run: `cargo test --lib core::input 2>&1 | tail -5`
Expected: all pass (existing tests that called `handle_*` directly are changed to `queue` + `begin_frame`).

Also run: `cargo check 2>&1 | grep -c "^error"` and fix engine/UI call sites that used `suppress_oneshots`, `unsuppress_oneshots` or `handle_*` so the old engine still compiles: in `engine.rs`, replace `self.input.handle_keyboard(key, state)` with `self.input.queue(InputEvent::Key { key, pressed: state == ElementState::Pressed })` and likewise for the other handlers; call `self.input.begin_frame()` at the top of `step_frame` instead of at the end; replace `suppress_oneshots()` with `set_fixed_step(1)` after the first fixed step and remove `unsuppress_oneshots()`. The old loop keeps working with frame-scoped input until Task 13 replaces it.

- [ ] **Step 5: Commit**

```bash
git add src/core/input.rs src/core/engine.rs src/core/input_script.rs src/core/testing/mod.rs src/core/ui
git commit -m "feat(input): frame-scoped Input resource with an event queue and consumption"
```

### Task 8: Messages module with engine message types and per-scene cursors

**Files:**
- Create: `src/core/messages/mod.rs`, `src/core/messages/builtin.rs`, `src/core/messages/scene.rs`
- Modify: `src/core/mod.rs` (add `pub mod messages;` beside the still-present `pub mod events;`)
- Test: inline in `mod.rs` and `scene.rs`

**Interfaces:**
- Produces:
  - Message types in `builtin.rs`: `CollisionStarted { entity_a, entity_b }`, `CollisionStopped { .. }` (both with `involves(e)` and `other(e)`), `TweenStarted { entity }`, `TweenCompleted { entity }`, `KnockbackStarted { entity }`, `KnockbackFinished { entity }`, `SquashStretchStarted { entity }`, `SquashStretchFinished { entity }`, `AssetReloaded { path: PathBuf }`, `AssetLoadFailed { path: PathBuf, error: String }`, all `#[derive(Message, Debug, Clone)]` (`Copy` where only entities).
  - `pub fn register_message<T: Message>(world: &mut World)`: inits `Messages<T>` and records an updater.
  - `pub fn register_engine_messages(world: &mut World)`.
  - `pub fn update_messages(world: &mut World)`: calls `update()` on every registered `Messages<T>` (runs in `First`).
  - `pub fn clear_messages(world: &mut World)`: empties every registered buffer (`SceneCleanup::MESSAGES`).
  - `pub struct SceneMessages` with `read<T: Message + Clone>(&mut self, world: &World) -> Vec<T>`.

- [ ] **Step 1: Write the failing tests**

`src/core/messages/mod.rs` tests:

```rust
#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ecs::kernel::{Message, MessageReader, Schedule, ScheduleLabel, World};

  #[derive(Message, Debug, Clone, PartialEq)]
  struct Ping(u32);

  #[derive(ScheduleLabel, Debug, Clone, Copy, PartialEq, Eq, Hash)]
  struct Step;

  #[test]
  fn a_message_written_this_frame_is_readable_next_frame_and_then_gone() {
    let mut world = World::new();
    register_message::<Ping>(&mut world);
    world.resource_mut::<Messages<Ping>>().write(Ping(1));
    // frame boundary
    update_messages(&mut world);
    let mut seen = Vec::new();
    let mut s = Schedule::new(Step);
    s.add_systems(move |mut reader: MessageReader<Ping>| {
      for p in reader.read() {
        seen.push(p.clone());
      }
      assert_eq!(seen, [Ping(1)]);
      seen.clear();
    });
    s.run(&mut world);
    update_messages(&mut world);
    update_messages(&mut world);
    assert_eq!(world.resource::<Messages<Ping>>().iter_current_update_messages().count(), 0);
  }

  #[test]
  fn clear_empties_every_registered_buffer() {
    let mut world = World::new();
    register_message::<Ping>(&mut world);
    world.resource_mut::<Messages<Ping>>().write(Ping(9));
    clear_messages(&mut world);
    let mut cursor = world.resource::<Messages<Ping>>().get_cursor();
    assert_eq!(cursor.read(world.resource::<Messages<Ping>>()).count(), 0);
  }
}
```

`src/core/messages/scene.rs` tests:

```rust
#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ecs::kernel::{Message, World};
  use crate::core::messages::{register_message, update_messages};

  #[derive(Message, Debug, Clone, PartialEq)]
  struct Ping(u32);

  #[test]
  fn each_scene_cursor_sees_each_message_once() {
    let mut world = World::new();
    register_message::<Ping>(&mut world);
    let mut a = SceneMessages::default();
    let mut b = SceneMessages::default();
    world.resource_mut::<Messages<Ping>>().write(Ping(1));
    assert_eq!(a.read::<Ping>(&world), [Ping(1)]);
    assert_eq!(a.read::<Ping>(&world), [], "already seen by this scene");
    assert_eq!(b.read::<Ping>(&world), [Ping(1)], "a different scene has its own cursor");
    update_messages(&mut world);
    world.resource_mut::<Messages<Ping>>().write(Ping(2));
    assert_eq!(a.read::<Ping>(&world), [Ping(2)]);
  }

  #[test]
  fn reading_an_unregistered_message_type_is_empty_not_a_panic() {
    let world = World::new();
    let mut a = SceneMessages::default();
    assert!(a.read::<Ping>(&world).is_empty());
  }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Create the three files with only the test modules and `pub mod messages;` in `src/core/mod.rs`. Run: `cargo test --lib core::messages 2>&1 | tail -5`
Expected: compile errors.

- [ ] **Step 3: Implement**

`src/core/messages/mod.rs`:

```rust
//! Buffered messages: written anywhere in a frame, readable throughout the next.

mod builtin;
mod scene;

pub use builtin::*;
pub use scene::SceneMessages;

use crate::core::ecs::kernel::{Message, Messages, Resource, World};

/// One updater per registered message type.
#[derive(Resource, Default)]
struct MessageUpdaters {
  update: Vec<fn(&mut World)>,
  clear: Vec<fn(&mut World)>,
}

fn update_one<T: Message>(world: &mut World) {
  world.resource_mut::<Messages<T>>().update();
}

fn clear_one<T: Message>(world: &mut World) {
  world.resource_mut::<Messages<T>>().clear();
}

/// Make `T` writable and readable in this world. Registering twice is a no-op.
pub fn register_message<T: Message>(world: &mut World) {
  if world.contains_resource::<Messages<T>>() {
    return;
  }
  world.init_resource::<Messages<T>>();
  let mut updaters = world.get_resource_or_insert_with(MessageUpdaters::default);
  updaters.update.push(update_one::<T>);
  updaters.clear.push(clear_one::<T>);
}

/// Advance every buffer by one frame.
pub fn update_messages(world: &mut World) {
  let updaters: Vec<fn(&mut World)> = world
    .get_resource::<MessageUpdaters>()
    .map(|u| u.update.clone())
    .unwrap_or_default();
  for update in updaters {
    update(world);
  }
}

/// Drop everything in every buffer.
pub fn clear_messages(world: &mut World) {
  let clearers: Vec<fn(&mut World)> = world
    .get_resource::<MessageUpdaters>()
    .map(|u| u.clear.clone())
    .unwrap_or_default();
  for clear in clearers {
    clear(world);
  }
}

/// Register the engine's own message types.
pub fn register_engine_messages(world: &mut World) {
  register_message::<CollisionStarted>(world);
  register_message::<CollisionStopped>(world);
  register_message::<TweenStarted>(world);
  register_message::<TweenCompleted>(world);
  register_message::<KnockbackStarted>(world);
  register_message::<KnockbackFinished>(world);
  register_message::<SquashStretchStarted>(world);
  register_message::<SquashStretchFinished>(world);
  register_message::<AssetReloaded>(world);
  register_message::<AssetLoadFailed>(world);
}
```

`src/core/messages/builtin.rs`: copy the ten engine event structs from `src/core/events/builtin.rs` (collision, tween, knockback, squash/stretch, asset), replace `#[derive(Debug, Clone ...)]` with `#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]` (no `Copy` for the two asset ones), keep their `involves`/`other` helpers, and import `crate::core::ecs::kernel::{Entity, Message}`. Do not copy the network, inventory or `StateChanged` types.

`src/core/messages/scene.rs`:

```rust
//! Per-scene read cursors, so a scene sees each message once.

use std::any::{Any, TypeId};
use std::collections::HashMap;

use crate::core::ecs::kernel::{Message, MessageCursor, Messages, World};

#[derive(Default)]
pub struct SceneMessages {
  cursors: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
}

impl SceneMessages {
  /// Every `T` written since this scene last read, cloned out.
  pub fn read<T: Message + Clone>(&mut self, world: &World) -> Vec<T> {
    let Some(messages) = world.get_resource::<Messages<T>>() else {
      return Vec::new();
    };
    let cursor = self
      .cursors
      .entry(TypeId::of::<T>())
      .or_insert_with(|| Box::new(messages.get_cursor()));
    let cursor: &mut MessageCursor<T> = cursor
      .downcast_mut()
      .expect("cursor type is keyed by the message type");
    cursor.read(messages).cloned().collect()
  }
}
```

`MessageCursor::read` takes the `Messages<T>` to read from; if the 0.20 signature wants a different argument shape, match what `target/doc/bevy_ecs/message/struct.MessageCursor.html` shows and keep the `cloned().collect()`.

- [ ] **Step 4: Run the tests**

Run: `cargo test --lib core::messages 2>&1 | tail -5`
Expected: `test result: ok. 4 passed`.

- [ ] **Step 5: Commit**

```bash
git add src/core/messages src/core/mod.rs
git commit -m "feat(messages): buffered engine messages with per-frame update and scene cursors"
```

---

## Migration rules (applied by every task from here on)

Every task in Phases 2 to 5 rewrites call sites with these rules. They are stated once here; a task says "apply the migration rules" and then lists only what is specific to it.

**World API**

| Old (hecs wrapper) | New (`ryo2d::ecs`) |
|---|---|
| `world.spawn((a, b))` returns `Entity` | `world.spawn((a, b)).id()` |
| `world.spawn_empty()` | `world.spawn_empty().id()` |
| `world.get::<T>(e)` returns `Option<Ref<T>>` | `world.get::<T>(e)` returns `Option<&T>` |
| `world.get_mut::<T>(e)` on `&self` | `world.get_mut::<T>(e)` on `&mut self`, returns `Option<Mut<T>>` |
| `world.has::<T>(e)` | `world.get::<T>(e).is_some()` |
| `world.despawn(e)` returns `Result` | `world.despawn(e)` returns `bool` (cascades to children) |
| `world.despawn_recursive(e)` | `world.despawn(e)` |
| `world.contains(e)` | `world.get_entity(e).is_ok()` |
| `world.entities_with::<T>()` then `get` per entity | `world.query::<(Entity, &T)>().iter(&world)` (read) or `world.query::<(Entity, &mut T)>().iter_mut(&mut world)` (write); inside systems, a `Query` parameter |
| `world.entities_with_two::<A, B>()` | `world.query::<(Entity, &A, &B)>().iter(&world)` |
| `world.query::<(Entity, &A)>().iter()` | `world.query::<(Entity, &A)>().iter(&world)` |
| `world.query_mut::<(&mut A, &B)>()` | `world.query::<(&mut A, &B)>().iter_mut(&mut world)` |
| `world.insert_one(e, c)`, `world.add_component(e, c)` | `world.entity_mut(e).insert(c)` |
| `world.insert(e, (a, b))` | `world.entity_mut(e).insert((a, b))` |
| `world.remove_one::<T>(e)` | `world.entity_mut(e).remove::<T>()` (returns `&mut EntityWorldMut`; use `.take::<T>()` to get the value) |
| `world.set_parent(child, parent)` | `world.entity_mut(child).insert(ChildOf(parent))` |
| `world.unparent(child)` | `world.detach_keep_world(child)` |
| `world.children(e)` (`Vec<Entity>`) | `world.get::<Children>(e)` then `.iter()` |
| `world.parent(e)` | `world.get::<ChildOf>(e).map(\|c\| c.parent())` |
| `world.len()` / `is_empty()` | `world.entities().len()` |
| `world.clear()` | `SceneCleanup::WORLD` does this; in tests: `let all: Vec<Entity> = world.query_filtered::<Entity, Allow<Disabled>>().iter(&world).collect(); for e in all { world.despawn(e); }` |
| `world.generation()` / `take_entities_changed()` | removed; nothing needs them |
| `Inactive` / `Active` markers | `Disabled` (insert to deactivate, remove to activate); plain queries skip disabled entities, so every `if world.has::<Inactive>(e) { continue }` is deleted |
| `hecs::Entity`, `hecs::NoSuchEntity` | `Entity`; `world.get_entity(e)` errors |

**Transform**

| Old | New |
|---|---|
| `Transform::new(x, y)` | `Transform::from_xy(x, y)` (origin is now the sprite centre; see anchor below) |
| `Transform::new_3d(x, y, z)` / `t.z` | `Transform::from_xy(x, y)`; `z` goes to `Renderable::with_z_order` |
| `t.x`, `t.y` | `t.translation.x`, `t.translation.y` |
| `t.scale_x`, `t.scale_y`, `.with_scale(sx, sy)` | `t.scale.x`, `t.scale.y`, `.with_scale(Vec2::new(sx, sy))` |
| `.with_uniform_scale(s)` | unchanged |
| `.with_rotation(r)` | unchanged (radians) |
| `.with_pivot(..)`, `.with_pivot_center()`, `.with_anchor(a)` on `Transform` | removed; the sprite's anchor is `Renderable::with_anchor(a)`, default `Anchor::Center`. A sprite previously placed by its top-left corner either keeps its position and adds `.with_anchor(Anchor::TopLeft)`, or keeps the default and moves its position by half its size |
| `.with_offset(dx, dy)` | removed; spawn a child with `Transform::from_xy(dx, dy)` |
| `WorldTransform` (`wt.x`, `wt.rotation`) | `GlobalTransform` (`g.translation()`, `g.rotation()`, `g.scale()`) |
| `PreviousTransform`, `Renderable::interpolated()`, `InterpolationSystem::skip_interpolation(world, e)` | `Interpolated` marker component; `world.skip_interpolation(e)` |
| `Velocity::new(dx, dy)`, `v.dx`, `v.dy` | unchanged (`Velocity { dx, dy }`; the `dz` field is removed) |

**Scene and context**

| Old | New |
|---|---|
| `fn render_ui(&mut self, ui: &mut UiContext) -> SceneAction` | `fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) -> SceneAction` |
| `fn render_gpu(&mut self, gpu: &mut GpuRenderContext)` | `fn render(&mut self, gpu: &mut GpuRenderContext)` |
| `fn render(&mut self, ctx, alpha)` | delete (was never called) |
| `ctx.time.fixed_dt`, `.scaled_dt`, `.real_dt`, `.render_alpha`, `.time_scale`, `.is_hitstop`, `.frame_count` | `ctx.time.fixed.timestep()`, `ctx.time.delta()`, `ctx.time.real.delta()`, `ctx.time.fixed.overstep_fraction()`, `ctx.time.game.speed()`, `ctx.time.game.is_hitstop()`, `ctx.time.frame_count()` |
| `ctx.set_time_scale(s)`, `ctx.time_scale()`, `ctx.hitstop(d)`, `ctx.is_hitstop_active()` | `ctx.time.game.set_speed(s)`, `.speed()`, `.hitstop(d)`, `.is_hitstop()` |
| `ctx.time_control` | removed |
| `ctx.events.emit(e)` / `ctx.events.read::<E>()` / `ctx.events.has::<E>()` | `ctx.write_message(e)` / `ctx.messages::<E>()` / `!ctx.messages::<E>().is_empty()` |
| `ctx.resources.get::<T>()`, `ctx.resource::<T>()`, `ctx.resource_mut::<T>()`, `ctx.resource_or_default::<T>()` | `ctx.resource::<T>()` (`Option<&T>`), `ctx.resource_mut::<T>()` (`Option<Mut<T>>`), `ctx.resource_or_default::<T>()`; `T` derives `Resource` |
| `EngineBuilder::resource(v)` | `EngineBuilder::insert_resource(v)` |
| `EngineBuilder::physics_hz(n)` | `EngineBuilder::fixed_hz(n)` |
| `ctx.effects` (field) | `ctx.effects()` (`Mut<EffectsManager>`) |
| `ctx.lighting` (`&mut Option<LightingRenderer>`), `lighting.set_ambient(..)`, `set_atmosphere`, `set_volumetric` | `ctx.lighting()` (`Mut<LightingSettings>`) with `set_ambient`, `set_atmosphere`, `set_volumetric`, `ambient()`, `atmosphere()`, `volumetric()` |
| `ctx.tilemap` | `ctx.tilemaps()` |
| `ctx.window_request.*`, `ctx.request_resolution(..)`, `ctx.request_display_mode(..)`, `ctx.request_ui_scale(..)` | helpers unchanged; the field is `ctx.window()` |
| `ctx.set_cursor(..)`, `show_cursor`, `hide_cursor`, `set_cursor_locked`, `set_custom_cursor*` | unchanged helpers |
| `ctx.knockback(e, dir, f)`, `ctx.knockback_with_decay(..)` | `Knockback::apply(ctx.world, e, dir, f)`, `Knockback::apply_with_decay(ctx.world, e, dir, f, decay)` |
| `ctx.squash(e, i)`, `squash_x`, `squash_on_axis`, `squash_with_decay` | `SquashStretch::apply(ctx.world, e, i)`, `::apply_x`, `::apply_on_axis`, `::apply_with_decay` |
| `ctx.declare_console_command(..)`, `register_console_command(..)`, `remove_builtin_console_command(..)` | `ctx.console().declare(..)`, `.register(..)`, `.remove_builtin(..)` |
| `ctx.render_pipeline` (field) | `ctx.render_pipeline()` (`Mut<RenderPipeline>`) |
| `ctx.enable_raycast()`, `disable_raycast()`, `raycast_renderer()`, `ctx.raycast` | removed (parked) |
| `ctx.audio: &mut Option<AudioManager>` with `if let Some(audio) = ctx.audio` | `ctx.audio: &mut Audio`; call methods directly (`ctx.audio.play_sound("hit")`), they are no-ops or `Err(AudioError::Unavailable)` without a device |
| `SceneCleanup::EVENTS` | `SceneCleanup::MESSAGES` |
| `SceneCleanup::RAYCAST` | removed |
| `InputState` | `Input` |
| `input.key_pressed_and_consume(k)` and friends | unchanged |

**Physics**

| Old | New |
|---|---|
| `Collider::box_collider(w, h).centered_in(w, h)` | `Collider::box_collider(w, h)` (centred on the entity origin) |
| `.with_offset(x, y)` | unchanged meaning (offset from the origin) |
| `RigidBody::new().with_mass(m)` | `RigidBody::new()` (mass removed) |
| `physics.step(gravity)` | `physics.step(gravity, dt)` |

**Messages**: `Events` struct, `ctx.events`, `#[derive(Event)]`-less plain event structs become `#[derive(Message, Clone)]` types; writers use `MessageWriter<T>` in systems or `ctx.write_message` in scenes.

**Mechanical checks** after each migrated crate or module: `cargo check -p <crate>` clean, `cargo clippy -p <crate> --all-targets` clean, `cargo test -p <crate>` green.

---

## Phase 2: the switch

### Task 9: Replace the hecs world with the kernel and reduce the build to what compiles

**Files:**
- Delete: `src/core/ecs/world.rs`, `src/core/ecs/hierarchy.rs`, `src/core/ecs/pool.rs`, `src/core/ecs/active.rs`, `src/core/ecs/component.rs`, `src/core/events/` (all files), `src/core/resources.rs`, `src/core/timing.rs`
- Rewrite: `src/core/ecs/components.rs` (only `Velocity` and `RenderLayer` remain), `src/core/ecs/mod.rs`
- Modify: `src/core/ecs/renderable.rs`, `material.rs`, `y_sort.rs`, `tween.rs`, `property_tween.rs`, `knockback.rs`, `squash_stretch.rs`, `sprite_effects.rs`; `src/core/graphics/animation.rs`, `tile.rs`; `src/core/lighting/light.rs`, `occluder.rs`, `renderer.rs`; `src/core/trail/trail.rs`, `line.rs`, `renderer.rs`; `src/core/render/frame.rs`, `mod.rs`; `src/core/mod.rs`; `src/lib.rs`; `Cargo.toml` (remove `hecs`)
- Test: existing inline tests in the modified files

**Interfaces:**
- Produces: `ryo2d::ecs` (`pub use crate::core::ecs;` in `lib.rs`) exporting everything in `kernel`, `transform`, `propagate::propagate_transforms`, `interpolate::{snapshot_previous_transforms, render_global}`, `world_ext::WorldExt`, `pooling::*`, `Velocity`, `RenderLayer`, `Renderable`, `RenderStyle`, shadows, `Material`, `SpriteShader`, `SpriteEffect`, `ShaderHandle`, `ShaderParams`, `ParamValue`, `YSort`, `Tween`, `TweenSequence`, `TweenLoop`, `PropertyTween`, `Lerp`, `Ease`, `Knockback`, `SquashStretch`, `SquashAxis`, and `glam::{Mat4, Vec2, Vec3, Vec4}`.
- `Renderable` gains `pub anchor: Anchor` (default `Anchor::Center`) and `with_anchor(Anchor)`; loses `interpolate` and `interpolated()`.

- [ ] **Step 1: Rewrite `src/core/ecs/mod.rs`**

```rust
//! Ryo2D's ECS: the kernel types plus the engine's own components.

pub mod kernel;
pub mod transform;
pub mod propagate;
pub mod interpolate;
pub mod world_ext;
pub mod pooling;

mod components;
mod knockback;
mod lerp;
mod material;
mod property_tween;
mod renderable;
mod sprite_effects;
mod squash_stretch;
mod tween;
mod y_sort;

pub use glam::{Mat4, Vec2, Vec3, Vec4};

pub use components::{RenderLayer, Velocity};
pub use interpolate::{render_global, snapshot_previous_transforms};
pub use kernel::*;
pub use knockback::Knockback;
pub use lerp::Lerp;
pub use material::{Material, ParamValue, ShaderHandle, ShaderParams, SpriteShader};
pub use pooling::{Pool, PoolConfig, PoolExhaustedPolicy, PoolStats};
pub use propagate::propagate_transforms;
pub use property_tween::PropertyTween;
pub use renderable::{BlobShadow, ContactShadow, DropShadow, RenderStyle, Renderable};
pub use sprite_effects::SpriteEffect;
pub use squash_stretch::{SquashAxis, SquashStretch};
pub use transform::{Anchor, GlobalTransform, Interpolated, PreviousGlobalTransform, Transform};
pub use tween::{Ease, Tween, TweenLoop, TweenSequence};
pub use world_ext::WorldExt;
pub use y_sort::YSort;
```

Keep whatever the old `mod.rs` additionally exported from these files (check with `git show HEAD:src/core/ecs/mod.rs`) and add it to the list; drop only the names of deleted types.

- [ ] **Step 2: Rewrite `components.rs` and derive `Component` on every remaining component**

`src/core/ecs/components.rs`:

```rust
//! Small shared components.

use serde::{Deserialize, Serialize};

use super::kernel::Component;

/// Common render layers for organising draw order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RenderLayer {
  Background = -1000,
  Default = 0,
  Foreground = 1000,
  UI = 2000,
}

impl RenderLayer {
  pub fn as_z_order(self) -> i32 {
    self as i32
  }
}

/// Movement per second, in pixels.
#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Velocity {
  pub dx: f32,
  pub dy: f32,
}

impl Velocity {
  pub fn new(dx: f32, dy: f32) -> Self {
    Self { dx, dy }
  }

  pub fn zero() -> Self {
    Self::new(0.0, 0.0)
  }

  pub fn from_vec2(v: glam::Vec2) -> Self {
    Self::new(v.x, v.y)
  }

  pub fn as_vec2(&self) -> glam::Vec2 {
    glam::Vec2::new(self.dx, self.dy)
  }
}
```

The `Replicated` impls for `Transform` and `Velocity` move to `src/core/network/replication/` (Task 17); delete them here.

Add `#[derive(Component)]` (import `crate::core::ecs::kernel::Component`) to: `Renderable`, `Material`, `YSort`, `Tween`, `TweenSequence`, `PropertyTween<T>` (the derive needs `T: Send + Sync + 'static`; add that bound to the struct), `Knockback`, `SquashStretch` in `ecs/`; `Animator`, `AnimationController`, `ValidatedAnimator` in `graphics/animation.rs`; `CompositeTile`, `GroupedCollider` in `graphics/tile.rs`; `Light2D`, `LightOccluder2D` in `lighting/`; `Trail2D`, `Line2D` in `trail/`. `Skeleton`, `SkeletalController`, `SkeletalAnimator`, `ParticleEmitter`, `ParticleAttractor`, `RigidBody`, `Collider`, `NavAgent`, `NavTarget` and the network components get theirs in their own tasks.

In `renderable.rs`: add `pub anchor: Anchor` (import from `super::transform`), initialise to `Anchor::Center` in every constructor, add `pub fn with_anchor(mut self, anchor: Anchor) -> Self`, remove the `interpolate` field, `interpolated()` and their tests. In `tween.rs` and `property_tween.rs`, apply the Transform mapping (`target_x`/`target_y` fields may stay as `f32`s; only reads and writes of `Transform` change). In `y_sort.rs` nothing changes except the derive.

- [ ] **Step 3: Reduce the build**

In `src/core/mod.rs`, comment out with a `// MIGRATION: Task N` marker: `pub mod systems;` (Task 11), `pub mod physics;` (12), `pub mod engine;`, `pub mod game;`, `pub mod scene;` (10 and 13), `pub mod testing;` (15), `pub mod particles;`, `pub mod tilemap;`, `pub mod navigation;`, `pub mod skeleton;` (16), `pub mod network;` (17), `pub mod raycast;` (parked in Task 20), `pub mod inventory;` (moved in Task 19). Delete `pub mod events;`, `pub mod resources;`, `pub mod timing;`. Keep `procgen` (it does not touch the world). In `src/core/render/mod.rs` comment out `mod entities;` and the `pub(crate) use entities::...` line (Task 13 restores it). In `src/core/render/frame.rs`, `LightingFrame.lights` becomes `&'a [(Vec2, Light2D)]` and `occluders` becomes `&'a [(GlobalTransform, LightOccluder2D)]`; update `lighting/renderer.rs` and `lighting/shadow.rs` to read `Vec2`/`GlobalTransform` instead of `Transform.x/y` (apply the mapping). In `trail/renderer.rs`, `add_trail_with_device(.., &Trail2D, &Transform)` and `add_line(&Line2D, x, y)` take a `Vec2` position instead of a `Transform`. In `config/debug.rs` comment out `overlay_lines` and the `OverlayFlags` consumers that take `&World` (Task 12 rewrites them); delete the `render_*_gpu`, `add_gpu_text`, `add_gpu_rect`, `upload_debug_font` functions and their `config/mod.rs` re-exports now (they have no callers).

In `src/lib.rs`: add `pub use crate::core::ecs;` after `pub mod core;`; rewrite the prelude's ECS block to `pub use crate::core::ecs::*;` (one glob; the module controls what it exports) and remove the Task 1 kernel line; comment out the prelude lines for every disabled module and for the deleted `events`, `resources`, `Transition`-independent `SceneManager` block, `TimeControl`, `TimeInfo`, `WindowRequest` (restored or replaced in Task 13). Remove `hecs` from `Cargo.toml`.

- [ ] **Step 4: Make it compile and pass**

Run: `cargo check 2>&1 | grep -E "^(error|warning: unused)" | head -40`. Fix each error in the enabled modules by applying the migration rules until:

Run: `cargo check 2>&1 | tail -3`
Expected: `Finished`.

Run: `cargo test --lib 2>&1 | tail -5`
Expected: all enabled tests pass (tween, property_tween, renderable, material, y_sort, knockback, squash_stretch, sprite_effects, lighting, trail, graphics, ui, asset, config, console, time, input, messages, schedule, transform, propagate, interpolate, world_ext, pooling).

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "refactor(ecs): switch the world to the kernel and reduce the build to the migrated modules"
```

### Task 10: `Scene`, `SceneContext`, `SceneManager` on the new world

**Files:**
- Rewrite: `src/core/scene.rs` (split into `src/core/scene/mod.rs`, `src/core/scene/context.rs`, `src/core/scene/manager.rs`, `src/core/scene/cleanup.rs`)
- Create: `src/core/audio/mod.rs` is NOT created; instead add `pub struct Audio` to `src/core/audio.rs`
- Create: `src/core/lighting/settings.rs` (`LightingSettings` resource: ambient, atmosphere, volumetric)
- Modify: `src/core/mod.rs` (re-enable `pub mod scene;`), `src/lib.rs` (restore the scene prelude block)
- Test: `src/core/scene/manager.rs` tests

**Interfaces:**
- Consumes: `Time`, `Input`, `Camera`, `AssetManager` (re-exported as `Assets`), `EffectsManager`, `LightingSettings`, `TilemapManager`, `WindowRequest`, `CursorRequest`, `RenderPipeline`, `CommandRegistry`, `SceneSchedules`, `SceneMessages`, `UiContext`, `GpuRenderContext` (the last two are still in disabled modules: declare them in `scene/mod.rs` behind `use` and keep `pub mod game;` disabled until Task 13; to let this task compile, move `GpuRenderContext` into `src/core/graphics/gpu_context.rs` now, with `world: &'a World` as the kernel world).
- Produces:
  - `pub trait Scene: 'static` as in the spec.
  - `pub struct SceneContext<'a> { pub world, pub time, pub input, pub camera, pub assets, pub audio, .. }`
  - `pub(crate) struct SceneSlot { scene: Box<dyn Scene>, schedules: SceneSchedules, messages: SceneMessages, scope_id: u64 }`
  - `pub struct SceneManager` with `with_scene`, `push`, `pop`, `switch`, `reset`, `update(&mut self, world, audio) -> SceneAction`, `ui_pass(&mut self, world, audio, ui) -> SceneAction`, `render(&mut self, gpu)`, `handle_action(action, world, audio) -> bool`, `update_transition(dt, world, audio)`, `run_fixed_systems(world)`, `run_update_systems(world)`, `is_physics_paused()`, `is_transitioning()`, `transition_overlay_alpha()`, `transition_overlay_color()`, `debug_stack()`, `scene_names()`, `len()`, `is_empty()`
  - `pub struct Audio` wrapping `Option<AudioManager>`; `Audio::new(Option<AudioManager>)`; one forwarding method per `AudioManager` public method
  - `pub struct LightingSettings { ambient: AmbientLight, atmosphere: Atmosphere, volumetric: VolumetricSettings }` (`Resource`, with getters and setters)
  - `pub struct PhysicsPaused(pub bool)` (`Resource`) in `scene/mod.rs`

- [ ] **Step 1: Write the failing tests** (`src/core/scene/manager.rs`)

```rust
#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ecs::{Component, Disabled, Allow, Entity, Resource, ResMut, Transform, World};
  use crate::core::messages::{register_message, update_messages};
  use crate::core::schedule::Phase;
  use std::sync::{Arc, Mutex};

  #[derive(Resource, Default)]
  struct Log(Vec<&'static str>);

  #[derive(Component)]
  struct Thing;

  #[derive(crate::core::ecs::Message, Clone)]
  struct Ping;

  type Shared = Arc<Mutex<Vec<&'static str>>>;

  struct Rec {
    log: Shared,
    name: &'static str,
    action: Option<fn() -> SceneAction>,
    updates_below: bool,
  }

  impl Scene for Rec {
    fn on_enter(&mut self, ctx: &mut SceneContext) {
      self.log.lock().unwrap().push("enter");
      let name = self.name;
      ctx
        .add_systems(Phase::FixedUpdate, move |mut log: ResMut<Log>| log.0.push(name))
        .unwrap();
      ctx.world.spawn((Transform::IDENTITY, Thing));
    }
    fn on_exit(&mut self, _ctx: &mut SceneContext) {
      self.log.lock().unwrap().push("exit");
    }
    fn on_pause(&mut self, _ctx: &mut SceneContext) {
      self.log.lock().unwrap().push("pause");
    }
    fn on_resume(&mut self, _ctx: &mut SceneContext, _r: Option<Box<dyn std::any::Any>>) {
      self.log.lock().unwrap().push("resume");
    }
    fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
      self.log.lock().unwrap().push("update");
      self.action.take().map(|f| f()).unwrap_or(SceneAction::Continue)
    }
    fn updates_below(&self) -> bool {
      self.updates_below
    }
  }

  fn world() -> (World, Audio) {
    let mut world = World::new();
    crate::core::engine::init_world_resources_for_tests(&mut world);
    world.init_resource::<Log>();
    register_message::<Ping>(&mut world);
    (world, Audio::new(None))
  }

  fn rec(log: &Shared, name: &'static str) -> Box<Rec> {
    Box::new(Rec { log: log.clone(), name, action: None, updates_below: false })
  }

  #[test]
  fn push_pop_lifecycle_and_scene_systems() {
    let log: Shared = Arc::default();
    let (mut world, mut audio) = world();
    let mut m = SceneManager::with_scene(rec(&log, "a"));
    m.enter_initial(&mut world, &mut audio);
    m.run_fixed_systems(&mut world);
    assert_eq!(world.resource::<Log>().0, ["a"]);

    let action = m.update(&mut world, &mut audio);
    assert!(matches!(action, SceneAction::Continue));
    assert!(m.handle_action(SceneAction::Push(rec(&log, "b")), &mut world, &mut audio));
    m.run_fixed_systems(&mut world);
    assert_eq!(world.resource::<Log>().0, ["a", "b"], "only the top scene's systems run");

    assert!(m.handle_action(SceneAction::Pop, &mut world, &mut audio));
    m.run_fixed_systems(&mut world);
    assert_eq!(world.resource::<Log>().0, ["a", "b", "a"], "b's systems went with it");
    assert_eq!(
      *log.lock().unwrap(),
      ["enter", "update", "pause", "enter", "exit", "resume"]
    );
  }

  #[test]
  fn updates_below_runs_lower_scenes_too() {
    let log: Shared = Arc::default();
    let (mut world, mut audio) = world();
    let mut m = SceneManager::with_scene(rec(&log, "a"));
    m.enter_initial(&mut world, &mut audio);
    let mut top = rec(&log, "b");
    top.updates_below = true;
    m.handle_action(SceneAction::Push(top), &mut world, &mut audio);
    m.run_fixed_systems(&mut world);
    assert_eq!(world.resource::<Log>().0, ["a", "b"]);
    assert!(!m.is_physics_paused(), "a top scene that updates below does not pause physics");
  }

  #[test]
  fn switch_cleans_the_world_and_messages_but_not_resources() {
    let log: Shared = Arc::default();
    let (mut world, mut audio) = world();
    let mut m = SceneManager::with_scene(rec(&log, "a"));
    m.enter_initial(&mut world, &mut audio);
    world.spawn((Transform::IDENTITY, Thing, Disabled));
    world.resource_mut::<crate::core::ecs::Messages<Ping>>().write(Ping);
    assert_eq!(world.query_filtered::<Entity, Allow<Disabled>>().iter(&world).count(), 2);
    m.handle_action(SceneAction::Switch(rec(&log, "b")), &mut world, &mut audio);
    assert_eq!(
      world.query_filtered::<&Thing, Allow<Disabled>>().iter(&world).count(),
      1,
      "only b's entity remains"
    );
    update_messages(&mut world);
    let mut cursor = world.resource::<crate::core::ecs::Messages<Ping>>().get_cursor();
    assert_eq!(cursor.read(world.resource::<crate::core::ecs::Messages<Ping>>()).count(), 0);
    assert!(world.get_resource::<Log>().is_some(), "resources survive cleanup");
  }

  #[test]
  fn stacked_opaque_scene_pauses_physics() {
    let log: Shared = Arc::default();
    let (mut world, mut audio) = world();
    let mut m = SceneManager::with_scene(rec(&log, "a"));
    m.enter_initial(&mut world, &mut audio);
    m.handle_action(SceneAction::Push(rec(&log, "pause-menu")), &mut world, &mut audio);
    assert!(m.is_physics_paused());
  }

  #[test]
  fn scoped_context_puts_resources_back() {
    let (mut world, mut audio) = world();
    let mut slot = SceneSlot::new(Box::new(Rec {
      log: Arc::default(),
      name: "x",
      action: None,
      updates_below: false,
    }));
    SceneContext::scoped(&mut world, &mut audio, &mut slot, |ctx| {
      ctx.camera.zoom = 2.0;
      ctx.time.game.set_speed(0.5);
      assert!(ctx.world.get_resource::<crate::core::time::Time>().is_none());
    });
    assert_eq!(world.resource::<crate::core::graphics::Camera>().zoom, 2.0);
    assert_eq!(world.resource::<crate::core::time::Time>().game.speed(), 0.5);
  }
}
```

`crate::core::engine::init_world_resources_for_tests` does not exist yet (engine is disabled); for this task create `pub(crate) fn init_world_resources(world: &mut World, config: &WorldResourceConfig)` in `src/core/scene/mod.rs` and a `#[cfg(test)] pub(crate) fn init_world_resources_for_tests(world)` next to it that inserts: `Time::new(60)`, `Input::new()`, `Camera::centered_at(400.0, 300.0, 800, 600)`, `AssetManager::new()` (as `Assets`), `EffectsManager::new()`, `LightingSettings::default()`, `TilemapManager::new()`, `WindowRequest::default()`, `CursorRequest::default()`, `RenderPipeline::default_2d()`, `CommandRegistry::new()` (with built-ins), `PhysicsPaused(false)`, and calls `register_engine_messages`. Task 13 moves the production version into `engine.rs` and keeps the test helper's path by re-exporting it. Adjust the test's path accordingly (`crate::core::scene::init_world_resources_for_tests`).

- [ ] **Step 2: Run the tests to verify they fail**

Re-enable `pub mod scene;` in `src/core/mod.rs`. Run: `cargo test --lib core::scene 2>&1 | tail -5`
Expected: compile errors.

- [ ] **Step 3: Implement `Audio`, `LightingSettings`, `PhysicsPaused`**

In `src/core/audio.rs` append:

```rust
/// The engine's audio, usable whether or not a device was found.
pub struct Audio {
  manager: Option<AudioManager>,
}

impl Audio {
  pub fn new(manager: Option<AudioManager>) -> Self {
    Self { manager }
  }

  /// Whether an output device is available.
  pub fn is_available(&self) -> bool {
    self.manager.is_some()
  }

  /// The underlying manager, when a device exists.
  pub fn manager(&mut self) -> Option<&mut AudioManager> {
    self.manager.as_mut()
  }
}
```

then one forwarding method per public method of `AudioManager`, in the same order they appear in the file, each shaped like:

```rust
  pub fn play_sound(&mut self, name: &str) -> AudioResult<SoundHandle> {
    match &mut self.manager {
      Some(m) => m.play_sound(name),
      None => Err(AudioError::Unavailable),
    }
  }

  pub fn set_master_volume(&mut self, volume: f32) {
    if let Some(m) = &mut self.manager {
      m.set_master_volume(volume);
    }
  }
```

Add `AudioError::Unavailable` (`#[error("no audio device")]`) to `src/core/error.rs`. Methods returning a value with no `Result` (`master_volume()`, `is_music_playing()`) return the type's default when unavailable.

`src/core/lighting/settings.rs`:

```rust
//! What a scene controls about lighting; the renderer reads it each frame.

use crate::core::ecs::Resource;

use super::{AmbientLight, Atmosphere, VolumetricSettings};

#[derive(Resource, Debug, Clone, Default)]
pub struct LightingSettings {
  ambient: AmbientLight,
  atmosphere: Atmosphere,
  volumetric: VolumetricSettings,
}

impl LightingSettings {
  pub fn ambient(&self) -> &AmbientLight {
    &self.ambient
  }
  pub fn set_ambient(&mut self, ambient: AmbientLight) {
    self.ambient = ambient;
  }
  pub fn atmosphere(&self) -> &Atmosphere {
    &self.atmosphere
  }
  pub fn set_atmosphere(&mut self, atmosphere: Atmosphere) {
    self.atmosphere = atmosphere;
  }
  pub fn volumetric(&self) -> &VolumetricSettings {
    &self.volumetric
  }
  pub fn set_volumetric(&mut self, volumetric: VolumetricSettings) {
    self.volumetric = volumetric;
  }
}
```

`LightingRenderer` keeps its own `set_ambient`/`set_atmosphere`/`set_volumetric`; Task 13's frame copies the settings into the renderer before the lighting pass. If `AmbientLight`, `Atmosphere` or `VolumetricSettings` lack `Default`, derive it with the values their `new()` uses.

In `src/core/scene/mod.rs`: `#[derive(Resource, Debug, Clone, Copy, Default)] pub struct PhysicsPaused(pub bool);`

- [ ] **Step 4: Implement the scene module**

`src/core/scene/mod.rs`: the `Scene` trait exactly as in the spec, `SceneAction` and `Transition`/`SlideDirection`/`downcast_return` moved unchanged from the old file, `SceneCleanup` with `MESSAGES` replacing `EVENTS` and `RAYCAST` removed (`ALL = 0b0111_1111`, `DEFAULT = 0b0101_1111`), `PhysicsPaused`, `init_world_resources`, and `pub use context::SceneContext; pub use manager::SceneManager; pub(crate) use manager::SceneSlot;`.

`src/core/scene/context.rs`:

```rust
use std::any::Any;

use crate::core::asset::AssetManager as Assets;
use crate::core::audio::Audio;
use crate::core::console::{CommandContext, CommandRegistry, CommandResult};
use crate::core::cursor::CursorRequest;
use crate::core::ecs::{Bundle, Entity, Message, Messages, Mut, Resource, World, WorldExt};
use crate::core::graphics::Camera;
use crate::core::input::Input;
use crate::core::lighting::LightingSettings;
use crate::core::messages::register_message;
use crate::core::render::RenderPipeline;
use crate::core::schedule::{Phase, SceneScheduleError};
use crate::core::shaders::EffectsManager;
use crate::core::texture::{TextureContext, TextureManager};
use crate::core::tilemap::TilemapManager;
use crate::core::time::Time;
use crate::core::window::WindowRequest; // moved from game.rs in this task: create src/core/window.rs holding WindowRequest and DisplayMode re-export

use super::SceneSlot;

/// What a scene callback can reach.
pub struct SceneContext<'a> {
  pub world: &'a mut World,
  pub time: &'a mut Time,
  pub input: &'a mut Input,
  pub camera: &'a mut Camera,
  pub assets: &'a mut Assets,
  pub audio: &'a mut Audio,
  slot: &'a mut SceneSlot,
}

impl<'a> SceneContext<'a> {
  /// Build a context for `slot` over `world`, run `f`, put the scoped resources back.
  pub(crate) fn scoped<R>(
    world: &mut World,
    audio: &mut Audio,
    slot: &mut SceneSlot,
    f: impl FnOnce(&mut SceneContext<'_>) -> R,
  ) -> R {
    world.resource_scope(|world, mut time: Mut<Time>| {
      world.resource_scope(|world, mut input: Mut<Input>| {
        world.resource_scope(|world, mut camera: Mut<Camera>| {
          world.resource_scope(|world, mut assets: Mut<Assets>| {
            let mut ctx = SceneContext {
              world,
              time: &mut time,
              input: &mut input,
              camera: &mut camera,
              assets: &mut assets,
              audio,
              slot,
            };
            f(&mut ctx)
          })
        })
      })
    })
  }

  // --- systems and messages ---

  pub fn add_systems<M>(
    &mut self,
    phase: Phase,
    systems: impl crate::core::ecs::IntoScheduleConfigs<bevy_ecs::schedule::ScheduleSystem, M>,
  ) -> Result<(), SceneScheduleError> {
    self.slot.schedules.add_systems(phase, systems)
  }

  pub fn messages<T: Message + Clone>(&mut self) -> Vec<T> {
    self.slot.messages.read::<T>(self.world)
  }

  pub fn write_message<T: Message>(&mut self, message: T) {
    register_message::<T>(self.world);
    self.world.resource_mut::<Messages<T>>().write(message);
  }

  // --- resources ---

  pub fn resource<T: Resource>(&self) -> Option<&T> {
    self.world.get_resource::<T>()
  }

  pub fn resource_mut<T: Resource>(&mut self) -> Option<Mut<'_, T>> {
    self.world.get_resource_mut::<T>()
  }

  pub fn resource_or_default<T: Resource + Default>(&mut self) -> Mut<'_, T> {
    self.world.get_resource_or_insert_with(T::default)
  }

  pub fn insert_resource<T: Resource>(&mut self, value: T) {
    self.world.insert_resource(value);
  }

  pub fn effects(&mut self) -> Mut<'_, EffectsManager> {
    self.world.resource_mut::<EffectsManager>()
  }

  pub fn lighting(&mut self) -> Mut<'_, LightingSettings> {
    self.world.resource_mut::<LightingSettings>()
  }

  pub fn tilemaps(&mut self) -> Mut<'_, TilemapManager> {
    self.world.resource_mut::<TilemapManager>()
  }

  pub fn render_pipeline(&mut self) -> Mut<'_, RenderPipeline> {
    self.world.resource_mut::<RenderPipeline>()
  }

  pub fn window(&mut self) -> Mut<'_, WindowRequest> {
    self.world.resource_mut::<WindowRequest>()
  }

  pub fn cursor(&mut self) -> Mut<'_, CursorRequest> {
    self.world.resource_mut::<CursorRequest>()
  }

  pub fn textures(&mut self) -> TextureContext<'_> {
    let gpu = self.world.resource::<crate::core::graphics::Gpu>().clone();
    let manager = self.world.resource_mut::<TextureManager>().into_inner();
    TextureContext::new(manager, gpu)
  }

  pub fn console(&mut self) -> ConsoleScope<'_> {
    ConsoleScope {
      registry: self.world.resource_mut::<CommandRegistry>().into_inner(),
      scope_id: self.slot.scope_id,
    }
  }

  // --- hierarchy and interpolation sugar ---

  pub fn spawn_child(&mut self, parent: Entity, bundle: impl Bundle) -> Entity {
    self.world.spawn_child(parent, bundle)
  }

  pub fn skip_interpolation(&mut self, entity: Entity) {
    self.world.skip_interpolation(entity);
  }

  // --- screen and window helpers (unchanged bodies from the old file) ---
  // screen_size, screen_width, screen_height, scale_factor, display_mode,
  // request_display_mode, request_resolution, request_display_change,
  // request_ui_scale, set_cursor, set_cursor_visible, show_cursor, hide_cursor,
  // set_cursor_confined, set_cursor_locked, set_custom_cursor, set_custom_cursor_centered
  // read `scale_factor` and `display_mode` from a `WindowInfo { scale_factor: f64, display_mode: DisplayMode }` resource.
}

/// Scene-scoped console command registration.
pub struct ConsoleScope<'a> {
  registry: &'a mut CommandRegistry,
  scope_id: u64,
}

impl ConsoleScope<'_> {
  pub fn declare(&mut self, name: impl Into<String>, description: impl Into<String>, verbose: Option<impl Into<String>>) {
    self.registry.declare(name, description, verbose);
  }

  pub fn register<F>(&mut self, name: &str, handler: F) -> bool
  where
    F: Fn(&[&str], &mut CommandContext) -> CommandResult + Send + Sync + 'static,
  {
    self.registry.register_handler(name, self.scope_id, handler)
  }

  pub fn remove_builtin(&mut self, name: &str) -> bool {
    self.registry.remove_builtin(name)
  }
}
```

The `TextureContext::new(manager, gpu)` signature changes to take the `Gpu` resource (`#[derive(Resource, Clone)] pub struct Gpu { pub device: Arc<wgpu::Device>, pub queue: Arc<wgpu::Queue> }` added to `src/core/graphics/mod.rs`); update `src/core/texture/mod.rs` accordingly. Create `src/core/window.rs` with `WindowRequest` (moved from `game.rs`, unchanged), `WindowInfo` and `pub use crate::core::config::save::DisplayMode;`, and add `pub mod window;` to `core/mod.rs`. Create `src/core/graphics/gpu_context.rs` with `GpuRenderContext` moved from `game.rs` (its `world` field is the kernel `World`), re-exported from `graphics/mod.rs`.

`src/core/scene/manager.rs`: port `SceneManager` with these changes: the stack is `Vec<SceneSlot>`; every callback goes through `SceneContext::scoped(world, audio, slot, |ctx| slot.scene.on_enter(ctx))` (take the slot out of the `Vec` with `std::mem::replace` or index access so the borrow works: use `let slot = &mut self.scenes[i]; SceneContext::scoped(world, audio, slot, |ctx| ..)` where `scoped` borrows `slot` mutably and the closure receives `ctx` whose `slot` field is the same borrow; inside `scoped`, call the scene through `ctx.slot.scene` by temporarily taking it: `let mut scene = std::mem::replace(&mut slot.scene, Box::new(Empty));` before building the context and restoring after; define a private `struct Empty; impl Scene for Empty {}`); `enter_initial(world, audio)` calls `on_enter` for the initial scene (Task 13's engine calls it after the GPU exists); `update(world, audio) -> SceneAction` runs `update_transition` then `Scene::update` for the top scene and for scenes below with `updates_below`, with `dt = world.resource::<Time>().fixed.timestep()`; `ui_pass(world, audio, ui) -> SceneAction` calls `Scene::ui` bottom-up from the first non-transparent scene; `render(gpu)` calls `Scene::render` the same way; `run_fixed_systems(world)` / `run_update_systems(world)` run the slots' schedules for the top scene and any `updates_below` chain; `handle_action` and the transitions port unchanged; `is_physics_paused()` as today and it also writes `PhysicsPaused` when asked via `sync_physics_paused(&self, world)`; `pop` keeps `skip_next_update` removed (input consumption makes it unnecessary).

`src/core/scene/cleanup.rs`: `execute_cleanup(cleanup, world, audio)`: `WORLD` despawns every entity including disabled ones (the query from the migration rules); `CAMERA` resets the `Camera` resource with `camera.reset()`; `EFFECTS` clears `EffectsManager`; `LIGHTING` resets `LightingSettings` to default; `TIME` sets `game.set_speed(1.0)`, `game.resume()`, and clears hitstop (add `GameClock::clear_hitstop()` to `time.rs`); `MUSIC` calls `audio.stop_music()`; `MESSAGES` calls `clear_messages(world)`.

- [ ] **Step 5: Run the tests**

Run: `cargo test --lib core::scene core::audio core::lighting 2>&1 | tail -5`
Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat(scene): Scene trait, scoped SceneContext and scene-owned schedules on the new world"
```

### Task 11: Juice systems as schedule systems

**Files:**
- Rewrite: `src/core/systems/animation.rs`, `tween.rs`, `knockback.rs`, `squash_stretch.rs`, `mod.rs`
- Delete: `src/core/systems/interpolation.rs`
- Keep disabled until their tasks: `skeleton.rs` (Task 16), `trail.rs` (Task 13), `physics.rs` (Task 12)
- Modify: `src/core/ecs/knockback.rs`, `squash_stretch.rs` (add the `apply*` associated functions that replace the context helpers)
- Modify: `src/core/mod.rs` (re-enable `pub mod systems;`)

**Interfaces:**
- Produces: `pub fn register_juice_systems(schedules: &mut EngineSchedules)` adding, in `FixedSet::Juice`, chained: `animate_sprites`, `update_tweens`, `update_tween_sequences`, `update_property_tweens`, `update_knockback`, `update_squash_stretch`.
- `Knockback::apply(world: &mut World, entity, direction: Vec2, force: f32)`, `Knockback::apply_with_decay(world, entity, direction, force, decay)`; `SquashStretch::apply(world, entity, intensity)`, `::apply_x`, `::apply_on_axis(world, entity, intensity, axis)`, `::apply_with_decay(world, entity, intensity, decay)`.

- [ ] **Step 1: Port the tests**

Each file keeps its tests, rewritten to build a `World`, register the messages the system writes (`register_message::<TweenCompleted>` and so on), run the system with a one-system `Schedule` (as in Task 4's tests), and assert on components and on `Messages<T>` via a cursor. Add one new test per system asserting that a `Disabled` entity is untouched (spawn it with `Disabled`, run, check nothing changed).

- [ ] **Step 2: Run to verify they fail**

Re-enable `pub mod systems;` with `mod skeleton;`, `mod trail;`, `mod physics;` commented out. Run: `cargo test --lib core::systems 2>&1 | tail -5`
Expected: compile errors.

- [ ] **Step 3: Implement**

Shape of each system (tween shown; the others follow the same pattern):

```rust
use crate::core::ecs::{Entity, Query, MessageWriter, Res, Transform, Renderable, Tween};
use crate::core::messages::{TweenCompleted, TweenStarted};
use crate::core::time::Time;

/// Advance every `Tween`, writing `Transform` and `Renderable` targets.
pub fn update_tweens(
  time: Res<Time>,
  mut query: Query<(Entity, &mut Tween, &mut Transform, Option<&mut Renderable>)>,
  mut started: MessageWriter<TweenStarted>,
  mut completed: MessageWriter<TweenCompleted>,
) {
  let dt = time.fixed.timestep();
  for (entity, mut tween, mut transform, renderable) in &mut query {
    let was_started = tween.just_started();
    tween.update(dt);
    tween.apply(&mut transform, renderable.as_deref_mut());
    if !was_started && tween.just_started() {
      started.write(TweenStarted { entity });
    }
    if tween.just_finished() {
      completed.write(TweenCompleted { entity });
    }
  }
}
```

Rules: the dt is `time.fixed.timestep()` (these run inside `FixedUpdate`); `Tween` and `PropertyTween` keep their state machines, with `Tween::apply` writing `translation`/`rotation`/`scale` and `Renderable.alpha`/`tint` (a tween targeting only alpha or tint on an entity without `Transform` must still advance: use two queries, one `With<Transform>` and one `Without<Transform>` for alpha/tint-only tweens, so the audit's "silently does nothing" bug is fixed); knockback writes `Velocity` when present and otherwise `Transform.translation`, never both (the double-move bug); squash/stretch re-reads the base scale when a tween is also present by storing `base_scale: Option<Vec2>` that resets when the effect finishes. `register_juice_systems` chains the six systems inside `FixedSet::Juice`.

- [ ] **Step 4: Run the tests**

Run: `cargo test --lib core::systems core::ecs 2>&1 | tail -5`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "refactor(systems): juice systems as fixed-update schedule systems"
```

### Task 12: Physics as fixed-update systems with the dt fix and change-detected sync

**Files:**
- Modify: `src/core/physics/rapier.rs`, `components.rs`, `collision.rs`, `mod.rs`
- Delete: `src/core/physics/spatial.rs` and the legacy `check_collision`/`AABB`/`Circle`/`Collision`/`CollisionPair`/`CollisionResult`/`PhysicsWorld` items in `collision.rs`
- Rewrite: `src/core/systems/physics.rs`
- Modify: `src/core/config/debug.rs` (`overlay_lines` over the new world)
- Modify: `src/core/mod.rs` (re-enable `pub mod physics;`)
- Test: inline tests in `rapier.rs` and `systems/physics.rs`

**Interfaces:**
- Produces:
  - `RapierPhysicsWorld` derives `Resource`; `step(&mut self, gravity: (f32, f32), dt: f32)`; `register_entity(entity, &Transform, Option<&Velocity>, &RigidBody, &Collider)` (origin-centred colliders plus `collider.offset`); `set_transform(entity, &Transform)` (teleport, wakes), `set_kinematic_target(entity, &Transform)`, `set_velocity(entity, &Velocity)` (wakes), `read_back(&self) -> impl Iterator<Item = (Entity, Vec2, Vec2)>` over awake non-kinematic bodies; `drain_collision_events()` unchanged.
  - `PhysicsConfig` derives `Resource`.
  - `pub fn register_physics_systems(schedules: &mut EngineSchedules)` adding to `FixedSet::Physics`, chained and `run_if(physics_enabled)`: `register_new_bodies`, `unregister_removed_bodies`, `sync_to_physics`, `step_physics`, `sync_from_physics`, `emit_collision_messages`.
  - `pub fn overlay_lines(world: &mut World, camera: &Camera, flags: OverlayFlags) -> Vec<Line2D>` in `config/debug.rs`.

- [ ] **Step 1: Write the failing tests** (`src/core/systems/physics.rs`)

```rust
#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ecs::{Disabled, Entity, Transform, Velocity, World};
  use crate::core::physics::{Collider, PhysicsConfig, RapierPhysicsWorld, RigidBody};
  use crate::core::schedule::{EngineSchedules, Phase};
  use crate::core::scene::PhysicsPaused;
  use crate::core::time::Time;

  fn world(hz: u32) -> (World, EngineSchedules) {
    let mut world = World::new();
    world.insert_resource(Time::new(hz));
    world.insert_resource(RapierPhysicsWorld::new());
    world.insert_resource(PhysicsConfig { gravity: (0.0, 980.0) });
    world.insert_resource(PhysicsPaused(false));
    crate::core::messages::register_engine_messages(&mut world);
    let mut schedules = EngineSchedules::new();
    register_physics_systems(&mut schedules);
    (world, schedules)
  }

  fn drop_for_one_second(hz: u32) -> f32 {
    let (mut world, mut s) = world(hz);
    let e = world
      .spawn((Transform::from_xy(0.0, 0.0), RigidBody::new(), Collider::circle_collider(8.0)))
      .id();
    for _ in 0..hz {
      s.run(Phase::FixedUpdate, &mut world);
    }
    world.get::<Transform>(e).unwrap().translation.y
  }

  #[test]
  fn fall_distance_per_second_is_independent_of_the_fixed_rate() {
    let at60 = drop_for_one_second(60);
    let at120 = drop_for_one_second(120);
    assert!((at60 - at120).abs() / at60 < 0.03, "{at60} vs {at120}");
    assert!((at60 - 490.0).abs() < 60.0, "roughly g/2 after one second: {at60}");
  }

  #[test]
  fn added_bodies_register_and_removed_or_disabled_bodies_unregister() {
    let (mut world, mut s) = world(60);
    let e = world
      .spawn((Transform::IDENTITY, RigidBody::new(), Collider::box_collider(4.0, 4.0)))
      .id();
    s.run(Phase::FixedUpdate, &mut world);
    assert!(world.resource::<RapierPhysicsWorld>().is_registered(e));
    world.entity_mut(e).insert(Disabled);
    s.run(Phase::FixedUpdate, &mut world);
    assert!(!world.resource::<RapierPhysicsWorld>().is_registered(e));
    world.entity_mut(e).remove::<Disabled>();
    s.run(Phase::FixedUpdate, &mut world);
    assert!(world.resource::<RapierPhysicsWorld>().is_registered(e));
    world.despawn(e);
    s.run(Phase::FixedUpdate, &mut world);
    assert!(!world.resource::<RapierPhysicsWorld>().is_registered(e));
  }

  #[test]
  fn a_resting_body_is_not_woken_by_the_sync() {
    let (mut world, mut s) = world(60);
    let floor = world
      .spawn((Transform::from_xy(0.0, 100.0), RigidBody::kinematic(), Collider::box_collider(400.0, 20.0)))
      .id();
    let ball = world
      .spawn((Transform::from_xy(0.0, 50.0), RigidBody::new(), Collider::circle_collider(10.0)))
      .id();
    for _ in 0..600 {
      s.run(Phase::FixedUpdate, &mut world);
    }
    let _ = floor;
    assert!(world.resource::<RapierPhysicsWorld>().is_sleeping(ball), "ten seconds at rest");
    let before = world.get::<Transform>(ball).unwrap().translation;
    for _ in 0..60 {
      s.run(Phase::FixedUpdate, &mut world);
    }
    assert!(world.resource::<RapierPhysicsWorld>().is_sleeping(ball), "sync must not wake it");
    assert_eq!(world.get::<Transform>(ball).unwrap().translation, before);
  }

  #[test]
  fn a_game_write_to_transform_teleports_and_a_kinematic_moves_by_transform() {
    let (mut world, mut s) = world(60);
    let kin = world
      .spawn((Transform::from_xy(0.0, 0.0), RigidBody::kinematic(), Collider::box_collider(4.0, 4.0)))
      .id();
    s.run(Phase::FixedUpdate, &mut world);
    world.get_mut::<Transform>(kin).unwrap().translation.x = 50.0;
    s.run(Phase::FixedUpdate, &mut world);
    let pos = world.resource::<RapierPhysicsWorld>().body_position(kin).unwrap();
    assert!((pos.x - 50.0).abs() < 1e-3);
  }

  #[test]
  fn paused_physics_runs_no_step() {
    let (mut world, mut s) = world(60);
    let e = world
      .spawn((Transform::IDENTITY, RigidBody::new(), Collider::circle_collider(8.0)))
      .id();
    world.resource_mut::<PhysicsPaused>().0 = true;
    for _ in 0..30 {
      s.run(Phase::FixedUpdate, &mut world);
    }
    assert_eq!(world.get::<Transform>(e).unwrap().translation.y, 0.0);
  }
}
```

Add `is_sleeping(entity) -> bool` and `body_position(entity) -> Option<Vec2>` (pixels) to `RapierPhysicsWorld`.

- [ ] **Step 2: Run to verify they fail**

Re-enable `pub mod physics;` and `mod physics;` in `systems/mod.rs`. Run: `cargo test --lib core::systems::physics 2>&1 | tail -5`
Expected: compile errors.

- [ ] **Step 3: Implement**

In `rapier.rs`: derive `Resource`; add `pub fn step(&mut self, gravity, dt: f32)` that sets `self.integration_parameters.dt = dt` before the pipeline step; register colliders at `collider.offset` (a `Vec2` field replacing `offset_x`/`offset_y`; `with_offset(x, y)` stays) with no pivot or `center_in_bounds` logic (delete `centered_in` and the field); keep rotation locked (physics sub-project); delete `sync_kinematic_from_ecs`, `sync_dynamic_from_ecs`, `sync_velocities_from_ecs`, `sync_to_ecs`; add:

```rust
  pub fn set_transform(&mut self, entity: Entity, transform: &Transform) {
    if let Some(body) = self.entity_to_body.get(&entity).and_then(|h| self.rigid_body_set.get_mut(*h)) {
      let p = transform.translation / PIXELS_PER_METER;
      if body.is_kinematic() {
        body.set_next_kinematic_translation(vector![p.x, p.y]);
      } else {
        body.set_translation(vector![p.x, p.y], true);
      }
    }
  }

  pub fn set_velocity(&mut self, entity: Entity, velocity: &Velocity) {
    if let Some(body) = self.entity_to_body.get(&entity).and_then(|h| self.rigid_body_set.get_mut(*h))
      && !body.is_kinematic()
    {
      body.set_linvel(vector![velocity.dx, velocity.dy] / PIXELS_PER_METER, true);
    }
  }

  /// Awake dynamic bodies: (entity, position px, velocity px/s)
  pub fn read_back(&self) -> impl Iterator<Item = (Entity, Vec2, Vec2)> + '_ {
    self.entity_to_body.iter().filter_map(|(&entity, &handle)| {
      let body = self.rigid_body_set.get(handle)?;
      if body.is_kinematic() || body.is_sleeping() {
        return None;
      }
      let p = body.translation();
      let v = body.linvel();
      Some((entity, Vec2::new(p.x, p.y) * PIXELS_PER_METER, Vec2::new(v.x, v.y) * PIXELS_PER_METER))
    })
  }

  pub fn is_sleeping(&self, entity: Entity) -> bool {
    self.entity_to_body.get(&entity).and_then(|h| self.rigid_body_set.get(*h)).is_some_and(|b| b.is_sleeping())
  }

  pub fn body_position(&self, entity: Entity) -> Option<Vec2> {
    let body = self.rigid_body_set.get(*self.entity_to_body.get(&entity)?)?;
    let p = body.translation();
    Some(Vec2::new(p.x, p.y) * PIXELS_PER_METER)
  }
```

In `systems/physics.rs`:

```rust
use crate::core::ecs::{
  Added, Allow, Changed, Disabled, Entity, DetectChangesMut, MessageWriter, Or, Query, RemovedComponents, Res, ResMut, Transform, Velocity, With,
};
use crate::core::messages::{CollisionStarted, CollisionStopped};
use crate::core::physics::{Collider, GroupedCollider, PhysicsConfig, RapierPhysicsWorld, RigidBody};
use crate::core::scene::PhysicsPaused;
use crate::core::schedule::{EngineSchedules, FixedSet, Phase};
use crate::core::time::Time;

pub fn physics_enabled(paused: Res<PhysicsPaused>) -> bool {
  !paused.0
}

pub fn register_new_bodies(
  mut physics: ResMut<RapierPhysicsWorld>,
  added: Query<(Entity, &Transform, Option<&Velocity>, &RigidBody, Option<&Collider>, Option<&GroupedCollider>), Added<RigidBody>>,
  re_enabled: Query<(Entity, &Transform, Option<&Velocity>, &RigidBody, Option<&Collider>, Option<&GroupedCollider>), With<RigidBody>>,
  mut removed_disabled: RemovedComponents<Disabled>,
) {
  for item in &added {
    register(&mut physics, item);
  }
  for entity in removed_disabled.read() {
    if let Ok(item) = re_enabled.get(entity) {
      register(&mut physics, item);
    }
  }
}

fn register(
  physics: &mut RapierPhysicsWorld,
  (entity, transform, velocity, body, collider, grouped): (Entity, &Transform, Option<&Velocity>, &RigidBody, Option<&Collider>, Option<&GroupedCollider>),
) {
  if physics.is_registered(entity) {
    return;
  }
  match (collider, grouped) {
    (Some(collider), _) => physics.register_entity(entity, transform, velocity, body, collider),
    (None, Some(grouped)) => physics.register_grouped_entity(entity, transform, velocity, body, grouped),
    (None, None) => {}
  }
}

pub fn unregister_removed_bodies(
  mut physics: ResMut<RapierPhysicsWorld>,
  mut removed: RemovedComponents<RigidBody>,
  disabled: Query<Entity, (With<RigidBody>, Added<Disabled>, Allow<Disabled>)>,
) {
  for entity in removed.read() {
    physics.unregister_entity(entity);
  }
  for entity in &disabled {
    physics.unregister_entity(entity);
  }
}

pub fn sync_to_physics(
  mut physics: ResMut<RapierPhysicsWorld>,
  moved: Query<(Entity, &Transform), (With<RigidBody>, Changed<Transform>)>,
  pushed: Query<(Entity, &Velocity), (With<RigidBody>, Changed<Velocity>)>,
) {
  for (entity, transform) in &moved {
    physics.set_transform(entity, transform);
  }
  for (entity, velocity) in &pushed {
    physics.set_velocity(entity, velocity);
  }
}

pub fn step_physics(mut physics: ResMut<RapierPhysicsWorld>, config: Res<PhysicsConfig>, time: Res<Time>) {
  physics.step(config.gravity, time.fixed.timestep());
}

pub fn sync_from_physics(
  physics: Res<RapierPhysicsWorld>,
  mut bodies: Query<(&mut Transform, Option<&mut Velocity>), With<RigidBody>>,
) {
  for (entity, position, velocity) in physics.read_back() {
    let Ok((mut transform, vel)) = bodies.get_mut(entity) else {
      continue;
    };
    // Physics' own writes must not look like game writes next step
    transform.bypass_change_detection().translation = position;
    if let Some(mut vel) = vel {
      let v = vel.bypass_change_detection();
      v.dx = velocity.x;
      v.dy = velocity.y;
    }
  }
}

pub fn emit_collision_messages(
  mut physics: ResMut<RapierPhysicsWorld>,
  mut started: MessageWriter<CollisionStarted>,
  mut stopped: MessageWriter<CollisionStopped>,
) {
  for event in physics.drain_collision_events() {
    if event.is_started() {
      started.write(CollisionStarted { entity_a: event.entity_a, entity_b: event.entity_b });
    } else {
      stopped.write(CollisionStopped { entity_a: event.entity_a, entity_b: event.entity_b });
    }
  }
}

pub fn register_physics_systems(schedules: &mut EngineSchedules) {
  schedules.add_systems(
    Phase::FixedUpdate,
    (
      register_new_bodies,
      unregister_removed_bodies,
      sync_to_physics,
      step_physics,
      sync_from_physics,
      emit_collision_messages,
    )
      .chain()
      .in_set(FixedSet::Physics)
      .run_if(physics_enabled),
  );
}
```

One subtlety: `sync_to_physics` sees `Changed<Transform>` for a body registered this step (its `Transform` was just added), which teleports it to where it already is; harmless. `Mut::bypass_change_detection` keeps the physics write-back from re-triggering `sync_to_physics` next step, which is what makes the sleep test pass.

`config/debug.rs`: rewrite `overlay_lines` to take `&mut World` and use `world.query::<(&GlobalTransform, &Collider)>()`, `(&GlobalTransform, &Velocity)` and `&GlobalTransform` for colliders, velocities and origins; the collider rectangle is centred on `global.translation() + collider.offset`.

- [ ] **Step 4: Run the tests**

Run: `cargo test --lib core::physics core::systems core::config 2>&1 | tail -5`
Expected: all pass (port the `rapier.rs` tests that used `sync_*` to the new methods; delete the `spatial.rs` and legacy collision tests with their code).

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "refactor(physics): fixed-update systems, Rapier dt from the fixed clock, change-detected sync"
```

### Task 13: The engine: resources, builder, the single frame loop, extraction, lighting and trail collection

**Files:**
- Rewrite: `src/core/engine.rs` (split into `src/core/engine/mod.rs` (struct, builder, resolve), `src/core/engine/frame.rs` (the loop), `src/core/engine/window.rs` (`ApplicationHandler`, window and cursor requests, dev windows), `src/core/engine/extract.rs` (draw-list building, lights, trails, overlays))
- Delete: `src/core/game.rs` (contents moved in Task 10)
- Restore: `src/core/render/entities.rs` (rewritten against the query), `src/core/systems/trail.rs` (as a system in `FixedSet::Trails`)
- Modify: `src/core/graphics/renderer.rs` (surface errors, present mode), `src/core/graphics/sprite_gpu.rs` (`SpriteGlobals` gets `real_time` and `game_time`), `src/core/shaders/` (globals), `src/core/capture/`, `src/core/input_script.rs` (queue-based), `src/core/mod.rs`, `src/lib.rs`
- Test: `src/core/engine/mod.rs` builder tests (ported), `src/core/engine/frame.rs` tests with an offscreen renderer

**Interfaces:**
- Consumes: everything from Tasks 2 to 12.
- Produces:
  - `EngineBuilder`: as today minus `physics_hz`/`resource`, plus `fixed_hz(u32)`, `insert_resource(impl Resource)`, `add_systems(Phase, systems)`, `add_message::<T: Message>()`.
  - `Engine::run()`, `pub(crate) fn frame(&mut self, real_delta: Option<Duration>) -> FrameOutcome` (`None` = wall clock), `pub(crate) fn world(&self)`, `world_mut()`, `input_mut()` (goes through the world), `fixed_timestep()`, `read_frame()`, `apply_headless_requests()`, `init_gpu(..)`, `run_offscreen_frames()`.
  - `pub(crate) fn init_world_resources(world: &mut World, config: &EngineConfig, gpu: Gpu, texture_manager: TextureManager)` and the test helper from Task 10 moved here.
  - `pub(crate) struct SceneHost { pub scenes: SceneManager, pub audio: Audio }` stored with `world.insert_non_send(host)`; exclusive systems `scene_update_system(world: &mut World)` (in `FixedSet::Scene`) and `scene_fixed_systems(world: &mut World)` (in `FixedSet::Game`), `scene_update_systems` (in `UpdateSet::Game`), `camera_shake_system` (`PostUpdateSet::Camera`, real dt), `advance_clock_system` (`First`), `publish_input_system` (`First`), `update_messages_system` (`First`), `asset_pump_system` (`PreUpdate`), `clear_trackers_system` (`Last`).

- [ ] **Step 1: Write the failing loop tests** (`src/core/engine/frame.rs`)

```rust
#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ecs::{Component, Interpolated, Resource, ResMut, Transform, With, Query};
  use crate::core::scene::{Scene, SceneAction, SceneContext};
  use crate::core::schedule::Phase;
  use std::time::Duration;

  #[derive(Resource, Default)]
  struct Steps(u32);

  #[derive(Component)]
  struct Mover;

  struct Fixture;
  impl Scene for Fixture {
    fn on_enter(&mut self, ctx: &mut SceneContext) {
      ctx.world.spawn((Transform::from_xy(0.0, 0.0), Mover, Interpolated));
      ctx
        .add_systems(Phase::FixedUpdate, |mut q: Query<&mut Transform, With<Mover>>, mut steps: ResMut<Steps>| {
          for mut t in &mut q {
            t.translation.x += 10.0;
          }
          steps.0 += 1;
        })
        .unwrap();
    }
    fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
      SceneAction::Continue
    }
  }

  fn engine() -> Engine {
    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      panic!("GPU required for this test; skipped by the harness without one");
    };
    let mut engine = EngineBuilder::new()
      .resolution(64, 48)
      .fixed_hz(50)
      .insert_resource(Steps::default())
      .start_scene(Fixture)
      .build_headless();
    engine.init_gpu(WgpuRenderer::new_offscreen(device, queue, 64, 48), 1.0);
    engine
  }

  #[test]
  fn two_fixed_steps_in_one_frame_snapshot_between_them() {
    if crate::core::testing::shared_gpu().is_err() {
      return;
    }
    let mut e = engine();
    e.frame(Some(Duration::from_millis(40)));
    assert_eq!(e.world().resource::<Steps>().0, 2);
    let mut query = e
      .world_mut()
      .query::<(&crate::core::ecs::GlobalTransform, &crate::core::ecs::PreviousGlobalTransform)>();
    let (global, prev) = query
      .iter(e.world())
      .next()
      .map(|(g, p)| (*g, *p))
      .unwrap();
    assert!((global.translation().x - 20.0).abs() < 1e-4);
    assert!((prev.global.translation().x - 10.0).abs() < 1e-4, "previous is the state after step one");
  }

  #[test]
  fn a_frame_with_no_step_keeps_the_previous_snapshot() {
    if crate::core::testing::shared_gpu().is_err() {
      return;
    }
    let mut e = engine();
    e.frame(Some(Duration::from_millis(20))); // exactly one step
    e.frame(Some(Duration::from_millis(5))); // none
    assert_eq!(e.world().resource::<Steps>().0, 1);
    let alpha = e.world().resource::<crate::core::time::Time>().fixed.overstep_fraction();
    assert!((alpha - 0.25).abs() < 1e-3, "{alpha}");
  }

  #[test]
  fn input_is_seen_once_per_frame_by_fixed_logic() {
    // covered end to end in Task 15's headless tests; here: the loop sets the fixed step index
    if crate::core::testing::shared_gpu().is_err() {
      return;
    }
    let mut e = engine();
    e.world_mut().resource_mut::<crate::core::input::Input>().queue(
      crate::core::input::InputEvent::Key { key: winit::keyboard::KeyCode::Space, pressed: true },
    );
    e.frame(Some(Duration::from_millis(40)));
    // after the frame the one-shot is cleared by the next begin_frame, so check the step index reset
    assert_eq!(e.world().resource::<crate::core::input::Input>().fixed_step(), 0);
  }
}
```

Add `pub fn fixed_step(&self) -> u32` to `Input`.

- [ ] **Step 2: Run to verify they fail**

Re-enable `pub mod engine;`, `pub mod testing;` (only `shared_gpu` is needed; keep the rest of `testing/mod.rs` behind `#[cfg(any())]` until Task 15 by wrapping `HeadlessEngine` in a `mod headless` that is commented out). Run: `cargo test --lib core::engine::frame 2>&1 | tail -5`
Expected: compile errors.

- [ ] **Step 3: Implement the engine**

`src/core/engine/mod.rs` keeps `EngineConfig` (rename `physics_hz` to `fixed_hz`), `WindowMode`, `EngineBuilder` and its `resolve` logic unchanged except: remove `resource`, `physics_hz`; add

```rust
  pub fn fixed_hz(mut self, hz: u32) -> Self {
    self.config.fixed_hz = hz.max(1);
    self
  }

  pub fn insert_resource(mut self, value: impl Resource) -> Self {
    self.pending_resources.push(Box::new(move |world: &mut World| world.insert_resource(value)));
    self
  }

  pub fn add_systems<M>(mut self, phase: Phase, systems: impl IntoScheduleConfigs<ScheduleSystem, M>) -> Self {
    self.schedules.add_systems(phase, systems);
    self
  }

  pub fn add_message<T: Message>(mut self) -> Self {
    self.pending_resources.push(Box::new(|world: &mut World| register_message::<T>(world)));
    self
  }
```

with fields `pending_resources: Vec<Box<dyn FnOnce(&mut World)>>` and `schedules: EngineSchedules` on the builder (`insert_resource` closures must be `FnOnce + 'static`; `Resource: 'static` already).

`Engine` struct:

```rust
pub struct Engine {
  config: EngineConfig,
  window: Option<Arc<Window>>,
  renderer: Option<WgpuRenderer>,
  world: World,
  schedules: EngineSchedules,
  // GPU renderers the passes draw with (engine-owned, not resources)
  sprite_gpu: Option<SpriteGpuRenderer>,
  lighting: Option<LightingRenderer>,
  trail_renderer: Option<TrailRenderer>,
  entity_drawer: EntityDrawer,
  draw_list: DrawList,
  batch_collector: BatchCollector,
  // UI
  ui_font: BitmapFont,
  ui_buffer: Vec<u8>,
  // Dev tools and window plumbing: console_state, capture, input_script, gilrs, custom_cursor_cache,
  // cursor_locked, surface_needs_reconfigure, frame_timings, smoothed_timings, overlay_mode,
  // debug_stats, smoothed_stats, debug_keys_enabled, profiler/logger windows and focus flags,
  // pending_* flags, scale_factor, ui_scale, console_quit_requested, scene_quit_requested,
  // first_frame_rendered, last_window_size, vsync, offscreen_run, offscreen_refused,
  // game_name, game_version  (all as today)
}
```

`init_world_resources` inserts: `Time::new(config.fixed_hz)`, `Input::new()`, `Camera::centered_at(w/2, h/2, w, h)`, `AssetManager::new()` (hot reload enabled as today), `EffectsManager::new()`, `LightingSettings::default()`, `TilemapManager::new()`, `WindowRequest::default()`, `CursorRequest::default()`, `WindowInfo { scale_factor: 1.0, display_mode }`, `RenderPipeline::default_2d()`, `Gpu { device, queue }`, `TextureManager::new()`, `PhysicsPaused(false)`, `config.physics.clone()` (`PhysicsConfig`), `RapierPhysicsWorld::new()`, `config.debug.clone()` (`DebugConfig`), `CommandRegistry` with built-ins and the builder's declarations, `FallbackErrorHandler(handler)` where `handler` panics under `cfg!(debug_assertions)` and otherwise logs with `engine_error!`; then `register_engine_messages`, then the builder's `pending_resources` closures, then `world.insert_non_send(SceneHost { scenes, audio })`.

Engine systems are registered in `Engine::new` on `self.schedules`:

| Phase / set | System |
|---|---|
| `First` | `advance_clock_system` (`world.resource_mut::<Time>().begin_frame()` or `advance(delta)` when the loop stored an explicit delta in a `PendingDelta(Option<Duration>)` resource), then `publish_input_system` (`Input::begin_frame`), then `update_messages_system` |
| `PreUpdate` | `asset_pump_system`: `assets.process_async_loads(); #[cfg(debug_assertions)] assets.process_reloads()` writing `AssetReloaded`/`AssetLoadFailed` messages |
| `FixedUpdate::Snapshot` | `snapshot_previous_transforms` |
| `FixedUpdate::Juice` | `register_juice_systems` |
| `FixedUpdate::Scene` | `scene_update_system` |
| `FixedUpdate::Game` | `scene_fixed_systems` |
| `FixedUpdate::Physics` | `register_physics_systems` |
| `FixedUpdate::Transforms` | `propagate_transforms` |
| `FixedUpdate::Trails` | `record_trail_points` (the old `TrailSystem::update` as a `Query<(&GlobalTransform, &mut Trail2D)>` system with the fixed dt) |
| `Update::Game` | `scene_update_systems` |
| `PostUpdate::Transforms` | `propagate_transforms` |
| `PostUpdate::Camera` | `camera_shake_system` (`camera.update_shake(time.real.delta())`) |
| `Last` | `clear_trackers_system` (`world.clear_trackers()`; every 1024 frames `world.check_change_ticks()`) |

The scene exclusive systems:

```rust
fn with_host<R>(world: &mut World, f: impl FnOnce(&mut World, &mut SceneHost) -> R) -> R {
  let mut host = world
    .remove_non_send::<SceneHost>() // `remove_non_send_resource` if that is the 0.20 name
    .expect("SceneHost is inserted at engine start and only taken by these systems");
  let out = f(world, &mut host);
  world.insert_non_send(host);
  out
}

fn scene_update_system(world: &mut World) {
  with_host(world, |world, host| {
    let SceneHost { scenes, audio } = host;
    let action = scenes.update(world, audio);
    let keep_running = scenes.handle_action(action, world, audio);
    if !keep_running {
      world.insert_resource(QuitRequested);
    }
    scenes.sync_physics_paused(world);
  });
}

fn scene_fixed_systems(world: &mut World) {
  with_host(world, |world, host| host.scenes.run_fixed_systems(world));
}

fn scene_update_systems(world: &mut World) {
  with_host(world, |world, host| host.scenes.run_update_systems(world));
}
```

`#[derive(Resource)] struct QuitRequested;` read by the loop.

`src/core/engine/frame.rs`:

```rust
impl Engine {
  /// One frame. `real_delta` of `None` means wall clock.
  pub(crate) fn frame(&mut self, real_delta: Option<Duration>) -> FrameOutcome {
    self.frame_timings = FrameTimings::default();
    self.world.insert_resource(PendingDelta(real_delta));

    // Scripted presses and gamepad events are queued before First publishes input
    let next_frame = self.capture.presented_frames() + 1;
    self.input_script.before_frame(next_frame, self.world.resource_mut::<Input>().into_inner());
    self.poll_gamepads();

    self.schedules.run(Phase::First, &mut self.world);
    self.handle_debug_keys(); // F1-F5, F9-F12, backquote; consumes through Input::consume_key
    self.schedules.run(Phase::PreUpdate, &mut self.world);
    self.console_tick(); // real dt; handles its own input while focused

    let update_start = Instant::now();
    self.ui_pass(); // Task 14 fills this in; until then it draws the debug overlays and console only

    let mut step = 0;
    loop {
      let ready = self.world.resource_mut::<Time>().try_expend_fixed_step();
      if !ready {
        break;
      }
      {
        let mut input = self.world.resource_mut::<Input>();
        input.set_fixed_step(step);
      }
      self.world.resource_mut::<Time>().set_in_fixed(true);
      self.schedules.run(Phase::FixedUpdate, &mut self.world);
      self.world.resource_mut::<Time>().set_in_fixed(false);
      step += 1;
      if self.world.contains_resource::<QuitRequested>() {
        break;
      }
    }
    self.world.resource_mut::<Input>().set_fixed_step(0);
    self.frame_timings.update = update_start.elapsed();

    if self.world.remove_resource::<QuitRequested>().is_some() || self.console_quit_requested {
      return FrameOutcome::Quit;
    }

    self.schedules.run(Phase::Update, &mut self.world);
    self.schedules.run(Phase::PostUpdate, &mut self.world);

    let outcome = match self.present_frame() {
      PresentOutcome::Fatal(error) => return FrameOutcome::Fatal(error),
      PresentOutcome::Presented => FrameOutcome::Presented,
      PresentOutcome::Skipped => FrameOutcome::Skipped,
    };

    self.schedules.run(Phase::Last, &mut self.world);
    self.input_script.after_frame(self.world.resource_mut::<Input>().into_inner());
    outcome
  }
}
```

`PendingDelta` is read by `advance_clock_system`: `Some(d)` → `time.advance(d)`, `None` → `time.begin_frame()`. `poll_gamepads` queues `InputEvent::Gamepad*` from gilrs into `Input` (no per-frame `String` allocation: compare the gamepad id before building the name). `handle_debug_keys` is the old F-key block using `input.key_pressed` + `input.consume_key`. `console_tick` is the old console update/handle-input block with `time.real.delta()`.

`present_frame` (`extract.rs`): ports today's body with these replacements: `scene_manager.render_gpu` becomes `with_host(.., |_, host| host.scenes.render(&mut gpu_ctx))` where `gpu_ctx` is built over `&self.world`; lights are collected by `world.query::<(&GlobalTransform, &Light2D)>()` into `Vec<(Vec2, Light2D)>` (cloning `Light2D`, a small `Copy`-able struct: derive `Copy` if it is not) and occluders by `world.query::<(&GlobalTransform, &LightOccluder2D)>()` into `Vec<(GlobalTransform, LightOccluder2D)>` (clone; the shadow geometry reads polygons); before the lighting pass copy `LightingSettings` into the renderer (`lighting.set_ambient(settings.ambient().clone())` and so on); trails and lines by `world.query::<(&GlobalTransform, &Trail2D)>()` and `(&GlobalTransform, &Line2D)`; overlays by `overlay_lines(&mut self.world, camera, flags)`; `EntityFrame { world: &self.world, camera, alpha: time.fixed.overstep_fraction(), size }` with no cache field; shader globals get `time.real.elapsed()` and `time.game.elapsed()`; the surface error match becomes

```rust
        Err(wgpu::SurfaceError::Outdated) | Err(wgpu::SurfaceError::Timeout) => {
          self.surface_needs_reconfigure = true;
        }
        Err(wgpu::SurfaceError::Lost) => { /* resize as today */ }
        Err(wgpu::SurfaceError::OutOfMemory) => {
          engine_error!(module: "Render", "out of GPU memory");
          outcome = PresentOutcome::Fatal("out of GPU memory".into());
        }
```

`render/entities.rs` is rewritten so `EntityDrawer::collect` runs one query:

```rust
    let mut items: Vec<(Entity, DrawOrder)> = Vec::new();
    let mut q = frame_world.query::<(Entity, &Renderable, &GlobalTransform, Option<&YSort>)>();
    for (entity, renderable, global, y_sort) in q.iter(frame.world) {
      if !renderable.visible {
        continue;
      }
      let order = match y_sort {
        Some(y) => DrawOrder::y_sorted(renderable.z_order, y.calculate_sort_y(global.translation().y, renderable.sprite.height() as f32)),
        None => DrawOrder::plain(renderable.z_order),
      };
      items.push((entity, order));
    }
    items.sort_by(|(_, a), (_, b)| a.cmp(b));
    for (entity, _) in items {
      collect_entity(frame, entity, &mut out);
    }
```

where `frame.world` is `&World` and `collect_entity` reads `(&Renderable, &GlobalTransform, Option<&PreviousGlobalTransform>, Option<&Material>, Option<&YSort>)` through `world.get`, computes `let drawn = render_global(global, previous, frame.alpha);`, takes `x, y, rotation, scale` from `drawn`, and places the sprite by `renderable.anchor.to_pivot()` instead of the transform pivot (the pivot-to-centre offset code stays, fed by the anchor). `DrawOrder` stays. `EntityFrame.cache` is deleted. The query needs `&mut World` to build; `EntityDrawer::collect` takes `&mut World` for that reason (the engine holds `&mut self.world` there anyway).

`graphics/renderer.rs`: the present mode is `Fifo` when `vsync` is true, otherwise `Mailbox` if available else `Immediate` (and `presentation_blocks` is `vsync`). The window loop's `wait_for_target_frame_time` runs only when `!vsync` (replace `should_apply_frame_limiting` with that rule and update its three tests). `TimeState::wait_for_target_frame_time` moves to `frame.rs` as `fn pace_frame(&self)` using `Time::frame_start()` and `config.target_fps`.

`window.rs`: `ApplicationHandler` as today, except `window_event` queues `InputEvent`s into `self.world.resource_mut::<Input>()`, `RedrawRequested` calls `self.frame(None)`, the unfocused tick calls `self.frame(None)` too (the old `tick_frame` is deleted), `about_to_wait` applies `WindowRequest`/`CursorRequest` from the world resources (taking them with `std::mem::take`), and the `WindowInfo` resource is updated on scale-factor changes.

`run_offscreen_frames` calls `self.frame(Some(fixed_timestep))` per iteration. `init_gpu` creates the renderers, inserts `Gpu` and `TextureManager`, calls `init_world_resources` if not yet done (headless builds call it in `new`), and then `with_host(.., |world, host| host.scenes.enter_initial(world, &mut host.audio))`.

- [ ] **Step 4: Run the tests**

Run: `cargo test --lib core::engine core::render 2>&1 | tail -5`
Expected: all pass (port the old builder tests: `physics_hz` becomes `fixed_hz`, `resource` becomes `insert_resource`).

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(engine): one frame loop over the phase schedules with query-based extraction"
```

### Task 14: The scene UI pass with an input snapshot

**Files:**
- Modify: `src/core/ui/immediate/context.rs` (`UiContext` owns a `UiInput` snapshot), `src/core/ui/immediate/mod.rs`
- Create: `src/core/ui/immediate/input.rs` (`UiInput`)
- Modify: `src/core/engine/frame.rs` (`ui_pass`), `src/core/console/mod.rs` (reads `UiInput` where it read `InputState`)
- Test: `src/core/ui/immediate/input.rs` tests; `src/core/engine/frame.rs` test

**Interfaces:**
- Produces: `pub struct UiInput` with the read methods `UiContext` uses today (`mouse_position`, `mouse_x`, `mouse_y`, `mouse_down`, `mouse_pressed`, `mouse_released`, `scroll_delta`, `key_down`, `key_pressed`, `key_released`, `key_repeat`, `text_input`, `ime_*`, `gamepad_button_pressed`, `left_stick`), `UiInput::snapshot(&Input) -> UiInput`, `UiInput::consumed(&self) -> UiConsumed { keys: Vec<KeyCode>, mouse: Vec<MouseButton>, text: bool }`, and `UiInput::apply_consumed(&self, &mut Input)`. `UiContext::new_scaled(buffer, w, h, UiInput, &BitmapFont, scale)`.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::input::{Input, InputEvent};
  use winit::keyboard::KeyCode;

  #[test]
  fn snapshot_reads_and_consumption_flows_back() {
    let mut input = Input::new();
    input.queue(InputEvent::Key { key: KeyCode::Escape, pressed: true });
    input.queue(InputEvent::MouseMove { x: 10.0, y: 20.0 });
    input.begin_frame();
    let mut ui = UiInput::snapshot(&input);
    assert!(ui.key_pressed(KeyCode::Escape));
    assert_eq!(ui.mouse_position(), (10.0, 20.0));
    ui.consume_key(KeyCode::Escape);
    assert!(!ui.key_pressed(KeyCode::Escape));
    ui.apply_consumed(&mut input);
    assert!(!input.key_pressed(KeyCode::Escape), "gameplay no longer sees it");
  }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --lib core::ui::immediate::input 2>&1 | tail -3`
Expected: compile errors.

- [ ] **Step 3: Implement**

`UiInput` copies the fields it needs from `Input` (positions, deltas, the `down`/`pressed`/`released` sets for keys and mouse, the text input `Option<String>` taken with `input.text_input()` semantics applied once, the IME state, and the first gamepad's buttons and left stick) and keeps `consumed_keys`, `consumed_mouse`, `consumed_text` sets. Every read method checks the consumed sets. `apply_consumed` calls `Input::consume_key`, `consume_mouse`, `consume_text` for each. In `UiContext` replace `input: &'a InputState` with `input: UiInput`, update the four constructors, and replace the `click_consumed`/`escape_consumed` plumbing with `self.input.consume_mouse(Left)`/`consume_key(Escape)` at the places that set those flags; `click_consumed()`/`escape_consumed()` stay as thin readers for the console. `set_mouse_occluded` and `consume_click` stay.

`Engine::ui_pass` (`frame.rs`):

```rust
  fn ui_pass(&mut self) {
    let (w, h) = match &self.renderer {
      Some(r) => (r.game_width(), r.game_height()),
      None => return,
    };
    self.ui_buffer.fill(0);
    let snapshot = UiInput::snapshot(&self.world.resource::<Input>());
    let real_dt = self.world.resource::<Time>().real.delta();
    let action;
    let consumed;
    {
      let mut ui = UiContext::new_scaled(&mut self.ui_buffer, w, h, snapshot, &self.ui_font, self.ui_scale);
      ui.set_dt(real_dt);
      if self.console_state.visible {
        let (mx, my) = ui.mouse_position();
        if self.console_state.window_rect().contains(mx, my) {
          ui.set_mouse_occluded();
          ui.consume_click();
        }
      }
      action = with_host(&mut self.world, |world, host| {
        host.scenes.ui_pass(world, &mut host.audio, &mut ui)
      });
      self.draw_debug_overlays(&mut ui, h);
      self.console_state.render(&mut ui, w, h);
      consumed = ui.into_input();
    }
    consumed.apply_consumed(self.world.resource_mut::<Input>().into_inner());
    if !matches!(action, SceneAction::Continue) {
      let keep = with_host(&mut self.world, |world, host| {
        let SceneHost { scenes, audio } = host;
        let keep = scenes.handle_action(action, world, audio);
        scenes.sync_physics_paused(world);
        keep
      });
      if !keep {
        self.world.insert_resource(QuitRequested);
      }
    }
  }
```

`UiContext::into_input(self) -> UiInput` returns the snapshot with its consumed sets. `ConsoleState::handle_input` keeps taking `&mut Input` (it runs in `console_tick`, outside the UI pass).

- [ ] **Step 4: Run the tests**

Run: `cargo test --lib core::ui core::console core::engine 2>&1 | tail -5`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(ui): scene UI pass before gameplay with an input snapshot that consumes"
```

### Task 15: `HeadlessEngine` on the new loop

**Files:**
- Modify: `src/core/testing/mod.rs`, `src/core/testing/gpu_world.rs`, `src/core/testing/AGENTS.md`
- Test: existing tests in `testing/`

**Interfaces:**
- `HeadlessEngine::step()` calls `engine.frame(Some(fixed_timestep))`; `world()`/`world_mut()` return the kernel `World`; `press_key`, `release_key`, `tap_key`, `type_text`, `press_mouse`, `release_mouse`, `mouse_move` queue `InputEvent`s; `resize` unchanged; `frame()`, `frame_count()`, `step_frames` unchanged.

- [ ] **Step 1: Port the tests**

In `testing/gpu_world.rs` apply the migration rules (`Transform::from_xy`, `.id()`, anchors where a test positioned by top-left: add `.with_anchor(Anchor::TopLeft)` to keep expected pixels). Add:

```rust
  #[test]
  fn a_tapped_key_is_seen_by_exactly_one_fixed_update() {
    use std::sync::{Arc, Mutex};
    let seen = Arc::new(Mutex::new(0));
    struct Counter(Arc<Mutex<u32>>);
    impl Scene for Counter {
      fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
        if ctx.input.key_pressed(KeyCode::Space) {
          *self.0.lock().unwrap() += 1;
        }
        SceneAction::Continue
      }
    }
    let Some(mut engine) = HeadlessEngine::new_or_skip(
      EngineBuilder::new().resolution(32, 32).start_scene(Counter(seen.clone())),
    ) else {
      return;
    };
    engine.tap_key(KeyCode::Space);
    engine.step_frames(3).unwrap();
    assert_eq!(*seen.lock().unwrap(), 1);
  }
```

- [ ] **Step 2: Implement and run**

Uncomment the `HeadlessEngine` module; replace `handle_keyboard(..)` calls with `queue(InputEvent::Key { .. })`, `handle_text_input` with `queue(InputEvent::Text(..))`, mouse likewise; `pending_releases` queue `Key { pressed: false }` after the step.

Run: `cargo test --lib core::testing 2>&1 | tail -5`
Expected: all pass (GPU tests still skip without a GPU).

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "refactor(testing): HeadlessEngine drives the new frame loop through the input queue"
```

### Task 16: Particles, tilemap, navigation, skeleton

**Files:**
- Modify: `src/core/particles/mod.rs`, `emitter.rs`, `attractor.rs`, `collision.rs`; `src/core/tilemap/manager.rs`; `src/core/navigation/grid.rs`, `agent.rs`; `src/core/skeleton/mod.rs`, `controller.rs`, `render.rs`; `src/core/systems/skeleton.rs`; `src/core/mod.rs`
- Test: existing tests

- [ ] **Step 1: Apply the migration rules module by module, re-enabling each in `core/mod.rs`**

- Particles: `ParticleEmitter`, `ParticleAttractor` derive `Component`; `ParticleSystem::update(&mut self, world: &mut World, dt: f32)` gathers emitters with `world.query::<(Entity, &GlobalTransform, &ParticleEmitter)>()` and attractors likewise (no `Events` parameter; particle bounce messages, if any, are written through `world.resource_mut::<Messages<T>>()` after registering); `collect_for_render(&self, world: &World, camera, collector)` looks emitter properties up once per emitter into a local `HashMap<Entity, (sprite, effect)>` before the particle loop (fixes the two-lookups-per-particle cost); replace `rng.gen_range(a..=b)` with a `fn range(rng, a, b)` helper that returns `a` when `b <= a`.
- Tilemap: `TilemapManager` derives `Resource`; its entity-keyed maps are unchanged.
- Navigation: `NavAgent`, `NavTarget` derive `Component`; `NavigationGrid::block_collider` takes the collider's world rectangle as `(GlobalTransform, &Collider)` with the collider centred on the origin plus offset, matching physics; `rebuild_from_world(&mut self, world: &mut World)` uses a query.
- Skeleton: `Skeleton`, `SkeletalController`, `SkeletalAnimator` derive `Component`; `systems/skeleton.rs` becomes `pub fn update_skeletons(time: Res<Time>, mut q: Query<(&mut Skeleton, Option<&mut SkeletalController>, Option<&mut SkeletalAnimator>)>)` registered in `FixedSet::Juice` after the squash system (add it to `register_juice_systems`); `skeleton_debug_lines(world: &mut World) -> Vec<Line2D>` uses `(&GlobalTransform, &Skeleton)`.
- `state_machine`: delete `StateChanged<S>` and its test.

- [ ] **Step 2: Run the tests**

Run: `cargo test --lib 2>&1 | tail -5`
Expected: all pass.

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "refactor: particles, tilemap, navigation and skeleton on the new world"
```

### Task 17: Networking compiles on the new world (mechanical only)

**Files:**
- Modify: `src/core/network/**` (replication, prediction, action, input, server, client), `src/core/network/replication/component.rs` (the legacy `Component` trait is replaced by a `Replicated: Component + Serialize + DeserializeOwned` bound), `src/core/mod.rs`, `src/lib.rs` (restore the network prelude block)
- Create: `src/core/network/messages.rs` (`ClientConnected`, `ClientDisconnected`, `ConnectedToServer`, `DisconnectedFromServer`, `DisconnectReason` as `#[derive(Message, Clone)]`, moved from the deleted events module)
- Modify: `src/core/error.rs` (move `ReplicationError` and `NetworkError` to `src/core/network/error.rs`; `error.rs` re-exports nothing network-specific)

- [ ] **Step 1: Apply the migration rules**

`NetworkEntity`, `Authority`, `DirtyTracker`, `Predicted`, `NetworkedValue<T>` derive `Component`. `ReplicationManager` methods that took `&World`/`&mut World` keep their signatures with the kernel world and use queries; `Replicated` impls for `Transform` (serialize `translation`, `rotation`, `scale`) and `Velocity` live in `replication/builtin.rs`. Network messages are written through `Messages<T>` resources by the client/server tick functions (`register_message` on first use).

- [ ] **Step 2: Run the tests**

Run: `cargo test --lib core::network 2>&1 | tail -5`
Expected: all pass.

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "refactor(network): compile on the new world without redesign"
```

### Task 18: Core green: lib tests, clippy, prelude, derive crate

**Files:**
- Modify: `src/lib.rs` (final prelude), `Cargo.toml`, `ryo2d-derive/src/lib.rs` (its generated code names no removed types)
- Test: whole lib

- [ ] **Step 1: Finish the prelude**

The prelude exports, in this order: `crate::core::ecs::*`; `crate::core::time::Time`; `crate::core::schedule::{Phase, FixedSet, UpdateSet, PostUpdateSet}`; `crate::core::scene::{Scene, SceneAction, SceneCleanup, SceneContext, SlideDirection, Transition, downcast_return}`; `crate::core::engine::{EngineBuilder, WindowMode}`; `crate::core::messages::*` (the engine message types); `crate::core::input::{GamepadButton, Input, InputEvent}` plus `MouseButton`, `KeyCode`; `crate::core::audio::{Audio, AudioManager, MusicHandle, MusicOptions, SoundHandle, SoundOptions}`; `crate::core::graphics::{AnimatedSprite, Sprite, Camera, Gpu, GpuRenderContext, ..}` as today minus nothing; the physics block minus the deleted legacy types; `crate::core::lighting::{AmbientLight, Atmosphere, GodRaysQuality, Light2D, LightOccluder2D, LightingSettings, VolumetricSettings}`; the shaders, trail, skeleton, particles, tilemap, texture, render, config, save, UI, navigation, state machine, profiler, console, logging and `rand` blocks as today; the network blocks as today; no `raycast`, `procgen`, `inventory`, `Resources`, `TimeControl`, `TimeInfo`, `RapierPhysicsWorld` stays exported (it is a resource games may query), `PhysicsConfig` stays.

- [ ] **Step 2: Green and clean**

Run: `cargo test --lib 2>&1 | tail -3`
Expected: `test result: ok` with 0 failed.

Run: `cargo clippy --lib --tests 2>&1 | grep -c "^warning"`
Expected: `0` (fix every warning; `#[allow]` only with a comment saying why).

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "chore: core compiles and tests green on the foundation"
```

---

## Phase 3: restructure

### Task 19: `crates/` with `ryo2d-derive`, `ryo2d-procgen`, `ryo2d-inventory`

**Files:**
- Move: `ryo2d-derive/` to `crates/ryo2d-derive/`
- Move: `src/core/procgen/` to `crates/ryo2d-procgen/src/` (plus `Cargo.toml`, `lib.rs`, `AGENTS.md`, its skill stays at `.claude/skills/ryo-procgen`)
- Move: `src/core/inventory/` to `crates/ryo2d-inventory/src/`; move the five inventory message types from the deleted events module into `crates/ryo2d-inventory/src/messages.rs`; move `InventoryError` from `src/core/error.rs` into the crate
- Modify: root `Cargo.toml` (`members` gains `"crates/*"`, loses `"ryo2d-derive"`; the `ryo2d-derive` path dependency becomes `crates/ryo2d-derive`), `src/core/mod.rs`, `src/lib.rs`, `src/core/navigation/grid.rs` (the `GenerationGrid::to_navigation_grid` conversion moves into the procgen crate as `impl From<&GenerationGrid> for NavigationGrid`)
- Test: each crate's tests; `tests/integration/inventory.rs` moves to `crates/ryo2d-inventory/tests/inventory.rs`

- [ ] **Step 1: Create the crates**

`crates/ryo2d-procgen/Cargo.toml`:

```toml
[package]
name = "ryo2d-procgen"
version = "0.0.1"
edition = "2024"
description = "Procedural generation for Ryo2D: BSP, caves, noise, WFC, Poisson"
license = "MIT"

[dependencies]
ryo2d = { path = "../.." }
noise = "0.9"
rand = "0.8"
serde = { version = "1.0", features = ["derive"] }
```

`crates/ryo2d-inventory/Cargo.toml` the same shape with `ryo2d`, `serde`, `serde_json`, `rand`, `thiserror`. Each `lib.rs` declares the moved modules and a `pub mod prelude` re-exporting what the old `ryo2d::prelude` exported for it. The `noise` and `serde_json` dependencies leave the root `Cargo.toml` if nothing else uses them (check with `cargo udeps` or grep).

- [ ] **Step 2: Fix references and build**

Run: `cargo check --workspace 2>&1 | tail -3` and `cargo test -p ryo2d-procgen -p ryo2d-inventory 2>&1 | tail -5`
Expected: `Finished`; all tests pass. `ryo2d`'s `Cargo.toml` must not mention either crate.

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "refactor: move procgen and inventory to optional crates under crates/"
```

### Task 20: Park the raycaster in `attic/`

**Files:**
- Move: `src/core/raycast/` to `attic/raycast/`; `examples/demos/raycasting/` to `attic/raycasting-demo/`
- Delete: `src/core/render/passes/raycast.rs`, the `raycast` entries in `render/passes/mod.rs`, `pass_ids::RAYCAST`, `FrameInfo::raycast`, `FrameScene::raycast`, the `GpuRaycastRenderer` references in `engine/`, `scene/cleanup.rs`, `lib.rs`, `src/core/shaders/core/raycast.wgsl` and `raycast_sprite.wgsl` (move them into `attic/raycast/shaders/`)
- Create: `attic/README.md`
- Modify: `Cargo.toml` members (the demo glob no longer matches it), `docs/architecture/overview.md` (remove the raycast pass), `.claude/skills/` (delete `ryo-raycast` if present; otherwise nothing), `dev.sh`/`dev.ps1` demo lists if they enumerate demos

- [ ] **Step 1: Move and write the README**

`attic/README.md`:

```markdown
# Attic

Code parked outside the build. Nothing here compiles today.

- `raycast/`, `raycasting-demo/`: the pseudo-3D raycaster and its demo, removed from the core on
  2026-10-11 (see `docs/specs/2026-10-11-foundation-design.md`). It returns as `crates/ryo2d-raycast`,
  a set of render passes plus components on the new render pipeline, in the renderer sub-project.
```

- [ ] **Step 2: Build**

Run: `cargo check --workspace 2>&1 | tail -3` and `cargo test --lib 2>&1 | tail -3`
Expected: green. The e2e and integration tests still reference raycast in `render_pipeline.rs` tests ("raycast black"); Task 23 removes those.

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "refactor: park the raycaster and its demo in attic/"
```

### Task 21: Delete dead code and dead events

**Files:**
- Delete: `src/core/config/save.rs` `WorldState`/`EntitySnapshot` and their tests; never-emitted event types in `src/core/navigation/events.rs` (`PathStarted`, `PathCompleted`, `PathFailed`, `PathInvalidated` are kept only if `NavAgent` writes them: it does not, so delete the file and the `navigation/mod.rs` doc paragraph that claims the engine emits them); `crates/ryo2d-procgen/src/events.rs` (six never-emitted types); any remaining `#[allow(dead_code)]` items flagged by `cargo clippy` after the deletions
- Modify: `src/core/error.rs` (variants with no users)

- [ ] **Step 1: Delete, then let the compiler and clippy find the rest**

Run: `cargo clippy --workspace --all-targets 2>&1 | grep -E "never used|dead_code" | head -30`
Expected: a list; delete each item (do not `#[allow]` it).

- [ ] **Step 2: Verify**

Run: `cargo test --lib 2>&1 | tail -3` and `cargo clippy --workspace --all-targets 2>&1 | grep -c "^warning"`
Expected: green; `0`.

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "chore: delete the legacy collision system, unused save containers and never-emitted events"
```

---

## Phase 4: test suites

### Task 22: Integration tests

**Files:**
- Modify: every file under `tests/integration/` (23 files) and `tests/integration.rs`
- Delete: `tests/integration/inventory.rs` (moved in Task 19)

- [ ] **Step 1: Apply the migration rules file by file**

Specifics:
- `transform.rs`: delete the "default pivot is top-left" regression tests and replace them with: `Transform::default() == Transform::IDENTITY`; `Renderable::sprite(h).anchor == Anchor::Center`; a child-of-rotated-parent world-position test through `propagate_transforms`; and a `detach_keep_world` test (duplicating Task 5's assertions at the public API level).
- `knockback.rs`, `squash_stretch.rs`, `tweening.rs`, `ui_tween.rs`: drive systems through a `Schedule` with `register_juice_systems` and read messages with a cursor.
- `console.rs`: register commands through a `SceneContext` built with `SceneContext::scoped` on a test world (expose `init_world_resources_for_tests` under `#[cfg(any(test, feature = "test-helpers"))]` and enable `test-helpers` in `[dev-dependencies]` for the integration crate; alternatively keep these tests under `src/core/console/` as unit tests).
- `network*.rs`, `prediction.rs`: mechanical.
- `navigation.rs`: `block_collider` now takes the world rectangle; update expected cells for centre-origin colliders.
- `tilemap.rs`, `lighting.rs`, `trail.rs`, `render_pipeline.rs`, `profiler.rs`, `config_resolution.rs`, `input_handler.rs`, `y_sort.rs`: mechanical; `render_pipeline.rs` loses its raycast assertions.

- [ ] **Step 2: Run**

Run: `cargo test --test integration 2>&1 | tail -3`
Expected: 0 failed.

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "test: integration suite on the foundation"
```

### Task 23: e2e GPU tests

**Files:**
- Modify: every file under `tests/e2e/` (13 files), `tests/e2e.rs`, `tests/common/`

- [ ] **Step 1: Apply the migration rules**

Specifics: scenes implement `ui`/`render` instead of `render_ui`/`render_gpu`; fixtures that positioned sprites by their top-left corner add `.with_anchor(Anchor::TopLeft)` so pixel expectations hold (prefer this over moving every expected coordinate); `engine_lifecycle.rs` gains a test that `fixed_hz(120)` runs two fixed updates per headless step at the default speed of one step per frame (headless advances exactly one timestep, so assert one update per step at any rate, and assert `time.fixed.step_count()` equals the number of steps); `render_pipeline.rs` drops the raycast test; `network.rs` mechanical.

- [ ] **Step 2: Run**

Run: `cargo test --test e2e 2>&1 | tail -3`
Expected: 0 failed (on this machine with a GPU; tests skip without one).

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "test: e2e suite on the foundation"
```

---

## Phase 5: examples

### Task 24: `demo-kit`

**Files:**
- Modify: `examples/demo-kit/src/{scene,demo,page,chrome,controls,settings,start,hold}.rs`, `examples/demo-kit/src/bin/demo-tour.rs`, `examples/demo-kit/Cargo.toml`

- [ ] **Step 1: Apply the migration rules**

Specifics: `PageScene` implements `ui(&mut self, ui, ctx)` (the shell chrome, settings panel and help all draw here; the shell's key handling moves from `update` into `ui` since the UI pass now runs first and consumes through the snapshot: use `ui.input()` for the shell keys and `ui.consume_key(..)`); `DemoPage` keeps `enter`, `update`, `settings`, `controls` and gains `fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) {}` and `fn render(&mut self, gpu: &mut GpuRenderContext) {}` (renamed from `render_ui`/`render_gpu` if present); `Demo::configure(|b| b.physics_hz(..))` call sites become `fixed_hz`; `Rc<DemoDef>` is fine (scenes are not `Send`).

- [ ] **Step 2: Build and tour one demo**

Run: `cargo check -p demo-kit && ./dev demos:tour bloom 2>&1 | tail -5`
Expected: the bloom demo still fails to build (not migrated yet) or, if it compiled by accident, captures; the point is that `demo-kit` itself compiles. Then migrate `examples/demos/bloom` as the first demo in this task so the tour has one passing demo: apply the rules.

Run: `./dev demos:tour bloom 2>&1 | tail -3`
Expected: every page captured.

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "refactor(demo-kit): the shell on the foundation, bloom migrated"
```

### Task 25: Demos, batch A

**Files:** `examples/demos/{anchor-positioning,asset-loading,camera-shake,console,controller,cursor,dpi-scaling,hierarchy-sprites}`

- [ ] **Step 1: Apply the migration rules to each**

Specifics: `anchor-positioning` demonstrates `Renderable::with_anchor` now (the page's text changes accordingly); `hierarchy-sprites` uses `spawn_child`, `ChildOf`, `detach_keep_world`, and shows interpolation of a child under an `Interpolated` parent.

- [ ] **Step 2: Build and tour**

Run: `./dev demos:tour anchor-positioning asset-loading camera-shake console controller cursor dpi-scaling hierarchy-sprites 2>&1 | tail -10`
Expected: every page captured; open two PNGs per demo and confirm sprites are where the page describes.

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "refactor(demos): batch A on the foundation"
```

### Task 26: Demos, batch B

**Files:** `examples/demos/{knockback,lighting,lighting-volumetric,networking,particles,pathfinding,post-processing,profile-and-log}`

- [ ] **Step 1: Apply the migration rules** (`knockback` uses `Knockback::apply(ctx.world, ..)`; `lighting*` use `ctx.lighting().set_ambient(..)`; `particles` owns its `ParticleSystem` and calls `update(ctx.world, dt)` in `update` and `collect_for_render(gpu.world, ..)` in `render`).

- [ ] **Step 2: Build and tour** the eight demos as in Task 25.

- [ ] **Step 3: Commit** `refactor(demos): batch B on the foundation`.

### Task 27: Demos, batch C

**Files:** `examples/demos/{procgen,inventory,shadows,skeleton,sprite-shaders,squash-stretch,state-machine,steering}`

- [ ] **Step 1: Apply the migration rules** (`procgen` and `inventory` depend on their crates and import `ryo2d_procgen::prelude::*` / `ryo2d_inventory::prelude::*`; `squash-stretch` uses `SquashStretch::apply(..)`).

- [ ] **Step 2: Build and tour** the eight demos.

- [ ] **Step 3: Commit** `refactor(demos): batch C on the foundation`.

### Task 28: Demos, batch D

**Files:** `examples/demos/{tilemap,time-control,trails,tweening,ui,ui-animations,validated-animator,y-sorting}`

- [ ] **Step 1: Apply the migration rules** (`time-control` uses `ctx.time.game.set_speed`, `.pause()`, `.hitstop(..)` and shows that physics obeys them; `ui` and `ui-animations` implement `ui(..)`).

- [ ] **Step 2: Build and tour** the eight demos.

- [ ] **Step 3: Commit** `refactor(demos): batch D on the foundation`.

### Task 29: Games

**Files:** `examples/games/{breakout,bullet-hell,flappy,leap-king,nigh_t}` (`main.rs` and `src/**`)

- [ ] **Step 1: Apply the migration rules**

Specifics: breakout's `.target_fps(144).physics_hz(144)` becomes `.target_fps(144).fixed_hz(120)` (the dt bug is fixed, so the rate is a choice; 120 keeps the ball smooth); its paddle and blocks were positioned by top-left corner with explicit `PADDLE_WIDTH / 2.0` maths: switch to centre positions and drop the half-size terms; flappy's bird rotation is radians already. Each game's pause menu scene, if it pushed over the game, relies on `PhysicsPaused` which the stack sets automatically.

- [ ] **Step 2: Build and run each headlessly**

Run for each game: `RYO_HEADLESS=1 RYO_CAPTURE_FRAMES=60 RYO_CAPTURE_EXIT=1 RYO_CAPTURE_DIR=<scratchpad>/games/<name> cargo run -p <name> 2>&1 | tail -3`
Expected: a captured frame at 60 and a clean exit; open the PNG and confirm the scene is laid out as before.

- [ ] **Step 3: Commit** `refactor(games): on the foundation`.

---

## Phase 6: documentation

### Task 30: AGENTS.md files, skills, architecture overview, init template

**Files:**
- Modify: `AGENTS.md` (root: workspace layout with `crates/` and `attic/`), `src/core/AGENTS.md` (replace the time-scale section with a pointer to `Time`; the cursor section stays), `src/core/ecs/AGENTS.md` (rewrite: phases, sets, resource-scope rule, interpolation model, hierarchy rules, `Disabled` pooling), `src/core/systems/AGENTS.md` (systems are schedule systems; how to add one), `src/core/physics/AGENTS.md` (origin-centred colliders, dt from the fixed clock, change-detected sync, root-only bodies), `src/core/graphics/AGENTS.md` and `src/core/render/AGENTS.md` (extraction by query, no render cache, no raycast pass), `src/core/testing/AGENTS.md`, `examples/demos/AGENTS.md` (`fixed_hz`, `ui`/`render` page hooks), new `crates/ryo2d-procgen/AGENTS.md` and `crates/ryo2d-inventory/AGENTS.md` (moved content)
- Rewrite: `.claude/skills/ryo-api/SKILL.md` (the canonical reference: every public type and signature of the new surface), `.claude/skills/ryo-ecs-internals/SKILL.md`, `.claude/skills/ryo-game-dev/SKILL.md`, `.claude/skills/ryo-engine-dev/SKILL.md` (the architecture diagram: phases instead of the hand-ordered list), `.claude/skills/ryo-physics/SKILL.md`, `.claude/skills/ryo-events/SKILL.md` (renamed to `ryo-messages`), `.claude/skills/ryo-animation/SKILL.md`, `.claude/skills/ryo-config-save/SKILL.md`, `.claude/skills/ryo-ui/SKILL.md` (the `ui` hook), `.claude/skills/ryo-particles/SKILL.md`, `.claude/skills/ryo-lighting/SKILL.md` (`ctx.lighting()`), `.claude/skills/ryo-procgen/SKILL.md` (crate dependency)
- Sync: `src/cli/skills/*.md` from `.claude/skills/` (the CLI embeds copies: run the existing sync step or copy the files; `AGENTS.md` in `src/cli` says how)
- Modify: `src/cli/commands/init.rs` templates (`fixed_hz`, optional crates commented out in `Cargo.toml`, the `main.rs` template's scene uses `ui`), `docs/architecture/overview.md` (new loop diagram, phases, three clocks, `crates/`), `README.md` quick start

- [ ] **Step 1: Write the docs** following the spec's Documentation section. The `ryo-api` skill lists, for each prelude item, its signature and a one-line use; generate the list from `cargo doc` output rather than memory.

- [ ] **Step 2: Verify the embedded skills match**

Run: `diff -r .claude/skills src/cli/skills 2>&1 | head` (or the project's sync check)
Expected: no differences for the skills the CLI embeds.

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "docs: foundation architecture, skills and templates"
```

---

## Phase 7: verification and merge

### Task 31: Benchmark

**Files:**
- Create: `benches/foundation.rs`
- Modify: `Cargo.toml` (`[dev-dependencies] criterion = "0.5"`, `[[bench]] name = "foundation" harness = false`)

- [ ] **Step 1: Write the benchmark**

```rust
use criterion::{Criterion, criterion_group, criterion_main};
use ryo2d::prelude::*;

fn hierarchy_world() -> (World, Schedule) {
  let mut world = World::new();
  for _ in 0..100 {
    let root = world.spawn(Transform::from_xy(1.0, 1.0)).id();
    for _ in 0..10 {
      let child = world.spawn((Transform::from_xy(1.0, 0.0), ChildOf(root))).id();
      for _ in 0..9 {
        world.spawn((Transform::from_xy(0.0, 1.0), ChildOf(child)));
      }
    }
  }
  #[derive(ScheduleLabel, Debug, Clone, Copy, PartialEq, Eq, Hash)]
  struct Bench;
  let mut schedule = Schedule::new(Bench);
  schedule.add_systems(propagate_transforms);
  schedule.run(&mut world);
  world.clear_trackers();
  (world, schedule)
}

fn propagate(c: &mut Criterion) {
  let (mut world, mut schedule) = hierarchy_world();
  c.bench_function("propagate 10k, nothing moved", |b| {
    b.iter(|| {
      schedule.run(&mut world);
      world.clear_trackers();
    })
  });
  c.bench_function("propagate 10k, every root moved", |b| {
    b.iter(|| {
      for mut t in world.query_filtered::<&mut Transform, Without<ChildOf>>().iter_mut(&mut world) {
        t.translation.x += 0.001;
      }
      schedule.run(&mut world);
      world.clear_trackers();
    })
  });
}

criterion_group!(benches, propagate);
criterion_main!(benches);
```

Add an extraction benchmark in the same file once `EntityDrawer::collect` is reachable from outside the crate; if it is `pub(crate)`, expose a `pub fn bench_collect(world: &mut World) -> usize` under a `bench` feature and call it here.

- [ ] **Step 2: Run and record**

Run: `cargo bench --bench foundation 2>&1 | grep -E "time:" `
Expected: two timings; record them in the final report. "Nothing moved" should be well under 10% of "every root moved".

- [ ] **Step 3: Commit** `test: foundation benchmark`.

### Task 32: Full verification and merge

- [ ] **Step 1: The whole workspace**

Run: `./dev check 2>&1 | tail -3`, `./dev lint 2>&1 | tail -3`, `./dev test 2>&1 | tail -5`, `./dev demos:tour 2>&1 | tail -5`
Expected: all green; every demo page captured.

- [ ] **Step 2: Whole-branch review**

Per the execution method chosen at handoff (a fresh reviewer on the most capable model reads the diff against `main` with the spec and this plan).

- [ ] **Step 3: Merge**

Use the `superpowers:finishing-a-development-branch` skill: merge `foundation` into `main` (no squash; the per-task commits are the record), delete the worktree, then delete this plan file per the project's rule that plans are deleted once executed and reviewed.

---

## Self-review notes

- Spec coverage: workspace layout (Tasks 19, 20, 21), `ryo2d::ecs` surface (1, 9, 18), phases and sets (6, 13), `Time` (2, 13), frame loop (13, 14), input (7, 13, 14, 15), messages (8, 10, 13), Transform/hierarchy/interpolation (3, 4, 5, 13), scene trait and context (10, 13, 14), physics changes (12), renderer changes (13), assets pump (13), other subsystems (11, 16, 17), migration order (0 to 32), testing (each task plus 22, 23, 31), documentation (30).
- The spec's "`physics_hz` renamed in `Ryo.toml` and the embed keys" turned out to be builder-only in the code; only `EngineBuilder::fixed_hz` changes (Task 13).
- The spec left the Rapier world "engine-owned or a resource"; this plan makes it a resource (Task 12) because the physics systems need it from inside the schedule.
- The spec names schedule labels for `add_systems`; the plan uses a `Phase` enum for selection and keeps the labels for the schedules themselves (Task 6), which avoids relying on an unverified `World::run_schedule`.
- Names used consistently across tasks: `Phase`, `FixedSet`, `UpdateSet`, `PostUpdateSet`, `EngineSchedules`, `SceneSchedules`, `SceneSlot`, `SceneHost`, `SceneContext::scoped`, `UiInput`, `InputEvent`, `Input::begin_frame`/`set_fixed_step`/`consume_key`, `Time::try_expend_fixed_step`/`set_in_fixed`, `register_message`/`update_messages`/`clear_messages`/`register_engine_messages`, `SceneMessages::read`, `propagate_transforms`, `snapshot_previous_transforms`, `render_global`, `WorldExt::{spawn_child, detach_keep_world, skip_interpolation}`, `Pool::{acquire, release, release_all}`, `LightingSettings`, `PhysicsPaused`, `Gpu`, `WindowInfo`, `QuitRequested`, `PendingDelta`.
