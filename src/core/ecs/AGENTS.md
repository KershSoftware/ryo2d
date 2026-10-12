# AGENTS.md - ECS Module

Ryo2D's ECS is `bevy_ecs` 0.20, presented as Ryo2D's own: `kernel.rs` re-exports a curated list of kernel types (`World`, `Entity`, `Query`, `Commands`, `Resource`, `Message`, `ChildOf`, `Disabled`, ...), and the rest of the module holds Ryo2D's components (`Transform` and friends, `Renderable`, `Velocity`, the juice components), `WorldExt`, `Pool`, propagation and interpolation. Skills: `ryo-ecs-internals` (contributors), `ryo-game-dev` (games), `ryo-animation` (tweens).

## The Kernel Name

`bevy_ecs` appears only in `Cargo.toml`, `kernel.rs` and the hidden re-exports in `src/lib.rs` (crate root and prelude). The derives (`#[derive(Component)]`, `Resource`, `Message`, ...) expand to paths rooted at `bevy_ecs`; a game's `use ryo2d::prelude::*` brings that name into scope, so derives work with no extra dependency. Everything else imports from `crate::core::ecs` (or `ecs::kernel` for crate-internal items such as `HookContext`); a kernel type a module needs is added to `kernel.rs`, `pub(crate)` unless games need it.

## Phases

`Engine::frame` (`engine/frame.rs`) runs, every frame:

| Step | Cadence | Engine work |
|------|---------|-------------|
| `First` | once | advance the clocks, publish `Input`, advance every message buffer; then the engine's debug and console keys consume their presses |
| `PreUpdate` | once | async loads and hot reload (`AssetReloaded` / `AssetLoadFailed`); then the console |
| UI pass | once | `Scene::ui` for visible scenes bottom-up, debug overlays, console; consumption applied to `Input`; a returned `SceneAction` applies at once, and a scene change redraws the UI once so the new scene shows this frame |
| `FixedUpdate` | 0..N | one run per fixed step; the sets below, chained |
| `Update` | once | `UpdateSet::Game`: scene per-frame systems |
| `PostUpdate` | once | `PostUpdateSet::Transforms` (propagation), `Camera` (shake, real time), `Extract` (timed effects, real time) |
| render | once | `present_frame` (`engine/extract.rs`): queries the world into the draw list, runs the render pipeline (not a schedule) |
| `Last` | once | physics registration bookkeeping, `clear_trackers`, `check_change_ticks` every 1024 frames |

`FixedSet`, in order:

| Set | Systems |
|-----|---------|
| `Snapshot` | `snapshot_previous_transforms` |
| `Juice` | animation, tweens, tween sequences, property tweens, knockback, squash/stretch, skeletons (`register_juice_systems`) |
| `Scene` | transitions, `Scene::update` for the active scene and those below `updates_below` scenes, then the action |
| `Game` | the scenes' own `FixedUpdate` schedules |
| `Physics` | register, unregister, sync to Rapier, step, sync back, collision messages (`register_physics_systems`; skipped while `PhysicsPaused`) |
| `Transforms` | `propagate_transforms` |
| `Trails` | `record_trail_points` |

A quit from the UI pass or a step ends the fixed loop; no step runs after it. Systems are registered in `register_engine_systems` (`engine/mod.rs`); every schedule uses the single-threaded executor. Systems may return `Result<(), SystemError>`: the `FallbackErrorHandler` resource panics in debug builds and logs in release.

