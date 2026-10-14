# Physics Gameplay (3b) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Gameplay physics on top of 3a: a character controller (Rapier's `KinematicCharacterController` plus our floor/wall/ceiling, platform and one-way layer), platformer and top-down movement presets driven by `MoveIntent`, one-way platforms for bodies and characters, trigger messages and contents with `Pickup`/`KillZone`/`Checkpoint`/`Respawn`, seamless tile colliders, knockback through physics, and the bundled games moved onto them.

**Architecture:** The physics chain becomes one exclusive system running a private schedule (`PhysicsSchedule`), with `move_characters` after `push_changes` and trigger bookkeeping after the write-back. A new `FixedSet::Movement` (between `Game` and `Physics`) holds the gameplay systems: zone behaviours, tile rebuilds, input bindings, knockback and the presets. Rapier stays inside `src/core/physics/`; `src/core/gameplay/` holds everything without Rapier and depends on physics, never the reverse.

**Tech Stack:** Rust 2024, rapier2d 0.36.1 (glam 0.33 via `physics/convert.rs`), bevy_ecs 0.20 (as `ryo2d::ecs`), criterion, `HeadlessEngine`, demo-kit.

**Spec:** `docs/specs/2026-10-14-physics-gameplay-design.md`

## Global Constraints

- `rapier2d` is named only in `Cargo.toml` and inside `src/core/physics/`; no Rapier type appears in a public signature. Vectors cross Rapier through `physics/convert.rs` (`to_rapier`, `from_rapier`, `pose`).
- bevy_ecs may be named only in `Cargo.toml`, `src/core/ecs/kernel.rs` and the hidden prelude re-export; everything else imports `crate::core::ecs::*` (add kernel re-exports as needed, `pub(crate)` unless games need them).
- `src/core/gameplay/` depends on `src/core/physics/`, never the reverse. `Knockback` (the component) stays in `src/core/ecs/knockback.rs` because physics reads it.
- No `unwrap`/`expect`/`panic!` in library code (tests are fine). A poisoned mutex is recovered with `unwrap_or_else(PoisonError::into_inner)`.
- LF line endings, two-space indent (`rustfmt.toml`).
- Coordinates: pixels, +Y down. A character's "up" is `(0, -1)`. Normals in `CharacterState`/`CharacterHit` point out of the surface toward the character: flat ground is `(0, -1)`.
- Rich components (`CharacterController`, `PlatformerMovement`, `TopDownMovement`, `MoveBindings`, `TileColliders`, `Pickup`, `KillZone`, `Checkpoint`, `Respawn`) are `#[non_exhaustive]` with public fields and builder constructors; engine-written state (`CharacterState`, `PlatformerState`, `TopDownState`, `TriggerContents`) is `#[non_exhaustive]` with public fields games read.
- Invalid input (a dynamic body with a controller, a controller without a collider on its own entity, non-finite velocities, zero one-way directions, a behaviour on a non-trigger collider, a bad tile size) is logged once per entity and skipped: physics uses `Physics::warn_once`; gameplay systems use a `Local<EntityHashSet>` and `crate::engine_warn!(module: "Gameplay", ...)`. Nothing panics.
- The sync invariants of 3a hold: nothing but `read_back` writes a body's `Transform` at `Physics::write_tick`; `move_characters` never writes `Transform` (it sets the body's next kinematic position and marks it driven, so `read_back` writes it). `the_write_back_skip_leaves_every_trajectory_as_it_was` keeps passing.
- Networking (`src/core/network/`) is untouched beyond what compiles. Breaking APIs, demos and games is fine; each task ports every game, demo, test and bench its change breaks, so every commit builds and passes.
- Skills: `.claude/skills/<name>/SKILL.md` and `src/cli/skills/<name>.md` stay byte-identical.
- Commits: `type: brief description` plus bullets if needed; no co-author or other attribution lines; never mention AGENTS.md files in commit messages.
- Task-end gate (every task): `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo clippy -p ryo2d --lib --target wasm32-unknown-unknown -- -D warnings`, `cargo test --workspace`, all green.

## Review Focus

1. **A character spawned overlapping solid geometry** (a respawn point inside a wall, two players spawned on one spot): expected pushed out within a few steps, never stuck, no NaN reaching Rapier. Test in Task 4 (`a_character_spawned_inside_a_wall_is_pushed_out`).
2. **The floor a character stands on is despawned or disabled** (a crumbling platform, a pooled lift): expected the character falls on the next step, `floor` becomes `None`, no lookup into a dead entity, no panic. Test in Task 5 (`a_character_whose_floor_is_despawned_falls`).
3. **A very fast character against a thin collider** (a dash at 3000 px/s into a 4 px wall, a fall onto a one-way segment): expected stopped by it, no tunnelling (the sweep is continuous). Tests in Task 4 (`a_fast_character_does_not_tunnel_through_a_thin_wall`) and Task 6 (`a_fast_fall_lands_on_a_one_way_segment`).
4. **Two entities enter one `Pickup` in the same step** (two players grab a coin together): expected exactly one `PickedUp`, the pickup gone once. Test in Task 12 (`two_collectors_in_one_step_collect_once`).
5. **Non-finite or out-of-range movement input** (an AI writes `MoveIntent.direction = (5, 0)` or NaN; a game writes NaN into a character's `Velocity`): expected the direction clamped to length 1 (NaN as zero), a non-finite velocity not applied and warned once, nothing non-finite in Rapier. Tests in Task 4 (`a_non_finite_velocity_does_not_move_a_character`) and Task 9 (`an_out_of_range_direction_is_clamped`).

## Plan Notes (decisions the plan makes against the spec)

The spec was corrected to match these before execution (same commit as this plan):

- **Input latch:** `Input` already keeps a press no fixed step has seen for the next frame's first fixed step and shows it there only (`end_fixed_steps`). `MoveBindings` therefore reads input inside the fixed step (`FixedSet::Movement`), with no latch of its own.
- **Gamepad:** the engine tracks the first connected gamepad only, with no public id, so `MoveBindings` has `gamepad: bool` (default on) instead of `with_gamepad(id)`.
- **Trigger messages:** `TriggerEntered`/`TriggerExited` carry `other_body: Entity` (the collider itself when it has no body), like `CollisionStarted.body_a`, instead of `Option<Entity>`; `actor()` returns it.
- **Knockback:** the component stays in `ecs/knockback.rs` (physics reads it for characters); its system moves to `gameplay/knockback.rs`.
- **One-way rule for characters:** a one-way collider blocks a character that started the step entirely on its solid side (no "and moves into it" clause); same outcomes, simpler.
- **`CharacterState::floor_one_way`:** added; the platformer preset needs it for drop-through.
- **Navigation:** the type is `NavigationGrid`; `rebuild_from_world` blocks tile maps cell-exactly and skips their outline colliders.
- **Change-tick clamp:** an observer records bevy's `CheckChangeTicks` in a resource and the engine clamps its own, the scenes' and physics' schedules after `Last`.

---

## File Structure

```
src/core/schedule.rs            FixedSet::Movement; EngineSchedules/SceneSchedules::check_change_ticks   (Task 1)
src/core/engine/frame.rs        PendingTickCheck, record_tick_check, Engine::clamp_schedule_ticks         (Task 1)
src/core/physics/
  sync.rs        PhysicsSchedule, run_physics_step; the chain gains move_characters and
                 update_trigger_contents; push_changes skips characters                                    (Tasks 1, 2, 4)
  world.rs       Owner.trigger, trigger bookkeeping, OneWayHooks in the step, drive_to/was_teleported     (Tasks 2, 3, 4)
  events.rs      CollisionStarted/Stopped lose `trigger`; TriggerEntered, TriggerExited                    (Task 2)
  triggers.rs    TriggerContents, update_trigger_contents                                                 (Task 2)
  one_way.rs     user-data encoding, OneWayHooks, the character rule                                      (Tasks 3, 6)
  collider.rs    Collider::one_way field and builders                                                     (Task 3)
  character.rs   CharacterController, CharacterMode, CharacterState, CharacterHit, move_characters        (Tasks 4-7)
  tiles.rs       TileCell, SlopeCorner, outlines/one_way_runs/solid_rects, TileColliders, rebuild         (Tasks 13, 14)
src/core/gameplay/
  mod.rs         module list, register_gameplay_systems, re-exports                                       (Task 7)
  knockback.rs   update_knockback (moved from systems/knockback.rs)                                       (Task 7)
  intent.rs      MoveIntent, MoveBindings, read_move_bindings                                             (Task 8)
  events.rs      MovementEvent, MovementEventKind, PickedUp, Killed, CheckpointReached                    (Tasks 9, 12)
  platformer.rs  PlatformerMovement, WallJump, Dash, PlatformerState, platformer_movement                 (Tasks 9, 10)
  top_down.rs    TopDownMovement, TopDownState, top_down_movement                                         (Task 11)
  zones.rs       Pickup, Respawn, KillZone, Checkpoint and their systems                                  (Task 12)
  test_util.rs   a physics world with the gameplay systems, a floor and a character                       (Task 7)
src/core/tilemap/map.rs         Tilemap::tile_colliders                                                   (Task 14)
src/core/navigation/grid.rs     NavigationGrid::block_tiles; rebuild_from_world and tile maps             (Task 14)
examples/demos/character/{Cargo.toml, Ryo.toml, Trunk.toml, index.html, main.rs}                          (Task 17)
benches/physics.rs              100 characters on a tile level                                            (Task 18)
.claude/skills/ryo-character/SKILL.md + src/cli/skills/ryo-character.md                                   (Task 18)
```

Deleted along the way: `src/core/systems/knockback.rs` (moved), `Knockback`'s velocity take-back (`applied`, `retarget`, the `on_discard` hook), bullet-hell's `PlayerVelocity` and hand collision code, leap-king's and the networking demo's hand landing code, flappy's x-comparison scoring.

---

### Task 1: The physics step as one system, `FixedSet::Movement`, schedule tick clamping

Make the physics chain one exclusive system over a private schedule so no game system can be ordered between its systems; add the `Movement` set the gameplay systems will use; clamp the change ticks of the schedules the engine owns.

**Files:**
- Modify: `src/core/schedule.rs` (FixedSet, `EngineSchedules::new`, new `check_change_ticks` methods, tests)
- Modify: `src/core/ecs/kernel.rs` (re-export `CheckChangeTicks`)
- Modify: `src/core/physics/sync.rs` (`PhysicsStep`, `PhysicsSchedule`, `run_physics_step`, `register_physics_systems`, the tests that place systems inside the chain)
- Modify: `src/core/physics/test_util.rs` (`physics_schedule` helper)
- Modify: `src/core/engine/frame.rs` (`PendingTickCheck`, `record_tick_check`, `Engine::clamp_schedule_ticks`, call in `Engine::frame`)
- Modify: `src/core/engine/mod.rs` (insert `PendingTickCheck`, add the observer at engine start)
- Modify: `src/core/scene/manager.rs` (`SceneManager::check_change_ticks`)
- Modify: `src/core/ecs/AGENTS.md` (FixedSet table: `Movement` row; `Physics` is one system), `src/core/physics/AGENTS.md` (The Step: one system over `PhysicsSchedule`)

**Interfaces:**
- Consumes: 3a's chain systems in `sync.rs` (`forget_removed` … `emit_messages`), `physics_enabled`.
- Produces: `FixedSet::Movement` (between `Game` and `Physics`); `pub(crate) struct PhysicsSchedule` with `PhysicsSchedule::new()`, `check_change_ticks(&mut self, CheckChangeTicks)` and (tests) `add_systems`; `pub(crate) fn run_physics_step(world: &mut World)`; `pub(crate) fn physics_schedule(world: &mut World) -> Mut<'_, PhysicsSchedule>` in `test_util.rs`. Later tasks edit the chain inside `PhysicsSchedule::new` (Task 2 appends `update_trigger_contents`; Task 4 inserts `move_characters` after `push_changes`).

- [ ] **Step 1: Write the failing tests**

In `src/core/schedule.rs` tests, extend `fixed_sets_run_in_the_declared_order`:

```rust
  #[test]
  fn fixed_sets_run_in_the_declared_order() {
    let mut world = World::new();
    world.init_resource::<Log>();
    let mut s = EngineSchedules::new();
    s.add_systems(
      Phase::FixedUpdate,
      push("physics").in_set(FixedSet::Physics),
    );
    s.add_systems(
      Phase::FixedUpdate,
      push("movement").in_set(FixedSet::Movement),
    );
    s.add_systems(
      Phase::FixedUpdate,
      push("snapshot").in_set(FixedSet::Snapshot),
    );
    s.add_systems(
      Phase::FixedUpdate,
      push("transforms").in_set(FixedSet::Transforms),
    );
    s.add_systems(Phase::FixedUpdate, push("game").in_set(FixedSet::Game));
    s.run(Phase::FixedUpdate, &mut world);
    assert_eq!(
      world.resource::<Log>().0,
      ["snapshot", "game", "movement", "physics", "transforms"]
    );
  }
```

In `src/core/physics/sync.rs` tests:

```rust
  /// Moves the first dynamic body to x = 500 every step.
  fn teleport_to_500(mut bodies: Query<&mut Transform, With<RigidBody>>) {
    for mut transform in &mut bodies {
      transform.translation.x = 500.0;
    }
  }

  fn nothing() {}

  #[test]
  fn a_system_ordered_only_after_another_never_runs_inside_the_physics_step() {
    // A system that names no set but orders itself after another call's
    // system belongs to no set: before, the scheduler could place it
    // between the chain's propagation and its write-back, which then
    // overwrote what it wrote. The step is one system now.
    let (mut world, mut s) = world(60);
    no_gravity(&mut world);
    let body = world
      .spawn((Transform::IDENTITY, RigidBody::dynamic(), Collider::circle(5.0)))
      .id();
    s.add_game_systems(Phase::FixedUpdate, nothing);
    s.add_game_systems(Phase::FixedUpdate, teleport_to_500.after(nothing));
    run(&mut world, &mut s, 3);
    let (position, _) = world.resource::<Physics>().body_pose(body).unwrap();
    assert!((position.x - 500.0).abs() < 1e-3, "{position:?}");
  }

  #[test]
  fn the_physics_step_runs_its_own_schedule() {
    let (mut world, mut s) = world(60);
    run(&mut world, &mut s, 1);
    assert!(world.contains_resource::<PhysicsSchedule>());
  }
```

In `src/core/engine/frame.rs` (add a `#[cfg(test)] mod tick_check_tests` at the bottom):

```rust
#[cfg(test)]
mod tick_check_tests {
  use super::*;
  use crate::core::ecs::World;

  /// Half a billion ticks: bevy only checks change ticks that far apart.
  /// About a second in debug builds.
  #[test]
  fn a_change_tick_check_is_recorded_for_the_engine() {
    let mut world = World::new();
    world.init_resource::<PendingTickCheck>();
    world.add_observer(record_tick_check);
    for _ in 0..520_000_000u32 {
      world.increment_change_tick();
    }
    assert!(world.check_change_ticks().is_some());
    let check = world.resource_mut::<PendingTickCheck>().0.take();
    let check = check.expect("the observer recorded the check");
    // Every schedule the engine owns accepts it
    let mut schedules = EngineSchedules::new();
    schedules.check_change_ticks(check);
    SceneSchedules::default().check_change_ticks(check);
    crate::core::physics::sync::PhysicsSchedule::new().check_change_ticks(check);
  }
}
```

If that test takes more than about 5 s in a debug build on this machine, mark it `#[ignore = "slow: half a billion change ticks"]` and say so in the report.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p ryo2d --lib fixed_sets_run_in_the_declared_order a_system_ordered_only_after_another the_physics_step_runs_its_own_schedule a_change_tick_check_is_recorded`
Expected: compile errors (`FixedSet::Movement`, `PhysicsSchedule`, `PendingTickCheck`, `check_change_ticks` do not exist). After Step 3's `FixedSet` change alone, `a_system_ordered_only_after_another_never_runs_inside_the_physics_step` must FAIL against the old registration (check this before replacing `register_physics_systems`; if the scheduler happens to place the system outside the chain on this machine, note it in the report and keep the test).

- [ ] **Step 3: Implement**

`src/core/ecs/kernel.rs`: add

```rust
// The world's change-tick check, for clamping schedules it does not own
pub(crate) use bevy_ecs::change_detection::CheckChangeTicks;
```

`src/core/schedule.rs`:

```rust
  /// The scenes' fixed systems, and builder systems that name no other place
  Game,
  /// Trigger behaviours, tile collider rebuilds, movement input, knockback
  /// and the movement presets
  Movement,
  /// The physics step: one system running physics' own schedule
  Physics,
```

and in `EngineSchedules::new` chain `FixedSet::Game, FixedSet::Movement, FixedSet::Physics`. Add (import `CheckChangeTicks` from `crate::core::ecs::kernel`):

```rust
  /// Clamp every system's change ticks after the world checked its own.
  /// Bevy clamps the schedules in its `Schedules` resource only; the
  /// engine's are not there (see `engine::frame::PendingTickCheck`).
  pub(crate) fn check_change_ticks(&mut self, check: CheckChangeTicks) {
    for schedule in [
      &mut self.first,
      &mut self.pre_update,
      &mut self.fixed_update,
      &mut self.update,
      &mut self.post_update,
      &mut self.last,
    ] {
      schedule.check_change_ticks(check);
    }
  }
```

and on `SceneSchedules`:

```rust
  pub(crate) fn check_change_ticks(&mut self, check: CheckChangeTicks) {
    for schedule in [&mut self.fixed, &mut self.update].into_iter().flatten() {
      schedule.check_change_ticks(check);
    }
  }
```

`src/core/scene/manager.rs` (`impl SceneManager`):

```rust
  /// Clamp the change ticks of every stacked scene's schedules.
  pub(crate) fn check_change_ticks(&mut self, check: CheckChangeTicks) {
    for slot in &mut self.scenes {
      slot.schedules.check_change_ticks(check);
    }
  }
```

`src/core/physics/sync.rs` (imports: `Mut, Schedule, ScheduleLabel, ScheduleSystem, SingleThreadedExecutor` and `CheckChangeTicks` from `crate::core::ecs::kernel`; `Resource` from `crate::core::ecs`):

```rust
#[derive(ScheduleLabel, Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct PhysicsStep;

/// Physics' own schedule: the step's systems, chained. It runs as one
/// system in `FixedSet::Physics` (`run_physics_step`), so a game system can
/// never be ordered between them: the write-back's tick and the chain's
/// order are physics' alone.
#[derive(Resource)]
pub(crate) struct PhysicsSchedule(Schedule);

impl PhysicsSchedule {
  pub(crate) fn new() -> Self {
    let mut schedule = Schedule::new(PhysicsStep);
    schedule.set_executor(SingleThreadedExecutor::new());
    schedule.add_systems(
      (
        forget_removed,
        track_disabled,
        propagate_transforms,
        register_bodies,
        sync_colliders,
        push_changes,
        sync_joints,
        step_simulation,
        read_back,
        emit_messages,
      )
        .chain(),
    );
    Self(schedule)
  }

  pub(crate) fn check_change_ticks(&mut self, check: CheckChangeTicks) {
    self.0.check_change_ticks(check);
  }

  /// Add systems inside the step, for tests that probe its invariants.
  #[cfg(test)]
  pub(crate) fn add_systems<M>(
    &mut self,
    systems: impl crate::core::ecs::IntoScheduleConfigs<ScheduleSystem, M>,
  ) {
    self.0.add_systems(systems);
  }
}

/// `FixedSet::Physics`: run physics' own schedule, made on first use.
pub(crate) fn run_physics_step(world: &mut World) {
  if !world.contains_resource::<PhysicsSchedule>() {
    world.insert_resource(PhysicsSchedule::new());
  }
  world.resource_scope(|world, mut schedule: Mut<PhysicsSchedule>| schedule.0.run(world));
}

/// Add the physics step to `FixedSet::Physics`, running only while physics
/// is not paused.
pub(crate) fn register_physics_systems(schedules: &mut EngineSchedules) {
  schedules.add_systems(
    Phase::FixedUpdate,
    run_physics_step
      .in_set(FixedSet::Physics)
      .run_if(physics_enabled),
  );
}
```

`src/core/physics/test_util.rs`:

```rust
/// Physics' own schedule, made if the step has not run yet: tests that
/// place systems inside the step add them here.
pub(crate) fn physics_schedule(world: &mut World) -> Mut<'_, PhysicsSchedule> {
  if !world.contains_resource::<PhysicsSchedule>() {
    world.insert_resource(PhysicsSchedule::new());
  }
  world.resource_mut::<PhysicsSchedule>()
}
```

Port the existing `sync.rs` tests that order systems against chain systems through `EngineSchedules` (search the tests module for `.after(sync_colliders)`, `.before(read_back)`, `.after(step_simulation)`, `.after(propagate_transforms)`; at least `a_command_applied_right_before_the_write_back_is_pushed`, `a_scale_written_inside_the_physics_chain_reaches_the_collider`, `writes_inside_the_physics_chain_end_as_without_the_skip` and the probes around lines 2790, 3125-3230): add those systems with `physics_schedule(&mut world).add_systems(...)` instead of `s.add_systems(Phase::FixedUpdate, ...)`. Their assertions stay as they are.

`src/core/engine/frame.rs`:

```rust
/// The last change-tick check the world ran, waiting for the engine to
/// clamp the schedules it owns: bevy clamps only the schedules in its
/// `Schedules` resource, and the engine's, the scenes' and physics' own are
/// not there. Every schedule run checks (`Schedule::run`), so the check is
/// observed rather than called.
#[derive(Resource, Default)]
pub(crate) struct PendingTickCheck(pub(crate) Option<CheckChangeTicks>);

/// Observer: remember a change-tick check for `Engine::clamp_schedule_ticks`.
pub(crate) fn record_tick_check(check: On<CheckChangeTicks>, mut pending: ResMut<PendingTickCheck>) {
  pending.0 = Some(*check);
}

impl Engine {
  /// Clamp the change ticks of the schedules the engine owns after the
  /// world checked its own.
  fn clamp_schedule_ticks(&mut self) {
    let Some(check) = self
      .world
      .get_resource_mut::<PendingTickCheck>()
      .and_then(|mut pending| pending.0.take())
    else {
      return;
    };
    self.schedules.check_change_ticks(check);
    with_host(&mut self.world, |_, host| host.scenes.check_change_ticks(check));
    if let Some(mut physics) = self
      .world
      .get_resource_mut::<crate::core::physics::sync::PhysicsSchedule>()
    {
      physics.check_change_ticks(check);
    }
  }
}
```

In `Engine::frame`, right after `self.schedules.run(Phase::Last, &mut self.world);`, call `self.clamp_schedule_ticks();`. In `init_world_resources` (`engine/mod.rs`) insert `PendingTickCheck::default()` and call `world.add_observer(record_tick_check);`. Keep `clear_trackers_system`'s 1024-frame `check_change_ticks` call: it is what triggers the observer when no schedule run did.

Update `src/core/ecs/AGENTS.md` (FixedSet table: a `Movement` row "zone behaviours, tile rebuilds, movement input, knockback, presets"; `Physics` "one system running physics' own schedule (`PhysicsSchedule`): ..."; the `Last` row "… `check_change_ticks` every 1024 frames; the engine then clamps its own, the scenes' and physics' schedules (`PendingTickCheck`)") and `src/core/physics/AGENTS.md` (The Step: "`FixedSet::Physics` is one exclusive system, `run_physics_step`, running physics' own schedule `PhysicsSchedule` (made on first use), chained as `PhysicsSchedule::new` lists them; no game system can be ordered between them. Tests that probe the chain add systems with `test_util::physics_schedule`.").

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ryo2d --lib schedule physics engine`
Expected: PASS, including every ported chain test and the trajectory checksum test.

- [ ] **Step 5: Gate and commit**

Run the task-end gate. Then:

```bash
git add -A
git commit -m "refactor: the physics step runs as one system; FixedSet::Movement" -m "- the physics chain is a private schedule run by one exclusive system, so no game system can be ordered inside it
- a Movement set between Game and Physics for the gameplay systems
- the engine's, the scenes' and physics' schedules get their change ticks clamped"
```

---

### Task 2: Trigger messages, `TriggerContents`, once per body

Trigger overlaps leave `CollisionStarted`/`CollisionStopped` and get their own messages; every trigger collider carries a live list of what is inside; a body with several colliders enters and exits once.

**Files:**
- Modify: `src/core/physics/events.rs` (drop `trigger`; add `TriggerEntered`, `TriggerExited`)
- Create: `src/core/physics/triggers.rs` (`TriggerContents`, `update_trigger_contents`)
- Modify: `src/core/physics/world.rs` (`Owner.trigger`; trigger bookkeeping in `step`; cleanup in `remove_collider`)
- Modify: `src/core/physics/sync.rs` (`emit_messages`; chain gains `update_trigger_contents`; tests)
- Modify: `src/core/physics/mod.rs`, `src/lib.rs` (exports), `src/core/messages/mod.rs` (register the two messages)
- Modify: `examples/games/breakout/src/scenes/game.rs` (death zone through `TriggerEntered`), `examples/games/flappy/src/scenes/game.rs` and `src/components/pipe.rs` (scoring gates)
- Modify: `src/core/physics/AGENTS.md` (Events and Contact State), `.claude/skills/ryo-physics/SKILL.md` + `src/cli/skills/ryo-physics.md`, `.claude/skills/ryo-api/SKILL.md` + `src/cli/skills/ryo-api.md` (message signatures)

**Interfaces:**
- Consumes: Task 1's `PhysicsSchedule::new` chain.
- Produces:
  - `pub struct TriggerEntered { pub trigger: Entity, pub other: Entity, pub other_body: Entity }` and `TriggerExited` (same fields), both `Message + Copy + PartialEq + Eq + Debug`, with `pub fn actor(&self) -> Entity` (= `other_body`).
  - `pub struct TriggerContents` (`Component`, `Default`, `PartialEq`) with `entities(&self) -> &[Entity]`, `contains(&self, Entity) -> bool`, `len`, `is_empty`.
  - `Owner { collider, body, trigger: bool }` (crate-internal).
  - `CollisionStarted { a, b, body_a, body_b, contact }` and `CollisionStopped { a, b, body_a, body_b }` (no `trigger`).

- [ ] **Step 1: Write the failing tests** (in `sync.rs` tests; replace `a_trigger_reports_no_contact_point`)

```rust
  #[test]
  fn entering_a_trigger_is_a_trigger_message_not_a_collision() {
    let (mut world, mut s) = world(60);
    let zone = world
      .spawn((
        Transform::from_xy(0.0, 100.0),
        Collider::rect(100.0, 40.0).as_trigger(),
      ))
      .id();
    let b = ball(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, 60);
    assert!(
      read::<CollisionStarted>(&world).iter().all(|m| !m.involves(zone)),
      "a trigger overlap is not a collision"
    );
    let entered = read::<TriggerEntered>(&world);
    assert_eq!(
      entered,
      [TriggerEntered { trigger: zone, other: b, other_body: b }]
    );
    assert_eq!(entered[0].actor(), b);
  }

  #[test]
  fn a_body_with_two_colliders_enters_and_exits_a_trigger_once() {
    let (mut world, mut s) = world(60);
    no_gravity(&mut world);
    let zone = world
      .spawn((Transform::from_xy(100.0, 0.0), Collider::rect(40.0, 200.0).as_trigger()))
      .id();
    let body = world
      .spawn((Transform::IDENTITY, RigidBody::kinematic(), Velocity::new(300.0, 0.0)))
      .id();
    for dy in [-10.0, 10.0] {
      world.spawn((
        Transform::from_xy(0.0, dy),
        Collider::rect(8.0, 8.0),
        ChildOf(body),
      ));
    }
    run(&mut world, &mut s, 60);
    let entered = read::<TriggerEntered>(&world);
    let exited = read::<TriggerExited>(&world);
    assert_eq!(entered.len(), 1, "{entered:?}");
    assert_eq!(exited.len(), 1, "{exited:?}");
    assert_eq!((entered[0].trigger, entered[0].other_body), (zone, body));
    assert_eq!((exited[0].trigger, exited[0].other_body), (zone, body));
  }

  #[test]
  fn two_overlapping_triggers_each_report_the_other() {
    let (mut world, mut s) = world(60);
    let still = world
      .spawn((Transform::from_xy(50.0, 0.0), Collider::rect(20.0, 20.0).as_trigger()))
      .id();
    let moving = world
      .spawn((
        Transform::IDENTITY,
        RigidBody::kinematic(),
        Velocity::new(300.0, 0.0),
        Collider::rect(20.0, 20.0).as_trigger(),
      ))
      .id();
    run(&mut world, &mut s, 10);
    let entered = read::<TriggerEntered>(&world);
    assert!(entered.iter().any(|m| m.trigger == still && m.other_body == moving));
    assert!(entered.iter().any(|m| m.trigger == moving && m.other_body == still));
  }

  #[test]
  fn trigger_contents_list_what_is_inside_in_entry_order() {
    let (mut world, mut s) = world(60);
    no_gravity(&mut world);
    let zone = world
      .spawn((Transform::IDENTITY, Collider::rect(200.0, 200.0).as_trigger()))
      .id();
    run(&mut world, &mut s, 1);
    assert!(world.get::<TriggerContents>(zone).unwrap().is_empty());
    let first = ball(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, 2);
    let second = ball(&mut world, 20.0, 0.0);
    run(&mut world, &mut s, 2);
    assert_eq!(world.get::<TriggerContents>(zone).unwrap().entities(), [first, second]);
    world.despawn(first);
    run(&mut world, &mut s, 2);
    assert_eq!(world.get::<TriggerContents>(zone).unwrap().entities(), [second]);
  }
```

