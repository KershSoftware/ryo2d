# Ryo2D Architecture

The big picture: how a frame runs, where state lives and what each part of the engine does. Module conventions and decisions are in the nested `AGENTS.md` files; the API is in rustdoc and the `ryo-api` skill.

## Workspace

| Path | What |
|------|------|
| `src/core/` | The engine (the `ryo2d` crate); the prelude in `src/lib.rs` is its public surface |
| `src/cli/` | The `ryo2d` command |
| `crates/ryo2d-derive` | `#[derive(NetworkInput)]`, re-exported by the core |
| `crates/ryo2d-procgen`, `crates/ryo2d-inventory` | Optional crates on top of `ryo2d`; a game adds them and imports their own preludes |
| `examples/` | `demo-kit` (the demo shell), `demos/*` (one feature each), `games/*` |
| `tests/integration`, `tests/e2e` | Integration tests and headless GPU end-to-end tests |
| `attic/` | Parked code outside the build: the raycaster and its demo, to return as `crates/ryo2d-raycast` on the render pipeline |

Dependencies point toward the core: the optional crates depend on `ryo2d`, never the reverse.

## Engine, World and Resources

```
main.rs
  EngineBuilder::new()
    .title(..) .resolution(..) .fixed_hz(60)
    .insert_resource(..) .add_message::<T>() .add_systems(phase, systems)
    .start_scene(MyScene)
    .build()   -> Engine: the world, six schedules, renderers, window
    .run()     -> winit event loop, one Engine::frame() per redraw
```

The world is Ryo2D's ECS: `bevy_ecs` 0.20, exposed through `ryo2d::ecs` and the prelude (`World`, `Entity`, `Query`, `Commands`, `Resource`, `Message`, ...). Game code never names the crate. Engine state that scenes and systems share is a resource; what cannot or need not be shared stays with the engine.

| Resources (in the world) | Engine-owned |
|---|---|
| `Time`, `Input`, `Camera`, `AssetManager`, `EffectsManager`, `LightingSettings`, `TilemapManager`, `WindowRequest`, `CursorRequest`, `WindowInfo`, `RenderPipeline`, `TextureManager`, `Gpu` (once the GPU is up), `RapierPhysicsWorld`, `PhysicsConfig`, `PhysicsPaused`, the console's `CommandRegistry`, one `Messages<T>` per message type, and the game's own resources | the window, the renderers (sprite, lighting, trail, compositor, post-process), gilrs, the dev windows, frame capture, the console overlay and the debug flags; the scene stack and the Kira audio manager sit in the world as one non-send value that only the scene systems take out |

In 0.20 a resource is a component on a hidden entity marked `IsResource`, so engine sweeps over "every entity" filter it out.

## The Frame

`Engine::frame()` runs on every redraw, and at the background rate while the window is unfocused. Headless runs call the same function with the clock advanced by exactly one fixed step.

```
Engine::frame()
 │
 ├─ queue scripted input (RYO_INPUT) and gamepad events
 ├─ First ........ advance the clocks, publish Input, advance message buffers (after a fixed step)
 ├─ debug keys ... F1-F5, `, F9, F10, F12 consume their presses
 ├─ PreUpdate .... async asset loads, hot reload, scene transitions (real time); then the console
 ├─ UI pass ...... Scene::ui for visible scenes (bottom-up; scenes under the top one
 │                 get no pointer), debug overlays, console
 │                 what the UI consumed is hidden from Input for the rest of the frame;
 │                 a press it took stays hidden until released
 │                 a returned SceneAction applies at once (a new scene draws its UI this frame)
 ├─ while fixed.accumulator >= timestep            (0..N times; a quit ends the loop)
 │   └─ FixedUpdate
 │        Snapshot ..... GlobalTransform -> PreviousGlobalTransform
 │        Juice ........ animation, tweens, knockback, squash/stretch, skeletons
 │        Scene ........ Scene::update(ctx, timestep) unless a transition runs, its SceneAction
 │        Game ......... the scenes' own FixedUpdate systems
 │        Physics ...... unregister, register (root bodies only; edits in place), sync to
 │                       Rapier, step, sync back, collision messages
 │        Transforms ... propagate GlobalTransform
 │        Trails ....... record trail points
 ├─ Update ....... UpdateSet::Game: the scenes' own per-frame systems
 ├─ PostUpdate ... Transforms (propagation), Camera (shake), Extract (timed effects)
 ├─ render ....... Scene::render, query the world into the draw list, run the RenderPipeline
 └─ Last ......... physics registration bookkeeping, clear change trackers
