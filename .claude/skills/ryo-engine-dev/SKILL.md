---
name: ryo-engine-dev
description: Working on the Ryo2D engine - the frame's phases, module map, resources, public API, crates, the dev_tools build condition, logging in engine code, profiling scopes and testing (the build without tools included).
metadata:
  version: 2.9.0
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
├─ PreUpdate          a running scene transition, then the asset pump, chained: finished loads
│                     and reloads land, code changes' events are sent, unheld assets are freed
│                     (AssetEvent<T>); the font registry follows the font stores; then tile maps:
│                     spawn_maps (ready maps' landings sent to the map builder thread, finished
│                     ones landed whole, MapEvent), reload_maps (spawned maps follow saved
│                     files), stream_maps (streamed maps unload chunks far from every anchor,
│                     ask the chunk builder thread for the missing ones near them and land
│                     those back, nearest first, ChunkEvent) and follow_chunk_swaps (class
│                     tiles follow the chunks loaded and unloaded whole); beside them
│                     drain_save_results (finished saves as SaveFinished, the settings
│                     file's results to its writer, SettingsSaveFailed); then the console (a dev
│                     tool; while open: the log's new records, animation, input, commands)
├─ UI pass            resolve first, once a frame, against last frame's UI records: the
│                     pointer, the wheel, then focus and navigation from the ui.* actions;
│                     then each visible scene bottom-up: its node trees (unchanged subtrees
│                     copied, unchanged nodes replayed), then Scene::ui (each top-level container
│                     laid out by Taffy when its show() returns, popups at the scene's end),
│                     debug overlays, console; consumption and used actions applied to Input;
│                     a SceneAction applies at once (a scene change redraws the UI once)
├─ FixedUpdate × N    while the fixed clock holds a step; a quit ends the steps
│   ├─ Snapshot       debug shapes of the last step cleared; GlobalTransform → PreviousGlobalTransform
│   ├─ Juice          animation, tweens, sequences, property tweens, squash
│   ├─ Scene          transitions, Scene::update (active scene and those under updates_below), action
│   ├─ Game           the scenes' own FixedUpdate schedules; builder systems by default
│   ├─ Movement       trigger behaviours, tile maps' collision sync, tile collider rebuilds, movement input,
│   │                 knockback, movement presets
│   ├─ Physics        one system running PhysicsSchedule: forget, track disabled, propagate, register,
│   │                 sync colliders, push, move characters, sync joints, step, read back, messages,
│   │                 trigger contents
│   ├─ Transforms     propagation, then the tile maps' floor lift
│   ├─ Skeletons      update_skeletons: each skeleton whose data is loaded, in entity order: its pose made
│   │                 anew for new data, its animation state advanced and applied over the setup pose,
│   │                 the game's attachments and bone overrides written over that, its bones placed under
│   │                 its entity's GlobalTransform, its sockets and what is below them placed, then its
│   │                 SkeletonEvents and SkeletonTrackEvents written
│   ├─ Particles      step_particles: game-time emitters spawn, their particles move and die
│   └─ Trails         trail point recording; later debug shapes last one frame
├─ settings           Engine::apply_settings: a changed Settings goes to the mixer, the window
│                     request, vsync and the FPS overlay; Engine::save_settings: the changed
│                     bindings, then the settings writer's debounced system.json write
├─ Update             UpdateSet::Game: the scenes' own Update schedules; builder systems by default
├─ PostUpdate         tile maps' occluder sync, then Transforms (propagation), then pose_new_skeletons
│                     (a skeleton added this frame, or whose data landed or reloaded in it, is posed
│                     where it stands and its sockets placed; a steady frame looks through none)
│                     → Camera (shake on real time, every CameraView)
│                     → Extract (parallax auto-scroll, timed effects, text reveals, unscaled particle
│                     emitters on real time); after Camera,
│                     audio_system: voice commands, mixer rebuilds, reloads, music, the listener
│                     and emitters, the game clock's holds and rates, the mock backend's render
├─ render             present_frame (engine/extract.rs): Scene::render (GPU batches), queries
│                     the world into the draw list (world space, sorted once: sprites, particle runs, entities,
│                     tile maps' visible chunks, built into the batch's chunk store when changed,
│                     trails and lines as polylines, world text; parallax and debug shapes per camera),
│                     the UI: UI commands, then the UI renderer (its own instances), then the UI image,
│                     then the composite,
│                     plans the cameras, lays each camera's items into the 2D batch's draws
│                     (Batch2D::prepare, uploaded once), runs the RenderPipeline:
│                     per camera: clear → world → lighting → unlit_world (Rgba16Float, linear)
│                     per frame:  cameras → hdr_effects → tonemap → transition → post_process
│                                 → ui → composite → capture   (tonemap writes the display image)
└─ Last               physics removal and re-enable bookkeeping, clear_trackers,
                      check_change_ticks every 1024 frames
```

- Phases are `bevy_ecs` schedules owned by `EngineSchedules` (`schedule.rs`); sets are `FixedSet`, `UpdateSet`, `PostUpdateSet`. Every schedule uses the single-threaded executor.
- The UI pass and render are not schedules; they are engine methods between phases.
- Engine systems are registered in `register_engine_systems` (`engine/mod.rs`), `register_juice_systems` (`systems/mod.rs`), `register_physics_systems` (`physics/sync.rs`), `register_gameplay_systems` (`gameplay/mod.rs`) and `register_tilemap_systems` (`tilemap/mod.rs`). Games add theirs with `EngineBuilder::add_systems` or `SceneContext::add_systems`.
- Three clocks (`time.rs`): `real` (wall), `game` (capped, scaled, pausable), `fixed` (constant steps fed by game time). Fixed-step code uses `time.fixed.timestep()`; real-time effects use `time.real`. Sprite shader globals carry game time in `x`.

## World and Engine State

| In the world (resources) | Engine-owned (fields of `Engine`) |
|--------------------------|-----------------------------------|
| `Time`, `Input`, `ScreenMapping`, `AssetServer` and one `Assets<T>` per asset type, `AudioEngine` (kira's manager, the buses and the voices; `ctx.audio()`), `ParticleStore` (the emitters' pools, the live count and the budget; `ctx.particles()`), `SkeletonStore` (crate-internal: which scenes' skeletons are held, and the numbers the step and the sockets' placings take), `SaveService` (the storage, the version chains and the save thread; `ctx.saves()`), `Settings` (the player's settings; `ctx.settings()`), `SettingsWriter` (when the game keeps settings), `EffectsManager`, `LightingSettings`, `Look`, `WindowRequest`, `CursorRequest`, `WindowInfo`, `RenderPipeline`, `RenderTargets`, `Fonts`, `TextIcons`, `TextEffects`, `Gpu` (after GPU init), `PhysicsPaused`, `PhysicsConfig`, `Physics`, `Console`, `ConsoleCommands` (with the dev tools), `FallbackErrorHandler`, `Messages<T>` per type | The window, the wgpu renderer (with the camera images), the sprite textures, the 2D batch and the lighting renderer, the camera uniforms, the draw list, the particle drawer, the tile drawer (`render/tiles.rs`; the padded tilesets it shares with map landings are the crate's `PaddedCache` resource), the text engine (layout cache), the UI's image sizes (`ImageSizes`, the image store's follower) and the frame's UI commands, the console's cursor override, debug config and overlays, dev windows, gilrs, capture state |
| Non-send `SceneHost` (the scene stack), taken out by the scene systems while they run | |

- `init_world_resources` (`engine/mod.rs`) inserts the resources when the engine is built, windowed or headless; `Gpu` follows in `init_gpu`.
- Scene callbacks get `Time` and `Input` as `SceneContext` fields, taken out of the world for the callback (`SceneContext::scoped`); `ctx.assets()` is an `AssetsView` borrowing the world (the `AssetServer` and the stores stay resources), and `ctx.audio()` an `AudioView` over the `AudioEngine` resource. Systems always see them as resources. Cameras are entities (`Camera2d`); the main one (`MainCamera`) is spawned with the world and survives world cleanup, and `ctx.camera()` edits it.
- Per-frame requests from scenes to the window go through resources applied in `about_to_wait` (`engine/window.rs`), never by borrowing the window during a callback: `WindowRequest` carries the game's UI scale and, crate-internally, the window size and display mode `Engine::apply_settings` takes from `Settings`; `CursorRequest` the cursor. The window's own changes (a resize, the OS's or F5's fullscreen) flow back into `Settings` through `Engine::set_display_mode` and `follow_os_window`, so nothing is asked of the window twice. The window's cursor is resolved there too (`CursorResolver`): the console's per-frame override (`Engine::console_cursor`, `None` = no opinion), else the game's last cursor, applied only when it changes and once more when the left button is released.

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
| `graphics/` | `Image`, the wgpu renderer, sprite textures (by image id) and game shader sources, cameras and render targets, the screen mapping, texture registry and mipmapped atlas, bind-group helper, compositor, sprite animation, tiles, `ParallaxLayer` |
| `shaders/` | Post-process effects (`EffectsManager`) and sprite shaders |
| `lighting/` | 2D lights, occluders, `LightingSettings` |
| `physics/` | Rapier integration, `RigidBody`, `Collider`, `Physics`, the character controller, one-way colliders, triggers, tile colliders |
| `gameplay/` | On top of physics: `MoveIntent` and bindings, the platformer and top-down presets, knockback, trigger behaviours (`Pickup`, `KillZone`, `Checkpoint`, `Respawn`) |
| `tilemap/` | Tile maps: the data model, the Tiled and LDtk readers (registry loaders), `MapRoot` and the landing on the map builder thread, `MapsView` (`ctx.maps()`) with edits and Wang terrain, map collision and occluders kept in step with the tiles, worlds, hot reload, padded tilesets; drawn by `render/tiles.rs`; `src/core/tilemap/AGENTS.md` |
| `skeleton/` | Skeletal animation: `SkeletonData` and its builder and validation, the animation state (tracks, mixing, queues, events), `Skeleton` with its skins, attachments and bone overrides, `BoneSocket`, the fixed step and the frame's pose pass, scene holds (`SkeletonStore`), the region quads the entity drawer pushes as runs, the `debug:skeletons` overlay; the `.skeleton.ron` loader is `asset/loaders/skeleton.rs`; `src/core/skeleton/AGENTS.md` |
| `particles/`, `trail/`, `navigation/`, `state_machine/` | Feature modules |
| `text/` | Fonts (`Fonts`), `TextStyle`, rich text and markup, layout (Parley), glyph rasterization (swash, fdsm), effects, reveal, `Text2d` and `CpuText` for the dev windows; GPU-free |
| `ui/` | `UiContext` and absolute drawing (`immediate/`); ids (`id.rs`), `UiMemory` (records, widget state, focus memory), layout over Taffy (`layout/`, the only Taffy code), the subtree and its resolution (`tree.rs`), containers, widgets (`widgets/`), popups and modals, the pointer and wheel resolve (`interact.rs`), focus and navigation (`nav.rs`), the theme (`theme/`), node trees (`nodes/`) and their cache (`nodes/cache.rs`, the records and the dirty set; `tape.rs`, what a build pushed and its replay; `fragment.rs`, the unchanged subtrees kept and copied), UI files (`file/`), virtual lists (`widgets/list.rs`, `nodes/list.rs`), the `debug:ui` and `debug:ui-rebuilds` overlays (`debug.rs`) |
| `hash.rs` | The engine's fast hasher (`FastHasher`, `FastMap`, `FastSet`, `fast_hash`) for keys it makes itself: ids, the UI's memory and layout maps, the text caches; never SipHash, never saved |
| `asset/` | The asset registry: `AssetId` and `Handle`, the `Assets<T>` stores, `AssetServer` (keys, loaders, sources, worker threads), the pump (`pump.rs`), the watcher and its debounce (`watch.rs`), the engine's loaders (`loaders/`); `src/core/asset/AGENTS.md` |
| `audio/` | The audio engine on kira: `AudioEngine` (device or mock backend, the voice table, limits, holds, reloads, the web's unlock), the bus tree and mixer, snapshots and ducks, spatial sound, the music player, the streamed clips' decoder (`stream.rs`), `audio_system`; `src/core/audio/AGENTS.md` |
| `config/` | The project: `Workspace` and `Ryo.toml` reading (misspelt keys warned), the project-settings table declared once (`project_table.rs`: Ryo.toml, `RYO_EMBED_*`, `src/build.rs`), `resolve_settings`, `ProjectFile` (`ryo2d::project!()`), the save location and the browser store's name, the preference filter; `src/core/config/AGENTS.md` |
| `saves/` | Game saves: `SavesView` / `Saves`, the save service and its writer thread, the checksummed save file, `SaveVersions`, the crash-safe replace and recover, the folder, browser (IndexedDB) and memory stores; `src/core/saves/AGENTS.md` |
| `settings/` | The player's settings: `Settings` and its game sections, `system.json` (format 3, older formats converted), the debounced `SettingsWriter`; `src/core/settings/AGENTS.md` |
| `diagnostics/` | The runtime `DebugConfig`, frame timings (`FrameCost`) and the F1 overlays' drawing; `src/core/diagnostics/AGENTS.md` |
| `console/` | Developer console and commands (a dev tool but `Command`, `CommandSpec` and `ConsoleScope`); `src/core/console/AGENTS.md` |
| `capture/` | Frame capture to PNG (`RYO_CAPTURE_*`) |
| `testing/` | `HeadlessEngine` (pad, wheel, trackpad and UI-focus helpers, its in-memory saves), `UiHarness` (UI frames without a GPU), `shared_gpu`, `MemorySaves` |
| `network/` | Client/server, replication, RPC (`ryo-network`) |
| `windows/` | Dev tool windows (profiler, logger; native dev tools); they draw `CpuText` (JetBrains Mono through the text module), with no hand-made font; `src/core/windows/AGENTS.md` |
| `logging/` | The log: the engine's `log::Log`, levels per target (`RYO_LOG`), stderr, the browser console, the log file and panic hook, the ring the dev tools read, `capture_log` for tests, the prelude's macros; `src/core/logging/AGENTS.md` |
| `window.rs`, `cursor.rs` | Window modes and requests, cursor requests |
| `profiler.rs`, `error.rs` | Puffin macros, error types |

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

- Export from the prelude what games spawn, query, implement or store: components, `Scene`, asset types and their loaders, builders, message types.
- Engine code that keeps something built from an asset keys it by `AssetId` (and the store's version), never by a `Handle`: only handles keep assets alive, so a cache holding one keeps what the game let go. A new engine asset type is registered with its loader in `insert_registry` (`asset/mod.rs`). How to add one: `src/core/asset/AGENTS.md`. ECS items arrive through the `crate::core::ecs::*` glob, so anything `pub` in `ecs/mod.rs` is public.
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
| Benchmarks | `benches/` (criterion): `foundation`, `physics`, `text` (the HUD frame, GPU), `ui` (a 200-widget settings screen under 60 µs and its node version within the larger of 10% or 10 µs of it, the node version unchanged under 35 µs, a 2,000-node static tree, an embedded node tree, the node cache's dirty check, a 1,000-row list, 10,000- and 100-row virtual lists and a 10,000-row node list, an empty frame; no GPU), `ui_render`, `assets` (a scene's start with `load_now` and with `load`, and a guard on the frame time of large hot reloads), `audio` (guards on 64 voices' main-thread time, 256 virtual loops ranked against 64 heard, `play`, a streamed `play` and reload landings, in an audio world; 1,024 virtual loops printed beside them), `tilemap` (GPU: guards on a static 1024 x 1024 map's frame, a one-tile edit's chunk, loop and occluder rebuilds, and a 256 x 256 map's load and landing; the record is `docs/backlog/tilemap.md`) | `cargo bench --bench ui` |

- Rendering regressions: write the failing pixel assertion in `tests/e2e/` first (see `src/core/testing/AGENTS.md`).
- UI behaviour: unit tests drive frames with `ui::test_util::UiDriver` (a `UiHarness` plus keys, pad buttons, held directions and scenes); games use the public `ryo2d::testing::UiHarness`.
- Node tree caching: the on/off oracle (`src/core/ui/nodes/oracle_tests.rs`) is the test of record. A change to what a node's build reads or does needs a scenario there that fails without it (`src/core/ui/AGENTS.md`, Cached node trees).
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

## Developer Tools: the `dev_tools` Condition

The developer tools (the console, the dev windows, the debug keys and overlays, the log's ring) are in a build only under the cfg `dev_tools`, which `src/build.rs` sets when debug assertions are on or the `dev-tools` feature is (`ryo2d build --dev-tools`; the `profiler` feature brings it too). `ryo2d::DEV_TOOLS` reads it. A game built with plain `ryo2d build` holds none of their code or strings.

- Gate a tool with `#[cfg(dev_tools)]` on items and fields, never `if cfg!(..)` around live code. Where both builds call into a tool, write a pair of small `cfg`ed functions (`Engine::console_rect`, `console_cursor`). Game-facing calls (`ctx.console()`, `Command`, `CommandSpec`, the builder's console calls, the log macros) exist in both builds and do nothing without the tools.
- `[debug]` switches resolve off in a build without the tools, whatever the project or the builder set (`DebugConfig::switch_tools_off`; `src/core/config/AGENTS.md`).
- The engine's tests also run in a release build, without the tools (`cargo test --release -p ryo2d`): a test of a tool is gated `#[cfg(dev_tools)]`. `cargo test --release --test no_tools` runs `tests/no_tools.rs`, what a shipped build leaves out (empty in a development run), and `cargo check -p ryo2d --lib --release` and `cargo check -p console-demo --release` keep that build warning-free. Details: `src/core/AGENTS.md`, A build without the tools.

## Logging

- Engine code logs with `log::trace!` to `log::error!` (the `log` crate; the target is the module path, so engine records are under `ryo2d::`). No `println!` or `eprintln!` in engine library code outside `src/cli/`, `src/core/network/` and the logging module's own stderr output.
- A log call must not format before its level is known (`log`'s macros check first), and an output never holds a lock another output waits on.
- Games log with the prelude's `info!` and the rest, under `game::`; a crate calling `log` directly is a library to the log (default level `warn`).
- **A thread the engine spawns** runs its body through `logging::scoped(..)`, read on the spawning thread, so its records carry its engine's log scope and reach that engine's console and a test's capture; a new spawn site must do the same (`src/core/logging/AGENTS.md`, Scopes and the frame).
- **Tests** open `capture_log()` before building any engine, world or asset server, and assert on it; they never read stderr and never lower a level default (raising one only under the levels guard).

## Profiling

- `profile_scope!("name")` / `profile_function!()` (`profiler.rs`, `use crate::profile_scope;` inside the engine); no-ops without the `profiler` feature, and recorded only while scopes are on (the profiler window open, or `[debug] enable_profiler`).
- The engine's scopes are named `cpu:…` or `gpu:…` (the prefix colours the bar): `cpu:update`, `cpu:physics`, `cpu:particles`, `cpu:render`, `cpu:render:particles`, `cpu:render:submit`, `cpu:atlas:spread texture`, `cpu:atlas:spread first band`, `gpu:present`, `cpu:scene_push`, `cpu:scene_switch`; add scopes inside systems that may be slow.
- `windows/profiler_window.rs` is a separate CPU-drawn flamegraph window (a puffin sink, throttled to 10 FPS, bars fade instead of flickering); it turns scopes on while open. Run with `./dev.sh <example> -p` or `ryo2d dev --profiler`, which build with `--features ryo2d/profiler` (no game feature needed). More in `ryo-profiler`.

## Documentation Homes

| What | Where |
|------|-------|
| API | Rustdoc on public items |
| Using the engine | Skills in `.claude/skills/` (the one copy: the CLI embeds every `<name>/SKILL.md` there at build time) |
| Module conventions and decisions | The nearest `AGENTS.md` |
| The big picture | `docs/architecture/` |
| Designs and their reasons | `docs/specs/` |

## Demos

Feature demos share one shell (keys, settings panel, layout) through `examples/demo-kit`; see `examples/demos/AGENTS.md`.
