# AGENTS.md - Core Engine Modules

Most subsystems have their own AGENTS.md or a skill in `.claude/skills/`. This file covers cross-cutting design and the root-level files and folders without one: `engine/`, `scene/`, `schedule.rs`, `time.rs`, `input.rs`, `messages/`, `window.rs`, `cursor.rs`.

## Design Philosophy

- **Engine owns the loop** - `Engine::frame` (`engine/frame.rs`) runs the phases; game code runs in `Scene` callbacks and in systems added to phases, never calls update or render itself
- **Ryo2D's ECS** - the kernel types (`World`, `Query`, `Commands`, `Resource`, `Message`, ...) are exposed directly from `ryo2d::ecs` and the prelude; `src/core/ecs/AGENTS.md` has the phases, the hierarchy and interpolation rules
- **Resources over engine fields** - per-frame state the scenes and systems share (`Time`, `Input`, `ScreenMapping`, `AssetManager`, `EffectsManager`, `LightingSettings`, `RenderPipeline`, `RenderTargets`, `DebugDraw`, ...) are resources, inserted by `init_world_resources` (`engine/mod.rs`); renderers, the window, Kira audio and the scene stack stay engine-owned
- **Cameras are entities** - `Camera2d` + `Transform` (+ the computed `CameraView`), not a resource: viewports, render order, interpolation and per-camera visibility (`RenderLayers`) come from the ECS. The main camera (`MainCamera`) survives world cleanup; `ctx.camera()` edits it
- **Assets via handles** - loaded through `AssetManager`, shared via `Arc`-backed handles (`SpriteHandle`)
- **Minimal public surface** - expose only what users need via the `lib.rs` prelude; internals stay private

## Time

`Time` (`time.rs`) is a resource and `ctx.time` in scene callbacks, with three clocks:

- `real`: wall clock, never scaled or paused. Camera shake, the console, UI animation (`ui.dt()`), timed post-process effects and the hitstop countdown use it.
- `game`: real delta times `speed` (`0..=100`), zero while paused or in hitstop, then capped: one frame adds at most `max_delta` (default 250 ms, or one fixed timestep if that is longer) of game time, after speed scaling. A stall or a high speed runs at most 15 fixed steps at 60 Hz. Sprite effects animate on it.
- `fixed`: accumulates game time into constant steps (`EngineBuilder::fixed_hz`, default 60). Each step runs `FixedUpdate` once: `Scene::update` (its `dt` is the timestep), juice, scene fixed systems, physics. Slow motion runs fewer steps, pause and hitstop run none; the step length never changes.

`time.delta()` is the fixed timestep inside `FixedUpdate` and the game delta elsewhere. Rapier steps by `fixed.timestep()`, so physics follows speed, pause and hitstop like everything else.

## Input

`Input` (`input.rs`) is rebuilt in `First` from queued `InputEvent`s (window events, gamepads, `RYO_INPUT`, the headless harness).

- One-shots (`key_pressed`, `mouse_pressed`, ...) are visible in the first fixed step of a frame only; a press in a frame that runs no fixed step waits for the next step (two views: the frame view and the fixed view, `end_fixed_steps`), except while the game clock is paused, when the fixed view is cleared so presses are not replayed on resume.
- `consume_key` / `consume_mouse` / `consume_text` / `consume_scroll` hide the input, held state included, for the rest of the frame. The UI pass runs before the fixed steps and consumes what it uses, so `update` never sees a click the UI took.
- `block_pointer` / `block_all` / `unblock` gate input for gameplay (the demo shell uses them under help and over its panel).

## Scenes (`scene/`)

- The stack (`SceneManager`) and the Kira `Audio` live in the world as the non-send `SceneHost`; the scene systems (`engine/frame.rs`) take it out, build a `SceneContext` per call and put it back.
- Each stacked scene owns its `FixedUpdate`/`Update` schedules, its message cursors (starting past every message written before it entered) and a console scope (unique per instance): its systems, unread messages and console handlers leave with it.
- Updating: the top scene, and below it while each scene above `updates_below`, bottom-up. Only the top scene's `SceneAction` is applied; a lower scene's non-`Continue` action (from `update` or `ui`) is dropped with a warning, logged once per scene instance. While a transition runs, `Scene::update` waits but scene systems keep running.
- Drawing and `ui`: the top, and below it while each scene above `is_transparent`.
- `PhysicsPaused` is true when the top scene `is_physics_paused`, or when it sits over another scene without `updates_below`.
- Cleanup (`SceneCleanup`, `scene/cleanup.rs`) runs on `Switch` and `Reset`, never on `Push`/`Pop`. `WORLD` uses `despawn_game_entities`, which keeps resources, observers and registered systems.
- An action returned from `ui` applies at once, before the fixed steps; if it changes the top scene the UI pass runs again so the new scene's UI shows this frame.

## Cursor (`cursor.rs`)

- Deferred requests avoid borrowing the window during scene callbacks: `ctx.set_cursor*()` / `show_cursor()` / `hide_cursor()` write the `CursorRequest` resource (`ctx.cursor()`), applied in `about_to_wait()` (`engine/window.rs`) via `apply_cursor_request()`. `WindowRequest` (`ctx.window()`) works the same way for resolution, display mode and UI scale.
- Demo: `examples/demos/cursor/`

## Related Skills

- Profiler (`profiler.rs`, `windows/profiler_window.rs`): skill `ryo-profiler`
- Developer console (`console/`): skill `ryo-console`
- Messages (`messages/`): skill `ryo-messages`