```

- Phases are bevy schedules labelled by `Phase`; the sets inside them are `FixedSet`, `UpdateSet` and `PostUpdateSet`, chained in the order shown. Every schedule uses the single-threaded executor.
- Games add systems with `EngineBuilder::add_systems(phase, systems)` for the whole run, or `ctx.add_systems(Phase::FixedUpdate | Phase::Update, systems)` for schedules a scene owns, which run in the `Game` sets while the scene is active and are dropped with it.
- The UI runs before gameplay so that input it consumes never reaches `Scene::update`.
- Systems may return `Result<(), SystemError>`; the fallback handler panics in debug builds and logs in release.

## Three Clocks

`Time` is a resource, and `ctx.time` in scene callbacks.

| Clock | Delta | Used by |
|-------|-------|---------|
| `time.real` | wall clock, never scaled or paused | camera shake, UI animation (`ui.dt()`), console, timed post-process effects, the hitstop countdown, lighting and trail animation |
| `time.game` | real delta capped at `max_delta` (250 ms), times `speed`, zero while paused or in hitstop | feeds the fixed clock; sprite effects animate on its elapsed time |
| `time.fixed` | constant `timestep` (`1 / fixed_hz`, default 60 Hz) | `FixedUpdate`: `Scene::update`, juice, physics, scene fixed systems |

- Slow motion runs fewer fixed steps, pause and hitstop run none; the step length never changes, so physics and tweens behave the same at any speed.
- `time.delta()` is the timestep inside `FixedUpdate` and the game delta elsewhere.
- `fixed.overstep_fraction()` is the render alpha used for interpolation.

## ECS: Transforms, Hierarchy, Interpolation

- `Transform` is local (`translation: Vec2`, `rotation` in radians, `scale: Vec2`). `GlobalTransform` is world space and written only by the engine; `Transform` requires it, and inserting a `Transform` places the `GlobalTransform` at once.
- The hierarchy is the kernel's `ChildOf(parent)` / `Children`. Despawning a parent despawns its children. `WorldExt` adds `spawn_child`, `detach_keep_world` and `skip_interpolation`.
- Propagation runs at the end of each fixed step and again in `PostUpdate`, both change-detected, so unmoved subtrees cost nothing.
- Interpolation is opt-in with `Interpolated` (covers descendants): each fixed step snapshots `GlobalTransform` into `PreviousGlobalTransform`, and the renderer draws between the two at the render alpha. Gameplay always reads the true `GlobalTransform`.
- Pooling uses the kernel's `Disabled` marker: every query skips a disabled entity unless it asks with `Allow<Disabled>`. `Pool<B>` disables and enables entities instead of spawning.

| Area | Components |
|------|------------|
| Placement | `Transform`, `GlobalTransform`, `ChildOf`, `Children`, `Interpolated`, `PreviousGlobalTransform` |
| Drawing | `Renderable` (sprite or tiled run, `Anchor` default `Center`, z order, tint, shadows, sprite shader), `Material`, `YSort`, `CompositeTile` |
| Animation | `Animator`, `AnimationController`, `ValidatedAnimator`, `Skeleton`, `SkeletalAnimator`, `SkeletalController` |
| Juice | `Tween`, `TweenSequence`, `PropertyTween<T>`, `Knockback`, `SquashStretch` |
| Physics | `RigidBody`, `Collider`, `GroupedCollider`, `Velocity` |
| Lights, trails, particles | `Light2D`, `LightOccluder2D`, `Trail2D`, `Line2D`, `ParticleEmitter`, `ParticleAttractor` |
| Navigation | `NavAgent`, `NavTarget` |

## Scenes

```
SceneManager (engine-owned stack over the one shared world)
  [PauseMenu]   top: update, ui; is_transparent -> scenes below still draw and get ui
  [Level]       paused unless the scene above updates_below
