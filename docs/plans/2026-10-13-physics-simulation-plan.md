# Physics Simulation (3a) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rewrite the physics wrapper on Rapier 0.36.1: Static/Dynamic/Kinematic bodies with rotation, the full 2D shape set with several colliders per body, colliders that follow scale, bodies anywhere in the hierarchy, layers that reach the simulation, collision messages with contact data, forces and impulses, scene queries, joints, and a debug overlay drawn from Rapier.

**Architecture:** The ECS stays the source of truth. Rich components (`RigidBody`, `Collider`, `Joint`) are mirrored into Rapier by change-detected, value-compared systems in `FixedSet::Physics`; live state (`Velocity`, `AngularVelocity`) is read back from awake bodies only. Forces, impulses, queries and contact state go through one public resource, `Physics`, whose Rapier internals are private to `src/core/physics/`. Rapier simulates in pixels with `IntegrationParameters::length_unit` set to the pixels per metre.

**Tech Stack:** Rust 2024, rapier2d 0.36.1 (glam 0.33 math, converted at the boundary to the engine's glam 0.31), bevy_ecs 0.20 (as `ryo2d::ecs`), criterion, `HeadlessEngine`, demo-kit.

**Spec:** `docs/specs/2026-10-13-physics-simulation-design.md`

## Global Constraints

- `rapier2d = { version = "=0.36.1", features = ["debug-render"] }`. `rapier2d` is named only in `Cargo.toml` and inside `src/core/physics/`; no Rapier type appears in a public signature.
- Rapier's `Vec2` is glam 0.33; the engine's is glam 0.31. Convert only through `physics/convert.rs` (`to_rapier`, `from_rapier`); never bump the engine's glam.
- bevy_ecs may be named only in `Cargo.toml`, `src/core/ecs/kernel.rs` and the hidden prelude re-export; use `crate::core::ecs::*`.
- No `unwrap`/`expect`/`panic!` in library code (tests are fine). A poisoned mutex is recovered with `unwrap_or_else(PoisonError::into_inner)`.
- LF line endings, two-space indent (`rustfmt.toml`).
- Units: pixels. `length_unit` = `PhysicsConfig::pixels_per_meter` (default 100). Gravity is `Vec2` in px/s², default `(0, 980)`, +Y down. Density is per square metre: Rapier gets `density / pixels_per_meter²`, so a 100×100 px box at density 1 weighs 1.
- Sync tolerance: a pose differs when translation differs by more than `1e-3` px or rotation (wrapped) by more than `1e-4` rad.
- Rich components (`RigidBody`, `Collider`, `Joint`, `PhysicsConfig`) are `#[non_exhaustive]` with public fields and builder constructors.
- Collider defaults: friction 0.5, restitution 0, density 1, layer 1, mask `u32::MAX`. Combine rules engine-wide: friction `GeometricMean`, restitution `Max`. Every collider: `ActiveEvents::COLLISION_EVENTS` and all `ActiveCollisionTypes` except `FIXED_FIXED`.
- Invalid input (non-finite or non-positive sizes, degenerate shapes, non-finite poses) is logged once per entity with `crate::engine_warn!(module: "Physics", ...)` and skipped. Nothing panics.
- Networking (`src/core/network/`) is untouched beyond what compiles. Breaking APIs, demos and games is fine.
- Each task ports every game, demo, test and bench its API change breaks, so every commit builds and passes.
- Commits: `type: brief description` plus bullets if needed; no co-author lines; never mention AGENTS.md files in commit messages.
- Task-end gate (every task): `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo clippy -p ryo2d --lib --target wasm32-unknown-unknown -- -D warnings`, `cargo test --workspace` all green.

## Review Focus

1. **A collider despawned while touching something** (a bullet hitting an enemy and both despawning in the same update): Rapier reports `Stopped` with the REMOVED flag on the next step. Expected: a `CollisionStopped` naming the despawned entities, no panic, no lookup into a dead entity. Test in Task 10.
2. **A collider whose body is spawned in the same command batch** (a ragdoll or a car spawned with children in one go): the collider must attach to that body, not become a static collider. Expected: bodies register before colliders each step. Test in Task 9.
3. **A non-finite `Transform` written by game code** (a divide by zero in movement): Expected: the pose is not pushed (logged once), the body keeps its last simulated pose, no panic, and Rapier never sees NaN. Test in Task 5.
4. **An entity scaled to zero** (a tween popping a pickup in from scale 0): Expected: its collider is left out (logged once) while the scale is zero and joins the simulation once the scale is non-zero. No panic. Test in Task 9.
5. **Spawn and despawn in the same step** (bullets culled the frame they spawn): Expected: nothing is registered or leaked; `body_count` and `collider_count` return to 0. Test in Task 5.

---

## File Structure

```
src/core/physics/
  mod.rs       module list and public re-exports
  convert.rs   glam 0.31 <-> Rapier glam 0.33, poses, angle helpers           (Task 3)
  config.rs    PhysicsConfig                                                   (Task 4)
  world.rs     Physics: Rapier sets, pipeline, entity<->handle maps, step     (Task 3-4, grows every task)
  sync.rs      the FixedSet::Physics systems and their registration           (Task 4, moved from systems/physics.rs)
  body.rs      RigidBody, BodyKind, AngularVelocity                            (Task 5)
  query.rs     forces, impulses, reads, scene queries, contact state          (Task 6, 11, 12)
  debug.rs     the F2 overlay from Rapier's debug render                       (Task 7)
  collider.rs  Collider, ColliderShape, world_bounds                           (Task 8, replaces collision.rs)
  shape.rs     ColliderShape + linear map -> Rapier SharedShape (crate-internal) (Task 8)
  events.rs    CollisionStarted/Stopped, Contact, ContactForce, JointBroken    (Task 10)
  joint.rs     Joint, JointKind, JointAnchor, JointMotor                       (Task 13)
benches/physics.rs                                                             (Task 1)
examples/demos/physics/{Cargo.toml, Ryo.toml, Trunk.toml, index.html, main.rs} (Task 14)
```

Deleted along the way: `src/core/physics/rapier.rs` (renamed to `world.rs`), `components.rs` (split into `config.rs` and `body.rs`), `collision.rs` (replaced by `collider.rs`), `src/core/systems/physics.rs` (moved to `physics/sync.rs`), `GroupedCollider` in `src/core/graphics/tile.rs`.

---

### Task 1: Physics benchmark baseline

Record the current wrapper's cost on a scene that never sleeps, before anything changes.

**Files:**
- Create: `benches/physics.rs`
- Modify: `Cargo.toml` (a `[[bench]]` entry after the `foundation` one)

**Interfaces:**
- Consumes: today's API (`RigidBody::new()`, `RigidBody::kinematic()`, `Collider::box_collider`, `Collider::circle_collider`, `with_gravity_scale`, `with_restitution`, `with_friction`).
- Produces: `benches/physics.rs` with benchmarks `physics frame, 1000 bouncing bodies` and `physics frame, empty scene`; later tasks port its API calls without changing the scene.

- [ ] **Step 1: Write the benchmark**

```rust
//! Physics benchmark: 1,000 bodies bouncing in a closed box with no gravity
//! and elastic, frictionless contacts, so nothing ever sleeps. One headless
//! frame runs exactly one fixed step; the empty scene is the frame overhead.

use criterion::{Criterion, criterion_group, criterion_main};
use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

const SIZE: f32 = 600.0;

struct Bouncing;

impl Scene for Bouncing {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    for (x, y, w, h) in [
      (SIZE / 2.0, -10.0, SIZE + 40.0, 20.0),
      (SIZE / 2.0, SIZE + 10.0, SIZE + 40.0, 20.0),
      (-10.0, SIZE / 2.0, 20.0, SIZE + 40.0),
      (SIZE + 10.0, SIZE / 2.0, 20.0, SIZE + 40.0),
    ] {
      ctx.world.spawn((
        Transform::from_xy(x, y),
        RigidBody::kinematic(),
        Collider::box_collider(w, h)
          .with_restitution(1.0)
          .with_friction(0.0),
      ));
    }
    for i in 0..1000usize {
      let (col, row) = ((i % 40) as f32, (i / 40) as f32);
      let angle = i as f32 * 2.399;
      let collider = if i % 2 == 0 {
        Collider::circle_collider(5.0)
      } else {
        Collider::box_collider(8.0, 8.0)
      };
      ctx.world.spawn((
        Transform::from_xy(20.0 + col * 14.0, 20.0 + row * 14.0),
        Velocity {
          dx: angle.cos() * 120.0,
          dy: angle.sin() * 120.0,
        },
        RigidBody::new().with_gravity_scale(0.0),
        collider.with_restitution(1.0).with_friction(0.0),
      ));
    }
  }
}

struct Empty;

impl Scene for Empty {}

fn frame(c: &mut Criterion) {
  for (name, builder) in [
    (
      "physics frame, 1000 bouncing bodies",
      EngineBuilder::new().resolution(300, 300).start_scene(Bouncing),
    ),
    (
      "physics frame, empty scene",
      EngineBuilder::new().resolution(300, 300).start_scene(Empty),
    ),
  ] {
    // Needs a GPU; without one this benchmark is skipped
    let Some(mut engine) = HeadlessEngine::new_or_skip(builder) else {
      return;
    };
    engine.step_frames(30).expect("warm-up frames");
    c.bench_function(name, |b| b.iter(|| engine.step().expect("frame")));
  }
}

criterion_group!(benches, frame);
criterion_main!(benches);
```

If `Scene` has required methods beyond `on_enter`, give `Empty` the same empty bodies `ManySprites` in `benches/foundation.rs` relies on (check the trait in `src/core/scene/mod.rs`).

- [ ] **Step 2: Register it**

In `Cargo.toml`, after the `foundation` bench:

```toml
[[bench]]
name = "physics"
harness = false
```

- [ ] **Step 3: Run it and record the baseline**

Run: `cargo bench --bench physics -- --noplot 2>&1 | grep -A2 'physics frame'`
Expected: two `time: [low mid high]` lines. Write both mid values into the ledger as `Baseline: bouncing <x> ms, empty <y> ms`; Task 15 compares against them.

- [ ] **Step 4: Commit**

```bash
git add benches/physics.rs Cargo.toml
git commit -m "test: physics benchmark"
```

---

### Task 2: The frame cap applies to scaled game time

**Files:**
- Modify: `src/core/time.rs` (`GameClock::advance`, `max_delta` docs, tests near `a_huge_speed_is_capped`)
- Modify: `src/core/AGENTS.md` and any skill text that says the cap is on real time (`grep -rn "max_delta\|250 ms" .claude/skills src/core/AGENTS.md src/cli/skills`)

**Interfaces:**
- Produces: `GameClock::advance` caps `real_delta × speed` at `max(max_delta, one fixed step)`.

- [ ] **Step 1: Write the failing tests**

Replace `a_huge_speed_is_capped` in `src/core/time.rs`'s tests with these (keep the module's `ms` helper):

```rust
  #[test]
  fn a_huge_speed_is_capped() {
    let mut t = Time::new(60);
    t.game.set_speed(f32::MAX);
    assert_eq!(t.game.speed(), GameClock::MAX_SPEED);
    t.advance(ms(250));
    // The cap applies to game time: one frame adds at most max_delta
    assert!((t.game.delta() - 0.25).abs() < 1e-4, "{}", t.game.delta());
  }

  #[test]
  fn the_cap_applies_after_speed_scaling() {
    let mut t = Time::new(60);
    t.game.set_speed(2.0);
    t.advance(ms(200));
    assert!((t.game.delta() - 0.25).abs() < 1e-4, "{}", t.game.delta());
  }

  #[test]
  fn slow_motion_below_the_cap_is_unchanged() {
    let mut t = Time::new(60);
    t.game.set_speed(0.5);
    t.advance(ms(100));
    assert!((t.game.delta() - 0.05).abs() < 1e-4, "{}", t.game.delta());
  }

  #[test]
  fn a_stall_at_100x_runs_at_most_fifteen_steps_at_60_hz() {
    let mut t = Time::new(60);
    t.game.set_speed(100.0);
    t.advance(ms(250));
    let mut steps = 0;
    while t.try_expend_fixed_step() {
      steps += 1;
    }
    assert_eq!(steps, 15);
  }
```

If the stepping method has another name, use the one `engine/frame.rs` calls in its step loop.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p ryo2d --lib time::tests`
Expected: `a_huge_speed_is_capped`, `the_cap_applies_after_speed_scaling` and the 100x test FAIL (25 s, 0.4 s, 1500 steps); the slow-motion test passes.

- [ ] **Step 3: Implement**

In `GameClock::advance`, replace the two lines computing `capped` and `self.delta`:

```rust
    let cap = self.max_delta.max(min_cap);
    self.delta = real_delta.mul_f32(self.speed).min(cap);
```

Update the `max_delta` doc comment:

```rust
  /// The most game time one frame may add, after speed scaling. A fixed
  /// timestep longer than this raises the cap to one step.
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ryo2d --lib time::`
Expected: PASS.

- [ ] **Step 5: Update the docs**

In `src/core/AGENTS.md` (and every hit of the grep above), state: "`max_delta` (default 250 ms) caps the game time one frame adds, after speed scaling: a stall or a high speed runs at most 15 fixed steps at 60 Hz."

- [ ] **Step 6: Gate and commit**

Run the task-end gate.

```bash
git add src/core/time.rs src/core/AGENTS.md .claude/skills src/cli/skills
git commit -m "fix: the frame cap applies to scaled game time"
```

---

### Task 3: Rapier 0.36 under the existing physics API

Port `src/core/physics/rapier.rs` to Rapier 0.36.1 without changing the public API. Rapier now simulates in pixels with `length_unit`, events arrive through the engine's own handler (no channels), and the hand conversion disappears.

**Files:**
- Modify: `Cargo.toml:41` (`rapier2d = { version = "=0.36.1", features = ["debug-render"] }`)
- Create: `src/core/physics/convert.rs`
- Modify: `src/core/physics/mod.rs`, `src/core/physics/rapier.rs`

**Interfaces:**
- Produces (crate-internal, used by every later task):
  - `convert::to_rapier(Vec2) -> rapier2d::math::Vector`, `convert::from_rapier(rapier2d::math::Vector) -> Vec2`
  - `convert::pose(translation: Vec2, rotation: f32) -> rapier2d::math::Pose`
  - `convert::angle_delta(from: f32, to: f32) -> f32` (shortest signed difference, in `[-PI, PI)`)
  - `convert::poses_differ(a: (Vec2, f32), b: (Vec2, f32)) -> bool` (the sync tolerance)
  - `const LENGTH_UNIT: f32 = 100.0` in `rapier.rs` until Task 4 makes it config.

- [ ] **Step 1: Write characterization tests that must survive the upgrade**

Add to `rapier.rs`'s tests (they pass on 0.22 and guard the port):

```rust
  #[test]
  fn a_100_px_box_weighs_one() {
    let mut world = RapierPhysicsWorld::new();
    let e = Entity::from_raw_u32(1).unwrap();
    world.register_entity(
      e,
      &Transform::from_xy(0.0, 0.0),
      None,
      &RigidBody::new(),
      &Collider::box_collider(100.0, 100.0),
    );
    let mass = world.body_mass(e).unwrap();
    assert!((mass - 1.0).abs() < 1e-3, "{mass}");
  }

  #[test]
  fn positions_are_pixels() {
    let mut world = RapierPhysicsWorld::new();
    let e = Entity::from_raw_u32(1).unwrap();
    world.register_entity(
      e,
      &Transform::from_xy(250.0, -40.0),
      None,
      &RigidBody::kinematic(),
      &Collider::circle_collider(5.0),
    );
    assert_eq!(world.body_position(e), Some(Vec2::new(250.0, -40.0)));
  }
```

Use whatever entity constructor the existing tests in this file use (they already make entities); add a `pub(crate) fn body_mass(&self, entity: Entity) -> Option<f32>` returning `self.body(entity)?.mass()`.

- [ ] **Step 2: Run them on 0.22**

Run: `cargo test -p ryo2d --lib physics::rapier`
Expected: PASS (they characterise today's behaviour).

- [ ] **Step 3: Bump Rapier and add the conversions**

`Cargo.toml`: `rapier2d = { version = "=0.36.1", features = ["debug-render"] }`.

`src/core/physics/convert.rs`:

```rust
//! The engine's glam (0.31) and Rapier's glam (0.33) are different crates:
//! every vector crossing into or out of Rapier goes through here.

use std::f32::consts::{PI, TAU};

use rapier2d::math::{Pose, Vector};

use crate::core::ecs::Vec2;

/// Translation tolerance for "the game moved it", in pixels.
pub(crate) const POSITION_TOLERANCE: f32 = 1e-3;
/// Rotation tolerance for "the game turned it", in radians.
pub(crate) const ROTATION_TOLERANCE: f32 = 1e-4;

pub(crate) fn to_rapier(v: Vec2) -> Vector {
  Vector::new(v.x, v.y)
}

pub(crate) fn from_rapier(v: Vector) -> Vec2 {
  Vec2::new(v.x, v.y)
}

pub(crate) fn pose(translation: Vec2, rotation: f32) -> Pose {
  Pose::new(to_rapier(translation), rotation)
}

/// The shortest signed turn from `from` to `to`, in `[-PI, PI)`.
pub(crate) fn angle_delta(from: f32, to: f32) -> f32 {
  (to - from + PI).rem_euclid(TAU) - PI
}

/// Whether two poses differ by more than the sync tolerance.
pub(crate) fn poses_differ(a: (Vec2, f32), b: (Vec2, f32)) -> bool {
  (a.0 - b.0).abs().max_element() > POSITION_TOLERANCE
    || angle_delta(a.1, b.1).abs() > ROTATION_TOLERANCE
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn angle_delta_takes_the_short_way_round() {
    assert!((angle_delta(3.0, -3.0) - (TAU - 6.0)).abs() < 1e-5);
    assert!((angle_delta(0.0, 0.5) - 0.5).abs() < 1e-6);
    assert!((angle_delta(10.0 * TAU, 0.25) - 0.25).abs() < 1e-3);
  }

  #[test]
  fn poses_within_tolerance_do_not_differ() {
    let a = (Vec2::new(10.0, 20.0), 1.0);
    assert!(!poses_differ(a, (Vec2::new(10.0005, 20.0), 1.00005)));
    assert!(poses_differ(a, (Vec2::new(10.01, 20.0), 1.0)));
    assert!(poses_differ(a, (Vec2::new(10.0, 20.0), 1.001)));
    assert!(!poses_differ(a, (Vec2::new(10.0, 20.0), 1.0 + TAU)));
  }
}
```

`mod.rs`: add `pub(crate) mod convert;`.

- [ ] **Step 4: Port `rapier.rs`**

- Replace `use crossbeam_channel as channel; use rapier2d::prelude::*;` with explicit imports:

```rust
use std::sync::{Mutex, PoisonError};

use rapier2d::prelude::{
  ActiveEvents, BroadPhaseBvh, CCDSolver, ColliderBuilder, ColliderHandle, ColliderSet,
  CollisionEvent, ContactPair, EventHandler, ImpulseJointSet, IntegrationParameters, IslandManager,
  LockedAxes, MultibodyJointSet, NarrowPhase, PhysicsPipeline, Real, RigidBodyBuilder,
  RigidBodyHandle, RigidBodySet, RigidBodyType, SoftBodySet, SoftBodyTearEvent,
};

use super::convert::{from_rapier, to_rapier};
```

- Replace `PIXELS_PER_METER` with `const LENGTH_UNIT: f32 = 100.0;` and remove every `/ PIXELS_PER_METER` and `* PIXELS_PER_METER`: positions, sizes, velocities and gravity are pixels now. `vector![x, y]` becomes `to_rapier(Vec2::new(x, y))` (or `to_rapier(v)` for a `Vec2`), and results go through `from_rapier`.
- `new()`: `broad_phase: BroadPhaseBvh::new()`, add `soft_bodies: SoftBodySet::new()` and `collector: EventCollector::default()` fields, and set the unit:

```rust
    let mut integration_parameters = IntegrationParameters::default();
    integration_parameters.length_unit = LENGTH_UNIT;
```

- Colliders keep their old masses: density 1 per square metre is `1 / LENGTH_UNIT²` per square pixel. Add `.density(1.0 / (LENGTH_UNIT * LENGTH_UNIT))` to both collider builders.
- `clear()` also resets `soft_bodies` and uses `BroadPhaseBvh::new()`.
- `unregister_entity`: `self.rigid_body_set.remove(body_handle, &mut self.island_manager, &mut self.collider_set, &mut self.impulse_joint_set, &mut self.multibody_joint_set, &mut self.soft_bodies, true);`
- The event handler replaces the channels:

```rust
/// Collects Rapier's events during a step; drained right after it.
#[derive(Default)]
struct EventCollector {
  collisions: Mutex<Vec<CollisionEvent>>,
}

impl EventHandler for EventCollector {
  fn handle_collision_event(
    &self,
    _bodies: &RigidBodySet,
    _colliders: &ColliderSet,
    event: CollisionEvent,
    _contact_pair: Option<&ContactPair>,
  ) {
    self
      .collisions
      .lock()
      .unwrap_or_else(PoisonError::into_inner)
      .push(event);
  }

  fn handle_contact_force_event(
    &self,
    _dt: Real,
    _bodies: &RigidBodySet,
    _colliders: &ColliderSet,
    _contact_pair: &ContactPair,
    _total_force_magnitude: Real,
  ) {
  }

  fn handle_soft_body_tear_event(&self, _soft_bodies: &SoftBodySet, _event: &SoftBodyTearEvent) {}
}
```

- `step`:

```rust
  pub fn step(&mut self, gravity: (f32, f32), dt: f32) {
    self.collision_events.clear();
    self.integration_parameters.dt = dt;
    self.physics_pipeline.step(
      to_rapier(Vec2::new(gravity.0, gravity.1)),
      &self.integration_parameters,
      &mut self.island_manager,
      &mut self.broad_phase,
      &mut self.narrow_phase,
      &mut self.rigid_body_set,
      &mut self.collider_set,
      &mut self.impulse_joint_set,
      &mut self.multibody_joint_set,
      &mut self.soft_bodies,
      &mut self.ccd_solver,
      &(),
      &self.collector,
    );
    let collisions = self
      .collector
      .collisions
      .get_mut()
      .unwrap_or_else(PoisonError::into_inner);
    for event in collisions.drain(..) {
      let (collider1, collider2, started) = (event.collider1(), event.collider2(), event.started());
      // (the existing lookup of entities and is_sensor, unchanged)
    }
  }
```

  The drained `Vec` keeps its capacity, so a step allocates nothing for events. Keep the body of the old loop (entity lookup, `is_sensor`) as it was.
- `set_transform`/`set_kinematic_target`/`set_velocity`/`read_back`/`body_velocity`/`body_position`: drop the unit factors and convert with `to_rapier`/`from_rapier`. `body.translation()` returns a `Vector` by value now.
- `register_entity`/`register_grouped_entity`/`build_body`: `.translation(to_rapier(..))`, `.linvel(to_rapier(..))`.

- [ ] **Step 5: Run the physics tests**

Run: `cargo test -p ryo2d --lib physics:: systems::physics knockback`
Expected: PASS, including the two characterization tests, the fall-distance test (about 490 px after one second at 60 and 120 Hz) and the resting-body-stays-asleep test. If Rapier 0.35's new sleep thresholds keep the resting ball awake after 600 steps, raise the step count in that test to 1200 and ledger it.

- [ ] **Step 6: Prove the web build**

Run: `cargo clippy -p ryo2d --lib --target wasm32-unknown-unknown -- -D warnings`
Expected: no errors. A failure here stops the plan for an owner decision (spec Risks).

- [ ] **Step 7: Gate and commit**

Run the task-end gate (`crossbeam-channel` stays: the asset manager uses it).

```bash
git add Cargo.toml Cargo.lock src/core/physics
git commit -m "refactor: Rapier 0.36 under the existing physics API" -m "- pixels through length_unit instead of hand conversion" -m "- events through the engine's own handler, no per-step channels"
```

---

### Task 4: The `Physics` resource, `PhysicsConfig` and the module layout

Mechanical: rename `RapierPhysicsWorld` to `Physics`, move the systems into the physics module, and make `PhysicsConfig` carry the unit and solver iterations. Behaviour is unchanged except that `pixels_per_meter` and `solver_iterations` now come from config.

**Files:**
- Rename: `src/core/physics/rapier.rs` -> `src/core/physics/world.rs` (`git mv`)
- Rename: `src/core/systems/physics.rs` -> `src/core/physics/sync.rs` (`git mv`)
- Create: `src/core/physics/config.rs` (`PhysicsConfig` moves here from `components.rs`)
- Modify: `src/core/physics/mod.rs`, `src/core/physics/components.rs` (keeps `RigidBody` only, until Task 5), `src/core/systems/mod.rs`, `src/core/engine/mod.rs`, `src/core/engine/frame.rs`, `src/core/engine/tests.rs`, `src/core/scene/context.rs`, `src/lib.rs`, `tests/e2e/headless_engine.rs` (if it names the old types)

**Interfaces:**
- Produces:
  - `pub struct Physics` (resource; the old `RapierPhysicsWorld` with its methods, renamed)
  - `Physics::configure(&mut self, config: &PhysicsConfig)` (crate-internal): sets `length_unit`, `num_solver_iterations` and the gravity used by `step`
  - `Physics::step(&mut self, dt: f32)` (gravity no longer an argument)
  - `PhysicsConfig { pub gravity: Vec2, pub pixels_per_meter: f32, pub solver_iterations: usize }`, `#[non_exhaustive]`, `Default`, builders `with_gravity(Vec2)`, `zero_gravity()`, `with_pixels_per_meter(f32)`, `with_solver_iterations(usize)`; crate-internal `length_unit() -> f32` and `iterations() -> usize` (sanitised)
  - `SceneContext::physics(&self) -> &Physics`, `SceneContext::physics_mut(&mut self) -> Mut<'_, Physics>`
  - `crate::core::physics::sync::register_physics_systems` and the `Last`-phase systems (crate-internal)

- [ ] **Step 1: Write the failing tests**

`src/core/physics/config.rs` tests:

```rust
#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn defaults() {
    let config = PhysicsConfig::default();
    assert_eq!(config.gravity, Vec2::new(0.0, 980.0));
    assert_eq!(config.pixels_per_meter, 100.0);
    assert_eq!(config.solver_iterations, 4);
  }

  #[test]
  fn builders() {
    let config = PhysicsConfig::default()
      .with_gravity(Vec2::new(1.0, 2.0))
      .with_pixels_per_meter(32.0)
      .with_solver_iterations(8);
    assert_eq!(config.gravity, Vec2::new(1.0, 2.0));
    assert_eq!(config.length_unit(), 32.0);
    assert_eq!(config.iterations(), 8);
    assert_eq!(PhysicsConfig::default().zero_gravity().gravity, Vec2::ZERO);
  }

  #[test]
  fn nonsense_values_fall_back() {
    let config = PhysicsConfig::default()
      .with_pixels_per_meter(f32::NAN)
      .with_solver_iterations(0);
    assert_eq!(config.length_unit(), PhysicsConfig::DEFAULT_PIXELS_PER_METER);
    assert_eq!(config.iterations(), 1);
    let negative = PhysicsConfig::default().with_pixels_per_meter(-5.0);
    assert_eq!(negative.length_unit(), PhysicsConfig::DEFAULT_PIXELS_PER_METER);
  }
}
```

In `sync.rs`'s tests, change the `world(hz)` helper to insert `Physics::new()` and `PhysicsConfig::default()`, rename every `RapierPhysicsWorld` to `Physics`, and add:

```rust
  #[test]
  fn gravity_is_pixels_whatever_the_unit() {
    let fell = |ppm: f32| {
      let (mut world, mut s) = world(60);
      *world.resource_mut::<PhysicsConfig>() = PhysicsConfig::default().with_pixels_per_meter(ppm);
      let e = world
        .spawn((
          Transform::IDENTITY,
          RigidBody::new(),
          Collider::circle_collider(8.0),
        ))
        .id();
      for _ in 0..60 {
        s.run(Phase::FixedUpdate, &mut world);
      }
      world.get::<Transform>(e).unwrap().translation.y
    };
    let (at50, at100) = (fell(50.0), fell(100.0));
    assert!((at50 - at100).abs() < 2.0, "{at50} vs {at100}");
  }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p ryo2d --lib physics::`
Expected: compile errors (`config` module, `Physics`, `with_pixels_per_meter` missing).

- [ ] **Step 3: Move and rename**

```bash
git mv src/core/physics/rapier.rs src/core/physics/world.rs
git mv src/core/systems/physics.rs src/core/physics/sync.rs
```

- `world.rs`: `pub struct RapierPhysicsWorld` -> `pub struct Physics` (and every `impl`, test and doc mention); delete `LENGTH_UNIT` and the `gravity` parameter of `step`; add a `gravity: Vec2` field (default `Vec2::new(0.0, 980.0)`) and:

```rust
  /// Take the unit, solver iterations and gravity from the config.
  pub(crate) fn configure(&mut self, config: &PhysicsConfig) {
    self.integration_parameters.length_unit = config.length_unit();
    self.integration_parameters.num_solver_iterations = config.iterations();
    self.gravity = if config.gravity.is_finite() {
      config.gravity
    } else {
      Vec2::ZERO
    };
  }

  /// Rapier's density for `density` per square metre.
  pub(crate) fn rapier_density(&self, density: f32) -> f32 {
    let unit = self.integration_parameters.length_unit;
    density / (unit * unit)
  }
```

  `new()` sets `length_unit = PhysicsConfig::DEFAULT_PIXELS_PER_METER`; collider builders use `.density(self.rapier_density(1.0))`; `step(&mut self, dt: f32)` passes `to_rapier(self.gravity)`.
- `config.rs`:

```rust
//! Global physics settings.

use crate::core::ecs::{Resource, Vec2};

/// Gravity, the pixel scale and solver effort. Edit it live through
/// `ctx.resource_mut::<PhysicsConfig>()`; changes apply on the next step.
#[derive(Resource, Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct PhysicsConfig {
  /// Acceleration in pixels per second squared (+Y is down).
  pub gravity: Vec2,
  /// How many pixels make a metre: Rapier's tolerances, sleep thresholds
  /// and speed cap are tuned for objects about a metre across, and density
  /// is per square metre.
  pub pixels_per_meter: f32,
  /// Constraint solver iterations per step: more is stiffer stacks and
  /// joints, at a cost.
  pub solver_iterations: usize,
}

impl PhysicsConfig {
  pub const DEFAULT_PIXELS_PER_METER: f32 = 100.0;

  pub fn with_gravity(mut self, gravity: Vec2) -> Self {
    self.gravity = gravity;
    self
  }

  /// No gravity: space and top-down games.
  pub fn zero_gravity(mut self) -> Self {
    self.gravity = Vec2::ZERO;
    self
  }

  pub fn with_pixels_per_meter(mut self, pixels: f32) -> Self {
    self.pixels_per_meter = pixels;
    self
  }

  pub fn with_solver_iterations(mut self, iterations: usize) -> Self {
    self.solver_iterations = iterations;
    self
  }

  /// `pixels_per_meter`, or the default when it is not a positive number.
  pub(crate) fn length_unit(&self) -> f32 {
    if self.pixels_per_meter.is_finite() && self.pixels_per_meter > 0.0 {
      self.pixels_per_meter
    } else {
      Self::DEFAULT_PIXELS_PER_METER
    }
  }

  /// At least one solver iteration.
  pub(crate) fn iterations(&self) -> usize {
    self.solver_iterations.max(1)
  }
}

impl Default for PhysicsConfig {
  fn default() -> Self {
    Self {
      gravity: Vec2::new(0.0, 980.0),
      pixels_per_meter: Self::DEFAULT_PIXELS_PER_METER,
      solver_iterations: 4,
    }
  }
}
```

- `components.rs`: delete `PhysicsConfig` and its tests.
- `sync.rs`: fix imports (`super::{Collider, Physics, PhysicsConfig, RigidBody}`); every system takes `ResMut<Physics>`; `step_physics` becomes:

```rust
pub(crate) fn step_physics(
  mut physics: ResMut<Physics>,
  config: Res<PhysicsConfig>,
  time: Res<Time>,
) {
  crate::profile_scope!("cpu:physics");
  physics.configure(&config);
  physics.step(time.fixed.timestep());
}
```

  Make every system and `register_physics_systems` `pub(crate)`.
- `mod.rs`:

```rust
//! Rapier physics: bodies, colliders and the simulation world.

pub mod collision;
pub mod components;
mod config;
pub(crate) mod convert;
pub(crate) mod sync;
mod world;

pub use collision::{Collider, ColliderShape};
pub use components::RigidBody;
pub use config::PhysicsConfig;
pub use world::{CollisionEventType, Physics, PhysicsCollisionEvent};
```

- `src/core/systems/mod.rs`: remove `mod physics;` and its `pub use physics::{..}` block.
- `src/core/engine/mod.rs`: import `crate::core::physics::{Physics, PhysicsConfig}` and `crate::core::physics::sync::{register_physics_systems, register_restored_bodies, unregister_removed_bodies}`; insert `Physics::new()`.
- `src/core/scene/context.rs`, next to `look()`:

```rust
  /// The simulation: forces and impulses, queries and contact state.
  pub fn physics(&self) -> &Physics {
    self.world.resource::<Physics>()
  }

  pub fn physics_mut(&mut self) -> Mut<'_, Physics> {
    self.world.resource_mut::<Physics>()
  }
```