(`ChildOf`, `TriggerEntered`, `TriggerExited`, `TriggerContents` come into the tests module through `use super::*` once `sync.rs` imports them, or add them to its `use` line.)

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p ryo2d --lib entering_a_trigger a_body_with_two_colliders two_overlapping_triggers trigger_contents_list`
Expected: compile errors (`TriggerEntered`, `TriggerContents` undefined).

- [ ] **Step 3: Implement**

`events.rs`: remove `trigger` from `CollisionStarted` and `CollisionStopped` (and their docs: "Two colliders started touching" / "stopped touching"; triggers report through `TriggerEntered`), then add:

```rust
/// A collider started overlapping a trigger (`Collider::as_trigger`). Sent
/// once per body: a body with several colliders enters on its first and
/// exits when its last one leaves. What can set a trigger off is the usual
/// layer and mask test. Two overlapping triggers each get a message.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct TriggerEntered {
  /// The trigger collider's entity
  pub trigger: Entity,
  /// The collider that entered
  pub other: Entity,
  /// Its body (the collider itself when it has none)
  pub other_body: Entity,
}

/// A body (or bare collider) left a trigger, or one of them went away:
/// either entity may already be despawned.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct TriggerExited {
  pub trigger: Entity,
  pub other: Entity,
  pub other_body: Entity,
}

macro_rules! trigger_helpers {
  ($($message:ty),*) => {$(
    impl $message {
      /// What entered or left: the other side's body, or its collider when it has none.
      pub fn actor(&self) -> Entity {
        self.other_body
      }
    }
  )*};
}

trigger_helpers!(TriggerEntered, TriggerExited);
```

`world.rs`:
- `Owner` gains `pub(crate) trigger: bool` ("whether the collider is a trigger"). Set it in `add_collider` (`trigger: collider.is_trigger`); in `sync_collider`, when `old.is_trigger != collider.is_trigger`, update the owner (`if let Some(owner) = self.owners.get_mut(&handle) { owner.trigger = collider.is_trigger; }`). `remove_body` keeps it.
- New fields on `Physics` (initialised in `new`):

```rust
  /// Per (trigger collider, body inside it), how many of that body's
  /// colliders overlap the trigger: entered at 1, exited at 0
  trigger_overlaps: HashMap<(Entity, Entity), u32>,
  /// What each trigger holds now, in entry order (`TriggerContents`)
  pub(crate) trigger_contents: EntityHashMap<Vec<Entity>>,
  /// Triggers whose contents changed in the last step
  pub(crate) changed_triggers: EntityHashSet,
  /// The last step's trigger entries and exits
  pub(crate) trigger_events: Vec<TriggerEvent>,
```

```rust
/// A body (or bare collider) entering or leaving a trigger in the last step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TriggerEvent {
  pub(crate) trigger: Entity,
  pub(crate) other: Entity,
  pub(crate) other_body: Entity,
  pub(crate) entered: bool,
}
```

- In `step`, clear `trigger_events` with `collisions`, and in the collision drain push sensor events through a new method instead of `self.collisions`:

```rust
      if let (Some(a), Some(b)) = (owner(event.collider1()), owner(event.collider2())) {
        if event.sensor() {
          sensors.push((a, b, event.started()));
        } else {
          self.collisions.push(RawCollision { a, b, started: event.started(), contact });
        }
      }
    }
    for (a, b, started) in sensors {
      self.record_trigger(a, b, started);
    }
```

(`sensors` is a local `Vec<(Owner, Owner, bool)>`; `RawCollision` loses its `sensor` field.)

```rust
  /// Count a trigger overlap starting or stopping, once per body: the body
  /// enters at its first collider inside and exits when its last one leaves.
  fn record_trigger(&mut self, a: Owner, b: Owner, started: bool) {
    for (trigger, other) in [(a, b), (b, a)] {
      if !trigger.trigger {
        continue;
      }
      let key = (trigger.collider, other.body);
      let count = self.trigger_overlaps.entry(key).or_insert(0);
      let changed = if started {
        *count += 1;
        *count == 1
      } else {
        *count = count.saturating_sub(1);
        *count == 0
      };
      if !started && *count == 0 {
        self.trigger_overlaps.remove(&key);
      }
      if !changed {
        continue;
      }
      let contents = self.trigger_contents.entry(trigger.collider).or_default();
      if started {
        contents.push(other.body);
      } else {
        contents.retain(|&inside| inside != other.body);
      }
      self.changed_triggers.insert(trigger.collider);
      self.trigger_events.push(TriggerEvent {
        trigger: trigger.collider,
        other: other.collider,
        other_body: other.body,
        entered: started,
      });
    }
  }
```

- In `remove_collider`, also `self.trigger_contents.remove(&entity);` (its overlaps end with Rapier's `Stopped` events next step; a despawned trigger needs no contents).

`sync.rs` `emit_messages`: write `CollisionStarted { a, b, body_a, body_b, contact }` / `CollisionStopped { a, b, body_a, body_b }` from `physics.collisions` (now solid only), and for each `physics.trigger_events` a `TriggerEntered` or `TriggerExited` (add `MessageWriter<TriggerEntered>` and `MessageWriter<TriggerExited>` parameters).

`triggers.rs`:

```rust
//! What is inside each trigger, kept on the trigger as a component.

use super::collider::Collider;
use super::world::Physics;
use crate::core::ecs::{Changed, Commands, Component, Entity, Query, ResMut};

/// The bodies (or bare colliders) inside a trigger now, in the order they
/// entered. The engine adds it to every trigger collider and updates it
/// after each step; read it for pressure plates, zones and "is anyone in
/// here" checks.
#[derive(Component, Debug, Clone, Default, PartialEq, Eq)]
pub struct TriggerContents {
  entities: Vec<Entity>,
}

impl TriggerContents {
  pub fn entities(&self) -> &[Entity] {
    &self.entities
  }

  pub fn contains(&self, entity: Entity) -> bool {
    self.entities.contains(&entity)
  }

  pub fn len(&self) -> usize {
    self.entities.len()
  }

  pub fn is_empty(&self) -> bool {
    self.entities.is_empty()
  }
}

/// After the step's messages: add `TriggerContents` to new triggers (and
/// take it from colliders that stopped being one), and bring the changed
/// ones up to date.
pub(crate) fn update_trigger_contents(
  mut physics: ResMut<Physics>,
  edited: Query<(Entity, &Collider, Option<&TriggerContents>), Changed<Collider>>,
  mut contents: Query<&mut TriggerContents>,
  mut commands: Commands,
) {
  for (entity, collider, has) in &edited {
    match (collider.is_trigger, has.is_some()) {
      (true, false) => {
        let entities = physics.trigger_contents.get(&entity).cloned().unwrap_or_default();
        commands.entity(entity).insert(TriggerContents { entities });
      }
      (false, true) => {
        commands.entity(entity).remove::<TriggerContents>();
      }
      _ => {}
    }
  }
  let mut changed = std::mem::take(&mut physics.changed_triggers);
  for trigger in changed.drain() {
    if let Ok(mut held) = contents.get_mut(trigger) {
      let now = physics.trigger_contents.get(&trigger).map(Vec::as_slice).unwrap_or(&[]);
      if held.entities != now {
        held.entities = now.to_vec();
      }
    }
  }
  physics.changed_triggers = changed;
}
```

Append `update_trigger_contents` after `emit_messages` in `PhysicsSchedule::new`'s chain. Export `TriggerEntered`, `TriggerExited` (from `events`) and `TriggerContents` (from `triggers`) in `physics/mod.rs` and the prelude; register both messages in `register_engine_messages`.

Ports:
- **breakout** (`scenes/game.rs` ~1178-1200): the death-zone branch reads `ctx.messages::<TriggerEntered>()` instead (`if Some(m.trigger) == self.death_zone`, the ball is `m.actor()`); the `CollisionStarted` loop keeps paddle and block hits.
- **flappy**: each pipe pair gets a score gate: a trigger collider in the gap (child of the pair's scrolling entity, or its own kinematic entity scrolled with it), `Collider::rect(4.0, gap_height).as_trigger()`, marked with a `ScoreGate` component. `update_pipes` drops the x comparison; a new check reads `ctx.messages::<TriggerEntered>()` and scores when `m.actor() == bird` and the trigger has `ScoreGate`. Keep `check_collisions` (solid pipe hits) as it is. `debug.rs`'s `CollisionStarted` loop needs no change unless it read `.trigger` (it does not).
- Every other `CollisionStarted { .., trigger, .. }` construction or `.trigger` read the compiler finds (tests in `sync.rs`, `tests/`).

Docs: `physics/AGENTS.md` Events and Contact State gains "Trigger overlaps are not collisions: `emit_messages` writes `TriggerEntered`/`TriggerExited`, counted per body (`Physics::trigger_overlaps`), and `update_trigger_contents` keeps `TriggerContents` on every trigger"; the `ryo-physics` and `ryo-api` skills (both copies) document the messages, `actor()` and `TriggerContents`, and remove `trigger` from the collision messages.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ryo2d --lib physics` and `cargo test --workspace`
Expected: PASS. Run breakout and flappy headless for 600 frames (`RYO_HEADLESS=1 RYO_CAPTURE_FRAMES=600 RYO_CAPTURE_EXIT=1 RYO_CAPTURE_DIR=<abs dir> cargo run -p breakout`, same for flappy with `RYO_INPUT` taps on Space every 25 frames): no panics; flappy's score rises past at least one pipe (log or capture).

- [ ] **Step 5: Gate and commit**

```bash
git add -A
git commit -m "feat: trigger messages and TriggerContents" -m "- trigger overlaps are TriggerEntered/TriggerExited, once per body; collision messages are solid contact only
- every trigger keeps a live TriggerContents
- breakout's death zone and flappy's score gates use them"
```

---

### Task 3: One-way colliders for bodies

`Collider::one_way()` and `one_way_towards(direction)`; dynamic bodies meet them through Rapier's contact hook.

**Files:**
- Modify: `src/core/physics/collider.rs` (field `one_way`, builders, docs)
- Create: `src/core/physics/one_way.rs` (encoding, `OneWayHooks`)
- Modify: `src/core/physics/world.rs` (`collider_builder`, `apply_material`, the warning in `sync_collider`, `step` passes `&OneWayHooks`)
- Modify: `src/core/physics/mod.rs` (`mod one_way;`)
- Modify: `src/core/physics/AGENTS.md`, `ryo-physics` and `ryo-api` skills (both copies)

**Interfaces:**
- Produces: `Collider.one_way: Option<Vec2>` (local direction things arrive from; `None` solid everywhere), `Collider::one_way(self) -> Self`, `Collider::one_way_towards(self, direction: Vec2) -> Self`; crate-internal `one_way::{encode, decode, hooks, OneWayHooks, ONE_WAY_ANGLE}` (Task 6 adds the character rule here).

- [ ] **Step 1: Write the failing tests** (`one_way.rs` tests module; use `crate::core::physics::test_util::*`)

```rust
#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ecs::{Transform, Velocity};
  use crate::core::physics::test_util::*;
  use crate::core::physics::{Collider, Physics, RigidBody};

  #[test]
  fn a_direction_round_trips_through_user_data() {
    let d = Vec2::new(0.6, -0.8);
    assert_eq!(decode(encode(Some(d))), Some(d));
    assert_eq!(decode(encode(None)), None);
    assert_eq!(decode(encode(Some(Vec2::ZERO))), None, "zero is two-sided");
    assert_eq!(decode(0), None);
  }

  fn platform(world: &mut crate::core::ecs::World, collider: Collider) {
    world.spawn((Transform::IDENTITY, collider));
  }

  #[test]
  fn a_crate_tossed_up_through_a_one_way_platform_lands_on_top() {
    let (mut world, mut s) = world(60);
    platform(&mut world, Collider::rect(200.0, 10.0).one_way());
    let crate_ = world
      .spawn((
        Transform::from_xy(0.0, 60.0),
        RigidBody::dynamic().lock_rotation(),
        Collider::rect(20.0, 20.0),
        Velocity::new(0.0, -700.0),
      ))
      .id();
    run(&mut world, &mut s, 180);
    let y = at(&world, crate_).translation.y;
    assert!((y + 15.0).abs() < 1.0, "resting on top at -15: {y}");
  }

  #[test]
  fn a_body_moving_sideways_passes_through_a_one_way_platform() {
    let (mut world, mut s) = world(60);
    no_gravity(&mut world);
    platform(&mut world, Collider::rect(40.0, 40.0).one_way());
    let crate_ = world
      .spawn((
        Transform::from_xy(-60.0, 0.0),
        RigidBody::dynamic().lock_rotation(),
        Collider::rect(10.0, 10.0),
        Velocity::new(300.0, 0.0),
      ))
      .id();
    run(&mut world, &mut s, 30);
    assert!(at(&world, crate_).translation.x > 60.0);
  }

  #[test]
  fn a_turned_one_way_collider_is_solid_along_its_own_up() {
    // Turned a quarter: its up, (0, -1), points along +X, so it stops what
    // comes from the right and lets through what comes from the left
    let (mut world, mut s) = world(60);
    no_gravity(&mut world);
    world.spawn((
      Transform::IDENTITY.with_rotation(std::f32::consts::FRAC_PI_2),
      Collider::rect(80.0, 10.0).one_way(),
    ));
    let from_right = world
      .spawn((
        Transform::from_xy(60.0, 0.0),
        RigidBody::dynamic().lock_rotation(),
        Collider::rect(10.0, 10.0),
        Velocity::new(-300.0, 0.0),
      ))
      .id();
    let from_left = world
      .spawn((
        Transform::from_xy(-60.0, 30.0),
        RigidBody::dynamic().lock_rotation(),
        Collider::rect(10.0, 10.0),
        Velocity::new(300.0, 0.0),
      ))
      .id();
    run(&mut world, &mut s, 30);
    assert!(at(&world, from_right).translation.x > 5.0, "stopped on the solid side");
    assert!(at(&world, from_left).translation.x > 60.0, "passed through");
  }

  #[test]
  fn making_a_collider_one_way_applies_in_place() {
    let (mut world, mut s) = world(60);
    let slab = world.spawn((Transform::IDENTITY, Collider::rect(200.0, 10.0))).id();
    run(&mut world, &mut s, 1);
    world.get_mut::<Collider>(slab).unwrap().one_way = Some(Vec2::NEG_Y);
    let crate_ = world
      .spawn((
        Transform::from_xy(0.0, 60.0),
        RigidBody::dynamic().lock_rotation(),
        Collider::rect(20.0, 20.0),
        Velocity::new(0.0, -700.0),
      ))
      .id();
    run(&mut world, &mut s, 180);
    assert!(at(&world, crate_).translation.y < -5.0);
  }

  #[test]
  fn a_zero_one_way_direction_is_solid_and_warned_once() {
    let (mut world, mut s) = world(60);
    platform(&mut world, Collider::rect(200.0, 10.0).one_way_towards(Vec2::ZERO));
    let crate_ = world
      .spawn((
        Transform::from_xy(0.0, 60.0),
        RigidBody::dynamic().lock_rotation(),
        Collider::rect(20.0, 20.0),
        Velocity::new(0.0, -700.0),
      ))
      .id();
    run(&mut world, &mut s, 60);
    assert!(at(&world, crate_).translation.y > 5.0, "blocked from below");
    assert_eq!(world.resource::<Physics>().warnings, 1);
  }
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p ryo2d --lib one_way`
Expected: compile errors (`one_way` module, `Collider::one_way` missing).

- [ ] **Step 3: Implement**

`collider.rs`: add to `Collider` (and `None` wherever a `Collider` is constructed):

```rust
  /// One-way: solid only against things arriving from this side, a
  /// direction in the collider's own space (`one_way()` is `(0, -1)`: from
  /// above), so it turns with the collider. `None` is solid from every side.
  pub one_way: Option<Vec2>,
```

```rust
  /// A one-way platform: solid only against things arriving from above (its
  /// own up). Things pass up through it and in from the sides; a character
  /// drops through with `CharacterController::drop_through`.
  pub fn one_way(self) -> Self {
    self.one_way_towards(Vec2::NEG_Y)
  }

  /// Solid only against things arriving from `direction`'s side, in the
  /// collider's own space: `Vec2::NEG_X` stops what comes from the left and
  /// lets through what comes from the right. A zero direction is solid from
  /// every side (logged once).
  pub fn one_way_towards(mut self, direction: Vec2) -> Self {
    self.one_way = Some(direction);
    self
  }
```

`one_way.rs`:

```rust
//! One-way colliders: the direction kept in Rapier's collider user data, the
//! contact hook dynamic bodies meet them through, and (Task 6) the rule the
//! character sweep applies.

use rapier2d::prelude::{ActiveHooks, ContactModificationContext, PhysicsHooks};

use super::convert::to_rapier;
use crate::core::ecs::Vec2;

/// How far a contact normal may turn from a one-way collider's direction and
/// still push (45°).
pub(crate) const ONE_WAY_ANGLE: f32 = std::f32::consts::FRAC_PI_4;

/// Marks user data that holds a direction.
const FLAG: u128 = 1 << 64;

/// A one-way direction as Rapier user data: the flag bit, then the unit
/// direction's `y` and `x` bits. A missing or zero direction is 0.
pub(crate) fn encode(direction: Option<Vec2>) -> u128 {
  match direction.and_then(Vec2::try_normalize) {
    Some(d) => FLAG | (u128::from(d.y.to_bits()) << 32) | u128::from(d.x.to_bits()),
    None => 0,
  }
}

pub(crate) fn decode(data: u128) -> Option<Vec2> {
  (data & FLAG != 0).then(|| {
    Vec2::new(
      f32::from_bits(data as u32),
      f32::from_bits((data >> 32) as u32),
    )
  })
}

/// The hooks a collider needs for its one-way setting.
pub(crate) fn hooks(direction: Option<Vec2>) -> ActiveHooks {
  if encode(direction) != 0 {
    ActiveHooks::MODIFY_SOLVER_CONTACTS
  } else {
    ActiveHooks::empty()
  }
}

/// The step's contact hook: a contact with a one-way collider pushes only
/// while its normal points along the collider's direction. Rapier tracks
/// each crossing in the manifold, so a body part-way through is not snapped
/// back.
pub(crate) struct OneWayHooks;

impl PhysicsHooks for OneWayHooks {
  fn modify_solver_contacts(&self, context: &mut ContactModificationContext) {
    let (Some(c1), Some(c2)) = (
      context.colliders.get(context.collider1),
      context.colliders.get(context.collider2),
    ) else {
      return;
    };
    // `update_as_oneway_platform` compares the normal leaving collider 1, in
    // collider 1's own space
    let allowed = if let Some(local) = decode(c1.user_data) {
      to_rapier(local)
    } else if let Some(local) = decode(c2.user_data) {
      let world = c2.position().rotation.transform_vector(to_rapier(local));
      c1.position().rotation.inverse().transform_vector(-world)
    } else {
      return;
    };
    context.update_as_oneway_platform(allowed, ONE_WAY_ANGLE);
  }
}
```