```

- A `Scene` implements any of `on_enter`, `on_exit`, `on_pause`, `on_resume`, `update` (each fixed step), `ui` (once a frame, before the steps) and `render` (GPU-batched sprites), plus the flags `is_transparent`, `updates_below`, `is_physics_paused`.
- `SceneAction`: `Continue`, `Push`, `Pop`, `Switch`, `Reset`, `PopReturn`, `Quit`, each with a `*With(Transition)` form (fade or slide).
- `SceneContext` has the fields `world`, `time`, `input`, `camera`, `assets`, `audio` and helpers for everything else (`effects()`, `lighting()`, `render_pipeline()`, `window()`, `cursor()`, `textures()`, `tilemaps()`, `console()`, `messages::<T>()`, `write_message`, `resource*`, `add_systems`). `time`, `input`, `camera` and `assets` are taken out of the world for the length of the callback.
- On `Switch` and `Reset` the leaving scene's `cleanup_on_exit()` (`SceneCleanup` flags: world, camera, effects, lighting, time, music, messages) resets engine state; `Push` and `Pop` keep it.

## Messages

- A message type derives `Message`. The engine's (`CollisionStarted`/`Stopped`, `TweenStarted`/`Completed`, `KnockbackStarted`/`Finished`, `SquashStretchStarted`/`Finished`, `AssetReloaded`, `AssetLoadFailed`) are registered at build; a game's register with `EngineBuilder::add_message::<T>()` or on the first `ctx.write_message`.
- Buffers advance in `First`, but only once a fixed step has run since the last advance: a message stays readable until two fixed steps have run after it was written, so every reader, per-frame or fixed-step, sees it at any frame rate. While no fixed step runs (the game clock paused) nothing is dropped.
- Systems use `MessageWriter<T>` / `MessageReader<T>`; scenes use `ctx.write_message(m)` and `ctx.messages::<T>()`, which keeps a cursor per scene so each scene sees each message once.
- Modules define no message type they do not write; games define their own notifications.

## Input

```
winit / gilrs / RYO_INPUT / headless harness
   -> queued InputEvents -> Input rebuilt in First -> UI pass (consumes) -> FixedUpdate / Update
```

- One-shots (`key_pressed`, `mouse_pressed`, ...) are seen by the first fixed step of a frame only; a press in a frame that runs no step waits for the next step, except while the game clock is paused, when it is dropped (gameplay never saw it).
- `consume_key` / `consume_mouse` / `consume_text` hide an input, held state included, for the rest of the frame. The UI pass consumes what its widgets use.
- `block_pointer` / `block_all` / `unblock` gate input for gameplay.

## Physics

Rapier2D behind the `RapierPhysicsWorld` resource, stepped in `FixedSet::Physics` with `dt = time.fixed.timestep()`, skipped while `PhysicsPaused`.

1. Register bodies that gained `RigidBody`, `Collider` or `GroupedCollider`, or were re-enabled; unregister removed and disabled ones (this bookkeeping also runs in `Last` every frame).
2. Push a changed `Transform` or `Velocity` to its body (only when it differs from the body's, so untouched bodies sleep).
3. Step.
4. Write positions and velocities of awake dynamic bodies back to `Transform` / `Velocity`.
5. Drain contacts into `CollisionStarted` / `CollisionStopped` messages.

- 100 pixels = 1 meter; gravity in pixels/s² (`PhysicsConfig`, default `(0, 980)`, +Y down).
- Colliders are centred on the entity origin plus `offset`. Bodies are dynamic or kinematic, rotation locked, and must be root entities.

## Rendering

`present_frame` builds each frame from the world by query; there is no render cache. It collects the scenes' `Scene::render` batches, every visible `Renderable` (at its interpolated `GlobalTransform`, placed by its `Anchor`), trails, lines, lights and occluders into one draw list, then runs the pass-based `RenderPipeline` (`RenderPipeline::default_2d()`): an ordered list of `RenderPass`es recorded on one command encoder with one submit. The world is drawn on the GPU; only the UI is drawn on the CPU, into a buffer composited last.

```
clear -> world -> lighting -> unlit_world -> transition
      -> post_process -> ui -> composite -> capture