- `src/lib.rs` prelude: `RapierPhysicsWorld` -> `Physics`.
- Fix every other `RapierPhysicsWorld` mention (`grep -rn RapierPhysicsWorld src tests examples benches`) and the old `PhysicsConfig { gravity: (..) }` literals (`PhysicsConfig::default().with_gravity(Vec2::new(..))`).

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ryo2d --lib physics:: engine:: knockback`
Expected: PASS.

- [ ] **Step 5: Gate and commit**

```bash
git add -A src tests examples benches
git commit -m "refactor: Physics resource and PhysicsConfig with pixels per metre" -m "- the physics systems move into the physics module" -m "- ctx.physics() and ctx.physics_mut()"
```

---
### Task 5: Body kinds, rotation and bare colliders

The core rewrite. Bodies and colliders register separately: a `Collider` with no `RigidBody` is a fixed collider on its own, and a body's own collider is attached to it. `RigidBody` becomes the `#[non_exhaustive]` builder type with `BodyKind::{Dynamic, Fixed, Kinematic}`. Bodies rotate, `AngularVelocity` is new, kinematic bodies can be driven by velocity, and `Disabled` uses Rapier's enabled flag. Bodies and colliders on child entities stay out of the simulation until Task 9.

**Files:**
- Create: `src/core/physics/body.rs` (replaces `components.rs`; delete it)
- Rewrite: `src/core/physics/world.rs`, `src/core/physics/sync.rs`
- Modify: `src/core/physics/mod.rs`, `src/lib.rs` (prelude), `src/core/engine/mod.rs` (the `Last` systems), `src/core/systems/knockback.rs`, `src/core/engine/frame.rs` (tests), `tests/e2e/headless_engine.rs`, `benches/physics.rs`, `examples/games/{breakout,bullet-hell,flappy}/**`, `examples/demos/{time-control,knockback}/main.rs`

**Interfaces:**
- Consumes: `convert::{to_rapier, from_rapier, pose, angle_delta, poses_differ}` (Task 3); `Physics::configure`, `Physics::rapier_density`, `PhysicsConfig` (Task 4).
- Produces:
  - `pub enum BodyKind { Dynamic, Fixed, Kinematic }` (`Default` = `Dynamic`)
  - `pub struct RigidBody { pub kind, pub gravity_scale: f32, pub linear_damping: f32, pub angular_damping: f32, pub lock_rotation: bool, pub ccd: bool, pub can_sleep: bool, pub dominance: i8 }`, `#[non_exhaustive]`, `#[require(Transform, Velocity, AngularVelocity)]`; `RigidBody::{dynamic, fixed, kinematic}()`, `with_gravity_scale`, `with_linear_damping`, `with_angular_damping`, `lock_rotation()`, `with_ccd()`, `never_sleep()`, `with_dominance(i8)`
  - `pub struct AngularVelocity(pub f32)`
  - crate-internal on `Physics`: `Owner { collider, body }`, `BodyEntry { handle, settings }`, `ColliderEntry { handle, body: Option<Entity>, collider: Collider }`, maps `body_entries`, `collider_entries`, `owners`; `add_body`, `update_body`, `remove_body`, `add_collider`, `remove_collider`, `set_enabled`, `move_body`, `move_collider`, `push_velocity`, `drive_kinematic`, `begin_push`, `step`, `moved_bodies() -> impl Iterator<Item = BodyState>`, `collisions: Vec<RawCollision>`, `warn_once`, `body_pose`, `body_count`, `collider_count`
  - `pub fn Physics::is_sleeping(&self, entity: Entity) -> bool` stays public
  - systems (crate-internal): `forget_removed`, `track_disabled`, `track_parents`, `register_new`, `push_changes`, `step_simulation`, `read_back`, `emit_messages`, `physics_enabled`, `register_physics_systems`
- Removed: `RigidBody::new`, `is_kinematic`, `drag`, `with_drag`; `PhysicsCollisionEvent`, `CollisionEventType` and the world's event list (`get_collision_events`, `drain_collision_events`, `get_collision_events_for`, `has_collision`, `collision_event_count`); `register_entity`, `register_grouped_entity`, `apply_body_settings`, `set_transform`, `set_kinematic_target`, `set_velocity`, `read_back`, `body_velocity`, `body_position`, `clear`.

- [ ] **Step 1: Write `body.rs` with its unit tests**

```rust
//! Rigid bodies: how physics moves an entity.

use crate::core::ecs::{Component, Transform, Velocity};

/// How a body moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BodyKind {
  /// Simulated: gravity, contacts, forces and joints move it.
  #[default]
  Dynamic,
  /// Static: never moves by itself. Writing its `Transform` teleports it,
  /// but it carries nothing. A `Collider` with no body is fixed too.
  Fixed,
  /// Moved only by the game and never pushed. Write its `Transform` and it
  /// travels there over the step (carrying what rides it), or set its
  /// `Velocity`/`AngularVelocity` and the engine moves it; when both are
  /// written in one step the `Transform` wins.
  Kinematic,
}

/// Makes an entity a physics body. Simulated with its `Collider`s (on the
/// entity itself; on children from Task 9 of the physics rewrite on), its
/// `Transform` is written back from the simulation.
///
/// Settings that do not apply to a kind are ignored: gravity, damping, CCD
/// and dominance only matter to dynamic bodies. Mass comes from the colliders.
/// Edits apply in place on the next step without resetting contacts or sleep.
#[derive(Component, Debug, Clone, PartialEq)]
#[non_exhaustive]
#[require(Transform, Velocity, AngularVelocity)]
pub struct RigidBody {
  pub kind: BodyKind,
  /// Multiplier on `PhysicsConfig::gravity` (0 floats, 2 falls twice as fast).
  pub gravity_scale: f32,
  /// How fast linear velocity decays, per second.
  pub linear_damping: f32,
  /// How fast spin decays, per second.
  pub angular_damping: f32,
  /// Never rotate: contacts and torques leave `Transform.rotation` alone.
  pub lock_rotation: bool,
  /// Continuous collision detection: fast bodies do not tunnel through thin ones.
  pub ccd: bool,
  /// Whether the body may fall asleep at rest (asleep it costs nothing).
  pub can_sleep: bool,
  /// A higher dominance pushes lower ones and is never pushed back by them.
  pub dominance: i8,
}

impl RigidBody {
  fn of_kind(kind: BodyKind) -> Self {
    Self {
      kind,
      gravity_scale: 1.0,
      linear_damping: 0.0,
      angular_damping: 0.0,
      lock_rotation: false,
      ccd: false,
      can_sleep: true,
      dominance: 0,
    }
  }

  /// A simulated body. It rotates unless `lock_rotation()` is called.
  pub fn dynamic() -> Self {
    Self::of_kind(BodyKind::Dynamic)
  }

  /// A static body: walls, floors, structures grouping several colliders.
  pub fn fixed() -> Self {
    Self::of_kind(BodyKind::Fixed)
  }

  /// A body only the game moves: platforms, doors, characters with their own movement.
  pub fn kinematic() -> Self {
    Self::of_kind(BodyKind::Kinematic)
  }

  pub fn with_gravity_scale(mut self, scale: f32) -> Self {
    self.gravity_scale = scale;
    self
  }

  pub fn with_linear_damping(mut self, damping: f32) -> Self {
    self.linear_damping = damping;
    self
  }

  pub fn with_angular_damping(mut self, damping: f32) -> Self {
    self.angular_damping = damping;
    self
  }

  pub fn lock_rotation(mut self) -> Self {
    self.lock_rotation = true;
    self
  }

  pub fn with_ccd(mut self) -> Self {
    self.ccd = true;
    self
  }

  pub fn never_sleep(mut self) -> Self {
    self.can_sleep = false;
    self
  }

  pub fn with_dominance(mut self, dominance: i8) -> Self {
    self.dominance = dominance;
    self
  }
}

impl Default for RigidBody {
  fn default() -> Self {
    Self::dynamic()
  }
}

/// Spin in radians per second, with the same sign as `Transform.rotation`.
/// The engine writes it back from awake dynamic bodies; set it to spin one.
#[derive(Component, Debug, Clone, Copy, PartialEq, Default)]
pub struct AngularVelocity(pub f32);

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn kinds_and_defaults() {
    let body = RigidBody::dynamic();
    assert_eq!(body.kind, BodyKind::Dynamic);
    assert_eq!(body.gravity_scale, 1.0);
    assert!(body.can_sleep && !body.lock_rotation && !body.ccd);
    assert_eq!(RigidBody::fixed().kind, BodyKind::Fixed);
    assert_eq!(RigidBody::kinematic().kind, BodyKind::Kinematic);
    assert_eq!(RigidBody::default(), RigidBody::dynamic());
  }

  #[test]
  fn builders() {
    let body = RigidBody::dynamic()
      .with_gravity_scale(0.5)
      .with_linear_damping(0.1)
      .with_angular_damping(0.2)
      .lock_rotation()
      .with_ccd()
      .never_sleep()
      .with_dominance(3);
    assert_eq!(body.gravity_scale, 0.5);
    assert_eq!(body.linear_damping, 0.1);
    assert_eq!(body.angular_damping, 0.2);
    assert!(body.lock_rotation && body.ccd && !body.can_sleep);
    assert_eq!(body.dominance, 3);
  }

  #[test]
  fn a_body_brings_its_motion_components() {
    let mut world = crate::core::ecs::World::new();
    let e = world.spawn(RigidBody::dynamic()).id();
    assert!(world.get::<Transform>(e).is_some());
    assert_eq!(world.get::<Velocity>(e), Some(&Velocity::zero()));
    assert_eq!(world.get::<AngularVelocity>(e), Some(&AngularVelocity(0.0)));
  }
}
```

- [ ] **Step 2: Write the system tests (replace the test module in `sync.rs`)**

```rust
#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ecs::{Messages, World};
  use crate::core::messages::CollisionStarted;
  use crate::core::scene::PhysicsPaused;
  use crate::core::schedule::{EngineSchedules, Phase};
  use crate::core::time::Time;

  fn world(hz: u32) -> (World, EngineSchedules) {
    let mut world = World::new();
    world.insert_resource(Time::new(hz));
    world.insert_resource(Physics::new());
    world.insert_resource(PhysicsConfig::default());
    world.insert_resource(PhysicsPaused(false));
    crate::core::messages::register_engine_messages(&mut world);
    let mut schedules = EngineSchedules::new();
    register_physics_systems(&mut schedules);
    (world, schedules)
  }

  fn run(world: &mut World, s: &mut EngineSchedules, steps: usize) {
    for _ in 0..steps {
      s.run(Phase::FixedUpdate, world);
    }
  }

  fn no_gravity(world: &mut World) {
    *world.resource_mut::<PhysicsConfig>() = PhysicsConfig::default().zero_gravity();
  }

  fn at(world: &World, e: Entity) -> Transform {
    *world.get::<Transform>(e).unwrap()
  }

  /// A static floor: a bare collider 400 wide whose top is at `top`.
  fn floor(world: &mut World, top: f32) -> Entity {
    world
      .spawn((
        Transform::from_xy(0.0, top + 10.0),
        Collider::box_collider(400.0, 20.0),
      ))
      .id()
  }

  fn ball(world: &mut World, x: f32, y: f32) -> Entity {
    world
      .spawn((
        Transform::from_xy(x, y),
        RigidBody::dynamic(),
        Collider::circle_collider(10.0),
      ))
      .id()
  }

  fn drop_for_one_second(hz: u32) -> f32 {
    let (mut world, mut s) = world(hz);
    let e = ball(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, hz as usize);
    at(&world, e).translation.y
  }

  #[test]
  fn fall_distance_per_second_is_independent_of_the_fixed_rate() {
    let at60 = drop_for_one_second(60);
    let at120 = drop_for_one_second(120);
    assert!((at60 - at120).abs() / at60 < 0.03, "{at60} vs {at120}");
    assert!((at60 - 490.0).abs() < 60.0, "roughly g/2 after one second: {at60}");
  }

  #[test]
  fn a_bare_collider_is_a_static_wall() {
    let (mut world, mut s) = world(60);
    let wall = floor(&mut world, 100.0);
    let b = ball(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, 120);
    assert!((at(&world, b).translation.y - 90.0).abs() < 2.0, "{:?}", at(&world, b));
    assert_eq!(at(&world, wall).translation, Vec2::new(0.0, 110.0));
    let physics = world.resource::<Physics>();
    assert_eq!((physics.body_count(), physics.collider_count()), (1, 2));
  }

  #[test]
  fn a_fixed_body_never_moves() {
    let (mut world, mut s) = world(60);
    let platform = world
      .spawn((
        Transform::from_xy(0.0, 110.0),
        RigidBody::fixed(),
        Collider::box_collider(400.0, 20.0),
      ))
      .id();
    ball(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, 120);
    assert_eq!(at(&world, platform).translation, Vec2::new(0.0, 110.0));
  }

  fn dropped_box(body: RigidBody) -> (f32, f32) {
    let (mut world, mut s) = world(60);
    floor(&mut world, 100.0);
    let e = world
      .spawn((
        Transform::from_xy(0.0, 40.0).with_rotation(0.4),
        body,
        Collider::box_collider(40.0, 40.0),
      ))
      .id();
    let mut fastest_spin: f32 = 0.0;
    for _ in 0..120 {
      run(&mut world, &mut s, 1);
      fastest_spin = fastest_spin.max(world.get::<AngularVelocity>(e).unwrap().0.abs());
    }
    (at(&world, e).rotation, fastest_spin)
  }

  #[test]
  fn a_box_landing_on_its_corner_rotates() {
    let (rotation, spin) = dropped_box(RigidBody::dynamic());
    assert!((rotation - 0.4).abs() > 0.1, "it tipped over: {rotation}");
    assert!(spin > 0.5, "AngularVelocity was written back: {spin}");
  }

  #[test]
  fn lock_rotation_holds() {
    let (rotation, spin) = dropped_box(RigidBody::dynamic().lock_rotation());
    assert!((rotation - 0.4).abs() < 1e-4, "{rotation}");
    assert_eq!(spin, 0.0);
  }

  #[test]
  fn a_spinning_body_s_rotation_stays_continuous() {
    let (mut world, mut s) = world(60);
    no_gravity(&mut world);
    let e = ball(&mut world, 0.0, 0.0);
    world.get_mut::<AngularVelocity>(e).unwrap().0 = 10.0;
    run(&mut world, &mut s, 60);
    let rotation = at(&world, e).rotation;
    assert!((rotation - 10.0).abs() < 0.2, "not wrapped into (-PI, PI]: {rotation}");
  }

  #[test]
  fn a_kinematic_body_moved_by_velocity_writes_its_transform_back() {
    let (mut world, mut s) = world(60);
    let e = world
      .spawn((
        Transform::IDENTITY,
        RigidBody::kinematic(),
        Velocity::new(60.0, 0.0),
        Collider::box_collider(10.0, 10.0),
      ))
      .id();
    run(&mut world, &mut s, 60);
    assert!((at(&world, e).translation.x - 60.0).abs() < 0.5, "{:?}", at(&world, e));
    assert_eq!(world.get::<Velocity>(e), Some(&Velocity::new(60.0, 0.0)));
  }

  #[test]
  fn a_kinematic_platform_moved_by_transform_carries_a_resting_box() {
    let (mut world, mut s) = world(60);
    let platform = world
      .spawn((
        Transform::from_xy(0.0, 110.0),
        RigidBody::kinematic(),
        Collider::box_collider(200.0, 20.0),
      ))
      .id();
    let crate_ = world
      .spawn((
        Transform::from_xy(0.0, 80.0),
        RigidBody::dynamic().lock_rotation(),
        Collider::box_collider(20.0, 20.0).with_friction(1.0),
      ))
      .id();
    run(&mut world, &mut s, 60);
    for _ in 0..60 {
      world.get_mut::<Transform>(platform).unwrap().translation.x += 1.0;
      run(&mut world, &mut s, 1);
    }
    let x = at(&world, crate_).translation.x;
    assert!((x - 60.0).abs() < 8.0, "carried with the platform: {x}");
  }

  #[test]
  fn higher_dominance_pushes_without_being_pushed() {
    let (mut world, mut s) = world(60);
    no_gravity(&mut world);
    let pusher = world
      .spawn((
        Transform::from_xy(0.0, 0.0),
        RigidBody::dynamic().lock_rotation().with_dominance(1),
        Velocity::new(100.0, 0.0),
        Collider::box_collider(20.0, 20.0),
      ))
      .id();
    let pushed = world
      .spawn((
        Transform::from_xy(50.0, 0.0),
        RigidBody::dynamic().lock_rotation(),
        Velocity::new(-100.0, 0.0),
        Collider::box_collider(20.0, 20.0),
      ))
      .id();
    run(&mut world, &mut s, 60);
    assert!(world.get::<Velocity>(pusher).unwrap().dx > 90.0);
    assert!(world.get::<Velocity>(pushed).unwrap().dx > 0.0);
  }

  #[test]
  fn a_resting_body_sleeps_and_the_sync_does_not_wake_it() {
    let (mut world, mut s) = world(60);
    floor(&mut world, 100.0);
    let b = ball(&mut world, 0.0, 50.0);
    run(&mut world, &mut s, 600);
    assert!(world.resource::<Physics>().is_sleeping(b), "ten seconds at rest");
    let before = at(&world, b);
    run(&mut world, &mut s, 60);
    assert!(world.resource::<Physics>().is_sleeping(b), "sync must not wake it");
    assert_eq!(at(&world, b), before);
  }

  #[test]
  fn never_sleep_keeps_a_resting_body_awake() {
    let (mut world, mut s) = world(60);
    floor(&mut world, 100.0);
    let b = world
      .spawn((
        Transform::from_xy(0.0, 50.0),
        RigidBody::dynamic().never_sleep(),
        Collider::circle_collider(10.0),
      ))
      .id();
    run(&mut world, &mut s, 600);
    assert!(!world.resource::<Physics>().is_sleeping(b));
  }

  fn fall_after(change: impl FnOnce(&mut World, Entity)) -> f32 {
    let (mut world, mut s) = world(60);
    let e = ball(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, 10);
    change(&mut world, e);
    let before = at(&world, e).translation.y;
    run(&mut world, &mut s, 10);
    at(&world, e).translation.y - before
  }

  #[test]
  fn a_body_made_kinematic_stops_falling() {
    let fell = fall_after(|world, e| {
      world.entity_mut(e).insert(RigidBody::kinematic());
    });
    assert!(fell.abs() < 1e-3, "a kinematic body fell {fell}");
  }

  #[test]
  fn a_body_removed_and_added_again_in_one_tick_keeps_simulating() {
    let fell = fall_after(|world, e| {
      world.entity_mut(e).remove::<RigidBody>();
      world.entity_mut(e).insert(RigidBody::dynamic());
    });
    assert!(fell > 1.0, "the body stopped: fell {fell}");
  }

  #[test]
  fn a_non_finite_transform_is_ignored() {
    let (mut world, mut s) = world(60);
    let e = ball(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, 5);
    world.get_mut::<Transform>(e).unwrap().translation.x = f32::NAN;
    run(&mut world, &mut s, 5);
    let (position, rotation) = world.resource::<Physics>().body_pose(e).unwrap();
    assert!(position.is_finite() && rotation.is_finite());
    assert!(at(&world, e).translation.is_finite(), "the write-back restored it");
  }

  #[test]
  fn spawn_and_despawn_in_one_step_leaves_nothing() {
    let (mut world, mut s) = world(60);
    let b = ball(&mut world, 0.0, 0.0);
    let f = floor(&mut world, 100.0);
    world.despawn(b);
    world.despawn(f);
    run(&mut world, &mut s, 1);
    let b = ball(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, 1);
    world.despawn(b);
    run(&mut world, &mut s, 1);
    let physics = world.resource::<Physics>();
    assert_eq!((physics.body_count(), physics.collider_count()), (0, 0));
  }

  #[test]
  fn removing_the_body_leaves_a_static_collider() {
    let (mut world, mut s) = world(60);
    let platform = world
      .spawn((
        Transform::from_xy(0.0, 110.0),
        RigidBody::kinematic(),
        Collider::box_collider(400.0, 20.0),
      ))
      .id();
    run(&mut world, &mut s, 1);
    world.entity_mut(platform).remove::<RigidBody>();
    let b = ball(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, 120);
    let physics = world.resource::<Physics>();
    assert_eq!((physics.body_count(), physics.collider_count()), (1, 2));
    assert!((at(&world, b).translation.y - 90.0).abs() < 2.0);
  }

  #[test]
  fn a_disabled_floor_lets_a_ball_through_and_stays_registered() {
    let (mut world, mut s) = world(60);
    let f = floor(&mut world, 100.0);
    run(&mut world, &mut s, 1);
    world.entity_mut(f).insert(Disabled);
    let b = ball(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, 60);
    assert!(at(&world, b).translation.y > 150.0, "fell through");
    assert_eq!(world.resource::<Physics>().collider_count(), 2);
    world.entity_mut(f).remove::<Disabled>();
    let caught = ball(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, 120);
    assert!((at(&world, caught).translation.y - 90.0).abs() < 2.0);
  }

  #[test]
  fn a_body_spawned_disabled_registers_when_enabled() {
    let (mut world, mut s) = world(60);
    let e = world
      .spawn((
        Transform::IDENTITY,
        RigidBody::dynamic(),
        Collider::circle_collider(4.0),
        Disabled,
      ))
      .id();
    run(&mut world, &mut s, 2);
    assert_eq!(world.resource::<Physics>().body_count(), 0);
    world.entity_mut(e).remove::<Disabled>();
    run(&mut world, &mut s, 2);
    assert_eq!(world.resource::<Physics>().body_count(), 1);
  }

  #[test]
  fn a_collider_added_after_the_body_still_registers() {
    let (mut world, mut s) = world(60);
    let e = world.spawn((Transform::IDENTITY, RigidBody::dynamic())).id();
    run(&mut world, &mut s, 1);
    assert_eq!(world.resource::<Physics>().collider_count(), 0);
    world.entity_mut(e).insert(Collider::circle_collider(4.0));
    run(&mut world, &mut s, 1);
    assert_eq!(world.resource::<Physics>().collider_count(), 1);
  }

  #[test]
  fn a_game_write_to_transform_moves_a_kinematic_body_over_the_step() {
    let (mut world, mut s) = world(60);
    let e = world
      .spawn((
        Transform::IDENTITY,
        RigidBody::kinematic(),
        Collider::box_collider(4.0, 4.0),
      ))
      .id();
    run(&mut world, &mut s, 1);
    world.get_mut::<Transform>(e).unwrap().translation.x = 50.0;
    run(&mut world, &mut s, 1);
    let (position, _) = world.resource::<Physics>().body_pose(e).unwrap();
    assert!((position.x - 50.0).abs() < 1e-3);
  }

  #[test]
  fn a_body_on_a_child_is_not_simulated_until_detached() {
    use crate::core::ecs::ChildOf;
    let (mut world, mut s) = world(60);
    let parent = world.spawn(Transform::IDENTITY).id();
    let child = world
      .spawn((
        Transform::from_xy(5.0, 0.0),
        RigidBody::dynamic(),
        Collider::box_collider(4.0, 4.0),
        ChildOf(parent),
      ))
      .id();
    run(&mut world, &mut s, 1);
    assert_eq!(world.resource::<Physics>().body_count(), 0);
    world.entity_mut(child).remove::<ChildOf>();
    run(&mut world, &mut s, 1);
    assert_eq!(world.resource::<Physics>().body_count(), 1);
  }

  #[test]
  fn paused_physics_runs_no_step() {
    let (mut world, mut s) = world(60);
    let e = ball(&mut world, 0.0, 0.0);
    world.resource_mut::<PhysicsPaused>().0 = true;
    run(&mut world, &mut s, 30);
    assert_eq!(at(&world, e).translation.y, 0.0);
  }

  #[test]
  fn touching_bodies_write_collision_messages() {
    let (mut world, mut s) = world(60);
    let f = floor(&mut world, 100.0);
    let b = ball(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, 60);
    let messages = world.resource::<Messages<CollisionStarted>>();
    let started: Vec<_> = messages.get_cursor().read(messages).copied().collect();
    assert!(
      started.iter().any(|m| m.involves(b) && m.other(b) == Some(f)),
      "{started:?}"
    );
  }
}
```

If `Transform` has no `with_rotation` builder, set `rotation` on the value before spawning.

- [ ] **Step 3: Run them to verify they fail**

Run: `cargo test -p ryo2d --lib physics::`
Expected: compile errors (`body` module, `RigidBody::dynamic`, `AngularVelocity`, `body_pose`, `collider_count`).

- [ ] **Step 4: Rewrite `world.rs`**

Keep the module doc, `EventCollector` (Task 3) and `Physics::{configure, rapier_density}` (Task 4). Replace the rest with the code below. Imports:

```rust
use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, PoisonError};

use rapier2d::prelude::{
  ActiveEvents, BroadPhaseBvh, CCDSolver, ColliderBuilder, ColliderHandle, ColliderSet,
  CollisionEvent, ContactPair, EventHandler, ImpulseJointSet, IntegrationParameters, IslandManager,
  MultibodyJointSet, NarrowPhase, PhysicsPipeline, Pose, Real, RigidBodyActivation,
  RigidBodyBuilder, RigidBodyHandle, RigidBodySet, RigidBodyType, SoftBodySet, SoftBodyTearEvent,
};

use super::body::{BodyKind, RigidBody};
use super::collision::{Collider, ColliderShape};
use super::config::PhysicsConfig;
use super::convert::{from_rapier, pose, poses_differ, to_rapier};
use crate::core::ecs::{Entity, Resource, Vec2};
```

Types and the resource:

```rust
/// A collider's entity and its body's entity (its own when it has no body).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Owner {
  pub(crate) collider: Entity,
  pub(crate) body: Entity,
}

pub(crate) struct BodyEntry {
  pub(crate) handle: RigidBodyHandle,
  /// The settings last applied, compared field by field on edits
  pub(crate) settings: RigidBody,
}

pub(crate) struct ColliderEntry {
  pub(crate) handle: ColliderHandle,
  /// The body it is attached to; `None` for a fixed collider on its own
  pub(crate) body: Option<Entity>,
  /// The component last applied
  pub(crate) collider: Collider,
}

/// A body the step moved, for the write-back.
pub(crate) struct BodyState {
  pub(crate) entity: Entity,
  pub(crate) translation: Vec2,
  pub(crate) rotation: f32,
  pub(crate) linvel: Vec2,
  pub(crate) angvel: f32,
  /// Dynamic bodies write their velocities back too
  pub(crate) dynamic: bool,
}

/// A contact that started or stopped during the last step.
pub(crate) struct RawCollision {
  pub(crate) a: Owner,
  pub(crate) b: Owner,
  pub(crate) started: bool,
  pub(crate) sensor: bool,
}

/// The simulation. Read and steer it with `ctx.physics()` and
/// `ctx.physics_mut()` (`Res<Physics>`/`ResMut<Physics>` in systems).
#[derive(Resource)]
pub struct Physics {
  pub(crate) bodies: RigidBodySet,
  pub(crate) colliders: ColliderSet,
  pub(crate) joints: ImpulseJointSet,
  pub(crate) multibody_joints: MultibodyJointSet,
  pub(crate) soft_bodies: SoftBodySet,
  pub(crate) islands: IslandManager,
  pub(crate) broad_phase: BroadPhaseBvh,
  pub(crate) narrow_phase: NarrowPhase,
  ccd_solver: CCDSolver,
  pipeline: PhysicsPipeline,
  pub(crate) params: IntegrationParameters,
  gravity: Vec2,
  collector: EventCollector,
  pub(crate) body_entries: HashMap<Entity, BodyEntry>,
  pub(crate) collider_entries: HashMap<Entity, ColliderEntry>,
  pub(crate) owners: HashMap<ColliderHandle, Owner>,
  /// Owners of colliders removed since the last step: Rapier reports their
  /// `Stopped` events during the next one
  departed: HashMap<ColliderHandle, Owner>,
  /// Bodies the game moved through `Transform` this step
  teleported: HashSet<Entity>,
  /// Kinematic bodies the engine moves by velocity this step
  driven: Vec<Entity>,
  warned: HashSet<Entity>,
  /// The last step's started and stopped contacts
  pub(crate) collisions: Vec<RawCollision>,
}
```

`new()` builds every set empty, `params` with `length_unit = PhysicsConfig::DEFAULT_PIXELS_PER_METER`, `gravity: Vec2::new(0.0, 980.0)`, and empty maps; `impl Default for Physics` calls it.

Helpers (free functions in `world.rs`):

```rust
fn body_type(kind: BodyKind) -> RigidBodyType {
  match kind {
    BodyKind::Dynamic => RigidBodyType::Dynamic,
    BodyKind::Fixed => RigidBodyType::Fixed,
    BodyKind::Kinematic => RigidBodyType::KinematicPositionBased,
  }
}

fn finite_or(value: f32, fallback: f32) -> f32 {
  if value.is_finite() { value } else { fallback }
}

fn body_builder(settings: &RigidBody) -> RigidBodyBuilder {
  let builder = RigidBodyBuilder::new(body_type(settings.kind))
    .gravity_scale(finite_or(settings.gravity_scale, 1.0))
    .linear_damping(finite_or(settings.linear_damping, 0.0).max(0.0))
    .angular_damping(finite_or(settings.angular_damping, 0.0).max(0.0))
    .ccd_enabled(settings.ccd)
    .can_sleep(settings.can_sleep)
    .dominance_group(settings.dominance);
  if settings.lock_rotation {
    builder.lock_rotations()
  } else {
    builder
  }
}

/// A collider's pose relative to its body.
fn local_pose(collider: &Collider) -> Pose {
  pose(collider.offset, 0.0)
}

/// A collider's pose in the world when its entity is at `translation`, turned by `rotation`.
fn world_pose(translation: Vec2, rotation: f32, collider: &Collider) -> Pose {
  pose(translation + Vec2::from_angle(rotation).rotate(collider.offset), rotation)
}
```

Methods (all `pub(crate)` unless marked public):