(Check in rapier2d-0.36.1 that `Collider::user_data` is a public `u128` field and that `ContactModificationContext::colliders` is `&ColliderSet`; both are in `src/geometry/collider.rs` and `src/pipeline/physics_hooks.rs`. If `rotation.transform_vector` is named differently on glamx's `Rot2`, use the method `Pose2::inverse` uses in `glamx-0.3.1/src/pose2.rs`.)

`world.rs`:
- `collider_builder`: `.user_data(one_way::encode(collider.one_way)).active_hooks(one_way::hooks(collider.one_way))`.
- `apply_material`: `if old.one_way != new.one_way { rapier.user_data = one_way::encode(new.one_way); rapier.set_active_hooks(one_way::hooks(new.one_way)); }`.
- At the top of `sync_collider`: `if collider.one_way.is_some_and(|d| d.try_normalize().is_none()) { self.warn_once(entity, "a one-way collider needs a non-zero direction; it is solid from every side"); }`.
- `step`: pass `&OneWayHooks` where it passes `&()`.

Docs: `physics/AGENTS.md` (a "One-Way Colliders" section: the user-data encoding, the hook, `ONE_WAY_ANGLE`, the direction turning with the collider; characters in Task 6), the `ryo-physics` and `ryo-api` skills (both copies): the two builders and the field.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ryo2d --lib one_way physics`
Expected: PASS.

- [ ] **Step 5: Gate and commit**

```bash
git add -A
git commit -m "feat: one-way colliders" -m "- Collider::one_way and one_way_towards, turning with the collider
- dynamic bodies meet them through Rapier's contact hook"
```

---

### Task 4: The character controller core

`CharacterController` on a kinematic body, swept each step by Rapier's `KinematicCharacterController`; floors, walls and ceilings told apart; `Velocity` left as the real velocity; `CharacterState` written every step.

**Files:**
- Create: `src/core/physics/character.rs`
- Modify: `src/core/physics/world.rs` (`was_teleported`, `drive_to`)
- Modify: `src/core/physics/sync.rs` (`move_characters` in the chain after `push_changes`; `push_changes`'s kinematic drive skips characters)
- Modify: `src/core/physics/mod.rs`, `src/lib.rs` (exports)
- Modify: `src/core/physics/AGENTS.md` (a "Characters" section; step list gains `move_characters`)

**Interfaces:**
- Consumes: Task 1's chain; 3a's `Physics` maps (`body_entries`, `collider_entries`, `owners`), `params.length_unit`, `warn_once`.
- Produces (all in the prelude):
  - `pub enum CharacterMode { Platformer, TopDown }` (`Default` = `Platformer`).
  - `pub struct CharacterController { pub mode, pub max_slope: f32 /* radians */, pub step_height: Option<f32>, pub snap_to_ground: Option<f32>, pub skin: Option<f32>, pub push: bool, pub drop_through: bool }` with `platformer()`, `top_down()`, `with_max_slope(degrees)`, `with_step_height(px)`, `with_snap_to_ground(px)`, `with_skin(px)`, `with_push(bool)`; `Default` = `platformer()`. Requires `Transform`, `Velocity`, `CharacterState` and `RigidBody` (kinematic, rotation locked).
  - `pub struct CharacterState { pub on_floor, pub on_wall, pub on_ceiling: bool, pub floor_normal, pub wall_normal: Vec2, pub floor: Option<Entity>, pub floor_one_way: bool, pub platform_velocity: Vec2, pub landed, pub left_floor, pub sliding_down_slope: bool, pub velocity_before: Vec2, pub hits: Vec<CharacterHit> }` plus crate-internal `floor_point: Vec2`, `floor_collider: Option<Entity>`, `stuck: bool`, `drop_from: Option<Entity>`.
  - `pub struct CharacterHit { pub entity: Entity, pub body: Entity, pub point: Vec2, pub normal: Vec2 }`.
  - Crate-internal: `Physics::move_character(&mut self, entity, &CharacterController, &mut CharacterState, &mut Vec2 /* velocity */, extra: Vec2, dt: f32) -> Option<CharacterMove>` with `pub(crate) struct CharacterMove { pub(crate) normals: Vec<Vec2>, pub(crate) drop_done: bool }`; `Physics::was_teleported(&self, Entity) -> bool`; `Physics::drive_to(&mut self, Entity, Vec2)`; `pub(crate) fn move_characters(...)`. Task 5 adds platform carry and pushing inside `move_character`; Task 6 the one-way rule; Task 7 the knockback term (`extra`) and clipping.

- [ ] **Step 1: Write the failing tests** (`character.rs` tests module)

```rust
#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ecs::{ChildOf, Transform, Velocity, World};
  use crate::core::physics::test_util::*;
  use crate::core::physics::{Collider, Physics, RigidBody};
  use crate::core::schedule::EngineSchedules;

  /// A 20×40 platformer character centred at (x, y).
  fn character(world: &mut World, x: f32, y: f32) -> Entity {
    world
      .spawn((
        Transform::from_xy(x, y),
        CharacterController::platformer(),
        Collider::rect(20.0, 40.0),
      ))
      .id()
  }

  /// One step with `gravity` px/s² added to the character's velocity first,
  /// as a game's movement code does.
  fn step(world: &mut World, s: &mut EngineSchedules, e: Entity, gravity: f32) {
    world.get_mut::<Velocity>(e).unwrap().dy += gravity / 60.0;
    run(world, s, 1);
  }

  fn state(world: &World, e: Entity) -> CharacterState {
    world.get::<CharacterState>(e).unwrap().clone()
  }

  fn velocity(world: &World, e: Entity) -> Vec2 {
    world.get::<Velocity>(e).unwrap().as_vec2()
  }

  #[test]
  fn a_character_falls_and_lands_on_the_floor() {
    let (mut world, mut s) = world(60);
    let f = floor(&mut world, 100.0);
    let e = character(&mut world, 0.0, 0.0);
    let mut landings = 0;
    for _ in 0..120 {
      step(&mut world, &mut s, e, 980.0);
      landings += usize::from(state(&world, e).landed);
    }
    let st = state(&world, e);
    assert!(st.on_floor);
    assert_eq!(landings, 1, "landed once");
    assert_eq!(st.floor, Some(f));
    assert!((st.floor_normal - Vec2::NEG_Y).length() < 1e-3, "{:?}", st.floor_normal);
    assert!(velocity(&world, e).y.abs() < 1e-3, "the fall stops");
    let bottom = at(&world, e).translation.y + 20.0;
    assert!((99.0..=100.0).contains(&bottom), "{bottom}");
  }

  #[test]
  fn a_wall_stops_the_run() {
    let (mut world, mut s) = world(60);
    world.spawn((Transform::from_xy(100.0, 0.0), Collider::rect(20.0, 400.0)));
    let e = character(&mut world, 0.0, 0.0);
    world.get_mut::<Velocity>(e).unwrap().dx = 300.0;
    for _ in 0..60 {
      world.get_mut::<Velocity>(e).unwrap().dx = 300.0;
      run(&mut world, &mut s, 1);
    }
    let st = state(&world, e);
    assert!(st.on_wall);
    assert!((st.wall_normal - Vec2::NEG_X).length() < 1e-3, "{:?}", st.wall_normal);
    assert_eq!(st.velocity_before.x, 300.0);
    assert!(velocity(&world, e).x.abs() < 1e-3);
    assert!(at(&world, e).translation.x + 10.0 <= 90.5);
  }

  #[test]
  fn a_ceiling_stops_the_rise() {
    let (mut world, mut s) = world(60);
    world.spawn((Transform::from_xy(0.0, -100.0), Collider::rect(400.0, 20.0)));
    let e = character(&mut world, 0.0, 0.0);
    world.get_mut::<Velocity>(e).unwrap().dy = -300.0;
    let mut bumped = false;
    for _ in 0..30 {
      run(&mut world, &mut s, 1);
      bumped |= state(&world, e).on_ceiling;
    }
    assert!(bumped);
    assert!(velocity(&world, e).y.abs() < 1e-3);
    assert!(at(&world, e).translation.y - 20.0 >= -90.5);
  }

  /// A ramp rising to the right at `degrees`, its foot at (0, 100).
  fn ramp(world: &mut World, degrees: f32) {
    let rise = 300.0 * degrees.to_radians().tan();
    world.spawn((
      Transform::IDENTITY,
      Collider::polygon([
        Vec2::new(0.0, 100.0),
        Vec2::new(300.0, 100.0),
        Vec2::new(300.0, 100.0 - rise),
      ]),
    ));
    floor(world, 100.0);
  }

  #[test]
  fn a_character_walks_up_a_gentle_slope() {
    let (mut world, mut s) = world(60);
    ramp(&mut world, 30.0);
    let e = character(&mut world, -60.0, 70.0);
    for _ in 0..120 {
      world.get_mut::<Velocity>(e).unwrap().dx = 150.0;
      step(&mut world, &mut s, e, 980.0);
    }
    assert!(at(&world, e).translation.y < 40.0, "climbed: {:?}", at(&world, e));
    assert!(state(&world, e).on_floor);
  }

  #[test]
  fn a_character_standing_on_a_gentle_slope_does_not_creep() {
    let (mut world, mut s) = world(60);
    ramp(&mut world, 30.0);
    let e = character(&mut world, 150.0, 0.0);
    for _ in 0..60 {
      step(&mut world, &mut s, e, 980.0);
    }
    let x = at(&world, e).translation.x;
    for _ in 0..60 {
      step(&mut world, &mut s, e, 980.0);
    }
    assert!((at(&world, e).translation.x - x).abs() < 1.0);
  }

  #[test]
  fn a_character_slides_down_a_steep_slope() {
    let (mut world, mut s) = world(60);
    ramp(&mut world, 60.0);
    let e = character(&mut world, 200.0, -280.0);
    let mut sliding = false;
    for _ in 0..90 {
      step(&mut world, &mut s, e, 980.0);
      sliding |= state(&world, e).sliding_down_slope;
    }
    assert!(sliding);
    assert!(at(&world, e).translation.x < 200.0, "slid back down");
  }

  #[test]
  fn a_ledge_below_the_step_height_is_climbed_and_a_higher_one_is_not() {
    for (height, climbs) in [(8.0, true), (20.0, false)] {
      let (mut world, mut s) = world(60);
      floor(&mut world, 100.0);
      world.spawn((
        Transform::from_xy(140.0, 100.0 - height / 2.0),
        Collider::rect(100.0, height),
      ));
      let e = world
        .spawn((
          Transform::from_xy(0.0, 79.0),
          CharacterController::platformer().with_step_height(12.0),
          Collider::rect(20.0, 40.0),
        ))
        .id();
      for _ in 0..90 {
        world.get_mut::<Velocity>(e).unwrap().dx = 150.0;
        step(&mut world, &mut s, e, 980.0);
      }
      let x = at(&world, e).translation.x;
      assert_eq!(x > 100.0, climbs, "ledge {height}: x = {x}");
    }
  }

  #[test]
  fn snapping_keeps_a_character_on_a_downward_slope() {
    let (mut world, mut s) = world(60);
    let rise = 300.0 * 20f32.to_radians().tan();
    world.spawn((
      Transform::IDENTITY,
      Collider::polygon([
        Vec2::new(0.0, 100.0 - rise),
        Vec2::new(0.0, 100.0),
        Vec2::new(300.0, 100.0),
      ]),
    ));
    floor(&mut world, 100.0);
    let e = character(&mut world, 20.0, 100.0 - rise - 30.0);
    for _ in 0..30 {
      step(&mut world, &mut s, e, 980.0);
    }
    let mut off = 0;
    for _ in 0..60 {
      world.get_mut::<Velocity>(e).unwrap().dx = 200.0;
      step(&mut world, &mut s, e, 980.0);
      off += usize::from(!state(&world, e).on_floor);
    }
    assert_eq!(off, 0, "left the ground on the way down");
  }

  #[test]
  fn top_down_characters_slide_along_walls_and_have_no_floor() {
    let (mut world, mut s) = world(60);
    world.spawn((Transform::from_xy(100.0, 0.0), Collider::rect(20.0, 800.0)));
    let e = world
      .spawn((
        Transform::IDENTITY,
        CharacterController::top_down(),
        Collider::circle(10.0),
      ))
      .id();
    let mut walled = false;
    for _ in 0..60 {
      world.get_mut::<Velocity>(e).unwrap().dx = 200.0;
      world.get_mut::<Velocity>(e).unwrap().dy = 200.0;
      run(&mut world, &mut s, 1);
      let st = state(&world, e);
      assert!(!st.on_floor && !st.on_ceiling);
      walled |= st.on_wall;
    }
    assert!(walled);
    let t = at(&world, e).translation;
    assert!(t.x <= 80.5 && t.y > 150.0, "slid along the wall: {t:?}");
  }

  #[test]
  fn a_dynamic_body_with_a_controller_is_warned_and_not_moved() {
    let (mut world, mut s) = world(60);
    let e = world
      .spawn((
        Transform::IDENTITY,
        RigidBody::dynamic(),
        CharacterController::platformer(),
        Collider::rect(20.0, 40.0),
      ))
      .id();
    run(&mut world, &mut s, 5);
    assert_eq!(world.resource::<Physics>().warnings, 1);
    assert!(state(&world, e).hits.is_empty());
  }

  #[test]
  fn a_controller_needs_a_collider_on_its_own_entity() {
    let (mut world, mut s) = world(60);
    let e = world
      .spawn((Transform::IDENTITY, CharacterController::platformer()))
      .id();
    world.spawn((Transform::IDENTITY, Collider::rect(20.0, 40.0), ChildOf(e)));
    run(&mut world, &mut s, 5);
    assert_eq!(world.resource::<Physics>().warnings, 1);
  }

  #[test]
  fn writing_transform_teleports_a_character_past_a_wall() {
    let (mut world, mut s) = world(60);
    world.spawn((Transform::from_xy(100.0, 0.0), Collider::rect(20.0, 400.0)));
    let e = character(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, 1);
    world.get_mut::<Transform>(e).unwrap().translation.x = 300.0;
    run(&mut world, &mut s, 2);
    let (position, _) = world.resource::<Physics>().body_pose(e).unwrap();
    assert!((position.x - 300.0).abs() < 1e-3, "{position:?}");
  }

  // Review focus 1
  #[test]
  fn a_character_spawned_inside_a_wall_is_pushed_out() {
    let (mut world, mut s) = world(60);
    world.spawn((Transform::from_xy(100.0, 0.0), Collider::rect(20.0, 400.0)));
    let e = character(&mut world, 85.0, 0.0);
    run(&mut world, &mut s, 10);
    let x = at(&world, e).translation.x;
    assert!(x.is_finite() && x + 10.0 <= 90.5, "{x}");
  }

  // Review focus 3
  #[test]
  fn a_fast_character_does_not_tunnel_through_a_thin_wall() {
    let (mut world, mut s) = world(60);
    world.spawn((Transform::from_xy(100.0, 0.0), Collider::rect(4.0, 400.0)));
    let e = character(&mut world, 0.0, 0.0);
    for _ in 0..10 {
      world.get_mut::<Velocity>(e).unwrap().dx = 3000.0;
      run(&mut world, &mut s, 1);
    }
    assert!(at(&world, e).translation.x + 10.0 <= 98.5);
  }

  // Review focus 5
  #[test]
  fn a_non_finite_velocity_does_not_move_a_character() {
    let (mut world, mut s) = world(60);
    let e = character(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, 1);
    world.get_mut::<Velocity>(e).unwrap().dx = f32::NAN;
    run(&mut world, &mut s, 3);
    assert_eq!(at(&world, e).translation, Vec2::ZERO);
    assert_eq!(velocity(&world, e), Vec2::ZERO);
    assert_eq!(world.resource::<Physics>().warnings, 1);
    let (position, _) = world.resource::<Physics>().body_pose(e).unwrap();
    assert!(position.is_finite());
  }
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p ryo2d --lib physics::character`
Expected: compile errors (`character` module missing).

- [ ] **Step 3: Implement**

`world.rs` (next to `drive_kinematic`):

```rust
  /// Whether the game moved this body through `Transform` this step.
  pub(crate) fn was_teleported(&self, entity: Entity) -> bool {
    self.teleported.contains(&entity)
  }

  /// Move a kinematic body to `translation` over the step (its rotation
  /// kept), and have the write-back place it.
  pub(crate) fn drive_to(&mut self, entity: Entity, translation: Vec2) {
    let Some(entry) = self.body_entries.get(&entity) else {
      return;
    };
    let Some(body) = self.bodies.get_mut(entry.handle) else {
      return;
    };
    let rotation = body.rotation().angle();
    body.set_next_kinematic_position(pose(translation, rotation));
    self.driven.insert(entity);
  }
```

`character.rs`:

```rust
//! The character controller: a kinematic body swept each step by Rapier's
//! `KinematicCharacterController`, with floors, walls and ceilings told
//! apart and `Velocity` left as the real velocity.

use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI};

use rapier2d::control::{
  CharacterAutostep, CharacterCollision, CharacterLength, KinematicCharacterController,
};
use rapier2d::parry::query::{QueryDispatcher, ShapeCastOptions, ShapeCastStatus};
use rapier2d::prelude::{QueryFilter as RapierFilter, Vector};

use super::body::RigidBody;
use super::convert::{from_rapier, to_rapier};
use super::world::Physics;
use crate::core::ecs::{Component, Entity, Query, Res, ResMut, Transform, Vec2, Velocity};
use crate::core::time::Time;

/// How a character's surfaces are told apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CharacterMode {
  /// Side view: up is -Y. Floors, walls and ceilings; snapping and steps.
  #[default]
  Platformer,
  /// Seen from above: no up. Every surface is a wall; no snapping or steps.
  TopDown,
}

fn character_body() -> RigidBody {
  RigidBody::kinematic().lock_rotation()
}

/// Makes an entity a character: a kinematic body (added when it has none)
/// the engine sweeps by its `Velocity` every step, sliding along what it
/// hits. The `Collider` on the same entity is the swept shape; it collides
/// with what that collider's layer and mask allow, and triggers never block
/// it.
///
/// `Velocity` is what the game wants (its gravity, run and jump, or a
/// preset's); after the step it is the real velocity: landing zeroes the
/// fall, a head bump stops the rise, a wall stops the run.
/// `CharacterState` says what the move touched. Writing `Transform`
/// teleports the character (no sweep), as for any kinematic body.
#[derive(Component, Debug, Clone, PartialEq)]
#[non_exhaustive]
#[require(Transform, Velocity, CharacterState, RigidBody = character_body())]
pub struct CharacterController {
  pub mode: CharacterMode,
  /// The steepest floor, in radians (default 45°): steeper surfaces are
  /// walls, and the character slides down them
  pub max_slope: f32,
  /// Walk up ledges this high, in px (default off; platformer only)
  pub step_height: Option<f32>,
  /// Keep to the ground down slopes and over bumps within this distance, in
  /// px; `None` is a fifth of the collider's height, 0 turns it off
  /// (platformer only)
  pub snap_to_ground: Option<f32>,
  /// The gap kept from surfaces, in px; `None` is 1% of the collider's height
  pub skin: Option<f32>,
  /// Push dynamic bodies it walks into, with a mass from its collider's
  /// density (default on)
  pub push: bool,
  /// Ignore one-way platforms until fully below the one underfoot; the
  /// engine clears it then (or at once when not on a one-way platform)
  pub drop_through: bool,
}

impl CharacterController {
  /// A side-view character: floors, walls, ceilings, slopes up to 45°.
  pub fn platformer() -> Self {
    Self {
      mode: CharacterMode::Platformer,
      max_slope: FRAC_PI_4,
      step_height: None,
      snap_to_ground: None,
      skin: None,
      push: true,
      drop_through: false,
    }
  }

  /// A top-down character: every surface is a wall it slides along.
  pub fn top_down() -> Self {
    Self {
      mode: CharacterMode::TopDown,
      ..Self::platformer()
    }
  }

  /// The steepest floor, in degrees.
  pub fn with_max_slope(mut self, degrees: f32) -> Self {
    self.max_slope = degrees.to_radians();
    self
  }

  /// Walk up ledges up to `px` high.
  pub fn with_step_height(mut self, px: f32) -> Self {
    self.step_height = Some(px);
    self
  }

  /// Keep to the ground within `px` (0 turns snapping off).
  pub fn with_snap_to_ground(mut self, px: f32) -> Self {
    self.snap_to_ground = Some(px);
    self
  }

  /// Keep `px` from surfaces.
  pub fn with_skin(mut self, px: f32) -> Self {
    self.skin = Some(px);
    self
  }

  /// Push dynamic bodies (default) or be blocked by them.
  pub fn with_push(mut self, push: bool) -> Self {
    self.push = push;
    self
  }
}

impl Default for CharacterController {
  fn default() -> Self {
    Self::platformer()
  }
}

/// Something a character's move touched: the collider and its body, the
/// world point, and the surface normal there (out of the surface, toward
/// the character).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CharacterHit {
  pub entity: Entity,
  pub body: Entity,
  pub point: Vec2,
  pub normal: Vec2,
}

/// What a character's last move found, written by the engine every step.
/// Normals point out of the surface: flat ground is `(0, -1)`.
#[derive(Component, Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct CharacterState {
  pub on_floor: bool,
  pub on_wall: bool,
  pub on_ceiling: bool,
  /// Zero off the floor
  pub floor_normal: Vec2,
  /// Zero off any wall
  pub wall_normal: Vec2,
  /// The body (or bare collider) underfoot
  pub floor: Option<Entity>,
  /// Whether the floor is a one-way platform
  pub floor_one_way: bool,
  /// How fast the floor moves under the character, in px/s
  pub platform_velocity: Vec2,
  /// True on the step the character lands
  pub landed: bool,
  /// True on the step it leaves the floor
  pub left_floor: bool,
  /// On a slope steeper than `max_slope`, sliding down
  pub sliding_down_slope: bool,
  /// The `Velocity` the step asked for, before anything clipped it (for bounces)
  pub velocity_before: Vec2,
  /// Everything the move touched
  pub hits: Vec<CharacterHit>,
  /// Where the floor was touched, for carrying
  pub(crate) floor_point: Vec2,
  /// The collider underfoot (drop-through starts from it)
  pub(crate) floor_collider: Option<Entity>,
  /// The last move started inside something solid, or the character is new
  /// or teleported: the next move pushes out first
  pub(crate) stuck: bool,
  /// The one-way collider a drop-through started from
  pub(crate) drop_from: Option<Entity>,
}

impl Default for CharacterState {
  fn default() -> Self {
    Self {
      on_floor: false,
      on_wall: false,
      on_ceiling: false,
      floor_normal: Vec2::ZERO,
      wall_normal: Vec2::ZERO,
      floor: None,
      floor_one_way: false,
      platform_velocity: Vec2::ZERO,
      landed: false,
      left_floor: false,
      sliding_down_slope: false,
      velocity_before: Vec2::ZERO,
      hits: Vec::new(),
      floor_point: Vec2::ZERO,
      floor_collider: None,
      stuck: true,
      drop_from: None,
    }
  }
}

/// What `Physics::move_character` reports back to the system.
pub(crate) struct CharacterMove {
  /// The normals of every surface the move touched (for clipping knockback)
  pub(crate) normals: Vec<Vec2>,
  /// A drop-through is over
  pub(crate) drop_done: bool,
}

/// How far below its feet a character looks for the floor, beyond its skin.
const FLOOR_PROBE: f32 = 2.0;
/// Slack on the floor and ceiling angle tests.
const ANGLE_SLACK: f32 = 1e-4;

impl Physics {
  /// Sweep one character by `velocity` plus `extra` (knockback) over `dt`;
  /// update its state and leave the real velocity in `velocity`. `None`
  /// when it is not moved: not registered yet, teleported this step, or
  /// refused (logged once).
  pub(crate) fn move_character(
    &mut self,
    entity: Entity,
    controller: &CharacterController,
    state: &mut CharacterState,
    velocity: &mut Vec2,
    extra: Vec2,
    dt: f32,
  ) -> Option<CharacterMove> {
    let body_handle = self.body_entries.get(&entity)?.handle;
    let body = self.bodies.get(body_handle)?;
    if !body.is_kinematic() {
      self.warn_once(
        entity,
        "a CharacterController needs a kinematic body (one is added when there is none); this one is not moved",
      );
      return None;
    }
    if !body.is_enabled() {
      return None;
    }
    let Some(collider_handle) = self.collider_entries.get(&entity).map(|entry| entry.handle) else {
      self.warn_once(entity, "a CharacterController needs a Collider on its own entity");
      return None;
    };
    if self.was_teleported(entity) {
      state.stuck = true;
      state.hits.clear();
      return None;
    }
    let mut wanted = *velocity;
    if !wanted.is_finite() {
      self.warn_once(entity, "a non-finite Velocity is not applied to a character");
      wanted = Vec2::ZERO;
    }
    let extra = if extra.is_finite() { extra } else { Vec2::ZERO };
    let platformer = controller.mode == CharacterMode::Platformer;
    let up = Vec2::NEG_Y;
    let max_slope = if controller.max_slope.is_finite() {
      controller.max_slope.clamp(0.0, FRAC_PI_2)
    } else {
      FRAC_PI_4
    };
    let (start, shape, groups) = {
      let collider = self.colliders.get(collider_handle)?;
      (
        *collider.position(),
        collider.shared_shape().clone(),
        collider.collision_groups(),
      )
    };
    let local = shape.compute_local_aabb();
    let height = (local.maxs.y - local.mins.y).max(f32::EPSILON);
    let skin = controller
      .skin
      .filter(|skin| skin.is_finite() && *skin > 0.0)
      .unwrap_or(0.01 * height);
    let kcc = KinematicCharacterController {
      up: to_rapier(up),
      offset: CharacterLength::Absolute(skin),
      slide: true,
      autostep: platformer
        .then_some(controller.step_height)
        .flatten()
        .filter(|h| h.is_finite() && *h > 0.0)
        .map(|h| CharacterAutostep {
          max_height: CharacterLength::Absolute(h),
          min_width: CharacterLength::Relative(0.5),
          include_dynamic_bodies: false,
        }),
      max_slope_climb_angle: if platformer { max_slope } else { PI },
      min_slope_slide_angle: if platformer { max_slope } else { PI },
      snap_to_ground: if platformer {
        match controller.snap_to_ground {
          None => Some(CharacterLength::Relative(0.2)),
          Some(d) if d.is_finite() && d > 0.0 => Some(CharacterLength::Absolute(d)),
          Some(_) => None,
        }
      } else {
        None
      },
      // Rapier's nudge is in world units, and the world is in pixels
      normal_nudge_factor: 1.0e-4 * self.params.length_unit,
    };
    let desired = (wanted + extra) * dt;

    let filter = RapierFilter::default()
      .exclude_rigid_body(body_handle)
      .exclude_sensors()
      .groups(groups);
    let dispatcher: &dyn QueryDispatcher = self.narrow_phase.query_dispatcher();
    let queries =
      self
        .broad_phase
        .as_query_pipeline(dispatcher, &self.bodies, &self.colliders, filter);
    let mut pose = start;
    if state.stuck {
      let out = kcc.move_shape(dt, &queries, &*shape, &pose, Vector::ZERO, |_| {});
      pose.translation += out.translation;
    }
    let mut collisions: Vec<CharacterCollision> = Vec::new();
    let moved = kcc.move_shape(
      dt,
      &queries,
      &*shape,
      &pose,
      to_rapier(desired),
      |collision| collisions.push(collision),
    );
    pose.translation += moved.translation;
    let floor_hit = platformer
      .then(|| {
        let options = ShapeCastOptions::with_max_time_of_impact(skin + FLOOR_PROBE);
        queries.cast_shape(&pose, to_rapier(Vec2::Y), &*shape, options)
      })
      .flatten();

    let cos = max_slope.cos() - ANGLE_SLACK;
    let was_on_floor = state.on_floor;
    let previous_platform = state.platform_velocity;
    state.hits.clear();
    state.on_wall = false;
    state.on_ceiling = false;
    state.wall_normal = Vec2::ZERO;
    let mut normals = Vec::with_capacity(collisions.len() + 1);
    let mut stuck = false;
    for collision in &collisions {
      let normal = from_rapier(collision.hit.normal1);
      stuck |= collision.hit.status == ShapeCastStatus::PenetratingOrWithinTargetDist;
      let Some(owner) = self.owners.get(&collision.handle).copied() else {
        continue;
      };
      state.hits.push(CharacterHit {
        entity: owner.collider,
        body: owner.body,
        point: from_rapier(collision.hit.witness1),
        normal,
      });
      if platformer && normal.dot(up) >= cos {
        // A floor: the probe below settles it
      } else if platformer && normal.dot(-up) >= cos {
        state.on_ceiling = true;
        normals.push(normal);
      } else {
        state.on_wall = true;
        state.wall_normal = normal;
        normals.push(normal);
      }
    }
    match floor_hit.filter(|(_, hit)| from_rapier(hit.normal1).dot(up) >= cos) {
      Some((handle, hit)) => {
        let owner = self.owners.get(&handle).copied();
        state.on_floor = true;
        state.floor_normal = from_rapier(hit.normal1);
        state.floor = owner.map(|owner| owner.body);
        state.floor_collider = owner.map(|owner| owner.collider);
        state.floor_point = from_rapier(hit.witness1);
        normals.push(up);
      }
      None => {
        state.on_floor = false;
        state.floor_normal = Vec2::ZERO;
        state.floor = None;
        state.floor_collider = None;
      }
    }
    drop(queries);

    // The real velocity: what runs into a wall or ceiling is removed, and on
    // a floor the fall stops (the vertical part only, so standing on a
    // gentle slope does not creep down it)
    let mut real = wanted;
    for normal in &normals {
      let into = real.dot(*normal);
      if into < 0.0 {
        real -= *normal * into;
      }
    }
    state.platform_velocity = Vec2::ZERO;
    if was_on_floor && !state.on_floor {
      real += previous_platform;
    }
    state.landed = state.on_floor && !was_on_floor;
    state.left_floor = was_on_floor && !state.on_floor;
    state.sliding_down_slope = moved.is_sliding_down_slope;
    state.velocity_before = wanted;
    state.stuck = stuck;
    *velocity = real;

    let delta = from_rapier(pose.translation - start.translation);
    let body_translation = from_rapier(self.bodies.get(body_handle)?.translation());
    self.drive_to(entity, body_translation + delta);
    Some(CharacterMove {
      normals,
      drop_done: false,
    })
  }
}

/// The physics step, after `push_changes`: sweep every character.
pub(crate) fn move_characters(
  mut physics: ResMut<Physics>,
  time: Res<Time>,
  mut characters: Query<(
    Entity,
    &mut CharacterController,
    &mut CharacterState,
    &mut Velocity,
  )>,
) {
  let dt = time.fixed.timestep();
  for (entity, controller, mut state, mut velocity) in &mut characters {
    let mut v = velocity.as_vec2();
    let Some(_moved) = physics.move_character(
      entity,
      &controller,
      &mut state,
      &mut v,
      Vec2::ZERO,
      dt,
    ) else {
      continue;
    };
    velocity.set_if_neq(Velocity::from_vec2(v));
  }
}
```

Notes for this step:
- Verify the Rapier names against rapier2d-0.36.1 / parry2d sources before relying on them: `rapier2d::control` exports, `Collider::shared_shape`, `Collider::collision_groups`, `QueryFilter::{exclude_rigid_body, exclude_sensors, groups}`, `QueryPipeline::cast_shape(&Pose, Vector, &dyn Shape, ShapeCastOptions)` (as `query.rs` uses it), `ShapeCastStatus::PenetratingOrWithinTargetDist`, and `ShapeCastHit::{normal1, witness1, status}`. `normal1`/`witness1` are on the hit collider, in world space (3a verified this for `shape_cast`; the floor test confirms the sign).
- The floor's normal is not used to clip the velocity: the floor removes the fall only (`normals.push(up)`), so the slope test holds; if the sweep itself slides a standing character down a gentle slope, fix it in the sweep input (zero the downward part of `desired` while on the floor and within `max_slope`), not by loosening the test.
- `push_changes`' kinematic drive loop skips characters: its query becomes `Query<(Entity, &RigidBody, &Velocity, &AngularVelocity), Without<CharacterController>>`.
- Insert `move_characters` after `push_changes` in `PhysicsSchedule::new`.

Exports: `CharacterController`, `CharacterMode`, `CharacterState`, `CharacterHit` in `physics/mod.rs` and the prelude.

Docs: `physics/AGENTS.md` gains "Characters": the controller is a kinematic body moved by `move_characters` right after `push_changes` (fresh game writes, queries at the last step's poses); it sets the next kinematic position and marks the body driven, so `read_back` writes `Transform` (the write-back tick invariant holds); a `Transform` write teleports (no sweep, `stuck` set so the next move pushes out first); Rapier's sweep plus our floor probe (`FLOOR_PROBE` px beyond the skin) and classification against up `(0, -1)`; the velocity rule; `normal_nudge_factor` scaled by the length unit; the warnings. The step list gains `move_characters` between `push_changes` and `sync_joints`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ryo2d --lib physics`
Expected: PASS, including every 3a physics test and the trajectory checksum test (characters are not in that scene).

- [ ] **Step 5: Gate and commit**

```bash
git add -A
git commit -m "feat: character controller" -m "- CharacterController sweeps a kinematic body with Rapier's character controller each step
- floors, walls and ceilings told apart; Velocity left as the real velocity
- CharacterState says what the move touched; Transform writes teleport"
```

---

### Task 5: Platform carry, momentum and pushing

Characters ride what they stand on (moved by `Transform`, by velocity, turning, or dynamic), keep a moving floor's momentum when they leave it, and push dynamic bodies.

**Files:**
- Modify: `src/core/physics/character.rs` (carry, leaving momentum, pushing; tests)
- Modify: `src/core/physics/AGENTS.md` (Characters section)

**Interfaces:**
- Consumes: Task 4's `move_character`, `CharacterState::{floor, floor_point}`.
- Produces: `Physics::floor_motion(&self, floor: Entity, at: Vec2, dt: f32) -> Vec2` (crate-internal); `CharacterState::platform_velocity` filled.

- [ ] **Step 1: Write the failing tests** (`character.rs` tests)