```

| Pass (`pass_ids`) | Draws |
|------|-------|
| `clear` | Clears the game texture |
| `world` | Entities, `Scene::render` sprites (particles, parallax) and lit trails/lines, in one z-sorted list |
| `lighting` | 2D lighting with point lights and shadows |
| `unlit_world` | Trails/lines with `with_lighting(false)`, by z |
| `transition` | Scene transition fade over the frame |
| `post_process` | Bloom, CRT, vignette and the other effects |
| `ui` | Uploads the UI layer |
| `composite` | Letterboxed present, UI blended last |
| `capture` | Frame capture readback |

Games reach the pipeline through `ctx.render_pipeline()` to toggle built-in passes or insert their own. See `src/core/render/AGENTS.md`.

- **Batching:** consecutive sprites on one atlas page and pipeline merge into one draw; at equal z an entity draws before a `Scene::render` sprite, then trails and lines. 128 bytes per instance (affine transform, UV rect, tint, effect parameters, UV bounds).
- **Time in shaders:** sprite effects and custom sprite shaders animate on game elapsed time (sprite globals `x`), so they stop with the game clock; the globals also carry real elapsed time in `w`. Post-process effects (built-in and custom), lighting, trails and transitions animate on real time.
- **Sprite effects** (`SpriteEffect`, 11 built in): Outline, Glow, Flash, ColorShift, Silhouette, Pixelate, Wave, Dissolve, Invert, Grayscale, DirectionalOutline. Custom sprite shaders through `Material`.
- **Post-processing** (`EffectsManager`, `ctx.effects()`): Fade, ColorTint, Vignette, ScreenFlash, CRT, ChromaticAberration, GodRays, AmbientOcclusion, Bloom.
- **Lighting:** `ctx.lighting()` (`LightingSettings`: ambient, atmosphere, forced on or off); lighting turns on by itself once a `Light2D` exists. The UI is never lit.
- **Surface errors:** `Outdated` and `Timeout` skip the frame, `Lost` reconfigures, only out-of-memory and device loss are fatal. Vsync uses `Fifo`; without it `Mailbox`, else `Immediate`, with the frame limiter at `target_fps`.

## Audio

[Kira](https://github.com/tesselode/kira), reached as `ctx.audio`.

- Volume is multiplicative: master × category (SFX or music) × the sound's own.
- SFX are loaded by name and cached (`load_sound`, `play_sound_with(SoundOptions)`: volume, panning, playback rate); music streams from disk (`play_music_with(MusicOptions)`: looping, fade in, `fade_to`, `fade_out`, `crossfade_music`).
- `SceneCleanup::MUSIC` stops music on a scene change; the default cleanup keeps it playing.

## UI

Immediate mode for simple elements, retained state for stateful widgets, drawn on the CPU in `Scene::ui(ui, ctx)` once a frame.

| Immediate | Retained state | Layout and style |
|-----------|----------------|------------------|
| `button`, `label`, `slider`, `checkbox`, `toggle`, `text_input`, `dropdown`, `scroll_view`, `draw_progress_bar`, `draw_rect`/`draw_text`/`draw_sprite` | `TextInputState`, `DropdownState`, `ScrollViewState`, `FocusTrap` | `HBox`, `VBox`, `Stack`; `Theme` (hot-reloadable from JSON); `UiTween` / `TweenFloat`/`Vec2`/`Color`/`Rect` |

- Widgets consume the input they use, so gameplay in `update` does not see it.
- A scissor stack clips nested views; focus moves with Tab / Shift+Tab, and `FocusTrap` keeps it inside a modal.

## Saves and Settings

| What | File | API |
|------|------|-----|
| Player settings (resolution, display mode, vsync, volumes, FPS, console) | `system.json`, uncompressed JSON | `SystemState`, `UserPreference`, `EngineBuilder::with_user_preferences` |
| Game data | `{slot}.ryo-save`: magic `RYO2`, version, DEFLATE-compressed RON | `SaveManager`, `GameSaveData<T>` |

Settings resolve in order: engine defaults, `Ryo.toml`, `EngineBuilder` calls, then the saved `system.json` preferences the game asks for. Saves go to the OS user data folder, or `data/` beside the game with `[release] portable`. The engine does not snapshot the world; a game saves the state it needs as its own data.

## Dev Tools

| Key | Action |
|-----|--------|
| F1 | Cycle overlay: off, FPS, frame timing, full stats |
| F2 / F3 / F4 | Collider outlines / velocity vectors / origin points |
| F5 | Toggle borderless fullscreen |
| `` ` `` | Developer console (`[system] console`) |
| F9 | Logger window |
| F10 | Profiler window (the `profiler` feature) |
| F12 | Capture a frame |

- **Profiler (F10):** a puffin flamegraph with game, CPU and GPU scopes; `profile_scope!` / `profile_function!` are no-ops without the feature.
- **Logger (F9):** live log with level and source filters, timestamps, selection and copy; `game_info!`, `game_warn!` and friends write to it.
- **Console:** built-in commands plus game commands declared and registered per scene (`ctx.console()`).
- **Scripting:** `RYO_HEADLESS`, `RYO_INPUT` and `RYO_CAPTURE_*` drive a game without a window; see the `ryo-cli` skill.

## CLI