Game systems: `EngineBuilder::add_systems(phase, systems)` for the whole run (any phase; in `FixedUpdate`/`Update` a call whose systems are not ordered against anything outside it runs in its `Game` set; one that names a set or another call's system (`.in_set`, `.before`, `.after`) keeps exactly that order), or `ctx.add_systems(Phase::FixedUpdate | Phase::Update, systems)` for a schedule the scene owns, run inside `FixedSet::Game` / `UpdateSet::Game` while the scene is active (or below an `updates_below` scene) and dropped with it. Other phases return `SceneScheduleError::UnsupportedPhase`.

## The Resource-Scope Rule

`SceneContext::scoped_parts` (`scene/context.rs`) takes `Time`, `Input`, `Camera` and `AssetManager` out of the world with nested `resource_scope`s for the length of a scene callback and hands them over as `ctx.time`, `ctx.input`, `ctx.camera`, `ctx.assets`; `ctx.audio` is engine-owned (the non-send `SceneHost`). So:

- Inside a callback those four are not in `ctx.world`: `ctx.world.resource::<Time>()` panics, and a system run on `ctx.world` that takes `Res<Time>` fails. Use the fields.
- Systems run outside callbacks and always see them as resources.
- Everything else stays in the world; the context's helpers (`effects()`, `lighting()`, `render_pipeline()`, `tilemaps()`, `window()`, `cursor()`, `console()`, `resource*`) return `Mut<..>` into it.
- A panic in a callback aborts the frame; the scoped resources are not put back.

## Transform and Hierarchy

- `Transform` is local (`translation: Vec2`, `rotation` in radians, `scale: Vec2`); `GlobalTransform` is world space, an `Affine2` written only by the engine. `Transform` requires `GlobalTransform`.
- **Insert hook:** inserting a `Transform` (spawn or `insert`) sets `GlobalTransform` at once: the parent's current global times the local transform for a child, the local transform otherwise. So an entity spawned in `update` is where it should be for the rest of the step (a burst emitter fires from its position, a navigation grid built after spawning walls sees them). Edits through `get_mut::<Transform>` wait for propagation.
- **Propagation** (`propagate_transforms`) runs at the end of every fixed step (`FixedSet::Transforms`, after physics) and in `PostUpdate` (per-frame edits). It walks from roots (`Transform` without `ChildOf`), recomputes a subtree when the root's `Transform` changed or an ancestor's `Transform` or `ChildOf` changed during the walk, and writes through `set_if_neq`, so an unmoved entity's `GlobalTransform` is not marked changed. Both runs are change-detected; the second is nearly free when nothing moved per frame. Children of a parent with no `Transform` start from the identity.
- **Hierarchy** is the kernel's `ChildOf(parent)` / `Children`. Spawn a child with `world.spawn_child(parent, bundle)` (`WorldExt`, also `ctx.spawn_child`) or `(bundle, ChildOf(parent))`. Reparent by inserting `ChildOf`; remove it to detach (the local transform becomes the world one), or call `detach_keep_world(entity)` to keep the entity where it is. Despawning a parent despawns its descendants. Nothing checks for cycles longer than an entity parenting itself: never make an entity its own ancestor.
- Physics bodies must be root entities (see `physics/AGENTS.md`).

## Interpolation

- Opt in with the `Interpolated` marker; it covers the entity's descendants.
- Propagation keeps `PreviousGlobalTransform` on every entity that is `Interpolated` or below one and removes it elsewhere. A new one starts with `skip: true`, so its first frame draws at the current transform.
- `FixedSet::Snapshot` (first in each step) copies `GlobalTransform` into `PreviousGlobalTransform` and clears `skip`.
- At render, `render_global(global, previous, time.fixed.overstep_fraction())` blends previous to current (linear translation and scale, shortest-arc rotation); with `skip` set or no previous, it is the current transform. Gameplay never reads a blend: `GlobalTransform` is always the true state.
- After a teleport, `skip_interpolation(entity)` (`WorldExt`, `ctx.skip_interpolation`) draws the new position at once; interpolation resumes from the next step.
- Everything an interpolated entity draws (single sprites, tiled runs, composite tiles, lights, occluders, lines) is placed at `render_global`, so it stays together. Trail points are recorded world positions and are not blended.

## Disabled and Pooling

- `Disabled` is the kernel's entity-disabling marker: every query skips a disabled entity unless it asks with `Allow<Disabled>` (or names `Disabled` in its terms). Engine systems, physics (unregisters, re-registers on enable), rendering and particles skip disabled entities with no filter of their own.
- `Pool<B>` (`pooling.rs`) spawns its batch disabled; `acquire` removes `Disabled`, `release` / `release_all` insert it. `acquire` does not reset components (set every field you rely on); it sets the interpolation skip so the entity does not slide from where it was released.
- `acquire`, `release` and `is_active` are O(1): the active list is a queue of (acquisition number, entity) plus an entity-to-number map, and a released entry stays in the queue as stale until skipped or compacted. A pool drops entities the world no longer has (a `SceneCleanup::WORLD` sweep) lazily; `RecycleOldest` pops stale and dead entries off the front, and the other policies purge fully when nothing is free. `Disabled` is not hierarchical, so `release` also disables the entity's descendants (marking the ones it disabled with a private `DisabledWithPool`) and `acquire` restores exactly those.

## Resources Are Entities

In `bevy_ecs` 0.20 a resource is a component on a hidden entity marked `IsResource`; observers (`Observer`) and registered one-shot systems are entities too, and plain entity queries see them. Any "every entity" sweep must filter them out, or it despawns a resource and panics in the resource hook:

```rust
world.query_filtered::<Entity, (Allow<Disabled>, Without<IsResource>)>()
```

`despawn_game_entities` (`world_ext.rs`, used by `SceneCleanup::WORLD`) also excludes `Observer` and `SystemIdMarker`; the entity count in the debug stats does the same per archetype.

## Messages

- A message type derives `Message` and is registered with `register_message::<T>` (`core/messages`): at engine build for the engine's types, `EngineBuilder::add_message::<T>()` for a game's, or lazily on the first `ctx.write_message`.
- Buffers are double-buffered. Buffers advance in `First`, but only once a fixed step has run since the last advance: a message stays readable until two fixed steps have run after it was written, so every reader, per-frame or fixed-step, sees it at any frame rate. While no fixed step runs (the game clock paused) nothing is dropped. Each reader sees a message once.
- Systems use `MessageWriter<T>` / `MessageReader<T>`. Scenes use `ctx.write_message(m)` and `ctx.messages::<T>()`, which reads through a `MessageCursor` kept per scene instance (`SceneMessages`), so every scene sees each message once.
- `SceneCleanup::MESSAGES` empties every buffer. Modules define no message type they do not write; a game that wants a notification defines its own.

## PropertyTween Auto-Driving

- `update_property_tweens` (`systems/tween.rs`) drives only `PropertyTween<f32>`, `<Vec2>` and `<Color>`.
- Other `Lerp` types (including `(f32, f32)` and `Rect`): call `tween.update(dt)` yourself, or add a query there.