```rust
  /// A kinematic platform 200×20 whose top is at y = 100, and a character landed on it.
  fn on_platform(world: &mut World, s: &mut EngineSchedules) -> (Entity, Entity) {
    let platform = world
      .spawn((
        Transform::from_xy(0.0, 110.0),
        RigidBody::kinematic(),
        Collider::rect(200.0, 20.0),
      ))
      .id();
    let e = character(world, 0.0, 70.0);
    for _ in 0..30 {
      step(world, s, e, 980.0);
    }
    assert!(state(world, e).on_floor);
    (platform, e)
  }

  #[test]
  fn a_character_rides_a_platform_moved_by_transform() {
    let (mut world, mut s) = world(60);
    let (platform, e) = on_platform(&mut world, &mut s);
    let x0 = at(&world, e).translation.x;
    for _ in 0..60 {
      world.get_mut::<Transform>(platform).unwrap().translation.x += 2.0;
      step(&mut world, &mut s, e, 980.0);
      assert!(state(&world, e).on_floor);
    }
    let moved = at(&world, e).translation.x - x0;
    assert!((moved - 120.0).abs() < 2.0, "{moved}");
  }

  #[test]
  fn a_character_rides_a_platform_moving_up_and_down() {
    let (mut world, mut s) = world(60);
    let (platform, e) = on_platform(&mut world, &mut s);
    let mut landings = 0;
    for i in 0..120 {
      let dy = if (i / 30) % 2 == 0 { -60.0 } else { 60.0 };
      world.get_mut::<Velocity>(platform).unwrap().dy = dy;
      step(&mut world, &mut s, e, 980.0);
      let st = state(&world, e);
      assert!(st.on_floor, "step {i}");
      landings += usize::from(st.landed);
    }
    assert_eq!(landings, 0, "never left the platform");
  }

  #[test]
  fn a_velocity_driven_platform_carries_a_character() {
    let (mut world, mut s) = world(60);
    let (platform, e) = on_platform(&mut world, &mut s);
    world.get_mut::<Velocity>(platform).unwrap().dx = 60.0;
    let x0 = at(&world, e).translation.x;
    for _ in 0..60 {
      step(&mut world, &mut s, e, 980.0);
    }
    let moved = at(&world, e).translation.x - x0;
    assert!((moved - 60.0).abs() < 2.0, "{moved}");
    assert!((state(&world, e).platform_velocity.x - 60.0).abs() < 1.0);
  }

  #[test]
  fn a_character_rides_a_turning_platform() {
    let (mut world, mut s) = world(60);
    let platform = world
      .spawn((
        Transform::from_xy(0.0, 110.0),
        RigidBody::kinematic(),
        Collider::rect(300.0, 20.0),
      ))
      .id();
    let e = character(&mut world, 100.0, 70.0);
    for _ in 0..30 {
      step(&mut world, &mut s, e, 980.0);
    }
    let before = at(&world, e).translation;
    world.get_mut::<AngularVelocity>(platform).unwrap().0 = 0.5;
    for _ in 0..30 {
      step(&mut world, &mut s, e, 980.0);
    }
    let after = at(&world, e).translation;
    assert!(state(&world, e).on_floor);
    assert!(after.y > before.y + 5.0, "carried down with the turning platform: {before:?} -> {after:?}");
  }

  #[test]
  fn jumping_off_a_moving_platform_keeps_its_momentum() {
    let (mut world, mut s) = world(60);
    let (platform, e) = on_platform(&mut world, &mut s);
    world.get_mut::<Velocity>(platform).unwrap().dx = 120.0;
    for _ in 0..10 {
      step(&mut world, &mut s, e, 980.0);
    }
    world.get_mut::<Velocity>(e).unwrap().dy = -400.0;
    for _ in 0..5 {
      step(&mut world, &mut s, e, 980.0);
    }
    assert!(!state(&world, e).on_floor);
    assert!((velocity(&world, e).x - 120.0).abs() < 1.0, "{:?}", velocity(&world, e));
  }

  #[test]
  fn a_character_pushes_a_dynamic_crate_unless_told_not_to() {
    for push in [true, false] {
      let (mut world, mut s) = world(60);
      floor(&mut world, 100.0);
      let crate_ = world
        .spawn((
          Transform::from_xy(60.0, 90.0),
          RigidBody::dynamic().lock_rotation(),
          Collider::rect(20.0, 20.0).with_friction(0.2),
        ))
        .id();
      let e = world
        .spawn((
          Transform::from_xy(0.0, 79.0),
          CharacterController::platformer().with_push(push),
          Collider::rect(20.0, 40.0),
        ))
        .id();
      for _ in 0..90 {
        world.get_mut::<Velocity>(e).unwrap().dx = 150.0;
        step(&mut world, &mut s, e, 980.0);
      }
      let moved = at(&world, crate_).translation.x - 60.0;
      assert_eq!(moved > 20.0, push, "push {push}: crate moved {moved}");
    }
  }

  // Review focus 2
  #[test]
  fn a_character_whose_floor_is_despawned_falls() {
    let (mut world, mut s) = world(60);
    let (platform, e) = on_platform(&mut world, &mut s);
    world.despawn(platform);
    let y0 = at(&world, e).translation.y;
    for _ in 0..10 {
      step(&mut world, &mut s, e, 980.0);
    }
    let st = state(&world, e);
    assert!(!st.on_floor && st.floor.is_none());
    assert!(at(&world, e).translation.y > y0 + 5.0);
  }
```

(`AngularVelocity` comes from `crate::core::physics`.)

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p ryo2d --lib physics::character`
Expected: the carry, momentum and push tests FAIL (the character slides off or stays); the despawn test may already pass — keep it.

- [ ] **Step 3: Implement**

In `character.rs`:

```rust
impl Physics {
  /// How far the body under a character moves the point `at` this step:
  /// a kinematic body from its pose to its next pose (moved by `Transform`
  /// or velocity, turning included), a dynamic body by its velocity there,
  /// anything else (fixed, gone) not at all.
  pub(crate) fn floor_motion(&self, floor: Entity, at: Vec2, dt: f32) -> Vec2 {
    let Some(body) = self
      .body_entries
      .get(&floor)
      .and_then(|entry| self.bodies.get(entry.handle))
    else {
      return Vec2::ZERO;
    };
    if body.is_kinematic() {
      let point = to_rapier(at);
      let now = body.position();
      let next = body.next_position();
      from_rapier(next.transform_point(now.inverse().transform_point(point))) - at
    } else if body.is_dynamic() {
      from_rapier(body.velocity_at_point(to_rapier(at))) * dt
    } else {
      Vec2::ZERO
    }
  }
}
```

In `move_character`:
- Before the sweep: `let carry = match state.floor { Some(floor) if state.on_floor => self.floor_motion(floor, state.floor_point, dt), _ => Vec2::ZERO };` and `let desired = (wanted + extra) * dt + carry;`.
- After the floor is settled (replace `state.platform_velocity = Vec2::ZERO;`):

```rust
    state.platform_velocity = match state.floor {
      Some(floor) if state.on_floor && dt > 0.0 => self.floor_motion(floor, state.floor_point, dt) / dt,
      _ => Vec2::ZERO,
    };
```

  (compute it before `self.drive_to`, after `drop(queries)`).
- Pushing, after `drop(queries)` and before `drive_to`:

```rust
    if controller.push && !collisions.is_empty() {
      let mass = self
        .colliders
        .get(collider_handle)
        .map(|collider| collider.mass())
        .filter(|mass| mass.is_finite() && *mass > 0.0)
        .unwrap_or(1.0);
      let filter = RapierFilter::default()
        .exclude_rigid_body(body_handle)
        .exclude_sensors()
        .groups(groups);
      let dispatcher: &dyn QueryDispatcher = self.narrow_phase.query_dispatcher();
      let mut queries = self.broad_phase.as_query_pipeline_mut(
        dispatcher,
        &mut self.bodies,
        &mut self.colliders,
        filter,
      );
      kcc.solve_character_collision_impulses(dt, &mut queries, &*shape, mass, collisions.iter());
    }
```

(Verify `Collider::mass`, `RigidBody::next_position`, `velocity_at_point` and `BroadPhaseBvh::as_query_pipeline_mut` in rapier2d-0.36.1; `Pose` is glamx `Pose2` with `inverse` and `transform_point`. If the borrow checker rejects the field-disjoint borrows, bind `let Physics { bodies, colliders, broad_phase, narrow_phase, .. } = self;` first.)

Docs: the Characters section gains carry (from the floor body's pose to its next pose at the touched point, or a dynamic floor's velocity there), momentum on leaving, `platform_velocity`, and pushing (Rapier's approximate impulses with the collider's mass; kinematic and fixed bodies are never pushed).

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ryo2d --lib physics`
Expected: PASS.

- [ ] **Step 5: Gate and commit**

```bash
git add -A
git commit -m "feat: characters ride platforms and push bodies" -m "- carried by the floor's motion: moved by Transform or velocity, turning, or dynamic
- a moving floor's momentum kept on leaving it
- dynamic bodies pushed with Rapier's impulses"
```

---

### Task 6: One-way platforms for characters, drop-through

The sweep ignores a one-way collider unless the character started the step entirely on its solid side; `drop_through` lets it fall through the one underfoot.

**Files:**
- Modify: `src/core/physics/one_way.rs` (`character_above`, `character_below`)
- Modify: `src/core/physics/character.rs` (filter predicate, `floor_one_way`, drop-through; tests)
- Modify: `src/core/physics/AGENTS.md` (One-Way Colliders: characters)

**Interfaces:**
- Consumes: Task 3's `one_way::decode`, Task 4's `move_character`, `CharacterState::{floor_collider, drop_from}`.
- Produces: `one_way::character_above(collider: &RapierCollider, local: Vec2, aabb: &Aabb) -> bool`, `one_way::character_below(...) -> bool`; `CharacterMove::drop_done`; `move_characters` clears `CharacterController::drop_through` when a drop is over.

- [ ] **Step 1: Write the failing tests** (`character.rs` tests)

```rust
  fn one_way_platform(world: &mut World, top: f32) -> Entity {
    world
      .spawn((
        Transform::from_xy(0.0, top + 5.0),
        Collider::rect(200.0, 10.0).one_way(),
      ))
      .id()
  }

  #[test]
  fn a_character_jumps_up_through_a_one_way_platform_and_lands_on_it() {
    let (mut world, mut s) = world(60);
    floor(&mut world, 200.0);
    let platform = one_way_platform(&mut world, 100.0);
    let e = character(&mut world, 0.0, 179.0);
    for _ in 0..10 {
      step(&mut world, &mut s, e, 980.0);
    }
    world.get_mut::<Velocity>(e).unwrap().dy = -620.0;
    for _ in 0..90 {
      step(&mut world, &mut s, e, 980.0);
    }
    let st = state(&world, e);
    assert!(st.on_floor && st.floor_one_way);
    assert_eq!(st.floor, Some(platform));
    let bottom = at(&world, e).translation.y + 20.0;
    assert!((99.0..=100.0).contains(&bottom), "{bottom}");
  }

  #[test]
  fn a_character_passes_a_one_way_platform_from_the_side() {
    let (mut world, mut s) = world(60);
    one_way_platform(&mut world, 0.0);
    let e = character(&mut world, -150.0, 5.0);
    for _ in 0..60 {
      world.get_mut::<Velocity>(e).unwrap().dx = 300.0;
      run(&mut world, &mut s, 1);
    }
    assert!(at(&world, e).translation.x > 150.0);
  }

  #[test]
  fn a_character_walks_off_a_one_way_platform() {
    let (mut world, mut s) = world(60);
    one_way_platform(&mut world, 100.0);
    let e = character(&mut world, 0.0, 70.0);
    for _ in 0..90 {
      world.get_mut::<Velocity>(e).unwrap().dx = 200.0;
      step(&mut world, &mut s, e, 980.0);
    }
    assert!(!state(&world, e).on_floor);
    assert!(at(&world, e).translation.y > 120.0, "fell past the end");
  }

  #[test]
  fn drop_through_falls_through_the_platform_underfoot_and_clears() {
    let (mut world, mut s) = world(60);
    floor(&mut world, 300.0);
    one_way_platform(&mut world, 100.0);
    let e = character(&mut world, 0.0, 70.0);
    for _ in 0..30 {
      step(&mut world, &mut s, e, 980.0);
    }
    assert!(state(&world, e).floor_one_way);
    world.get_mut::<CharacterController>(e).unwrap().drop_through = true;
    for _ in 0..90 {
      step(&mut world, &mut s, e, 980.0);
    }
    assert!(at(&world, e).translation.y > 250.0, "on the floor below");
    assert!(!world.get::<CharacterController>(e).unwrap().drop_through);
  }

  #[test]
  fn drop_through_on_solid_ground_does_nothing_and_clears() {
    let (mut world, mut s) = world(60);
    floor(&mut world, 100.0);
    let e = character(&mut world, 0.0, 79.0);
    for _ in 0..10 {
      step(&mut world, &mut s, e, 980.0);
    }
    world.get_mut::<CharacterController>(e).unwrap().drop_through = true;
    step(&mut world, &mut s, e, 980.0);
    assert!(state(&world, e).on_floor);
    assert!(!world.get::<CharacterController>(e).unwrap().drop_through);
  }

  #[test]
  fn a_turned_one_way_platform_blocks_a_character_along_its_own_up() {
    let (mut world, mut s) = world(60);
    // Its up turned to +X: solid against what comes from the right
    world.spawn((
      Transform::IDENTITY.with_rotation(std::f32::consts::FRAC_PI_2),
      Collider::rect(200.0, 10.0).one_way(),
    ));
    let spawn = |world: &mut World, x: f32| {
      world
        .spawn((
          Transform::from_xy(x, 0.0),
          CharacterController::top_down(),
          Collider::circle(8.0),
        ))
        .id()
    };
    let from_right = spawn(&mut world, 60.0);
    let from_left = spawn(&mut world, -60.0);
    for _ in 0..30 {
      world.get_mut::<Velocity>(from_right).unwrap().dx = -300.0;
      world.get_mut::<Velocity>(from_left).unwrap().dx = 300.0;
      run(&mut world, &mut s, 1);
    }
    assert!(at(&world, from_right).translation.x > 10.0, "blocked");
    assert!(at(&world, from_left).translation.x > 60.0, "passed");
  }

  // Review focus 3
  #[test]
  fn a_fast_fall_lands_on_a_one_way_segment() {
    let (mut world, mut s) = world(60);
    world.spawn((
      Transform::IDENTITY,
      Collider::segment(Vec2::new(-100.0, 0.0), Vec2::new(100.0, 0.0)).one_way(),
    ));
    let e = character(&mut world, 0.0, -400.0);
    for _ in 0..30 {
      world.get_mut::<Velocity>(e).unwrap().dy = 3000.0;
      run(&mut world, &mut s, 1);
    }
    let bottom = at(&world, e).translation.y + 20.0;
    assert!((-1.0..=0.0).contains(&bottom), "{bottom}");
  }

  #[test]
  fn solid_ground_is_not_one_way() {
    let (mut world, mut s) = world(60);
    floor(&mut world, 100.0);
    let e = character(&mut world, 0.0, 79.0);
    for _ in 0..10 {
      step(&mut world, &mut s, e, 980.0);
    }
    let st = state(&world, e);
    assert!(st.on_floor && !st.floor_one_way);
  }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p ryo2d --lib physics::character`
Expected: the one-way tests FAIL (the platform blocks the jump from below; `floor_one_way` is false; `drop_through` is never cleared).

- [ ] **Step 3: Implement**

`one_way.rs`:

```rust
use rapier2d::parry::bounding_volume::Aabb;
use rapier2d::prelude::{Collider as RapierCollider, Vector};

/// How far a character may sit inside a one-way collider's top and still
/// count as above it, in px.
const ABOVE_TOLERANCE: f32 = 1.0;

/// The lowest and highest of `p · d` over a box's corners.
fn extent(corners: [Vector; 4], d: Vector) -> (f32, f32) {
  corners
    .iter()
    .map(|corner| corner.dot(d))
    .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), x| (lo.min(x), hi.max(x)))
}

fn corners(aabb: &Aabb) -> [Vector; 4] {
  [
    aabb.mins,
    Vector::new(aabb.maxs.x, aabb.mins.y),
    aabb.maxs,
    Vector::new(aabb.mins.x, aabb.maxs.y),
  ]
}

/// A one-way collider's world direction and its extent along it (its own
/// box, turned with it).
fn collider_extent(collider: &RapierCollider, local: Vec2) -> (Vector, (f32, f32)) {
  let pose = collider.position();
  let d = pose.rotation.transform_vector(to_rapier(local));
  let own = collider.shape().compute_local_aabb();
  let world = corners(&own).map(|corner| pose.transform_point(corner));
  (d, extent(world, d))
}

/// Whether a character whose shape filled `aabb` is entirely on a one-way
/// collider's solid side (above it), within `ABOVE_TOLERANCE`.
pub(crate) fn character_above(collider: &RapierCollider, local: Vec2, aabb: &Aabb) -> bool {
  let (d, (_, top)) = collider_extent(collider, local);
  let (bottom, _) = extent(corners(aabb), d);
  bottom >= top - ABOVE_TOLERANCE
}

/// Whether a character whose shape fills `aabb` is entirely past a one-way
/// collider, below its lowest point.
pub(crate) fn character_below(collider: &RapierCollider, local: Vec2, aabb: &Aabb) -> bool {
  let (d, (low, _)) = collider_extent(collider, local);
  let (_, high) = extent(corners(aabb), d);
  high < low
}
```

`character.rs` `move_character`:
- Before building the filter:

```rust
    let start_aabb = shape.compute_aabb(&start);
    let dropping = controller.drop_through;
    let one_way_rule = |_: ColliderHandle, collider: &RapierCollider| match one_way::decode(collider.user_data) {
      None => true,
      Some(_) if dropping => false,
      Some(local) => one_way::character_above(collider, local, &start_aabb),
    };
```

  and add `.predicate(&one_way_rule)` to both filters (the sweep's and the push's).
- In the floor match: `state.floor_one_way = self.colliders.get(handle).is_some_and(|collider| one_way::decode(collider.user_data).is_some());` (and `false` when off the floor).
- Drop-through bookkeeping, after the floor is settled:

```rust
    let mut drop_done = false;
    if dropping {
      if state.drop_from.is_none() {
        state.drop_from = state.floor_collider.filter(|_| state.floor_one_way);
      }
      let end_aabb = shape.compute_aabb(&pose);
      let still_over = state
        .drop_from
        .and_then(|from| self.collider_entries.get(&from))
        .and_then(|entry| self.colliders.get(entry.handle))
        .and_then(|collider| {
          one_way::decode(collider.user_data)
            .map(|local| !one_way::character_below(collider, local, &end_aabb))
        })
        .unwrap_or(false);
      if !still_over {
        drop_done = true;
        state.drop_from = None;
      }
    }
```

  Note the order: `drop_from` is taken from the floor the character stood on at the start of the drop, so record it before the floor fields are overwritten by this step's probe (keep the previous `floor_collider`/`floor_one_way` in locals at the top of the function and use them here). Return `drop_done` in `CharacterMove`.
- `move_characters`: `if moved.drop_done { controller.drop_through = false; }` (the query holds `&mut CharacterController` already).

(Imports: `ColliderHandle`, `Collider as RapierCollider` from `rapier2d::prelude`; `super::one_way`. Verify `Shape::compute_aabb(&Pose) -> Aabb`, `compute_local_aabb`, and `rapier2d::parry::bounding_volume::Aabb { mins, maxs }`.)

Docs: One-Way Colliders gains the character rule (blocked only when the character started the step entirely on the solid side, within 1 px; drop-through ignores one-way colliders until the character is entirely below the one it stood on, then the engine clears the flag), and `CharacterState::floor_one_way`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ryo2d --lib physics`
Expected: PASS.

- [ ] **Step 5: Gate and commit**

```bash
git add -A
git commit -m "feat: one-way platforms for characters" -m "- a one-way collider blocks a character only from its solid side
- drop_through falls through the platform underfoot and clears itself"
```

---

### Task 7: Knockback through physics; the gameplay module

`Knockback` acts through physics per target: a character's sweep (stopped and clipped by surfaces), one change in velocity for a dynamic body, nothing for kinematic and fixed bodies (warned), `Transform` for bodiless entities. Its system moves into the new `gameplay` module, which runs in `FixedSet::Movement`.

**Files:**
- Create: `src/core/gameplay/mod.rs`, `src/core/gameplay/knockback.rs`, `src/core/gameplay/test_util.rs`
- Delete: `src/core/systems/knockback.rs`; Modify: `src/core/systems/mod.rs` (drop the module, its export and its place in `register_juice_systems`)
- Modify: `src/core/ecs/knockback.rs` (drop `applied`, `retarget`, the `on_discard` hook and their docs; add `clip`)
- Modify: `src/core/physics/character.rs` (the knockback term and clipping)
- Modify: `src/core/mod.rs` (`pub mod gameplay;`), `src/core/engine/mod.rs` (`register_gameplay_systems` after `register_physics_systems`)
- Modify: `tests/integration/knockback.rs`, `tests/integration/support.rs` (a world with the gameplay systems)
- Modify: `examples/demos/knockback/main.rs` (a character target stopped by walls; dynamic crates of different mass)
- Create: `src/core/gameplay/AGENTS.md`; Modify: `src/core/AGENTS.md` (module list, the `Movement` set)

**Interfaces:**
- Consumes: Task 4's `move_characters`/`move_character(extra)` and `CharacterMove::normals`.
- Produces: `pub fn register_gameplay_systems(schedules: &mut EngineSchedules)` (adds the gameplay chain to `FixedSet::Movement`; later tasks extend the chain in this order: `place_respawn_points`, `warn_untriggered`, `collect_pickups`, `respawn_pickups`, `apply_kill_zones`, `apply_checkpoints`, `read_move_bindings`, `update_knockback`, `platformer_movement`, `top_down_movement`); `pub fn update_knockback(...)`; `Knockback::clip(&mut self, normal: Vec2)` (crate-internal); `gameplay::test_util::{world, player}`.

- [ ] **Step 1: Write the failing tests**

`src/core/gameplay/test_util.rs`:

```rust
//! A physics world with the gameplay systems, for the gameplay tests.

use crate::core::ecs::{Entity, Transform, World};
use crate::core::input::Input;
use crate::core::physics::{CharacterController, Collider};
use crate::core::schedule::EngineSchedules;

pub(crate) use crate::core::physics::test_util::{at, floor, no_gravity, run};

pub(crate) fn world(hz: u32) -> (World, EngineSchedules) {
  let (mut world, mut schedules) = crate::core::physics::test_util::world(hz);
  world.insert_resource(Input::new());
  super::register_gameplay_systems(&mut schedules);
  (world, schedules)
}

/// A 20×40 platformer character centred at (x, y).
pub(crate) fn player(world: &mut World, x: f32, y: f32) -> Entity {
  world
    .spawn((
      Transform::from_xy(x, y),
      CharacterController::platformer(),
      Collider::rect(20.0, 40.0),
    ))
    .id()
}
```

`src/core/gameplay/knockback.rs` tests: move every test from `systems/knockback.rs`, using `crate::core::systems::test_util::{messages, run, world}` as before for the single-system ones, with these changes:
- `knockback_adds_to_a_dynamic_body_s_velocity` becomes:

```rust
  #[test]
  fn a_dynamic_body_takes_the_push_once_as_a_change_in_velocity() {
    let mut world = world(60);
    let entity = falling_body(&mut world);
    run(&mut world, update_knockback);
    assert_eq!(*world.get::<Velocity>(entity).unwrap(), Velocity::new(100.0, 300.0));
    run(&mut world, update_knockback);
    assert_eq!(
      *world.get::<Velocity>(entity).unwrap(),
      Velocity::new(100.0, 300.0),
      "physics slows it from here"
    );
  }
```

- Delete `a_finished_knockback_takes_back_the_velocity_it_added`, `a_push_a_wall_absorbed_is_not_given_back`, `a_stopped_knockback_takes_back_what_it_added`, `removing_a_knockback_takes_back_what_it_added` (the take-back is gone).
- Add:

```rust
  #[test]
  fn knockback_on_a_kinematic_or_fixed_body_does_nothing_and_warns_once() {
    let mut world = world(60);
    let mut schedule = crate::core::ecs::Schedule::new(Probe);
    schedule.add_systems(update_knockback);
    let kinematic = world
      .spawn((Transform::IDENTITY, RigidBody::kinematic(), Knockback::new(Vec2::X, 100.0)))
      .id();
    let fixed = world
      .spawn((Transform::IDENTITY, RigidBody::fixed(), Knockback::new(Vec2::X, 100.0)))
      .id();
    for _ in 0..3 {
      schedule.run(&mut world);
    }
    for entity in [kinematic, fixed] {
      assert_eq!(world.get::<Transform>(entity).unwrap().translation, Vec2::ZERO);
      assert_eq!(*world.get::<Velocity>(entity).unwrap(), Velocity::zero());
    }
  }
```

  (`#[derive(ScheduleLabel, Debug, Clone, Copy, PartialEq, Eq, Hash)] struct Probe;` in the tests module, so the `Local` warning set persists across runs; the warning count is not observable here, so the test pins the behaviour.)
- In `ecs/knockback.rs` tests add:

```rust
  #[test]
  fn clipping_drops_the_part_running_into_a_surface() {
    let mut kb = Knockback::new(Vec2::new(1.0, 1.0), 100.0);
    kb.clip(Vec2::NEG_X); // a wall on the right, its normal pointing left
    let (vx, vy) = kb.current_velocity();
    assert!(vx.abs() < 1e-4 && vy > 0.0, "({vx}, {vy})");
    let mut head_on = Knockback::new(Vec2::X, 100.0);
    head_on.clip(Vec2::NEG_X);
    assert!(head_on.is_finished());
  }
```

Character tests in `gameplay/knockback.rs` (a second tests module using `gameplay::test_util`):

```rust
#[cfg(test)]
mod with_physics {
  use crate::core::ecs::{Knockback, Transform, Vec2};
  use crate::core::gameplay::test_util::*;
  use crate::core::physics::Collider;

  #[test]
  fn knockback_carries_a_character_in_the_open() {
    let (mut world, mut s) = world(60);
    no_gravity(&mut world);
    let e = player(&mut world, 0.0, 0.0);
    world.entity_mut(e).insert(Knockback::new(Vec2::X, 400.0));
    run(&mut world, &mut s, 60);
    let x = at(&world, e).translation.x;
    assert!(x > 20.0, "{x}");
  }

  #[test]
  fn a_character_knocked_into_a_wall_stops_at_it() {
    let (mut world, mut s) = world(60);
    no_gravity(&mut world);
    world.spawn((Transform::from_xy(60.0, 0.0), Collider::rect(20.0, 400.0)));
    let e = player(&mut world, 0.0, 0.0);
    world.entity_mut(e).insert(Knockback::new(Vec2::X, 2000.0).with_decay_rate(1.0));
    run(&mut world, &mut s, 30);
    assert!(at(&world, e).translation.x + 10.0 <= 50.5);
    let kb = world.get::<Knockback>(e).unwrap();
    assert!(kb.is_finished() || kb.direction().x <= 1e-4, "clipped: {:?}", kb.direction());
  }
}
```

(`no_gravity` only affects dynamic bodies; the character has no gravity unless a game or preset adds it.)

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p ryo2d --lib knockback`
Expected: compile errors (`gameplay` module, `Knockback::clip`).

- [ ] **Step 3: Implement**

`ecs/knockback.rs`: remove `retarget`, `take_back_on_discard`, the `#[component(on_discard = ...)]` attribute, the `applied` field (and its `Default`/constructor entries), and the "Physics Integration" doc section; replace it with:

```rust
//! # Physics Integration
//!
//! What the push does depends on the entity: a character
//! (`CharacterController`) is swept by it, so walls, floors and ceilings
//! stop it and the part running into them is dropped; a dynamic body takes
//! it as one change in velocity when it starts, and physics slows it from
//! there; a kinematic body without a controller, or a fixed body, ignores
//! it (logged once); an entity without a body moves through its `Transform`.
```

and add:

```rust
  /// Drop the part of the push running into a surface whose normal (out of
  /// the surface) is `normal`; a push straight into it stops.
  pub(crate) fn clip(&mut self, normal: Vec2) {
    if !self.active {
      return;
    }
    let (vx, vy) = self.current_velocity();
    let push = Vec2::new(vx, vy);
    let into = push.dot(normal);
    if into >= 0.0 {
      return;
    }
    let rest = push - normal * into;
    match rest.try_normalize() {
      Some(direction) if rest.length() > 1e-3 => {
        self.force *= rest.length() / push.length();
        self.direction_x = direction.x;
        self.direction_y = direction.y;
      }
      _ => self.stop(),
    }
  }
```

`gameplay/knockback.rs`:

```rust
//! Knockback: decay each `Knockback` and apply it the way its entity moves.

use crate::core::ecs::{
  Entity, EntityHashSet, Has, Knockback, Local, MessageWriter, Query, Res, Transform, Vec2,
  Velocity,
};
use crate::core::messages::{KnockbackFinished, KnockbackStarted};
use crate::core::physics::{BodyKind, CharacterController, RigidBody};
use crate::core::time::Time;

type Pushed = (
  Entity,
  &'static mut Knockback,
  Option<&'static mut Velocity>,
  Option<&'static RigidBody>,
  Has<CharacterController>,
  Option<&'static mut Transform>,
);

/// `FixedSet::Movement`: decay every active `Knockback` by one fixed step
/// and apply it. A character is swept by it during the physics step (see
/// `Knockback`); a dynamic body gets it as one change in velocity when it
/// starts; a kinematic body without a controller and a fixed body ignore it
/// (logged once); anything else moves through its `Transform`.
pub fn update_knockback(
  time: Res<Time>,
  mut query: Query<Pushed>,
  mut started: MessageWriter<KnockbackStarted>,
  mut finished: MessageWriter<KnockbackFinished>,
  mut warned: Local<EntityHashSet>,
) {
  let dt = time.fixed.timestep();
  for (entity, mut knockback, velocity, body, character, transform) in &mut query {
    if !knockback.active {
      continue;
    }
    let just_started = knockback.just_started;
    let direction = knockback.direction();
    let force = knockback.force();
    knockback.update(dt);
    if just_started {
      started.write(KnockbackStarted { entity });
    }
    if knockback.just_finished {
      finished.write(KnockbackFinished { entity });
    }
    if character {
      continue;
    }
    match body.map(|body| body.kind) {
      Some(BodyKind::Dynamic) => {
        if just_started && let Some(mut velocity) = velocity {
          velocity.dx += direction.x * force;
          velocity.dy += direction.y * force;
        }
      }
      Some(BodyKind::Kinematic | BodyKind::Fixed) => {
        if warned.insert(entity) {
          crate::engine_warn!(
            module: "Gameplay",
            "{entity}: Knockback on a kinematic body without a CharacterController, or on a fixed body, does nothing"
          );
        }
      }
      None => {
        if let Some(mut transform) = transform {
          let (dx, dy) = knockback.current_velocity();
          transform.translation += Vec2::new(dx, dy) * dt;
        }
      }
    }
  }
}
```