| Command | Does |
|---------|------|
| `ryo2d init [name]` | Create a project (Ryo.toml, entry point, assets, optional skills) |
| `ryo2d dev [-p]` | Development run with file watching (`-p`: profiler) |
| `ryo2d build` / `ryo2d run` | Optimised release build / run it |
| `ryo2d export` | Package the release into a zip |
| `ryo2d install-skills` | Install the agent skills to `.claude/skills/` or `.github/skills/` |
| `ryo2d netplay` | Start a signalling server for netplay testing |

The CLI finds the workspace by searching up for `Ryo.toml` and embeds its configuration at build time through `RYO_EMBED_*` variables.

### Ryo.toml

| Section | Holds | When |
|---------|-------|------|
| `[project]` | name, title, version, description | build time |
| `[paths]` | entry point, assets, save data | build time |
| `[system]` | defaults for `system.json`: resolution, vsync, display mode, volumes, FPS, console | first run, then the player's |
| `[debug]` | overlay flags, debug keys, profiler, logger, dev window keys | development |
| `[release]` | logo, portable saves, save folder | build time |
| `[env]` | game-specific values | game code |

The fixed rate is not a Ryo.toml setting: set it with `EngineBuilder::fixed_hz`.

## Coordinates

```
World space (pixels)                     Screen space
camera at its centre                     (0,0) at the top-left

          ▲ -Y                                0 ─────────────► X
          │                                   │
◄─────────┼─────────► +X                      │   (after the camera transform)
          │                                   │
          ▼ +Y                                ▼ Y
```

- +Y is down in both. A sprite's `Anchor` (default `Center`) is the point placed at its entity's position; rotation and scale happen about it (a `TopLeft` sprite is the exception: it turns about its centre). Tiled runs and composite tiles are laid out from the position as their first tile's top-left.
- Rapier works in meters: 100 pixels = 1 meter.

## Networking

Client-server multiplayer with listen-server support. See [the network architecture](./network.md).

- **Transport:** UDP via renet (unreliable, reliable ordered, reliable unordered channels); WebRTC through matchbox with the `matchbox` feature.
- **Servers:** `NetworkServer` (dedicated), `ListenServer` (player-hosted).
- **Replication:** `NetworkEntity`, `Authority`, `DirtyTracker`, `ReplicationManager`; **RPC:** `RpcRegistry`; **prediction:** `Predicted`, `PredictionHelper`.

## Modules

| Module | Purpose |
|--------|---------|
| `ecs/` | Kernel re-exports, `Transform`, propagation, interpolation, pooling, `Renderable`, juice components |
| `engine/`, `scene/`, `schedule.rs`, `time.rs`, `input.rs`, `messages/` | The loop, scenes, phases, clocks, input, messages |
| `systems/` | The engine's built-in schedule systems |
| `graphics/` | Sprites, animation, camera and shake, parallax, tiled and composite rendering, the sprite renderer |
| `render/` | The render pipeline, passes and the draw list |
| `shaders/` | Post-process effects and custom shaders |
| `texture/` | Texture handles and arrays for custom passes |
| `lighting/`, `trail/`, `particles/`, `skeleton/`, `tilemap/` | Feature renderers and their components |
| `physics/` | Rapier integration, colliders, bodies |
| `navigation/` | Grids, A* pathfinding, agents, steering |
| `state_machine/` | Generic state machines and stacks |
| `asset/`, `audio.rs` | Asset loading and hot reload; audio |
| `ui/` | Immediate and retained UI, themes, layout, UI tweens |
| `config/` | Workspace (`Ryo.toml`), settings, saves |
| `console/`, `windows/`, `capture/`, `dev_log.rs`, `profiler.rs` | Developer tools |
| `window.rs`, `cursor.rs` | Window and cursor requests |
| `network/` | Networking |
| `testing/` | `HeadlessEngine` and the shared GPU for tests |

## Key Dependencies

| Crate | Purpose |
|-------|---------|
| `bevy_ecs` | The ECS: storage, queries, schedules, change detection, hierarchy, messages |
| `glam` | Math types (`Vec2`, `Affine2`, `Mat4`) |
| `wgpu` | GPU abstraction for rendering |
| `winit` | Windowing and input |
| `rapier2d` | Physics simulation |
| `kira` | Audio |
| `gilrs` | Gamepads |
| `renet` | Networking transport |
| `puffin` | Profiling (optional) |
| `image`, `resvg` | Image and SVG loading |
| `notify` | File watching for hot reload |
| `ron`, `serde_json`, `flate2`, `toml` | Saves, settings and configuration |
| `arboard` | Clipboard |
