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
| `Time`, `Input`, `ScreenMapping`, `AssetServer` and one `Assets<T>` per asset type, `AudioEngine`, `EffectsManager`, `LightingSettings`, `Look`, `WindowRequest`, `CursorRequest`, `WindowInfo`, `RenderPipeline`, `RenderTargets`, `Gpu` (once the GPU is up), `Physics`, `PhysicsConfig`, `PhysicsPaused`, the console's `CommandRegistry`, one `Messages<T>` per message type, and the game's own resources | the window, the renderers (sprite, lighting, trail, compositor, post-process, the tile drawer), gilrs, the dev windows, frame capture, the console overlay and the debug flags; the scene stack sits in the world as a non-send value that only the scene systems take out |

In 0.20 a resource is a component on a hidden entity marked `IsResource`, so engine sweeps over "every entity" filter it out. Cameras are entities too (`Camera2d` + `Transform`); the main one (`MainCamera`) is spawned with the world and survives world cleanup.

## The Frame

`Engine::frame()` runs on every redraw, and at the background rate while the window is unfocused. Headless runs call the same function with the clock advanced by exactly one fixed step.

```
Engine::frame()
 │
 ├─ queue scripted input (RYO_INPUT) and gamepad events
 ├─ First ........ advance the clocks, publish Input, advance message buffers (after a fixed step)
 ├─ debug keys ... F1-F5, `, F9, F10, F12 consume their presses
 ├─ PreUpdate .... a running scene transition (real time), then the asset pump: loads and
 │                 reloads land, unheld assets are freed, their AssetEvents are sent; then tile
 │                 maps: ready maps start landing (worked out on the map builder thread),
 │                 landings that came back land whole (MapEvent::Spawned), spawned maps follow
 │                 saved files (MapEvent::Reloaded); then the console
 ├─ UI pass ...... resolve the pointer, the wheel and focus against last frame's UI records;
 │                 Scene::ui for visible scenes (bottom-up; only the top scene's widgets take
 │                 input), debug overlays, console
 │                 what the UI consumed (and the actions navigation used) is hidden from Input
 │                 for the rest of the frame; a press or binding it took stays hidden until released
 │                 a returned SceneAction applies at once (a new scene draws its UI this frame)
 ├─ while fixed.accumulator >= timestep            (0..N times; a quit ends the loop)
 │   └─ FixedUpdate
 │        Snapshot ..... GlobalTransform -> PreviousGlobalTransform
 │        Juice ........ animation, tweens, squash/stretch, skeletons
 │        Scene ........ Scene::update(ctx, timestep) unless a transition runs, its SceneAction
 │        Game ......... the scenes' own FixedUpdate systems; builder systems by default
 │        Movement ..... trigger behaviours, tile maps' collision sync, tile collider rebuilds,
 │                       movement input, knockback, movement presets
 │        Physics ...... one system: forget removed, track disabled, propagate, register bodies,
 │                       sync colliders, push changes, move characters, sync joints, step,
 │                       read back, messages, trigger contents
 │        Transforms ... propagate GlobalTransform
 │        Trails ....... record trail points
 ├─ Update ....... UpdateSet::Game: the scenes' own per-frame systems; builder ones by default
 ├─ PostUpdate ... tile maps' occluder sync, then Transforms (propagation), Camera (shake, every
 │                 camera's view), Extract (timed effects);
 │                 audio_system after Camera (voices, buses, music, listener and emitters, game clock)
 ├─ render ....... Scene::render, query the world into the draw list (tile maps' visible chunks
 │                 from the batch's chunk store), plan the cameras, run the RenderPipeline
 └─ Last ......... physics removal and re-enable bookkeeping, clear change trackers
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
| `time.game` | real delta times `speed`, capped at `max_delta` (250 ms, or one fixed timestep if longer) per frame, zero while paused or in hitstop | feeds the fixed clock; sprite effects animate on its elapsed time; audio buses pause with it and follow its speed (not its hitstop) |
| `time.fixed` | constant `timestep` (`1 / fixed_hz`, default 60 Hz) | `FixedUpdate`: juice, `Scene::update`, scene fixed systems, gameplay, physics |

- Slow motion runs fewer fixed steps, pause and hitstop run none; the step length never changes, so physics and tweens behave the same at any speed.
- `time.delta()` is the timestep inside `FixedUpdate` and the game delta elsewhere.
- `fixed.overstep_fraction()` is the render alpha used for interpolation.

## ECS: Transforms, Hierarchy, Interpolation

- `Transform` is local (`translation: Vec2`, `rotation` in radians, `scale: Vec2`). `GlobalTransform` is world space and written only by the engine; `Transform` requires it, and inserting a `Transform` places the `GlobalTransform` at once.
- The hierarchy is the kernel's `ChildOf(parent)` / `Children`. Despawning a parent despawns its children. `WorldExt` adds `spawn_child`, `detach_keep_world` and `skip_interpolation`.
- Propagation runs at the end of each fixed step, in physics' own schedule and again in `PostUpdate`. A run starts from the entities that changed since its last run and walks only their subtrees, each from its parent's stored `GlobalTransform`, so a still subtree (a big tile map's thousands of collider parts and shadow chains) is not visited, and an unmoved entity's `GlobalTransform` is not rewritten. Finding the changed entities is still a change-tick check per entity per run. Once a fifth of the entities with a `Transform` changed (and more than 64), a run walks every hierarchy instead, where the two walks cost about the same (`src/core/ecs/AGENTS.md`; `docs/backlog/tilemap.md`, Limits).
- Interpolation is opt-in with `Interpolated` (covers descendants): each fixed step snapshots `GlobalTransform` into `PreviousGlobalTransform`, and the renderer draws between the two at the render alpha. Gameplay always reads the true `GlobalTransform`.
- Pooling uses the kernel's `Disabled` marker: every query skips a disabled entity unless it asks with `Allow<Disabled>`. `Pool<B>` disables and enables entities instead of spawning.

| Area | Components |
|------|------------|
| Placement | `Transform`, `GlobalTransform`, `ChildOf`, `Children`, `Interpolated`, `PreviousGlobalTransform` |
| Drawing | `Renderable` (sprite or tiled run, `Anchor` default `Center`, z order, tint, shadows, sprite shader), `Material`, `YSort`, `CompositeTile` |
| Animation | `Animator`, `AnimationController`, `ValidatedAnimator`, `Skeleton`, `SkeletalAnimator`, `SkeletalController` |
| Juice | `Tween`, `TweenSequence`, `PropertyTween<T>`, `Knockback`, `SquashStretch` |
| Physics | `RigidBody`, `Collider`, `Joint`, `Velocity`, `AngularVelocity`, `CharacterController` (and the engine-written `CharacterState`), `TileColliders`, `TriggerContents` (engine-written) |
| Gameplay | `MoveIntent`, `MoveBindings`, `PlatformerMovement`, `TopDownMovement` (and the engine-written `PlatformerState`, `TopDownState`), `Pickup`, `KillZone`, `Checkpoint`, `Respawn` |
| Lights, trails, particles | `Light2D`, `LightOccluder2D`, `Trail2D`, `Line2D`, `ParticleEmitter`, `ParticleAttractor` |
| Navigation | `NavAgent`, `NavTarget` |
| Tile maps | `MapRoot`; on what it spawns, `MapIds`, `MapLayerRef`, `LayerInfo`, `TileLayerData`, `ImageLayer`, `MapObject`, `MapTile`, `MapOccluder`; `WorldLevelOf` on a world's spawned levels |

## Scenes

```
SceneManager (engine-owned stack over the one shared world)
  [PauseMenu]   top: update, ui; is_transparent -> scenes below still draw and get ui
  [Level]       paused (and not heard) unless the scene above updates_below
```

- A `Scene` implements any of `on_enter`, `on_exit`, `on_pause`, `on_resume`, `update` (each fixed step), `ui` (once a frame, before the steps) and `render` (GPU-batched sprites), plus the flags `is_transparent`, `updates_below`, `is_physics_paused` and `holds_audio_below` (the scenes a top scene does not update are not heard: their voices are held until they update again).
- `SceneAction`: `Continue`, `Push`, `Pop`, `Switch`, `Reset`, `PopReturn`, `Quit`, each with a `*With(Transition)` form (fade or slide).
- `SceneContext` has the fields `world`, `time`, `input` and helpers for everything else (`assets()`, `audio()`, `camera()`, `render_targets()`, `effects()`, `lighting()`, `render_pipeline()`, `window()`, `cursor()`, `maps()`, `console()`, `messages::<T>()`, `write_message`, `resource*`, `add_systems`). `time` and `input` are taken out of the world for the length of the callback; `assets()` and `audio()` borrow the world for as long as they are used.
- On `Switch` and `Reset` the leaving scene's `cleanup_on_exit()` (`SceneCleanup` flags: world, camera, effects, lighting, time, music, messages) resets engine state; `Push` and `Pop` keep it.

## Messages

- A message type derives `Message`. The engine's (`CollisionStarted`/`Stopped`, `TriggerEntered`/`Exited`, `ContactForce`, `JointBroken`, `MovementEvent`, `PickedUp`, `Killed`, `CheckpointReached`, `TweenStarted`/`Completed`, `KnockbackStarted`/`Finished`, `SquashStretchStarted`/`Finished`, `ParticleCollision`, `MapEvent`, `UiEvent`, `UiReloaded`, `UiListBind`) are registered at build, and each asset type's `AssetEvent<T>` with its type; a game's register with `EngineBuilder::add_message::<T>()` or on the first `ctx.write_message`.
- Buffers advance in `First`, but only once a fixed step has run since the last advance: a message stays readable until two fixed steps have run after it was written, so every reader, per-frame or fixed-step, sees it at any frame rate. While no fixed step runs (the game clock paused) nothing is dropped.
- Systems use `MessageWriter<T>` / `MessageReader<T>`; scenes use `ctx.write_message(m)` and `ctx.messages::<T>()`, which keeps a cursor per scene so each scene sees each message once.
- Modules define no message type they do not write; games define their own notifications.

## Input

```
winit / gilrs / RYO_INPUT / headless harness
   -> queued InputEvents -> Input rebuilt in First -> UI pass (consumes) -> FixedUpdate / Update
```

- One-shots (`key_pressed`, `mouse_pressed`, ...) are seen by the first fixed step of a frame only; a press in a frame that runs no step waits for the next step, except while the game clock is paused, when it is dropped (gameplay never saw it).
- **Actions** (`Actions` in `Input`, `ctx.actions()`): named bindings to keys, mouse buttons, pad buttons and stick directions (digital past 0.5 deflection), read with `action_pressed` / `action_held` / `action_released`. Rebinding swaps a collision inside a group (`ui.*` or the game's); with `UserPreference::Bindings` the player's bindings are saved in `system.json`. The UI navigates with the `ui.*` actions, which the engine binds and games may rebind.
- `last_device()` follows the newest key, mouse or pad input; prompts and the UI's focus ring follow it, and `PadFamily` (from the pad's name) labels the buttons. Scripted and headless pad input arrives as id-less `PadButton` / `PadStick` events on the tracked pad, or a virtual one.
- `consume_key` / `consume_mouse` / `consume_text` hide an input, held state included, for the rest of the frame. The UI pass consumes what its widgets and navigation use.
- `block_pointer` / `block_all` / `unblock` gate input for gameplay.

## Physics

Rapier2D 0.36 behind the public `Physics` resource, which mirrors the ECS: components describe bodies, colliders and joints, and the systems in `physics/sync.rs` keep Rapier's world in step with them. `rapier2d` is named only inside `src/core/physics/`. Stepped in `FixedSet::Physics` with `dt = time.fixed.timestep()`, skipped while `PhysicsPaused`, by one exclusive system running physics' own schedule (`PhysicsSchedule`), so no game system can be ordered inside the step; the chain is `forget_removed`, `track_disabled`, `propagate_transforms`, `register_bodies`, `sync_colliders`, `push_changes`, `move_characters`, `sync_joints`, `step_simulation`, `read_back`, `emit_messages`, `update_trigger_contents`.

1. Forget what went away and what was disabled (also in `Last` every frame, so a despawn is never missed at high render rates); propagate transforms so world poses are current.
2. Register bodies and colliders (a collider attaches to the nearest `RigidBody` at or above it; none above makes it a fixed collider), apply edits in place, make joints.
3. Push a changed `Transform`, `Velocity` or `AngularVelocity` to its body, compared by value so untouched bodies sleep (what the last write-back alone changed, recognised by its change tick, is skipped without comparing); a kinematic body travels to its written `Transform` over the step.
4. Sweep every character (`CharacterController`, a kinematic body) by its `Velocity` through Rapier's queries (a 2D port of Rapier's character controller): floors, walls and ceilings told apart into `CharacterState`, carried by what it stands on, pushing dynamic bodies, held by one-way colliders only from their solid side; the real velocity goes back into `Velocity`. A game write to its `Transform` is a teleport.
5. Step, then break joints pulled past their break force. One-way colliders shape the solver's contacts through a contact hook.
6. Write back the poses of awake dynamic bodies, velocity-driven kinematic bodies and characters into the local `Transform` (through the parent), and dynamic bodies' velocities into `Velocity` and `AngularVelocity`.
7. Send `CollisionStarted`, `CollisionStopped`, `TriggerEntered`, `TriggerExited` (once per body, its trigger colliders apart from its solid ones), `ContactForce` and `JointBroken` messages, and keep every trigger's `TriggerContents`.

- Public surface: components `RigidBody`, `Collider` (with `one_way` and `as_trigger`), `Joint`, `AngularVelocity`, `CharacterController` / `CharacterState`, `TileColliders` (a grid of cells that builds seamless outline, one-way, slope and authored-shape colliders as child entities, packed by 32-cell chunk; an edit retraces only the outlines through it and reshapes their chunks' colliders in place), `TriggerContents`; resources `Physics` (queries: `raycast`, `shape_cast`, `overlap_*`, `nearest`; contact state: `contacts`, `overlaps`, `in_contact`; forces and impulses) and `PhysicsConfig`; reached as `ctx.physics()` / `ctx.physics_mut()`.
- `PhysicsConfig::pixels_per_meter` pixels make a Rapier metre (default 100; choose it so typical moving bodies are 0.5 to 2 m); gravity in pixels/s² (default `(0, 980)`, +Y down).
- Time: one frame adds at most `max_delta` (250 ms) of game time after speed scaling, so a stall runs about 15 fixed steps at 60 Hz, not an unbounded number. Slow motion runs fewer steps; pause and hitstop run none.
- Colliders follow the entity's world scale, mirroring and rotation. Details and decisions: `src/core/physics/AGENTS.md`.

## Gameplay

`src/core/gameplay/` builds on physics (never the reverse; the one component physics reads, `Knockback`, lives in `ecs/`) and runs in `FixedSet::Movement`, between the scene's fixed systems and the physics step, as one chain: the trigger behaviours (`Pickup`, `KillZone`, `Checkpoint`, with `Respawn`) act on the previous step's `TriggerEntered`; `MoveBindings` fill each `MoveIntent` from the keyboard and gamepad (AI and networking write it directly); knockback decays; and the movement presets (`PlatformerMovement`, `TopDownMovement`) turn the intent and the last move's `CharacterState` into each character's `Velocity`, which the physics step then sweeps. Physics adds the `TileColliders` rebuild to the same set. Under `PhysicsPaused` the input, knockback, presets and pickup timers wait, while the behaviours keep draining the trigger messages that arrived before the pause. The presets report `MovementEvent`, the behaviours `PickedUp`, `Killed` and `CheckpointReached`, read by `update` on the next step. Details: `src/core/gameplay/AGENTS.md`; usage: the `ryo-character` skill.

## Rendering

`present_frame` builds each frame from the world by query; there is no render cache but one, the tile chunks: each 32 x 32-cell chunk of a tile layer keeps its instances in the batch's chunk store, built again only when its tiles or look change, while where it draws is read every frame. Drawables are gathered once, in world space, and sorted once: the scenes' `Scene::render` sprites, every visible `Renderable` (at its interpolated `GlobalTransform`, placed by its `Anchor`), trails and lines (as joined polylines), lights and occluders; parallax layers and debug shapes are laid out per camera. Then each camera keeps what it sees (its `RenderLayers`, its view's bounds) and one 2D batch (`render/batch`) lays it into draws, and the pass-based `RenderPipeline` (`RenderPipeline::default_2d()`) runs on one command encoder with one submit: the camera passes once per camera, then the frame passes once. Everything is drawn on the GPU through each camera's view-projection matrix, the UI included: it is recorded as commands, built by a UI renderer of its own (`render/ui_renderer`) into an image of its own through a UI camera, and composited last.

```
gather once (world space, sorted) ──→ cameras in render order: texture cameras, then window cameras (by order)
                                       │
  for each camera, on its image:       clear -> world -> lighting -> unlit_world
  (Rgba16Float, linear light,          │   (the first full-window camera draws straight into the scene image;
   may exceed 1)                       │    other window cameras into images of their own; texture cameras
                                       │    into their render targets)
                                       ▼
  once per frame:                      cameras -> hdr_effects -> tonemap -> transition -> post_process
                                         -> ui -> composite -> capture
                                       (window cameras' images into their viewports; the scene image is
                                        Rgba16Float up to tonemap, which writes the Rgba8UnormSrgb display image)
```

The world is drawn in linear HDR: every colour in the API is sRGB and is decoded once (`Color::to_linear`) where it reaches the GPU, blending happens in linear light on `Rgba16Float` images, and light above 1 survives until the `tonemap` pass applies the frame's `Look` (`ctx.look()`: exposure, auto-exposure, a tonemap curve, colour grading and a LUT) and writes the 8-bit display image. Effects run in two stages: HDR effects (bloom, god rays, ambient occlusion) see linear light before the look; display effects (fade, tint, vignette, flash, CRT, chromatic aberration) work on the display image after it. Custom effects choose their stage (`EffectsManager::add_custom_in`). `Renderable::with_emission` lets a sprite, particle, trail or line glow past white and feed bloom.

| Pass (`pass_ids`) | Scope | Draws |
|------|-------|-------|
| `clear` | camera | Clears the camera's image (background, a colour, or transparent so the cameras below show) |
| `world` | camera | The camera's lit batch draws: entities, `Scene::render` sprites (particles), parallax, trails, lines and debug shapes, in one z-sorted list |
| `lighting` | camera | 2D lighting with point lights and shadows, in the camera's own light buffers |
| `unlit_world` | camera | Anything `with_lighting(false)` (sprites, particles, trails, lines) and debug shapes, by z |
| `cameras` | frame | Window cameras' own images into their viewports, in order |
| `hdr_effects` | frame | Bloom, god rays, ambient occlusion and HDR custom effects on the scene image |
| `tonemap` | frame | The look: the scene image into the display image (exposure, curve, grading) |
| `transition` | frame | Scene transition fade over the frame |
| `post_process` | frame | Fade, tint, vignette, flash, CRT, chromatic aberration and display custom effects on the display image |
| `ui` | frame | Draws the UI (rects, sprites, lines and text) into the UI image |
| `composite` | frame | To the window through `ScreenMapping` (bars for pixel art), the UI image blended last in sRGB space |
| `capture` | frame | Frame capture readback |

Games reach the pipeline through `ctx.render_pipeline()` to toggle built-in passes or insert their own (`RenderPass::scope` picks once per frame or once per camera). See `src/core/render/AGENTS.md`.

- **Cameras:** `Camera2d { zoom, projection, viewport, order, target, clear }` on an entity with a `Transform`; the engine computes each camera's `CameraView` (view-projection, bounds) from its interpolated transform and shake every frame. Every camera has one uniform buffer that the batch, lights and shadows all draw through, so shake, rotation and zoom move them together. Split screen and minimaps are window cameras with viewports; `RenderTargets` gives textures that texture cameras render into (first, so a sprite showing one shows this frame's image) and `Renderable::render_target` shows.
- **Textures:** every texture has one `TextureId` in the texture registry (images by asset id, named pixel textures, render targets). Sprite images live in shared atlas pages with four mip levels (8-pixel borders on an 8-pixel grid, levels averaged in linear light); large images get textures of their own with full mip chains; render targets are `Rgba16Float` pages of their own (linear light). Two samplers (nearest; trilinear) and a bind group per page and sampler, made with the page.
- **Batching:** one batch with two streams: instanced quads (128 bytes: affine world transform, UV rect, tint, effect parameters, UV bounds) and indexed triangles (trails, lines and debug shapes). Consecutive drawables with equal draw states (page, shader, blend mode, sampling) merge into one draw; at equal z an entity draws before a `Scene::render` sprite, then trails and lines. Each camera's visible quads get a range of the instance buffer.
- **Blend and filter modes:** `BlendMode::{Alpha, Premultiplied, Additive, Multiply, Screen}` and `Filter::{Default, Nearest, Linear}` on sprites, particles, batch sprites, parallax layers, trails and lines. Every batch shader writes premultiplied colour, so each mode is one fixed blend state; `Filter::Default` is nearest for pixel art and linear otherwise.
- **World text** draws are batch items too: glyphs from the glyph pages (`TextMask`, `TextColor`, `TextMsdf`) share the batch's streams, sorting and merging. The UI has a renderer of its own: one instance format for rects, glyphs, sprites and line triangles, one pipeline that clips in the shader, and a draw only where a bound texture changes. See [Text and UI](#text-and-ui).
- **Bind groups** are made with the resources they bind (per camera, per texture page and sampler, or cached by the frame resources a pass binds), never per frame; tests count them.
- **Debug drawing:** `ctx.debug_draw()` (`DebugDraw`) draws world-space lines, rectangles, circles, arrows and crosses one screen pixel wide in every camera, for a fixed step or a frame; the engine's overlays and `draw_skeleton` use it.
- **Time in shaders:** sprite effects and custom sprite shaders animate on game elapsed time, so they stop with the game clock; post-process effects (built-in and custom), lighting, trail effects and transitions animate on real time. Parallax auto-scroll runs on game time. Custom shader errors are collected asynchronously and never block a frame.
- **Sprite effects** (`SpriteEffect`, 11 built in; they work in sRGB): Outline, Glow, Flash, ColorShift, Silhouette, Pixelate, Wave, Dissolve, Invert, Grayscale, DirectionalOutline. Custom sprite shaders through `Material`.
- **Post-processing** (`EffectsManager`, `ctx.effects()`): Fade, ColorTint, Vignette, ScreenFlash, CRT, ChromaticAberration (display stage) and GodRays, AmbientOcclusion, Bloom (HDR stage). Once per frame, not per camera.
- **The look:** `ctx.look()` (`Look`: exposure, `AutoExposure`, `Tonemap`, `ColorGrading` with an optional `ColorLut`) is applied by the `tonemap` pass.
- **Lighting:** `ctx.lighting()` (`LightingSettings`: ambient, atmosphere, forced on or off); lighting turns on by itself once a `Light2D` exists and then lights every camera. Lights and shadow volumes are batch draws into each camera's light buffer, with no light cap; each shadow darkens only its own light. The UI is never lit.
- **Surface and GPU errors:** a suboptimal surface draws and then reconfigures; timeout and occlusion skip the frame; outdated reconfigures; lost recreates; a validation error or a lost device is fatal. GPU setup failures are `GpuError`s. Vsync uses `Fifo`; without it `Mailbox`, else `Immediate`, with the frame limiter at `target_fps`.

### Text and UI

- **The text core** is `src/core/text/`, GPU-free: `Fonts` (a resource: the faces of the `Font` and `BitmapFont` assets the world holds, following their stores; Inter, JetBrains Mono and m5x7 are built in behind the `default-fonts` feature), `TextStyle`, rich text and markup, effects and the reveal clock. Parley lays text out (no system fonts), swash rasterizes glyphs and fdsm makes distance fields; **those three crates are named only in `src/core/text/`** (a test guards it), and the renderer receives `GlyphImage` bitmaps, never a font type. Layouts are cached by text, style, options, scale and font generation, so an unchanged immediate-mode call is a hash lookup: the UI hashes each text once a frame with the engine's fast hasher (`TextKey`), and a hit is confirmed against the stored text, so a collision is never another text's layout.
- **The glyph atlas** holds glyph bitmaps in glyph pages beside the sprite atlas: raster coverage, colour glyphs and MSDF fields, each placed on first use; when a format is full its least recently used page is cleared. Raster glyphs are made at the size the camera sees, so text stays sharp at any zoom.
- **World text** is a `Text2d` entity: laid out when its text, style or fonts change, its glyphs emitted per camera into the same z-sorted draw list as sprites (unlit by default, lit with `with_lighting`). Effects and the typewriter run on game time; opacity is applied at draw time, so fades never lay text out again.
- **The UI** is commands: `Scene::ui` records rects, sprites, lines and text in UI units. The UI renderer builds them into its own instances in painter's order, the `ui` pass draws them through a UI camera into a gamma-encoded UI image, and the composite blends that image over the finished frame in sRGB space (a 50% black panel over white is mid grey, as on the web). Effects, lighting and post-processing never touch it. UI units equal screen units with the automatic scale and are sharp on HiDPI; `ui_scale` makes the whole UI bigger. Popups (open lists, `ui.popup`, modals), tooltips and the drag ghost draw on a popup layer above the scene's page, and the engine's overlays and the console above that. The UI's model is in [UI](#ui).
- The dev windows are separate OS windows with CPU pixel buffers; they draw text with `CpuText` (the mono font through swash).

Details: `src/core/text/AGENTS.md`, `src/core/ui/AGENTS.md`, `src/core/render/AGENTS.md`.

### Resolution and the window

`ScreenMapping` (a resource) is the one mapping between window pixels and screen space, used by the compositor, the mouse and the tests. With a resolution set (`resolution(w, h)` or `[system] resolution`), the fit presentation makes screen space `w`x`h`: the frame renders at the window's physical size, scaled by any factor to fit the window and centred with bars where the aspect ratios differ. With `pixel_art(w, h)` (or `pixel_art = true` in `Ryo.toml`) the world renders into a `w`x`h` target scaled by a whole number to fit the window, centred with bars, sampled nearest, with sprites snapped to whole target pixels; screen space is the target. With no resolution the game is resolution-independent: the world renders at the window's physical size (sharp on HiDPI), screen space is the window in logical pixels, and the main camera's `Projection::Fit` keeps a 1280x720 design area in view at any window size. The UI is in UI units (screen units with the automatic scale) and is composited over the scene.

### The web build

The engine builds for `wasm32-unknown-unknown` and runs in browsers with WebGPU: winit starts the app asynchronously (the GPU arrives as an event), time comes from `web-time`, file-backed features (hot reload, saves, the clipboard, reading asset files) are compiled out or report `Unsupported`, so a web game embeds its assets and adds them, and with no threads the asset pump runs queued loads itself, at most 4 ms a frame. `./dev.sh web <demo>` serves a demo with trunk (demos with an `index.html` and `Trunk.toml`); `check` and `lint` build the engine for wasm32 too. The `ryo2d` command has no web build yet.

## Assets

```
ctx.assets().load_now::<Image>("ship.png")   loads on this thread and inserts it: drawn this frame
ctx.assets().load::<Image>("level2/bg.png")  queued for a worker thread; a handle at once
ctx.assets().add(Image::solid_rect(..))      an asset made in code
   │
   worker threads (one to four; on the web the pump runs the jobs, at most 4 ms a frame)
   │   AssetSource::read -> AssetLoader::load, dependencies through LoadContext
   ▼
PreUpdate pump: lands finished loads (once their dependencies have, and the loader's check
                passes), sends the events of changes made in code, frees unheld assets
   │
   Assets<T> (a store per type, a content version per value) + AssetEvent<T> messages
   (Added, Modified, Removed, Failed, ReloadFailed)
```

- One registry for every asset type: `AssetServer` (a resource) loads by asset path, relative to the project's `assets/` folder, through `AssetLoader`s chosen by extension, and each type's values are in its `Assets<T>` resource. Scenes reach both through `ctx.assets()`; systems take `Res<AssetServer>` and `Res<Assets<T>>`.
- A `Handle<T>` keeps its asset alive, and the pump frees an asset at its first run after the last handle drops. The engine's caches (textures, compiled shaders, text layouts) key by `AssetId`, so the renderer never keeps alive what the game let go.
- An asset is its type, its path and its loader's settings (`load_with`): asking again gives the same handle, and one file at two SVG sizes is two images. Each value has a content version, and what is built from an asset (a texture, a UI tree, a text layout) is built again when it moves.
- The engine's types are `Image` (raster formats and SVG), `Shader` (WGSL), `Font` and `BitmapFont`, `UiFile`, `Theme`, `AudioClip` (WAV, OGG Vorbis, MP3, FLAC), `Sound` and `Music` (definitions) and `Mixer`, and for tile maps `TileMap` and `Tileset` (Tiled), `TileWorld` (Tiled worlds) and `LdtkProject`; a game adds its own (`impl Asset`, an `AssetLoader`, `EngineBuilder::asset_loader`). A UI file's imports and images, a theme's skin images and sounds, a bitmap font's sheet and a sound definition's clips are dependencies: the asset is inserted once they have loaded and its loader's `check` passes.
- An image that is not loaded draws a magenta checker in debug builds and nothing in release ones.
- **Hot reload** is on in debug builds with a window (`EngineBuilder::hot_reload`): the asset folder is watched, a saved file reloads under the same handle about 100 ms after its last write, the assets that hold it are checked again, and a reload that fails keeps the last good value.

Details: `src/core/asset/AGENTS.md`; usage: the `ryo-assets` skill.

## Audio

A mixer on [kira](https://github.com/tesselode/kira) 0.12, built for sound authored in a DAW and shaped in data files, reached as `ctx.audio()`.

```
AudioClip  .wav .ogg .mp3 .flac: decoded (up to 10 s) or streamed   ┐
Sound      .sound.ron: clips, bus, level, pitch, limits, spatial    │ registry assets,
Music      .music.ron: an intro into a gapless loop, or a loop      │ hot reload
Mixer      audio/mixer.mixer.ron: buses, effects, sends, snapshots  ┘

ctx.audio().play(&sound) -> Voice -> its bus's track -> .. -> master -> device (or mock backend)
```

- **Assets.** Clips, sound definitions, music and the mixer load through the registry by asset path, with dependencies and hot reload. A clip up to 10 s decodes to memory on a worker; a longer one keeps its bytes and streams through the engine's own decoder, which opens the file on kira's decoder thread so a play costs the main thread microseconds. The web decodes every clip.
- **The mixer.** The standard buses always exist (`master`, then `music`, `sfx`, `ui`, `voice`, `ambience`); the mixer file adjusts them and adds a game's own under them, each a kira track with its effects, sends, limit and whether it pauses with the game or follows time scale. A bus's gain is layers added in dB: the mixer's (or the game's) volume, the player's volume setting (amplitude = setting², so dB = 40·log10(v)), the active snapshot and any duck. Snapshots blend the mix to named presets; ducking rules dip one bus while another plays.
- **Voices.** `play` builds a mixer that has landed, picks a clip by weight (avoiding recent picks), draws gain and pitch, starts a sound at most `per_frame` times a frame (once by default: copies started together only stack into distortion), applies the cooldown and the voice limits (per sound, per bus, and a cap of 96 a bus track below kira's 128, stealing the least important voice; priority is 0 to 255, higher more important; nothing stops a more important voice or an equal one started that frame) and returns a `Voice` handle, which never stops when dropped. A loop whose sound is still loading waits and starts once it is ready; a one-shot then is refused. A voice belongs to the scene that played it (its loops and its one-shots with more than 2 s left stop as the scene exits; its short one-shots finish), to the engine (`persist`), or to an `AudioEmitter`'s entity. The game's `set_volume` and `set_pitch` are a layer over the sound's authored level. Every stop, pause and change is smoothed over at least 10 ms, so none of them clicks (a start fades in over the definition's `fade_in`), and fades to and from silence are shaped so a crossfade holds its level.
- **The game clock.** `audio_system` (`PostUpdate`) holds the buses that pause with the game while it is paused, and on those buses the voices of the scenes the stack does not update (a pause menu pushed over the level silences it as it freezes its physics), plays the buses that follow time scale at the game's speed (up to 8x), and leaves hitstop alone. 2D spatial sound (falloff, pan, a distance low-pass) follows the listener (an `AudioListener`, else the main camera) and the emitters there.
- **Music** is the engine's one player: crossfades, carries on across scene changes, and joins an intro to its loop into one sound so the loop is sample-exact.
- **No device** (headless, or a device that failed to open): the same mixer runs silently on kira's mock backend, rendered by the frame's time; headless runs are seeded, so tests repeat and assert on the audio log. **The web** opens the device at the player's first click, tap or key.

Details: `src/core/audio/AGENTS.md`; usage: the `ryo-audio` skill; measurements: `docs/backlog/audio.md`.

## Tile Maps

Levels made in Tiled and LDtk, read by the engine's own readers through the asset registry, spawned as entities and kept in step with their files (design: `docs/specs/2026-10-29-tilemap-design.md`; orthogonal maps, with isometric, hexagonal and infinite maps next, `docs/backlog/tilemap-features.md`).

```
TileMap      .tmx .tmj: layers over tilesets (or built in code)  ┐
Tileset      .tsx .tsj: tiles, shapes, animation, Wang sets      │ registry assets,
LdtkProject  .ldtk (+ .ldtkl): worlds, levels, tilesets          │ hot reload
TileWorld    .world: Tiled maps placed in a world                ┘

MapRoot entity ─(assets ready)→ landing job on the map builder thread (colliders, shadows,
   paddings) ─→ lands whole in one frame: layer, object and class-tile entities, TileColliders,
   MapOccluder chains, MapEvent::Spawned
```

- **Assets.** Maps, tilesets, LDtk projects and Tiled worlds load by asset path with their dependencies (tilesets, images, a world's maps); what a file only reads (templates, the Tiled project, LDtk level files) is read through the registry too, so a save of it reloads the reader. Every layer encoding loads on every target (zstd through `ruzstd`), and a bad file fails with the place in it.
- **Spawning.** A `MapRoot` (a map, an LDtk level, or a world's level) on an entity whose `Transform` places the map. `spawn_maps` (`PreUpdate`, after the asset pump) sends a ready map's landing to the map builder thread and lands what comes back: layers, objects, class tiles, colliders and shadows arrive in one frame with `MapEvent::Spawned`, so a body placed then stands on its ground. The web, with no threads, works the landing out in that frame.
- **Reading and editing.** `MapsView` (`ctx.maps()`) reads tiles, IntGrids and tile data, converts cells and world positions, edits (`set_tile`, `fill`) and paints Wang terrain as Tiled's brush does; an edit touches only its chunk's drawing and the collision outlines and shadow chains through it.
- **Collision and shadows.** `sync_map_colliders` (`FixedSet::Movement`, before the tile collider rebuild) keeps each map's colliding layers in `TileColliders` grids, setting only the cells whose kind changed; `sync_map_occluders` (`PostUpdate`, before propagation) keeps `LightOccluder2D::chain`s of the occluding boundary per chunk.
- **Drawing.** The render step's tile drawer pads each tileset against bleeding (made on the landing's thread the first time), builds each changed chunk's instances into the batch's chunk store, and draws the chunks a camera can see through a per-draw slot (the map's transform, the camera's parallax, the tileset's place in the atlas), y-sorted layers row by row among the entities.
- **Worlds and hot reload.** `spawn_levels_in` spawns a world's levels near a place; when to unload is the game's call. `reload_maps` (`PreUpdate`) brings spawned maps to saved files, keeping layer and object entities and the game's state on what the save left alone.

Details: `src/core/tilemap/AGENTS.md`; usage: the `ryo-tilemap` skill; measurements: `docs/backlog/tilemap.md`.

## UI

Built every frame through `UiContext`'s builders, laid out with flexbox and grid, and drawn as commands on the GPU (see [Text and UI](#text-and-ui)). It comes in two styles that share everything below the builders, and node trees can be written in files:

- **Closures.** `Scene::ui(ui, ctx)` calls the builders each frame (`ui.button("Play").clicked()`); the game's values stay in the game, passed as `&mut bool`, `&mut f32`, `&mut usize` or `&mut String`.
- **Node trees.** A `UiNode` tree is described once and spawned as entities (`ctx.spawn_ui`). Each frame the engine walks a scene's trees through the same builders, keyed by entity. A node's state is its components (`UiLabel`, `UiChecked`, `UiValue`, ...), which the game writes and the walk writes back when the player changes them. What the player did arrives as `UiEvent` messages, also triggered on the node for observers. A root belongs to the scene that spawned it and is despawned when the scene exits.

- **UI files.** A node tree can be written in a `*.ui.ron` file (`src/core/ui/file/`) that loads into the same `UiNode`. A reader of RON syntax of our own (it knows every value's line and column) feeds one vocabulary table that maps kinds and options onto `UiNode`'s constructors and methods, so a file says nothing a `UiNode` cannot. Files have templates with parameters (used from files and from code), imports, and named bindings: `{score}` in text and `bind: "volume"` on value widgets, set by name with `ctx.ui_set`, writing the same state components. A file is a registry asset (`UiFile`, loaded by `UiFileLoader` with its imports and images as dependencies), checked when it loads, and every mistake is reported with line, column and a suggested name. Each built node remembers its source. `ctx.update_ui` and hot reload are one operation, the reconcile (`nodes/reconcile.rs`): children match by key, matched nodes keep their entities (so ids, focus and widget state survive), and the game's and the player's values survive unless the new tree changes that same value. When a file or one it imports reloads (hot reload is on in debug builds with a window), the trees built from it are brought in line before the frame's UI pass and `UiReloaded` is sent. `ryo2d check-ui` checks files without a game.

They meet in two ways. A scene's trees are drawn before its `ui` closure, in the same scene scope, so the closure draws above them and shares their focus, layout memory and hit tests. A closure can also embed a tree spawned with `.embedded()` inside one of its containers (`ui.node(root, world)`), so one focus path runs through both.

**The frame.**

```
resolve (once a frame, before any ui hook, against last frame's records)
  pointer: hit test (topmost by layer, then order; top scene only) -> hot, active, clicks, drags, drops
  wheel:   the scroll areas under the pointer, innermost first; over any UI, never gameplay's
  focus:   the ui.* actions -> spatial navigation inside focus scopes, Confirm, Back, tab switching
  lists:   each node list's pool follows the scroll; a row given an item sends UiListBind
     |
hooks (each visible scene bottom-up, under its own root id scope)
  the scene's node trees are walked: unchanged nodes replay their last build, the rest
  run the builders; their UiEvents are delivered
  then Scene::ui runs its builders (and draws embedded trees with ui.node)
  builders record nodes, draws and records; each answers from the resolve (Response)
  each top-level container is laid out by Taffy when its show() returns; popups when the scene's ui ends
     |
commands (UiCommand per draw, ordered by layer then recording order) -> UI renderer -> UI image -> composite
```

- **Clicks land on what was drawn.** Hit testing and navigation read the records the last frame wrote (rect, clip, layer, order, scene, flags, focus scope, scroll area), so a click goes to what the player saw under the pointer, even if the widget moves or disappears this frame. Only the top scene's records are candidates: lower scenes draw, and their widgets report nothing.
- **Ids.** A widget's `UiId` is a hash of its enclosing scope, its call site (`#[track_caller]`) and how often that site ran in the scope this frame; `.id(key)` replaces the count for lists that reorder, and `ui.scope(key, ..)` keys a group without a node. A node is keyed by its entity, so its state follows it when siblings change. Each scene is a root scope, so the same code in two scenes makes two ids. The hash folds a word at a time; ids live only in memory. The same fast hasher (`core::hash`) keys the UI's and the text engine's maps: their keys are the engine's own data, so SipHash's flood resistance would buy nothing.
- **Memory.** `UiMemory` holds last frame's and this frame's records, typed widget state (an open list, a caret, a scroll offset, a tooltip timer) keyed by id and dropped when a widget is not drawn for a frame, and focus memory per scope for the scene's lifetime; a scene's entries are forgotten when it exits. Interaction state (hot, active, focused, drag) lives beside it.
- **Layout.** `src/core/ui/layout/` is the only code that names Taffy: a persistent tree keyed by `UiId`, text measured through the text engine's cache. A top-level subtree is placed by its anchor and offset in the UI (or at its `at` rect), and its rects are snapped to render pixels. Only a node whose style hash changed is sent to Taffy again, and the buffers a frame uses are kept between frames. A 200-widget settings screen is a frame of under 60 µs, and the same screen as a node tree within the larger of 10% or 10 µs of it (`benches/ui.rs`).
- **Cached node trees.** A node tree's frame is incremental, as retained UI is in Unity's UI Toolkit, Godot or Flutter: each node keeps a record of what its builder pushed (a tape of nodes and draws, with a slot where each child's output goes), and a node whose inputs did not change replays it instead of building. A node rebuilds, with the path from its root but never its siblings, when its components change (by change ticks, so `ui_set`, `update_ui` and reloads count and a `bypass_change_detection` write does not), when it holds an interaction role this frame or the last, while it is live (a scroll area, a list, an unfinished typewriter), when something it read elsewhere moved (a neighbour's id, an anchor's rect, a sprite's size), or when the environment changes (theme, fonts, UI scale and size, prompts, the top scene). A replay is stamped where it lands exactly as a build is, so its records and commands are a build's and caching is not observable: a differential oracle runs every scenario with the cache on and off and compares each frame, and debug builds spot-check one record a frame. The records live in the `UiNodes` resource, never on the game's entities. `debug:ui-rebuilds` outlines what rebuilt, and `ui:cache off` (or `EngineBuilder::ui_cache(false)`) turns caching off. Closure UI stays immediate.
- **Fragments and scroll slots.** Each outermost clean subtree of a node tree is a fragment: it keeps its layout in the layout engine and its resolved records and commands, relative to its scroll slot, and a later frame copies them, re-based to where its orders and slot are now, instead of replaying, laying out and resolving it again; when something it was resolved from changed (where its root landed, its clip, its layer, a nine-slice's sprite), it is resolved again from what it kept. So a frame costs about what changed: the unchanged settings screen as nodes is guarded under 15 µs a frame and a 2,000-node still tree under 100 µs (`benches/ui.rs`). A scroll area draws its contents at its offset rounded to whole render pixels, placed relative to the area's slot, so scrolling moves the slot and leaves what it shows unchanged; records stay absolute for hit tests. The UI renderer keeps each fragment run's instances and copies them while the run is the same, and its shader moves a slot's instances by the slot's offset. Kept or rebuilt, the output is the same: the oracles run fragments on against the node cache off, records and commands frame by frame, and pixels on every ui demo page and breakout's screens. `ui:fragments off` (or `EngineBuilder::ui_fragments(false)`) turns them off.
- **Virtual lists.** `ui.list(count)` and the node `UiNode::list(count, row_height)` (`List` in files) are scroll areas of an exact length, `rows × (row_height + gap) − gap`, that build only the rows in view and an overscan either side, between spacers, so a 10,000-row list costs what a screenful does. Both place their rows through one helper (`widgets/list.rs`), each item in a container keyed by its index, so focus and widget state follow the index. A node list keeps about a screenful of row entities and reuses them as it scrolls (the model/view pool of Unity's and Unreal's ListView or Android's RecyclerView): once a frame, before any tree is drawn, it gives rows the items that came into view and sends `UiListBind` for each, and the game fills the row (each row is its own binding scope) before it is drawn the next frame. The focused row stays built wherever the list scrolls, so the pad walks the whole list, and a jump (or any scroll heading more than two views away) moves to one view short of where it is going and eases the rest. A list needs a height: one given none grows to its whole length, builds only the rows that show and warns.
- **Focus scopes.** Keyboard and pad navigation, Tab and Confirm work only inside explicit scopes (`.focus_scope()` containers, open lists, popups and modals), so a HUD never takes the arrows, Space or A from gameplay. A scope focuses its remembered or default widget when it appears; traps (modals, lists, `.trap()`) hold the focus until they close and give it back. Directions move spatially to the nearest focusable, scrolling long lists; Back is offered to the focused widget, then to the first `ui.back()` caller.
- **The theme** is the `UiTheme` world resource holding an `Arc<Theme>` (`ctx.ui_theme()`): a palette, metrics (timings, sizes, the focus ring), text roles, a look per widget kind and state, named variants, nine-slice skins and sounds; `.theme.ron` files are registry assets merged over the dark theme, and `ctx.follow_ui_theme(handle)` uses one at once and again whenever it reloads. `UiContext` takes the `Arc` once per pass, so a change applies from the next frame.
- **Actions.** Navigation reads the `ui.*` actions (arrows, D-pad, left stick; Enter, Space, A; Escape, B; Tab; Q/E and the bumpers). The engine hides the bindings of every action the UI used until they are released, so a menu's Confirm never jumps in gameplay. Prompts (`ui.prompt`, `[prompt=..]` markup) show the binding for the last device.
- **Absolute drawing** (`draw_rect`, `draw_text`, `ui.text(..).at(..)`, sprites, nine-slices, lines) records at once for HUDs and overlays; a clip stack of any depth clips it (a clip rect per command, tested in the UI shader).
- **Testing.** `ryo2d::testing::UiHarness` runs UI frames with no window or GPU over scripted input, node trees and UI files included (`spawn_ui`, `spawn_ui_file`, `reload_ui`, `events`); `HeadlessEngine` adds pad input and reads the focus (`ui_focused`, `ui_focusables`). The console's `debug:ui` outlines every record with its id.

Details: `src/core/ui/AGENTS.md`; the designs: `docs/specs/2026-10-16-ui-interaction-design.md` (interaction); `docs/specs/2026-10-17-ui-nodes-design.md` (node trees), `docs/specs/2026-10-17-ui-files-design.md` (files), `docs/specs/2026-10-18-ui-fast-design.md` (the cheap frame and virtual lists), `docs/specs/2026-10-18-ui-cache-design.md` (cached node trees) and `docs/specs/2026-10-21-ui-fragments-design.md` (fragments and scroll slots).

## Saves and Settings

| What | File | API |
|------|------|-----|
| Player settings (resolution, display mode, vsync, volumes, FPS, console, input bindings) | `system.json`, uncompressed JSON | `SystemState`, `UserPreference`, `EngineBuilder::with_user_preferences` |
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
| `ryo2d check-ui [files]` | Check UI files (`*.ui.ron`) without running the game |
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
World space (world units)                Screen space
a camera at the centre of its view       (0,0) at the top-left

          ▲ -Y                                0 ─────────────► X
          │                                   │
◄─────────┼─────────► +X                      │   (through the camera's view)
          │                                   │
          ▼ +Y                                ▼ Y
```

- +Y is down in both; rotation is clockwise. The main camera starts at the centre of the design area, so world (0, 0) is its top-left and one world unit is one design pixel. A sprite's `Anchor` (default `Center`) is the point placed at its entity's position; rotation and scale happen about it. Tiled runs and composite tiles are laid out from the position as their first tile's top-left.
- Screen space is the window in logical pixels (resolution-independent) or the pixel-art target; the UI and the mouse live there. `CameraView::world_to_screen` / `screen_to_world` convert, shake and rotation included.
- Physics positions are world units (pixels); `PhysicsConfig::pixels_per_meter` of them make a Rapier metre (default 100).

## Networking

Client-server multiplayer with listen-server support. See [the network architecture](./network.md).

- **Transport:** UDP via renet (unreliable, reliable ordered, reliable unordered channels); WebRTC through matchbox with the `matchbox` feature.
- **Servers:** `NetworkServer` (dedicated), `ListenServer` (player-hosted).
- **Replication:** `NetworkEntity`, `Authority`, `DirtyTracker`, `ReplicationManager`; **RPC:** `RpcRegistry`; **prediction:** `Predicted`, `PredictionHelper`.

## Modules

| Module | Purpose |
|--------|---------|
| `ecs/` | Kernel re-exports, `Transform`, propagation, interpolation, pooling, `Renderable`, juice components |
| `engine/`, `scene/`, `schedule.rs`, `time.rs`, `input/`, `messages/` | The loop, scenes, phases, clocks, input and actions, messages |
| `systems/` | The engine's built-in schedule systems |
| `graphics/` | Images and sprites, animation, cameras and shake, render targets, render layers, the screen mapping, parallax, tiled and composite rendering, the texture registry and atlas, the sprite renderer |
| `render/` | The render pipeline, passes, the draw list, the frame's camera plan, the 2D batch (`batch/`: keys, planning, pipelines, polylines) and debug drawing |
| `shaders/` | The engine's WGSL and its shared includes, post-process effects and custom shaders |
| `lighting/`, `trail/`, `particles/`, `skeleton/` | Feature renderers (lighting) and components (trails and lines, particles, skeletons) |
| `tilemap/` | Tile maps: the data model, the Tiled and LDtk readers, maps spawned as entities and landed off the main thread, reads and edits with Wang terrain, collision and occluders kept in step with the tiles, worlds, hot reload; drawn by `render/tiles.rs` |
| `physics/` | Rapier integration, colliders, bodies, joints, the character controller, one-way colliders, triggers, tile colliders |
| `gameplay/` | Movement input and presets, knockback, trigger behaviours |
| `navigation/` | Grids, A* pathfinding, agents, steering |
| `state_machine/` | Generic state machines and stacks |
| `asset/` | The asset registry (handles, stores, the server, loaders, worker threads, the pump, hot reload) |
| `audio/` | The audio engine: the device or mock backend, the bus tree and mix, voices and limits, spatial sound, music, the streamed clips' decoder |
| `text/` | Fonts, text styles, rich text and markup, layout, glyph rasterization, effects and reveal, world text (`Text2d`), `CpuText` for the dev windows |
| `ui/` | `UiContext` and absolute drawing, ids, memory, layout (Taffy), containers and widgets, popups and modals, the pointer and focus resolve, the theme, UI tweens |
| `config/` | Workspace (`Ryo.toml`), settings, saves |
| `console/`, `windows/`, `capture/`, `dev_log.rs`, `profiler.rs` | Developer tools |
| `window.rs`, `cursor.rs` | Window and cursor requests |
| `network/` | Networking |
| `testing/` | `HeadlessEngine`, `UiHarness` and the shared GPU for tests |

## Key Dependencies

| Crate | Purpose |
|-------|---------|
| `bevy_ecs` | The ECS: storage, queries, schedules, change detection, hierarchy, messages |
| `glam` | Math types (`Vec2`, `Affine2`, `Mat4`) |
| `wgpu` | GPU abstraction for rendering |
| `winit` | Windowing and input (also on the web) |
| `web-time` | Clocks on native and the web |
| `rapier2d` | Physics simulation |
| `kira` | Audio: the mixer, tracks, effects, the device (through cpal) and the mock backend |
| `symphonia` | Decoding streamed clips (the engine's own kira decoder, `src/core/audio/stream.rs`; kira's version and formats) |
| `gilrs` | Gamepads |
| `renet` | Networking transport |
| `puffin` | Profiling (optional) |
| `parley`, `swash`, `fdsm` | Text layout, glyph rasterization and MSDF generation (named only in `src/core/text/`) |
| `taffy` | UI layout: flexbox and grid (named only in `src/core/ui/layout/`) |
| `image`, `resvg` | Decoding images and rasterising SVG (the image loader) |
| `notify` | Watching the asset folder (hot reload; the debounce is the engine's own) |
| `ron`, `serde_json`, `flate2`, `toml` | Saves, settings and configuration |
| `arboard` | Clipboard |