```rust
impl Physics {
  pub(crate) fn warn_once(&mut self, entity: Entity, what: &str) {
    if self.warned.insert(entity) {
      crate::engine_warn!(module: "Physics", "{entity}: {what}");
    }
  }

  pub(crate) fn add_body(
    &mut self,
    entity: Entity,
    translation: Vec2,
    rotation: f32,
    linvel: Vec2,
    angvel: f32,
    settings: &RigidBody,
  ) {
    if self.body_entries.contains_key(&entity) {
      return;
    }
    if !(translation.is_finite() && rotation.is_finite()) {
      self.warn_once(entity, "a body with a non-finite Transform is not simulated");
      return;
    }
    let linvel = if linvel.is_finite() { linvel } else { Vec2::ZERO };
    let handle = self.bodies.insert(
      body_builder(settings)
        .pose(pose(translation, rotation))
        .linvel(to_rapier(linvel))
        .angvel(finite_or(angvel, 0.0)),
    );
    self.body_entries.insert(entity, BodyEntry { handle, settings: settings.clone() });
    // A collider already simulated on this entity joins its new body
    if let Some(entry) = self.collider_entries.get_mut(&entity)
      && entry.body.is_none()
    {
      self.colliders.set_parent(entry.handle, Some(handle), &mut self.bodies);
      if let Some(collider) = self.colliders.get_mut(entry.handle) {
        collider.set_position_wrt_parent(local_pose(&entry.collider));
      }
      entry.body = Some(entity);
      if let Some(owner) = self.owners.get_mut(&entry.handle) {
        owner.body = entity;
      }
    }
  }

  /// Apply an edited `RigidBody` in place: only fields that changed are written.
  pub(crate) fn update_body(&mut self, entity: Entity, settings: &RigidBody) {
    let Some(entry) = self.body_entries.get_mut(&entity) else {
      return;
    };
    let Some(body) = self.bodies.get_mut(entry.handle) else {
      return;
    };
    let old = &entry.settings;
    if old.kind != settings.kind {
      body.set_body_type(body_type(settings.kind), true);
    }
    if old.gravity_scale != settings.gravity_scale {
      body.set_gravity_scale(finite_or(settings.gravity_scale, 1.0), true);
    }
    if old.linear_damping != settings.linear_damping {
      body.set_linear_damping(finite_or(settings.linear_damping, 0.0).max(0.0));
    }
    if old.angular_damping != settings.angular_damping {
      body.set_angular_damping(finite_or(settings.angular_damping, 0.0).max(0.0));
    }
    if old.lock_rotation != settings.lock_rotation {
      body.lock_rotations(settings.lock_rotation, true);
    }
    if old.ccd != settings.ccd {
      body.enable_ccd(settings.ccd);
    }
    if old.can_sleep != settings.can_sleep {
      *body.activation_mut() = if settings.can_sleep {
        RigidBodyActivation::active()
      } else {
        RigidBodyActivation::cannot_sleep()
      };
    }
    if old.dominance != settings.dominance {
      body.set_dominance_group(settings.dominance);
    }
    entry.settings = settings.clone();
  }

  /// Remove a body. Its colliders stay, parentless: fixed colliders until
  /// they join another body.
  pub(crate) fn remove_body(&mut self, entity: Entity) {
    let Some(entry) = self.body_entries.remove(&entity) else {
      return;
    };
    let attached: Vec<ColliderHandle> = self
      .bodies
      .get(entry.handle)
      .map(|body| body.colliders().to_vec())
      .unwrap_or_default();
    self.bodies.remove(
      entry.handle,
      &mut self.islands,
      &mut self.colliders,
      &mut self.joints,
      &mut self.multibody_joints,
      &mut self.soft_bodies,
      false,
    );
    for handle in attached {
      if let Some(owner) = self.owners.get_mut(&handle) {
        owner.body = owner.collider;
        if let Some(collider) = self.collider_entries.get_mut(&owner.collider) {
          collider.body = None;
        }
      }
    }
    self.warned.remove(&entity);
  }

  pub(crate) fn add_collider(
    &mut self,
    entity: Entity,
    body: Option<Entity>,
    translation: Vec2,
    rotation: f32,
    collider: &Collider,
  ) {
    if self.collider_entries.contains_key(&entity) {
      return;
    }
    let Some(builder) = self.collider_builder(collider) else {
      self.warn_once(entity, "an invalid collider shape is not simulated");
      return;
    };
    let parent = body.and_then(|body| Some((body, self.body_entries.get(&body)?.handle)));
    let handle = match parent {
      Some((_, handle)) => self.colliders.insert_with_parent(
        builder.position(local_pose(collider)),
        handle,
        &mut self.bodies,
      ),
      None => {
        if !(translation.is_finite() && rotation.is_finite()) {
          self.warn_once(entity, "a collider with a non-finite Transform is not simulated");
          return;
        }
        self
          .colliders
          .insert(builder.position(world_pose(translation, rotation, collider)))
      }
    };
    let body = parent.map(|(body, _)| body);
    self.collider_entries.insert(
      entity,
      ColliderEntry { handle, body, collider: collider.clone() },
    );
    self.owners.insert(handle, Owner { collider: entity, body: body.unwrap_or(entity) });
  }

  /// Box and circle until Task 8 brings the full shape set.
  fn collider_builder(&self, collider: &Collider) -> Option<ColliderBuilder> {
    let valid = |v: f32| v.is_finite() && v > 0.0;
    let builder = match collider.shape {
      ColliderShape::Box { width, height } if valid(width) && valid(height) => {
        ColliderBuilder::cuboid(width / 2.0, height / 2.0)
      }
      ColliderShape::Circle { radius } if valid(radius) => ColliderBuilder::ball(radius),
      _ => return None,
    };
    Some(
      builder
        .density(self.rapier_density(1.0))
        .restitution(collider.restitution)
        .friction(collider.friction)
        .sensor(collider.is_trigger)
        .active_events(ActiveEvents::COLLISION_EVENTS),
    )
  }

  pub(crate) fn remove_collider(&mut self, entity: Entity) {
    let Some(entry) = self.collider_entries.remove(&entity) else {
      return;
    };
    if let Some(owner) = self.owners.remove(&entry.handle) {
      self.departed.insert(entry.handle, owner);
    }
    self.colliders.remove(
      entry.handle,
      &mut self.islands,
      &mut self.bodies,
      &mut self.soft_bodies,
      true,
    );
    self.warned.remove(&entity);
  }

  /// Disable or re-enable an entity's body and collider in Rapier; their
  /// velocities, contacts and joints survive.
  pub(crate) fn set_enabled(&mut self, entity: Entity, enabled: bool) {
    if let Some(entry) = self.body_entries.get(&entity)
      && let Some(body) = self.bodies.get_mut(entry.handle)
    {
      body.set_enabled(enabled);
    }
    if let Some(entry) = self.collider_entries.get(&entity)
      && let Some(collider) = self.colliders.get_mut(entry.handle)
    {
      collider.set_enabled(enabled);
    }
  }

  /// Start pushing a step's changes.
  pub(crate) fn begin_push(&mut self) {
    self.teleported.clear();
    self.driven.clear();
  }

  /// Put a body where the game moved it, if that differs from Rapier's pose:
  /// a kinematic body travels there over the step, any other body teleports
  /// and wakes.
  pub(crate) fn move_body(&mut self, entity: Entity, translation: Vec2, rotation: f32) {
    let Some(entry) = self.body_entries.get(&entity) else {
      return;
    };
    let Some(body) = self.bodies.get_mut(entry.handle) else {
      return;
    };
    let current = (from_rapier(body.translation()), body.rotation().angle());
    if !poses_differ(current, (translation, rotation)) {
      return;
    }
    if body.is_kinematic() {
      body.set_next_kinematic_position(pose(translation, rotation));
    } else {
      body.set_position(pose(translation, rotation), true);
    }
    self.teleported.insert(entity);
  }

  /// Move a fixed collider that has no body.
  pub(crate) fn move_collider(&mut self, entity: Entity, translation: Vec2, rotation: f32) {
    let Some(entry) = self.collider_entries.get(&entity) else {
      return;
    };
    if entry.body.is_some() {
      return;
    }
    let target = world_pose(translation, rotation, &entry.collider);
    let Some(collider) = self.colliders.get_mut(entry.handle) else {
      return;
    };
    let current = (from_rapier(collider.translation()), collider.rotation().angle());
    let wanted = (from_rapier(target.translation), target.rotation.angle());
    if poses_differ(current, wanted) {
      collider.set_position(target);
    }
  }

  /// Give a dynamic body the game's velocities where they differ from Rapier's.
  pub(crate) fn push_velocity(&mut self, entity: Entity, linvel: Vec2, angvel: f32) {
    let Some(entry) = self.body_entries.get(&entity) else {
      return;
    };
    let Some(body) = self.bodies.get_mut(entry.handle) else {
      return;
    };
    if !body.is_dynamic() || !linvel.is_finite() || !angvel.is_finite() {
      return;
    }
    if from_rapier(body.linvel()) != linvel {
      body.set_linvel(to_rapier(linvel), true);
    }
    if body.angvel() != angvel {
      body.set_angvel(angvel, true);
    }
  }

  /// Move a kinematic body by its velocity over the step, unless the game
  /// moved it through `Transform` this step.
  pub(crate) fn drive_kinematic(&mut self, entity: Entity, linvel: Vec2, angvel: f32, dt: f32) {
    if self.teleported.contains(&entity) || !linvel.is_finite() || !angvel.is_finite() {
      return;
    }
    let Some(entry) = self.body_entries.get(&entity) else {
      return;
    };
    let Some(body) = self.bodies.get_mut(entry.handle) else {
      return;
    };
    if !body.is_kinematic() {
      return;
    }
    let translation = from_rapier(body.translation()) + linvel * dt;
    let rotation = body.rotation().angle() + angvel * dt;
    body.set_next_kinematic_position(pose(translation, rotation));
    self.driven.push(entity);
  }

  /// Advance by `dt` seconds, then resolve the step's contacts to entities.
  pub(crate) fn step(&mut self, dt: f32) {
    self.params.dt = dt;
    self.pipeline.step(
      to_rapier(self.gravity),
      &self.params,
      &mut self.islands,
      &mut self.broad_phase,
      &mut self.narrow_phase,
      &mut self.bodies,
      &mut self.colliders,
      &mut self.joints,
      &mut self.multibody_joints,
      &mut self.soft_bodies,
      &mut self.ccd_solver,
      &(),
      &self.collector,
    );
    self.collisions.clear();
    let events = self
      .collector
      .collisions
      .get_mut()
      .unwrap_or_else(PoisonError::into_inner);
    for event in events.drain(..) {
      let owner = |handle| self.owners.get(&handle).or_else(|| self.departed.get(&handle)).copied();
      if let (Some(a), Some(b)) = (owner(event.collider1()), owner(event.collider2())) {
        self.collisions.push(RawCollision {
          a,
          b,
          started: event.started(),
          sensor: event.sensor(),
        });
      }
    }
    self.departed.clear();
  }

  /// Bodies the step moved: awake dynamic bodies and kinematic bodies driven
  /// by velocity.
  pub(crate) fn moved_bodies(&self) -> impl Iterator<Item = BodyState> + '_ {
    let state = |entity: Entity, body: &rapier2d::prelude::RigidBody, dynamic: bool| BodyState {
      entity,
      translation: from_rapier(body.translation()),
      rotation: body.rotation().angle(),
      linvel: from_rapier(body.linvel()),
      angvel: body.angvel(),
      dynamic,
    };
    let dynamic = self.body_entries.iter().filter_map(move |(&entity, entry)| {
      let body = self.bodies.get(entry.handle)?;
      (body.is_dynamic() && body.is_enabled() && !body.is_sleeping())
        .then(|| state(entity, body, true))
    });
    let driven = self.driven.iter().filter_map(move |&entity| {
      let body = self.bodies.get(self.body_entries.get(&entity)?.handle)?;
      Some(state(entity, body, false))
    });
    dynamic.chain(driven)
  }

  pub(crate) fn body_pose(&self, entity: Entity) -> Option<(Vec2, f32)> {
    let body = self.bodies.get(self.body_entries.get(&entity)?.handle)?;
    Some((from_rapier(body.translation()), body.rotation().angle()))
  }

  pub(crate) fn body_count(&self) -> usize {
    self.body_entries.len()
  }

  pub(crate) fn collider_count(&self) -> usize {
    self.collider_entries.len()
  }

  /// Whether the entity's body is asleep (at rest, costing nothing).
  pub fn is_sleeping(&self, entity: Entity) -> bool {
    self
      .body_entries
      .get(&entity)
      .and_then(|entry| self.bodies.get(entry.handle))
      .is_some_and(|body| body.is_sleeping())
  }
}
```

The borrow in `step` (the `events` drain while reading `owners`/`departed` and pushing to `collisions`) is over disjoint fields; if the closure captures `self` whole, inline the two lookups instead. Keep `are_intersecting` and `get_current_intersections`, rewritten to look colliders up in `collider_entries` (Task 12 replaces them). Delete the old public event API and `PhysicsCollisionEvent`/`CollisionEventType`.

- [ ] **Step 5: Rewrite `sync.rs` (everything above the tests)**

```rust
//! Keep Rapier in step with the world. Each fixed step, in
//! `FixedSet::Physics` while `PhysicsPaused` is false: forget what went away,
//! track disabled and parented entities, register what is new, push the
//! game's changes, step, write the simulated bodies back and turn contacts
//! into messages.

use super::body::{AngularVelocity, BodyKind, RigidBody};
use super::collision::Collider;
use super::config::PhysicsConfig;
use super::convert::angle_delta;
use super::world::Physics;
use crate::core::ecs::{
  Added, Allow, Changed, ChildOf, DetectChangesMut, Disabled, Entity, Has, IntoScheduleConfigs,
  MessageWriter, Or, Query, RemovedComponents, Res, ResMut, Transform, Vec2, Velocity, With,
  Without,
};
use crate::core::graphics::GroupedCollider;
use crate::core::messages::{CollisionStarted, CollisionStopped};
use crate::core::scene::PhysicsPaused;
use crate::core::schedule::{EngineSchedules, FixedSet, Phase};
use crate::core::time::Time;

/// Run condition: physics steps unless the scene stack paused it.
pub(crate) fn physics_enabled(paused: Res<PhysicsPaused>) -> bool {
  !paused.0
}

type BodyItem<'a> = (Entity, &'a Transform, &'a Velocity, &'a AngularVelocity, &'a RigidBody);
type ColliderItem<'a> = (
  Entity,
  &'a Transform,
  Option<&'a Collider>,
  Option<&'a GroupedCollider>,
  Has<RigidBody>,
);
type HasPhysics = Or<(With<RigidBody>, With<Collider>, With<GroupedCollider>)>;

fn add_body(physics: &mut Physics, (entity, transform, velocity, angular, body): BodyItem) {
  physics.add_body(
    entity,
    transform.translation,
    transform.rotation,
    velocity.as_vec2(),
    angular.0,
    body,
  );
}

fn add_collider(physics: &mut Physics, (entity, transform, collider, grouped, has_body): ColliderItem) {
  let collider = match (collider, grouped) {
    (Some(collider), _) => collider.clone(),
    (None, Some(grouped)) => grouped_collider(grouped),
    (None, None) => return,
  };
  physics.add_collider(
    entity,
    has_body.then_some(entity),
    transform.translation,
    transform.rotation,
    &collider,
  );
}

/// A composite tile's box as a `Collider`: its offset is the box's top-left.
fn grouped_collider(grouped: &GroupedCollider) -> Collider {
  let size = Vec2::new(grouped.width, grouped.height);
  let center = Vec2::new(grouped.offset_x, grouped.offset_y) + size / 2.0;
  let mut collider = Collider::box_collider(size.x, size.y)
    .with_offset(center.x, center.y)
    .with_restitution(grouped.restitution)
    .with_friction(grouped.friction);
  collider.is_trigger = grouped.is_trigger;
  collider
}

/// Forget colliders and bodies whose components went away (despawn
/// included). Colliders run first; a body's colliders that stay become fixed.
pub(crate) fn forget_removed(
  mut physics: ResMut<Physics>,
  mut colliders: RemovedComponents<Collider>,
  mut grouped: RemovedComponents<GroupedCollider>,
  mut bodies: RemovedComponents<RigidBody>,
) {
  for entity in colliders.read().chain(grouped.read()) {
    physics.remove_collider(entity);
  }
  for entity in bodies.read() {
    physics.remove_body(entity);
  }
}

/// Disable the simulation of disabled entities; re-enable (or register, if
/// they were spawned disabled) re-enabled ones.
pub(crate) fn track_disabled(
  mut physics: ResMut<Physics>,
  disabled: Query<Entity, (Added<Disabled>, Allow<Disabled>, HasPhysics)>,
  mut enabled: RemovedComponents<Disabled>,
  bodies: Query<BodyItem, Without<ChildOf>>,
  colliders: Query<ColliderItem, Without<ChildOf>>,
) {
  for entity in &disabled {
    physics.set_enabled(entity, false);
  }
  for entity in enabled.read() {
    if let Ok(item) = bodies.get(entity) {
      add_body(&mut physics, item);
    }
    if let Ok(item) = colliders.get(entity) {
      add_collider(&mut physics, item);
    }
    physics.set_enabled(entity, true);
  }
}

/// Bodies and colliders on child entities are not simulated yet: one that
/// gained a parent leaves (logged once), one that lost it comes back.
pub(crate) fn track_parents(
  mut physics: ResMut<Physics>,
  parented: Query<Entity, (Added<ChildOf>, HasPhysics)>,
  mut detached: RemovedComponents<ChildOf>,
  bodies: Query<BodyItem, Without<ChildOf>>,
  colliders: Query<ColliderItem, Without<ChildOf>>,
) {
  for entity in &parented {
    physics.remove_collider(entity);
    physics.remove_body(entity);
    physics.warn_once(entity, "bodies and colliders on child entities are not simulated yet");
  }
  for entity in detached.read() {
    if let Ok(item) = bodies.get(entity) {
      add_body(&mut physics, item);
    }
    if let Ok(item) = colliders.get(entity) {
      add_collider(&mut physics, item);
    }
  }
}

/// Register new bodies, then new colliders (so a collider spawned with its
/// body attaches to it), and apply edited bodies in place.
pub(crate) fn register_new(
  mut physics: ResMut<Physics>,
  config: Res<PhysicsConfig>,
  bodies: Query<BodyItem, (Added<RigidBody>, Without<ChildOf>)>,
  edited: Query<(Entity, &RigidBody), Changed<RigidBody>>,
  colliders: Query<ColliderItem, (Or<(Added<Collider>, Added<GroupedCollider>)>, Without<ChildOf>)>,
) {
  physics.configure(&config);
  for item in &bodies {
    add_body(&mut physics, item);
  }
  for (entity, body) in &edited {
    physics.update_body(entity, body);
  }
  for item in &colliders {
    add_collider(&mut physics, item);
  }
}

/// Push the game's writes since the last step. The write-back is an
/// ordinary change too, so values equal to Rapier's (within the sync
/// tolerance) are skipped: untouched bodies stay asleep.
pub(crate) fn push_changes(
  mut physics: ResMut<Physics>,
  time: Res<Time>,
  moved: Query<(Entity, &Transform, Has<RigidBody>), (Changed<Transform>, HasPhysics, Without<ChildOf>)>,
  pushed: Query<
    (Entity, &Velocity, &AngularVelocity),
    (With<RigidBody>, Or<(Changed<Velocity>, Changed<AngularVelocity>)>),
  >,
  kinematic: Query<(Entity, &RigidBody, &Velocity, &AngularVelocity)>,
) {
  physics.begin_push();
  for (entity, transform, has_body) in &moved {
    if !(transform.translation.is_finite() && transform.rotation.is_finite()) {
      physics.warn_once(entity, "a non-finite Transform is not pushed to physics");
      continue;
    }
    if has_body {
      physics.move_body(entity, transform.translation, transform.rotation);
    } else {
      physics.move_collider(entity, transform.translation, transform.rotation);
    }
  }
  for (entity, velocity, angular) in &pushed {
    physics.push_velocity(entity, velocity.as_vec2(), angular.0);
  }
  let dt = time.fixed.timestep();
  for (entity, body, velocity, angular) in &kinematic {
    let still = velocity.as_vec2() == Vec2::ZERO && angular.0 == 0.0;
    if body.kind == BodyKind::Kinematic && !still {
      physics.drive_kinematic(entity, velocity.as_vec2(), angular.0, dt);
    }
  }
}

/// Step the simulation by the fixed timestep.
pub(crate) fn step_simulation(mut physics: ResMut<Physics>, time: Res<Time>) {
  crate::profile_scope!("cpu:physics");
  physics.step(time.fixed.timestep());
}

/// Write the bodies the step moved back. Rotation stays continuous: the
/// turn is added instead of wrapping into (-PI, PI].
pub(crate) fn read_back(
  physics: Res<Physics>,
  mut bodies: Query<(&mut Transform, &mut Velocity, &mut AngularVelocity), With<RigidBody>>,
) {
  for state in physics.moved_bodies() {
    let Ok((mut transform, mut velocity, mut angular)) = bodies.get_mut(state.entity) else {
      continue;
    };
    if transform.translation != state.translation {
      transform.translation = state.translation;
    }
    let turn = angle_delta(transform.rotation, state.rotation);
    if turn != 0.0 {
      transform.rotation += turn;
    }
    if state.dynamic {
      velocity.set_if_neq(Velocity::from_vec2(state.linvel));
      angular.set_if_neq(AngularVelocity(state.angvel));
    }
  }
}

/// Turn the step's contacts into messages.
pub(crate) fn emit_messages(
  physics: Res<Physics>,
  mut started: MessageWriter<CollisionStarted>,
  mut stopped: MessageWriter<CollisionStopped>,
) {
  for collision in &physics.collisions {
    let (entity_a, entity_b) = (collision.a.collider, collision.b.collider);
    if collision.started {
      started.write(CollisionStarted { entity_a, entity_b });
    } else {
      stopped.write(CollisionStopped { entity_a, entity_b });
    }
  }
}

/// Add the physics systems to `FixedSet::Physics`, chained, running only
/// while physics is not paused.
pub(crate) fn register_physics_systems(schedules: &mut EngineSchedules) {
  schedules.add_systems(
    Phase::FixedUpdate,
    (
      forget_removed,
      track_disabled,
      track_parents,
      register_new,
      push_changes,
      step_simulation,
      read_back,
      emit_messages,
    )
      .chain()
      .in_set(FixedSet::Physics)
      .run_if(physics_enabled),
  );
}
```

`src/core/engine/mod.rs`, the `Last` chain: `crate::core::physics::sync::forget_removed, crate::core::physics::sync::track_disabled` replace `unregister_removed_bodies, register_restored_bodies` (update the comment above it: removals and re-enables are tracked every frame; additions wait for the next fixed step).

- [ ] **Step 6: Wire the module and the prelude**

`mod.rs`: replace `pub mod components;` with `mod body;`; `pub use body::{AngularVelocity, BodyKind, RigidBody};`; drop `CollisionEventType`/`PhysicsCollisionEvent` from `pub use world::..`. `src/lib.rs` prelude physics block:

```rust
  pub use crate::core::physics::{
    AngularVelocity, BodyKind, Collider, ColliderShape, Physics, PhysicsConfig, RigidBody,
  };
```

- [ ] **Step 7: Port the knockback system**

`src/core/systems/knockback.rs`: `use crate::core::physics::{BodyKind, RigidBody};` and

```rust
    let dynamic = body.is_some_and(|body| body.kind == BodyKind::Dynamic);
```

In its tests, `RigidBody::new()` becomes `RigidBody::dynamic()`.

- [ ] **Step 8: Port games, demos, tests and the bench**

Rules, applied with `grep -rn "RigidBody::\|with_drag\|is_kinematic" examples tests benches src`:

| Old | New |
|---|---|
| `RigidBody::new()` | `RigidBody::dynamic()` |
| `.with_drag(x)` on a `RigidBody` | `.with_linear_damping(x)` |
| `RigidBody::kinematic()` on something the game never moves (walls, floors, blocks, death zones, static platforms, turrets, barriers) | `RigidBody::fixed()` |
| `RigidBody::kinematic()` on something the game moves through `Transform` (a paddle, pipes, a player, drones, rockets, orbs) | unchanged |