(Add `Local` to the kernel re-exports if it is not there: `pub use bevy_ecs::system::Local;`.)

`gameplay/mod.rs`:

```rust
//! Gameplay on top of physics: movement presets and the input that drives
//! them, trigger behaviours and knockback. Nothing here names Rapier; this
//! module depends on `physics`, never the reverse.

mod knockback;
#[cfg(test)]
pub(crate) mod test_util;

pub use knockback::update_knockback;

use crate::core::ecs::IntoScheduleConfigs;
use crate::core::schedule::{EngineSchedules, FixedSet, Phase};

/// Add the gameplay systems to `FixedSet::Movement`, chained.
pub fn register_gameplay_systems(schedules: &mut EngineSchedules) {
  schedules.add_systems(
    Phase::FixedUpdate,
    (update_knockback,).chain().in_set(FixedSet::Movement),
  );
}
```

`character.rs` `move_characters`: the query gains `Option<&mut Knockback>`; pass `extra = knockback.as_ref().filter(|kb| kb.active).map_or(Vec2::ZERO, |kb| { let (x, y) = kb.current_velocity(); Vec2::new(x, y) })`, and after the move `if let Some(mut kb) = knockback { for normal in &moved.normals { kb.clip(*normal); } }`.

Engine: `register_gameplay_systems(schedules)` in `register_engine_systems` right after `register_physics_systems(schedules)`. `systems/mod.rs` loses `knockback`. `tests/integration/support.rs` gains `pub fn gameplay_world(hz: u32) -> (World, EngineSchedules)`: a `World` with `Time::new(hz)`, `Input::new()` and `register_engine_messages`, and `register_gameplay_systems` on fresh `EngineSchedules` (physics' registration is crate-internal, and knockback on bodiless entities and dynamic bodies' `Velocity` needs no simulation). `tests/integration/knockback.rs` moves to it, drops the take-back tests and asserts the new dynamic-body rule (one change in velocity at the start). Later tasks that add gameplay systems keep this world working: every gameplay system's resources are `Time`, `Input` and messages.

Knockback demo: keep its page; its targets become a character (`CharacterController::platformer()`, `Collider`, the page adds 980 px/s² to its `Velocity` each update) in an arena of fixed walls, so a hit near a wall stops at it, and two dynamic crates of different `with_density` that leave at the same speed; update the description. Check it headless (`RYO_HEADLESS=1 RYO_CAPTURE_FRAMES=120 RYO_CAPTURE_EXIT=1 RYO_CAPTURE_DIR=<abs dir> cargo run -p knockback`) and look at the PNG.

`src/core/gameplay/AGENTS.md` (new): the module's purpose and dependency rule, the `Movement` set and its chain order (filled in by later tasks), knockback per target, test helpers. `src/core/AGENTS.md`: list `gameplay/`; the fixed step gains `Movement`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ryo2d --lib knockback gameplay physics` and `cargo test --test integration knockback`
Expected: PASS.

- [ ] **Step 5: Gate and commit**

```bash
git add -A
git commit -m "feat: knockback through physics" -m "- characters are swept by it and stopped by walls; dynamic bodies take it as one change in velocity
- kinematic and fixed bodies ignore it with a warning; bodiless entities move by Transform
- its system moves to the new gameplay module, in FixedSet::Movement"
```

---

### Task 8: `MoveIntent` and `MoveBindings`

The one input every preset reads, and the optional component that fills it from the keyboard and gamepad.

**Files:**
- Create: `src/core/gameplay/intent.rs`
- Modify: `src/core/gameplay/mod.rs` (module, exports, `read_move_bindings` in the chain before `update_knockback`), `src/lib.rs` (prelude)
- Modify: `src/core/gameplay/AGENTS.md`

**Interfaces:**
- Produces: `pub struct MoveIntent { pub direction: Vec2, pub jump_held: bool, pub jump_pressed: bool, pub dash_pressed: bool }` (`Component`, `Copy`, `Default`, `PartialEq`) with `pub fn clamped_direction(&self) -> Vec2`; `pub struct MoveBindings { pub left, pub right, pub up, pub down, pub jump, pub dash: Vec<KeyCode>, pub gamepad: bool }` (requires `MoveIntent`) with `Default`, `with_left/with_right/with_up/with_down/with_jump/with_dash(keys)`, `keyboard_only()`; `pub fn read_move_bindings(input: Res<Input>, bound: Query<(&MoveBindings, &mut MoveIntent)>)`.

- [ ] **Step 1: Write the failing tests** (`intent.rs`)

```rust
#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ecs::World;
  use crate::core::input::InputEvent;
  use crate::core::systems::test_util::run;

  fn bound(world: &mut World) -> crate::core::ecs::Entity {
    world.spawn(MoveBindings::default()).id()
  }

  fn press(world: &mut World, key: KeyCode) {
    world
      .resource_mut::<Input>()
      .queue(InputEvent::Key { key, pressed: true });
  }

  fn intent(world: &World, e: crate::core::ecs::Entity) -> MoveIntent {
    *world.get::<MoveIntent>(e).unwrap()
  }

  fn world() -> World {
    let mut world = World::new();
    world.insert_resource(Input::new());
    world
  }

  #[test]
  fn wasd_and_arrows_move() {
    let mut world = world();
    let e = bound(&mut world);
    press(&mut world, KeyCode::KeyD);
    press(&mut world, KeyCode::ArrowUp);
    world.resource_mut::<Input>().begin_frame();
    world.resource_mut::<Input>().set_fixed_step(0);
    run(&mut world, read_move_bindings);
    assert_eq!(intent(&world, e).direction, Vec2::new(1.0, -1.0));
  }

  #[test]
  fn a_press_shows_in_one_fixed_step_and_the_hold_in_every_one() {
    let mut world = world();
    let e = bound(&mut world);
    press(&mut world, KeyCode::Space);
    world.resource_mut::<Input>().begin_frame();
    world.resource_mut::<Input>().set_fixed_step(0);
    run(&mut world, read_move_bindings);
    assert!(intent(&world, e).jump_pressed && intent(&world, e).jump_held);
    world.resource_mut::<Input>().set_fixed_step(1);
    run(&mut world, read_move_bindings);
    assert!(!intent(&world, e).jump_pressed && intent(&world, e).jump_held);
  }

  #[test]
  fn a_press_in_a_frame_without_a_fixed_step_reaches_the_next_one() {
    let mut world = world();
    let e = bound(&mut world);
    press(&mut world, KeyCode::ShiftLeft);
    world.resource_mut::<Input>().begin_frame();
    world.resource_mut::<Input>().end_fixed_steps(false);
    world.resource_mut::<Input>().begin_frame();
    world.resource_mut::<Input>().set_fixed_step(0);
    run(&mut world, read_move_bindings);
    assert!(intent(&world, e).dash_pressed);
  }

  #[test]
  fn rebound_keys_replace_the_defaults() {
    let mut world = world();
    let e = world
      .spawn(MoveBindings::default().with_jump([KeyCode::KeyK]))
      .id();
    press(&mut world, KeyCode::Space);
    world.resource_mut::<Input>().begin_frame();
    world.resource_mut::<Input>().set_fixed_step(0);
    run(&mut world, read_move_bindings);
    assert!(!intent(&world, e).jump_pressed, "Space no longer jumps");
    world.resource_mut::<Input>().end_fixed_steps(true);
    press(&mut world, KeyCode::KeyK);
    world.resource_mut::<Input>().begin_frame();
    world.resource_mut::<Input>().set_fixed_step(0);
    run(&mut world, read_move_bindings);
    assert!(intent(&world, e).jump_pressed, "K does");
  }

  #[test]
  fn the_direction_is_clamped_to_length_one_and_nan_is_zero() {
    let at = |direction| MoveIntent { direction, ..MoveIntent::default() }.clamped_direction();
    assert_eq!(at(Vec2::new(5.0, 0.0)), Vec2::X);
    assert!((at(Vec2::ONE).length() - 1.0).abs() < 1e-6);
    assert_eq!(at(Vec2::new(0.5, 0.0)), Vec2::new(0.5, 0.0));
    assert_eq!(at(Vec2::new(f32::NAN, 1.0)), Vec2::ZERO);
  }
}
```

(Check `Input::queue`, `begin_frame`, `set_fixed_step` and `end_fixed_steps` are reachable from `crate::core::input` tests; they are `pub`.)

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p ryo2d --lib gameplay::intent`
Expected: compile errors.

- [ ] **Step 3: Implement** (`intent.rs`)

```rust
//! Movement input: `MoveIntent`, which every preset reads, and
//! `MoveBindings`, which fills it from the keyboard and the gamepad.

use winit::keyboard::KeyCode;

use crate::core::ecs::{Component, Query, Res, Vec2};
use crate::core::input::{GamepadButton, Input};

/// What a character wants to do this step. The movement presets read it;
/// `MoveBindings` fills it from input, an AI or networking can write it
/// instead.
#[derive(Component, Debug, Clone, Copy, Default, PartialEq)]
pub struct MoveIntent {
  /// Where to move, -1 to 1 per axis (+Y down), analog from sticks.
  /// Presets clamp it to length 1 and read a non-finite one as zero.
  pub direction: Vec2,
  /// Jump is held (a jump released early is cut short)
  pub jump_held: bool,
  /// Jump was pressed; the presets act on it and clear it
  pub jump_pressed: bool,
  /// Dash was pressed; the presets act on it and clear it
  pub dash_pressed: bool,
}

impl MoveIntent {
  /// The direction clamped to length 1; zero when it is not finite.
  pub fn clamped_direction(&self) -> Vec2 {
    if self.direction.is_finite() {
      self.direction.clamp_length_max(1.0)
    } else {
      Vec2::ZERO
    }
  }
}

/// Fill the entity's `MoveIntent` from the keyboard (and the first gamepad)
/// every fixed step. A press is seen in the first fixed step after it, once,
/// even in a frame that runs no fixed step (`Input`'s fixed view).
#[derive(Component, Debug, Clone, PartialEq)]
#[non_exhaustive]
#[require(MoveIntent)]
pub struct MoveBindings {
  pub left: Vec<KeyCode>,
  pub right: Vec<KeyCode>,
  pub up: Vec<KeyCode>,
  pub down: Vec<KeyCode>,
  pub jump: Vec<KeyCode>,
  pub dash: Vec<KeyCode>,
  /// Also read the first connected gamepad: the left stick and d-pad move,
  /// South jumps, West dashes
  pub gamepad: bool,
}

impl Default for MoveBindings {
  /// WASD and the arrows move, Space jumps, Shift dashes, plus the gamepad.
  fn default() -> Self {
    Self {
      left: vec![KeyCode::KeyA, KeyCode::ArrowLeft],
      right: vec![KeyCode::KeyD, KeyCode::ArrowRight],
      up: vec![KeyCode::KeyW, KeyCode::ArrowUp],
      down: vec![KeyCode::KeyS, KeyCode::ArrowDown],
      jump: vec![KeyCode::Space],
      dash: vec![KeyCode::ShiftLeft, KeyCode::ShiftRight],
      gamepad: true,
    }
  }
}

impl MoveBindings {
  pub fn with_left(mut self, keys: impl IntoIterator<Item = KeyCode>) -> Self {
    self.left = keys.into_iter().collect();
    self
  }

  pub fn with_right(mut self, keys: impl IntoIterator<Item = KeyCode>) -> Self {
    self.right = keys.into_iter().collect();
    self
  }

  pub fn with_up(mut self, keys: impl IntoIterator<Item = KeyCode>) -> Self {
    self.up = keys.into_iter().collect();
    self
  }

  pub fn with_down(mut self, keys: impl IntoIterator<Item = KeyCode>) -> Self {
    self.down = keys.into_iter().collect();
    self
  }

  pub fn with_jump(mut self, keys: impl IntoIterator<Item = KeyCode>) -> Self {
    self.jump = keys.into_iter().collect();
    self
  }

  pub fn with_dash(mut self, keys: impl IntoIterator<Item = KeyCode>) -> Self {
    self.dash = keys.into_iter().collect();
    self
  }

  /// Ignore the gamepad (a second keyboard player, say).
  pub fn keyboard_only(mut self) -> Self {
    self.gamepad = false;
    self
  }
}

/// `FixedSet::Movement`: fill every bound `MoveIntent` from this step's input.
pub fn read_move_bindings(input: Res<Input>, mut bound: Query<(&MoveBindings, &mut MoveIntent)>) {
  for (bindings, mut intent) in &mut bound {
    let down = |keys: &[KeyCode]| keys.iter().any(|&key| input.key_down(key));
    let pressed = |keys: &[KeyCode]| keys.iter().any(|&key| input.key_pressed(key));
    let pad = bindings.gamepad;
    let held = |button| pad && input.gamepad_button_down(button);
    let tapped = |button| pad && input.gamepad_button_pressed(button);
    let axis = |negative: bool, positive: bool| f32::from(u8::from(positive)) - f32::from(u8::from(negative));
    let (stick_x, stick_y) = if pad { input.left_stick() } else { (0.0, 0.0) };
    let x = axis(
      down(&bindings.left) || held(GamepadButton::DPadLeft),
      down(&bindings.right) || held(GamepadButton::DPadRight),
    ) + stick_x;
    // Sticks report +Y up; the world is +Y down
    let y = axis(
      down(&bindings.up) || held(GamepadButton::DPadUp),
      down(&bindings.down) || held(GamepadButton::DPadDown),
    ) - stick_y;
    intent.set_if_neq(MoveIntent {
      direction: Vec2::new(x.clamp(-1.0, 1.0), y.clamp(-1.0, 1.0)),
      jump_held: down(&bindings.jump) || held(GamepadButton::South),
      jump_pressed: pressed(&bindings.jump) || tapped(GamepadButton::South),
      dash_pressed: pressed(&bindings.dash) || tapped(GamepadButton::West),
    });
  }
}
```

Register `read_move_bindings` before `update_knockback` in `register_gameplay_systems`; export both types and the system in the prelude. `gameplay/AGENTS.md`: input (fixed view, no latch; bindings assign the intent every step; presets clear the pressed flags for intents written by code).

Remove the leftover `space_only` lines from `rebound_keys_replace_the_defaults` if clippy flags them; the test's point is that `KeyK` now jumps.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ryo2d --lib gameplay`
Expected: PASS.

- [ ] **Step 5: Gate and commit**

```bash
git add -A
git commit -m "feat: MoveIntent and MoveBindings" -m "- the movement input every preset reads
- bindings fill it from the keyboard and the first gamepad each fixed step"
```

---

### Task 9: `PlatformerMovement`: run, jump, fall, forgiveness

**Files:**
- Create: `src/core/gameplay/platformer.rs`, `src/core/gameplay/events.rs`
- Modify: `src/core/gameplay/mod.rs` (modules, exports, `platformer_movement` after `update_knockback`), `src/core/messages/mod.rs` (register `MovementEvent`), `src/lib.rs`
- Modify: `src/core/gameplay/AGENTS.md`

**Interfaces:**
- Consumes: `CharacterController`/`CharacterMode`/`CharacterState` (Task 4, 6), `MoveIntent` (Task 8), `Knockback`.
- Produces:
  - `pub struct PlatformerMovement { pub run_speed, pub accel_time, pub decel_time, pub air_control, pub jump_height, pub time_to_apex, pub fall_multiplier, pub max_fall_speed, pub coyote_time, pub jump_buffer, pub jump_cut, pub knockback_control: f32 }` (requires `MoveIntent`, `PlatformerState`, `CharacterController::platformer()`), `Default` and builders `with_run(speed, accel_time, decel_time)`, `with_air_control(fraction)`, `with_jump(height, time_to_apex)`, `with_fall(multiplier, max_speed)`, `with_coyote_time(s)`, `with_jump_buffer(s)`, `with_jump_cut(factor)`, `with_knockback_control(fraction)`.
  - `pub struct PlatformerState { pub facing: f32, pub jumping: bool, .. }` with `pub fn reset(&mut self)`.
  - `pub struct MovementEvent { pub entity: Entity, pub kind: MovementEventKind }`, `pub enum MovementEventKind { Jumped, AirJumped, WallJumped, Dashed, Landed }` (`#[non_exhaustive]`).
  - `pub(crate) fn jump_physics(height: f32, time: f32, dt: f32) -> (f32, f32)` (gravity, launch speed); `pub fn platformer_movement(...)`.

- [ ] **Step 1: Write the failing tests** (`platformer.rs`, using `gameplay::test_util`)

```rust
#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ecs::{Messages, Transform, World};
  use crate::core::gameplay::test_util::*;
  use crate::core::physics::Collider;
  use crate::core::schedule::EngineSchedules;

  fn runner(world: &mut World, settings: PlatformerMovement) -> Entity {
    let e = player(world, 0.0, 79.0);
    world.entity_mut(e).insert(settings);
    e
  }

  fn intent(world: &mut World, e: Entity) -> crate::core::ecs::Mut<'_, MoveIntent> {
    world.get_mut::<MoveIntent>(e).unwrap()
  }

  fn events(world: &World) -> Vec<MovementEventKind> {
    let messages = world.resource::<Messages<MovementEvent>>();
    messages.get_cursor().read(messages).map(|m| m.kind).collect()
  }

  fn settle(world: &mut World, s: &mut EngineSchedules) {
    run(world, s, 30);
  }

  #[test]
  fn the_jump_reaches_its_height_at_its_apex_time() {
    let (mut world, mut s) = world(60);
    floor(&mut world, 100.0);
    let e = runner(&mut world, PlatformerMovement::default().with_jump(64.0, 0.38));
    settle(&mut world, &mut s);
    let start = at(&world, e).translation.y;
    intent(&mut world, e).jump_pressed = true;
    intent(&mut world, e).jump_held = true;
    let (mut top, mut top_step) = (start, 0);
    for i in 1..=60 {
      intent(&mut world, e).jump_held = true;
      run(&mut world, &mut s, 1);
      let y = at(&world, e).translation.y;
      if y < top {
        (top, top_step) = (y, i);
      }
    }
    assert!((start - top - 64.0).abs() <= 1.0, "height {}", start - top);
    assert!((top_step as f32 - 0.38 * 60.0).abs() <= 1.0, "apex at step {top_step}");
  }

  #[test]
  fn running_reaches_full_speed_in_its_acceleration_time() {
    let (mut world, mut s) = world(60);
    floor(&mut world, 100.0);
    let e = runner(&mut world, PlatformerMovement::default().with_run(180.0, 0.1, 0.05));
    settle(&mut world, &mut s);
    for _ in 0..6 {
      intent(&mut world, e).direction = Vec2::X;
      run(&mut world, &mut s, 1);
    }
    assert!((world.get::<Velocity>(e).unwrap().dx - 180.0).abs() < 1e-3);
  }

  /// Walk right off a platform ending at x = 100; press jump `late` steps
  /// after leaving it. Whether it jumped.
  fn coyote_jump(late: usize) -> bool {
    let (mut world, mut s) = world(60);
    world.spawn((Transform::from_xy(0.0, 110.0), Collider::rect(200.0, 20.0)));
    let e = runner(&mut world, PlatformerMovement::default());
    settle(&mut world, &mut s);
    let mut since = None;
    for _ in 0..120 {
      intent(&mut world, e).direction = Vec2::X;
      if since == Some(late) {
        intent(&mut world, e).jump_pressed = true;
        intent(&mut world, e).jump_held = true;
      }
      run(&mut world, &mut s, 1);
      let st = world.get::<CharacterState>(e).unwrap();
      if since.is_none() && st.left_floor {
        since = Some(0);
      } else if let Some(n) = since.as_mut() {
        *n += 1;
      }
    }
    events(&world).contains(&MovementEventKind::Jumped)
  }

  #[test]
  fn coyote_time_allows_a_late_jump_within_its_window_only() {
    assert!(coyote_jump(3), "0.05 s after the edge");
    assert!(!coyote_jump(9), "0.15 s after the edge");
  }

  /// Drop onto the floor, pressing jump `early` steps before landing.
  fn buffered_jump(early: usize) -> bool {
    let landing = {
      let (mut world, mut s) = world(60);
      floor(&mut world, 100.0);
      let e = player(&mut world, 0.0, -100.0);
      world.entity_mut(e).insert(PlatformerMovement::default());
      (1..200)
        .find(|_| {
          run(&mut world, &mut s, 1);
          world.get::<CharacterState>(e).unwrap().landed
        })
        .unwrap_or(0)
    };
    let (mut world, mut s) = world(60);
    floor(&mut world, 100.0);
    let e = player(&mut world, 0.0, -100.0);
    world.entity_mut(e).insert(PlatformerMovement::default());
    for step in 1..landing + 10 {
      if step + early == landing {
        intent(&mut world, e).jump_pressed = true;
        intent(&mut world, e).jump_held = true;
      }
      run(&mut world, &mut s, 1);
    }
    events(&world).contains(&MovementEventKind::Jumped)
  }

  #[test]
  fn a_jump_pressed_just_before_landing_is_kept() {
    assert!(buffered_jump(3), "0.05 s early");
    assert!(!buffered_jump(12), "0.2 s early");
  }

  #[test]
  fn releasing_jump_early_cuts_it_short() {
    let (mut world, mut s) = world(60);
    floor(&mut world, 100.0);
    let e = runner(&mut world, PlatformerMovement::default().with_jump(64.0, 0.38));
    settle(&mut world, &mut s);
    let start = at(&world, e).translation.y;
    intent(&mut world, e).jump_pressed = true;
    intent(&mut world, e).jump_held = true;
    let mut top = start;
    for i in 0..60 {
      intent(&mut world, e).jump_held = i < 6;
      run(&mut world, &mut s, 1);
      top = top.min(at(&world, e).translation.y);
    }
    assert!(start - top < 0.7 * 64.0, "{}", start - top);
  }

  #[test]
  fn a_jump_says_jumped_then_landed() {
    let (mut world, mut s) = world(60);
    floor(&mut world, 100.0);
    let e = runner(&mut world, PlatformerMovement::default());
    settle(&mut world, &mut s);
    let before = events(&world).len();
    intent(&mut world, e).jump_pressed = true;
    run(&mut world, &mut s, 90);
    let kinds = &events(&world)[before..];
    assert_eq!(kinds, [MovementEventKind::Jumped, MovementEventKind::Landed]);
  }

  #[test]
  fn down_and_jump_on_a_one_way_platform_drops_through_instead() {
    let (mut world, mut s) = world(60);
    world.spawn((Transform::from_xy(0.0, 105.0), Collider::rect(200.0, 10.0).one_way()));
    let e = runner(&mut world, PlatformerMovement::default());
    settle(&mut world, &mut s);
    intent(&mut world, e).direction = Vec2::Y;
    intent(&mut world, e).jump_pressed = true;
    run(&mut world, &mut s, 1);
    assert!(!events(&world).contains(&MovementEventKind::Jumped));
    run(&mut world, &mut s, 30);
    assert!(at(&world, e).translation.y > 120.0, "fell through");
  }

  // Review focus 5
  #[test]
  fn an_out_of_range_direction_is_clamped() {
    let (mut world, mut s) = world(60);
    floor(&mut world, 100.0);
    let e = runner(&mut world, PlatformerMovement::default());
    settle(&mut world, &mut s);
    for _ in 0..30 {
      intent(&mut world, e).direction = Vec2::new(5.0, 0.0);
      run(&mut world, &mut s, 1);
    }
    let dx = world.get::<Velocity>(e).unwrap().dx;
    assert!((dx - 180.0).abs() < 1e-3, "{dx}");
    let x = at(&world, e).translation.x;
    for _ in 0..10 {
      intent(&mut world, e).direction = Vec2::new(f32::NAN, 0.0);
      run(&mut world, &mut s, 1);
    }
    assert!(world.get::<Velocity>(e).unwrap().as_vec2().is_finite());
    assert!(at(&world, e).translation.x.is_finite() && at(&world, e).translation.x >= x);
  }
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p ryo2d --lib gameplay::platformer`
Expected: compile errors.

- [ ] **Step 3: Implement**

`events.rs`:

```rust
//! Gameplay messages.

use crate::core::ecs::{Entity, Message};

/// What a movement preset just did, for sounds, particles and squash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MovementEventKind {
  Jumped,
  AirJumped,
  WallJumped,
  Dashed,
  Landed,
}

/// A movement preset's character jumped, dashed or landed.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct MovementEvent {
  pub entity: Entity,
  pub kind: MovementEventKind,
}
```

`platformer.rs`:

