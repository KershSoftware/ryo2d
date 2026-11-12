# AGENTS.md - ECS Module

Ryo2D's ECS is `bevy_ecs` 0.20, presented as Ryo2D's own: `kernel.rs` re-exports a curated list of kernel types (`World`, `Entity`, `Query`, `Commands`, `Resource`, `Message`, `ChildOf`, `Disabled`, ...), and the rest of the module holds Ryo2D's components (`Transform` and friends, `Renderable`, `Velocity`, `Floor` (a storey, which physics filters by and drawing lifts by, with the crate's `DrawLift`), the juice components), `WorldExt`, `Pool`, propagation and interpolation. Skills: `ryo-ecs-internals` (contributors), `ryo-game-dev` (games), `ryo-animation` (tweens).

## The Kernel Name

`bevy_ecs` appears only in `Cargo.toml`, `kernel.rs` and the hidden re-exports in `src/lib.rs` (crate root and prelude). The derives (`#[derive(Component)]`, `Resource`, `Message`, ...) expand to paths rooted at `bevy_ecs`; a game's `use ryo2d::prelude::*` brings that name into scope, so derives work with no extra dependency. Everything else imports from `crate::core::ecs` (or `ecs::kernel` for crate-internal items such as `HookContext`); a kernel type a module needs is added to `kernel.rs`, `pub(crate)` unless games need it.

## Phases

`Engine::frame` (`engine/frame.rs`) runs, every frame:

