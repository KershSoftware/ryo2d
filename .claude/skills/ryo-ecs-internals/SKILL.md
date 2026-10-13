---
name: ryo-ecs-internals
description: Working on Ryo2D's ECS (bevy_ecs presented as ryo2d::ecs) - kernel re-exports, components, propagation, interpolation, pooling and engine systems.
metadata:
  version: 2.0.0
---

# Ryo2D ECS Internals

You help contributors work on `src/core/ecs/` and the engine's systems. Ryo2D's ECS is `bevy_ecs` 0.20, presented as Ryo2D's own: games import it through `ryo2d::prelude`/`ryo2d::ecs` and never name `bevy_ecs`.

## Read First

| Doc | Covers |
|-----|--------|
| `src/core/ecs/AGENTS.md` | Phases and sets, the resource-scope rule, transform and hierarchy, interpolation, `Disabled` and pooling, resources as entities, messages |
| `src/core/systems/AGENTS.md` | The built-in systems, their sets, conventions, adding a system |
| `ryo-api` skill | Public signatures |
| `ryo-game-dev` skill | How games use all of this |

This skill adds the working knowledge for changing that code; the AGENTS.md files are the reference for the rules.

## Module Map

```
src/core/ecs/
  kernel.rs          the curated bevy_ecs re-exports (the only file naming the crate)
  transform.rs       Anchor, Transform (+ insert hook), GlobalTransform, PreviousGlobalTransform, Interpolated
  propagate.rs       propagate_transforms
  interpolate.rs     snapshot_previous_transforms, render_global
  world_ext.rs       WorldExt (spawn_child, detach_keep_world, skip_interpolation), despawn_game_entities
  pooling.rs         Pool, PoolConfig, PoolExhaustedPolicy, PoolStats
  components.rs      Velocity, ZLayer
  renderable.rs      Renderable, RenderStyle, DropShadow, BlobShadow, ContactShadow
  material.rs        Material, SpriteShader, ShaderHandle, ShaderParams, ParamValue
  sprite_effects.rs  SpriteEffect
  y_sort.rs          YSort
  tween.rs           Tween, TweenSequence, TweenLoop, Ease
  property_tween.rs  PropertyTween<T>
  lerp.rs            Lerp
  knockback.rs       Knockback (+ world-level Knockback::apply)
  squash_stretch.rs  SquashStretch, SquashAxis (+ SquashStretch::apply)
src/core/systems/    the engine systems over these components
src/core/schedule.rs Phase, FixedSet, UpdateSet, PostUpdateSet, EngineSchedules, SceneSchedules
src/core/messages/   engine message types, register_message, SceneMessages
```

`ecs/mod.rs` re-exports the public items, and the prelude glob-imports `crate::core::ecs::*`: **anything `pub` in `ecs/mod.rs` (including `pub use kernel::*`) is in every game's prelude.** Keep crate-internal items `pub(crate)`.

## The Kernel Rule

- `bevy_ecs` is named only in `Cargo.toml`, `ecs/kernel.rs` and the hidden re-exports in `src/lib.rs` (crate root and prelude).
- The derives (`Component`, `Resource`, `Message`, `Event`, `SystemSet`, `ScheduleLabel`, `Bundle`) expand to paths rooted at `bevy_ecs`. A game's `use ryo2d::prelude::*` brings that name into scope through the hidden prelude re-export; that is why the derives work downstream with no extra dependency. Do not remove it.
- Engine code imports from `crate::core::ecs` (or `crate::core::ecs::kernel` for crate-internal items such as `HookContext`, `DeferredWorld`, `SystemIdMarker`).
- Need another kernel type? Add one line to `kernel.rs`: `pub use` if games need it (it lands in the prelude), `pub(crate) use` otherwise.

## Components

```rust
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Health {
  pub current: f32,
  pub max: f32,
}
```

1. Define it beside the code that owns it (an `ecs/` file for general components, the feature module otherwise).
2. Export it from that module; export it from the prelude in `src/lib.rs` if games use it (ECS items arrive there through the glob).
3. Write rustdoc on the type: rustdoc is the API reference.