```rust
//! The platformer preset: run, jump and fall from `MoveIntent`, with the
//! forgiveness that makes jumps feel fair.

use super::events::{MovementEvent, MovementEventKind};
use super::intent::MoveIntent;
use crate::core::ecs::{
  Component, Entity, EntityHashSet, Knockback, Local, MessageWriter, Query, Res, Vec2, Velocity,
};
use crate::core::physics::{CharacterController, CharacterMode, CharacterState};
use crate::core::time::Time;

fn platformer_controller() -> CharacterController {
  CharacterController::platformer()
}

/// A side-view character's movement from its `MoveIntent`: running with
/// acceleration, air control, a jump set by its height and the time to its
/// top, a heavier fall, coyote time, jump buffering and a shorter jump when
/// the button is released early. Down + jump on a one-way platform drops
/// through it. It writes the character's `Velocity` every step, with its own
/// gravity (characters are not dynamic bodies).
#[derive(Component, Debug, Clone, PartialEq)]
#[non_exhaustive]
#[require(MoveIntent, PlatformerState, CharacterController = platformer_controller())]
pub struct PlatformerMovement {
  /// Top running speed, px/s
  pub run_speed: f32,
  /// Seconds from standing to full speed (0 is instant)
  pub accel_time: f32,
  /// Seconds from full speed to standing (0 is instant)
  pub decel_time: f32,
  /// The share of ground acceleration available in the air
  pub air_control: f32,
  /// Jump height, px
  pub jump_height: f32,
  /// Seconds from the jump to its top
  pub time_to_apex: f32,
  /// Gravity multiplier while falling
  pub fall_multiplier: f32,
  /// Fastest fall, px/s
  pub max_fall_speed: f32,
  /// Seconds after leaving a floor a jump still works
  pub coyote_time: f32,
  /// Seconds a jump pressed before landing is kept
  pub jump_buffer: f32,
  /// Upward speed kept when jump is released early (1 keeps full jumps)
  pub jump_cut: f32,
  /// The share of steering kept while knockback is active (0: none)
  pub knockback_control: f32,
}

impl Default for PlatformerMovement {
  fn default() -> Self {
    Self {
      run_speed: 180.0,
      accel_time: 0.08,
      decel_time: 0.06,
      air_control: 0.65,
      jump_height: 64.0,
      time_to_apex: 0.38,
      fall_multiplier: 1.5,
      max_fall_speed: 600.0,
      coyote_time: 0.1,
      jump_buffer: 0.1,
      jump_cut: 0.5,
      knockback_control: 0.0,
    }
  }
}

impl PlatformerMovement {
  pub fn with_run(mut self, speed: f32, accel_time: f32, decel_time: f32) -> Self {
    (self.run_speed, self.accel_time, self.decel_time) = (speed, accel_time, decel_time);
    self
  }

  pub fn with_air_control(mut self, fraction: f32) -> Self {
    self.air_control = fraction;
    self
  }

  pub fn with_jump(mut self, height: f32, time_to_apex: f32) -> Self {
    (self.jump_height, self.time_to_apex) = (height, time_to_apex);
    self
  }

  pub fn with_fall(mut self, multiplier: f32, max_speed: f32) -> Self {
    (self.fall_multiplier, self.max_fall_speed) = (multiplier, max_speed);
    self
  }

  pub fn with_coyote_time(mut self, seconds: f32) -> Self {
    self.coyote_time = seconds;
    self
  }

  pub fn with_jump_buffer(mut self, seconds: f32) -> Self {
    self.jump_buffer = seconds;
    self
  }

  pub fn with_jump_cut(mut self, factor: f32) -> Self {
    self.jump_cut = factor;
    self
  }

  pub fn with_knockback_control(mut self, fraction: f32) -> Self {
    self.knockback_control = fraction;
    self
  }
}

/// A platformer character's movement state, written by the engine.
#[derive(Component, Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct PlatformerState {
  /// -1 facing left, 1 facing right
  pub facing: f32,
  /// Rising from a jump (releasing jump cuts it short)
  pub jumping: bool,
  pub(crate) coyote: f32,
  pub(crate) buffer: f32,
}

impl Default for PlatformerState {
  fn default() -> Self {
    Self {
      facing: 1.0,
      jumping: false,
      coyote: 0.0,
      buffer: 0.0,
    }
  }
}

impl PlatformerState {
  /// Forget jumps, timers and dashes in progress, keeping the facing (a respawn).
  pub fn reset(&mut self) {
    *self = Self {
      facing: self.facing,
      ..Self::default()
    };
  }
}

/// Gravity and launch speed for a jump `height` px high that reaches its
/// top after `time` s, exact at the fixed step `dt`: with gravity added
/// before each move, `height = (g / 2)(T² - T·dt)` at `T = v / g`.
pub(crate) fn jump_physics(height: f32, time: f32, dt: f32) -> (f32, f32) {
  let time = time.max(2.0 * dt);
  let gravity = 2.0 * height.max(0.0) / (time * time - time * dt);
  (gravity, gravity * time)
}

/// `value` moved toward `target` by at most `step` (straight there when the
/// step is infinite).
fn approach(value: f32, target: f32, step: f32) -> f32 {
  if (target - value).abs() <= step {
    target
  } else {
    value + step.copysign(target - value)
  }
}

/// Speed change per second toward `target`: accelerating toward a faster
/// speed the same way, decelerating otherwise. A zero time is instant.
fn run_rate(settings: &PlatformerMovement, current: f32, target: f32) -> f32 {
  let accelerating = target != 0.0
    && (current == 0.0 || current.signum() == target.signum())
    && target.abs() >= current.abs();
  let time = if accelerating {
    settings.accel_time
  } else {
    settings.decel_time
  };
  if time > 0.0 {
    settings.run_speed / time
  } else {
    f32::INFINITY
  }
}

type Mover = (
  Entity,
  &'static PlatformerMovement,
  &'static mut PlatformerState,
  &'static mut MoveIntent,
  &'static CharacterState,
  &'static mut CharacterController,
  &'static mut Velocity,
  Option<&'static Knockback>,
);

/// `FixedSet::Movement`: every platformer character's `Velocity` from its
/// intent and its last move.
pub fn platformer_movement(
  time: Res<Time>,
  mut movers: Query<Mover>,
  mut events: MessageWriter<MovementEvent>,
  mut warned: Local<EntityHashSet>,
) {
  let dt = time.fixed.timestep();
  for (entity, settings, mut state, mut intent, character, mut controller, mut velocity, knockback) in
    &mut movers
  {
    if controller.mode != CharacterMode::Platformer {
      if warned.insert(entity) {
        crate::engine_warn!(module: "Gameplay", "{entity}: PlatformerMovement needs CharacterController::platformer()");
      }
      continue;
    }
    let direction = intent.clamped_direction();
    let mut v = velocity.as_vec2();
    if !v.is_finite() {
      v = Vec2::ZERO;
    }
    let (gravity, jump_speed) = jump_physics(settings.jump_height, settings.time_to_apex, dt);
    if character.landed {
      events.write(MovementEvent { entity, kind: MovementEventKind::Landed });
    }

    state.coyote = if character.on_floor {
      settings.coyote_time
    } else {
      (state.coyote - dt).max(0.0)
    };
    state.buffer = if intent.jump_pressed {
      settings.jump_buffer.max(dt)
    } else {
      (state.buffer - dt).max(0.0)
    };

    // Running; knockback takes the steering away
    let control = if knockback.is_some_and(|knockback| knockback.active) {
      settings.knockback_control
    } else {
      1.0
    };
    let target = direction.x * settings.run_speed * control;
    let mut rate = run_rate(settings, v.x, target);
    if !character.on_floor {
      rate *= settings.air_control;
    }
    v.x = approach(v.x, target, rate * dt);
    if direction.x.abs() > 0.01 {
      state.facing = direction.x.signum();
    }

    // Dropping through, or jumping
    if intent.jump_pressed && direction.y > 0.5 && character.on_floor && character.floor_one_way {
      controller.drop_through = true;
      state.buffer = 0.0;
    } else if state.buffer > 0.0 && state.coyote > 0.0 {
      v.y = -jump_speed;
      state.jumping = true;
      state.buffer = 0.0;
      state.coyote = 0.0;
      events.write(MovementEvent { entity, kind: MovementEventKind::Jumped });
    }

    // A jump released early is cut short, once
    if state.jumping {
      if v.y >= 0.0 {
        state.jumping = false;
      } else if !intent.jump_held {
        v.y *= settings.jump_cut;
        state.jumping = false;
      }
    }

    // Falling, heavier on the way down
    let pull = if v.y > 0.0 {
      gravity * settings.fall_multiplier
    } else {
      gravity
    };
    v.y = (v.y + pull * dt).min(settings.max_fall_speed);

    intent.jump_pressed = false;
    intent.dash_pressed = false;
    velocity.set_if_neq(Velocity::from_vec2(v));
  }
}
```

Register `platformer_movement` after `update_knockback`; register `MovementEvent` in `register_engine_messages`; export `PlatformerMovement`, `PlatformerState`, `MovementEvent`, `MovementEventKind` in the prelude.

Note on `the_jump_reaches_its_height_at_its_apex_time`: if snapping pulls the character back down on the jump step, or the measured height is off by more than a pixel, the cause is in the controller (snapping while the move goes up), not the formula: check `move_shape`'s snap condition and fix it in `move_character` (skip snapping when `desired` points up) rather than widening the tolerance.

`gameplay/AGENTS.md`: the platformer preset (its order each step: landed event, timers, run, drop or jump, cut, gravity, clear the presses), the jump formula and why it is exact at the fixed step, the defaults.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ryo2d --lib gameplay`
Expected: PASS.

- [ ] **Step 5: Gate and commit**

```bash
git add -A
git commit -m "feat: platformer movement preset" -m "- run, air control, jump by height and time to apex, heavier fall
- coyote time, jump buffering, jump cut; down + jump drops through one-way platforms
- MovementEvent for jumps and landings"
```

---

### Task 10: Platformer extras: air jumps, wall slide and wall jump, dash, knockback control

**Files:**
- Modify: `src/core/gameplay/platformer.rs` (fields, builders, state, the system; tests)
- Modify: `src/core/gameplay/AGENTS.md`

**Interfaces:**
- Consumes: Task 9's preset.
- Produces: `PlatformerMovement` gains `pub air_jumps: u32`, `pub wall_slide: Option<f32>`, `pub wall_jump: Option<WallJump>`, `pub dash: Option<Dash>` with `with_air_jumps(n)`, `with_wall_slide(max_speed)`, `with_wall_jump(launch: Vec2, lock_time: f32)`, `with_dash(speed, duration, cooldown)`; `pub struct WallJump { pub launch: Vec2, pub lock_time: f32 }`, `pub struct Dash { pub speed: f32, pub duration: f32, pub cooldown: f32 }` (both `Copy`, `PartialEq`, exported; `Dash` is shared with Task 11); `PlatformerState` gains `pub air_jumps_left: u32`, `pub wall_sliding: bool`, `pub dashing: bool`, `pub dash_ready: bool` and crate-internal `wall_coyote`, `wall_side`, `lock`, `dash_left`, `dash_cooldown`, `dash_direction`.

- [ ] **Step 1: Write the failing tests** (`platformer.rs` tests)

```rust
  /// A wall on the right (its face at x = 60) and a character in the air beside it.
  fn beside_a_wall(settings: PlatformerMovement) -> (World, EngineSchedules, Entity) {
    let (mut world, mut s) = world(60);
    floor(&mut world, 400.0);
    world.spawn((Transform::from_xy(70.0, 0.0), Collider::rect(20.0, 1000.0)));
    let e = player(&mut world, 40.0, 0.0);
    world.entity_mut(e).insert(settings);
    for _ in 0..10 {
      intent(&mut world, e).direction = Vec2::X;
      run(&mut world, &mut s, 1);
    }
    (world, s, e)
  }

  #[test]
  fn air_jumps_count_down_and_refill_on_landing() {
    let (mut world, mut s) = world(60);
    floor(&mut world, 100.0);
    let e = runner(&mut world, PlatformerMovement::default().with_air_jumps(1));
    settle(&mut world, &mut s);
    for _ in 0..3 {
      intent(&mut world, e).jump_pressed = true;
      run(&mut world, &mut s, 10);
    }
    let kinds = events(&world);
    assert_eq!(kinds.iter().filter(|k| **k == MovementEventKind::Jumped).count(), 1);
    assert_eq!(kinds.iter().filter(|k| **k == MovementEventKind::AirJumped).count(), 1);
    run(&mut world, &mut s, 90);
    assert_eq!(world.get::<PlatformerState>(e).unwrap().air_jumps_left, 1);
  }

  #[test]
  fn sliding_down_a_wall_caps_the_fall() {
    let (mut world, mut s, e) =
      beside_a_wall(PlatformerMovement::default().with_wall_slide(80.0));
    for _ in 0..30 {
      intent(&mut world, e).direction = Vec2::X;
      run(&mut world, &mut s, 1);
    }
    assert!(world.get::<PlatformerState>(e).unwrap().wall_sliding);
    assert!(world.get::<Velocity>(e).unwrap().dy <= 80.0 + 1e-3);
  }

  #[test]
  fn a_wall_jump_launches_away_and_holds_the_direction() {
    let (mut world, mut s, e) = beside_a_wall(
      PlatformerMovement::default()
        .with_wall_slide(80.0)
        .with_wall_jump(Vec2::new(300.0, 350.0), 0.2),
    );
    intent(&mut world, e).direction = Vec2::X;
    intent(&mut world, e).jump_pressed = true;
    run(&mut world, &mut s, 1);
    assert!(events(&world).contains(&MovementEventKind::WallJumped));
    for _ in 0..6 {
      intent(&mut world, e).direction = Vec2::X;
      run(&mut world, &mut s, 1);
      assert!(world.get::<Velocity>(e).unwrap().dx < 0.0, "input held off during the lock");
    }
  }

  #[test]
  fn a_dash_is_fast_level_and_limited_in_the_air() {
    let (mut world, mut s) = world(60);
    floor(&mut world, 100.0);
    // Cooldown shorter than the dash, so the second try is limited by the
    // air dash alone, while the character is still in the air
    let e = runner(&mut world, PlatformerMovement::default().with_dash(600.0, 0.15, 0.1));
    settle(&mut world, &mut s);
    intent(&mut world, e).jump_pressed = true;
    intent(&mut world, e).jump_held = true;
    run(&mut world, &mut s, 5);
    let y = at(&world, e).translation.y;
    intent(&mut world, e).direction = Vec2::X;
    intent(&mut world, e).dash_pressed = true;
    run(&mut world, &mut s, 1);
    assert_eq!(world.get::<Velocity>(e).unwrap().as_vec2(), Vec2::new(600.0, 0.0));
    run(&mut world, &mut s, 3);
    assert!((at(&world, e).translation.y - y).abs() < 1.0, "no gravity while dashing");
    run(&mut world, &mut s, 7);
    assert!(!world.get::<CharacterState>(e).unwrap().on_floor, "still in the air");
    intent(&mut world, e).dash_pressed = true;
    run(&mut world, &mut s, 1);
    let dashes = events(&world).iter().filter(|k| **k == MovementEventKind::Dashed).count();
    assert_eq!(dashes, 1, "one air dash until landing");
  }

  #[test]
  fn knockback_takes_the_steering_away() {
    let (mut world, mut s) = world(60);
    floor(&mut world, 100.0);
    let e = runner(&mut world, PlatformerMovement::default());
    settle(&mut world, &mut s);
    world
      .entity_mut(e)
      .insert(Knockback::new(Vec2::NEG_X, 200.0).with_decay_rate(1.0));
    for _ in 0..10 {
      intent(&mut world, e).direction = Vec2::X;
      run(&mut world, &mut s, 1);
    }
    assert!(world.get::<Velocity>(e).unwrap().dx.abs() < 1e-3);
  }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p ryo2d --lib gameplay::platformer`
Expected: compile errors (`with_air_jumps` etc.), then FAILs.

- [ ] **Step 3: Implement**

Add the fields (defaults: `air_jumps: 0`, the three options `None`), builders, the two settings structs and the state fields (`dash_ready: true` in `Default`). Extend `platformer_movement`, in this order within each step:

1. After the landed event: on `character.landed`, `state.air_jumps_left = settings.air_jumps; state.dash_ready = true;`.
2. Timers: also `state.lock = (state.lock - dt).max(0.0); state.dash_cooldown = (state.dash_cooldown - dt).max(0.0);`. Wall contact in the air: `if character.on_wall && !character.on_floor { state.wall_coyote = settings.coyote_time; state.wall_side = character.wall_normal.x.signum(); } else { state.wall_coyote = (state.wall_coyote - dt).max(0.0); }`.
3. Dash start: `if let Some(dash) = settings.dash && intent.dash_pressed && state.dash_ready && state.dash_cooldown <= 0.0 && state.dash_left <= 0.0` → `state.dash_direction = direction.try_normalize().unwrap_or(Vec2::new(state.facing, 0.0)); state.dash_left = dash.duration; state.dash_cooldown = dash.cooldown; if !character.on_floor { state.dash_ready = false; } state.jumping = false;` and write `Dashed`.
4. While dashing (`state.dash_left > 0.0`, with `dash` set): `v = state.dash_direction * dash.speed; state.dash_left -= dt;` and when it reaches zero `v = state.dash_direction * dash.speed.min(settings.run_speed)`; set `state.dashing = state.dash_left > 0.0`, clear the intent's presses, write `Velocity` and `continue` (no running, jumping or gravity this step). Otherwise `state.dashing = false`.
5. Running: skip the input while `state.lock > 0.0` (`v.x` unchanged).
6. Jumps, in order: drop-through; ground jump (as before); wall jump: `else if let Some(wall) = settings.wall_jump && state.buffer > 0.0 && state.wall_coyote > 0.0 && !character.on_floor` → `v.x = state.wall_side * wall.launch.x; v.y = -wall.launch.y; state.lock = wall.lock_time; state.air_jumps_left = settings.air_jumps; state.facing = state.wall_side; state.jumping = true; state.buffer = 0.0; state.wall_coyote = 0.0;` write `WallJumped`; air jump: `else if intent.jump_pressed && !character.on_floor && state.coyote <= 0.0 && state.air_jumps_left > 0` → `v.y = -jump_speed; state.air_jumps_left -= 1; state.jumping = true; state.buffer = 0.0;` write `AirJumped`.
7. After gravity, the wall slide: `state.wall_sliding = false; if let Some(max) = settings.wall_slide && character.on_wall && !character.on_floor && v.y > 0.0 && direction.x * character.wall_normal.x < 0.0 { v.y = v.y.min(max); state.wall_sliding = true; }`.

`PlatformerState::reset` keeps working (`Default` covers the new fields). `gameplay/AGENTS.md`: the extras and their order (dash > drop-through > ground jump > wall jump > air jump), wall coyote shares `coyote_time`, one air dash refilled on landing.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ryo2d --lib gameplay`
Expected: PASS.

- [ ] **Step 5: Gate and commit**

```bash
git add -A
git commit -m "feat: platformer extras" -m "- air jumps, wall slide and wall jump with an input lock, dash
- knockback control"
```

---

### Task 11: `TopDownMovement`

**Files:**
- Create: `src/core/gameplay/top_down.rs`
- Modify: `src/core/gameplay/mod.rs` (module, exports, `top_down_movement` after `platformer_movement`), `src/lib.rs`, `src/core/gameplay/AGENTS.md`

**Interfaces:**
- Consumes: `Dash` (Task 10), `MoveIntent`, `CharacterController::top_down()`.
- Produces: `pub struct TopDownMovement { pub max_speed, pub accel_time, pub decel_time: f32, pub dash: Option<Dash>, pub knockback_control: f32 }` (requires `MoveIntent`, `TopDownState`, `CharacterController::top_down()`; default 160 px/s, 0.08 s, 0.1 s), builders `with_speed(max, accel_time, decel_time)`, `with_dash(speed, duration, cooldown)`, `with_knockback_control(fraction)`; `pub struct TopDownState { pub facing: Vec2, pub dashing: bool, .. }` with `reset`; `pub fn top_down_movement(...)`.

- [ ] **Step 1: Write the failing tests** (`top_down.rs`)

```rust
#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ecs::{Transform, World};
  use crate::core::gameplay::test_util::*;
  use crate::core::physics::Collider;

  fn walker(world: &mut World) -> Entity {
    world
      .spawn((
        Transform::IDENTITY,
        Collider::circle(8.0),
        TopDownMovement::default(),
      ))
      .id()
  }

  fn steer(world: &mut World, e: Entity, direction: Vec2) {
    world.get_mut::<MoveIntent>(e).unwrap().direction = direction;
  }

  #[test]
  fn reaches_full_speed_in_its_acceleration_time() {
    let (mut world, mut s) = world(60);
    let e = walker(&mut world);
    for _ in 0..5 {
      steer(&mut world, e, Vec2::X);
      run(&mut world, &mut s, 1);
    }
    assert!((world.get::<Velocity>(e).unwrap().dx - 160.0).abs() < 1e-3);
  }

  #[test]
  fn diagonals_are_no_faster_and_half_a_stick_is_half_speed() {
    let (mut world, mut s) = world(60);
    let e = walker(&mut world);
    for _ in 0..20 {
      steer(&mut world, e, Vec2::ONE);
      run(&mut world, &mut s, 1);
    }
    assert!((world.get::<Velocity>(e).unwrap().as_vec2().length() - 160.0).abs() < 1e-2);
    for _ in 0..20 {
      steer(&mut world, e, Vec2::new(0.5, 0.0));
      run(&mut world, &mut s, 1);
    }
    assert!((world.get::<Velocity>(e).unwrap().as_vec2().length() - 80.0).abs() < 1e-2);
  }

  #[test]
  fn letting_go_stops_in_its_deceleration_time() {
    let (mut world, mut s) = world(60);
    let e = walker(&mut world);
    for _ in 0..20 {
      steer(&mut world, e, Vec2::X);
      run(&mut world, &mut s, 1);
    }
    for _ in 0..7 {
      steer(&mut world, e, Vec2::ZERO);
      run(&mut world, &mut s, 1);
    }
    assert_eq!(world.get::<Velocity>(e).unwrap().as_vec2(), Vec2::ZERO);
  }

  #[test]
  fn a_dash_bursts_in_the_facing_direction() {
    let (mut world, mut s) = world(60);
    let e = world
      .spawn((
        Transform::IDENTITY,
        Collider::circle(8.0),
        TopDownMovement::default().with_dash(500.0, 0.1, 0.4),
      ))
      .id();
    steer(&mut world, e, Vec2::Y);
    run(&mut world, &mut s, 3);
    steer(&mut world, e, Vec2::ZERO);
    world.get_mut::<MoveIntent>(e).unwrap().dash_pressed = true;
    run(&mut world, &mut s, 1);
    assert_eq!(world.get::<Velocity>(e).unwrap().as_vec2(), Vec2::new(0.0, 500.0));
  }
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p ryo2d --lib gameplay::top_down`
Expected: compile errors.

- [ ] **Step 3: Implement**

Mirror `platformer.rs`: `fn top_down_controller() -> CharacterController { CharacterController::top_down() }`; the component, its `Default` (160, 0.08, 0.1, `None`, 0.0) and builders; `TopDownState { facing: Vec2::Y, dashing: false, dash_left, dash_cooldown, dash_direction }` with `reset`; the system:

```rust
/// `FixedSet::Movement`: every top-down character's `Velocity` from its intent.
pub fn top_down_movement(
  time: Res<Time>,
  mut movers: Query<Mover>, // (Entity, &TopDownMovement, &mut TopDownState, &mut MoveIntent, &CharacterController, &mut Velocity, Option<&Knockback>)
  mut events: MessageWriter<MovementEvent>,
  mut warned: Local<EntityHashSet>,
) {
  let dt = time.fixed.timestep();
  for (entity, settings, mut state, mut intent, controller, mut velocity, knockback) in &mut movers {
    if controller.mode != CharacterMode::TopDown {
      if warned.insert(entity) {
        crate::engine_warn!(module: "Gameplay", "{entity}: TopDownMovement needs CharacterController::top_down()");
      }
      continue;
    }
    let direction = intent.clamped_direction();
    let mut v = velocity.as_vec2();
    if !v.is_finite() {
      v = Vec2::ZERO;
    }
    if let Some(facing) = direction.try_normalize() {
      state.facing = facing;
    }
    state.dash_cooldown = (state.dash_cooldown - dt).max(0.0);
    if let Some(dash) = settings.dash
      && intent.dash_pressed
      && state.dash_cooldown <= 0.0
      && state.dash_left <= 0.0
    {
      state.dash_direction = state.facing;
      state.dash_left = dash.duration;
      state.dash_cooldown = dash.cooldown;
      events.write(MovementEvent { entity, kind: MovementEventKind::Dashed });
    }
    if let Some(dash) = settings.dash
      && state.dash_left > 0.0
    {
      v = state.dash_direction * dash.speed;
      state.dash_left -= dt;
      if state.dash_left <= 0.0 {
        v = state.dash_direction * dash.speed.min(settings.max_speed);
      }
    } else {
      let control = if knockback.is_some_and(|knockback| knockback.active) {
        settings.knockback_control
      } else {
        1.0
      };
      let target = direction * settings.max_speed * control;
      let time = if target != Vec2::ZERO {
        settings.accel_time
      } else {
        settings.decel_time
      };
      let rate = if time > 0.0 { settings.max_speed / time } else { f32::INFINITY };
      let gap = target - v;
      v = if gap.length() <= rate * dt { target } else { v + gap.normalize() * rate * dt };
    }
    state.dashing = state.dash_left > 0.0;
    intent.jump_pressed = false;
    intent.dash_pressed = false;
    velocity.set_if_neq(Velocity::from_vec2(v));
  }
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ryo2d --lib gameplay`
Expected: PASS.

- [ ] **Step 5: Gate and commit**

```bash
git add -A
git commit -m "feat: top-down movement preset" -m "- acceleration and deceleration in eight directions, analog magnitude kept
- an optional dash"
```

---

### Task 12: Trigger behaviours: `Pickup`, `Respawn`, `KillZone`, `Checkpoint`

**Files:**
- Create: `src/core/gameplay/zones.rs`
- Modify: `src/core/gameplay/events.rs` (`PickedUp`, `Killed`, `CheckpointReached`), `src/core/gameplay/mod.rs` (the zone systems at the head of the chain), `src/core/messages/mod.rs` (register them), `src/lib.rs`
- Modify: `src/core/gameplay/AGENTS.md`

**Interfaces:**
- Consumes: `TriggerEntered` (Task 2), `PlatformerState`/`TopDownState::reset`, `Knockback::stop`, `Disabled`.
- Produces: `pub struct Pickup { pub respawn_after: Option<f32> }` with `Pickup::new()`, `respawn_after(seconds)`; `pub struct Respawn { pub point: Vec2 }` with `Respawn::here()`, `Respawn::at(point)`; `pub struct KillZone { pub despawn: bool }` with `KillZone::new()`, `despawning()`; `pub struct Checkpoint { pub offset: Vec2 }` with `Checkpoint::new()`, `with_offset(Vec2)`; messages `PickedUp { pickup, by }`, `Killed { entity, zone }`, `CheckpointReached { checkpoint, entity }`; systems `place_respawn_points`, `warn_untriggered`, `collect_pickups`, `respawn_pickups`, `apply_kill_zones`, `apply_checkpoints` (all `pub(crate)`).

- [ ] **Step 1: Write the failing tests** (`zones.rs`, with `gameplay::test_util`)

```rust
#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ecs::{Messages, Transform, Velocity, World};
  use crate::core::gameplay::test_util::*;
  use crate::core::physics::{Collider, RigidBody};

  fn read<M: crate::core::ecs::Message + Copy>(world: &World) -> Vec<M> {
    let messages = world.resource::<Messages<M>>();
    messages.get_cursor().read(messages).copied().collect()
  }

  fn zone(world: &mut World, x: f32, behaviour: impl crate::core::ecs::Bundle) -> Entity {
    world
      .spawn((Transform::from_xy(x, 0.0), Collider::rect(40.0, 200.0).as_trigger(), behaviour))
      .id()
  }

  fn mover(world: &mut World, x: f32, vx: f32) -> Entity {
    world
      .spawn((
        Transform::from_xy(x, 0.0),
        RigidBody::kinematic(),
        Collider::rect(10.0, 10.0),
        Velocity::new(vx, 0.0),
      ))
      .id()
  }

  #[test]
  fn a_pickup_is_collected_once_and_despawned() {
    let (mut world, mut s) = world(60);
    let coin = zone(&mut world, 60.0, Pickup::new());
    let player = mover(&mut world, 0.0, 300.0);
    run(&mut world, &mut s, 20);
    assert_eq!(read::<PickedUp>(&world), [PickedUp { pickup: coin, by: player }]);
    assert!(world.get_entity(coin).is_err());
  }

  // Review focus 4
  #[test]
  fn two_collectors_in_one_step_collect_once() {
    let (mut world, mut s) = world(60);
    let coin = zone(&mut world, 0.0, Pickup::new());
    mover(&mut world, -2.0, 0.0);
    mover(&mut world, 2.0, 0.0);
    run(&mut world, &mut s, 5);
    let picked = read::<PickedUp>(&world);
    assert_eq!(picked.len(), 1, "{picked:?}");
    assert_eq!(picked[0].pickup, coin);
  }

  #[test]
  fn a_respawning_pickup_comes_back() {
    let (mut world, mut s) = world(60);
    let coin = zone(&mut world, 60.0, Pickup::new().respawn_after(0.5));
    mover(&mut world, 0.0, 300.0);
    run(&mut world, &mut s, 20);
    assert!(world.get::<crate::core::ecs::Disabled>(coin).is_some());
    run(&mut world, &mut s, 40);
    assert!(world.get::<crate::core::ecs::Disabled>(coin).is_none());
  }

  #[test]
  fn a_kill_zone_respawns_an_entity_with_respawn_and_clears_its_motion() {
    let (mut world, mut s) = world(60);
    let pit = zone(&mut world, 60.0, KillZone::new());
    let player = mover(&mut world, 0.0, 300.0);
    world.entity_mut(player).insert(Respawn::at(Vec2::new(-200.0, 0.0)));
    run(&mut world, &mut s, 20);
    assert_eq!(read::<Killed>(&world)[0], Killed { entity: player, zone: pit });
    assert!(at(&world, player).translation.x < -150.0);
    assert_eq!(*world.get::<Velocity>(player).unwrap(), Velocity::zero());
  }

  #[test]
  fn a_kill_zone_only_reports_others_unless_it_despawns_them() {
    let (mut world, mut s) = world(60);
    zone(&mut world, 60.0, KillZone::new());
    zone(&mut world, -60.0, KillZone::new().despawning());
    let kept = mover(&mut world, 0.0, 300.0);
    let gone = mover(&mut world, 0.0, -300.0);
    run(&mut world, &mut s, 20);
    assert_eq!(read::<Killed>(&world).len(), 2);
    assert!(world.get_entity(kept).is_ok());
    assert!(world.get_entity(gone).is_err());
  }

  #[test]
  fn a_checkpoint_moves_the_respawn_point_and_says_so_once() {
    let (mut world, mut s) = world(60);
    let flag = zone(&mut world, 60.0, Checkpoint::new().with_offset(Vec2::new(0.0, -20.0)));
    let player = mover(&mut world, 0.0, 120.0);
    world.entity_mut(player).insert(Respawn::here());
    run(&mut world, &mut s, 60);
    assert_eq!(world.get::<Respawn>(player).unwrap().point, Vec2::new(60.0, -20.0));
    assert_eq!(
      read::<CheckpointReached>(&world),
      [CheckpointReached { checkpoint: flag, entity: player }]
    );
  }

  #[test]
  fn respawn_here_takes_the_position_when_added() {
    let (mut world, mut s) = world(60);
    let player = mover(&mut world, 33.0, 0.0);
    world.entity_mut(player).insert(Respawn::here());
    run(&mut world, &mut s, 1);
    assert_eq!(world.get::<Respawn>(player).unwrap().point, Vec2::new(33.0, 0.0));
  }
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p ryo2d --lib gameplay::zones`
Expected: compile errors.

- [ ] **Step 3: Implement**

`events.rs` gains:

