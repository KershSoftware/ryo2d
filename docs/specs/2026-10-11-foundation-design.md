# Foundation: ECS, Clocks, Transform - Design

## Overview

Ryo2D's foundation is rebuilt on `bevy_ecs` 0.20 (exposed as Ryo2D's own ECS), a three-clock time model with one frame loop, and a 2D `Transform`/`GlobalTransform` pair over the built-in entity hierarchy. This is sub-project 1 of the professional-grade overhaul decided on 2026-10-11 (see `docs/research/2026-10-11-engine-audit.md`). Later sub-projects, each with its own spec: 2. renderer core, 3. physics, 4. UI and text, 5. assets and audio, then the feature modules. Networking is untouched until its own rethink.

**Why:** the audit found that the time model is wrong in six verified places (Rapier dt never set, physics ignoring time scale, interpolation snapshot once per frame instead of per step, "real dt" that is the fixed dt, frame-count shader time, events cleared per fixed step), that the hierarchy has a rotation-unit mismatch, orphaning on despawn and incomplete render-cache invalidation, and that every engine system walks the world with a `Vec` of entities plus per-entity lookups. The engine has home-grown versions of scheduling, resources, events, hierarchy and change detection, and each has a bug. `bevy_ecs` provides all of them.

**Owner decisions (2026-10-11):** adopt `bevy_ecs` as a dependency but present it as Ryo2D's ECS (game code never names `bevy_ecs`); hybrid programming model (the `Scene` trait stays, scenes and the builder can register systems into named phases); expose the ECS types directly rather than through a wrapper; keep procgen and inventory as optional crates under `crates/`, park the raycaster until the renderer sub-project; breaking demos, games and APIs is acceptable.

**Out of scope:** camera matrix, resolution model, HDR, unified batch (renderer); collider shapes, static bodies, collision groups, queries, joints, parented bodies (physics); GPU UI, real text, widget identity (UI); typed asset handles and loaders, audio buses (assets and audio); engine-driven particles; tilemap rendering; multi-threaded execution (designed for, not enabled); WebGPU.

## Workspace layout

```
ryo2d/                  core crate, sources in src/ (unchanged location)
  src/core/ecs/         Ryo2D's ECS: re-exports of the bevy_ecs types we endorse, our components
                        (Transform, GlobalTransform, PreviousGlobalTransform, Interpolated),
                        extension traits (spawn_child, detach_keep_world), pooling
crates/
  ryo2d-derive/         moved from the repository root
  ryo2d-procgen/        moved from src/core/procgen; depends on ryo2d
  ryo2d-inventory/      moved from src/core/inventory; its messages, errors and world helpers move with it
examples/               demo-kit, demos/*, games/* as now; the procgen and inventory demos depend on their crates
```

Rules:

