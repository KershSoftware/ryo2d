---
name: ryo-engine-dev
description: Working on the Ryo2D engine - the frame's phases, module map, resources, public API, crates and testing.
metadata:
  version: 2.1.0
---

# Ryo2D Engine Developer

You help developers work on the Ryo2D engine itself (games use `ryo-game-dev`).

## Read First

| Doc | Covers |
|-----|--------|
| `AGENTS.md` (root) | Workspace layout, dev scripts, conventions, commit rules |
| `src/core/AGENTS.md` | Cross-cutting design, time, input, cursor and window requests |
| `src/core/ecs/AGENTS.md` | Phases and sets, resource-scope rule, transforms, interpolation, pooling, messages |
| `src/core/systems/AGENTS.md` | Built-in systems and how to add one |
| `src/core/*/AGENTS.md` | Each module's conventions and decisions (read the nearest one) |
| `src/cli/AGENTS.md` | The `ryo2d` command and the embedded skills |
| `docs/architecture/overview.md` | The big picture |
| Skills `ryo-ecs-internals`, `ryo-renderer`, `ryo-api` | ECS internals, the renderer, public signatures |

## The Frame

`Engine::frame` (`engine/frame.rs`) runs once per redraw, and on the unfocused tick at the background rate. Windowed and headless runs use the same function.

```
Engine::frame
│
├─ queue input        RYO_INPUT script, gamepads (gilrs), window events already queued
├─ First              advance the clocks · publish Input · advance every message buffer
│                     then the engine's debug keys consume their presses
├─ PreUpdate          async asset loads, hot reload (AssetReloaded / AssetLoadFailed)
│                     then the console (animation, input, commands)
├─ UI pass            resolve first, once a frame, against last frame's UI records: the
│                     pointer, the wheel, then focus and navigation from the ui.* actions;
│                     then Scene::ui for visible scenes bottom-up (each top-level container
│                     laid out by Taffy when its show() returns, popups at the scene's end),
│                     debug overlays, console; consumption and used actions applied to Input;
│                     a SceneAction applies at once (a scene change redraws the UI once)
├─ FixedUpdate × N    while the fixed clock holds a step; a quit ends the steps
│   ├─ Snapshot       debug shapes of the last step cleared; GlobalTransform → PreviousGlobalTransform
│   ├─ Juice          animation, tweens, sequences, property tweens, squash, skeletons
│   ├─ Scene          transitions, Scene::update (active scene and those under updates_below), action
│   ├─ Game           the scenes' own FixedUpdate schedules; builder systems by default
│   ├─ Movement       trigger behaviours, tile collider rebuilds, movement input, knockback, movement presets
│   ├─ Physics        one system running PhysicsSchedule: forget, track disabled, propagate, register,
│   │                 sync colliders, push, move characters, sync joints, step, read back, messages,
│   │                 trigger contents
│   ├─ Transforms     propagation
│   └─ Trails         trail point recording; later debug shapes last one frame
├─ Update             UpdateSet::Game: the scenes' own Update schedules; builder systems by default
├─ PostUpdate         Transforms (propagation) → Camera (shake on real time, every CameraView)
│                     → Extract (parallax auto-scroll, timed effects, text reveals)
├─ render             present_frame (engine/extract.rs): Scene::render (GPU batches), queries
│                     the world into the draw list (world space, sorted once: sprites, entities,
│                     trails and lines as polylines, world text; parallax and debug shapes per camera),
│                     the UI: UI commands, then the UI camera through the batch, then the UI image,
│                     then the composite,
│                     plans the cameras, lays each camera's items into the 2D batch's draws
│                     (Batch2D::prepare, uploaded once), runs the RenderPipeline:
│                     per camera: clear → world → lighting → unlit_world (Rgba16Float, linear)
│                     per frame:  cameras → hdr_effects → tonemap → transition → post_process
│                                 → ui → composite → capture   (tonemap writes the display image)
└─ Last               physics removal and re-enable bookkeeping, forget despawned tilemaps, clear_trackers,
                      check_change_ticks every 1024 frames
```

- Phases are `bevy_ecs` schedules owned by `EngineSchedules` (`schedule.rs`); sets are `FixedSet`, `UpdateSet`, `PostUpdateSet`. Every schedule uses the single-threaded executor.
- The UI pass and render are not schedules; they are engine methods between phases.
- Engine systems are registered in `register_engine_systems` (`engine/mod.rs`), `register_juice_systems` (`systems/mod.rs`), `register_physics_systems` (`physics/sync.rs`) and `register_gameplay_systems` (`gameplay/mod.rs`). Games add theirs with `EngineBuilder::add_systems` or `SceneContext::add_systems`.
- Three clocks (`time.rs`): `real` (wall), `game` (capped, scaled, pausable), `fixed` (constant steps fed by game time). Fixed-step code uses `time.fixed.timestep()`; real-time effects use `time.real`. Sprite shader globals carry game time in `x`.

## World and Engine State