Patterns the engine uses:

| Need | How | Example |
|------|-----|---------|
| Always present with another component | `#[require(Other)]` | `Transform` requires `GlobalTransform` |
| React on insert/remove | `#[component(on_insert = hook)]`, hook `fn(DeferredWorld, HookContext)` | `place_on_insert` in `transform.rs` |
| Hidden from queries | the kernel's `Disabled` | pooling |
| Written by networking | `Component<Mutability = Mutable>` (the default derive) | `Replicated` |
| Resource with `ResMut` / `resource_mut` | `Resource<Mutability = Mutable>` (the default derive) | `Time`, `Input` |

A resource must be `Send + Sync`. The scene stack and audio are not, so they live in the non-send `SceneHost` (`engine/mod.rs`).

## Transform Internals

- **Insert hook** (`place_on_insert`): inserting a `Transform` sets `GlobalTransform` at once, from the parent's current global for a child. Spawning in `update` therefore places the entity for the rest of the step. Edits through `get_mut::<Transform>` do not fire it; they wait for propagation.
- **Propagation** (`propagate_transforms`): one system, run in `FixedSet::Transforms` and `PostUpdateSet::Transforms`. Roots are `Transform` without `ChildOf`; a subtree is recomputed when the root's `Transform` changed or, during the walk, an ancestor's `Transform`/`ChildOf` changed. Children of a parent with no `Transform` (a grouping entity) start from the identity; they are recomputed like any node, plus when the grouping entity has just lost its `Transform`. Writes go through `set_if_neq`, so an unmoved entity's `GlobalTransform` is never marked changed.
- **Previous transforms**: propagation also inserts `PreviousGlobalTransform` (with `skip: true`) on every entity that is `Interpolated` or under one, and removes it elsewhere, through `Commands` (applied at the end of the phase).
- **Snapshot**: `snapshot_previous_transforms` runs first in each fixed step and copies `GlobalTransform` into `PreviousGlobalTransform`, clearing `skip`.
- **Render**: extraction calls `render_global(global, previous, time.fixed.overstep_fraction())`; gameplay never sees a blend.

Changing the transform model means changing all four places and their tests (`propagate.rs`, `transform.rs`, `interpolate.rs`, `tests/integration/transform.rs`).

## Change-Detection Discipline

Propagation and physics sync are change-detected, so a spurious write costs work and can wake a sleeping body.

- Assign a component only when the value changed, or use `set_if_neq`.
- Iterating `&mut T` does not mark anything; dereferencing `Mut<T>` mutably does.
- Physics' write-back is an ordinary change (propagation must see bodies move); `sync_to_physics` therefore compares values with the body before pushing.
- `bypass_change_detection()` hides a write from propagation as well: do not use it on `Transform`.

## Resources Are Entities

In `bevy_ecs` 0.20 a resource is a component on a hidden entity marked `IsResource`; observers and registered one-shot systems are entities too. Any sweep over "every entity" must exclude them:

- `despawn_game_entities` (`world_ext.rs`, behind `SceneCleanup::WORLD`) filters `(Allow<Disabled>, Without<IsResource>, Without<Observer>, Without<SystemIdMarker>)`.
- Counting entities (debug stats) skips archetypes holding those markers.
- Tests that count entities count their own marker component, not `world.entities()`.

## Disabled and Pooling

- `Disabled` is the kernel's entity-disabling component: every query skips it unless it includes `Allow<Disabled>` (or names `Disabled`). Systems need no filter of their own for pooled entities.
- Bookkeeping that must see disabled entities asks for them: physics' `(With<RigidBody>, Added<Disabled>, Allow<Disabled>)` unregisters a body as it is disabled.
- `Pool<B: Bundle>` spawns its batches disabled; `acquire` removes `Disabled`, `release`/`release_all` insert it. `acquire` does not reset components and skips the entity's interpolation for this step. Acquire, release and `is_active` are O(1) plus the entity's children (release disables descendants, acquire restores the ones it disabled); a pool forgets entities the world no longer has (a cleanup sweep) when it meets them, and refills.

