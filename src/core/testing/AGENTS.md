# AGENTS.md - Headless Testing

`HeadlessEngine` (`ryo2d::testing`) runs the real engine without a window: an offscreen `WgpuRenderer`, one `Engine::frame` per `step`, pixels via `frame()`. Spec: `docs/specs/2026-10-10-headless-rendering-design.md`.

## How it fits

- `HeadlessEngine::new(builder)` → `EngineBuilder::build_headless` → `Engine::new(.., headless: true)` → `init_gpu(WgpuRenderer::new_offscreen(..))` on the `shared_gpu()` device, which inserts the `Gpu` resource and enters the start scene. `new_or_skip` returns `None` (printing a skip line) when there is no GPU.
- `step()` calls the same `Engine::frame` as the window loop with one fixed timestep as the frame's real time: the full frame runs (`First`, the UI pass, the fixed steps, `Update`, `PostUpdate`, render, `Last`), and at game speed 1 that is exactly one fixed step. A paused or hitstopped game clock runs none and slow motion runs fewer, as in a windowed run.
- Input is queued on `Input` (`InputEvent`) the way window events are and published when the next frame begins. `tap_key` queues the release after the step that saw the press. The UI pass runs before the fixed step, so a scene that records input in `update()` and draws it in `ui()` shows the previous step's value; probe with `ui.input()` in `ui()` when the frame must show the current one.
- `world()` / `world_mut()` reach the ECS between steps; `frame()` reads the last presented frame (scene + UI) at game resolution.

## Gotchas

- One process-wide device (`shared_gpu`): concurrent device creation deadlocks on Windows. `tests/common/gpu.rs` reuses it.
- Headless mode has no audio device (`ctx.audio` works, but its `Result` methods return `AudioError::Unavailable`), no real gamepads (`press_pad`, `tap_pad`, `set_stick` drive a virtual pad), hot reload, dev windows or user preferences, ignores `RYO_CAPTURE_*` and `RYO_INPUT`, and forces the FPS, timing and stats overlays off. Engine debug keys (F1-F5, F9, F10, F12) are inert, so injected F-keys reach the game like any other key; the console key (`` ` ``) still toggles the console when `[debug] dev_console` is on (the default in debug builds). Keep new wall-clock-dependent features out of headless frames the same way.
- Particle emitters seeded from entropy differ run to run; use `ParticleSystem::with_seed` when comparing frames exactly.
- Tests in this module reach engine internals (`HeadlessEngine::engine`, `Engine::sprite_gpu`); pixel-level checks belong in `tests/e2e/`.