- Dependencies point only toward the core. Nothing in `src/` references a crate in `crates/`. Inventory and network message and error types move to their owners (`core::events::builtin` and `error.rs` lose them).
- The prelude exports the core only. A game that wants inventory adds `ryo2d-inventory` and imports its prelude. The `ryo2d init` template lists optional crates commented out. Each crate keeps its skill.
- `ryo2d::ecs` re-exports a curated list, not a glob of everything `bevy_ecs` has. The crate is additionally re-exported from the prelude under its own name solely so derive macros resolve (bevy's derives expand to paths rooted at `bevy_ecs`); nobody writes that name.
- The raycaster is parked, not deleted: `src/core/raycast/` and `examples/demos/raycasting` move to `attic/raycast/` and `attic/raycasting-demo/`, outside the workspace and not compiled, with an `attic/README.md` saying why and that it returns as `crates/ryo2d-raycast` on the new render pipeline. Its render pass, `SceneContext` methods and `engine.rs` wiring are removed from the core.
- Deleted now: the legacy collision system (`physics/spatial.rs`, `check_collision`, `AABB`/`Circle`/`Collision`/`CollisionPair`/`CollisionResult`/`PhysicsWorld`/`SpatialHash` exports), every event type that nothing emits (procgen, navigation, state machine, asset and inventory events that are never written; the owners may reintroduce ones they actually emit), the unused GPU overlay code in `config/debug.rs`, the `WorldState`/`EntitySnapshot` save containers, and the engine's `Resources` map.

## Public API: `ryo2d::ecs` (in the prelude)

Re-exported from `bevy_ecs` under these names:

- Core: `World`, `Entity`, `Component`, `Bundle`, `Resource`, `Commands`, `EntityCommands`, `EntityWorldMut`, `Query`, `Res`, `ResMut`, `With`, `Without`, `Or`, `Has`, `Allow`, `Changed`, `Added`, `Ref`, `Mut`, `DetectChanges`, `RemovedComponents`, `Disabled`.
- Scheduling: `Schedule`, `ScheduleLabel`, `SystemSet`, `IntoScheduleConfigs` (so `.chain()`, `.before()`, `.after()`, `.in_set()`, `.run_if()` work), `IntoSystem`, `System`.
- Messages and events: `Message`, `Messages`, `MessageReader`, `MessageWriter`, `MessageCursor`, `Event`, `EntityEvent`, `On`.
- Hierarchy: `ChildOf`, `Children`.
- Errors: `BevyError` re-exported as `SystemError`, `FallbackErrorHandler`.

Derives: `#[derive(Component)]`, `#[derive(Resource)]`, `#[derive(Message)]`, `#[derive(Event)]`, `#[derive(SystemSet)]`, `#[derive(ScheduleLabel)]` are the bevy derives, available through the prelude.

Ryo2D's own types in `ryo2d::ecs`:

- `Transform`, `GlobalTransform`, `PreviousGlobalTransform`, `Interpolated` (below).
- `trait WorldExt` on `World`: `spawn_child(parent, bundle) -> Entity`, `detach_keep_world(entity)`, `skip_interpolation(entity)`.
- `Pool` (object pooling, rebuilt on `Disabled`).
- Phase labels and sets (below).

`Anchor` (the existing 9-point enum) moves to `ryo2d::ecs` beside `Transform` since the sprite side, physics and the UI all use it; `ryo2d::ui` re-exports it.

## Phases and schedules

The engine owns these schedules and runs them in this order every frame:

| Phase (`ScheduleLabel`) | Cadence | Engine systems, in order |
|---|---|---|
| `First` | once per frame | advance clocks; rebuild `Input` from queued window and gamepad events; `Messages` update for every registered message type |
| `PreUpdate` | once per frame | asset async pumping and hot reload |
| scene UI pass | once per frame | `Scene::ui` for visible scenes, bottom-up; the returned `SceneAction` is applied immediately |
| `FixedUpdate` | zero or more per frame | sets `FixedSet::Snapshot`, `FixedSet::Juice`, `FixedSet::Scene`, `FixedSet::Game`, `FixedSet::Physics`, `FixedSet::Transforms`, `FixedSet::Trails`, chained in that order |
| `Update` | once per frame | `UpdateSet::Game` (scene-registered per-frame systems) |
| `PostUpdate` | once per frame | `PostUpdateSet::Transforms` (change-detected propagation), `PostUpdateSet::Camera` (shake, real dt), `PostUpdateSet::Extract` (anything that prepares render data) |
| render | once per frame | `present_frame` reads the world (not a schedule in this sub-project) |
| `Last` | once per frame | `world.clear_trackers()`; `check_change_ticks()` every 1024 frames |

Set contents:

- `FixedSet::Snapshot`: copy `GlobalTransform` into `PreviousGlobalTransform` for interpolated entities (honouring the skip flag).
- `FixedSet::Juice`: animation, tweens, knockback, squash/stretch, skeleton (the systems that exist today, migrated).
- `FixedSet::Scene`: an exclusive system that builds `SceneContext`, runs `SceneManager::update` (transitions, `Scene::update` for the active scene and any below with `updates_below`), and applies the action.
- `FixedSet::Game`: an exclusive system that runs the fixed schedules owned by the active scene and by scenes below with `updates_below`.
- `FixedSet::Physics`: registration (`Added<RigidBody>`, `RemovedComponents<RigidBody>`), sync to Rapier, step, sync back. Run condition: `!PhysicsPaused`, a resource the scene manager maintains from `is_physics_paused` and the stack shape.
- `FixedSet::Transforms`: change-detected propagation (below).
- `FixedSet::Trails`: trail point recording.
- `UpdateSet::Game`: an exclusive system that runs the per-frame schedules owned by the active scene and `updates_below` scenes.

Executor: `SingleThreadedExecutor` on every schedule. Systems declare their real access so switching to `MultiThreadedExecutor` later is configuration.

Engine-level registration: `EngineBuilder::add_systems(label, systems)` adds to an engine schedule for the whole run. Scene-level registration: `SceneContext::add_systems(label, systems)` adds to a `Schedule` owned by the calling scene (one per phase, created lazily), run by the `Game` sets above and dropped with the scene. Only `FixedUpdate` and `Update` accept scene systems; other labels return an error.

Error handling: a `FallbackErrorHandler` is installed that logs the error with its context and continues in release builds and panics in debug builds. Systems may return `Result<(), SystemError>`.

## Time

One `Time` resource (also `ctx.time` in scene callbacks):

```rust
pub struct Time { pub real: RealClock, pub game: GameClock, pub fixed: FixedClock, frame_count: u64 }
```

- `RealClock`: `delta() -> f32`, `delta_duration()`, `elapsed() -> f32`. Wall clock since the previous frame, uncapped.
- `GameClock`: `delta()`, `elapsed()`, `speed() -> f32`, `set_speed(f32)` (clamped to >= 0), `pause()`, `resume()`, `is_paused()`, `hitstop(secs)`, `is_hitstop()`, `max_delta` (default 250 ms). Delta = min(real delta, max_delta) x speed, or zero while paused or in hitstop. Hitstop counts down in real time.
- `FixedClock`: `timestep() -> f32` (default 1/60, set by `EngineBuilder::fixed_hz`, replacing `physics_hz`), `accumulator()`, `overstep_fraction() -> f32` (the render alpha), `step_count() -> u64`. Fed by the game clock's delta each frame. While `accumulator >= timestep`, `FixedUpdate` runs and a timestep is subtracted.
- `frame_count()`: frames since start.

Inside `FixedUpdate`, `time.delta()` (a convenience on `Time`) returns the fixed timestep; outside it returns the game clock's delta. The loop flips a private flag on `Time` around each fixed step to make that choice. `Scene::update`'s `dt` argument is the fixed timestep. The fixed delta is constant: slow-mo runs fewer steps, hitstop runs none. `scaled_fixed_dt`, `TimeControl`, `TimeInfo` and `sync_from_time_control` are removed.

Consumers: camera shake, console, UI animation and the hitstop countdown read `real`. Tweens, animation, knockback and squash run in `FixedUpdate` with the fixed delta. Rapier's `IntegrationParameters.dt` is set from `fixed.timestep()` when the engine builds and whenever the timestep changes. Shader globals carry `real_elapsed` and `game_elapsed` seconds; built-in post-process effects use real time, sprite effects use game time.

Headless: `HeadlessEngine::step()` advances the game clock by exactly one timestep, so each step runs one `FixedUpdate` and one render.

## The frame loop

One function, `Engine::frame()`, runs on every `RedrawRequested` and on the unfocused tick. The unfocused path only changes how often the frame is requested (`target_background_fps`, `background_rendering`); `tick_frame` is deleted. The frame:

1. Record the frame start instant on `Time`.
2. `First` (whose first system advances the clocks from that instant), then `PreUpdate`.
3. Scene UI pass.
4. `while fixed.accumulator >= timestep { FixedUpdate; consume }` with the first-step input rule below.
5. `Update`, `PostUpdate`.
6. Render: build the draw list from the world, run the render pipeline, present. Surface `Outdated` and `Timeout` skip the frame; `Lost` reconfigures; only `OutOfMemory`/device loss is fatal.
7. `Last`.
8. Frame pacing: with vsync on, the surface is `Fifo` and nothing else waits; with vsync off, `Immediate` (or `Mailbox` if available) plus the sleep-and-spin limiter capped at `target_fps`.

The headless loop calls the same `frame()` with the clock advanced by one timestep.

## Input

`Input` is a resource rebuilt in `First` from the window and gamepad events queued since the previous frame:

- `pressed(key)`, `just_pressed(key)`, `just_released(key)` and the mouse, scroll, text, IME and gamepad equivalents keep today's names and semantics, except that `just_*` is true for exactly one frame.
- Inside the fixed loop, `just_*` is visible in the first fixed step of a frame and suppressed in later steps of the same frame (the loop sets a flag on `Input` between steps). A key press is therefore handled once in fixed logic and once per frame in variable logic.
- `consume(key)`, `consume_mouse(button)`, `consume_text()`: a consumed input reads as not pressed for the rest of the frame. The scene UI pass runs before the fixed loop and consumes what it uses, so gameplay never sees a click the UI took. The UI's `click_consumed`/`escape_consumed` flags, `suppress_oneshots`, `post_render_cleanup` and `SceneManager::skip_next_update` are removed.
- `RYO_INPUT` scripting feeds the same event queue.

## Messages

- Engine messages (`CollisionStarted`, `CollisionStopped`, `TweenStarted`, `TweenCompleted`, `KnockbackStarted`, `KnockbackFinished`, `SquashStretchStarted`, `SquashStretchFinished`, `AssetReloaded`, `AssetLoadFailed`) are `#[derive(Message)]` types registered at engine build. Game message types register through `EngineBuilder::add_message::<T>()` or lazily on first `ctx.write_message`.
- Systems use `MessageWriter<T>` and `MessageReader<T>`.
- Scenes use `ctx.messages::<T>()`, backed by a `MessageCursor<T>` stored per scene (keyed by `TypeId`), so each scene sees each message once, and `ctx.write_message(value)`.
- Buffers advance once per frame in `First`, so a message written anywhere in frame N is readable throughout frame N+1. `SceneCleanup::EVENTS` is renamed `SceneCleanup::MESSAGES` and clears all message buffers.

## Transform and hierarchy

```rust
#[derive(Component, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[require(GlobalTransform)]
pub struct Transform { pub translation: Vec2, pub rotation: f32, pub scale: Vec2 }

#[derive(Component, Clone, Copy, PartialEq, Default)]
pub struct GlobalTransform(Affine2);

#[derive(Component, Clone, Copy)]
pub struct PreviousGlobalTransform { pub affine: Affine2, pub skip: bool }

#[derive(Component, Clone, Copy, Default)]
pub struct Interpolated;
```

- `Transform`: `IDENTITY`, `from_xy(x, y)`, `from_translation(Vec2)`, `with_rotation(radians)`, `with_rotation_degrees(deg)`, `with_scale(Vec2)`, `with_uniform_scale(f32)`, `rotation_degrees()`, `set_rotation_degrees()`, `to_affine()`. Rotation is radians everywhere. `z`, pivot and offset are gone: draw order is `Renderable.z_order`; the sprite's anchor is `Renderable.anchor: Anchor` with default `Anchor::Center`; a pixel nudge is a child entity.
- `GlobalTransform`: `translation()`, `rotation()`, `scale()`, `affine()`, `transform_point(Vec2)`, `to_transform()`. Written only by the engine; `Transform` requires it so every placed entity has one.
- Hierarchy: the built-in `ChildOf(parent)` and `Children`. `spawn_child(parent, bundle)` is `world.spawn((bundle, ChildOf(parent))).id()`. Reparent by inserting `ChildOf`; detach by removing it. `detach_keep_world(entity)` removes `ChildOf` and sets `Transform` from the current `GlobalTransform` so the entity stays put. Despawn cascades to descendants. The relationship machinery rejects cycles and maintains `Children`.
- Propagation (`FixedSet::Transforms` and `PostUpdateSet::Transforms`): walk from roots (`Transform` without `ChildOf`); a subtree is recomputed when the root's `Transform` changed or, during the walk, any ancestor's `Transform` or `ChildOf` changed; `GlobalTransform` is written through `set_if_neq` so unchanged entities are not marked changed. The walk also ensures `PreviousGlobalTransform` exists on every entity that is `Interpolated` or has an `Interpolated` ancestor, and removes it where that stops being true.
- Interpolation: `FixedSet::Snapshot` copies `GlobalTransform` into `PreviousGlobalTransform` and clears `skip`. The renderer draws an entity that has `PreviousGlobalTransform` at `lerp(previous, current, fixed.overstep_fraction())` with shortest-arc rotation, except while `skip` is set, when it draws at `GlobalTransform`; entities without the component draw at `GlobalTransform`. So a teleport in step N renders at the new position immediately, and interpolation resumes from step N+1. `Renderable::interpolated()` is removed; games add `Interpolated`. `skip_interpolation(entity)` sets the flag.
- Y-sort reads `GlobalTransform.translation().y` plus the renderable's anchor-derived foot offset.
- Physics reads and writes `Transform` for root entities (an entity with `ChildOf` and `RigidBody` is logged once and not simulated). Colliders are centred on the entity origin plus `Collider.offset`; `centered_in` and the pivot-based auto-centring are removed.

## Scene trait and `SceneContext`

```rust
pub trait Scene: 'static {
  fn on_enter(&mut self, ctx: &mut SceneContext) {}
  fn on_exit(&mut self, ctx: &mut SceneContext) {}
  fn on_pause(&mut self, ctx: &mut SceneContext) {}
  fn on_resume(&mut self, ctx: &mut SceneContext, returned: Option<Box<dyn Any>>) {}
  fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction { SceneAction::Continue }
  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) -> SceneAction { SceneAction::Continue }
  fn render(&mut self, gpu: &mut GpuRenderContext) {}
  fn is_transparent(&self) -> bool { false }
  fn updates_below(&self) -> bool { false }
  fn is_physics_paused(&self) -> bool { false }
  fn cleanup_on_exit(&self) -> SceneCleanup { SceneCleanup::DEFAULT }
  fn name(&self) -> &str { type name }
}
```

Changes from today: `update` has a default; `render_ui` becomes `ui` and receives the context; the never-called `render(ctx, alpha)` is removed and `render_gpu` becomes `render`. The stack, `SceneAction` (including the `*With(Transition)` variants and `PopReturn`), transitions and `SceneCleanup` keep their semantics over the single shared world. `SceneCleanup::TIME` resets the game clock's speed, pause and hitstop. `SceneCleanup::RAYCAST` is removed.

```rust
pub struct SceneContext<'a> {
  pub world:  &'a mut World,
  pub time:   &'a mut Time,
  pub input:  &'a mut Input,
  pub camera: &'a mut Camera,
  pub assets: &'a mut Assets,
  pub audio:  &'a mut Audio,
  // private: this scene's schedules, message cursors, scope id for console commands
}
```

The engine builds the context in one place (`SceneContext::scoped(world, audio, scene_state, f)`), taking `Time`, `Input`, `Camera` and `Assets` out of the world with nested `resource_scope`s for the duration of the callback and putting them back after. Systems therefore always see them as resources; scene callbacks see them as independent fields.

Helpers on `SceneContext` (each a thin call into the world):

- `add_systems(label, systems)`: scene-owned schedules.
- `messages::<T>()`, `write_message(value)`.
- `resource::<T>()`, `resource_mut::<T>()`, `insert_resource(value)`: the game's `#[derive(Resource)]` types; replaces `Resources` and `EngineBuilder::resource`, which becomes `EngineBuilder::insert_resource`.
- `effects()` (`EffectsManager`), `lighting()` (`LightingSettings`: ambient, atmosphere, volumetric; the renderer itself is engine-internal), `textures()` (`TextureContext` over the `Gpu` resource and the texture manager), `tilemaps()`.
- `window()` (`WindowRequest`: resolution, display mode, UI scale), `cursor()` (`CursorRequest`), `screen_size()`, `scale_factor()`, `display_mode()`.
- `console()`: `declare`, `register` (scoped to the scene), `remove_builtin`.
- `render_pipeline()`.
- `skip_interpolation(entity)`, `spawn_child(parent, bundle)` (forward to `WorldExt`).

Removed from the context: `knockback*`, `squash*` (now `Knockback::apply(world, entity, dir, force)`, `Knockback::apply_with_decay`, `SquashStretch::apply(world, entity, intensity)`, `SquashStretch::apply_on_axis`, `SquashStretch::apply_with_decay`), `hitstop`/`set_time_scale`/`time_scale`/`is_hitstop_active` (now `ctx.time.game.*`), `enable_raycast`/`disable_raycast`/`raycast_renderer`, `device`/`queue`/`texture_manager` fields, `events`, `time_control`, `window_request`/`cursor_request` fields, `command_registry`/`scope_id` fields.

Resources the engine inserts (all `Send + Sync`): `Time`, `Input`, `Camera`, `Assets`, `Messages<T>` per type, `EffectsManager`, `LightingSettings`, `TilemapManager`, `WindowRequest`, `CursorRequest`, `RenderPipeline`, `Gpu { device: Arc<Device>, queue: Arc<Queue> }`, `TextureManager`, `PhysicsPaused`, `PhysicsConfig`, `DebugConfig`, `ConsoleCommands` (the registry), `FallbackErrorHandler`. Engine-owned, not resources: the Kira `Audio` manager, gilrs, the window, the renderers (sprite, lighting, trail, compositor, post-process pipeline), the Rapier world (reached through the `Physics` set's exclusive system; a `PhysicsWorld` resource is the physics sub-project's call), dev windows, capture state.

Pooling: `Pool<B>` keeps its API; `Inactive`/`Active` become the built-in `Disabled` (pooled entities are disabled, so every query skips them without a filter) and acquire/release insert or remove `Disabled`. The "skip `Inactive` in every system" convention is deleted; `render` code that wants disabled entities uses `Allow<Disabled>`.

## Other subsystems in this sub-project

Mechanical migration for all: queries instead of `entities_with` plus per-entity `get`, `Disabled` instead of `Inactive`, resources instead of `Engine` fields, `GlobalTransform` instead of `Transform`-or-`WorldTransform` for anything world-space. Specific changes:

- **Physics:** dt from the fixed clock; step in `FixedSet::Physics`; registration by `Added<RigidBody>` and `RemovedComponents<RigidBody>` (which covers despawn), plus unregister on `Added<Disabled>` and re-register on `RemovedComponents<Disabled>` for pooled bodies (those queries carry `Allow<Disabled>` so the default filter does not hide them); sync to Rapier only for `Changed<Transform>` / `Changed<Velocity>` since the last sync, with physics' own write-back done through `bypass_change_detection()` so a body is woken only by a game write; `RigidBody.mass` removed; `PIXELS_PER_METER` stays. Shapes, static bodies, groups, queries and joints are not touched.
- **Renderer:** `present_frame` collects entities with one query over `(Entity, &GlobalTransform, Option<&PreviousGlobalTransform>, &Renderable, Option<&Material>, Option<&YSort>)`, computes the draw order per frame and pushes into the existing `DrawList`; `renderable_cache`, `y_sort_entities`, `render_cache_dirty`, `World::take_entities_changed` and `WorldTransform` are deleted. `Camera` is a resource. Surface-error handling, the vsync mapping and clock-based shader globals as described above. Lights, occluders, trails and lines are collected by queries without cloning polygon data into intermediate `Vec`s where a borrow suffices.
- **UI:** unchanged internally; driven from the scene UI pass with the new context; consumption through `Input::consume*`; `UiContext` reads `time.real` for its animations.
- **Assets:** `Assets` resource (the current `AssetManager`); the engine pumps `process_async_loads` in `PreUpdate`.
- **Audio, console, capture, dev windows, tilemap, navigation, skeleton, trails, lighting, particles, state machine, networking:** migration only. Particles remain scene-driven. Networking's `Replicated` impl for `Transform` follows the new fields.
- **Events module:** becomes the message type definitions and registration; `Events` struct, `read`, `emit`, `clear` are deleted.
- **Config:** `physics_hz` is renamed `fixed_hz` in `Ryo.toml`, the embed keys and the builder; the `DebugConfig` and overlay rendering split is left for later except for deleting the dead GPU overlay code.

## Migration

The two ECS crates cannot share a world, so the work happens on a branch (`foundation`) in a worktree and merges once complete; `main` stays green.

1. Spike: a throwaway crate outside the workspace depends only on `ryo2d` and compiles `#[derive(Component)]`, `#[derive(Resource)]`, `#[derive(Message)]` and a system with `Query`/`Res` using only `use ryo2d::prelude::*`. If the derives do not resolve through the prelude re-export, `ryo2d-derive` gains thin `Component`/`Resource`/`Message`/`Event` derives that emit the bevy derive with the path rewritten, and the plan is updated before step 2.
2. Core: `ecs` module, `Time`, `Input` rebuild, schedules and phases, `Transform`/`GlobalTransform`/propagation/interpolation, messages, the new `SceneContext` and `Scene` trait, `Engine::frame`, the physics and renderer changes above, the engine's own systems migrated. `cargo check` and `cargo test --lib` pass.
3. `tests/integration` and `tests/e2e` migrated and passing.
4. `examples/demo-kit`, then demos and games, migrated in parallel batches; each crate builds, and `./dev demos:tour` captures every page.
5. `crates/` created: `ryo2d-derive` moved, `ryo2d-procgen` and `ryo2d-inventory` extracted with their demos pointed at them; raycast module and demo moved to `attic/`, its pass and wiring removed; dead code deleted.
6. Docs: `AGENTS.md` (root, `src/core`, `ecs`, `systems`, `physics`, `graphics`, `render`, `scene`-related notes in `core/AGENTS.md`, `examples/demos`), the skills `ryo-api`, `ryo-ecs-internals`, `ryo-game-dev`, `ryo-engine-dev`, `ryo-physics`, `ryo-events`, `ryo-animation`, `ryo-config-save` (for `fixed_hz`), the CLI's embedded skill copies, `docs/architecture/overview.md`, and the `init` template.
7. `./dev check`, `./dev lint`, `./dev test`, demo tour; merge.

Commits are checkpoints at the end of each numbered step, confirmed with the owner.

## Testing

Test-first for the new core pieces (unit tests beside the code, integration tests under `tests/integration`):

- Clocks: accumulation across uneven frames, `max_delta` cap, `set_speed(0.5)` halves step count over a fixed real duration, `pause` and `hitstop` run zero steps and hitstop ends in real time, `overstep_fraction` in [0, 1), `delta()` inside versus outside `FixedUpdate`.
- Propagation: parent at 90 degrees places a child at (10, 0) local at (0, 10) world; three-level chains; reparent; `detach_keep_world` leaves the world position unchanged; cycle insertion is rejected; parent despawn removes descendants; unchanged subtrees are not rewritten (`Changed<GlobalTransform>` stays false).
- Interpolation: a frame with zero steps keeps the previous snapshot and renders between the same pair with a larger alpha; a frame with two steps snapshots after step one; `skip_interpolation` renders at the new position on the next frame; a child of an interpolated parent is interpolated.
- Input: `just_pressed` true for one frame; visible in the first fixed step only when two steps run; consumption hides a key from gameplay in the same frame; `RYO_INPUT` still drives presses.
- Messages: written in frame N, readable in frame N+1 by a system and by a scene cursor, each scene sees each message once, buffers survive a frame with no fixed step.
- Scene schedules: systems registered in `on_enter` run in `FixedUpdate`/`Update`; they stop after Pop/Switch/Reset; `updates_below` scenes keep running; `is_physics_paused` stops `FixedSet::Physics`.
- Context: `Time`, `Input`, `Camera` and `Assets` are visible as resources to systems during a frame and are back in the world after every scene callback. A panic inside a scene callback aborts the frame; restoring the scoped resources after a panic is not required and is documented as such.
- Physics: a body under gravity covers the same distance per second at `fixed_hz` 60 and 120; speed 0.5 halves its distance per real second; a kinematic body moved by `Transform` arrives; an untouched sleeping body is not woken by the sync.
- Regression: all existing unit, integration and e2e tests migrated; e2e pixel expectations updated where `Anchor::Center` moves sprites; `./dev demos:tour` captures every page of every surviving demo.
- Benchmark: `benches/foundation.rs` (criterion) with 10k entities in a three-level hierarchy through propagation and extraction, recorded in the plan's final report.

## Documentation

Rustdoc on every public type above. The `ryo-api` skill is the canonical reference and is rewritten for the new surface; the other listed skills are updated to match. `src/core/ecs/AGENTS.md` explains the phases, sets, the resource-scope rule and the interpolation model; `src/core/AGENTS.md` loses the time-scale section in favour of a pointer to `Time`.

## Design decisions

- **bevy_ecs rather than hecs plus a bespoke layer:** the engine needs scheduling, resources, messages, hierarchy and change detection; hand-written versions of each were found buggy; agents produce correct code against the bevy API far more reliably than against a bespoke one.
- **Expose the types, not a wrapper:** a wrapper that is almost bevy's API is the surface agents get wrong, and it blocks `Query`-based systems from being first class.
- **Constant fixed delta, variable step count:** stability for physics and tweens; matches Bevy and Godot.
- **Interpolate globals at render time, Godot-style:** `GlobalTransform` is always a true state, so gameplay never reads a blend; children of interpolated parents work without extra marking.
- **Propagate at the end of each fixed step and again in `PostUpdate`:** both change-detected, so the second pass is free when per-frame systems moved nothing.
- **Scene-owned schedules instead of scope ids:** `bevy_ecs` schedules have no remove-by-id; owning the schedule gives scoping for free.
- **Five hot resources as context fields via resource scopes:** avoids the borrow conflicts a single `&mut World` field would cause for the most common scene code.
- **Single-threaded executor now:** determinism and simplicity while every subsystem is rewritten; the multi-threaded executor is a configuration change later.
- **UI before gameplay in the frame:** the only ordering under which consumption is correct for immediate-mode UI.
- **Default anchor Center:** the convention of Godot and Unity, and the one under which collider auto-centring is trivial.

## Risks

- Derive macro path resolution from downstream crates (mitigated by the step-1 spike and the derive-shim fallback).
- `bevy_ecs` 0.20 is three days old at the time of writing; the 0.19 to 0.20 migration guide was still marked draft. The plan pins `0.20.x` and the first task compiles the ECS module against it before anything else.
- The migration is a long-lived branch; it is mitigated by the ordered steps, each leaving the branch compiling and tested.