Decide "never moves" by grepping the entity's marker component for `Transform` writes in the game's systems; when unsure, keep it kinematic (behaviour stays as today). Add `.lock_rotation()` to every dynamic body these games and demos spawn (breakout balls, bullet-hell bullets, the flappy bird in `game.rs` and `debug.rs`, the time-control ball, the knockback demo box, the bench's 1,000 bodies), so they behave and benchmark as before. `tests/e2e/headless_engine.rs`: the falling box becomes `RigidBody::dynamic()` and the kinematic floor `RigidBody::fixed()`. `src/core/engine/frame.rs` tests: `RigidBody::new()` -> `RigidBody::dynamic()`, `RapierPhysicsWorld` lookups -> `Physics::body_count()` (or `body_pose`).

- [ ] **Step 9: Run the tests**

Run: `cargo test -p ryo2d --lib`
Expected: PASS. Then `cargo test --workspace` (the e2e headless collision test included).

- [ ] **Step 10: Check the games by eye**

Run each and play ten seconds: `./dev breakout`, `./dev bullet-hell`, `./dev flappy`. Expected: unchanged behaviour (balls bounce, bullets hit, the bird dies on pipes).

- [ ] **Step 11: Gate and commit**

```bash
git add -A src tests examples benches
git commit -m "feat: fixed bodies, rotation and bare colliders" -m "- RigidBody::{dynamic, fixed, kinematic} with damping, rotation lock, CCD, sleep and dominance" -m "- AngularVelocity; kinematic bodies move by Transform or by velocity" -m "- a Collider without a body is a fixed collider" -m "- disabled entities keep their simulation state"
```

---
### Task 6: Forces, impulses and mass

**Files:**
- Create: `src/core/physics/query.rs` (the public `Physics` API: this task's forces and reads; queries arrive in Tasks 11 and 12)
- Create: `src/core/physics/test_util.rs` (`#[cfg(test)]`; the helpers from `sync.rs`'s tests move here so every physics test file shares them)
- Modify: `src/core/physics/world.rs` (queue and apply), `src/core/physics/sync.rs` (apply the queue in `push_changes`; tests use `test_util`), `src/core/physics/mod.rs`

**Interfaces:**
- Consumes: `Physics::{body_entries, bodies}`, `push_changes`, `step` (Task 5).
- Produces:
  - `pub fn apply_impulse(&mut self, body: Entity, impulse: Vec2)`, `apply_impulse_at(&mut self, body: Entity, impulse: Vec2, point: Vec2)`, `apply_angular_impulse(&mut self, body: Entity, impulse: f32)`, `apply_force(&mut self, body: Entity, force: Vec2)`, `apply_force_at(&mut self, body: Entity, force: Vec2, point: Vec2)`, `apply_torque(&mut self, body: Entity, torque: f32)`, `pub fn mass(&self, body: Entity) -> Option<f32>` on `Physics`
  - crate-internal `Physics::apply_queued(&mut self)` (called by `push_changes` last), one-step forces reset at the end of `step`
  - `physics::test_util::{world, run, no_gravity, at, floor, ball}` with the signatures of Task 5's test helpers

- [ ] **Step 1: Move the test helpers**

Create `src/core/physics/test_util.rs` holding `world`, `run`, `no_gravity`, `at`, `floor` and `ball` exactly as Task 5 wrote them (made `pub(crate)`), with the imports they need. In `mod.rs`: `#[cfg(test)] pub(crate) mod test_util;`. In `sync.rs`'s tests, delete the helpers and `use crate::core::physics::test_util::*;`.

- [ ] **Step 2: Write the failing tests (in `query.rs`)**

```rust
#[cfg(test)]
mod tests {
  use crate::core::ecs::{Transform, Vec2, Velocity};
  use crate::core::physics::test_util::*;
  use crate::core::physics::{AngularVelocity, Collider, Physics, RigidBody};

  fn crate_100(world: &mut crate::core::ecs::World, body: RigidBody) -> crate::core::ecs::Entity {
    world
      .spawn((Transform::IDENTITY, body, Collider::box_collider(100.0, 100.0)))
      .id()
  }

  #[test]
  fn mass_comes_from_collider_area() {
    let (mut world, mut s) = world(60);
    let one = crate_100(&mut world, RigidBody::dynamic());
    let two = world
      .spawn((
        Transform::from_xy(500.0, 0.0),
        RigidBody::dynamic(),
        Collider::box_collider(200.0, 100.0),
      ))
      .id();
    run(&mut world, &mut s, 1);
    let physics = world.resource::<Physics>();
    assert!((physics.mass(one).unwrap() - 1.0).abs() < 1e-3);
    assert!((physics.mass(two).unwrap() - 2.0).abs() < 1e-3);
  }

  #[test]
  fn an_impulse_changes_velocity_by_impulse_over_mass() {
    let (mut world, mut s) = world(60);
    no_gravity(&mut world);
    let e = crate_100(&mut world, RigidBody::dynamic().lock_rotation());
    // Queued before the body is registered: applied once it is
    world.resource_mut::<Physics>().apply_impulse(e, Vec2::new(100.0, 0.0));
    run(&mut world, &mut s, 1);
    let dx = world.get::<Velocity>(e).unwrap().dx;
    assert!((dx - 100.0).abs() < 0.5, "{dx}");
  }

  #[test]
  fn a_force_acts_for_one_step() {
    let (mut world, mut s) = world(60);
    no_gravity(&mut world);
    let e = crate_100(&mut world, RigidBody::dynamic().lock_rotation());
    run(&mut world, &mut s, 1);
    world.resource_mut::<Physics>().apply_force(e, Vec2::new(600.0, 0.0));
    run(&mut world, &mut s, 1);
    let after_one = world.get::<Velocity>(e).unwrap().dx;
    assert!((after_one - 10.0).abs() < 0.1, "600 / 1 / 60: {after_one}");
    run(&mut world, &mut s, 1);
    assert_eq!(world.get::<Velocity>(e).unwrap().dx, after_one, "gone after its step");
  }

  #[test]
  fn torques_and_off_centre_impulses_spin_a_body() {
    let (mut world, mut s) = world(60);
    no_gravity(&mut world);
    let a = crate_100(&mut world, RigidBody::dynamic());
    let b = world
      .spawn((
        Transform::from_xy(500.0, 0.0),
        RigidBody::dynamic(),
        Collider::box_collider(100.0, 100.0),
      ))
      .id();
    run(&mut world, &mut s, 1);
    {
      let mut physics = world.resource_mut::<Physics>();
      physics.apply_torque(a, 5000.0);
      physics.apply_impulse_at(b, Vec2::new(0.0, 100.0), Vec2::new(550.0, 0.0));
      physics.apply_angular_impulse(a, 0.0);
    }
    run(&mut world, &mut s, 1);
    assert!(world.get::<AngularVelocity>(a).unwrap().0 > 0.0);
    assert!(world.get::<AngularVelocity>(b).unwrap().0 > 0.0);
    assert!(world.get::<Velocity>(b).unwrap().dy > 0.0);
  }

  #[test]
  fn pushes_on_non_dynamic_or_unknown_entities_are_ignored() {
    let (mut world, mut s) = world(60);
    let wall = crate_100(&mut world, RigidBody::fixed());
    let nobody = world.spawn(Transform::IDENTITY).id();
    {
      let mut physics = world.resource_mut::<Physics>();
      physics.apply_impulse(wall, Vec2::new(1e6, 0.0));
      physics.apply_force(nobody, Vec2::new(1e6, 0.0));
      physics.apply_impulse(wall, Vec2::new(f32::NAN, 0.0));
    }
    run(&mut world, &mut s, 10);
    assert_eq!(at(&world, wall).translation, Vec2::ZERO);
    assert_eq!(world.resource::<Physics>().mass(nobody), None);
  }
}
```

- [ ] **Step 3: Run them to verify they fail**

Run: `cargo test -p ryo2d --lib physics::query`
Expected: compile errors (`apply_impulse`, `mass` missing).

- [ ] **Step 4: Implement**

`query.rs`:

```rust
//! The public side of `Physics`: forces and impulses and body reads (scene
//! queries and contact state join them later).

use super::world::{Physics, Push};
use crate::core::ecs::{Entity, Vec2};

impl Physics {
  /// An instant change of momentum (mass times px/s), applied before the
  /// next step. Ignored for bodies that are not dynamic.
  pub fn apply_impulse(&mut self, body: Entity, impulse: Vec2) {
    self.queue(body, Push::Impulse(impulse));
  }

  /// An impulse at a world point: off-centre it also spins the body.
  pub fn apply_impulse_at(&mut self, body: Entity, impulse: Vec2, point: Vec2) {
    self.queue(body, Push::ImpulseAt(impulse, point));
  }

  /// An instant change of angular momentum.
  pub fn apply_angular_impulse(&mut self, body: Entity, impulse: f32) {
    self.queue(body, Push::AngularImpulse(impulse));
  }

  /// A force (mass times px/s²) for the next step only: call it every step
  /// for a steady push.
  pub fn apply_force(&mut self, body: Entity, force: Vec2) {
    self.queue(body, Push::Force(force));
  }

  /// A force at a world point for the next step only.
  pub fn apply_force_at(&mut self, body: Entity, force: Vec2, point: Vec2) {
    self.queue(body, Push::ForceAt(force, point));
  }

  /// A torque for the next step only.
  pub fn apply_torque(&mut self, body: Entity, torque: f32) {
    self.queue(body, Push::Torque(torque));
  }

  /// The body's mass: the sum of its colliders' (a 100×100 px box at
  /// density 1 weighs 1). `None` for an entity without a simulated body.
  pub fn mass(&self, body: Entity) -> Option<f32> {
    let entry = self.body_entries.get(&body)?;
    Some(self.bodies.get(entry.handle)?.mass())
  }
}
```

`world.rs`:

```rust
/// A push queued for the next step.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Push {
  Impulse(Vec2),
  ImpulseAt(Vec2, Vec2),
  AngularImpulse(f32),
  Force(Vec2),
  ForceAt(Vec2, Vec2),
  Torque(f32),
}

impl Push {
  fn is_finite(self) -> bool {
    match self {
      Push::Impulse(v) | Push::Force(v) => v.is_finite(),
      Push::ImpulseAt(v, p) | Push::ForceAt(v, p) => v.is_finite() && p.is_finite(),
      Push::AngularImpulse(t) | Push::Torque(t) => t.is_finite(),
    }
  }
}
```

New `Physics` fields `queued: Vec<(Entity, Push)>` and `forced: Vec<RigidBodyHandle>`, and:

```rust
  pub(crate) fn queue(&mut self, body: Entity, push: Push) {
    if push.is_finite() {
      self.queued.push((body, push));
    }
  }

  /// Apply the queued pushes to dynamic bodies; one-step forces are
  /// remembered so `step` can clear them.
  pub(crate) fn apply_queued(&mut self) {
    let mut queued = std::mem::take(&mut self.queued);
    for (entity, push) in queued.drain(..) {
      let Some(entry) = self.body_entries.get(&entity) else {
        continue;
      };
      let Some(body) = self.bodies.get_mut(entry.handle) else {
        continue;
      };
      if !body.is_dynamic() {
        continue;
      }
      match push {
        Push::Impulse(impulse) => body.apply_impulse(to_rapier(impulse), true),
        Push::ImpulseAt(impulse, point) => {
          body.apply_impulse_at_point(to_rapier(impulse), to_rapier(point), true)
        }
        Push::AngularImpulse(impulse) => body.apply_torque_impulse(impulse, true),
        Push::Force(force) => {
          body.add_force(to_rapier(force), true);
          self.forced.push(entry.handle);
        }
        Push::ForceAt(force, point) => {
          body.add_force_at_point(to_rapier(force), to_rapier(point), true);
          self.forced.push(entry.handle);
        }
        Push::Torque(torque) => {
          body.add_torque(torque, true);
          self.forced.push(entry.handle);
        }
      }
    }
    self.queued = queued;
  }
```

At the end of `step` (after the events), clear one-step forces:

```rust
    for handle in self.forced.drain(..) {
      if let Some(body) = self.bodies.get_mut(handle) {
        body.reset_forces(false);
        body.reset_torques(false);
      }
    }
```

`push_changes` calls `physics.apply_queued();` last. `mod.rs`: `mod query;`. While physics is paused the queue waits for the next step.

- [ ] **Step 5: Run the tests**

Run: `cargo test -p ryo2d --lib physics::`
Expected: PASS.

- [ ] **Step 6: Gate and commit**

```bash
git add -A src/core/physics
git commit -m "feat: forces, impulses and mass on Physics"
```

---

### Task 7: The F2 overlay draws the simulation

Draw what Rapier simulates (every shape after scale, rotation and offset, joints and contacts) instead of re-deriving geometry from components. Later shape and joint work needs no overlay changes.

**Files:**
- Create: `src/core/physics/debug.rs`
- Modify: `src/core/config/debug.rs` (`colors`, `overlay_shapes`, its test), `src/core/physics/mod.rs`
- Test: `tests/e2e/gpu_world.rs` (the collider overlay test keeps passing)

**Interfaces:**
- Consumes: `Physics::{bodies, colliders, joints, multibody_joints, narrow_phase, soft_bodies}`.
- Produces: `pub(crate) fn physics::debug::draw(physics: &Physics, debug: &mut DebugDraw)`; `config::debug::colors::{DYNAMIC, KINEMATIC, FIXED, TRIGGER, JOINT, CONTACT}` (replacing `COLLIDER`).

- [ ] **Step 1: Write the failing test**

Replace `collider_outlines_are_drawn_where_the_collider_is` in `src/core/config/debug.rs`:

```rust
  #[test]
  fn collider_outlines_are_drawn_where_physics_has_them() {
    use crate::core::ecs::Transform;
    use crate::core::physics::test_util::{run, world};
    let (mut world, mut s) = world(60);
    world.spawn((
      Transform::from_xy(100.0, 50.0).with_rotation(std::f32::consts::FRAC_PI_2),
      Collider::box_collider(20.0, 10.0),
    ));
    run(&mut world, &mut s, 1);
    let colliders = OverlayFlags {
      colliders: true,
      ..Default::default()
    };
    let mut debug = DebugDraw::default();
    overlay_shapes(&mut world, colliders, &mut debug);
    let points: Vec<Vec2> = debug.each().flat_map(|(_, points)| points.to_vec()).collect();
    assert!(!points.is_empty());
    let min = points.iter().fold(Vec2::MAX, |m, p| m.min(*p));
    let max = points.iter().fold(Vec2::MIN, |m, p| m.max(*p));
    // Turned a quarter: 10 wide and 20 high around (100, 50)
    assert!((min - Vec2::new(95.0, 40.0)).abs().max_element() < 0.01, "{min}");
    assert!((max - Vec2::new(105.0, 60.0)).abs().max_element() < 0.01, "{max}");
    debug.end_frame();
    overlay_shapes(&mut world, OverlayFlags::default(), &mut debug);
    assert_eq!(debug.len(), 0);
  }
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p ryo2d --lib config::debug`
Expected: FAIL: the old overlay draws an unrotated 20×10 rect.

- [ ] **Step 3: Implement `physics/debug.rs`**

```rust
//! The F2 overlay: Rapier's own geometry (every collider shape, joints and
//! contact points) drawn into `DebugDraw`, coloured by body kind.

use rapier2d::prelude::{
  DebugColor, DebugRenderBackend, DebugRenderMode, DebugRenderObject, DebugRenderPipeline,
  DebugRenderStyle, RigidBodySet, RigidBodyType, Vector,
};

use super::convert::from_rapier;
use super::world::Physics;
use crate::core::config::debug::colors;
use crate::core::render::DebugDraw;
use crate::core::ui::Color;

/// Alpha for sleeping bodies and disabled colliders.
const DIMMED: u8 = 0x60;

struct Overlay<'a> {
  debug: &'a mut DebugDraw,
  bodies: &'a RigidBodySet,
}

impl DebugRenderBackend for Overlay<'_> {
  fn draw_line(&mut self, object: DebugRenderObject, a: Vector, b: Vector, _color: DebugColor) {
    if let Some(color) = colour(object, self.bodies) {
      self.debug.line(from_rapier(a), from_rapier(b), color);
    }
  }
}

fn rgba(c: [u8; 4]) -> Color {
  Color::new(c[0], c[1], c[2], c[3])
}

fn colour(object: DebugRenderObject, bodies: &RigidBodySet) -> Option<Color> {
  match object {
    DebugRenderObject::Collider(_, collider) => {
      let body = collider.parent().and_then(|handle| bodies.get(handle));
      let base = if collider.is_sensor() {
        colors::TRIGGER
      } else {
        match body.map(|body| body.body_type()) {
          Some(RigidBodyType::Dynamic) => colors::DYNAMIC,
          Some(RigidBodyType::KinematicPositionBased | RigidBodyType::KinematicVelocityBased) => {
            colors::KINEMATIC
          }
          _ => colors::FIXED,
        }
      };
      let mut color = rgba(base);
      if body.is_some_and(|body| body.is_sleeping()) || !collider.is_enabled() {
        color.a = DIMMED;
      }
      Some(color)
    }
    DebugRenderObject::ImpulseJoint(..) => Some(rgba(colors::JOINT)),
    DebugRenderObject::ContactPair(..) => Some(rgba(colors::CONTACT)),
    _ => None,
  }
}

/// Draw the simulation's colliders, joints and contacts into `debug`.
pub(crate) fn draw(physics: &Physics, debug: &mut DebugDraw) {
  let mode = DebugRenderMode::COLLIDER_SHAPES | DebugRenderMode::JOINTS | DebugRenderMode::CONTACTS;
  let mut pipeline = DebugRenderPipeline::new(DebugRenderStyle::default(), mode);
  let mut overlay = Overlay {
    debug,
    bodies: &physics.bodies,
  };
  pipeline.render(
    &mut overlay,
    &physics.bodies,
    &physics.colliders,
    &physics.joints,
    &physics.multibody_joints,
    &physics.narrow_phase,
    &physics.soft_bodies,
  );
}
```

If the `DebugRenderObject` variant set differs (it is `#[non_exhaustive]`-free in 0.36.1, see the Rapier notes), keep the catch-all arm.

- [ ] **Step 4: Use it in `overlay_shapes`**

In `src/core/config/debug.rs`, `colors`:

```rust
pub mod colors {
  /// Green for dynamic bodies' colliders
  pub const DYNAMIC: [u8; 4] = [0x00, 0xff, 0x00, 0xff];
  /// Orange for kinematic bodies' colliders
  pub const KINEMATIC: [u8; 4] = [0xff, 0xa0, 0x00, 0xff];
  /// Blue for fixed bodies and bare colliders
  pub const FIXED: [u8; 4] = [0x60, 0x90, 0xff, 0xff];
  /// Yellow for triggers
  pub const TRIGGER: [u8; 4] = [0xff, 0xff, 0x00, 0xff];
  /// Magenta for joints
  pub const JOINT: [u8; 4] = [0xff, 0x00, 0xff, 0xff];
  /// Red for contact points and normals
  pub const CONTACT: [u8; 4] = [0xff, 0x30, 0x30, 0xff];
  /// Cyan for velocity vectors
  pub const VELOCITY: [u8; 4] = [0x00, 0xff, 0xff, 0xff];
  /// Yellow for origin crosshairs
  pub const ORIGIN: [u8; 4] = [0xff, 0xff, 0x00, 0xff];
  /// Red for error indicators
  pub const ERROR: [u8; 4] = [0xff, 0x00, 0x00, 0xff];
}
```

and the colliders branch of `overlay_shapes` becomes:

```rust
  if flags.colliders
    && let Some(physics) = world.get_resource::<Physics>()
  {
    crate::core::physics::debug::draw(physics, debug);
  }
```

Drop the `Collider`/`ColliderShape`/`GroupedCollider` imports it no longer needs and update the module doc's overlay table ("F2: the simulation's colliders, joints and contacts, coloured by body kind"). `mod.rs`: `pub(crate) mod debug;`.

- [ ] **Step 5: Run the tests**

Run: `cargo test -p ryo2d --lib config::debug && cargo test --test e2e gpu_world`
Expected: PASS. If the GPU overlay test in `tests/e2e/gpu_world.rs` drew before any fixed step ran, step the engine once before capturing (the collider is registered in the first fixed step).

- [ ] **Step 6: Gate and commit**

```bash
git add -A src tests
git commit -m "feat: the collider overlay draws the simulation" -m "- every shape, joint and contact Rapier has, coloured by body kind"
```

---

### Task 8: The full collider set

**Files:**
- Create: `src/core/physics/collider.rs` (replaces `collision.rs`; delete it), `src/core/physics/shape.rs`
- Modify: `src/core/physics/world.rs`, `src/core/physics/sync.rs`, `src/core/physics/mod.rs`, `src/lib.rs`
- Modify: `src/core/graphics/tile.rs` (remove `GroupedCollider`; `build_with_collider` returns a `Collider`), `src/core/graphics/mod.rs`
- Modify: `src/core/particles/mod.rs`, `src/core/navigation/grid.rs`, `tests/integration/navigation.rs`
- Modify: every `Collider::box_collider`/`circle_collider`/`ColliderShape::{Box, Circle}`/`world_bounds(position)`/`world_center`/`GroupedCollider` use in `examples`, `tests`, `benches`, `src` (grep)

**Interfaces:**
- Consumes: `Physics::{add_collider, collider_entries, rapier_density, warn_once}` (Task 5).
- Produces:
  - `pub enum ColliderShape { Rect { size: Vec2 }, Circle { radius: f32 }, Capsule { height: f32, radius: f32 }, Polygon { points: Vec<Vec2> }, Segment { a: Vec2, b: Vec2 }, Polyline { points: Vec<Vec2> } }`, `#[non_exhaustive]`
  - `pub struct Collider { pub shape, pub offset: Vec2, pub rotation: f32, pub is_trigger: bool, pub layer: u32, pub mask: u32, pub density: f32, pub mass: Option<f32>, pub friction: f32, pub restitution: f32, pub force_threshold: Option<f32> }`, `#[non_exhaustive]`, `#[require(Transform)]`
  - `Collider::{rect, circle, capsule, polygon, segment, polyline}`, `with_offset(x, y)`, `with_rotation(radians)`, `as_trigger()`, `with_layer(u32)`, `with_mask(u32)`, `with_density(f32)`, `with_mass(f32)`, `with_friction(f32)`, `with_restitution(f32)`, `with_force_events(threshold: f32)`, `world_bounds(&self, &GlobalTransform) -> (Vec2, Vec2)`
  - `pub(crate) fn shape::build(shape: &ColliderShape, map: glam::Mat2) -> Option<rapier2d::prelude::SharedShape>`; `map` is the linear map from the shape's local space into the frame Rapier places it in (identity until Task 9)
  - `Physics::update_collider(&mut self, entity: Entity, body: Option<Entity>, translation: Vec2, rotation: f32, collider: &Collider)` (crate-internal; registers it when not yet simulated)
  - `CompositeTileBuilder::build_with_collider(self) -> (CompositeTile, Collider)`

- [ ] **Step 1: Write the failing unit tests**

`collider.rs` tests:

```rust
#[cfg(test)]
mod tests {
  use super::*;
  use std::f32::consts::FRAC_PI_2;

  fn close(a: (Vec2, Vec2), b: (Vec2, Vec2)) -> bool {
    (a.0 - b.0).abs().max_element() < 1e-3 && (a.1 - b.1).abs().max_element() < 1e-3
  }

  #[test]
  fn defaults() {
    let c = Collider::rect(10.0, 20.0);
    assert_eq!(c.shape, ColliderShape::Rect { size: Vec2::new(10.0, 20.0) });
    assert_eq!((c.friction, c.restitution, c.density), (0.5, 0.0, 1.0));
    assert_eq!((c.layer, c.mask), (1, u32::MAX));
    assert!(!c.is_trigger && c.mass.is_none() && c.force_threshold.is_none());
  }

  #[test]
  fn builders() {
    let c = Collider::circle(4.0)
      .with_offset(1.0, 2.0)
      .with_rotation(0.5)
      .as_trigger()
      .with_layer(2)
      .with_mask(6)
      .with_density(3.0)
      .with_mass(7.0)
      .with_friction(0.1)
      .with_restitution(0.9)
      .with_force_events(50.0);
    assert_eq!(c.offset, Vec2::new(1.0, 2.0));
    assert_eq!(c.rotation, 0.5);
    assert!(c.is_trigger);
    assert_eq!((c.layer, c.mask), (2, 6));
    assert_eq!((c.density, c.mass), (3.0, Some(7.0)));
    assert_eq!((c.friction, c.restitution), (0.1, 0.9));
    assert_eq!(c.force_threshold, Some(50.0));
  }

  #[test]
  fn shapes() {
    assert_eq!(
      Collider::capsule(40.0, 10.0).shape,
      ColliderShape::Capsule { height: 40.0, radius: 10.0 }
    );
    let tri = [Vec2::ZERO, Vec2::X, Vec2::Y];
    assert_eq!(Collider::polygon(tri).shape, ColliderShape::Polygon { points: tri.to_vec() });
    assert_eq!(Collider::polyline(tri).shape, ColliderShape::Polyline { points: tri.to_vec() });
    assert_eq!(
      Collider::segment(Vec2::ZERO, Vec2::X).shape,
      ColliderShape::Segment { a: Vec2::ZERO, b: Vec2::X }
    );
  }

  #[test]
  fn world_bounds_follow_offset_rotation_and_scale() {
    let at = |t: Transform| GlobalTransform::from_transform(&t);
    let rect = Collider::rect(20.0, 10.0);
    assert!(close(
      rect.world_bounds(&at(Transform::from_xy(100.0, 50.0))),
      (Vec2::new(90.0, 45.0), Vec2::new(110.0, 55.0))
    ));
    assert!(close(
      rect.world_bounds(&at(Transform::from_xy(100.0, 50.0).with_rotation(FRAC_PI_2))),
      (Vec2::new(95.0, 40.0), Vec2::new(105.0, 60.0))
    ));
    assert!(close(
      rect.world_bounds(&at(Transform::IDENTITY.with_scale(Vec2::splat(2.0)))),
      (Vec2::new(-20.0, -10.0), Vec2::new(20.0, 10.0))
    ));
    let circle = Collider::circle(8.0).with_offset(0.0, -10.0);
    assert!(close(
      circle.world_bounds(&at(Transform::IDENTITY)),
      (Vec2::new(-8.0, -18.0), Vec2::new(8.0, -2.0))
    ));
    let capsule = Collider::capsule(40.0, 10.0);
    assert!(close(
      capsule.world_bounds(&at(Transform::IDENTITY)),
      (Vec2::new(-10.0, -20.0), Vec2::new(10.0, 20.0))
    ));
  }
}
```

`shape.rs` tests:

```rust
#[cfg(test)]
mod tests {
  use super::*;
  use rapier2d::prelude::{Pose, ShapeType};

  fn local_aabb(shape: &SharedShape) -> (Vec2, Vec2) {
    let aabb = shape.compute_aabb(&Pose::IDENTITY);
    (from_rapier(aabb.mins), from_rapier(aabb.maxs))
  }

  #[test]
  fn every_shape_builds_under_the_identity() {
    let tri = vec![Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(0.0, 10.0)];
    for shape in [
      ColliderShape::Rect { size: Vec2::new(10.0, 4.0) },
      ColliderShape::Circle { radius: 3.0 },
      ColliderShape::Capsule { height: 20.0, radius: 4.0 },
      ColliderShape::Polygon { points: tri.clone() },
      ColliderShape::Segment { a: Vec2::ZERO, b: Vec2::X },
      ColliderShape::Polyline { points: tri },
    ] {
      assert!(build(&shape, Mat2::IDENTITY).is_some(), "{shape:?}");
    }
  }

  #[test]
  fn degenerate_shapes_are_rejected() {
    let line = vec![Vec2::ZERO, Vec2::X, Vec2::new(2.0, 0.0)];
    for shape in [
      ColliderShape::Rect { size: Vec2::new(0.0, 4.0) },
      ColliderShape::Rect { size: Vec2::new(f32::NAN, 4.0) },
      ColliderShape::Circle { radius: -1.0 },
      ColliderShape::Capsule { height: 0.0, radius: 4.0 },
      ColliderShape::Polygon { points: vec![Vec2::ZERO, Vec2::X] },
      ColliderShape::Polygon { points: line },
      ColliderShape::Segment { a: Vec2::X, b: Vec2::X },
      ColliderShape::Polyline { points: vec![Vec2::X] },
      ColliderShape::Polyline { points: vec![Vec2::X, Vec2::X] },
    ] {
      assert!(build(&shape, Mat2::IDENTITY).is_none(), "{shape:?}");
    }
    let rect = ColliderShape::Rect { size: Vec2::ONE };
    assert!(build(&rect, Mat2::ZERO).is_none(), "zero scale");
  }

  #[test]
  fn a_capsule_shorter_than_its_width_is_a_circle() {
    let shape = build(&ColliderShape::Capsule { height: 6.0, radius: 4.0 }, Mat2::IDENTITY).unwrap();
    assert_eq!(shape.shape_type(), ShapeType::Ball);
  }

  #[test]
  fn a_diagonal_map_scales_a_rect_into_a_cuboid() {
    let map = Mat2::from_diagonal(Vec2::new(2.0, -3.0));
    let shape = build(&ColliderShape::Rect { size: Vec2::new(10.0, 4.0) }, map).unwrap();
    assert_eq!(shape.shape_type(), ShapeType::Cuboid);
    assert_eq!(local_aabb(&shape), (Vec2::new(-10.0, -6.0), Vec2::new(10.0, 6.0)));
  }

  #[test]
  fn a_skewing_map_turns_a_rect_into_a_polygon() {
    let map = Mat2::from_cols(Vec2::new(1.0, 0.0), Vec2::new(0.5, 1.0));
    let shape = build(&ColliderShape::Rect { size: Vec2::new(10.0, 10.0) }, map).unwrap();
    assert_eq!(shape.shape_type(), ShapeType::ConvexPolygon);
    let (min, max) = local_aabb(&shape);
    assert!((max.x - 7.5).abs() < 1e-4 && (min.x + 7.5).abs() < 1e-4, "{min} {max}");
  }

  #[test]
  fn circles_scale_uniformly_and_stretch_into_polygons() {
    let circle = ColliderShape::Circle { radius: 5.0 };
    let uniform = build(&circle, Mat2::from_diagonal(Vec2::splat(2.0))).unwrap();
    assert_eq!(uniform.shape_type(), ShapeType::Ball);
    assert_eq!(local_aabb(&uniform), (Vec2::splat(-10.0), Vec2::splat(10.0)));
    let stretched = build(&circle, Mat2::from_diagonal(Vec2::new(2.0, 1.0))).unwrap();
    assert_eq!(stretched.shape_type(), ShapeType::ConvexPolygon);
    let (min, max) = local_aabb(&stretched);
    assert!((max.x - 10.0).abs() < 0.01 && (max.y - 5.0).abs() < 0.01, "{min} {max}");
  }

  #[test]
  fn a_mirrored_polygon_is_still_valid() {
    let tri = ColliderShape::Polygon {
      points: vec![Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(0.0, 10.0)],
    };
    let shape = build(&tri, Mat2::from_diagonal(Vec2::new(-1.0, 1.0))).unwrap();
    assert_eq!(local_aabb(&shape), (Vec2::new(-10.0, 0.0), Vec2::new(0.0, 10.0)));
  }
}
```

- [ ] **Step 2: Write the failing behaviour tests (in `sync.rs`'s tests)**

```rust
  #[test]
  fn a_bouncy_ball_bounces_off_a_default_floor() {
    let (mut world, mut s) = world(60);
    floor(&mut world, 200.0);
    let b = world
      .spawn((
        Transform::IDENTITY,
        RigidBody::dynamic(),
        Collider::circle(10.0).with_restitution(1.0),
      ))
      .id();
    // Down, bounce, and most of the way back up within two seconds
    let mut lowest_after_bounce = f32::MAX;
    let mut bounced = false;
    for _ in 0..120 {
      run(&mut world, &mut s, 1);
      let v = world.get::<Velocity>(b).unwrap().dy;
      bounced |= v < 0.0;
      if bounced {
        lowest_after_bounce = lowest_after_bounce.min(at(&world, b).translation.y);
      }
    }
    assert!(bounced && lowest_after_bounce < 30.0, "{lowest_after_bounce}");
  }

  fn slid(friction: f32) -> f32 {
    let (mut world, mut s) = world(60);
    floor(&mut world, 100.0);
    let e = world
      .spawn((
        Transform::from_xy(0.0, 90.0),
        RigidBody::dynamic().lock_rotation(),
        Velocity::new(100.0, 0.0),
        Collider::rect(20.0, 20.0).with_friction(friction),
      ))
      .id();
    run(&mut world, &mut s, 60);
    world.get::<Velocity>(e).unwrap().dx
  }

  #[test]
  fn zero_friction_on_either_side_slides() {
    assert!(slid(0.0) > 95.0, "{}", slid(0.0));
    assert!(slid(0.5) < 50.0, "{}", slid(0.5));
  }

  #[test]
  fn layers_filter_contacts_both_ways() {
    let fell = |collider: Collider| {
      let (mut world, mut s) = world(60);
      floor(&mut world, 100.0);
      let e = world.spawn((Transform::IDENTITY, RigidBody::dynamic(), collider)).id();
      run(&mut world, &mut s, 60);
      at(&world, e).translation.y
    };
    assert!(fell(Collider::circle(10.0)) < 95.0, "lands by default");
    assert!(fell(Collider::circle(10.0).with_layer(0b10).with_mask(0b10)) > 150.0, "its mask skips the floor");
  }

  #[test]
  fn the_floor_s_mask_matters_too() {
    let (mut world, mut s) = world(60);
    world.spawn((
      Transform::from_xy(0.0, 110.0),
      Collider::rect(400.0, 20.0).with_mask(0b1),
    ));
    let e = world
      .spawn((Transform::IDENTITY, RigidBody::dynamic(), Collider::circle(10.0).with_layer(0b100)))
      .id();
    run(&mut world, &mut s, 60);
    assert!(at(&world, e).translation.y > 150.0, "the floor does not collide with layer 3");
  }

  #[test]
  fn a_kinematic_body_entering_a_fixed_trigger_is_detected() {
    let (mut world, mut s) = world(60);
    let zone = world
      .spawn((Transform::from_xy(100.0, 0.0), Collider::rect(40.0, 40.0).as_trigger()))
      .id();
    let mover = world
      .spawn((
        Transform::IDENTITY,
        RigidBody::kinematic(),
        Velocity::new(100.0, 0.0),
        Collider::rect(10.0, 10.0),
      ))
      .id();
    run(&mut world, &mut s, 60);
    let messages = world.resource::<Messages<CollisionStarted>>();
    let started: Vec<_> = messages.get_cursor().read(messages).copied().collect();
    assert!(started.iter().any(|m| m.involves(mover) && m.involves(zone)), "{started:?}");
  }

  #[test]
  fn density_and_mass_set_the_body_s_mass() {
    let (mut world, mut s) = world(60);
    let dense = world
      .spawn((Transform::IDENTITY, RigidBody::dynamic(), Collider::rect(100.0, 100.0).with_density(2.0)))
      .id();
    let heavy = world
      .spawn((Transform::from_xy(300.0, 0.0), RigidBody::dynamic(), Collider::rect(10.0, 10.0).with_mass(5.0)))
      .id();
    run(&mut world, &mut s, 1);
    let physics = world.resource::<Physics>();
    assert!((physics.mass(dense).unwrap() - 2.0).abs() < 1e-3);
    assert!((physics.mass(heavy).unwrap() - 5.0).abs() < 1e-3);
  }

  #[test]
  fn an_edited_collider_applies_in_place() {
    let (mut world, mut s) = world(60);
    let f = floor(&mut world, 100.0);
    let b = ball(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, 120);
    // The floor grows 40 px upward: its top is now at 60
    *world.get_mut::<Collider>(f).unwrap() = Collider::rect(400.0, 100.0);
    world.get_mut::<Transform>(f).unwrap().translation.y = 110.0;
    run(&mut world, &mut s, 120);
    assert!((at(&world, b).translation.y - 50.0).abs() < 3.0, "{:?}", at(&world, b));
  }

  #[test]
  fn an_invalid_shape_is_skipped_until_it_is_fixed() {
    let (mut world, mut s) = world(60);
    let e = world.spawn((Transform::IDENTITY, Collider::rect(0.0, 10.0))).id();
    run(&mut world, &mut s, 1);
    assert_eq!(world.resource::<Physics>().collider_count(), 0);
    world.get_mut::<Collider>(e).unwrap().shape = ColliderShape::Rect { size: Vec2::splat(10.0) };
    run(&mut world, &mut s, 1);
    assert_eq!(world.resource::<Physics>().collider_count(), 1);
  }

  #[test]
  fn capsules_polygons_and_polylines_hold_things_up() {
    let (mut world, mut s) = world(60);
    world.spawn((
      Transform::IDENTITY,
      Collider::polyline([Vec2::new(-200.0, 100.0), Vec2::new(200.0, 100.0)]),
    ));
    let capsule = world
      .spawn((Transform::from_xy(-50.0, 0.0), RigidBody::dynamic().lock_rotation(), Collider::capsule(40.0, 10.0)))
      .id();
    let triangle = world
      .spawn((
        Transform::from_xy(50.0, 0.0),
        RigidBody::dynamic().lock_rotation(),
        Collider::polygon([Vec2::new(-10.0, 10.0), Vec2::new(10.0, 10.0), Vec2::new(0.0, -10.0)]),
      ))
      .id();
    run(&mut world, &mut s, 120);
    assert!((at(&world, capsule).translation.y - 80.0).abs() < 2.0);
    assert!((at(&world, triangle).translation.y - 90.0).abs() < 2.0);
  }
```

`tile.rs` test (replace the `GroupedCollider` tests):

```rust
  #[test]
  fn build_with_collider_returns_a_rect_over_the_parts() {
    let sprite = SpriteHandle::from_sprite(Sprite::solid_rect(32, 32, 255, 255, 255, 255), "tile-test");
    let (composite, collider) = CompositeTileBuilder::new()
      .cap(sprite.clone(), 0.0, 0.0)
      .body_tiled(sprite, 0.0, 32.0, TileDirection::Down, 128.0)
      .build_with_collider();
    assert_eq!(composite.size(), (32.0, 160.0));
    assert_eq!(collider.shape, ColliderShape::Rect { size: Vec2::new(32.0, 160.0) });
    assert_eq!(collider.offset, Vec2::new(16.0, 80.0));
  }
```

(Adapt the sprite construction to how `tile.rs`'s existing tests build one.)

- [ ] **Step 3: Run them to verify they fail**

Run: `cargo test -p ryo2d --lib`
Expected: compile errors (`Collider::rect`, `ColliderShape::Rect`, `shape::build`).

- [ ] **Step 4: Write `collider.rs`**

```rust
//! Collision shapes and their material.

use std::f32::consts::FRAC_PI_2;

use crate::core::ecs::{Component, GlobalTransform, Transform, Vec2};

/// The geometry of a collider, in its own space: centred on its origin
/// (the entity's origin plus `Collider::offset`, turned by `rotation`).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ColliderShape {
  /// A box `size` wide and high.
  Rect { size: Vec2 },
  Circle { radius: f32 },
  /// An upright capsule `height` tall including its round ends; a capsule
  /// no taller than its width is a circle. Rotate it for a horizontal one.
  Capsule { height: f32, radius: f32 },
  /// The convex hull of the points. Concave shapes are several colliders.
  Polygon { points: Vec<Vec2> },
  /// One edge.
  Segment { a: Vec2, b: Vec2 },
  /// An open chain of edges: terrain and level outlines.
  Polyline { points: Vec<Vec2> },
}

/// A collision shape and its material. On an entity with a `RigidBody` (or
/// below one) it is part of that body; with no body above it, it is a fixed
/// collider of its own. The shape follows the entity's world scale. Edits
/// apply in place on the next step.
#[derive(Component, Debug, Clone, PartialEq)]
#[non_exhaustive]
#[require(Transform)]
pub struct Collider {
  pub shape: ColliderShape,
  /// The shape's origin relative to the entity's.
  pub offset: Vec2,
  /// The shape's turn relative to the entity, in radians.
  pub rotation: f32,
  /// Detects overlaps but never pushes.
  pub is_trigger: bool,
  /// The layers it is on (bits). Two colliders interact only if each one's
  /// layer is in the other's mask.
  pub layer: u32,
  /// The layers it interacts with (bits).
  pub mask: u32,
  /// Mass per square metre (`PhysicsConfig::pixels_per_meter`).
  pub density: f32,
  /// This collider's mass, overriding `density`.
  pub mass: Option<f32>,
  /// 0 is frictionless. Two surfaces combine as the geometric mean.
  pub friction: f32,
  /// 0 does not bounce, 1 bounces back fully. Two surfaces take the larger.
  pub restitution: f32,
  /// Send `ContactForce` messages while its contact force exceeds this.
  pub force_threshold: Option<f32>,
}

impl Collider {
  fn new(shape: ColliderShape) -> Self {
    Self {
      shape,
      offset: Vec2::ZERO,
      rotation: 0.0,
      is_trigger: false,
      layer: 1,
      mask: u32::MAX,
      density: 1.0,
      mass: None,
      friction: 0.5,
      restitution: 0.0,
      force_threshold: None,
    }
  }

  pub fn rect(width: f32, height: f32) -> Self {
    Self::new(ColliderShape::Rect { size: Vec2::new(width, height) })
  }

  pub fn circle(radius: f32) -> Self {
    Self::new(ColliderShape::Circle { radius })
  }

  /// An upright capsule `height` tall, ends included.
  pub fn capsule(height: f32, radius: f32) -> Self {
    Self::new(ColliderShape::Capsule { height, radius })
  }

  /// The convex hull of `points`.
  pub fn polygon(points: impl IntoIterator<Item = Vec2>) -> Self {
    Self::new(ColliderShape::Polygon { points: points.into_iter().collect() })
  }

  pub fn segment(a: Vec2, b: Vec2) -> Self {
    Self::new(ColliderShape::Segment { a, b })
  }

  /// An open chain through `points`.
  pub fn polyline(points: impl IntoIterator<Item = Vec2>) -> Self {
    Self::new(ColliderShape::Polyline { points: points.into_iter().collect() })
  }

  pub fn with_offset(mut self, x: f32, y: f32) -> Self {
    self.offset = Vec2::new(x, y);
    self
  }

  pub fn with_rotation(mut self, radians: f32) -> Self {
    self.rotation = radians;
    self
  }

  pub fn as_trigger(mut self) -> Self {
    self.is_trigger = true;
    self
  }

  pub fn with_layer(mut self, layer: u32) -> Self {
    self.layer = layer;
    self
  }

  pub fn with_mask(mut self, mask: u32) -> Self {
    self.mask = mask;
    self
  }

  pub fn with_density(mut self, density: f32) -> Self {
    self.density = density;
    self
  }

  pub fn with_mass(mut self, mass: f32) -> Self {
    self.mass = Some(mass);
    self
  }

  pub fn with_friction(mut self, friction: f32) -> Self {
    self.friction = friction;
    self
  }

  pub fn with_restitution(mut self, restitution: f32) -> Self {
    self.restitution = restitution;
    self
  }

  /// Opt into `ContactForce` messages above `threshold` (mass·px/s²).
  pub fn with_force_events(mut self, threshold: f32) -> Self {
    self.force_threshold = Some(threshold);
    self
  }

  /// The world-space bounding box `(min, max)` when the collider's entity
  /// has `global`. Round ends grow by the largest scale.
  pub fn world_bounds(&self, global: &GlobalTransform) -> (Vec2, Vec2) {
    let affine = global.affine();
    let turn = Vec2::from_angle(self.rotation);
    let place = |p: Vec2| affine.transform_point2(self.offset + turn.rotate(p));
    let scale = global.scale().max_element();
    let round = |centre: Vec2, radius: f32| {
      let c = place(centre);
      let r = Vec2::splat(radius * scale);
      (c - r, c + r)
    };
    match &self.shape {
      ColliderShape::Rect { size } => {
        let h = *size / 2.0;
        bounds_of([(-h.x, -h.y), (h.x, -h.y), (h.x, h.y), (-h.x, h.y)].map(|(x, y)| place(Vec2::new(x, y))))
      }
      ColliderShape::Circle { radius } => round(Vec2::ZERO, *radius),
      ColliderShape::Capsule { height, radius } => {
        let h = (height / 2.0 - radius).max(0.0);
        let (a, b) = (round(Vec2::new(0.0, -h), *radius), round(Vec2::new(0.0, h), *radius));
        (a.0.min(b.0), a.1.max(b.1))
      }
      ColliderShape::Polygon { points } | ColliderShape::Polyline { points } => {
        bounds_of(points.iter().map(|p| place(*p)))
      }
      ColliderShape::Segment { a, b } => bounds_of([place(*a), place(*b)]),
    }
  }
}

fn bounds_of(points: impl IntoIterator<Item = Vec2>) -> (Vec2, Vec2) {
  let mut points = points.into_iter();
  let Some(first) = points.next() else {
    return (Vec2::ZERO, Vec2::ZERO);
  };
  points.fold((first, first), |(min, max), p| (min.min(p), max.max(p)))
}
```

(`FRAC_PI_2` is only used by the tests; import it there instead if clippy flags it.)

- [ ] **Step 5: Write `shape.rs`**

```rust
//! Rapier shapes from `ColliderShape`s, under the linear part of the
//! collider's world transform taken into the frame Rapier places it in
//! (scale, mirroring and, for turned shapes under non-uniform scale, skew).

use std::f32::consts::{PI, TAU};

use glam::Mat2;
use rapier2d::prelude::SharedShape;

use super::collider::ColliderShape;
use super::convert::to_rapier;
#[cfg(test)]
use super::convert::from_rapier;
use crate::core::ecs::Vec2;

/// Points on a full circle approximating a stretched round shape.
const ARC_POINTS: usize = 24;
/// Relative tolerance for "this map has no off-diagonal part" and "these
/// two scales are equal".
const MAP_TOLERANCE: f32 = 1e-4;

fn positive(v: f32) -> bool {
  v.is_finite() && v > 0.0
}

/// The axis scales when `map` is diagonal (mirroring allowed).
fn diagonal(map: Mat2) -> Option<Vec2> {
  let scale = Vec2::new(map.x_axis.x, map.y_axis.y);
  let tolerance = MAP_TOLERANCE * scale.abs().max_element().max(1.0);
  (map.x_axis.y.abs() < tolerance && map.y_axis.x.abs() < tolerance).then(|| scale.abs())
}

/// The scale when `map` is diagonal with equal axis scales.
fn uniform(map: Mat2) -> Option<f32> {
  let scale = diagonal(map)?;
  ((scale.x - scale.y).abs() < MAP_TOLERANCE * scale.x.max(1.0)).then_some(scale.x)
}

/// `count` points on an arc of `radius` around `centre`, from `from` to `to` radians.
fn arc(centre: Vec2, radius: f32, from: f32, to: f32, count: usize) -> impl Iterator<Item = Vec2> {
  (0..count).map(move |i| {
    let t = from + (to - from) * i as f32 / count as f32;
    centre + Vec2::from_angle(t) * radius
  })
}

fn hull(points: impl IntoIterator<Item = Vec2>, map: Mat2) -> Option<SharedShape> {
  let mapped: Vec<_> = points.into_iter().map(|p| to_rapier(map * p)).collect();
  SharedShape::convex_hull(&mapped)
}

/// The Rapier shape for `shape` with `map` applied to its points, or `None`
/// when the result is degenerate (a zero, negative or non-finite size, a
/// polygon without three corners, a zero-length segment, a polyline without
/// an edge, or a map that flattens everything).
pub(crate) fn build(shape: &ColliderShape, map: Mat2) -> Option<SharedShape> {
  if !map.is_finite() || map.determinant().abs() <= f32::EPSILON {
    return None;
  }
  match shape {
    ColliderShape::Rect { size } => {
      if !(positive(size.x) && positive(size.y)) {
        return None;
      }
      let h = *size / 2.0;
      match diagonal(map) {
        Some(scale) => Some(SharedShape::cuboid(h.x * scale.x, h.y * scale.y)),
        None => hull(
          [(-h.x, -h.y), (h.x, -h.y), (h.x, h.y), (-h.x, h.y)].map(|(x, y)| Vec2::new(x, y)),
          map,
        ),
      }
    }
    ColliderShape::Circle { radius } => {
      if !positive(*radius) {
        return None;
      }
      match uniform(map) {
        Some(scale) => Some(SharedShape::ball(radius * scale)),
        None => hull(arc(Vec2::ZERO, *radius, 0.0, TAU, ARC_POINTS), map),
      }
    }
    ColliderShape::Capsule { height, radius } => {
      if !(positive(*height) && positive(*radius)) {
        return None;
      }
      let half = (height / 2.0 - radius).max(0.0);
      match uniform(map) {
        Some(scale) if half == 0.0 => Some(SharedShape::ball(radius * scale)),
        Some(scale) => Some(SharedShape::capsule_y(half * scale, radius * scale)),
        None if half == 0.0 => hull(arc(Vec2::ZERO, *radius, 0.0, TAU, ARC_POINTS), map),
        None => hull(
          arc(Vec2::new(0.0, half), *radius, 0.0, PI, ARC_POINTS / 2)
            .chain(arc(Vec2::new(0.0, -half), *radius, PI, TAU, ARC_POINTS / 2))
            .chain([Vec2::new(-radius, half), Vec2::new(radius, -half)]),
          map,
        ),
      }
    }
    ColliderShape::Polygon { points } => {
      if points.len() < 3 || !points.iter().all(|p| p.is_finite()) {
        return None;
      }
      hull(points.iter().copied(), map)
    }
    ColliderShape::Segment { a, b } => {
      let (a, b) = (map * *a, map * *b);
      (a.is_finite() && b.is_finite() && a.distance(b) > f32::EPSILON)
        .then(|| SharedShape::segment(to_rapier(a), to_rapier(b)))
    }
    ColliderShape::Polyline { points } => {
      let mut mapped: Vec<Vec2> = Vec::with_capacity(points.len());
      for p in points {
        let p = map * *p;
        if !p.is_finite() {
          return None;
        }
        if mapped.last().is_none_or(|last| last.distance(p) > f32::EPSILON) {
          mapped.push(p);
        }
      }
      (mapped.len() >= 2)
        .then(|| SharedShape::polyline(mapped.into_iter().map(to_rapier).collect(), None))
    }
  }
}
```

If `SharedShape::convex_hull` accepts collinear points and returns `Some`, add a check that the hull has three points (`shape.as_convex_polygon().is_some_and(|p| p.points().len() >= 3)`) so the collinear-polygon test passes.

- [ ] **Step 6: Update `world.rs`**

`mod.rs`: `mod collider;` and `pub(crate) mod shape;` replace `pub mod collision;`, with `pub use collider::{Collider, ColliderShape};`. Imports in `world.rs` add `ActiveCollisionTypes, CoefficientCombineRule, Group, InteractionGroups, InteractionTestMode` and `glam::Mat2`; `use super::collider::Collider; use super::shape;`. Replace `collider_builder`, `local_pose` and `world_pose`:

```rust
/// A collider's pose relative to its body.
fn local_pose(collider: &Collider) -> Pose {
  pose(collider.offset, collider.rotation)
}

/// A collider's pose in the world when its entity is at `translation`, turned by `rotation`.
fn world_pose(translation: Vec2, rotation: f32, collider: &Collider) -> Pose {
  pose(
    translation + Vec2::from_angle(rotation).rotate(collider.offset),
    rotation + collider.rotation,
  )
}

fn groups(collider: &Collider) -> InteractionGroups {
  InteractionGroups::new(
    Group::from(collider.layer),
    Group::from(collider.mask),
    InteractionTestMode::And,
  )
}

fn events(collider: &Collider) -> ActiveEvents {
  if collider.force_threshold.is_some() {
    ActiveEvents::COLLISION_EVENTS | ActiveEvents::CONTACT_FORCE_EVENTS
  } else {
    ActiveEvents::COLLISION_EVENTS
  }
}

fn active_types() -> ActiveCollisionTypes {
  ActiveCollisionTypes::all().difference(ActiveCollisionTypes::FIXED_FIXED)
}

impl Physics {
  /// Rapier's collider for `collider` with `map` applied to its shape, or
  /// `None` when the shape is degenerate.
  fn collider_builder(&self, collider: &Collider, map: Mat2) -> Option<ColliderBuilder> {
    let shape = shape::build(&collider.shape, map)?;
    let builder = ColliderBuilder::new(shape)
      .friction(finite_or(collider.friction, 0.5).max(0.0))
      .friction_combine_rule(CoefficientCombineRule::GeometricMean)
      .restitution(finite_or(collider.restitution, 0.0).max(0.0))
      .restitution_combine_rule(CoefficientCombineRule::Max)
      .sensor(collider.is_trigger)
      .collision_groups(groups(collider))
      .active_collision_types(active_types())
      .active_events(events(collider))
      .contact_force_event_threshold(finite_or(collider.force_threshold.unwrap_or(0.0), 0.0));
    Some(match collider.mass {
      Some(mass) => builder.mass(finite_or(mass, 0.0).max(0.0)),
      None => builder.density(self.rapier_density(finite_or(collider.density, 1.0).max(0.0))),
    })
  }
}
```

`add_collider` calls `self.collider_builder(collider, Mat2::IDENTITY)`. Add:

```rust
  /// Apply an edited `Collider` in place; one not simulated yet (a shape that
  /// was invalid) is registered instead. An edit to an invalid shape takes it
  /// out of the simulation.
  pub(crate) fn update_collider(
    &mut self,
    entity: Entity,
    body: Option<Entity>,
    translation: Vec2,
    rotation: f32,
    new: &Collider,
  ) {
    let Some(entry) = self.collider_entries.get(&entity) else {
      self.add_collider(entity, body, translation, rotation, new);
      return;
    };
    if entry.collider == *new {
      return;
    }
    let (handle, attached, old) = (entry.handle, entry.body.is_some(), entry.collider.clone());
    if old.shape != new.shape {
      let Some(shape) = shape::build(&new.shape, Mat2::IDENTITY) else {
        self.remove_collider(entity);
        self.warn_once(entity, "an invalid collider shape is not simulated");
        return;
      };
      if let Some(collider) = self.colliders.get_mut(handle) {
        collider.set_shape(shape);
      }
    }
    let density = self.rapier_density(finite_or(new.density, 1.0).max(0.0));
    let Some(collider) = self.colliders.get_mut(handle) else {
      return;
    };
    if old.offset != new.offset || old.rotation != new.rotation {
      if attached {
        collider.set_position_wrt_parent(local_pose(new));
      } else {
        collider.set_position(world_pose(translation, rotation, new));
      }
    }
    if old.friction != new.friction {
      collider.set_friction(finite_or(new.friction, 0.5).max(0.0));
    }
    if old.restitution != new.restitution {
      collider.set_restitution(finite_or(new.restitution, 0.0).max(0.0));
    }
    if old.density != new.density || old.mass != new.mass {
      match new.mass {
        Some(mass) => collider.set_mass(finite_or(mass, 0.0).max(0.0)),
        None => collider.set_density(density),
      }
    }
    if old.layer != new.layer || old.mask != new.mask {
      collider.set_collision_groups(groups(new));
    }
    if old.is_trigger != new.is_trigger {
      collider.set_sensor(new.is_trigger);
    }
    if old.force_threshold != new.force_threshold {
      collider.set_active_events(events(new));
      collider.set_contact_force_event_threshold(finite_or(new.force_threshold.unwrap_or(0.0), 0.0));
    }
    if let Some(entry) = self.collider_entries.get_mut(&entity) {
      entry.collider = new.clone();
    }
  }
```

- [ ] **Step 7: Update `sync.rs`**

Remove every `GroupedCollider` mention (`grouped_collider`, the `Option<&GroupedCollider>` in `ColliderItem`, the `RemovedComponents<GroupedCollider>`); `ColliderItem` becomes `(Entity, &Transform, &Collider, Has<RigidBody>)` and `HasPhysics` drops `GroupedCollider`. In `register_new`, colliders use `Changed<Collider>` (which includes added ones) and upsert:

```rust
  for (entity, transform, collider, has_body) in &colliders {
    physics.update_collider(
      entity,
      has_body.then_some(entity),
      transform.translation,
      transform.rotation,
      collider,
    );
  }
```

with `colliders: Query<ColliderItem, (Changed<Collider>, Without<ChildOf>)>`. `add_collider` in `track_disabled`/`track_parents` passes `collider` directly.

- [ ] **Step 8: Remove `GroupedCollider`**

`src/core/graphics/tile.rs`: delete the `GroupedCollider` struct, impls and tests; update the module doc example (`build_with_collider()` now gives a `Collider`, the body `RigidBody::kinematic()`); and:

```rust
  /// Build the composite tile and a rect `Collider` covering all its parts.
  pub fn build_with_collider(self) -> (CompositeTile, Collider) {
    let composite = CompositeTile {
      parts: self.parts,
      visible: self.visible,
      z_order: self.z_order,
    };
    let (min_x, min_y, max_x, max_y) = composite.bounds();
    let size = Vec2::new(max_x - min_x, max_y - min_y);
    let collider = Collider::rect(size.x, size.y)
      .with_offset(min_x + size.x / 2.0, min_y + size.y / 2.0);
    (composite, collider)
  }
```

Remove `GroupedCollider` from `graphics/mod.rs` and the prelude.

- [ ] **Step 9: Port the geometry users**

- `src/core/particles/mod.rs`, `ParticleCollider::from_component`:

```rust
    let geometry = match collider.shape {
      ColliderShape::Circle { radius } => {
        let center = global.transform_point(collider.offset);
        ColliderGeometry::Circle {
          x: center.x,
          y: center.y,
          radius: radius * global.scale().max_element(),
        }
      }
      // Every other shape collides by its world bounds until the particles rewrite
      _ => {
        let (min, max) = collider.world_bounds(global);
        ColliderGeometry::Box { min_x: min.x, min_y: min.y, max_x: max.x, max_y: max.y }
      }
    };
```

  and its tests: `Collider::box_collider` -> `Collider::rect`.
- `src/core/navigation/grid.rs`, `block_collider`: `let (min, max) = collider.world_bounds(global);` and fix its doc ("the collider's world bounds, as physics places it").
- Every other use (grep `box_collider\|circle_collider\|ColliderShape::Box\|ColliderShape::Circle\|world_center\|world_bounds(\|GroupedCollider\|grouped_collider`): `box_collider(w, h)` -> `rect(w, h)`, `circle_collider(r)` -> `circle(r)`, `ColliderShape::Box { width, height }` -> `ColliderShape::Rect { size }` (`size.x`, `size.y`). leap-king `collider.world_bounds(transform.translation)` -> `collider.world_bounds(&GlobalTransform::from_transform(transform))` (its players and platforms are root entities). flappy `debug.rs`: the pipe check reads the pipe's `Collider` (a rect whose `offset` is its centre) instead of `GroupedCollider` (whose offset was the top-left), and `ColliderShape::Box { width, height }` becomes `ColliderShape::Rect { size }`.
- Defaults changed: restitution 0 (was 0.5) and friction 0.5 (was 0.3). breakout's ball sets its own restitution; check each game by eye in Step 11.

- [ ] **Step 10: Run the tests**

Run: `cargo test -p ryo2d --lib && cargo test --workspace`
Expected: PASS.

- [ ] **Step 11: Check the games by eye**

`./dev breakout`, `./dev flappy`, `./dev bullet-hell`, `./dev leap-king` (host a local game and play until the first platform), plus `./dev pathfinding` and `./dev particles` (the collider-aware pages). Expected: unchanged behaviour.

- [ ] **Step 12: Gate and commit**

```bash
git add -A src tests examples benches
git commit -m "feat: the full collider set" -m "- rect, circle, capsule, polygon, segment and polyline shapes" -m "- layers filter the simulation; triggers see kinematic bodies" -m "- density or mass, Box2D-style friction and restitution, defaults 0.5 and 0" -m "- collider edits apply in place; invalid shapes are skipped" -m "- GroupedCollider removed: composite tiles build a Collider"
```

---
### Task 9: Hierarchy and scale

Bodies may sit anywhere in the hierarchy and colliders may sit on any entity below their body. Physics reads world poses from `GlobalTransform` (propagated at the start of the physics chain), writes bodies back into their local `Transform` through their parent, and builds collider shapes under the collider's world scale.

**Files:**
- Modify: `src/core/physics/world.rs` (placements; one `sync_collider` path replaces `add_collider`'s callers, `move_collider` and `update_collider`)
- Modify: `src/core/physics/sync.rs` (propagation in the chain; `register_bodies` and `sync_colliders` replace `register_new` and `track_parents`; poses from `GlobalTransform`; parent-aware write-back)
- Modify: `src/core/schedule.rs` (the `FixedSet::Physics` doc)

**Interfaces:**
- Consumes: `shape::build` (Task 8), `Physics::{add_body, remove_body, remove_collider, warn_once, rapier_density}`, `crate::core::ecs::propagate::propagate_transforms`.
- Produces:
  - `pub(crate) struct Placement { translation: Vec2, rotation: f32, map: Mat2 }` (relative to the body when attached, else world)
  - `pub(crate) fn sync::placement(global: &GlobalTransform, collider: &Collider, body: Option<(Vec2, f32)>) -> Placement`
  - `Physics::sync_collider(&mut self, entity: Entity, body: Option<Entity>, placement: Placement, collider: &Collider)` (register, reparent, reshape, re-place, material edits)
  - `Physics::waiting: HashSet<Entity>` (colliders whose body is not simulated yet; retried every step)
  - `Physics::collider_pose(&self, entity) -> Option<(Vec2, f32)>` (world pose, crate-internal, for tests)
  - systems: `forget_removed`, `track_disabled`, `propagate_transforms`, `register_bodies`, `sync_colliders`, `push_changes`, `step_simulation`, `read_back`, `emit_messages` (chained in this order)
- Removed: `track_parents`, `Physics::move_collider`, `Physics::update_collider`, the parented-body warning.

- [ ] **Step 1: Write the failing tests**

In `sync.rs`'s tests (remove `a_body_on_a_child_is_not_simulated_until_detached`):

```rust
  use crate::core::ecs::{ChildOf, GlobalTransform};
  use std::f32::consts::FRAC_PI_2;

  #[test]
  fn placement_composes_offset_rotation_scale_and_body() {
    let global = GlobalTransform::from_transform(
      &Transform::from_xy(100.0, 0.0)
        .with_rotation(FRAC_PI_2)
        .with_scale(Vec2::new(2.0, 1.0)),
    );
    let collider = Collider::rect(10.0, 10.0).with_offset(10.0, 0.0);
    let world = placement(&global, &collider, None);
    // The offset is scaled (x2) and turned a quarter: 20 px down
    assert!((world.translation - Vec2::new(100.0, 20.0)).length() < 1e-4, "{:?}", world);
    assert!((world.rotation - FRAC_PI_2).abs() < 1e-5);
    assert!((world.map - glam::Mat2::from_diagonal(Vec2::new(2.0, 1.0))).abs_diff_eq(glam::Mat2::ZERO, 1e-5));
    let relative = placement(&global, &collider, Some((Vec2::new(100.0, 0.0), FRAC_PI_2)));
    assert!((relative.translation - Vec2::new(20.0, 0.0)).length() < 1e-4, "{:?}", relative);
    assert!(relative.rotation.abs() < 1e-5);
  }

  #[test]
  fn child_colliders_add_mass_and_move_with_their_body() {
    let (mut world, mut s) = world(60);
    floor(&mut world, 100.0);
    let body = world
      .spawn((Transform::IDENTITY, RigidBody::dynamic().lock_rotation()))
      .id();
    for x in [-30.0, 30.0] {
      world.spawn((Transform::from_xy(x, 0.0), Collider::rect(20.0, 20.0), ChildOf(body)));
    }
    run(&mut world, &mut s, 120);
    let mass = world.resource::<Physics>().mass(body).unwrap();
    assert!((mass - 0.08).abs() < 1e-4, "two 20x20 boxes: {mass}");
    assert!((at(&world, body).translation.y - 90.0).abs() < 2.0, "{:?}", at(&world, body));
  }

  #[test]
  fn a_collider_spawned_with_its_body_attaches_to_it() {
    let (mut world, mut s) = world(60);
    let body = world.spawn((Transform::IDENTITY, RigidBody::dynamic())).id();
    world.spawn((Transform::IDENTITY, Collider::circle(10.0), ChildOf(body)));
    run(&mut world, &mut s, 30);
    assert!(world.resource::<Physics>().mass(body).unwrap() > 0.0);
    assert!(at(&world, body).translation.y > 10.0, "it falls with its body");
  }

  #[test]
  fn a_kinematic_child_body_follows_its_moving_parent() {
    let (mut world, mut s) = world(60);
    let parent = world.spawn(Transform::IDENTITY).id();
    let child = world
      .spawn((
        Transform::from_xy(0.0, 10.0),
        RigidBody::kinematic(),
        Collider::rect(10.0, 10.0),
        ChildOf(parent),
      ))
      .id();
    for _ in 0..60 {
      world.get_mut::<Transform>(parent).unwrap().translation.x += 1.0;
      run(&mut world, &mut s, 1);
    }
    let (position, _) = world.resource::<Physics>().body_pose(child).unwrap();
    assert!((position - Vec2::new(60.0, 10.0)).length() < 1.01, "{position}");
  }

  #[test]
  fn a_falling_body_under_a_turned_parent_writes_its_local_transform() {
    let (mut world, mut s) = world(60);
    let parent = world
      .spawn(Transform::from_xy(100.0, 0.0).with_rotation(FRAC_PI_2))
      .id();
    let child = world
      .spawn((Transform::IDENTITY, RigidBody::dynamic().lock_rotation(), Collider::circle(5.0), ChildOf(parent)))
      .id();
    run(&mut world, &mut s, 30);
    let (world_position, _) = world.resource::<Physics>().body_pose(child).unwrap();
    assert!((world_position.x - 100.0).abs() < 1e-2, "falls straight down in the world");
    let local = at(&world, child);
    // World +Y is the parent's +X once turned a quarter
    assert!(local.translation.x > 10.0 && local.translation.y.abs() < 1e-2, "{local:?}");
  }

  #[test]
  fn a_scaled_entity_s_collider_scales() {
    let (mut world, mut s) = world(60);
    world.spawn((
      Transform::from_xy(0.0, 110.0).with_scale(Vec2::new(1.0, 2.0)),
      Collider::rect(400.0, 20.0),
    ));
    let b = ball(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, 120);
    // 40 high now: its top is at 90, so the ball rests at 80
    assert!((at(&world, b).translation.y - 80.0).abs() < 2.0, "{:?}", at(&world, b));
  }

  #[test]
  fn a_turned_entity_turns_its_collider() {
    let (mut world, mut s) = world(60);
    no_gravity(&mut world);
    world.spawn((
      Transform::from_xy(100.0, 0.0).with_rotation(FRAC_PI_2),
      Collider::rect(100.0, 10.0),
    ));
    let b = world
      .spawn((Transform::IDENTITY, RigidBody::dynamic().lock_rotation(), Velocity::new(200.0, 0.0), Collider::circle(10.0)))
      .id();
    run(&mut world, &mut s, 60);
    assert!(at(&world, b).translation.x < 86.0, "the upright bar stopped it: {:?}", at(&world, b));
  }

  #[test]
  fn a_zero_scale_leaves_the_collider_out_until_it_grows() {
    let (mut world, mut s) = world(60);
    let e = world
      .spawn((Transform::IDENTITY.with_scale(Vec2::ZERO), Collider::circle(10.0)))
      .id();
    run(&mut world, &mut s, 2);
    assert_eq!(world.resource::<Physics>().collider_count(), 0);
    world.get_mut::<Transform>(e).unwrap().scale = Vec2::ONE;
    run(&mut world, &mut s, 1);
    assert_eq!(world.resource::<Physics>().collider_count(), 1);
    world.get_mut::<Transform>(e).unwrap().scale = Vec2::ZERO;
    run(&mut world, &mut s, 1);
    assert_eq!(world.resource::<Physics>().collider_count(), 0);
  }

  #[test]
  fn reparenting_a_collider_moves_its_mass() {
    let (mut world, mut s) = world(60);
    no_gravity(&mut world);
    let a = world.spawn((Transform::IDENTITY, RigidBody::dynamic())).id();
    let b = world.spawn((Transform::from_xy(200.0, 0.0), RigidBody::dynamic())).id();
    let part = world
      .spawn((Transform::IDENTITY, Collider::rect(100.0, 100.0), ChildOf(a)))
      .id();
    run(&mut world, &mut s, 1);
    assert!((world.resource::<Physics>().mass(a).unwrap() - 1.0).abs() < 1e-3);
    world.entity_mut(part).insert(ChildOf(b));
    run(&mut world, &mut s, 1);
    let physics = world.resource::<Physics>();
    assert!(physics.mass(a).unwrap() < 1e-3);
    assert!((physics.mass(b).unwrap() - 1.0).abs() < 1e-3);
  }

  #[test]
  fn a_parent_losing_its_body_leaves_its_child_colliders_static() {
    let (mut world, mut s) = world(60);
    let platform = world.spawn((Transform::from_xy(0.0, 110.0), RigidBody::kinematic())).id();
    world.spawn((Transform::IDENTITY, Collider::rect(400.0, 20.0), ChildOf(platform)));
    run(&mut world, &mut s, 1);
    world.entity_mut(platform).remove::<RigidBody>();
    let b = ball(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, 120);
    assert!((at(&world, b).translation.y - 90.0).abs() < 2.0);
  }

  #[test]
  fn disabling_a_body_disables_its_child_colliders() {
    let (mut world, mut s) = world(60);
    let platform = world.spawn((Transform::from_xy(0.0, 110.0), RigidBody::fixed())).id();
    world.spawn((Transform::IDENTITY, Collider::rect(400.0, 20.0), ChildOf(platform)));
    run(&mut world, &mut s, 1);
    world.entity_mut(platform).insert(Disabled);
    let b = ball(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, 60);
    assert!(at(&world, b).translation.y > 150.0, "fell through");
  }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p ryo2d --lib physics::sync`
Expected: compile errors (`placement`, `Placement`), then failures for the hierarchy tests.

- [ ] **Step 3: Placements in `world.rs`**

```rust
/// Where a collider sits and how its shape is mapped: relative to its body
/// when it has one, in the world otherwise.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Placement {
  pub(crate) translation: Vec2,
  pub(crate) rotation: f32,
  /// The linear map from the shape's own space into this frame (scale,
  /// mirroring, skew)
  pub(crate) map: Mat2,
}

impl Placement {
  fn is_finite(&self) -> bool {
    self.translation.is_finite() && self.rotation.is_finite() && self.map.is_finite()
  }

  fn pose(&self) -> Pose {
    pose(self.translation, self.rotation)
  }
}

/// Whether two shape maps differ enough to rebuild the shape.
fn maps_differ(a: Mat2, b: Mat2) -> bool {
  let tolerance = 1e-4 * a.abs().to_cols_array().into_iter().fold(1.0, f32::max);
  !a.abs_diff_eq(b, tolerance)
}
```

`ColliderEntry` becomes `{ handle, body: Option<Entity>, collider: Collider, pose: (Vec2, f32), map: Mat2 }`: the pose and map last applied to Rapier (updated only when applied, so sub-tolerance drift never accumulates). Add `pub(crate) waiting: HashSet<Entity>` to `Physics`.

Replace `add_collider`, `move_collider`, `update_collider`, `local_pose` and `world_pose` with:

```rust
  fn add_collider(
    &mut self,
    entity: Entity,
    body: Option<(Entity, RigidBodyHandle)>,
    placement: Placement,
    collider: &Collider,
  ) {
    if !placement.is_finite() {
      self.warn_once(entity, "a collider with a non-finite transform is not simulated");
      return;
    }
    let Some(builder) = self.collider_builder(collider, placement.map) else {
      self.warn_once(entity, "a degenerate collider shape (zero scale included) is not simulated");
      return;
    };
    let builder = builder.position(placement.pose());
    let handle = match body {
      Some((_, parent)) => self.colliders.insert_with_parent(builder, parent, &mut self.bodies),
      None => self.colliders.insert(builder),
    };
    let body = body.map(|(body, _)| body);
    self.collider_entries.insert(
      entity,
      ColliderEntry {
        handle,
        body,
        collider: collider.clone(),
        pose: (placement.translation, placement.rotation),
        map: placement.map,
      },
    );
    self.owners.insert(handle, Owner { collider: entity, body: body.unwrap_or(entity) });
  }

  /// Bring a collider in line with its component, its body and its
  /// placement: register it, move it to another body (or none), rebuild its
  /// shape when the shape or scale changed, move it, and apply material
  /// edits. A collider whose body is not simulated yet waits (retried every
  /// step); a shape that becomes degenerate leaves the simulation until it is
  /// valid again.
  pub(crate) fn sync_collider(
    &mut self,
    entity: Entity,
    body: Option<Entity>,
    placement: Placement,
    collider: &Collider,
  ) {
    let parent = match body {
      Some(body) => match self.body_entries.get(&body) {
        Some(entry) => Some((body, entry.handle)),
        None => {
          self.waiting.insert(entity);
          return;
        }
      },
      None => None,
    };
    self.waiting.remove(&entity);
    let Some(entry) = self.collider_entries.get(&entity) else {
      self.add_collider(entity, parent, placement, collider);
      return;
    };
    let moved = poses_differ(entry.pose, (placement.translation, placement.rotation));
    let reshape = entry.collider.shape != collider.shape || maps_differ(entry.map, placement.map);
    let reparent = entry.body != body;
    if !moved && !reshape && !reparent && entry.collider == *collider {
      return;
    }
    if !placement.is_finite() {
      self.warn_once(entity, "a collider with a non-finite transform is not moved");
      return;
    }
    let (handle, old) = (entry.handle, entry.collider.clone());
    if reshape {
      let Some(shape) = shape::build(&collider.shape, placement.map) else {
        self.remove_collider(entity);
        self.warn_once(entity, "a degenerate collider shape (zero scale included) is not simulated");
        return;
      };
      if let Some(rapier) = self.colliders.get_mut(handle) {
        rapier.set_shape(shape);
      }
    }
    if reparent {
      self.colliders.set_parent(handle, parent.map(|(_, h)| h), &mut self.bodies);
      if let Some(owner) = self.owners.get_mut(&handle) {
        owner.body = body.unwrap_or(entity);
      }
    }
    let density = self.rapier_density(finite_or(collider.density, 1.0).max(0.0));
    if let Some(rapier) = self.colliders.get_mut(handle) {
      if moved || reparent {
        if parent.is_some() {
          rapier.set_position_wrt_parent(placement.pose());
        } else {
          rapier.set_position(placement.pose());
        }
      }
      apply_material(rapier, &old, collider, density);
    }
    if let Some(entry) = self.collider_entries.get_mut(&entity) {
      entry.body = body;
      entry.collider = collider.clone();
      if moved || reparent {
        entry.pose = (placement.translation, placement.rotation);
      }
      if reshape {
        entry.map = placement.map;
      }
    }
  }

  /// A collider's world pose (for tests and the overlay).
  pub(crate) fn collider_pose(&self, entity: Entity) -> Option<(Vec2, f32)> {
    let collider = self.colliders.get(self.collider_entries.get(&entity)?.handle)?;
    Some((from_rapier(collider.translation()), collider.rotation().angle()))
  }

  pub(crate) fn has_body(&self, entity: Entity) -> bool {
    self.body_entries.contains_key(&entity)
  }
```

`apply_material` is the material half of Task 8's `update_collider` (friction, restitution, density or mass, groups, sensor, force events), moved into a free function `fn apply_material(rapier: &mut rapier2d::prelude::Collider, old: &Collider, new: &Collider, density: f32)`. Delete the "a collider already simulated on this entity joins its new body" block from `add_body` (`sync_colliders` handles it).

- [ ] **Step 4: `sync.rs`**

Imports add `Children, GlobalTransform` and `glam::Mat2`, `use super::world::Placement;` and `use crate::core::ecs::propagate::propagate_transforms;`. Replace `register_new` and `track_parents` (delete it) and the collider half of `track_disabled`:

```rust
/// How deep a collider may sit below its body.
const MAX_DEPTH: usize = 64;

/// The nearest entity at or above `entity` with a `RigidBody`.
fn owning_body(
  entity: Entity,
  parents: &Query<&ChildOf>,
  bodies: &Query<(), With<RigidBody>>,
) -> Option<Entity> {
  let mut current = entity;
  for _ in 0..MAX_DEPTH {
    if bodies.contains(current) {
      return Some(current);
    }
    current = parents.get(current).ok()?.parent();
  }
  None
}

/// `root` and everything below it.
fn subtree(root: Entity, children: &Query<&Children>, out: &mut Vec<Entity>) {
  let mut stack = vec![(root, 0)];
  while let Some((entity, depth)) = stack.pop() {
    out.push(entity);
    if depth < MAX_DEPTH
      && let Ok(kids) = children.get(entity)
    {
      stack.extend(kids.iter().map(|kid| (kid, depth + 1)));
    }
  }
}

/// A collider's placement: its frame is its entity's world transform, moved
/// by `offset` and turned by `rotation`; relative to `body` (that body's world
/// translation and rotation) when it has one.
pub(crate) fn placement(
  global: &GlobalTransform,
  collider: &Collider,
  body: Option<(Vec2, f32)>,
) -> Placement {
  let affine = global.affine();
  let origin = affine.transform_point2(collider.offset);
  let rotation = global.rotation() + collider.rotation;
  // Shape points: turned by the collider's rotation, mapped by the entity's
  // linear part, then turned back by the frame's rotation
  let map = Mat2::from_angle(-rotation) * affine.matrix2 * Mat2::from_angle(collider.rotation);
  match body {
    Some((translation, body_rotation)) => Placement {
      translation: Vec2::from_angle(-body_rotation).rotate(origin - translation),
      rotation: rotation - body_rotation,
      map,
    },
    None => Placement { translation: origin, rotation, map },
  }
}

type BodyItem<'a> = (Entity, &'a GlobalTransform, &'a Velocity, &'a AngularVelocity, &'a RigidBody);

fn add_body(physics: &mut Physics, (entity, global, velocity, angular, body): BodyItem) {
  physics.add_body(entity, global.translation(), global.rotation(), velocity.as_vec2(), angular.0, body);
}

/// Register new bodies at their world pose and apply edited ones in place.
pub(crate) fn register_bodies(
  mut physics: ResMut<Physics>,
  config: Res<PhysicsConfig>,
  bodies: Query<BodyItem, Added<RigidBody>>,
  edited: Query<(Entity, &RigidBody), Changed<RigidBody>>,
) {
  physics.configure(&config);
  for item in &bodies {
    add_body(&mut physics, item);
  }
  for (entity, body) in &edited {
    physics.update_body(entity, body);
  }
}

/// Bring every collider that changed, moved, scaled or changed owner in line
/// with the simulation. Owners change when a `RigidBody` appears or goes
/// above a collider, or when a collider or an ancestor is reparented.
#[allow(clippy::too_many_arguments)] // one query or removal reader per input
pub(crate) fn sync_colliders(
  mut physics: ResMut<Physics>,
  changed: Query<
    Entity,
    (With<Collider>, Or<(Changed<Collider>, Changed<GlobalTransform>, Changed<ChildOf>)>),
  >,
  new_bodies: Query<Entity, Added<RigidBody>>,
  reparented: Query<Entity, Changed<ChildOf>>,
  mut lost_bodies: RemovedComponents<RigidBody>,
  mut detached: RemovedComponents<ChildOf>,
  children: Query<&Children>,
  parents: Query<&ChildOf>,
  bodies: Query<(), With<RigidBody>>,
  colliders: Query<(&GlobalTransform, &Collider)>,
  globals: Query<&GlobalTransform>,
) {
  let mut todo: Vec<Entity> = changed.iter().collect();
  todo.extend(physics.waiting.drain());
  for root in new_bodies
    .iter()
    .chain(reparented.iter())
    .chain(lost_bodies.read())
    .chain(detached.read())
  {
    subtree(root, &children, &mut todo);
  }
  todo.sort_unstable();
  todo.dedup();
  for entity in todo {
    let Ok((global, collider)) = colliders.get(entity) else {
      continue;
    };
    let body = owning_body(entity, &parents, &bodies);
    let body_pose = body
      .and_then(|body| globals.get(body).ok())
      .map(|g| (g.translation(), g.rotation()));
    physics.sync_collider(entity, body, placement(global, collider, body_pose), collider);
  }
}
```

`track_disabled`: its bodies query becomes `Query<BodyItem>` (no `Without<ChildOf>`); for a re-enabled entity with a `Collider` it calls `physics.waiting.insert(entity)` instead of registering (the next `sync_colliders` places it).

`push_changes`: the moved query becomes bodies only, by world pose:

```rust
  moved: Query<(Entity, &GlobalTransform), (With<RigidBody>, Changed<GlobalTransform>)>,
```

with `physics.move_body(entity, global.translation(), global.rotation())` after the finiteness check (fixed colliders without a body are moved by `sync_colliders`).

`read_back` gains the parent:

```rust
pub(crate) fn read_back(
  physics: Res<Physics>,
  parents: Query<&ChildOf>,
  globals: Query<&GlobalTransform>,
  mut bodies: Query<(&mut Transform, &mut Velocity, &mut AngularVelocity), With<RigidBody>>,
) {
  for state in physics.moved_bodies() {
    // Into the parent's space: a parent that is a body itself moved this
    // step too, so its new pose is used (with its scale)
    let parent = parents.get(state.entity).ok().map(|child_of| {
      let parent = child_of.parent();
      let global = globals.get(parent).copied().unwrap_or_default();
      match physics.body_pose(parent) {
        Some((t, r)) => glam::Affine2::from_scale_angle_translation(global.scale(), r, t),
        None => global.affine(),
      }
    });
    let (translation, rotation) = match parent {
      Some(parent) => (
        parent.inverse().transform_point2(state.translation),
        state.rotation - parent.matrix2.x_axis.y.atan2(parent.matrix2.x_axis.x),
      ),
      None => (state.translation, state.rotation),
    };
    let Ok((mut transform, mut velocity, mut angular)) = bodies.get_mut(state.entity) else {
      continue;
    };
    if transform.translation != translation {
      transform.translation = translation;
    }
    let turn = angle_delta(transform.rotation, rotation);
    if turn != 0.0 {
      transform.rotation += turn;
    }
    if state.dynamic {
      velocity.set_if_neq(Velocity::from_vec2(state.linvel));
      angular.set_if_neq(AngularVelocity(state.angvel));
    }
  }
}
```

The chain:

```rust
    (
      forget_removed,
      track_disabled,
      propagate_transforms,
      register_bodies,
      sync_colliders,
      push_changes,
      step_simulation,
      read_back,
      emit_messages,
    )
```

`src/core/schedule.rs`: `FixedSet::Physics`'s doc becomes "Propagate, register, sync, step, write back".

- [ ] **Step 5: Run the tests**

Run: `cargo test -p ryo2d --lib physics:: && cargo test --workspace`
Expected: PASS. If `disabling_a_body_disables_its_child_colliders` fails because Rapier keeps a disabled body's colliders active, make `set_enabled` also walk the body's colliders (`body.colliders()`) and disable each, and ledger it.

- [ ] **Step 6: Gate and commit**

```bash
git add -A src
git commit -m "feat: bodies and colliders anywhere in the hierarchy" -m "- colliders on descendants attach to the nearest body above them" -m "- bodies under a parent write their local Transform back" -m "- collider shapes follow world scale, mirroring and rotation" -m "- a zero scale takes a collider out until it grows"
```

---
### Task 10: Collision messages with contact data, and contact forces

**Files:**
- Create: `src/core/physics/events.rs`
- Modify: `src/core/messages/builtin.rs` (remove `CollisionStarted`/`CollisionStopped`), `src/core/messages/mod.rs` (register the physics messages), `src/core/physics/world.rs` (capture contacts and forces), `src/core/physics/sync.rs` (`emit_messages`), `src/core/physics/mod.rs`, `src/lib.rs`
- Modify: `examples/games/breakout/src/scenes/game.rs`, `examples/games/flappy/src/scenes/{game,debug}.rs`, `tests/e2e/headless_engine.rs`, `src/core/engine/frame.rs` (tests) and any `entity_a`/`entity_b` reader (grep)

**Interfaces:**
- Consumes: `Physics::{owners, departed, collisions}`, `EventCollector` (Task 5), `with_force_events` (Task 8).
- Produces:
  - `pub struct Contact { pub point: Vec2, pub normal: Vec2 }`
  - `CollisionStarted { pub a, pub b, pub body_a, pub body_b: Entity, pub trigger: bool, pub contact: Option<Contact> }`, `CollisionStopped { a, b, body_a, body_b, trigger }`, `ContactForce { a, b, body_a, body_b, force: f32, direction: Vec2, started: bool }`, `JointBroken { pub joint: Entity, pub a: Entity, pub b: Entity }` (registered now, sent from Task 13); each pair message has `involves(e) -> bool`, `other(e) -> Option<Entity>` (the other body), `other_collider(e) -> Option<Entity>`
  - crate-internal `world::first_contact(colliders: &ColliderSet, pair: &ContactPair, from: ColliderHandle) -> Option<(Vec2, Vec2)>` (deepest point, normal pointing away from `from`; Task 12 reuses it)
  - `RawCollision` gains `contact: Option<(Vec2, Vec2)>`; `Physics::forces: Vec<RawForce>` with `RawForce { a: Owner, b: Owner, force: f32, direction: Vec2, started: bool }`

- [ ] **Step 1: Write the failing tests (in `sync.rs`'s tests)**

```rust
  use crate::core::physics::{CollisionStopped, ContactForce};

  fn read<M: crate::core::ecs::Message + Copy>(world: &World) -> Vec<M> {
    let messages = world.resource::<Messages<M>>();
    messages.get_cursor().read(messages).copied().collect()
  }

  #[test]
  fn started_carries_the_first_contact_from_a_to_b() {
    let (mut world, mut s) = world(60);
    let f = floor(&mut world, 100.0);
    let b = ball(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, 60);
    let started = read::<CollisionStarted>(&world);
    let hit = started.iter().find(|m| m.involves(b) && m.involves(f)).expect("ball hit floor");
    let contact = hit.contact.expect("a solid contact has a point");
    let down = if hit.a == b { Vec2::Y } else { -Vec2::Y };
    assert!((contact.normal - down).length() < 1e-3, "{contact:?}");
    assert!((contact.point.y - 100.0).abs() < 1.5, "{contact:?}");
    assert!(!hit.trigger);
  }

  #[test]
  fn a_trigger_reports_no_contact_point() {
    let (mut world, mut s) = world(60);
    let zone = world
      .spawn((Transform::from_xy(0.0, 100.0), Collider::rect(100.0, 40.0).as_trigger()))
      .id();
    let b = ball(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, 60);
    let started = read::<CollisionStarted>(&world);
    let hit = started.iter().find(|m| m.involves(zone)).expect("entered the zone");
    assert!(hit.trigger && hit.contact.is_none());
    assert_eq!(hit.other(zone), Some(b));
  }

  #[test]
  fn messages_name_the_owning_body() {
    use crate::core::ecs::ChildOf;
    let (mut world, mut s) = world(60);
    let f = floor(&mut world, 100.0);
    let body = world.spawn((Transform::IDENTITY, RigidBody::dynamic())).id();
    let part = world.spawn((Transform::IDENTITY, Collider::circle(10.0), ChildOf(body))).id();
    run(&mut world, &mut s, 60);
    let started = read::<CollisionStarted>(&world);
    let hit = started.iter().find(|m| m.involves(f)).expect("landed");
    assert!(hit.involves(body) && hit.involves(part));
    assert_eq!(hit.other(f), Some(body));
    assert_eq!(hit.other_collider(f), Some(part));
    assert_eq!(hit.other(body), Some(f));
  }

  #[test]
  fn contact_force_fires_above_its_threshold_only() {
    let forces = |threshold: f32| {
      let (mut world, mut s) = world(60);
      floor(&mut world, 100.0);
      world.spawn((
        Transform::from_xy(0.0, 60.0),
        RigidBody::dynamic().lock_rotation(),
        Collider::rect(20.0, 20.0).with_mass(10.0).with_force_events(threshold),
      ));
      run(&mut world, &mut s, 30);
      read::<ContactForce>(&world)
    };
    let felt = forces(50.0);
    assert!(!felt.is_empty() && felt.iter().all(|m| m.force > 50.0), "{felt:?}");
    assert!(forces(1e9).is_empty());
  }

  #[test]
  fn a_collider_despawned_while_touching_sends_collision_stopped() {
    let (mut world, mut s) = world(60);
    floor(&mut world, 100.0);
    let b = ball(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, 120);
    world.despawn(b);
    run(&mut world, &mut s, 1);
    let stopped = read::<CollisionStopped>(&world);
    assert!(stopped.iter().any(|m| m.involves(b)), "{stopped:?}");
  }
```

Update `touching_bodies_write_collision_messages` and the knockback/frame tests to the new fields if they name `entity_a`/`entity_b`.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p ryo2d --lib physics::sync`
Expected: compile errors (`contact`, `ContactForce`, `other_collider`).

- [ ] **Step 3: Write `events.rs`**

```rust
//! What the simulation reports, sent after each step: read them with
//! `ctx.messages::<T>()` (`MessageReader<T>` in systems). A collider with no
//! body is its own body. On `CollisionStopped` either entity may already be
//! despawned. The order of `a` and `b` is unspecified.

use crate::core::ecs::{Entity, Message, Vec2};

/// Where two colliders first touched: a world point and the contact normal,
/// pointing from `a` to `b`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Contact {
  pub point: Vec2,
  pub normal: Vec2,
}

/// Two colliders started touching, or overlapping for a trigger.
#[derive(Message, Debug, Clone, Copy, PartialEq)]
pub struct CollisionStarted {
  pub a: Entity,
  pub b: Entity,
  pub body_a: Entity,
  pub body_b: Entity,
  /// Either side is a trigger
  pub trigger: bool,
  /// The first contact; `None` for triggers
  pub contact: Option<Contact>,
}

/// Two colliders stopped touching (or one of them went away).
#[derive(Message, Debug, Clone, Copy, PartialEq)]
pub struct CollisionStopped {
  pub a: Entity,
  pub b: Entity,
  pub body_a: Entity,
  pub body_b: Entity,
  pub trigger: bool,
}

/// A contact pushing harder than a collider's `with_force_events` threshold,
/// sent every step it does: breakables, impact sounds.
#[derive(Message, Debug, Clone, Copy, PartialEq)]
pub struct ContactForce {
  pub a: Entity,
  pub b: Entity,
  pub body_a: Entity,
  pub body_b: Entity,
  /// The total contact force (mass·px/s²)
  pub force: f32,
  /// The direction of the strongest contact force
  pub direction: Vec2,
  /// True on the first step above the threshold
  pub started: bool,
}

/// A joint pulled harder than its `with_break_force` and was removed (its
/// `Joint` component too).
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct JointBroken {
  pub joint: Entity,
  pub a: Entity,
  pub b: Entity,
}

macro_rules! pair_helpers {
  ($($message:ty),*) => {$(
    impl $message {
      /// Whether `entity` is either side's collider or body.
      pub fn involves(&self, entity: Entity) -> bool {
        [self.a, self.b, self.body_a, self.body_b].contains(&entity)
      }

      /// The other side's body, given one side's collider or body.
      pub fn other(&self, entity: Entity) -> Option<Entity> {
        if entity == self.a || entity == self.body_a {
          Some(self.body_b)
        } else if entity == self.b || entity == self.body_b {
          Some(self.body_a)
        } else {
          None
        }
      }

      /// The other side's collider, given one side's collider or body.
      pub fn other_collider(&self, entity: Entity) -> Option<Entity> {
        if entity == self.a || entity == self.body_a {
          Some(self.b)
        } else if entity == self.b || entity == self.body_b {
          Some(self.a)
        } else {
          None
        }
      }
    }
  )*};
}

pair_helpers!(CollisionStarted, CollisionStopped, ContactForce);
```

`mod.rs`: `mod events; pub use events::{CollisionStarted, CollisionStopped, Contact, ContactForce, JointBroken};`. `messages/mod.rs`: register `CollisionStarted`, `CollisionStopped`, `ContactForce`, `JointBroken` from `crate::core::physics` (and its test asserting the resource exists). Delete them from `builtin.rs`. Prelude: drop `CollisionStarted, CollisionStopped` from the messages block; the physics block adds `CollisionStarted, CollisionStopped, Contact, ContactForce, JointBroken`.

- [ ] **Step 4: Capture contacts and forces in `world.rs`**

Imports add `ContactForceEvent`. The collector keeps the pair's first contact while Rapier still has it:

```rust
/// A collision event with its first contact, captured during the step.
struct Captured {
  event: CollisionEvent,
  contact: Option<(Vec2, Vec2)>,
}

#[derive(Default)]
struct EventCollector {
  collisions: Mutex<Vec<Captured>>,
  forces: Mutex<Vec<ContactForceEvent>>,
}

impl EventHandler for EventCollector {
  fn handle_collision_event(
    &self,
    _bodies: &RigidBodySet,
    colliders: &ColliderSet,
    event: CollisionEvent,
    contact_pair: Option<&ContactPair>,
  ) {
    let contact = contact_pair
      .filter(|_| event.started() && !event.sensor())
      .and_then(|pair| first_contact(colliders, pair, event.collider1()));
    self
      .collisions
      .lock()
      .unwrap_or_else(PoisonError::into_inner)
      .push(Captured { event, contact });
  }

  fn handle_contact_force_event(
    &self,
    dt: Real,
    _bodies: &RigidBodySet,
    _colliders: &ColliderSet,
    contact_pair: &ContactPair,
    total_force_magnitude: Real,
  ) {
    self
      .forces
      .lock()
      .unwrap_or_else(PoisonError::into_inner)
      .push(ContactForceEvent::from_contact_pair(dt, contact_pair, total_force_magnitude));
  }

  fn handle_soft_body_tear_event(&self, _soft_bodies: &SoftBodySet, _event: &SoftBodyTearEvent) {}
}

/// The deepest contact of `pair` as a world point, with the contact normal
/// pointing away from collider `from`. Rapier picks which collider of a pair
/// is first; its manifold normal points from `collider1` to `collider2`.
pub(crate) fn first_contact(
  colliders: &ColliderSet,
  pair: &ContactPair,
  from: ColliderHandle,
) -> Option<(Vec2, Vec2)> {
  let (manifold, contact) = pair.find_deepest_contact()?;
  let collider1 = colliders.get(pair.collider1)?;
  let point = from_rapier(*collider1.position() * contact.local_p1);
  let normal = from_rapier(manifold.data.normal);
  Some((point, if pair.collider1 == from { normal } else { -normal }))
}

pub(crate) struct RawForce {
  pub(crate) a: Owner,
  pub(crate) b: Owner,
  pub(crate) force: f32,
  pub(crate) direction: Vec2,
  pub(crate) started: bool,
}
```

`RawCollision` gains `pub(crate) contact: Option<(Vec2, Vec2)>`; `Physics` gains `pub(crate) forces: Vec<RawForce>`. In `step`, resolve both queues (the lookup closure is the same for both: `owners`, then `departed`):

```rust
    self.collisions.clear();
    self.forces.clear();
    let collisions = self.collector.collisions.get_mut().unwrap_or_else(PoisonError::into_inner);
    for Captured { event, contact } in collisions.drain(..) {
      let owner = |handle| self.owners.get(&handle).or_else(|| self.departed.get(&handle)).copied();
      if let (Some(a), Some(b)) = (owner(event.collider1()), owner(event.collider2())) {
        self.collisions.push(RawCollision { a, b, started: event.started(), sensor: event.sensor(), contact });
      }
    }
    let forces = self.collector.forces.get_mut().unwrap_or_else(PoisonError::into_inner);
    for event in forces.drain(..) {
      let owner = |handle| self.owners.get(&handle).copied();
      if let (Some(a), Some(b)) = (owner(event.collider1), owner(event.collider2)) {
        self.forces.push(RawForce {
          a,
          b,
          force: event.total_force_magnitude,
          direction: from_rapier(event.max_force_direction),
          started: event.started,
        });
      }
    }
    self.departed.clear();
```

(Split the borrows as in Task 5 if the closures capture `self` whole.)

- [ ] **Step 5: Emit them (`sync.rs`)**

```rust
pub(crate) fn emit_messages(
  physics: Res<Physics>,
  mut started: MessageWriter<CollisionStarted>,
  mut stopped: MessageWriter<CollisionStopped>,
  mut forces: MessageWriter<ContactForce>,
) {
  for c in &physics.collisions {
    let (a, b, body_a, body_b) = (c.a.collider, c.b.collider, c.a.body, c.b.body);
    if c.started {
      started.write(CollisionStarted {
        a,
        b,
        body_a,
        body_b,
        trigger: c.sensor,
        contact: c.contact.map(|(point, normal)| Contact { point, normal }),
      });
    } else {
      stopped.write(CollisionStopped { a, b, body_a, body_b, trigger: c.sensor });
    }
  }
  for f in &physics.forces {
    forces.write(ContactForce {
      a: f.a.collider,
      b: f.b.collider,
      body_a: f.a.body,
      body_b: f.b.body,
      force: f.force,
      direction: f.direction,
      started: f.started,
    });
  }
}
```

- [ ] **Step 6: Port the readers**

`grep -rn "entity_a\|entity_b\|CollisionStarted\|CollisionStopped" examples tests src --include=*.rs`: `entity_a`/`entity_b` become `a`/`b` (or `body_a`/`body_b` where the game wants the body); `involves`/`other` keep working. Imports of the messages move from `crate::core::messages` to `crate::core::physics` inside the engine.

- [ ] **Step 7: Run the tests**

Run: `cargo test -p ryo2d --lib && cargo test --workspace`
Expected: PASS.

- [ ] **Step 8: Gate and commit**

```bash
git add -A src tests examples
git commit -m "feat: collision messages carry contact data" -m "- CollisionStarted/Stopped name colliders and bodies, triggers, and the first contact point and normal" -m "- ContactForce for colliders opted in with with_force_events" -m "- JointBroken registered for the joints to come"
```

---

### Task 11: Scene queries

**Files:**
- Modify: `src/core/physics/query.rs`, `src/core/physics/mod.rs`, `src/lib.rs`

**Interfaces:**
- Consumes: `Physics::{broad_phase, narrow_phase, bodies, colliders, owners}`, `shape::build` (Task 8), `convert::{pose, to_rapier, from_rapier}`.
- Produces:
  - `pub struct QueryFilter` (`Default`: every layer, solids only) with `new()`, `with_mask(u32)`, `excluding(Entity)`, `with_triggers()`
  - `pub struct Hit { pub collider, pub body: Entity, pub point: Vec2, pub normal: Vec2, pub distance: f32 }`, `pub struct Overlap { pub collider, pub body: Entity }`, `pub struct PointHit { pub collider, pub body: Entity, pub point: Vec2, pub inside: bool }`
  - on `Physics`: `raycast(origin, direction, max_distance, &QueryFilter) -> Option<Hit>`, `raycast_all(..) -> Vec<Hit>`, `shape_cast(&ColliderShape, origin, rotation, direction, max_distance, &QueryFilter) -> Option<Hit>`, `overlap_point(point, &QueryFilter) -> Vec<Overlap>`, `overlap_shape(&ColliderShape, position, rotation, &QueryFilter) -> Vec<Overlap>`, `nearest(point, max_distance, &QueryFilter) -> Option<PointHit>`

- [ ] **Step 1: Write the failing tests (in `query.rs`'s tests)**

```rust
  use crate::core::physics::{ColliderShape, Hit, Overlap, QueryFilter};

  /// A floor whose top is at y 200, a fixed 20 px box at (100, 100) on layer 2,
  /// and a trigger zone 40 px wide at (0, 100); one step so queries see them.
  fn scene() -> (crate::core::ecs::World, [crate::core::ecs::Entity; 3]) {
    let (mut world, mut s) = world(60);
    let floor_e = floor(&mut world, 200.0);
    let block = world
      .spawn((Transform::from_xy(100.0, 100.0), RigidBody::fixed(), Collider::rect(20.0, 20.0).with_layer(0b10)))
      .id();
    let zone = world
      .spawn((Transform::from_xy(0.0, 100.0), Collider::rect(40.0, 40.0).as_trigger()))
      .id();
    run(&mut world, &mut s, 1);
    (world, [floor_e, block, zone])
  }

  fn close(a: Vec2, b: Vec2) -> bool {
    (a - b).length() < 1e-2
  }

  #[test]
  fn a_ray_hits_the_first_solid_and_skips_triggers() {
    let (world, [floor_e, _, zone]) = scene();
    let physics = world.resource::<Physics>();
    let hit = physics.raycast(Vec2::ZERO, Vec2::Y, 1000.0, &QueryFilter::new()).unwrap();
    assert_eq!((hit.collider, hit.body), (floor_e, floor_e));
    assert!(close(hit.point, Vec2::new(0.0, 200.0)) && close(hit.normal, -Vec2::Y), "{hit:?}");
    assert!((hit.distance - 200.0).abs() < 1e-2);
    let with = physics.raycast(Vec2::ZERO, Vec2::Y * 5.0, 1000.0, &QueryFilter::new().with_triggers()).unwrap();
    assert_eq!(with.collider, zone);
    assert!((with.distance - 80.0).abs() < 1e-2, "the direction is normalised: {with:?}");
    assert!(physics.raycast(Vec2::ZERO, Vec2::Y, 150.0, &QueryFilter::new()).is_none(), "out of range");
  }

  #[test]
  fn degenerate_rays_hit_nothing() {
    let (world, _) = scene();
    let physics = world.resource::<Physics>();
    assert!(physics.raycast(Vec2::ZERO, Vec2::ZERO, 1000.0, &QueryFilter::new()).is_none());
    assert!(physics.raycast(Vec2::ZERO, Vec2::new(f32::NAN, 1.0), 1000.0, &QueryFilter::new()).is_none());
    assert!(physics.raycast(Vec2::new(f32::INFINITY, 0.0), Vec2::Y, 1000.0, &QueryFilter::new()).is_none());
    assert!(physics.raycast_all(Vec2::ZERO, Vec2::ZERO, 1000.0, &QueryFilter::new()).is_empty());
  }

  #[test]
  fn raycast_all_is_nearest_first_and_filters_apply() {
    let (world, [floor_e, block, _]) = scene();
    let physics = world.resource::<Physics>();
    let all: Vec<_> = physics
      .raycast_all(Vec2::new(100.0, 0.0), Vec2::Y, 1000.0, &QueryFilter::new())
      .iter()
      .map(|hit: &Hit| hit.collider)
      .collect();
    assert_eq!(all, [block, floor_e]);
    let floor_only = QueryFilter::new().with_mask(0b1);
    assert_eq!(physics.raycast(Vec2::new(100.0, 0.0), Vec2::Y, 1000.0, &floor_only).unwrap().collider, floor_e);
    let skipping = QueryFilter::new().excluding(block);
    assert_eq!(physics.raycast(Vec2::new(100.0, 0.0), Vec2::Y, 1000.0, &skipping).unwrap().collider, floor_e);
  }

  #[test]
  fn excluding_a_body_skips_all_its_colliders() {
    use crate::core::ecs::ChildOf;
    let (mut world, mut s) = world(60);
    floor(&mut world, 200.0);
    let body = world.spawn((Transform::from_xy(0.0, 100.0), RigidBody::fixed())).id();
    for y in [-20.0, 20.0] {
      world.spawn((Transform::from_xy(0.0, y), Collider::rect(20.0, 10.0), ChildOf(body)));
    }
    run(&mut world, &mut s, 1);
    let physics = world.resource::<Physics>();
    assert_eq!(physics.raycast(Vec2::ZERO, Vec2::Y, 1000.0, &QueryFilter::new()).unwrap().body, body);
    let hit = physics.raycast(Vec2::ZERO, Vec2::Y, 1000.0, &QueryFilter::new().excluding(body)).unwrap();
    assert_ne!(hit.body, body);
  }

  #[test]
  fn a_shape_cast_stops_where_the_shape_touches() {
    let (world, [floor_e, ..]) = scene();
    let physics = world.resource::<Physics>();
    let circle = ColliderShape::Circle { radius: 10.0 };
    let hit = physics.shape_cast(&circle, Vec2::new(-100.0, 0.0), 0.0, Vec2::Y, 1000.0, &QueryFilter::new()).unwrap();
    assert_eq!(hit.collider, floor_e);
    assert!((hit.distance - 190.0).abs() < 0.05, "{hit:?}");
    assert!(close(hit.normal, -Vec2::Y), "{hit:?}");
    let inside = physics.shape_cast(&circle, Vec2::new(-100.0, 205.0), 0.0, Vec2::Y, 1000.0, &QueryFilter::new()).unwrap();
    assert_eq!(inside.distance, 0.0);
  }

  #[test]
  fn overlaps_and_the_nearest_point() {
    let (world, [floor_e, block, _]) = scene();
    let physics = world.resource::<Physics>();
    let at_floor = physics.overlap_point(Vec2::new(0.0, 205.0), &QueryFilter::new());
    assert_eq!(at_floor, [Overlap { collider: floor_e, body: floor_e }]);
    let area = ColliderShape::Rect { size: Vec2::splat(300.0) };
    let mut found: Vec<_> = physics
      .overlap_shape(&area, Vec2::new(50.0, 150.0), 0.0, &QueryFilter::new())
      .into_iter()
      .map(|o| o.collider)
      .collect();
    found.sort();
    let mut expected = vec![floor_e, block];
    expected.sort();
    assert_eq!(found, expected, "the trigger only with with_triggers");
    assert_eq!(physics.overlap_shape(&area, Vec2::new(50.0, 150.0), 0.0, &QueryFilter::new().with_triggers()).len(), 3);
    let near = physics.nearest(Vec2::new(-150.0, 150.0), 100.0, &QueryFilter::new()).unwrap();
    assert_eq!(near.collider, floor_e);
    assert!(close(near.point, Vec2::new(-150.0, 200.0)) && !near.inside, "{near:?}");
    assert!(physics.nearest(Vec2::new(-150.0, 0.0), 50.0, &QueryFilter::new()).is_none());
  }

  #[test]
  fn queries_see_the_world_as_the_last_step_left_it() {
    let (mut world, mut s) = world(60);
    floor(&mut world, 200.0);
    let probe = |world: &crate::core::ecs::World| {
      world.resource::<Physics>().raycast(Vec2::ZERO, Vec2::Y, 1000.0, &QueryFilter::new())
    };
    assert!(probe(&world).is_none(), "not stepped yet");
    run(&mut world, &mut s, 1);
    assert!(probe(&world).is_some());
  }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p ryo2d --lib physics::query`
Expected: compile errors (`QueryFilter`, `raycast`).

- [ ] **Step 3: Implement (in `query.rs`)**

```rust
use glam::Mat2;
use rapier2d::parry::query::{QueryDispatcher, ShapeCastOptions};
use rapier2d::prelude::{
  Collider as RapierCollider, ColliderHandle, QueryFilter as RapierFilter, QueryPipeline, Ray,
};

use super::collider::ColliderShape;
use super::convert::{from_rapier, pose, to_rapier};
use super::shape;
use super::world::Owner;

/// What a query may hit. The default hits every layer and skips triggers
/// (so a line of sight is not blocked by a trigger zone).
#[derive(Debug, Clone, PartialEq)]
pub struct QueryFilter {
  mask: u32,
  excluded: Vec<Entity>,
  triggers: bool,
}

impl Default for QueryFilter {
  fn default() -> Self {
    Self { mask: u32::MAX, excluded: Vec::new(), triggers: false }
  }
}

impl QueryFilter {
  pub fn new() -> Self {
    Self::default()
  }

  /// Hit only colliders whose layer meets `mask` (whatever their own mask).
  pub fn with_mask(mut self, mask: u32) -> Self {
    self.mask = mask;
    self
  }

  /// Skip a body (all its colliders) or one collider. Call it again to skip more.
  pub fn excluding(mut self, entity: Entity) -> Self {
    self.excluded.push(entity);
    self
  }

  /// Hit triggers too.
  pub fn with_triggers(mut self) -> Self {
    self.triggers = true;
    self
  }
}

/// A ray or shape cast's first contact: the world point, the surface normal
/// there and how far along the cast it is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hit {
  pub collider: Entity,
  pub body: Entity,
  pub point: Vec2,
  pub normal: Vec2,
  pub distance: f32,
}

/// A collider found by an overlap test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Overlap {
  pub collider: Entity,
  pub body: Entity,
}

/// The closest point on a collider's surface, and whether the query point
/// was inside it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointHit {
  pub collider: Entity,
  pub body: Entity,
  pub point: Vec2,
  pub inside: bool,
}

/// A normalised ray, or `None` for a zero or non-finite one or a negative or NaN range.
fn ray(origin: Vec2, direction: Vec2, max_distance: f32) -> Option<Ray> {
  let direction = direction.normalize_or_zero();
  (origin.is_finite() && direction != Vec2::ZERO && max_distance >= 0.0)
    .then(|| Ray::new(to_rapier(origin), to_rapier(direction)))
}

impl Physics {
  fn owner_of(&self, handle: ColliderHandle) -> Option<Owner> {
    self.owners.get(&handle).copied()
  }

  /// Run `f` with Rapier's queries, filtered by `filter`.
  fn with_queries<R>(&self, filter: &QueryFilter, f: impl FnOnce(QueryPipeline<'_>) -> R) -> R {
    let passes = |handle: ColliderHandle, collider: &RapierCollider| {
      u32::from(collider.collision_groups().memberships) & filter.mask != 0
        && self.owners.get(&handle).is_some_and(|owner| {
          !filter.excluded.contains(&owner.collider) && !filter.excluded.contains(&owner.body)
        })
    };
    let mut rapier_filter = RapierFilter::default().predicate(&passes);
    if !filter.triggers {
      rapier_filter = rapier_filter.exclude_sensors();
    }
    let dispatcher: &dyn QueryDispatcher = self.narrow_phase.query_dispatcher();
    f(self.broad_phase.as_query_pipeline(dispatcher, &self.bodies, &self.colliders, rapier_filter))
  }

  /// The first collider along a ray from `origin` (the direction need not be
  /// normalised), within `max_distance` px. Queries see the world as the
  /// last step left it.
  pub fn raycast(&self, origin: Vec2, direction: Vec2, max_distance: f32, filter: &QueryFilter) -> Option<Hit> {
    let ray = ray(origin, direction, max_distance)?;
    self.with_queries(filter, |queries| {
      let (handle, hit) = queries.cast_ray_and_get_normal(&ray, max_distance, true)?;
      let owner = self.owner_of(handle)?;
      Some(Hit {
        collider: owner.collider,
        body: owner.body,
        point: from_rapier(ray.point_at(hit.time_of_impact)),
        normal: from_rapier(hit.normal),
        distance: hit.time_of_impact,
      })
    })
  }

  /// Every collider along the ray, nearest first.
  pub fn raycast_all(&self, origin: Vec2, direction: Vec2, max_distance: f32, filter: &QueryFilter) -> Vec<Hit> {
    let Some(ray) = ray(origin, direction, max_distance) else {
      return Vec::new();
    };
    let mut hits: Vec<Hit> = self.with_queries(filter, |queries| {
      queries
        .intersect_ray(ray, max_distance, true)
        .filter_map(|(handle, _, hit)| {
          let owner = self.owner_of(handle)?;
          Some(Hit {
            collider: owner.collider,
            body: owner.body,
            point: from_rapier(ray.point_at(hit.time_of_impact)),
            normal: from_rapier(hit.normal),
            distance: hit.time_of_impact,
          })
        })
        .collect()
    });
    hits.sort_by(|a, b| a.distance.total_cmp(&b.distance));
    hits
  }

  /// Sweep `shape` (at `origin`, turned by `rotation`) along `direction`:
  /// the first collider it would touch. A shape that starts overlapping
  /// something hits it at distance 0.
  pub fn shape_cast(
    &self,
    shape: &ColliderShape,
    origin: Vec2,
    rotation: f32,
    direction: Vec2,
    max_distance: f32,
    filter: &QueryFilter,
  ) -> Option<Hit> {
    let direction = direction.normalize_or_zero();
    if direction == Vec2::ZERO || !origin.is_finite() || !rotation.is_finite() || !(max_distance >= 0.0) {
      return None;
    }
    let shape = shape::build(shape, Mat2::IDENTITY)?;
    let options = ShapeCastOptions {
      stop_at_penetration: true,
      compute_impact_geometry_on_penetration: true,
      ..ShapeCastOptions::with_max_time_of_impact(max_distance)
    };
    self.with_queries(filter, |queries| {
      let (handle, hit) = queries.cast_shape(&pose(origin, rotation), to_rapier(direction), &*shape, options)?;
      let owner = self.owner_of(handle)?;
      Some(Hit {
        collider: owner.collider,
        body: owner.body,
        point: from_rapier(hit.witness1),
        normal: from_rapier(hit.normal1),
        distance: hit.time_of_impact,
      })
    })
  }

  /// Every collider containing `point`.
  pub fn overlap_point(&self, point: Vec2, filter: &QueryFilter) -> Vec<Overlap> {
    if !point.is_finite() {
      return Vec::new();
    }
    self.with_queries(filter, |queries| {
      queries
        .intersect_point(to_rapier(point))
        .filter_map(|(handle, _)| self.owner_of(handle))
        .map(|owner| Overlap { collider: owner.collider, body: owner.body })
        .collect()
    })
  }

  /// Every collider overlapping `shape` at `position`, turned by `rotation`.
  pub fn overlap_shape(&self, shape: &ColliderShape, position: Vec2, rotation: f32, filter: &QueryFilter) -> Vec<Overlap> {
    if !position.is_finite() || !rotation.is_finite() {
      return Vec::new();
    }
    let Some(shape) = shape::build(shape, Mat2::IDENTITY) else {
      return Vec::new();
    };
    self.with_queries(filter, |queries| {
      queries
        .intersect_shape(pose(position, rotation), &*shape)
        .filter_map(|(handle, _)| self.owner_of(handle))
        .map(|owner| Overlap { collider: owner.collider, body: owner.body })
        .collect()
    })
  }

  /// The closest collider surface point to `point` within `max_distance`.
  pub fn nearest(&self, point: Vec2, max_distance: f32, filter: &QueryFilter) -> Option<PointHit> {
    if !point.is_finite() || !(max_distance >= 0.0) {
      return None;
    }
    self.with_queries(filter, |queries| {
      let (handle, projection) = queries.project_point(to_rapier(point), max_distance, true)?;
      let owner = self.owner_of(handle)?;
      Some(PointHit {
        collider: owner.collider,
        body: owner.body,
        point: from_rapier(projection.point),
        inside: projection.is_inside,
      })
    })
  }
}
```

With `solid = true` a point inside a shape projects onto itself: if `nearest_inside` style behaviour is wanted later, use `solid = false`. `mod.rs`: `pub use query::{Hit, Overlap, PointHit, QueryFilter};`; add them to the prelude.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ryo2d --lib physics::query`
Expected: PASS. If the shape cast reports `normal1` pointing into the floor, negate it and ledger the convention (normals on `Hit` point out of the surface that was hit).

- [ ] **Step 5: Gate and commit**

```bash
git add -A src
git commit -m "feat: raycasts, shape casts, overlap tests and nearest points" -m "- QueryFilter: layers, excluded bodies or colliders, triggers off by default"
```

---

### Task 12: Contact state

**Files:**
- Modify: `src/core/physics/query.rs`, `src/core/physics/world.rs` (remove `are_intersecting`/`get_current_intersections`), `src/core/physics/mod.rs`, `src/lib.rs`

**Interfaces:**
- Consumes: `world::first_contact` (Task 10), `Physics::{narrow_phase, body_entries, collider_entries, owners}`.
- Produces: `pub struct Touch { pub collider, pub body: Entity, pub point: Vec2, pub normal: Vec2, pub impulse: f32 }`; on `Physics`: `contacts(entity) -> Vec<Touch>`, `overlaps(entity) -> Vec<Overlap>`, `in_contact(a, b) -> bool` (each takes a body entity, meaning all its colliders, or a collider entity).

- [ ] **Step 1: Write the failing tests (in `query.rs`'s tests)**

```rust
  #[test]
  fn contacts_report_the_ground_below() {
    let (mut world, mut s) = world(60);
    let floor_e = floor(&mut world, 100.0);
    let crate_e = world
      .spawn((Transform::from_xy(0.0, 80.0), RigidBody::dynamic().lock_rotation(), Collider::rect(20.0, 20.0)))
      .id();
    run(&mut world, &mut s, 30);
    let physics = world.resource::<Physics>();
    let touches = physics.contacts(crate_e);
    assert_eq!(touches.len(), 1, "{touches:?}");
    let touch = touches[0];
    assert_eq!((touch.collider, touch.body), (floor_e, floor_e));
    assert!(close(touch.normal, Vec2::Y), "from the crate toward the floor: {touch:?}");
    assert!(touch.impulse > 0.0 && (touch.point.y - 100.0).abs() < 1.5);
    let from_floor = physics.contacts(floor_e);
    assert!(close(from_floor[0].normal, -Vec2::Y));
    assert!(physics.in_contact(crate_e, floor_e) && physics.in_contact(floor_e, crate_e));
  }

  #[test]
  fn overlaps_list_what_is_inside_a_trigger() {
    let (mut world, mut s) = world(60);
    no_gravity(&mut world);
    let zone = world
      .spawn((Transform::IDENTITY, Collider::rect(100.0, 100.0).as_trigger()))
      .id();
    let inside = ball(&mut world, 10.0, 10.0);
    ball(&mut world, 300.0, 0.0);
    run(&mut world, &mut s, 2);
    let physics = world.resource::<Physics>();
    assert_eq!(physics.overlaps(zone), [Overlap { collider: inside, body: inside }]);
    assert!(physics.in_contact(zone, inside));
    assert!(physics.contacts(zone).is_empty(), "triggers have no solid contacts");
  }

  #[test]
  fn contact_ends_when_bodies_part() {
    let (mut world, mut s) = world(60);
    let floor_e = floor(&mut world, 100.0);
    let b = ball(&mut world, 0.0, 85.0);
    run(&mut world, &mut s, 30);
    assert!(world.resource::<Physics>().in_contact(b, floor_e));
    world.get_mut::<Transform>(b).unwrap().translation.y = -500.0;
    run(&mut world, &mut s, 2);
    assert!(!world.resource::<Physics>().in_contact(b, floor_e));
  }

  #[test]
  fn a_body_s_contacts_include_every_collider() {
    use crate::core::ecs::ChildOf;
    let (mut world, mut s) = world(60);
    floor(&mut world, 100.0);
    let body = world.spawn((Transform::from_xy(0.0, 80.0), RigidBody::dynamic().lock_rotation())).id();
    for x in [-30.0, 30.0] {
      world.spawn((Transform::from_xy(x, 0.0), Collider::rect(20.0, 20.0), ChildOf(body)));
    }
    run(&mut world, &mut s, 30);
    assert_eq!(world.resource::<Physics>().contacts(body).len(), 2);
  }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p ryo2d --lib physics::query`
Expected: compile errors (`contacts`, `Touch`).

- [ ] **Step 3: Implement (in `query.rs`)**

```rust
use super::world::first_contact;

/// A solid contact touching one of an entity's colliders: the other side,
/// the deepest point, the normal from the entity toward the other side
/// (ground below is `(0, +1)`) and the step's normal impulse.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Touch {
  pub collider: Entity,
  pub body: Entity,
  pub point: Vec2,
  pub normal: Vec2,
  pub impulse: f32,
}

impl Physics {
  /// The colliders of `entity`: all of a body's, or one collider.
  fn handles_of(&self, entity: Entity) -> Vec<ColliderHandle> {
    if let Some(entry) = self.body_entries.get(&entity)
      && let Some(body) = self.bodies.get(entry.handle)
    {
      return body.colliders().to_vec();
    }
    self.collider_entries.get(&entity).map(|entry| vec![entry.handle]).unwrap_or_default()
  }

  /// The solid contacts touching `entity` (a body: all its colliders; or one
  /// collider) after the last step.
  pub fn contacts(&self, entity: Entity) -> Vec<Touch> {
    let mut touches = Vec::new();
    for handle in self.handles_of(entity) {
      for pair in self.narrow_phase.contact_pairs_with(handle) {
        if !pair.has_any_active_contact() {
          continue;
        }
        let other = if pair.collider1 == handle { pair.collider2 } else { pair.collider1 };
        let (Some(owner), Some((point, normal))) =
          (self.owner_of(other), first_contact(&self.colliders, pair, handle))
        else {
          continue;
        };
        touches.push(Touch {
          collider: owner.collider,
          body: owner.body,
          point,
          normal,
          impulse: pair.total_impulse_magnitude(),
        });
      }
    }
    touches
  }

  /// The colliders overlapping `entity`'s triggers, or the triggers `entity` is inside.
  pub fn overlaps(&self, entity: Entity) -> Vec<Overlap> {
    let mut overlaps = Vec::new();
    for handle in self.handles_of(entity) {
      for (a, b, intersecting) in self.narrow_phase.intersection_pairs_with(handle) {
        let other = if a == handle { b } else { a };
        if intersecting && let Some(owner) = self.owner_of(other) {
          overlaps.push(Overlap { collider: owner.collider, body: owner.body });
        }
      }
    }
    overlaps
  }

  /// Whether any collider of `a` touches or overlaps any collider of `b`.
  pub fn in_contact(&self, a: Entity, b: Entity) -> bool {
    let theirs = self.handles_of(b);
    self.handles_of(a).into_iter().any(|mine| {
      theirs.iter().any(|&other| {
        self.narrow_phase.contact_pair(mine, other).is_some_and(|pair| pair.has_any_active_contact())
          || self.narrow_phase.intersection_pair(mine, other) == Some(true)
      })
    })
  }
}
```

Delete `are_intersecting` and `get_current_intersections` from `world.rs`. `mod.rs` and the prelude export `Touch`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ryo2d --lib physics::`
Expected: PASS.

- [ ] **Step 5: Gate and commit**

```bash
git add -A src
git commit -m "feat: contacts, trigger overlaps and in_contact on Physics" -m "- replaces are_intersecting and get_current_intersections, which could not see triggers"
```

---
### Task 13: Joints

**Files:**
- Create: `src/core/physics/joint.rs`
- Modify: `src/core/physics/world.rs` (joint entries, insert, edit, inert, remove, break), `src/core/physics/sync.rs` (`sync_joints`; removals, disabling and `JointBroken`), `src/core/physics/mod.rs`, `src/lib.rs`

**Interfaces:**
- Consumes: `Physics::{body_entries, body_pose, remove_body, set_enabled, warn_once}`, `JointBroken` (Task 10), `convert::{pose, to_rapier, from_rapier}`.
- Produces:
  - `pub enum JointKind { Revolute, Prismatic { axis: Vec2 }, Fixed, Rope { max_length: f32 }, Spring { rest_length: f32, stiffness: f32, damping: f32 } }` (`#[non_exhaustive]`)
  - `pub enum JointAnchor { Origins, World(Vec2), Local { a: Vec2, b: Vec2 } }` (`Default` = `Origins`)
  - `pub enum JointMotor { Velocity { target: f32, max_force: f32 }, Position { target: f32, stiffness: f32, damping: f32 } }`
  - `pub struct Joint { pub a: Entity, pub b: Entity, pub kind: JointKind, pub anchor: JointAnchor, pub limits: Option<(f32, f32)>, pub motor: Option<JointMotor>, pub collisions: bool, pub break_force: Option<f32> }` (`#[non_exhaustive]`), with `Joint::{revolute, prismatic, fixed, rope, spring}`, `at(Vec2)`, `with_anchors(Vec2, Vec2)`, `with_limits(f32, f32)`, `with_motor_velocity(target, max_force)`, `with_motor_position(target, stiffness, damping)`, `with_collisions()`, `with_break_force(f32)`
  - crate-internal `joint::generic_joint(joint: &Joint, pose_a: (Vec2, f32), pose_b: (Vec2, f32)) -> Option<GenericJoint>`; `Physics::{sync_joint, remove_joint, retry_inert_joints, joint_count}`; `Physics::broken: Vec<(Entity, Entity, Entity)>` (joint, a, b) filled by `step`

- [ ] **Step 1: Write the failing tests (in `joint.rs`)**

```rust
#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ecs::{Messages, Transform, World};
  use crate::core::physics::test_util::*;
  use crate::core::physics::{AngularVelocity, Collider, JointBroken, Physics, RigidBody};
  use crate::core::ecs::Velocity;

  fn anchor(world: &mut World, at: Vec2) -> Entity {
    world.spawn((Transform::from_translation(at), RigidBody::fixed())).id()
  }

  fn bob(world: &mut World, at: Vec2) -> Entity {
    world
      .spawn((Transform::from_translation(at), RigidBody::dynamic(), Collider::circle(5.0)))
      .id()
  }

  fn distance(world: &World, a: Entity, b: Entity) -> f32 {
    at(world, a).translation.distance(at(world, b).translation)
  }

  #[test]
  fn a_pendulum_keeps_its_length_and_swings() {
    let (mut world, mut s) = world(60);
    let pivot = anchor(&mut world, Vec2::ZERO);
    let weight = bob(&mut world, Vec2::new(100.0, 0.0));
    world.spawn(Joint::revolute(pivot, weight).at(Vec2::ZERO));
    run(&mut world, &mut s, 60);
    assert!((distance(&world, pivot, weight) - 100.0).abs() < 1.0);
    assert!(at(&world, weight).translation.y > 50.0, "it swung down");
  }

  #[test]
  fn a_slider_moves_along_its_axis_within_its_limits() {
    let (mut world, mut s) = world(60);
    no_gravity(&mut world);
    let base = anchor(&mut world, Vec2::ZERO);
    let slider = world
      .spawn((Transform::IDENTITY, RigidBody::dynamic(), Velocity::new(200.0, 200.0), Collider::rect(10.0, 10.0)))
      .id();
    world.spawn(Joint::prismatic(base, slider, Vec2::X).with_limits(-50.0, 50.0));
    run(&mut world, &mut s, 60);
    let p = at(&world, slider).translation;
    assert!(p.y.abs() < 0.5 && (p.x - 50.0).abs() < 1.0, "{p}");
  }

  #[test]
  fn a_weld_keeps_the_relative_pose() {
    let (mut world, mut s) = world(60);
    let left = world.spawn((Transform::IDENTITY, RigidBody::dynamic(), Collider::rect(20.0, 20.0))).id();
    let right = world
      .spawn((Transform::from_xy(30.0, 0.0), RigidBody::dynamic(), Collider::rect(20.0, 20.0)))
      .id();
    world.spawn(Joint::fixed(left, right));
    run(&mut world, &mut s, 60);
    let offset = at(&world, right).translation - at(&world, left).translation;
    assert!((offset - Vec2::new(30.0, 0.0)).length() < 0.5, "{offset}");
    assert!(at(&world, left).translation.y > 100.0, "they fell together");
  }

  #[test]
  fn a_rope_stops_at_its_length() {
    let (mut world, mut s) = world(60);
    let pivot = anchor(&mut world, Vec2::ZERO);
    let weight = bob(&mut world, Vec2::new(0.0, 50.0));
    world.spawn(Joint::rope(pivot, weight, 100.0));
    run(&mut world, &mut s, 60);
    let d = distance(&world, pivot, weight);
    assert!((d - 100.0).abs() < 1.0, "{d}");
  }

  #[test]
  fn a_spring_settles_at_its_rest_length() {
    let (mut world, mut s) = world(60);
    no_gravity(&mut world);
    let pivot = anchor(&mut world, Vec2::ZERO);
    let weight = bob(&mut world, Vec2::new(200.0, 0.0));
    world.spawn(Joint::spring(pivot, weight, 100.0, 50.0, 5.0));
    run(&mut world, &mut s, 300);
    let d = distance(&world, pivot, weight);
    assert!((d - 100.0).abs() < 5.0, "{d}");
  }

  fn wheel(world: &mut World) -> (Entity, Entity) {
    no_gravity(world);
    let axle = anchor(world, Vec2::ZERO);
    let wheel = bob(world, Vec2::ZERO);
    let joint = world
      .spawn(Joint::revolute(axle, wheel).with_motor_velocity(5.0, 1e6))
      .id();
    (wheel, joint)
  }

  #[test]
  fn a_velocity_motor_turns_a_wheel_and_edits_apply_in_place() {
    let (mut world, mut s) = world(60);
    let (wheel, joint) = wheel(&mut world);
    run(&mut world, &mut s, 60);
    let spin = world.get::<AngularVelocity>(wheel).unwrap().0;
    assert!((spin - 5.0).abs() < 0.5, "{spin}");
    world.get_mut::<Joint>(joint).unwrap().motor = Some(JointMotor::Velocity { target: -5.0, max_force: 1e6 });
    run(&mut world, &mut s, 60);
    let spin = world.get::<AngularVelocity>(wheel).unwrap().0;
    assert!((spin + 5.0).abs() < 0.5, "{spin}");
    assert_eq!(world.resource::<Physics>().joint_count(), 1);
  }

  #[test]
  fn limits_hold_a_hinge() {
    let (mut world, mut s) = world(60);
    let pivot = anchor(&mut world, Vec2::ZERO);
    let arm = world
      .spawn((Transform::from_xy(50.0, 0.0), RigidBody::dynamic(), Collider::rect(100.0, 10.0)))
      .id();
    world.spawn(Joint::revolute(pivot, arm).at(Vec2::ZERO).with_limits(-0.5, 0.5));
    run(&mut world, &mut s, 120);
    let r = at(&world, arm).rotation;
    assert!(r.abs() < 0.55, "{r}");
  }

  #[test]
  fn connected_bodies_collide_only_when_asked() {
    let gap = |collide: bool| {
      let (mut world, mut s) = world(60);
      no_gravity(&mut world);
      let a = world.spawn((Transform::IDENTITY, RigidBody::dynamic().lock_rotation(), Collider::rect(20.0, 20.0))).id();
      let b = world
        .spawn((Transform::from_xy(5.0, 0.0), RigidBody::dynamic().lock_rotation(), Collider::rect(20.0, 20.0)))
        .id();
      let joint = Joint::rope(a, b, 1000.0);
      world.spawn(if collide { joint.with_collisions() } else { joint });
      run(&mut world, &mut s, 30);
      distance(&world, a, b)
    };
    assert!((gap(false) - 5.0).abs() < 0.5);
    assert!(gap(true) > 15.0);
  }

  #[test]
  fn a_joint_breaks_past_its_break_force() {
    let (mut world, mut s) = world(60);
    let pivot = anchor(&mut world, Vec2::ZERO);
    let weight = world
      .spawn((Transform::from_xy(0.0, 50.0), RigidBody::dynamic(), Collider::circle(5.0).with_mass(10.0)))
      .id();
    let joint = world.spawn(Joint::rope(pivot, weight, 50.0).with_break_force(1000.0)).id();
    run(&mut world, &mut s, 60);
    let messages = world.resource::<Messages<JointBroken>>();
    let broken: Vec<_> = messages.get_cursor().read(messages).copied().collect();
    assert_eq!(broken, [JointBroken { joint, a: pivot, b: weight }]);
    assert!(world.get::<Joint>(joint).is_none(), "the component went with it");
    assert!(at(&world, weight).translation.y > 200.0, "and the weight fell");
  }

  #[test]
  fn a_joint_waits_for_its_bodies() {
    let (mut world, mut s) = world(60);
    let pivot = anchor(&mut world, Vec2::ZERO);
    let later = world.spawn((Transform::from_xy(0.0, 50.0), Collider::circle(5.0))).id();
    world.spawn(Joint::rope(pivot, later, 50.0));
    run(&mut world, &mut s, 2);
    assert_eq!(world.resource::<Physics>().joint_count(), 0, "inert, no panic");
    world.entity_mut(later).insert(RigidBody::dynamic());
    run(&mut world, &mut s, 2);
    assert_eq!(world.resource::<Physics>().joint_count(), 1);
  }

  #[test]
  fn despawning_a_body_leaves_its_joint_inert() {
    let (mut world, mut s) = world(60);
    let pivot = anchor(&mut world, Vec2::ZERO);
    let weight = bob(&mut world, Vec2::new(0.0, 50.0));
    let joint = world.spawn(Joint::rope(pivot, weight, 50.0)).id();
    run(&mut world, &mut s, 2);
    world.despawn(weight);
    run(&mut world, &mut s, 2);
    assert_eq!(world.resource::<Physics>().joint_count(), 0);
    world.despawn(joint);
    run(&mut world, &mut s, 1);
  }

  #[test]
  fn builders() {
    let (a, b) = (Entity::from_raw_u32(1).unwrap(), Entity::from_raw_u32(2).unwrap());
    let j = Joint::revolute(a, b)
      .at(Vec2::new(1.0, 2.0))
      .with_limits(-1.0, 1.0)
      .with_motor_position(0.5, 10.0, 1.0)
      .with_collisions()
      .with_break_force(100.0);
    assert_eq!(j.anchor, JointAnchor::World(Vec2::new(1.0, 2.0)));
    assert_eq!(j.limits, Some((-1.0, 1.0)));
    assert_eq!(j.motor, Some(JointMotor::Position { target: 0.5, stiffness: 10.0, damping: 1.0 }));
    assert!(j.collisions);
    assert_eq!(j.break_force, Some(100.0));
    assert_eq!(Joint::spring(a, b, 1.0, 2.0, 3.0).kind, JointKind::Spring { rest_length: 1.0, stiffness: 2.0, damping: 3.0 });
    assert_eq!(Joint::fixed(a, b).with_anchors(Vec2::X, Vec2::Y).anchor, JointAnchor::Local { a: Vec2::X, b: Vec2::Y });
  }
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p ryo2d --lib physics::joint`
Expected: compile errors (`joint` module).

- [ ] **Step 3: Write `joint.rs`**

```rust
//! Joints: constraints between two bodies. A `Joint` lives on its own
//! entity (or any entity) and names the two body entities it connects.

use rapier2d::prelude::{
  FixedJointBuilder, GenericJoint, JointAxis, Pose, PrismaticJointBuilder, RevoluteJointBuilder,
  RopeJointBuilder, SpringJointBuilder,
};

use super::convert::{from_rapier, pose, to_rapier};
use crate::core::ecs::{Component, Entity, Vec2};

/// Damping gain of a velocity motor: how hard it pulls toward its target
/// speed each step (Rapier's acceleration-based model).
const VELOCITY_MOTOR_GAIN: f32 = 10.0;

/// What a joint does.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum JointKind {
  /// A hinge or wheel: the bodies turn freely about one shared point.
  Revolute,
  /// A slider: b moves along `axis` (in a's frame) without turning relative to a.
  Prismatic { axis: Vec2 },
  /// A weld: the bodies keep the relative pose they had when it was made.
  Fixed,
  /// A maximum distance between the anchors; slack below it.
  Rope { max_length: f32 },
  /// A damped spring pulling the anchors toward `rest_length` apart.
  Spring { rest_length: f32, stiffness: f32, damping: f32 },
}

/// Where a joint attaches to its bodies.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum JointAnchor {
  /// Revolute and prismatic: at b's origin. Rope and spring: between the origins.
  #[default]
  Origins,
  /// A world point (revolute and prismatic), turned into each body's local
  /// anchor when the joint is made.
  World(Vec2),
  /// Anchors in each body's own space.
  Local { a: Vec2, b: Vec2 },
}

/// A motor on a revolute joint's angle or a prismatic joint's position.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum JointMotor {
  /// Reach `target` speed (rad/s or px/s) with at most `max_force`.
  Velocity { target: f32, max_force: f32 },
  /// Spring toward `target` (radians or px).
  Position { target: f32, stiffness: f32, damping: f32 },
}

/// A constraint between bodies `a` and `b`. Edits apply in place on the next
/// step (an unchanged joint never wakes its bodies). A joint whose bodies are
/// not both simulated waits (logged once); despawning a body leaves the joint
/// inert. Builders that do not apply to a kind are ignored: limits and motors
/// only drive revolute and prismatic joints, `at` only places their pivot.
#[derive(Component, Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Joint {
  pub a: Entity,
  pub b: Entity,
  pub kind: JointKind,
  pub anchor: JointAnchor,
  /// Revolute: the angle range in radians; prismatic: the distance range in px.
  pub limits: Option<(f32, f32)>,
  pub motor: Option<JointMotor>,
  /// Whether the two bodies collide with each other (Box2D's collideConnected).
  pub collisions: bool,
  /// Remove the joint (sending `JointBroken`) when it pulls harder than this
  /// force (mass·px/s²).
  pub break_force: Option<f32>,
}

impl Joint {
  fn new(a: Entity, b: Entity, kind: JointKind) -> Self {
    Self {
      a,
      b,
      kind,
      anchor: JointAnchor::Origins,
      limits: None,
      motor: None,
      collisions: false,
      break_force: None,
    }
  }

  pub fn revolute(a: Entity, b: Entity) -> Self {
    Self::new(a, b, JointKind::Revolute)
  }

  pub fn prismatic(a: Entity, b: Entity, axis: Vec2) -> Self {
    Self::new(a, b, JointKind::Prismatic { axis })
  }

  pub fn fixed(a: Entity, b: Entity) -> Self {
    Self::new(a, b, JointKind::Fixed)
  }

  pub fn rope(a: Entity, b: Entity, max_length: f32) -> Self {
    Self::new(a, b, JointKind::Rope { max_length })
  }

  pub fn spring(a: Entity, b: Entity, rest_length: f32, stiffness: f32, damping: f32) -> Self {
    Self::new(a, b, JointKind::Spring { rest_length, stiffness, damping })
  }

  /// The pivot at a world point.
  pub fn at(mut self, point: Vec2) -> Self {
    self.anchor = JointAnchor::World(point);
    self
  }

  /// Anchors in each body's own space.
  pub fn with_anchors(mut self, a: Vec2, b: Vec2) -> Self {
    self.anchor = JointAnchor::Local { a, b };
    self
  }

  pub fn with_limits(mut self, min: f32, max: f32) -> Self {
    self.limits = Some((min, max));
    self
  }

  pub fn with_motor_velocity(mut self, target: f32, max_force: f32) -> Self {
    self.motor = Some(JointMotor::Velocity { target, max_force });
    self
  }

  pub fn with_motor_position(mut self, target: f32, stiffness: f32, damping: f32) -> Self {
    self.motor = Some(JointMotor::Position { target, stiffness, damping });
    self
  }

  pub fn with_collisions(mut self) -> Self {
    self.collisions = true;
    self
  }

  pub fn with_break_force(mut self, force: f32) -> Self {
    self.break_force = Some(force);
    self
  }
}

fn finite(values: &[f32]) -> bool {
  values.iter().all(|v| v.is_finite())
}

/// Rapier's joint for `joint` between bodies at world poses `pose_a` and
/// `pose_b`, or `None` for nonsense (non-finite values, a zero slider axis, a
/// negative rope length).
pub(crate) fn generic_joint(joint: &Joint, pose_a: (Vec2, f32), pose_b: (Vec2, f32)) -> Option<GenericJoint> {
  let (a, b) = (pose(pose_a.0, pose_a.1), pose(pose_b.0, pose_b.1));
  let pivots = |p: Vec2| {
    (
      from_rapier(a.inverse_transform_point(to_rapier(p))),
      from_rapier(b.inverse_transform_point(to_rapier(p))),
    )
  };
  let hinged = matches!(joint.kind, JointKind::Revolute | JointKind::Prismatic { .. });
  let (la, lb) = match joint.anchor {
    JointAnchor::Local { a, b } => (a, b),
    JointAnchor::World(p) if hinged => pivots(p),
    _ if hinged => pivots(pose_b.0),
    _ => (Vec2::ZERO, Vec2::ZERO),
  };
  if !(la.is_finite() && lb.is_finite()) {
    return None;
  }
  let (la, lb) = (to_rapier(la), to_rapier(lb));
  let mut generic: GenericJoint = match joint.kind {
    JointKind::Revolute => RevoluteJointBuilder::new().local_anchor1(la).local_anchor2(lb).into(),
    JointKind::Prismatic { axis } => {
      let axis = axis.normalize_or_zero();
      if axis == Vec2::ZERO {
        return None;
      }
      // The same direction seen from b, however the bodies are turned
      let axis2 = Vec2::from_angle(pose_a.1 - pose_b.1).rotate(axis);
      PrismaticJointBuilder::new(to_rapier(axis))
        .local_axis2(to_rapier(axis2))
        .local_anchor1(la)
        .local_anchor2(lb)
        .into()
    }
    JointKind::Fixed => FixedJointBuilder::new().local_frame1(a.inv_mul(&b)).local_frame2(Pose::IDENTITY).into(),
    JointKind::Rope { max_length } => {
      if !(max_length.is_finite() && max_length >= 0.0) {
        return None;
      }
      RopeJointBuilder::new(max_length).local_anchor1(la).local_anchor2(lb).into()
    }
    JointKind::Spring { rest_length, stiffness, damping } => {
      if !finite(&[rest_length, stiffness, damping]) {
        return None;
      }
      SpringJointBuilder::new(rest_length.max(0.0), stiffness.max(0.0), damping.max(0.0))
        .local_anchor1(la)
        .local_anchor2(lb)
        .into()
    }
  };
  generic.set_contacts_enabled(joint.collisions);
  let axis = match joint.kind {
    JointKind::Revolute => Some(JointAxis::AngX),
    JointKind::Prismatic { .. } => Some(JointAxis::LinX),
    _ => None,
  };
  if let Some(axis) = axis {
    if let Some((min, max)) = joint.limits
      && finite(&[min, max])
    {
      generic.set_limits(axis, [min.min(max), min.max(max)]);
    }
    match joint.motor {
      Some(JointMotor::Velocity { target, max_force }) if finite(&[target, max_force]) => {
        generic.set_motor_velocity(axis, target, VELOCITY_MOTOR_GAIN);
        generic.set_motor_max_force(axis, max_force.max(0.0));
      }
      Some(JointMotor::Position { target, stiffness, damping }) if finite(&[target, stiffness, damping]) => {
        generic.set_motor_position(axis, target, stiffness.max(0.0), damping.max(0.0));
      }
      _ => {}
    }
  }
  Some(generic)
}
```

If a builder's `into()` to `GenericJoint` needs `.build()` first, call it (`RevoluteJointBuilder::new()...build().into()`).

- [ ] **Step 4: Joints in `world.rs`**

```rust
pub(crate) struct JointEntry {
  /// `None` while inert (a body is missing)
  pub(crate) handle: Option<ImpulseJointHandle>,
  pub(crate) joint: Joint,
  /// The bodies' world poses when it was made: anchors stay where they were put
  pub(crate) poses: ((Vec2, f32), (Vec2, f32)),
}
```

`Physics` gains `pub(crate) joint_entries: HashMap<Entity, JointEntry>`, `joint_of: HashMap<ImpulseJointHandle, Entity>` and `pub(crate) broken: Vec<(Entity, Entity, Entity)>`. Methods:

```rust
  /// Register a new or edited joint. Motors, limits, collisions and the break
  /// force change in place; new bodies, kind or anchors remake it.
  pub(crate) fn sync_joint(&mut self, entity: Entity, joint: &Joint) {
    if let Some(entry) = self.joint_entries.get(&entity) {
      if entry.joint == *joint {
        return;
      }
      let same_frame = entry.joint.a == joint.a
        && entry.joint.b == joint.b
        && entry.joint.kind == joint.kind
        && entry.joint.anchor == joint.anchor;
      if same_frame && let Some(handle) = entry.handle {
        let (pose_a, pose_b) = entry.poses;
        if let Some(data) = generic_joint(joint, pose_a, pose_b)
          && let Some(rapier) = self.joints.get_mut(handle, true)
        {
          rapier.data = data;
        }
        if let Some(entry) = self.joint_entries.get_mut(&entity) {
          entry.joint = joint.clone();
        }
        return;
      }
      self.remove_joint(entity);
    }
    self.insert_joint(entity, joint);
  }

  fn insert_joint(&mut self, entity: Entity, joint: &Joint) {
    let made = (joint.a != joint.b)
      .then(|| {
        let a = self.body_entries.get(&joint.a)?.handle;
        let b = self.body_entries.get(&joint.b)?.handle;
        let poses = (self.body_pose(joint.a)?, self.body_pose(joint.b)?);
        Some((a, b, poses, generic_joint(joint, poses.0, poses.1)?))
      })
      .flatten();
    let Some((a, b, poses, data)) = made else {
      self.warn_once(entity, "a joint needs two different simulated bodies and sane values; it waits");
      self.joint_entries.insert(
        entity,
        JointEntry { handle: None, joint: joint.clone(), poses: ((Vec2::ZERO, 0.0), (Vec2::ZERO, 0.0)) },
      );
      return;
    };
    let handle = self.joints.insert(a, b, data, true);
    self.joint_of.insert(handle, entity);
    self.joint_entries.insert(entity, JointEntry { handle: Some(handle), joint: joint.clone(), poses });
  }

  /// Try inert joints again: their bodies may be simulated now.
  pub(crate) fn retry_inert_joints(&mut self) {
    let ready: Vec<(Entity, Joint)> = self
      .joint_entries
      .iter()
      .filter(|(_, entry)| {
        entry.handle.is_none() && self.has_body(entry.joint.a) && self.has_body(entry.joint.b)
      })
      .map(|(&entity, entry)| (entity, entry.joint.clone()))
      .collect();
    for (entity, joint) in ready {
      self.joint_entries.remove(&entity);
      self.insert_joint(entity, &joint);
    }
  }

  pub(crate) fn remove_joint(&mut self, entity: Entity) {
    if let Some(entry) = self.joint_entries.remove(&entity)
      && let Some(handle) = entry.handle
    {
      self.joint_of.remove(&handle);
      self.joints.remove(handle, true);
    }
    self.warned.remove(&entity);
  }

  pub(crate) fn joint_count(&self) -> usize {
    self.joint_entries.values().filter(|entry| entry.handle.is_some()).count()
  }
```

`remove_body`, before removing the Rapier body, makes its joints inert (Rapier removes them with the body):

```rust
    let joints: Vec<ImpulseJointHandle> = self
      .joints
      .attached_joints(entry.handle)
      .map(|(_, _, handle, _)| handle)
      .collect();
    for handle in joints {
      if let Some(joint_entity) = self.joint_of.remove(&handle)
        && let Some(joint) = self.joint_entries.get_mut(&joint_entity)
      {
        joint.handle = None;
      }
    }
```

`set_enabled` also toggles a joint on the entity:

```rust
    if let Some(entry) = self.joint_entries.get(&entity)
      && let Some(handle) = entry.handle
      && let Some(joint) = self.joints.get_mut(handle, true)
    {
      joint.data.set_enabled(enabled);
    }
```

At the end of `step`, break joints pulled past their limit:

```rust
    self.broken.clear();
    let breakable: Vec<(Entity, ImpulseJointHandle, f32, Entity, Entity)> = self
      .joint_entries
      .iter()
      .filter_map(|(&entity, entry)| {
        Some((entity, entry.handle?, entry.joint.break_force?, entry.joint.a, entry.joint.b))
      })
      .collect();
    for (entity, handle, limit, a, b) in breakable {
      let Some(joint) = self.joints.get(handle) else {
        continue;
      };
      let pull = Vec2::new(joint.impulses.x, joint.impulses.y).length();
      if pull > limit * dt {
        self.remove_joint(entity);
        self.broken.push((entity, a, b));
      }
    }
```

- [ ] **Step 5: Systems (`sync.rs`)**

```rust
/// Register new and edited joints, and retry inert ones.
pub(crate) fn sync_joints(mut physics: ResMut<Physics>, joints: Query<(Entity, &Joint), Changed<Joint>>) {
  for (entity, joint) in &joints {
    physics.sync_joint(entity, joint);
  }
  physics.retry_inert_joints();
}
```

Chain it after `sync_colliders`. `forget_removed` also reads `RemovedComponents<Joint>` (calling `remove_joint`) before the colliders. `HasPhysics` (in `track_disabled`) includes `With<Joint>`. `emit_messages` gains `mut broken_joints: MessageWriter<JointBroken>` and `mut commands: Commands`:

```rust
  for &(joint, a, b) in &physics.broken {
    broken_joints.write(JointBroken { joint, a, b });
    if let Ok(mut entity) = commands.get_entity(joint) {
      entity.remove::<Joint>();
    }
  }
```

`mod.rs`: `mod joint; pub use joint::{Joint, JointAnchor, JointKind, JointMotor};` and add them to the prelude.

- [ ] **Step 6: Run the tests**

Run: `cargo test -p ryo2d --lib physics::`
Expected: PASS. If `a_velocity_motor_turns_a_wheel_and_edits_apply_in_place` reaches less than 90% of its target in a second, raise `VELOCITY_MOTOR_GAIN` (try 50, then 100) and ledger the value.

- [ ] **Step 7: Gate and commit**

```bash
git add -A src
git commit -m "feat: joints" -m "- revolute, prismatic, fixed, rope and spring joints between body entities" -m "- limits, velocity and position motors, optional collisions between the bodies" -m "- break forces with JointBroken; edits apply in place"
```

---
### Task 14: The `physics` demo

Four pages under the demo-kit standard, loading no files so the demo runs on the web. Everything is drawn as outlines of what physics simulates (`ctx.debug_draw()`), coloured by body kind or layer, so the pages show the simulation itself.

**Files:**
- Create: `examples/demos/physics/{Cargo.toml, Ryo.toml, Trunk.toml, index.html, main.rs}`
- Modify: `Cargo.lock` (the new workspace member)

**Interfaces:**
- Consumes: everything public from Tasks 5-13 (`RigidBody`, `Collider`, `ColliderShape`, `Joint`, `JointBroken`, `ContactForce`, `QueryFilter`, `Physics::{raycast, shape_cast, overlap_shape, nearest, overlaps}`), demo-kit (`Demo`, `DemoPage`, `Settings`, `Control`, `content_area`).
- Produces: the `physics-demo` binary; four pages in the tour.

- [ ] **Step 1: Write the package files**

`Cargo.toml`:

```toml
[package]
name = "physics-demo"
version = "0.1.0"
edition = "2024"
description = "Shapes, joints, scene queries, layers and triggers demo for Ryo2D engine"

[[bin]]
name = "physics-demo"
path = "main.rs"

[dependencies]
ryo2d = { path = "../../.." }
demo-kit = { path = "../../demo-kit" }
```

`Ryo.toml`, `Trunk.toml` and `index.html`: copies of `examples/demos/look/`'s with `Look` replaced by `Physics` and `look-demo` by `physics-demo`.

- [ ] **Step 2: Write `main.rs`**

```rust
//! Physics: shapes and stacking, joints, scene queries, layers and triggers.
//! Everything is drawn as outlines of what the simulation holds. Loads no
//! files, so it runs on the web too.

use std::f32::consts::{PI, TAU};

use demo_kit::prelude::*;

fn main() {
  Demo::new("Physics")
    .resolution(960, 640)
    .page(ShapesPage::default)
    .page(JointsPage::default)
    .page(QueriesPage::default)
    .page(LayersPage::default)
    .run();
}

const DYNAMIC: Color = Color::new(90, 220, 110, 255);
const KINEMATIC: Color = Color::new(255, 170, 40, 255);
const FIXED: Color = Color::new(110, 150, 255, 255);
const TRIGGER: Color = Color::new(255, 230, 60, 255);
const JOINT: Color = Color::new(240, 80, 240, 255);
const HIGHLIGHT: Color = Color::WHITE;
/// Pages keep their scenes left of the settings panel.
const RIGHT_EDGE: f32 = 520.0;

/// A repeatable pseudo-random number in [0, 1) for spawn variety.
fn noise(i: u32) -> f32 {
  ((i as f32 * 12.9898).sin() * 43_758.547).fract().abs()
}

/// `collider`'s outline on an entity at `global`, in world space (a circle
/// gets a spoke so its spin shows).
fn outline(global: &GlobalTransform, collider: &Collider) -> Vec<Vec2> {
  let arc = |centre: Vec2, radius: f32, from: f32, to: f32| {
    (0..=12).map(move |i| centre + Vec2::from_angle(from + (to - from) * i as f32 / 12.0) * radius)
  };
  let local: Vec<Vec2> = match &collider.shape {
    ColliderShape::Rect { size } => {
      let h = *size / 2.0;
      vec![Vec2::new(-h.x, -h.y), Vec2::new(h.x, -h.y), h, Vec2::new(-h.x, h.y), Vec2::new(-h.x, -h.y)]
    }
    ColliderShape::Circle { radius } => arc(Vec2::ZERO, *radius, 0.0, TAU).chain([Vec2::ZERO]).collect(),
    ColliderShape::Capsule { height, radius } => {
      let h = (height / 2.0 - radius).max(0.0);
      let mut points: Vec<Vec2> = arc(Vec2::new(0.0, h), *radius, 0.0, PI)
        .chain(arc(Vec2::new(0.0, -h), *radius, PI, TAU))
        .collect();
      points.push(points[0]);
      points
    }
    ColliderShape::Polygon { points } => points.iter().copied().chain(points.first().copied()).collect(),
    ColliderShape::Segment { a, b } => vec![*a, *b],
    ColliderShape::Polyline { points } => points.clone(),
    _ => Vec::new(),
  };
  let affine = global.affine();
  let turn = Vec2::from_angle(collider.rotation);
  local
    .into_iter()
    .map(|p| affine.transform_point2(collider.offset + turn.rotate(p)))
    .collect()
}

/// The kind of body a collider belongs to (its own or its parent's).
fn colour_of(world: &World, entity: Entity, collider: &Collider) -> Color {
  if collider.is_trigger {
    return TRIGGER;
  }
  let body = world.get::<RigidBody>(entity).or_else(|| {
    world.get::<ChildOf>(entity).and_then(|child_of| world.get::<RigidBody>(child_of.parent()))
  });
  match body.map(|body| body.kind) {
    Some(BodyKind::Dynamic) => DYNAMIC,
    Some(BodyKind::Kinematic) => KINEMATIC,
    _ => FIXED,
  }
}

/// Outline every collider, coloured by `colour` (default: by body kind).
fn draw_colliders(ctx: &mut SceneContext, colour: impl Fn(&World, Entity, &Collider) -> Color) {
  let mut query = ctx.world.query::<(Entity, &GlobalTransform, &Collider)>();
  let lines: Vec<(Vec<Vec2>, Color)> = query
    .iter(ctx.world)
    .map(|(entity, global, collider)| (outline(global, collider), colour(ctx.world, entity, collider)))
    .collect();
  let mut debug = ctx.debug_draw();
  for (points, color) in lines {
    debug.polyline(&points, color);
  }
}

/// A line between every joint's two bodies.
fn draw_joints(ctx: &mut SceneContext) {
  let mut query = ctx.world.query::<&Joint>();
  let ends: Vec<(Vec2, Vec2)> = query
    .iter(ctx.world)
    .filter_map(|joint| {
      let a = ctx.world.get::<GlobalTransform>(joint.a)?.translation();
      let b = ctx.world.get::<GlobalTransform>(joint.b)?.translation();
      Some((a, b))
    })
    .collect();
  let mut debug = ctx.debug_draw();
  for (a, b) in ends {
    debug.line(a, b, JOINT);
  }
}

fn mouse_world(ctx: &mut SceneContext) -> Vec2 {
  let (x, y) = ctx.input.mouse_position();
  ctx.camera().screen_to_world(Vec2::new(x, y))
}

/// Walls and a floor around the page's play area.
fn arena(ctx: &mut SceneContext) {
  ctx.world.spawn((
    Transform::IDENTITY,
    Collider::polyline([
      Vec2::new(20.0, 80.0),
      Vec2::new(20.0, 600.0),
      Vec2::new(RIGHT_EDGE, 600.0),
      Vec2::new(RIGHT_EDGE, 80.0),
    ]),
  ));
}

// --- Shapes ---------------------------------------------------------------

const SHAPES: [&str; 5] = ["Mix", "Box", "Circle", "Capsule", "Triangle"];
const MAX_BODIES: u32 = 300;

struct ShapesPage {
  shape: usize,
  bounce: f32,
  friction: f32,
  spawned: u32,
}

impl Default for ShapesPage {
  fn default() -> Self {
    Self { shape: 0, bounce: 0.1, friction: 0.5, spawned: 0 }
  }
}

impl ShapesPage {
  fn spawn(&mut self, ctx: &mut SceneContext, at: Vec2) {
    if self.spawned >= MAX_BODIES {
      return;
    }
    let i = self.spawned;
    self.spawned += 1;
    let size = 14.0 + noise(i) * 18.0;
    let pick = if self.shape == 0 { 1 + i as usize % 4 } else { self.shape };
    let collider = match pick {
      1 => Collider::rect(size, size * (0.6 + noise(i + 7) * 0.8)),
      2 => Collider::circle(size / 2.0),
      3 => Collider::capsule(size * 1.8, size / 2.0),
      _ => Collider::polygon([Vec2::new(-size, size / 2.0), Vec2::new(size, size / 2.0), Vec2::new(0.0, -size)]),
    };
    ctx.world.spawn((
      Transform::from_translation(at).with_rotation(noise(i + 3) * TAU),
      RigidBody::dynamic(),
      collider.with_restitution(self.bounce).with_friction(self.friction),
    ));
  }
}

impl DemoPage for ShapesPage {
  fn name(&self) -> &str {
    "Shapes"
  }
  fn description(&self) -> &str {
    "Boxes, circles, capsules and polygons that tumble, stack and sleep (dimmed by F2 when asleep)."
  }
  fn settings(&mut self, s: &mut Settings) {
    s.choice("Shape", &mut self.shape, &SHAPES);
    s.float("Bounce", &mut self.bounce, 0.0..=1.0, 0.05);
    s.float("Friction", &mut self.friction, 0.0..=1.0, 0.05);
  }
  fn controls(&self) -> Vec<Control> {
    vec![Control::mouse("Drop one"), Control::key(KeyCode::Space, "Drop ten")]
  }
  fn enter(&mut self, ctx: &mut SceneContext) {
    arena(ctx);
    // A valley with a ledge
    ctx.world.spawn((
      Transform::IDENTITY,
      Collider::polyline([Vec2::new(20.0, 420.0), Vec2::new(200.0, 560.0), Vec2::new(380.0, 560.0)]),
    ));
    ctx.world.spawn((Transform::from_xy(430.0, 380.0).with_rotation(-0.3), Collider::rect(160.0, 16.0)));
    for i in 0..20 {
      self.spawn(ctx, Vec2::new(120.0 + (i % 10) as f32 * 30.0, 120.0 + (i / 10) as f32 * 40.0));
    }
  }
  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) {
    if ctx.input.mouse_pressed_and_consume(MouseButton::Left) {
      let at = mouse_world(ctx);
      self.spawn(ctx, at);
    }
    if ctx.input.key_pressed_and_consume(KeyCode::Space) {
      for k in 0..10 {
        self.spawn(ctx, Vec2::new(80.0 + k as f32 * 40.0, 100.0));
      }
    }
    draw_colliders(ctx, colour_of);
  }
  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) {
    let area = content_area(ui);
    ui.draw_text(&format!("Bodies: {} / {MAX_BODIES}", self.spawned), area.x, area.y, Color::WHITE);
  }
}

// --- Joints ---------------------------------------------------------------

const STRENGTHS: [&str; 3] = ["Unbreakable", "Strong", "Weak"];

struct JointsPage {
  motor: f32,
  strength: usize,
  broken: u32,
  car: Vec<Entity>,
  bridge: Vec<Entity>,
}

impl Default for JointsPage {
  fn default() -> Self {
    Self { motor: 6.0, strength: 1, broken: 0, car: Vec::new(), bridge: Vec::new() }
  }
}

impl JointsPage {
  fn break_force(&self) -> Option<f32> {
    match self.strength {
      1 => Some(20_000.0),
      2 => Some(4_000.0),
      _ => None,
    }
  }
}

fn fixed_point(ctx: &mut SceneContext, at: Vec2) -> Entity {
  ctx.world.spawn((Transform::from_translation(at), RigidBody::fixed())).id()
}

impl DemoPage for JointsPage {
  fn name(&self) -> &str {
    "Joints"
  }
  fn description(&self) -> &str {
    "A pendulum, a plank bridge that can break, a spring and a car driven by wheel motors."
  }
  fn settings(&mut self, s: &mut Settings) {
    s.float("Motor (rad/s)", &mut self.motor, -15.0..=15.0, 1.0);
    s.choice("Bridge", &mut self.strength, &STRENGTHS);
  }
  fn controls(&self) -> Vec<Control> {
    vec![Control::key(KeyCode::Space, "Drop a crate on the bridge")]
  }
  fn enter(&mut self, ctx: &mut SceneContext) {
    arena(ctx);
    // Pendulum
    let pivot = fixed_point(ctx, Vec2::new(90.0, 110.0));
    let bob = ctx
      .world
      .spawn((Transform::from_xy(170.0, 110.0), RigidBody::dynamic(), Collider::circle(14.0)))
      .id();
    ctx.world.spawn(Joint::revolute(pivot, bob).at(Vec2::new(90.0, 110.0)));
    // Spring
    let hook = fixed_point(ctx, Vec2::new(440.0, 100.0));
    let weight = ctx
      .world
      .spawn((Transform::from_xy(440.0, 200.0), RigidBody::dynamic(), Collider::rect(30.0, 30.0)))
      .id();
    ctx.world.spawn(Joint::spring(hook, weight, 80.0, 30.0, 2.0));
    // Bridge: planks hinged end to end between two posts
    let (left, right) = (Vec2::new(200.0, 300.0), Vec2::new(480.0, 300.0));
    let planks = 9;
    let step = (right.x - left.x) / planks as f32;
    let mut previous = fixed_point(ctx, left);
    for i in 0..planks {
      let centre = Vec2::new(left.x + step * (i as f32 + 0.5), left.y);
      let plank = ctx
        .world
        .spawn((Transform::from_translation(centre), RigidBody::dynamic(), Collider::rect(step - 2.0, 8.0)))
        .id();
      let hinge = Vec2::new(left.x + step * i as f32, left.y);
      self.bridge.push(ctx.world.spawn(Joint::revolute(previous, plank).at(hinge)).id());
      previous = plank;
    }
    let post = fixed_point(ctx, right);
    self.bridge.push(ctx.world.spawn(Joint::revolute(previous, post).at(right)).id());
    // Car: a chassis on two motored wheels
    let chassis = ctx
      .world
      .spawn((Transform::from_xy(110.0, 545.0), RigidBody::dynamic(), Collider::rect(80.0, 18.0)))
      .id();
    for x in [85.0, 135.0] {
      let at = Vec2::new(x, 570.0);
      let wheel = ctx
        .world
        .spawn((Transform::from_translation(at), RigidBody::dynamic(), Collider::circle(14.0).with_friction(1.0)))
        .id();
      self.car.push(ctx.world.spawn(Joint::revolute(chassis, wheel).at(at).with_motor_velocity(self.motor, 50_000.0)).id());
    }
  }
  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) {
    if ctx.input.key_pressed_and_consume(KeyCode::Space) {
      ctx.world.spawn((
        Transform::from_xy(340.0, 160.0),
        RigidBody::dynamic(),
        Collider::rect(40.0, 40.0).with_mass(3.0),
      ));
    }
    // Settings apply in place: the joints are edited, not rebuilt
    let motor = Some(JointMotor::Velocity { target: self.motor, max_force: 50_000.0 });
    for &joint in &self.car {
      if let Some(mut j) = ctx.world.get_mut::<Joint>(joint)
        && j.motor != motor
      {
        j.motor = motor;
      }
    }
    let break_force = self.break_force();
    for &joint in &self.bridge {
      if let Some(mut j) = ctx.world.get_mut::<Joint>(joint)
        && j.break_force != break_force
      {
        j.break_force = break_force;
      }
    }
    self.broken += ctx.messages::<JointBroken>().len() as u32;
    draw_colliders(ctx, colour_of);
    draw_joints(ctx);
  }
  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) {
    let area = content_area(ui);
    ui.draw_text(&format!("Broken joints: {}", self.broken), area.x, area.y, Color::WHITE);
  }
}

// --- Queries --------------------------------------------------------------

#[derive(Default)]
struct QueriesPage {
  triggers: bool,
  report: String,
}

impl DemoPage for QueriesPage {
  fn name(&self) -> &str {
    "Queries"
  }
  fn description(&self) -> &str {
    "Move the mouse: a ray toward it, a circle cast down from above it, an overlap box and the nearest surface."
  }
  fn settings(&mut self, s: &mut Settings) {
    s.toggle("Hit triggers", &mut self.triggers);
  }
  fn controls(&self) -> Vec<Control> {
    vec![Control::mouse("Aim the queries")]
  }
  fn enter(&mut self, ctx: &mut SceneContext) {
    arena(ctx);
    for (x, y, w, h, turn) in [(150.0, 250.0, 80.0, 30.0, 0.3), (380.0, 200.0, 40.0, 120.0, -0.2), (260.0, 450.0, 140.0, 24.0, 0.0)] {
      ctx.world.spawn((Transform::from_xy(x, y).with_rotation(turn), Collider::rect(w, h)));
    }
    ctx.world.spawn((Transform::from_xy(420.0, 420.0), Collider::circle(40.0)));
    ctx.world.spawn((
      Transform::from_xy(120.0, 470.0),
      Collider::polygon([Vec2::new(-40.0, 30.0), Vec2::new(40.0, 30.0), Vec2::new(0.0, -40.0)]),
    ));
    ctx.world.spawn((Transform::from_xy(300.0, 320.0), Collider::rect(90.0, 60.0).as_trigger()));
  }
  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) {
    let mouse = mouse_world(ctx);
    let filter = if self.triggers { QueryFilter::new().with_triggers() } else { QueryFilter::new() };
    let origin = Vec2::new(40.0, 320.0);
    let probe = Collider::rect(120.0, 80.0);
    let (ray, cast, near, overlapping) = {
      let physics = ctx.physics();
      (
        physics.raycast(origin, mouse - origin, 700.0, &filter),
        physics.shape_cast(&ColliderShape::Circle { radius: 12.0 }, Vec2::new(mouse.x, 90.0), 0.0, Vec2::Y, 600.0, &filter),
        physics.nearest(mouse, 200.0, &filter),
        physics.overlap_shape(&probe.shape, mouse, 0.0, &filter),
      )
    };
    draw_colliders(ctx, |_, entity, collider| {
      if overlapping.iter().any(|o| o.collider == entity) {
        HIGHLIGHT
      } else if collider.is_trigger {
        TRIGGER
      } else {
        FIXED
      }
    });
    let mut debug = ctx.debug_draw();
    debug.rect(mouse, Vec2::new(120.0, 80.0), Color::new(255, 255, 255, 90));
    match ray {
      Some(hit) => {
        debug.line(origin, hit.point, Color::new(255, 120, 120, 255));
        debug.arrow(hit.point, hit.point + hit.normal * 30.0, Color::new(255, 120, 120, 255));
      }
      None => debug.line(origin, origin + (mouse - origin).normalize_or_zero() * 700.0, Color::new(255, 120, 120, 90)),
    }
    if let Some(hit) = cast {
      let centre = Vec2::new(mouse.x, 90.0 + hit.distance);
      debug.line(Vec2::new(mouse.x, 90.0), centre, Color::new(120, 200, 255, 160));
      debug.circle(centre, 12.0, Color::new(120, 200, 255, 255));
    }
    if let Some(near) = near {
      debug.line(mouse, near.point, Color::new(200, 255, 160, 255));
    }
    self.report = match ray {
      Some(hit) => format!("Ray: hit at ({:.0}, {:.0}), {:.0} px away; {} overlapping", hit.point.x, hit.point.y, hit.distance, overlapping.len()),
      None => format!("Ray: no hit; {} overlapping", overlapping.len()),
    };
  }
  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) {
    let area = content_area(ui);
    ui.draw_text(&self.report, area.x, area.y, Color::WHITE);
  }
}

// --- Layers -----------------------------------------------------------------

const LAYER_COLOURS: [Color; 3] = [Color::new(240, 90, 80, 255), Color::new(90, 220, 110, 255), Color::new(90, 140, 255, 255)];

struct LayersPage {
  threshold: f32,
  timer: f32,
  dropped: u32,
  zone: Option<Entity>,
  inside: usize,
  flashes: Vec<(Vec2, f32)>,
}

impl Default for LayersPage {
  fn default() -> Self {
    Self { threshold: 400.0, timer: 0.0, dropped: 0, zone: None, inside: 0, flashes: Vec::new() }
  }
}

impl DemoPage for LayersPage {
  fn name(&self) -> &str {
    "Layers"
  }
  fn description(&self) -> &str {
    "Each shelf catches only its own colour; a trigger counts the balls in it; hard impacts flash."
  }
  fn settings(&mut self, s: &mut Settings) {
    s.float("Impact threshold", &mut self.threshold, 50.0..=2000.0, 50.0);
  }
  fn controls(&self) -> Vec<Control> {
    Vec::new()
  }
  fn enter(&mut self, ctx: &mut SceneContext) {
    arena(ctx);
    for (k, y) in [(0u32, 220.0), (1, 330.0), (2, 440.0)] {
      let layer = 1 << (k + 1);
      ctx.world.spawn((
        Transform::from_xy(150.0 + k as f32 * 110.0, y).with_rotation(0.15),
        Collider::rect(200.0, 12.0).with_layer(layer).with_mask(layer),
      ));
    }
    self.zone = Some(ctx.world.spawn((Transform::from_xy(270.0, 560.0), Collider::rect(500.0, 80.0).as_trigger())).id());
  }
  fn update(&mut self, ctx: &mut SceneContext, dt: f32) {
    self.timer += dt;
    if self.timer > 0.25 && self.dropped < 150 {
      self.timer = 0.0;
      let k = self.dropped % 3;
      ctx.world.spawn((
        Transform::from_xy(60.0 + noise(self.dropped) * 420.0, 100.0),
        RigidBody::dynamic(),
        Collider::circle(9.0).with_layer(1 << (k + 1)).with_restitution(0.3).with_force_events(self.threshold),
      ));
      self.dropped += 1;
    }
    for impact in ctx.messages::<ContactForce>() {
      if impact.started && let Some(global) = ctx.world.get::<GlobalTransform>(impact.body_a) {
        self.flashes.push((global.translation(), 0.3));
      }
    }
    self.flashes.retain_mut(|(_, life)| {
      *life -= dt;
      *life > 0.0
    });
    if let Some(zone) = self.zone {
      self.inside = ctx.physics().overlaps(zone).len();
    }
    draw_colliders(ctx, |_, _, collider| {
      if collider.is_trigger {
        TRIGGER
      } else {
        (0..3).find(|k| collider.layer == 1 << (k + 1)).map_or(FIXED, |k| LAYER_COLOURS[k])
      }
    });
    let flashes = self.flashes.clone();
    let mut debug = ctx.debug_draw();
    for (at, life) in flashes {
      debug.circle(at, 24.0 * (1.0 - life / 0.3) + 8.0, Color::WHITE);
    }
  }
  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) {
    let area = content_area(ui);
    ui.draw_text(&format!("In the trigger: {}", self.inside), area.x, area.y, Color::WHITE);
  }
}
```

`Color::new` is `const`; `DebugDraw::rect` takes a centre and a size; `content_area` returns a `Rect` with `x`/`y`. Changing `Impact threshold` affects balls dropped afterwards.

- [ ] **Step 3: Build and run it**

Run: `cargo build -p physics-demo && ./dev physics`
Expected: four pages; play each for half a minute: shapes tumble and come to rest, the bridge holds a crate (Strong) and breaks under one (Weak), the car drives and reverses with the motor setting, the queries follow the mouse, each shelf catches its own colour and the trigger count changes.

- [ ] **Step 4: Tour it**

Run: `cargo run -p demo-kit --bin demo-tour -- physics`
Expected: `target/demo-tour/physics/page-{1..4}.png` captured without failures; look at each.

- [ ] **Step 5: Check the web build**

Run: `cd examples/demos/physics && trunk build` (then serve `dist/` and open it in Chromium, WebKit and Firefox with the Playwright tools; clean `.playwright-mcp/` in the repo root afterwards).
Expected: the demo starts in each browser and its pages work.

- [ ] **Step 6: Gate and commit**

```bash
git add examples/demos/physics Cargo.lock
git commit -m "feat: physics demo" -m "- shapes and stacking, joints with a breakable bridge and a motored car, scene queries, layers and triggers"
```

---

### Task 15: Documentation

**Files:**
- Rewrite: `.claude/skills/ryo-physics/SKILL.md` and its embedded copy `src/cli/skills/ryo-physics/SKILL.md` (keep them identical)
- Modify: `.claude/skills/ryo-api/SKILL.md` and `src/cli/skills/ryo-api/SKILL.md` (every physics signature), `.claude/skills/ryo-game-dev/SKILL.md` and its copy (recipes)
- Rewrite: `src/core/physics/AGENTS.md`
- Modify: `docs/architecture/overview.md` (physics section), `src/core/AGENTS.md` (anything still describing the old wrapper)

**Interfaces:**
- Consumes: the final public API of Tasks 4-13.

- [ ] **Step 1: Find every stale mention**

Run: `grep -rn "RapierPhysicsWorld\|GroupedCollider\|box_collider\|circle_collider\|is_kinematic\|with_drag\|PhysicsCollisionEvent\|get_collision_events\|are_intersecting\|get_current_intersections\|entity_a\|PIXELS_PER_METER\|no static body\|Rotation is locked\|rotation is locked" .claude src/cli/skills docs/architecture src --include=*.md`
Expected: a list; every hit is rewritten in the steps below, and the grep comes back empty at the end.

- [ ] **Step 2: Rewrite `src/core/physics/AGENTS.md`**

Cover, briefly, with the code as the truth:
- Rapier 0.36 behind `Physics`; `rapier2d` named only in this module; glam 0.33 vs 0.31 through `convert.rs`.
- Units: pixels, `length_unit` = `pixels_per_meter`, density per m², gravity px/s².
- The step: the chained systems in order (`forget_removed`, `track_disabled`, `propagate_transforms`, `register_bodies`, `sync_colliders`, `sync_joints`, `push_changes`, `step_simulation`, `read_back`, `emit_messages`) and the `Last`-phase pair, with why each is where it is.
- Sync rules: change-detected and value-compared with the `1e-3` px / `1e-4` rad tolerance; the write-back is an ordinary change; read back only awake dynamic and velocity-driven kinematic bodies; rotation written back continuously.
- Hierarchy: a collider attaches to the nearest body at or above it; placements (pose relative to the body plus the shape's linear map); bodies under parents write local transforms through the parent (a parent body's new pose); shear under rotated non-uniform parents is approximated.
- Shapes: `shape::build`, when a rect stays a cuboid and when round shapes become polygons; degenerate shapes and zero scale leave the simulation (logged once).
- Events: the collector captures the first contact during the step; `departed` keeps removed colliders' owners until their `Stopped` is reported.
- Joints: entries, inert joints and retries, anchors resolved at registration, in-place edits, breaking by impulse after the step.
- Invariants for agents: never name Rapier types outside the module; never `unwrap`; queries see the last step's world.

- [ ] **Step 3: Rewrite the `ryo-physics` skill**

Sections: when to use which body kind (fixed walls, kinematic platforms and characters, dynamic props), colliders and shapes (and child colliders for compound shapes), materials and the combine rules, layers and masks (the AND rule, with a worked bit table), triggers, messages (`CollisionStarted` with `contact`, `involves`/`other`/`other_collider`, `ContactForce`, `JointBroken`), forces and impulses (one-step forces), queries (`raycast`, `shape_cast`, `overlap_*`, `nearest`, `QueryFilter`), contact state (`contacts` for ground checks, `overlaps`, `in_contact`), joints (each kind with a snippet), units and `PhysicsConfig`, the F2 overlay colours, hierarchy and scale rules (squash and stretch on a child sprite), pitfalls (queries see the last step; set `lock_rotation` for characters; despawned entities in `CollisionStopped`). Copy the file to `src/cli/skills/ryo-physics/SKILL.md`.

- [ ] **Step 4: Update `ryo-api` and `ryo-game-dev`**

`ryo-api`: replace the physics section with every public type and signature from Tasks 4-13 (`Physics`, `PhysicsConfig`, `RigidBody`, `BodyKind`, `AngularVelocity`, `Collider`, `ColliderShape`, `Joint`, `JointKind`, `JointAnchor`, `JointMotor`, `QueryFilter`, `Hit`, `Overlap`, `PointHit`, `Touch`, `Contact`, the four messages, `SceneContext::{physics, physics_mut}`). `ryo-game-dev`: three recipes: a stack of crates on a fixed floor, a rope bridge with a break force, a line-of-sight check with `raycast` and a `QueryFilter` that excludes the looker. Copy both into `src/cli/skills/`.

- [ ] **Step 5: Architecture overview**

`docs/architecture/overview.md`: the physics section describes the ECS-to-Rapier mirror, the step's place in `FixedSet::Physics`, the public `Physics` resource and the time cap (`max_delta` after speed scaling).

- [ ] **Step 6: Verify and commit**

Run the Step 1 grep again. Expected: no hits. Run `cargo test --workspace` (the CLI's skill-embedding tests compare the copies).

```bash
git add .claude/skills src/cli/skills src/core docs/architecture
git commit -m "docs: physics on Rapier 0.36" -m "- the physics skill, API reference and recipes" -m "- module notes and the architecture overview"
```

---

### Task 16: Verification

**Files:** none changed unless a check finds something (each finding is fixed test-first in the task that owns the code and committed on its own).

- [ ] **Step 1: The whole gate**

Run: `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo clippy -p ryo2d --lib --target wasm32-unknown-unknown -- -D warnings && cargo test --workspace`
Expected: all green; record the test count in the ledger.

- [ ] **Step 2: The benchmark against the baseline**

Run: `cargo bench --bench physics -- --noplot 2>&1 | grep -A2 'physics frame'`
Expected: record both numbers next to Task 1's baseline. The bouncing scene minus the empty scene is the physics cost; a regression above 20% is investigated (profile with `./dev <demo> -p`) before finishing.

- [ ] **Step 3: The demo tour**

Run: `./dev demos:tour` and compare against a tour from the commit before Task 1 (`git worktree add /tmp/physics-before <base> && (cd /tmp/physics-before && ./dev demos:tour)`, then `compare -metric AE` per page).
Expected: only pages that use physics differ (time-control, knockback, pathfinding, particles' collision page, the new physics demo). List every differing page with its reason in the ledger; anything unexplained is a finding.

- [ ] **Step 4: Games by eye**

Capture frame 600 of each game (`RYO_CAPTURE_FRAMES=600 RYO_CAPTURE_EXIT=1 RYO_CAPTURE_DIR=<abs dir> cargo run -p <game>`) and play each for a minute: breakout (ball bounces off walls, paddle and blocks), bullet-hell (bullets hit, the player lands on platforms), flappy (dies on pipes and ground), leap-king (host a local game; platforms and the lamp level), nigh_t (unchanged). Expected: unchanged behaviour; differences are findings.

- [ ] **Step 5: Web**

The physics demo and breakout in Chromium, WebKit and Firefox through trunk and the Playwright tools. Expected: both run in all three. Remove `.playwright-mcp/` from the repo root afterwards.

- [ ] **Step 6: Record**

Ledger: test count, benchmark numbers against the baseline, tour differences with reasons, games and browsers checked. No commit unless a finding was fixed.