```rust
/// A `Pickup` was collected by `by` (a body, or a bare collider).
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct PickedUp {
  pub pickup: Entity,
  pub by: Entity,
}

/// `entity` entered a `KillZone`.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Killed {
  pub entity: Entity,
  pub zone: Entity,
}

/// `entity`'s respawn point moved to a `Checkpoint`.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct CheckpointReached {
  pub checkpoint: Entity,
  pub entity: Entity,
}
```

`zones.rs`:

```rust
//! Ready-made trigger behaviours. Each goes on a trigger collider and acts
//! in `FixedSet::Movement` on the previous step's `TriggerEntered` messages;
//! what can set it off is the collider's layer and mask.

use super::events::{CheckpointReached, Killed, PickedUp};
use super::platformer::PlatformerState;
use super::top_down::TopDownState;
use crate::core::ecs::{
  Added, Allow, Commands, Component, Disabled, Entity, EntityHashSet, GlobalTransform, Knockback,
  Local, MessageReader, MessageWriter, Or, Query, Res, Transform, Vec2, Velocity, With,
};
use crate::core::physics::{Collider, TriggerEntered};
use crate::core::time::Time;

/// Collected by the first thing its mask lets in: `PickedUp` is written and
/// the pickup despawned, or disabled and brought back after `respawn_after`
/// seconds.
#[derive(Component, Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct Pickup {
  pub respawn_after: Option<f32>,
  pub(crate) timer: f32,
}

impl Pickup {
  pub fn new() -> Self {
    Self::default()
  }

  /// Come back `seconds` after being collected (arcade coins, power-ups).
  pub fn respawn_after(mut self, seconds: f32) -> Self {
    self.respawn_after = Some(seconds);
    self
  }
}

/// Where a `KillZone` puts this entity back; a `Checkpoint` moves it. Put it
/// on root entities (it sets `Transform.translation`).
#[derive(Component, Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct Respawn {
  pub point: Vec2,
  pub(crate) placed: bool,
}

impl Respawn {
  /// The entity's position when the component is added.
  pub fn here() -> Self {
    Self { point: Vec2::ZERO, placed: false }
  }

  pub fn at(point: Vec2) -> Self {
    Self { point, placed: true }
  }
}

/// Writes `Killed` for what enters. An entity with `Respawn` is put back at
/// its point with its velocity, knockback and preset state cleared; others
/// only get the message, unless the zone is `despawning()`.
#[derive(Component, Debug, Clone, Copy, PartialEq, Default)]
#[non_exhaustive]
pub struct KillZone {
  pub despawn: bool,
}

impl KillZone {
  pub fn new() -> Self {
    Self::default()
  }

  /// Despawn what enters without a `Respawn` (bullets, enemies falling out).
  pub fn despawning(mut self) -> Self {
    self.despawn = true;
    self
  }
}

/// Moves the `Respawn` point of what enters to the checkpoint's position
/// plus `offset`; writes `CheckpointReached` when the point changes.
#[derive(Component, Debug, Clone, Copy, PartialEq, Default)]
#[non_exhaustive]
pub struct Checkpoint {
  pub offset: Vec2,
}

impl Checkpoint {
  pub fn new() -> Self {
    Self::default()
  }

  pub fn with_offset(mut self, offset: Vec2) -> Self {
    self.offset = offset;
    self
  }
}

/// Give `Respawn::here()` its entity's position: its `Transform`, which is
/// what a respawn writes back (a world position on a root entity), and which
/// is set on the step it was spawned, unlike `GlobalTransform`.
pub(crate) fn place_respawn_points(mut points: Query<(&mut Respawn, &Transform)>) {
  for (mut respawn, transform) in &mut points {
    if !respawn.placed {
      respawn.point = transform.translation;
      respawn.placed = true;
    }
  }
}

type Behaviours = Or<(Added<Pickup>, Added<KillZone>, Added<Checkpoint>)>;

/// A behaviour on a collider that is not a trigger never fires: say so once.
pub(crate) fn warn_untriggered(
  added: Query<(Entity, Option<&Collider>), Behaviours>,
  mut warned: Local<EntityHashSet>,
) {
  for (entity, collider) in &added {
    if !collider.is_some_and(|collider| collider.is_trigger) && warned.insert(entity) {
      crate::engine_warn!(
        module: "Gameplay",
        "{entity}: Pickup, KillZone and Checkpoint need a trigger collider (Collider::as_trigger) on the same entity"
      );
    }
  }
}

pub(crate) fn collect_pickups(
  mut entered: MessageReader<TriggerEntered>,
  mut pickups: Query<&mut Pickup>,
  mut picked: MessageWriter<PickedUp>,
  mut commands: Commands,
) {
  let mut collected = EntityHashSet::default();
  for message in entered.read() {
    let Ok(mut pickup) = pickups.get_mut(message.trigger) else {
      continue;
    };
    if !collected.insert(message.trigger) {
      continue;
    }
    picked.write(PickedUp { pickup: message.trigger, by: message.actor() });
    match pickup.respawn_after {
      Some(seconds) => {
        pickup.timer = seconds;
        commands.entity(message.trigger).insert(Disabled);
      }
      None => commands.entity(message.trigger).despawn(),
    }
  }
}

pub(crate) fn respawn_pickups(
  time: Res<Time>,
  mut waiting: Query<(Entity, &mut Pickup), (With<Disabled>, Allow<Disabled>)>,
  mut commands: Commands,
) {
  let dt = time.fixed.timestep();
  for (entity, mut pickup) in &mut waiting {
    if pickup.respawn_after.is_none() {
      continue;
    }
    pickup.timer -= dt;
    if pickup.timer <= 0.0 {
      commands.entity(entity).remove::<Disabled>();
    }
  }
}

type Respawned = (
  &'static Respawn,
  &'static mut Transform,
  Option<&'static mut Velocity>,
  Option<&'static mut Knockback>,
  Option<&'static mut PlatformerState>,
  Option<&'static mut TopDownState>,
);

pub(crate) fn apply_kill_zones(
  mut entered: MessageReader<TriggerEntered>,
  zones: Query<&KillZone>,
  mut actors: Query<Respawned>,
  mut killed: MessageWriter<Killed>,
  mut commands: Commands,
) {
  for message in entered.read() {
    let Ok(zone) = zones.get(message.trigger) else {
      continue;
    };
    let entity = message.actor();
    killed.write(Killed { entity, zone: message.trigger });
    match actors.get_mut(entity) {
      Ok((respawn, mut transform, velocity, knockback, platformer, top_down)) => {
        transform.translation = respawn.point;
        if let Some(mut velocity) = velocity {
          *velocity = Velocity::zero();
        }
        if let Some(mut knockback) = knockback {
          knockback.stop();
        }
        if let Some(mut state) = platformer {
          state.reset();
        }
        if let Some(mut state) = top_down {
          state.reset();
        }
      }
      Err(_) if zone.despawn => {
        if let Ok(mut actor) = commands.get_entity(entity) {
          actor.despawn();
        }
      }
      Err(_) => {}
    }
  }
}

pub(crate) fn apply_checkpoints(
  mut entered: MessageReader<TriggerEntered>,
  checkpoints: Query<(&Checkpoint, &GlobalTransform)>,
  mut respawns: Query<&mut Respawn>,
  mut reached: MessageWriter<CheckpointReached>,
) {
  for message in entered.read() {
    let Ok((checkpoint, global)) = checkpoints.get(message.trigger) else {
      continue;
    };
    let entity = message.actor();
    let Ok(mut respawn) = respawns.get_mut(entity) else {
      continue;
    };
    let point = global.translation() + checkpoint.offset;
    if !respawn.placed || respawn.point.distance(point) > 0.5 {
      respawn.point = point;
      respawn.placed = true;
      reached.write(CheckpointReached { checkpoint: message.trigger, entity });
    }
  }
}
```

The chain becomes `(place_respawn_points, warn_untriggered, collect_pickups, respawn_pickups, apply_kill_zones, apply_checkpoints, read_move_bindings, update_knockback, platformer_movement, top_down_movement).chain().in_set(FixedSet::Movement)`. Register the three messages; export the four components and three messages in the prelude.

`gameplay/AGENTS.md`: the behaviours, that they act one step after the overlap (they read the previous step's messages, keeping game rules out of the physics step; a respawn teleports at the next `push_changes`), `Respawn` on roots.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ryo2d --lib gameplay` and `cargo test --test integration`
Expected: PASS.

- [ ] **Step 5: Gate and commit**

```bash
git add -A
git commit -m "feat: Pickup, KillZone, Checkpoint and Respawn" -m "- ready-made trigger behaviours acting on TriggerEntered
- PickedUp, Killed and CheckpointReached messages"
```

---

### Task 13: Tile outlines

The pure geometry: a grid of cells to closed outlines (seamless, slopes included), one-way runs and greedy solid rectangles, all in tile units.

**Files:**
- Create: `src/core/physics/tiles.rs` (the cell types and the three functions; Task 14 adds the component)
- Modify: `src/core/physics/mod.rs` (`mod tiles;`, export `TileCell`, `SlopeCorner`), `src/lib.rs`

**Interfaces:**
- Produces: `pub enum TileCell { Empty, Solid, OneWay, Slope(SlopeCorner) }` (`Copy`, `Eq`, `Default` = `Empty`); `pub enum SlopeCorner { BottomLeft, BottomRight, TopLeft, TopRight }` with `pub fn from_name(name: &str) -> Option<Self>` (`"bottom-left"` …); crate-internal `type Corner = (i32, i32)`, `fn outlines(width: u32, height: u32, cells: &[TileCell]) -> Vec<Vec<Corner>>` (closed: last = first), `fn one_way_runs(width, height, cells) -> Vec<(Corner, Corner)>` (left and right ends of each run's top edge), `fn solid_rects(width, height, cells) -> Vec<(Corner, Corner)>` (min corner and size), `fn corners(cell: TileCell) -> &'static [Corner]`.

- [ ] **Step 1: Write the failing tests** (`tiles.rs`)

```rust
#[cfg(test)]
mod tests {
  use super::*;
  use TileCell::{Empty as E, OneWay as O, Solid as S};

  /// A loop as a set of directed edges, so tests do not depend on where it starts.
  fn edges(outline: &[Corner]) -> std::collections::BTreeSet<(Corner, Corner)> {
    outline.windows(2).map(|w| (w[0], w[1])).collect()
  }

  #[test]
  fn one_cell_is_one_square() {
    let loops = outlines(1, 1, &[S]);
    assert_eq!(loops.len(), 1);
    assert_eq!(loops[0].len(), 5, "four corners, closed");
    assert_eq!(loops[0].first(), loops[0].last());
  }

  #[test]
  fn a_row_merges_into_one_rectangle() {
    let loops = outlines(3, 1, &[S, S, S]);
    assert_eq!(loops.len(), 1);
    assert_eq!(
      edges(&loops[0]),
      edges(&[(0, 0), (3, 0), (3, 1), (0, 1), (0, 0)])
    );
  }

  #[test]
  fn an_l_shape_has_six_corners() {
    let loops = outlines(2, 2, &[S, E, S, S]);
    assert_eq!(loops.len(), 1);
    assert_eq!(loops[0].len(), 7);
  }

  #[test]
  fn a_ring_has_an_outline_and_a_hole() {
    let loops = outlines(3, 3, &[S, S, S, S, E, S, S, S, S]);
    assert_eq!(loops.len(), 2);
    assert!(loops.iter().all(|l| l.len() == 5));
  }

  #[test]
  fn cells_touching_at_a_corner_stay_two_outlines() {
    let loops = outlines(2, 2, &[S, E, E, S]);
    assert_eq!(loops.len(), 2);
    assert!(loops.iter().all(|l| l.len() == 5), "{loops:?}");
  }

  #[test]
  fn a_slope_meets_the_floor_without_a_seam() {
    // Top row: empty, a slope filled at its bottom right; bottom row solid
    let cells = [E, TileCell::Slope(SlopeCorner::BottomRight), S, S];
    let loops = outlines(2, 2, &cells);
    assert_eq!(loops.len(), 1);
    assert_eq!(
      edges(&loops[0]),
      edges(&[(0, 1), (1, 1), (2, 0), (2, 2), (0, 2), (0, 1)])
    );
  }

  #[test]
  fn one_way_cells_make_runs_along_their_tops() {
    let runs = one_way_runs(6, 2, &[O, O, O, E, O, O, E, E, E, E, E, E]);
    assert_eq!(runs, [((0, 0), (3, 0)), ((4, 0), (6, 0))]);
  }

  #[test]
  fn solid_cells_merge_into_few_rectangles() {
    assert_eq!(solid_rects(2, 2, &[S, S, S, S]), [((0, 0), (2, 2))]);
    assert_eq!(solid_rects(3, 1, &[S, E, S]).len(), 2);
  }

  #[test]
  fn a_short_cell_list_counts_as_empty() {
    assert!(outlines(4, 4, &[S]).len() == 1);
    assert!(outlines(0, 0, &[]).is_empty());
  }

  #[test]
  fn slope_names_parse() {
    assert_eq!(SlopeCorner::from_name("bottom-left"), Some(SlopeCorner::BottomLeft));
    assert_eq!(SlopeCorner::from_name("TOP-RIGHT"), Some(SlopeCorner::TopRight));
    assert_eq!(SlopeCorner::from_name("sideways"), None);
  }
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p ryo2d --lib physics::tiles`
Expected: compile errors.

- [ ] **Step 3: Implement** (`tiles.rs`)

```rust
//! Tile colliders: a grid of cells becomes seamless outlines (or merged
//! rectangles), one-way runs and 45° slopes. Coordinates here are in tiles,
//! +Y down, cell (0, 0)'s top-left at the origin.

use std::collections::{BTreeMap, BTreeSet};

/// One cell of a tile collision grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TileCell {
  #[default]
  Empty,
  Solid,
  /// Solid only from above: things jump up through it and land on it
  OneWay,
  /// A 45° half-cell, filled at this corner
  Slope(SlopeCorner),
}

/// Which corner of a slope cell is filled (its right angle).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlopeCorner {
  BottomLeft,
  BottomRight,
  TopLeft,
  TopRight,
}

impl SlopeCorner {
  /// From `"bottom-left"`, `"bottom-right"`, `"top-left"` or `"top-right"`
  /// (any case), as a Tiled `slope` property spells it.
  pub fn from_name(name: &str) -> Option<Self> {
    match name.to_ascii_lowercase().as_str() {
      "bottom-left" => Some(Self::BottomLeft),
      "bottom-right" => Some(Self::BottomRight),
      "top-left" => Some(Self::TopLeft),
      "top-right" => Some(Self::TopRight),
      _ => None,
    }
  }
}

/// A tile corner, in tiles.
pub(crate) type Corner = (i32, i32);

/// A filled cell's corners relative to its top-left, wound the same way
/// (clockwise on screen) so an edge two cells share runs both ways and cancels.
pub(crate) fn corners(cell: TileCell) -> &'static [Corner] {
  match cell {
    TileCell::Solid => &[(0, 0), (1, 0), (1, 1), (0, 1)],
    TileCell::Slope(SlopeCorner::BottomLeft) => &[(0, 0), (1, 1), (0, 1)],
    TileCell::Slope(SlopeCorner::BottomRight) => &[(1, 0), (1, 1), (0, 1)],
    TileCell::Slope(SlopeCorner::TopLeft) => &[(0, 0), (1, 0), (0, 1)],
    TileCell::Slope(SlopeCorner::TopRight) => &[(0, 0), (1, 0), (1, 1)],
    TileCell::Empty | TileCell::OneWay => &[],
  }
}

/// The cell at (x, y); missing cells are empty.
fn cell(width: u32, cells: &[TileCell], x: u32, y: u32) -> TileCell {
  let index = y as usize * width as usize + x as usize;
  cells.get(index).copied().unwrap_or_default()
}

fn sub(a: Corner, b: Corner) -> Corner {
  (a.0 - b.0, a.1 - b.1)
}

/// The signed turn from heading `from` to heading `to`, in radians.
fn turn(from: Corner, to: Corner) -> f32 {
  let cross = (from.0 * to.1 - from.1 * to.0) as f32;
  let dot = (from.0 * to.0 + from.1 * to.1) as f32;
  cross.atan2(dot)
}

/// The closed outlines around every region of solid and slope cells (holes
/// too), straight runs merged: each is a loop of corners whose last repeats
/// its first.
pub(crate) fn outlines(width: u32, height: u32, cells: &[TileCell]) -> Vec<Vec<Corner>> {
  // Every filled cell adds its edges; an edge two cells share appears once
  // each way and cancels, leaving only the boundary
  let mut edges: BTreeSet<(Corner, Corner)> = BTreeSet::new();
  for y in 0..height {
    for x in 0..width {
      let shape = corners(cell(width, cells, x, y));
      for i in 0..shape.len() {
        let at = |c: Corner| (x as i32 + c.0, y as i32 + c.1);
        let (a, b) = (at(shape[i]), at(shape[(i + 1) % shape.len()]));
        if !edges.remove(&(b, a)) {
          edges.insert((a, b));
        }
      }
    }
  }
  let mut outgoing: BTreeMap<Corner, Vec<Corner>> = BTreeMap::new();
  for &(a, b) in &edges {
    outgoing.entry(a).or_default().push(b);
  }
  let mut loops = Vec::new();
  while let Some(&start) = outgoing.keys().next() {
    let mut points = vec![start];
    let (mut at, mut heading) = (start, None);
    while let Some(nexts) = outgoing.get_mut(&at) {
      // Where two regions touch at a corner, the sharpest turn keeps their
      // outlines apart
      let pick = match heading {
        Some(heading) if nexts.len() > 1 => (0..nexts.len())
          .max_by(|&i, &j| {
            turn(heading, sub(nexts[i], at)).total_cmp(&turn(heading, sub(nexts[j], at)))
          })
          .unwrap_or(0),
        _ => 0,
      };
      let next = nexts.swap_remove(pick);
      if nexts.is_empty() {
        outgoing.remove(&at);
      }
      heading = Some(sub(next, at));
      at = next;
      points.push(at);
      if at == start {
        break;
      }
    }
    loops.push(merge_straight_runs(&points));
  }
  loops
}

/// Drop the corners in the middle of straight runs; `points` is closed
/// (its last repeats its first) and so is the result.
fn merge_straight_runs(points: &[Corner]) -> Vec<Corner> {
  let ring = &points[..points.len().saturating_sub(1)];
  let n = ring.len();
  let mut kept: Vec<Corner> = (0..n)
    .filter(|&i| {
      let (a, b) = (sub(ring[i], ring[(i + n - 1) % n]), sub(ring[(i + 1) % n], ring[i]));
      let straight = a.0 * b.1 - a.1 * b.0 == 0 && a.0 * b.0 + a.1 * b.1 > 0;
      !straight
    })
    .map(|i| ring[i])
    .collect();
  if let Some(&first) = kept.first() {
    kept.push(first);
  }
  kept
}

/// Each horizontal run of one-way cells as the left and right ends of its top edge.
pub(crate) fn one_way_runs(width: u32, height: u32, cells: &[TileCell]) -> Vec<(Corner, Corner)> {
  let mut runs = Vec::new();
  for y in 0..height {
    let mut x = 0;
    while x < width {
      if cell(width, cells, x, y) != TileCell::OneWay {
        x += 1;
        continue;
      }
      let start = x;
      while x < width && cell(width, cells, x, y) == TileCell::OneWay {
        x += 1;
      }
      runs.push(((start as i32, y as i32), (x as i32, y as i32)));
    }
  }
  runs
}