| In the world (resources) | Engine-owned (fields of `Engine`) |
|--------------------------|-----------------------------------|
| `Time`, `Input`, `ScreenMapping`, `AssetManager`, `EffectsManager`, `LightingSettings`, `Look`, `TilemapManager`, `WindowRequest`, `CursorRequest`, `WindowInfo`, `RenderPipeline`, `RenderTargets`, `Fonts`, `TextIcons`, `TextEffects`, `Gpu` (after GPU init), `PhysicsPaused`, `PhysicsConfig`, `Physics`, `CommandRegistry`, `FallbackErrorHandler`, `Messages<T>` per type | The window, the wgpu renderer (with the camera images), the sprite textures, the 2D batch and the lighting renderer, the camera uniforms, the draw list, the text engine (layout cache) and the frame's UI commands, console state, debug config and overlays, dev windows, gilrs, capture state |
| Non-send `SceneHost` (scene stack + Kira `Audio`), taken out by the scene systems while they run | |

- `init_world_resources` (`engine/mod.rs`) inserts the resources when the engine is built, windowed or headless; `Gpu` follows in `init_gpu`.
- Scene callbacks get `Time`, `Input` and `AssetManager` as `SceneContext` fields, taken out of the world for the callback (`SceneContext::scoped`). Systems always see them as resources. Cameras are entities (`Camera2d`); the main one (`MainCamera`) is spawned with the world and survives world cleanup, and `ctx.camera()` edits it.
- Per-frame requests from scenes to the window go through resources (`WindowRequest`, `CursorRequest`) applied in `about_to_wait` (`engine/window.rs`), never by borrowing the window during a callback.

## Module Map (`src/core/`)

| Module | Purpose |
|--------|---------|
| `engine/` | `Engine`, `EngineBuilder`, `EngineConfig`; `frame.rs` the loop, `extract.rs` world-to-draw-list and present, `window.rs` the winit handler |
| `schedule.rs` | `Phase`, the sets, `EngineSchedules`, `SceneSchedules` |
| `time.rs` | `Time` and the three clocks |
| `input/`, `input_script.rs` | `Input` rebuilt each frame from `InputEvent`s (virtual pad events included), the last-used device; `actions.rs` named actions with bindings, groups, rebinding with swaps and saved overrides; `labels.rs` prompt labels and `PadFamily`; `RYO_INPUT` scripting (keys, mouse, wheel, pad buttons, sticks) |
| `ecs/` | Kernel re-exports, `Transform`/`GlobalTransform`, propagation, interpolation, pooling, the engine's components |
| `systems/` | The built-in systems (juice, physics, trails) |
| `messages/` | Engine message types, registration, per-scene cursors |
| `scene/` | `Scene`, `SceneAction`, `SceneContext`, `SceneManager`, cleanup |
| `render/` | `RenderPipeline` of `RenderPass`es (camera and frame scopes), the draw list, entity extraction, the frame's camera plan, the 2D batch (`batch/`: keys, planning, pipelines, polylines), `DebugDraw` |
| `graphics/` | wgpu renderer, sprite textures and game shader sources, cameras and render targets, the screen mapping, texture registry and mipmapped atlas, bind-group helper, compositor, sprite animation, tiles, `ParallaxLayer` |
| `shaders/` | Post-process effects (`EffectsManager`) and sprite shaders |
| `lighting/` | 2D lights, occluders, `LightingSettings` |
| `physics/` | Rapier integration, `RigidBody`, `Collider`, `Physics`, the character controller, one-way colliders, triggers, tile colliders |
| `gameplay/` | On top of physics: `MoveIntent` and bindings, the platformer and top-down presets, knockback, trigger behaviours (`Pickup`, `KillZone`, `Checkpoint`, `Respawn`) |
| `particles/`, `trail/`, `skeleton/`, `tilemap/`, `navigation/`, `state_machine/` | Feature modules |
| `text/` | Fonts (`Fonts`), `TextStyle`, rich text and markup, layout (Parley), glyph rasterization (swash, fdsm), effects, reveal, `Text2d` and `CpuText` for the dev windows; GPU-free |
| `ui/` | `UiContext` and absolute drawing (`immediate/`); ids (`id.rs`), `UiMemory` (records, widget state, focus memory), layout over Taffy (`layout/`, the only Taffy code), the subtree and its resolution (`tree.rs`), containers, widgets (`widgets/`), popups and modals, the pointer and wheel resolve (`interact.rs`), focus and navigation (`nav.rs`), the theme (`theme/`), the `debug:ui` overlay |
| `asset/` | `AssetManager`, async loads, hot reload |
| `audio.rs` | Kira audio (`Audio`, `AudioManager`) |
| `config/` | `Ryo.toml` workspace, user preferences (`system.json`), saves |
| `console/` | Developer console and commands |
| `capture/` | Frame capture to PNG (`RYO_CAPTURE_*`) |
| `testing/` | `HeadlessEngine` (pad, wheel and UI-focus helpers), `UiHarness` (UI frames without a GPU), `shared_gpu` |
| `network/` | Client/server, replication, RPC (`ryo-network`) |
| `windows/` | Dev tool windows (profiler, logger); they draw `CpuText` (JetBrains Mono through the text module), with no hand-made font |
| `window.rs`, `cursor.rs` | Window modes and requests, cursor requests |
| `profiler.rs`, `dev_log.rs`, `error.rs` | Puffin macros, logging, error types |