| Step | Cadence | Engine work |
|------|---------|-------------|
| `First` | once | advance the clocks, publish `Input`, advance every message buffer; then the engine's debug and console keys consume their presses |
| `PreUpdate` | once | a running scene transition, then the asset registry's pump (finished loads, reloads and frees land, and their `AssetEvent`s are sent), chained so a switch's frees and the entering scene's loads are pumped in its frame, then the maps whose asset is ready spawn (`spawn_maps`); then the console |
| UI pass | once | `Scene::ui` for visible scenes bottom-up, debug overlays, console; consumption applied to `Input`; a returned `SceneAction` applies at once, and a scene change redraws the UI once so the new scene shows this frame |
| `FixedUpdate` | 0..N | one run per fixed step; the sets below, chained |
| `Update` | once | `UpdateSet::Game`: scene per-frame systems and builder ones by default |
| `PostUpdate` | once | `PostUpdateSet::Transforms` (propagation), `Camera` (shake on real time, then every camera's `CameraView` from its interpolated transform), `Extract` (timed effects, real time) |
| render | once | `present_frame` (`engine/extract.rs`): queries the world into the draw list, plans the cameras, runs the render pipeline (not a schedule) |
| `Last` | once | physics registration bookkeeping, `clear_trackers`, `check_change_ticks` every 1024 frames; the engine then clamps its own, the scenes' and physics' schedules and the UI node cache's tick (`PendingTickCheck`) |

`FixedSet`, in order:

| Set | Systems |
|-----|---------|
| `Snapshot` | `snapshot_previous_transforms` |
| `Juice` | animation, tweens, tween sequences, property tweens, squash/stretch (`register_juice_systems`) |
| `Scene` | transitions, `Scene::update` for the active scene and those below `updates_below` scenes, then the action |
| `Game` | the scenes' own `FixedUpdate` schedules |
| `Movement` | zone behaviours, tile rebuilds, movement input, knockback, presets (`register_gameplay_systems`, `core/gameplay/`) |
| `Physics` | one system running physics' own schedule (`PhysicsSchedule`): register, unregister, sync to Rapier, step, sync back, collision messages (`register_physics_systems`; skipped while `PhysicsPaused`) |
| `Transforms` | `propagate_transforms` (then the tile map's `lift_floored`) |
| `Skeletons` | nothing yet: the set is where skeleton animation will run (`core/skeleton/`) |
| `Particles` | `step_particles`: emitters spawn, and their particles move and die (`core/particles/`) |
| `Trails` | `record_trail_points` |

A quit from the UI pass or a step ends the fixed loop; no step runs after it. Systems are registered in `register_engine_systems` (`engine/mod.rs`); every schedule uses the single-threaded executor. Systems may return `Result<(), SystemError>`: the `FallbackErrorHandler` resource panics in debug builds and logs in release.

Game systems: `EngineBuilder::add_systems(phase, systems)` for the whole run (any phase; in `FixedUpdate`/`Update` a call whose systems are not ordered against anything outside it runs in its `Game` set; one that names a set or another call's system (`.in_set`, `.before`, `.after`) keeps exactly that order), or `ctx.add_systems(Phase::FixedUpdate | Phase::Update, systems)` for a schedule the scene owns, run inside `FixedSet::Game` / `UpdateSet::Game` while the scene is active (or below an `updates_below` scene) and dropped with it. Other phases return `SceneScheduleError::UnsupportedPhase`. A scene's schedule is a `Schedule` of its own, run by one engine system in the `Game` set, so the engine's sets have no members in it: a call that names one where it does not hold (`.after(FixedSet::Physics)`, `.in_set(FixedSet::Juice)`) is refused with `EngineSet` rather than silently ignored, and one that restates the `Game` set (`.before(FixedSet::Physics)`) is accepted. A call from the line that added any of the same systems (by `System::system_type`) in an earlier scene callback is refused with `AlreadyAdded`: a call in `update` would otherwise add a copy every step. `SceneSchedules::add_systems` (`schedule.rs`) puts each call in a `SceneCallSet` of its own, checks the graph, and takes a refused call out with `remove_systems_in_set`.

## The Resource-Scope Rule

`SceneContext::scoped_parts` (`scene/context.rs`) takes `Time` and `Input` out of the world with nested `resource_scope`s for the length of a scene callback and hands them over as `ctx.time` and `ctx.input`; the audio engine is a resource too (`AudioEngine`, reached by `ctx.audio()`). Cameras are entities, so `ctx.camera()` works on `ctx.world` directly. So:

- Inside a callback those two are not in `ctx.world`: `ctx.world.resource::<Time>()` panics, and a system run on `ctx.world` that takes `Res<Time>` fails. Use the fields.
- Systems run outside callbacks and always see them as resources.
- Everything else stays in the world; the context's helpers (`assets()`, `camera()`, `effects()`, `lighting()`, `render_pipeline()`, `render_targets()`, `window()`, `cursor()`, `console()`, `resource*`) reach into it.
- A panic in a callback aborts the frame; the scoped resources are not put back.

## Transform and Hierarchy

- `Transform` is local (`translation: Vec2`, `rotation` in radians, `scale: Vec2`); `GlobalTransform` is world space, an `Affine2` written only by the engine. `Transform` requires `GlobalTransform`.
- **Insert hook:** inserting a `Transform` (spawn or `insert`) sets `GlobalTransform` at once: the parent's current global times the local transform for a child, the local transform otherwise. So an entity spawned in `update` is where it should be for the rest of the step (a burst emitter fires from its position, a navigation grid built after spawning walls sees them). Edits through `get_mut::<Transform>` wait for propagation.
- **Propagation** (`propagate_transforms`) runs at the end of every fixed step (`FixedSet::Transforms`, after physics), in physics' own schedule, and in `PostUpdate` (per-frame edits); each run sees the changes since its own last run. A run starts from the entities that changed: those the `Placement` filter finds (`Transform` or `ChildOf` changed, `Interpolated` added), those that lost their parent or `Interpolated`, and the children of a parent that lost its `Transform`. Each is recomputed from its parent's stored `GlobalTransform`, then its subtree, unless an ancestor changed too, whose walk reaches it; the walk up stops at the first parent without a `Transform`, and skips an entity under a disabled one (no walk reaches it, as before). So a subtree where nothing changed is not visited: a big static map costs nothing to walk while the camera or a player moves. Once the changed entities are a fifth of the entities with a `Transform` (`FULL_WALK_SHARE`), and more than 64 (`ALWAYS_SEEDED`), a run walks every hierarchy from its roots (`Transform` without `ChildOf`) instead, recomputing a subtree whose root or an ancestor changed. The changed subtrees' walk costs the change filter's full scan (about 0.7 ns an entity; the full walk's scan stops at the share) and, for each changed entity, a walk up its ancestors by entity lookups (about 25 ns for a leaf two deep) and its subtree; the full walk visits each entity in about 5 ns. So the two cost the same at about a fifth (`benches/foundation.rs`: 2,000 leaves moved of 10,000 entities, 83 against 81 µs), and its 100 roots moved, whose subtrees are every entity, cost about 5% more through the changed subtrees than through the full walk: the scan. Entities marked `ParentPlaced` (`pub(crate)`, in `transform.rs`: a map's collider parts and shadow chains, which have no children) are `Without`-ed out of the `Placement` scan, so thousands of still ones cost it nothing; only their parent's walk and `Transform`'s insert hook place them, and their own `Transform`, `ChildOf` or `Interpolated` changing in place seeds no walk (the marker's insert hook gives one under an interpolated ancestor its `PreviousGlobalTransform` at once). Writes go through `set_if_neq`, so an unmoved entity's `GlobalTransform` is not marked changed. Children of a parent with no `Transform` start from the identity and inherit no interpolation. The walk's stack and its lists of changes are `Local`s, kept between runs, so a warm run allocates nothing (`tests/tilemap_alloc.rs`: a map's child entities grew a stack made fresh each run, about 18 allocations a frame). A chain of more parents than the world has entities is taken for a cycle and skipped, so a cycle (never valid) cannot hang the walk. **What remains:** finding the changed entities is a change-tick check per entity per run, for the filter's scan: about 7 µs a run for 10,000 entities (`benches/foundation.rs`, `nothing moved` and `one leaf moved`). With a map's colliders and shadow chains out of the scan (`ParentPlaced`), `benches/tilemap.rs`'s `static_frame` reads 35.8 to 47.8 µs over an empty frame on every grid, the hexagonal map the dearest at 44.4 to 47.8, and the orthogonal map's 2,882 children add 0.7 to 3.0 µs to the map drawn only, where they added 9.9 to 10.3 before the marker (three runs, `docs/backlog/tilemap.md`); an edit's frame was 16 to 24 µs over an ordinary frame before it (265 to 279 µs before that; `docs/backlog/tilemap.md`, with the limits).
- **Hierarchy** is the kernel's `ChildOf(parent)` / `Children`. Spawn a child with `world.spawn_child(parent, bundle)` (`WorldExt`, also `ctx.spawn_child`) or `(bundle, ChildOf(parent))`. Reparent by inserting `ChildOf`; remove it to detach (the local transform becomes the world one), or call `detach_keep_world(entity)` to keep the entity where it is. Despawning a parent despawns its descendants. Nothing checks for cycles longer than an entity parenting itself: never make an entity its own ancestor.
- Physics bodies may sit anywhere in the hierarchy (see `physics/AGENTS.md`); `Respawn` (gameplay) writes a world position into `Transform.translation`, so it belongs on root entities.

## Floors (`floor.rs`)

- `Floor` (public) is the storey an entity is on; physics filters colliders by it (`physics/AGENTS.md`). An entity's `Transform` is where it stands on the ground: a `Floor` never moves it.
- `DrawLift { lift, floor }` (`pub(crate)`) is how far up an entity draws, a world vector, and the storey its draw order ties on (`draw_floor`: `Every` as 0). The tile map module's `lift_floored` (`tilemap/floors.rs`, after propagation in each fixed step, `FixedSet::Transforms` before `Particles`, and each frame, `PostUpdate` after `PostUpdateSet::Transforms`, before `Camera`) alone writes it: on every entity with a `Floor` (not `ParentPlaced`: a map's own parts are drawn by their layers) and every descendant, disabled ones too, down to one with a `Floor` of its own, the floor's height on the map with floors under the entity (zero off every one); absent on the ground floor, unlifted. A map's objects and class tiles on a higher floor carry that `Floor` (the layer does not). So a sprite on a child, a held item, a name tag draw lifted with their entity. Its rules (what it rescans, removal) are in the tilemap module notes.
- Every draw site adds `lift` after interpolating (`render_global(..) + lift`: sprites with their shadows and tiled runs, composite tiles, world text), and particles and trails add it where they record a point (emission, `record_trail_points`): a trail spawned on a floor records its first point lifted and an emitter spawned and fired in one step fires lifted, because `lift_floored` runs before both. Sorting keeps the ground: a y-sort key and `YSort`'s depth scale read where the entity stands, and `DrawOrder::floor` takes `draw_floor`. Lines and the debug overlays stay in ground space.

## Interpolation

- Opt in with the `Interpolated` marker; it covers the entity's descendants.
- Propagation keeps `PreviousGlobalTransform` on every entity that is `Interpolated` or below one and removes it elsewhere. A new one starts with `skip: true`, so its first frame draws at the current transform.
- `FixedSet::Snapshot` (first in each step) copies `GlobalTransform` into `PreviousGlobalTransform` and clears `skip`.
- At render, `render_global(global, previous, time.fixed.overstep_fraction())` blends previous to current (linear translation and scale, shortest-arc rotation); with `skip` set or no previous, it is the current transform. The blend keeps exact whatever did not change: a still entity draws exactly where it is, and a moving one keeps an unchanged axis, rotation and scale. Without that, a still camera on a half pixel drew a pixel off on the frames that blended it (a fixed step ran) and not on the others, so a whole map jittered by a pixel. Gameplay never reads a blend: `GlobalTransform` is always the true state.
- After a teleport, `skip_interpolation(entity)` (`WorldExt`, `ctx.skip_interpolation`) draws the new position at once; interpolation resumes from the next step. It also marks a `ParticleEmitter` on the entity, interpolated or not, so its next particle step spawns only at the new place (particles module notes).
- Everything an interpolated entity draws (single sprites, tiled runs, composite tiles, lights, occluders, lines) is placed at `render_global`, so it stays together. On a floor, sprites, tiled runs, composite tiles and world text add `DrawLift::lift` after the blend (Floors, above); lines, lights and occluders stay where the entity stands. Trail points are recorded world positions and are not blended.

## Disabled and Pooling

- `Disabled` is the kernel's entity-disabling marker: every query skips a disabled entity unless it asks with `Allow<Disabled>` (or names `Disabled` in its terms). Engine systems, physics (unregisters, re-registers on enable), rendering and particles skip disabled entities with no filter of their own.
- `Pool<B>` (`pooling.rs`) spawns its batch disabled; `acquire` removes `Disabled`, `release` / `release_all` insert it. `acquire` does not reset components (set every field you rely on); it sets the interpolation skip so the entity does not slide from where it was released.
- `acquire`, `release` and `is_active` are O(1): the active list is a queue of (acquisition number, entity) plus an entity-to-number map, and a released entry stays in the queue as stale until skipped or compacted. A pool drops entities the world no longer has (a `SceneCleanup::WORLD` sweep) lazily; `RecycleOldest` pops stale and dead entries off the front, and the other policies purge fully when nothing is free. A recycled entity is put back as `release` puts one back (disabled, with its descendants) before `acquire` enables it again, so whatever watches `Disabled` (joints, tile colliders, audio emitters) sees a recycle as a release and an acquire. `Disabled` is not hierarchical, so `release` also disables the entity's descendants (marking the ones it disabled with a private `DisabledWithRoot`) and `acquire` restores exactly those. The two halves are crate-visible (`disable_descendants`, `enable_descendants`): a respawning `Pickup` (gameplay) uses them too.

## Resources Are Entities

In `bevy_ecs` 0.20 a resource is a component on a hidden entity marked `IsResource`; observers (`Observer`) and registered one-shot systems are entities too, and plain entity queries see them. Any "every entity" sweep must filter them out, or it despawns a resource and panics in the resource hook:

```rust
world.query_filtered::<Entity, (Allow<Disabled>, Without<IsResource>)>()
```

`despawn_game_entities` (`world_ext.rs`, used by `SceneCleanup::WORLD`) also excludes `Observer` and `SystemIdMarker`; the entity count in the debug stats does the same per archetype. It despawns each hierarchy from its top (the entities sorted by depth; the world lists them by archetype, children often first), so a component's discard hook still sees the entity's children: a streamed map's root saves its changed chunks from its layers and class tiles as a scene's cleanup takes it (`despawn_game_entities_despawns_each_hierarchy_from_its_top`).

Each resource also has an archetype of its own, and a query visits archetypes in hash-map order, so adding any resource can reorder a query's results. Wherever an order shows (draw ties, physics registration, a message order), sort by `ecs::entity_order` (entity index), never by a query's or a hash set's order. Tests built on `test_util::insert_unrelated` check an order for 0 to 8 extra resources.

## Messages

- A message type derives `Message` and is registered with `register_message::<T>` (`core/messages`): at engine build for the engine's types, `EngineBuilder::add_message::<T>()` for a game's, or lazily on the first `ctx.write_message`.
- Buffers are double-buffered. Buffers advance in `First`, but only once a fixed step has run since the last advance: a message stays readable until two fixed steps have run after it was written, so every reader, per-frame or fixed-step, sees it at any frame rate. While no fixed step runs (the game clock paused) nothing is dropped. Each reader sees a message once.
- Systems use `MessageWriter<T>` / `MessageReader<T>`. Scenes use `ctx.write_message(m)` and `ctx.messages::<T>()`, which reads through a `MessageCursor` kept per scene instance (`SceneMessages`), so every scene sees each message once.
- `SceneCleanup::MESSAGES` empties every buffer. Modules define no message type they do not write; a game that wants a notification defines its own.

## PropertyTween Auto-Driving

- `update_property_tweens` (`systems/tween.rs`) drives only `PropertyTween<f32>`, `<Vec2>` and `<Color>`.
- Other `Lerp` types (including `(f32, f32)` and `Rect`): call `tween.update(dt)` yourself, or add a query there.