/// Solid cells greedily merged into rectangles: (min corner, size).
pub(crate) fn solid_rects(width: u32, height: u32, cells: &[TileCell]) -> Vec<(Corner, Corner)> {
  let solid = |x: u32, y: u32| cell(width, cells, x, y) == TileCell::Solid;
  let mut taken = vec![false; width as usize * height as usize];
  let index = |x: u32, y: u32| y as usize * width as usize + x as usize;
  let mut rects = Vec::new();
  for y in 0..height {
    for x in 0..width {
      if !solid(x, y) || taken[index(x, y)] {
        continue;
      }
      let mut w = 1;
      while x + w < width && solid(x + w, y) && !taken[index(x + w, y)] {
        w += 1;
      }
      let mut h = 1;
      while y + h < height && (x..x + w).all(|cx| solid(cx, y + h) && !taken[index(cx, y + h)]) {
        h += 1;
      }
      for ty in y..y + h {
        for tx in x..x + w {
          taken[index(tx, ty)] = true;
        }
      }
      rects.push(((x as i32, y as i32), (w as i32, h as i32)));
    }
  }
  rects
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ryo2d --lib physics::tiles`
Expected: PASS.

- [ ] **Step 5: Gate and commit**

```bash
git add -A
git commit -m "feat: tile outlines" -m "- solid and 45° slope cells become seamless closed outlines, holes included
- one-way runs and greedy solid rectangles"
```

---

### Task 14: `TileColliders`, the tilemap adapter, navigation

**Files:**
- Modify: `src/core/physics/tiles.rs` (the component, its colliders, `TilePart`, `rebuild_tile_colliders`; tests)
- Modify: `src/core/physics/sync.rs` (`register_physics_systems` adds `rebuild_tile_colliders` to `FixedSet::Movement`)
- Modify: `src/core/physics/mod.rs`, `src/lib.rs` (export `TileColliders`)
- Modify: `src/core/tilemap/map.rs` (`Tilemap::tile_colliders`; test), `src/core/tilemap/AGENTS.md`
- Modify: `src/core/navigation/grid.rs` (`block_tiles`; `rebuild_from_world`; tests), `src/core/navigation/AGENTS.md`
- Modify: `src/core/physics/AGENTS.md` (a "Tile Colliders" section)

**Interfaces:**
- Consumes: Task 13's functions, Task 3's `Collider::one_way`.
- Produces: `pub struct TileColliders { pub tile_size: Vec2, pub layer: u32, pub mask: u32, pub friction: f32, pub restitution: f32, pub solid_rects: bool, .. }` (requires `Transform`) with `new(width, height, tile_size)`, `from_cells(width, height, tile_size, cells)`, `set(x, y, cell) -> bool`, `get(x, y) -> TileCell`, `width()`, `height()`, `with_layer/with_mask/with_friction/with_restitution`, `solid_rects()`, and crate-internal `colliders(&self) -> Vec<Collider>`; `pub(crate) struct TilePart` (marks the colliders it built); `Tilemap::tile_colliders(&self) -> TileColliders`; `NavigationGrid::block_tiles(&mut self, tiles: &TileColliders, origin: Vec2)`.

- [ ] **Step 1: Write the failing tests**

`tiles.rs` (a second tests module, with physics):

```rust
#[cfg(test)]
mod with_physics {
  use super::*;
  use crate::core::ecs::{ChildOf, Transform, Velocity, World};
  use crate::core::physics::test_util::*;
  use crate::core::physics::{CharacterController, Collider, ColliderShape, RigidBody};

  fn parts(world: &mut World, map: Entity) -> Vec<Collider> {
    let mut query = world.query::<(&ChildOf, &Collider)>();
    query
      .iter(world)
      .filter(|(child_of, _)| child_of.parent() == map)
      .map(|(_, collider)| collider.clone())
      .collect()
  }

  #[test]
  fn a_grid_builds_its_colliders_and_rebuilds_when_a_cell_changes() {
    let (mut world, mut s) = world(60);
    let map = world
      .spawn((Transform::IDENTITY, TileColliders::from_cells(4, 1, Vec2::splat(16.0), vec![TileCell::Solid; 4])))
      .id();
    run(&mut world, &mut s, 1);
    assert_eq!(parts(&mut world, map).len(), 1);
    world.get_mut::<TileColliders>(map).unwrap().set(1, 0, TileCell::Empty);
    run(&mut world, &mut s, 1);
    assert_eq!(parts(&mut world, map).len(), 2, "two outlines, the old one gone");
  }

  #[test]
  fn one_way_cells_become_one_way_segments_and_rects_mode_makes_rects() {
    let (mut world, mut s) = world(60);
    let cells = vec![TileCell::OneWay, TileCell::OneWay, TileCell::Solid, TileCell::Solid];
    let map = world
      .spawn((Transform::IDENTITY, TileColliders::from_cells(2, 2, Vec2::splat(16.0), cells).solid_rects()))
      .id();
    run(&mut world, &mut s, 1);
    let built = parts(&mut world, map);
    assert_eq!(built.len(), 2);
    assert!(built.iter().any(|c| c.one_way.is_some() && matches!(c.shape, ColliderShape::Segment { .. })));
    assert!(built.iter().any(|c| matches!(c.shape, ColliderShape::Rect { .. })));
  }

  #[test]
  fn a_box_sliding_across_a_hundred_tiles_never_stops() {
    let (mut world, mut s) = world(60);
    world.spawn((
      Transform::from_xy(0.0, 100.0),
      TileColliders::from_cells(100, 1, Vec2::splat(16.0), vec![TileCell::Solid; 100]).with_friction(0.0),
    ));
    let crate_ = world
      .spawn((
        Transform::from_xy(20.0, 94.0),
        RigidBody::dynamic().lock_rotation(),
        Collider::rect(12.0, 12.0).with_friction(0.0),
        Velocity::new(300.0, 0.0),
      ))
      .id();
    run(&mut world, &mut s, 240);
    let x = at(&world, crate_).translation.x;
    assert!(x - 20.0 >= 0.95 * 300.0 * 4.0, "snagged at {x}");
  }

  #[test]
  fn a_character_running_across_a_hundred_tiles_never_snags() {
    let (mut world, mut s) = world(60);
    world.spawn((
      Transform::from_xy(0.0, 100.0),
      TileColliders::from_cells(100, 1, Vec2::splat(16.0), vec![TileCell::Solid; 100]),
    ));
    let e = world
      .spawn((Transform::from_xy(20.0, 79.0), CharacterController::platformer(), Collider::rect(12.0, 40.0)))
      .id();
    run(&mut world, &mut s, 10);
    let x0 = at(&world, e).translation.x;
    for _ in 0..240 {
      let mut v = world.get_mut::<Velocity>(e).unwrap();
      v.dx = 300.0;
      v.dy += 980.0 / 60.0;
      run(&mut world, &mut s, 1);
    }
    let travelled = at(&world, e).translation.x - x0;
    assert!(travelled >= 0.99 * 300.0 * 4.0, "{travelled}");
  }

  #[test]
  fn a_bad_tile_size_builds_nothing() {
    let (mut world, mut s) = world(60);
    let map = world
      .spawn((Transform::IDENTITY, TileColliders::from_cells(1, 1, Vec2::ZERO, vec![TileCell::Solid])))
      .id();
    run(&mut world, &mut s, 1);
    assert!(parts(&mut world, map).is_empty());
  }
}
```

`tilemap/map.rs` tests:

```rust
  #[test]
  fn a_tilemap_gives_tile_colliders_from_its_properties() {
    use crate::core::physics::{SlopeCorner, TileCell};
    let mut map = Tilemap::new(16, 16, 3, 1);
    let mut tileset = Tileset { name: "t".into(), image_path: String::new(), tile_width: 16, tile_height: 16, columns: 4, tile_count: 4, spacing: 0, margin: 0, image_width: 64, image_height: 16, properties: Default::default() };
    tileset.properties.insert(0, TileProperties { solid: true, ..Default::default() });
    tileset.properties.insert(1, TileProperties { platform: true, ..Default::default() });
    let mut slope = TileProperties::default();
    slope.custom.insert("slope".into(), "bottom-right".into());
    tileset.properties.insert(2, slope);
    map.add_tileset(tileset);
    let mut layer = TilemapLayer::new("ground", 3, 1);
    for (x, id) in [(0, 0), (1, 1), (2, 2)] {
      layer.set_tile(x, 0, id);
    }
    map.add_layer(layer);
    let tiles = map.tile_colliders();
    assert_eq!(tiles.get(0, 0), TileCell::Solid);
    assert_eq!(tiles.get(1, 0), TileCell::OneWay);
    assert_eq!(tiles.get(2, 0), TileCell::Slope(SlopeCorner::BottomRight));
    assert_eq!(tiles.tile_size, crate::core::ecs::Vec2::splat(16.0));
  }
```


`navigation/grid.rs` tests:

```rust
  #[test]
  fn block_tiles_blocks_exactly_the_solid_cells() {
    use crate::core::physics::{TileCell, TileColliders};
    let cells = vec![TileCell::Solid, TileCell::Empty, TileCell::OneWay, TileCell::Solid];
    let tiles = TileColliders::from_cells(2, 2, Vec2::splat(10.0), cells);
    let mut grid = NavigationGrid::new(40.0, 40.0, 10.0);
    grid.block_tiles(&tiles, Vec2::new(10.0, 10.0));
    assert!(!grid.is_walkable(GridCoord::new(1, 1)));
    assert!(grid.is_walkable(GridCoord::new(2, 1)));
    assert!(grid.is_walkable(GridCoord::new(1, 2)), "one-way cells are walkable");
    assert!(!grid.is_walkable(GridCoord::new(2, 2)));
    assert!(grid.is_walkable(GridCoord::new(3, 3)));
  }

  #[test]
  fn rebuilding_from_the_world_keeps_the_inside_of_a_tile_u_open() {
    use crate::core::physics::{TileCell, TileColliders};
    let (s, e) = (TileCell::Solid, TileCell::Empty);
    let mut world = World::new();
    let map = world
      .spawn((Transform::IDENTITY, TileColliders::from_cells(3, 2, Vec2::splat(10.0), vec![s, e, s, s, s, s])))
      .id();
    // As the engine would have built it: an outline collider child
    world.spawn((
      Transform::IDENTITY,
      GlobalTransform::IDENTITY,
      Collider::polyline([Vec2::ZERO, Vec2::new(30.0, 0.0), Vec2::new(30.0, 20.0), Vec2::ZERO]),
      crate::core::physics::TilePart,
      crate::core::ecs::ChildOf(map),
    ));
    let mut grid = NavigationGrid::new(30.0, 20.0, 10.0);
    grid.rebuild_from_world(&mut world);
    assert!(grid.is_walkable(GridCoord::new(1, 0)), "the U's inside");
    assert!(!grid.is_walkable(GridCoord::new(0, 0)));
    assert!(!grid.is_walkable(GridCoord::new(1, 1)));
  }
```

(`Transform` and `GlobalTransform` come from `crate::core::ecs`; `TilePart` must be reachable from `navigation` — `pub(crate)` in `physics`, re-exported `pub(crate) use tiles::TilePart;` in `physics/mod.rs`.)

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p ryo2d --lib tiles tilemap navigation`
Expected: compile errors.

- [ ] **Step 3: Implement**

In `tiles.rs`:

```rust
/// A grid of tile cells that builds its own colliders: seamless outlines
/// around solid and slope cells (or merged rectangles with `solid_rects()`)
/// and one-way segments along runs of one-way cells. Its `Transform` places
/// cell (0, 0)'s top-left; +Y is down. The colliders are child entities,
/// rebuilt (all of them) on the next step after any change. With no body
/// they are fixed; with a kinematic `RigidBody` the whole map moves.
///
/// Outlines are hollow: a body that ends up inside a solid region is not
/// pushed out. `solid_rects()` trades the seamless floors for solid
/// interiors.
#[derive(Component, Debug, Clone, PartialEq)]
#[non_exhaustive]
#[require(Transform)]
pub struct TileColliders {
  width: u32,
  height: u32,
  cells: Vec<TileCell>,
  /// One cell's size, px
  pub tile_size: Vec2,
  pub layer: u32,
  pub mask: u32,
  pub friction: f32,
  pub restitution: f32,
  /// Build merged rectangles instead of outlines
  pub solid_rects: bool,
}

impl TileColliders {
  /// An empty grid.
  pub fn new(width: u32, height: u32, tile_size: Vec2) -> Self {
    Self::from_cells(width, height, tile_size, Vec::new())
  }

  /// A grid from its cells, row by row from the top; missing cells are
  /// empty and extra ones are dropped.
  pub fn from_cells(width: u32, height: u32, tile_size: Vec2, mut cells: Vec<TileCell>) -> Self {
    cells.resize(width as usize * height as usize, TileCell::Empty);
    Self {
      width,
      height,
      cells,
      tile_size,
      layer: 1,
      mask: u32::MAX,
      friction: 0.5,
      restitution: 0.0,
      solid_rects: false,
    }
  }

  pub fn width(&self) -> u32 {
    self.width
  }

  pub fn height(&self) -> u32 {
    self.height
  }

  /// The cell at (x, y); `Empty` outside the grid.
  pub fn get(&self, x: u32, y: u32) -> TileCell {
    if x < self.width && y < self.height {
      self.cells[y as usize * self.width as usize + x as usize]
    } else {
      TileCell::Empty
    }
  }

  /// Change a cell (the colliders rebuild next step); false outside the grid.
  pub fn set(&mut self, x: u32, y: u32, cell: TileCell) -> bool {
    if x >= self.width || y >= self.height {
      return false;
    }
    self.cells[y as usize * self.width as usize + x as usize] = cell;
    true
  }

  pub fn with_layer(mut self, layer: u32) -> Self {
    self.layer = layer;
    self
  }

  pub fn with_mask(mut self, mask: u32) -> Self {
    self.mask = mask;
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

  /// Merged rectangles instead of outlines (solid interiors, seamed floors).
  pub fn solid_rects(mut self) -> Self {
    self.solid_rects = true;
    self
  }

  fn material(&self, collider: Collider) -> Collider {
    collider
      .with_layer(self.layer)
      .with_mask(self.mask)
      .with_friction(self.friction)
      .with_restitution(self.restitution)
  }

  /// The colliders the grid needs, in its own space.
  pub(crate) fn colliders(&self) -> Vec<Collider> {
    let size = self.tile_size;
    let px = |(x, y): Corner| Vec2::new(x as f32 * size.x, y as f32 * size.y);
    let mut built = Vec::new();
    if self.solid_rects {
      for (min, cells) in solid_rects(self.width, self.height, &self.cells) {
        let extent = px(cells);
        let centre = px(min) + extent / 2.0;
        built.push(self.material(Collider::rect(extent.x, extent.y).with_offset(centre.x, centre.y)));
      }
      for y in 0..self.height {
        for x in 0..self.width {
          if let TileCell::Slope(_) = self.get(x, y) {
            let points = corners(self.get(x, y))
              .iter()
              .map(|c| px((x as i32 + c.0, y as i32 + c.1)));
            built.push(self.material(Collider::polygon(points)));
          }
        }
      }
    } else {
      for outline in outlines(self.width, self.height, &self.cells) {
        built.push(self.material(Collider::polyline(outline.into_iter().map(px))));
      }
    }
    for (a, b) in one_way_runs(self.width, self.height, &self.cells) {
      built.push(self.material(Collider::segment(px(a), px(b)).one_way()));
    }
    built
  }
}

/// Marks a collider a `TileColliders` built; its parent owns it.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct TilePart;

/// `FixedSet::Movement`: rebuild the colliders of every grid that changed.
pub(crate) fn rebuild_tile_colliders(
  maps: Query<(Entity, &TileColliders, Option<&Children>), Changed<TileColliders>>,
  parts: Query<(), With<TilePart>>,
  mut commands: Commands,
  mut warned: Local<EntityHashSet>,
) {
  for (map, tiles, children) in &maps {
    for &child in children.map(|children| &**children).unwrap_or(&[]) {
      if parts.contains(child) {
        commands.entity(child).despawn();
      }
    }
    if !(tiles.tile_size.is_finite() && tiles.tile_size.min_element() > 0.0) {
      if warned.insert(map) {
        crate::engine_warn!(module: "Physics", "{map}: TileColliders needs a positive tile size; no colliders built");
      }
      continue;
    }
    for collider in tiles.colliders() {
      commands.spawn((Transform::IDENTITY, collider, TilePart, ChildOf(map)));
    }
  }
}
```

(Imports: `Changed, ChildOf, Children, Commands, Component, Entity, EntityHashSet, Local, Query, Transform, Vec2, With` from `crate::core::ecs`; `Collider` and `ColliderShape` from `super::collider`. Check how `Children` derefs to a slice in bevy 0.20 and adapt the loop.)

Register in `register_physics_systems`:

```rust
  schedules.add_systems(Phase::FixedUpdate, tiles::rebuild_tile_colliders.in_set(FixedSet::Movement));
```

`tilemap/map.rs`:

```rust
  /// Tile colliders for this map, placed at the map's origin: tiles whose
  /// properties are `solid` are solid, `platform` tiles are one-way, and a
  /// `slope` custom property (`"bottom-left"`, `"bottom-right"`,
  /// `"top-left"`, `"top-right"`) makes a 45° slope with that corner
  /// filled. Layers combine: solid beats a slope, a slope beats one-way.
  pub fn tile_colliders(&self) -> TileColliders {
    let rank = |cell: TileCell| match cell {
      TileCell::Empty => 0,
      TileCell::OneWay => 1,
      TileCell::Slope(_) => 2,
      TileCell::Solid => 3,
    };
    let size = Vec2::new(self.tile_width as f32, self.tile_height as f32);
    let mut tiles = TileColliders::new(self.map_width, self.map_height, size);
    for layer in &self.layers {
      let Some(tileset) = self.tilesets.get(layer.tileset_index) else {
        continue;
      };
      for y in 0..self.map_height.min(layer.height) {
        for x in 0..self.map_width.min(layer.width) {
          let Some(id) = layer.get_tile(x, y) else {
            continue;
          };
          let properties = tileset.get_properties(id);
          let cell = if properties.solid {
            TileCell::Solid
          } else if let Some(corner) = properties.custom.get("slope").and_then(|name| SlopeCorner::from_name(name)) {
            TileCell::Slope(corner)
          } else if properties.platform {
            TileCell::OneWay
          } else {
            continue;
          };
          if rank(cell) > rank(tiles.get(x, y)) {
            tiles.set(x, y, cell);
          }
        }
      }
    }
    tiles
  }
```

`navigation/grid.rs`:

```rust
  /// Block the cells under a tile grid's solid and slope cells, its cell
  /// (0, 0)'s top-left at `origin` (its entity's position). One-way cells
  /// stay walkable.
  pub fn block_tiles(&mut self, tiles: &TileColliders, origin: Vec2) {
    let size = tiles.tile_size;
    if !(size.is_finite() && size.min_element() > 0.0) {
      return;
    }
    for y in 0..tiles.height() {
      for x in 0..tiles.width() {
        if matches!(tiles.get(x, y), TileCell::Solid | TileCell::Slope(_)) {
          let min = origin + Vec2::new(x as f32, y as f32) * size;
          // Just inside the far edges, so a tile does not block its neighbours' cells
          let max = min + size - Vec2::splat(1e-3);
          self.block_rect(min.x, min.y, max.x, max.y);
        }
      }
    }
  }
```

and `rebuild_from_world` skips colliders with `TilePart` (`world.query_filtered::<(Entity, &GlobalTransform, &Collider), Without<TilePart>>()`) and then blocks every tile grid cell-exactly (`world.query::<(&GlobalTransform, &TileColliders)>()`, collected first, `block_tiles(tiles, global.translation())`); update its doc ("tile grids block exactly their solid cells; their outline colliders would block their bounding boxes").

Docs: physics AGENTS "Tile Colliders" (the edge-cancelling outline, sharpest turn at shared corners, hollow outlines and `solid_rects`, rebuild on change, `TilePart`), tilemap AGENTS (`tile_colliders`, the `slope` property; the old `generate_collision_rects` stays for code that wants rectangles), navigation AGENTS (`block_tiles`, rebuild).

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ryo2d --lib`
Expected: PASS.

- [ ] **Step 5: Gate and commit**

```bash
git add -A
git commit -m "feat: TileColliders" -m "- a grid component that builds seamless outline colliders, one-way segments and slopes, or merged rectangles
- Tilemap::tile_colliders reads solid, platform and slope tile properties
- NavigationGrid::block_tiles; rebuilding from the world blocks tile grids cell-exactly"
```

---

### Task 15: bullet-hell on the platformer preset

**Files:**
- Modify: `examples/games/bullet-hell/src/components/player.rs` (delete `PlayerVelocity`, `WallSide` and the wall fields and methods of `Player`; keep lives and invincibility)
- Modify: `examples/games/bullet-hell/src/scenes/game/player.rs` (delete the movement, wall and collision code; keep invincibility visuals; squash and stretch on `MovementEvent`)
- Modify: `examples/games/bullet-hell/src/scenes/game/spawning.rs` (player, platforms, walls and ground), `combat.rs` (bullet hits), `mod.rs` (update order), `constants.rs` (drop the constants the preset replaces)

**Interfaces:**
- Consumes: `CharacterController`, `PlatformerMovement` (with wall slide and wall jump), `MoveBindings`, `MovementEvent`, `Collider::one_way`, `TriggerEntered`, `CharacterState`.

- [ ] **Step 1: Record the current behaviour**

Run bullet-hell headless for 600 frames with scripted input (`RYO_HEADLESS=1 RYO_CAPTURE_FRAMES=150,300,450,600 RYO_CAPTURE_EXIT=1 RYO_CAPTURE_DIR=<abs dir>/before RYO_INPUT="60:+KeyD 90:Space 150:-KeyD 160:+KeyA 200:Space 260:-KeyA" cargo run -p bullet-hell`; check the `RYO_INPUT` syntax in the `ryo-cli` skill). Keep the captures for comparison.

- [ ] **Step 2: Port the player**

- `spawn_player`: spawn `(Transform, Interpolated, CharacterController::platformer(), Collider::rect(PLAYER_SIZE, PLAYER_SIZE), PlatformerMovement::default().with_run(PLAYER_SPEED, 0.0, 0.0).with_air_control(1.0).with_jump(154.0, 0.56).with_fall(1.0, 2000.0).with_jump_cut(1.0).with_wall_slide(WALL_SLIDE_MAX_SPEED).with_wall_jump(Vec2::new(WALL_JUMP_FORCE_X, WALL_JUMP_FORCE_Y), WALL_JUMP_PUSH_DURATION), MoveBindings::default().with_jump([KeyCode::Space, KeyCode::KeyW]), Player::new(..))`. The numbers keep the old feel: instant run at 600 px/s, a 154 px jump reaching its top in 0.56 s (550 px/s at 980 px/s²), symmetric gravity, no jump cut, the old wall slide cap and wall jump.
- Add the screen's walls and ground as fixed colliders the old code clamped against: a ground rect whose top is at `screen_height - 30`, and walls just outside `x = 0` and `x = screen_width` tall enough to cover the play area.
- Platforms (`spawn_platforms`): `Collider::rect(..).one_way()` (the old code landed on them only while falling). Barriers stay kinematic and moved by `update_barriers` through `Transform`; the controller rides their tops and slides down their sides (wall slide), replacing `apply_barrier_riding`, `check_player_barrier_collision` and `check_wall_slide_on_barrier`.
- `update_player` keeps the invincibility timer and flash; the movement, gravity, wall state, screen clamping and every `check_player_*_collision` that only resolved movement go. Squash and stretch: read `ctx.messages::<MovementEvent>()` and call `stretch(ctx, player)` on `Jumped` and `WallJumped` for the player.
- Contact damage from turrets and drones (`check_player_turret_collision`, `check_player_drone_collision`): if the old code only detected overlap (the player could stand in a turret), make those enemies' colliders triggers and damage on `TriggerEntered` with the player as `actor()`; otherwise damage when the player's `CharacterState::hits` contains the enemy. The player is never pushed by an enemy it could overlap before.
- Bullets (`combat.rs`): keep them dynamic triggers; replace the hand distance and AABB tests against the player, platforms and barriers with `TriggerEntered` (bullet as `trigger`, the player/platform/barrier as `actor()`), keeping the out-of-bounds cull.
- Remove `PlayerVelocity` everywhere; enemies that aimed with the player's `Transform` keep doing so.

- [ ] **Step 3: Check it**

`cargo clippy -p bullet-hell -- -D warnings`, then the same headless run as Step 1 into `<abs dir>/after`. Compare by eye: the player lands on platforms and the ground, rides a barrier, wall-slides on a screen edge and a barrier, and bullets still hit and vanish on platforms. Add `examples/games/bullet-hell/tests/headless.rs` (the crate's `lib` exposes `scenes`): start the game scene with `HeadlessEngine::new_or_skip` (it returns `None` without a GPU), hold `KeyD` for 60 frames, tap `Space`, run 120 frames, and assert the player's `CharacterState::on_floor` is true and that it moved right; add `ryo2d` with its `testing` module as a dev-dependency if the crate needs it.

- [ ] **Step 4: Gate and commit**

```bash
git add -A
git commit -m "refactor: bullet-hell's player on the platformer preset" -m "- the controller, one-way platforms and wall slide replace its hand movement and collision code
- bullet hits use trigger messages"
```

---

### Task 16: leap-king and the networking demo on the controller

**Files:**
- Modify: `examples/games/leap-king/src/scenes/game.rs` (spawn, `update_physics`, landing, walls, platforms, checkpoints, respawn, player collisions), `src/components/player.rs` if needed, `src/constants.rs`
- Modify: `examples/demos/networking/src/scenes/game.rs`, `src/components/player.rs`, `src/constants.rs`

**Interfaces:**
- Consumes: `CharacterController`, `CharacterState::{on_floor, landed, left_floor, on_wall, wall_normal, velocity_before, floor}`, `Collider::one_way`, `Knockback`, `Checkpoint`, `Respawn`, `CheckpointReached`.

- [ ] **Step 1: Record the current behaviour**

Headless single-player runs of leap-king (charge with `Space` held 30 frames, aim with `KeyD`, release; repeat) and the networking demo's local mode, captured before the change as in Task 15.

- [ ] **Step 2: Port leap-king**

- **Who gets a controller:** single player and the host simulate every player, so every player there gets `CharacterController::platformer()`; a client gives one to its local player only. Remote players on a client keep their bare collider (no body) and stay `Transform`-driven, so `dead_reckon_remotes` and the server corrections work as now. Decide in `spawn_player` from `self.mode` and `is_local`.
- **Players don't block each other:** put players on their own layer (`with_layer(PLAYER_LAYER)`, `with_mask(WORLD_LAYER)`, with `PLAYER_LAYER = 2`, `WORLD_LAYER = 1`; platforms and walls keep the default layer 1), so the controller ignores other players and `check_player_collisions` keeps its overlap rules (head bounce, air push, side push).
- **The player's `Transform` is its top-left corner** and its collider is offset by half its size; the controller sweeps that collider, which is fine.
- **Movement:** while airborne, the game adds `GRAVITY * dt` to `Velocity.dy` as before; the controller moves the player. Delete the position integration, the grounded-support re-check and `check_platform_landing`. Read `CharacterState` instead: `landed` → zero `Velocity.dx`, `grounded = true`, `LeapState::Landing { timer: LANDING_RECOVERY_TIME }` and the land sound; `left_floor` while not leaping → `grounded = false`, `LeapState::Airborne`; `on_wall` while airborne → bounce: `vel.dx = -velocity_before.x * WALL_BOUNCE_FACTOR` using `CharacterState::velocity_before` (the controller already zeroed `dx`).
- **Walls:** two fixed colliders at `x = -10` and `x = screen_width + 10`, 20 px wide and tall enough for the whole tower (the tower grows upward, toward negative y), replacing the screen-edge clamp.
- **Platforms:** `spawn_platform_entity` gives each platform `Collider::rect(width, PLATFORM_HEIGHT).with_offset(..).one_way()` (keep its top-left placement); moving platforms also get `RigidBody::kinematic()`, so `update_moving_platforms`' `Transform` writes carry riders.
- **Pushes:** `Knockback::apply_with_decay` on a controlled player now goes through the sweep, so a shove stops at a wall. The air-to-air push and head bounce keep writing `Velocity`.
- **Checkpoints:** a `Checkpoint` trigger spanning each checkpoint platform's width, about 1.5 player heights tall, standing on it; players get `Respawn::here()`. Replace the height-threshold detection (`find_checkpoint_platform_at_y` and its callers) with `ctx.messages::<CheckpointReached>()`: update that player's `PlayerCheckpoint` and the platform's visuals the way the old detection did. `execute_respawn` places the player at its `Respawn::point` (its spawn rule: the checkpoint's spawn x and the platform top minus the player height, as `checkpoint_spawn_x` computes, set as the checkpoint's offset) and zeroes `Velocity`. The fall-death rule (`DEATH_Y_OFFSET`) and the respawn delay stay game code.
- **Networking:** reconcile and respawn RPCs write `Transform`, `Velocity` and `Player` as before; for a controlled player a `Transform` write is a teleport. Nothing in `src/core/network/` changes.

- [ ] **Step 3: Port the networking demo**

The same pattern: its `PlayerVelocity` becomes the engine `Velocity` on players with `CharacterController::platformer()` (the host's and the local one), hand gravity stays game code, `update_physics`' landing becomes a one-way platform and `CharacterState`, players on their own layer as in leap-king, and the push keeps editing velocity.

- [ ] **Step 4: Check them**

`cargo clippy -p leap-king -p networking -- -D warnings`, then the headless runs from Step 1 after the change: players charge, leap, land on platforms (including a moving one), bounce off the walls and respawn after a fall. Add `examples/games/leap-king/tests/headless.rs` as in Task 15 (single-player mode: hold `Space` 30 frames with `KeyD`, release, run 120 frames; assert the local player is `on_floor` higher than it started or on the starting platform, and no panic). The owner checks the networked level by eye later; say so in the report.

- [ ] **Step 5: Gate and commit**

```bash
git add -A
git commit -m "refactor: leap-king and the networking demo on the character controller" -m "- one-way and moving platforms, wall bounce and landing come from the controller
- checkpoints are Checkpoint triggers; pushes stop at walls"
```

---

### Task 17: The `character` demo

**Files:**
- Create: `examples/demos/character/{Cargo.toml, Ryo.toml, Trunk.toml, index.html, main.rs}` (copy the `physics` demo's four config files, renaming `physics-demo` to `character-demo` and the titles to "Character")
- Modify: `Cargo.lock` (the new workspace member)

**Interfaces:**
- Consumes: everything above; `demo_kit::prelude::*`.

- [ ] **Step 1: Write the demo** (`main.rs`; demo-kit standard: arrows belong to the settings panel, so pages move with WASD; `Space` jumps, `Shift` dashes; loads no files)

```rust
//! Character: the controller and its presets on a tile level, top-down
//! movement, and custom movement on the bare controller. Loads no files, so
//! it runs on the web too.

use demo_kit::prelude::*;

fn main() {
  Demo::new("Character")
    .resolution(960, 640)
    .page(PlatformerPage::default)
    .page(TopDownPage::default)
    .page(CustomPage::default)
    .run();
}

/// WASD moves (the arrows belong to the panel), Space jumps, Shift dashes.
fn bindings() -> MoveBindings {
  MoveBindings::default()
    .with_left([KeyCode::KeyA])
    .with_right([KeyCode::KeyD])
    .with_up([KeyCode::KeyW])
    .with_down([KeyCode::KeyS])
}
```

Pages (each draws its colliders with `ctx.debug_draw()` outlines as the `physics` demo does, and keeps its play area left of x ≈ 520, clear of the settings panel):

1. **Platformer** (`PlatformerPage`): a `TileColliders` level built from a string grid (`#` solid, `=` one-way, `/` and `\` 45° slopes as `Slope(BottomRight)`/`Slope(BottomLeft)`, `.` empty) at 16 px tiles; a kinematic platform moving left and right and one moving up and down (`Velocity`, reversed at their ends in `update`); three dynamic crates; coins (`Pickup::new().respawn_after(3.0)`, small trigger rects); spikes (`KillZone::new()`); two flags (`Checkpoint`). The player: `CharacterController::platformer()`, `Collider::rect(14, 26)`, `PlatformerMovement`, `bindings()`, `Respawn::here()`. Settings (applied to the player's `PlatformerMovement` every update): run speed, jump height, time to apex, coyote time, jump buffer, jump cut, air jumps (int 0-2), wall slide (toggle), wall jump (toggle), dash (toggle). Controls: `Control::keys("WASD", &[..], "Move")`, `Space` jump (down + Space drops through one-way floors), `Shift` dash. The UI shows coins collected (count `PickedUp`), deaths (`Killed`) and the last `MovementEvent`.
2. **Top-down** (`TopDownPage`): a tile-walled room with pillars, crates to push, coins; the player with `CharacterController::top_down()`, `Collider::circle(9)`, `TopDownMovement` (settings: max speed, acceleration time, deceleration time, dash toggle), `bindings()`.
3. **Custom** (`CustomPage`): the bare controller with movement written in `update`: hold `Space` to charge a leap aimed with A/D (as leap-king does), bounce off walls from `velocity_before`; a live readout of `CharacterState` (on floor/wall/ceiling, the normals, `floor`, `platform_velocity`, `hits.len()`).

Write the pages fully in the demo's own style (see `examples/demos/physics/main.rs`); every setting is a field read in `update`.

- [ ] **Step 2: Check it**

`./dev demos:tour character` and look at the three PNGs (`target/demo-tour/character/page-*.png`); run it windowed (`./dev character` or `cargo run -p character-demo`) and play each page; build it for the web (`trunk build` in its folder, as the `physics` demo is) and open it in Chromium, WebKit and Firefox with the Playwright tools (clean up `.playwright-mcp/` afterwards); note any console errors.

- [ ] **Step 3: Gate and commit**

```bash
git add -A
git commit -m "feat: character demo" -m "- a platformer page on a tile level with every preset option, pickups, spikes and checkpoints
- top-down and custom-movement pages"
```

---

### Task 18: Documentation and the benchmark

**Files:**
- Create: `.claude/skills/ryo-character/SKILL.md` and its byte-identical copy `src/cli/skills/ryo-character.md` (and register the new skill wherever the CLI lists embedded skills: search `src/cli/` for `ryo-physics.md`)
- Modify: `.claude/skills/ryo-physics/SKILL.md`, `.claude/skills/ryo-api/SKILL.md`, `.claude/skills/ryo-game-dev/SKILL.md` and their `src/cli/skills/` copies
- Modify: `src/core/physics/AGENTS.md`, `src/core/gameplay/AGENTS.md`, `src/core/AGENTS.md`, `src/core/ecs/AGENTS.md`, `docs/architecture/overview.md`
- Modify: `benches/physics.rs`

- [ ] **Step 1: The skills**

`ryo-character` (new): when to use the core controller, a preset, or both; `CharacterController` (modes, every builder, the `Velocity` contract, `CharacterState` fields and normal convention, teleporting, pushing, platforms, limits); one-way platforms and drop-through; `MoveIntent`/`MoveBindings` (and an AI writing `MoveIntent`); `PlatformerMovement` and `TopDownMovement` (every option, defaults, `MovementEvent`, `PlatformerState`); triggers (`TriggerEntered`/`TriggerExited`, `actor()`, `TriggerContents`) and the behaviours (`Pickup`, `KillZone`, `Checkpoint`, `Respawn`); `TileColliders` and `Tilemap::tile_colliders`; knockback per target. Recipes, each a complete snippet that compiles against the prelude: a platformer player with bindings; a top-down player; an enemy that patrols by writing `MoveIntent`; custom movement on the bare controller (a charge jump with a wall bounce); a level from a string grid with coins, spikes and checkpoints. `ryo-physics`: link to `ryo-character`, one-way colliders, trigger messages, knockback. `ryo-api`: every new signature from Tasks 2-14. `ryo-game-dev`: move its platformer and pickup recipes onto the presets and behaviours. Keep each pair byte-identical (`cmp`).

- [ ] **Step 2: The AGENTS files and the overview**

Check every section the tasks added reads as one document: physics (the step as one system, Characters, One-Way Colliders, Triggers, Tile Colliders), gameplay (the `Movement` chain and order, input, presets, behaviours, knockback, test helpers), core and ecs (the `Movement` set, the tick clamp). `docs/architecture/overview.md`: a gameplay paragraph next to physics.

- [ ] **Step 3: The benchmark**

Add a scene to `benches/physics.rs`: a 120×40 `TileColliders` level of 16 px tiles (a floor, steps and one-way rows) with 100 characters (`CharacterController::platformer()`, `Collider::rect(12, 24)`, `PlatformerMovement`) whose `MoveIntent` a small scene system drives (direction flips every 2 s per character, offset by index; jump every 1.5 s), timed with the same fresh-engine windows as the bouncing scenes (bench name `physics frame, 100 characters on tiles`). Run `cargo bench --bench physics -- --noplot` with nothing else CPU-heavy running and record the result (and the bouncing scenes, to show they did not move) in the report.

- [ ] **Step 4: Final checks**

- `./dev demos:tour` (every demo; look at every page that changed: knockback, physics, character) and the five games captured headless (breakout, flappy, bullet-hell, leap-king, nigh_t) and checked by eye.
- The task-end gate.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "docs: character controller, presets, triggers and tiles" -m "- a ryo-character skill; physics, api and game-dev skills updated
- the physics benchmark times 100 characters on a tile level"
```