## Workspace

| Path | Rule |
|------|------|
| `src/lib.rs` | The prelude is the public surface; games use only `ryo2d::prelude::*` (plus `ryo2d::testing`) |
| `crates/ryo2d-derive` | `#[derive(NetworkInput)]`, re-exported by `ryo2d` |
| `crates/ryo2d-procgen`, `crates/ryo2d-inventory` | Optional crates on top of `ryo2d`, each with its own prelude and AGENTS.md |
| `attic/` | Parked code outside the workspace, not compiled (the raycaster and its demo) |
| `examples/demo-kit`, `examples/demos/*`, `examples/games/*` | The demo shell, feature demos, sample games |

Dependencies point toward the core: nothing in `src/` uses a crate in `crates/`, and the `ryo2d` prelude exports the core only. A feature that only some games need belongs in a crate.

## Public API

- Export from the prelude what games spawn, query, implement or store: components, `Scene`, handles, builders, message types. ECS items arrive through the `crate::core::ecs::*` glob, so anything `pub` in `ecs/mod.rs` is public.
- Keep engine plumbing out: system functions, `SceneManager`, renderers, registration helpers.
- Games never name `bevy_ecs`; it is re-exported (hidden) only so the derives resolve. New kernel types go through `ecs/kernel.rs`.
- Rustdoc every public item; it is the API reference. The `ryo-api` skill mirrors the prelude.
- No `unwrap()` in library code; return `Result` with the module's error type (`error.rs` re-exports them).

## Adding a Component or System

See `ryo-ecs-internals` and `src/core/systems/AGENTS.md`. In short: derive `Component`, export it, write a system over `Query`/`Res`, put it in a set, test it through a `Schedule` with `systems::test_util`.

## Testing

| Layer | Where | Run |
|-------|-------|-----|
| Unit | beside the code (`#[cfg(test)]`) | `cargo test --lib` |
| Integration | `tests/integration/` (target `integration`) | `cargo test --test integration` |
| End-to-end, GPU | `tests/e2e/` with `HeadlessEngine` (target `e2e`) | `cargo test --test e2e`, `./dev.sh test:e2e` |
| Demo tour | every page of every demo, headless | `./dev.sh demos:tour [demo...]` → `target/demo-tour/<demo>/page-N.png` |
| Benchmarks | `benches/` (criterion): `foundation`, `physics`, `text` (the HUD frame, GPU), `ui` (a 200-widget settings screen under 60 µs and its node version within the larger of 10% or 10 µs of it, a 1,000-row list, an empty frame; no GPU) | `cargo bench --bench ui` |

- Rendering regressions: write the failing pixel assertion in `tests/e2e/` first (see `src/core/testing/AGENTS.md`).
- UI behaviour: unit tests drive frames with `ui::test_util::UiDriver` (a `UiHarness` plus keys, text, pad buttons, held directions and scenes); games use the public `ryo2d::testing::UiHarness`.
- `cargo test` takes one name filter; several go after `--`: `cargo test --lib -- core::scene core::audio`.
- Verify a rendering change from captures, not guesses: `RYO_CAPTURE_FRAMES=<n> RYO_CAPTURE_EXIT=1 RYO_CAPTURE_DIR=<abs dir>`, plus `RYO_HEADLESS=1` (no window) and `RYO_INPUT="60:Digit2 120:ArrowRight"` to reach pages behind keys (`ryo-cli`).

## Dev Scripts

```bash
./dev.sh check            # engine + every example
./dev.sh lint             # clippy, tests included
./dev.sh test             # unit + integration (test:unit, test:integration, test:e2e, test:all)
./dev.sh <example>        # run an example (-d dev mode, -p profiler, -r release)
./dev.sh demos:tour       # capture every demo page
./dev.sh coverage         # coverage reports
```

`./dev.ps1` is the Windows twin. Before finishing: `cargo check`, `cargo clippy`, `cargo test`, and the e2e suite for rendering changes.

## Profiling

- `profile_scope!("name")` / `profile_function!()` (`profiler.rs`, `use crate::profile_scope;` inside the engine); no-ops without the `profiler` feature.
- The frame records `cpu:update`, `cpu:render` and `gpu:present`; add scopes inside systems that may be slow.
- `windows/profiler_window.rs` is a separate CPU-drawn flamegraph window (a puffin sink, throttled to 10 FPS, bars fade instead of flickering). Run with `./dev.sh <example> -p` or `ryo2d dev --profiler`. More in `ryo-profiler`.

## Documentation Homes

| What | Where |
|------|-------|
| API | Rustdoc on public items |
| Using the engine | Skills in `.claude/skills/` (the CLI embeds copies from `src/cli/skills/`; keep both in step) |
| Module conventions and decisions | The nearest `AGENTS.md` |
| The big picture | `docs/architecture/` |
| Designs and their reasons | `docs/specs/` |

## Demos

Feature demos share one shell (keys, settings panel, layout) through `examples/demo-kit`; see `examples/demos/AGENTS.md`.