## Engine Systems

Engine systems are plain functions over `Query`, `Res`, `ResMut`, `MessageWriter`, `Commands`, or exclusive `fn(&mut World)` when they need the whole world (the scene systems, message updates, tracker clearing).

```rust
/// Decay every active `Knockback` by one fixed step.
pub fn update_knockback(
  time: Res<Time>,
  mut query: Query<(Entity, &mut Knockback, Option<&mut Velocity>, Option<&RigidBody>, Option<&mut Transform>)>,
  mut started: MessageWriter<KnockbackStarted>,
  mut finished: MessageWriter<KnockbackFinished>,
) {
  let dt = time.fixed.timestep();
  // ...
}
```

- Fixed-step systems integrate with `time.fixed.timestep()`; per-frame systems that must run through pause use `time.real.delta()`.
- Register in `register_juice_systems` (`systems/mod.rs`), `register_physics_systems` (`systems/physics.rs`) or `register_engine_systems` (`engine/mod.rs`), with `.in_set(..)` and `.chain()`/`.before()`/`.after()`. Every schedule runs on the single-threaded executor, but declare real access so a parallel executor stays possible.
- Errors: return `Result<(), SystemError>`; the installed `FallbackErrorHandler` panics in debug and logs in release.
- Profile with `profile_scope!("name")` inside the system.

Test a system through a real `Schedule` with `systems::test_util`:

```rust
use crate::core::systems::test_util::{messages, run, world};

#[test]
fn knockback_moves_an_entity_without_velocity() {
  let mut world = world(10); // Time at 10 Hz plus the engine messages
  let entity = world
    .spawn((Transform::IDENTITY, Knockback::new(Vec2::X, 100.0)))
    .id();
  run(&mut world, update_knockback);
  assert!(world.get::<Transform>(entity).unwrap().translation.x > 0.0);
  assert_eq!(messages::<KnockbackStarted>(&world), [KnockbackStarted { entity }]);
}
```

`set_hz(world, hz)` changes the step, `clear_messages(world)` empties the buffers. Cross-module behaviour (engine plus schedules) goes in `tests/integration/` (`support.rs`: `juice_world`, `run_fixed`, `messages`); anything that needs a frame, `tests/e2e/` with `HeadlessEngine`.

## Messages

- A message type derives `Message` (and `Clone` for scene readers). `register_message::<T>(world)` inserts `Messages<T>` and adds an updater and a clearer (`MessageUpdaters`); registering twice is a no-op.
- The engine's types register in `register_engine_messages` at build; games use `EngineBuilder::add_message::<T>()`, or `ctx.write_message` registers lazily. A system with `MessageReader<T>` for an unregistered `T` fails validation, so register types systems read.
- `update_messages_system` (`First`) advances every buffer only when a fixed step has run since the last advance (`MessagesAdvancedAt` holds the step count), so fixed-step readers never miss a message; `clear_messages` backs `SceneCleanup::MESSAGES`.
- Scenes read through `SceneMessages` (`messages/scene.rs`): one `MessageCursor<T>` per type per scene instance.
- Add a message type only if engine code writes it; games define their own notifications.

## Queries Outside Systems

`World::query` returns a `QueryState`; iterate with the world:

```rust
let mut q = world.query::<(Entity, &GlobalTransform, &Renderable)>();
for (entity, global, renderable) in q.iter(world) {}

let ids: Vec<Entity> = world
  .query_filtered::<Entity, With<Collider>>()
  .iter(world)
  .collect();
```

Building a `QueryState` is not free: in per-frame engine code prefer a system with `Query` parameters, or keep the state.

## Checks

```bash
cargo test --lib -- core::ecs core::systems   # several filters go after --
cargo test --test integration
cargo clippy --lib --tests
```
