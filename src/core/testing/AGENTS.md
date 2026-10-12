# AGENTS.md - Headless Testing

`HeadlessEngine` runs the real engine without a window: offscreen `WgpuRenderer`, one fixed update per `step`, pixels via `frame()`. Spec: `docs/specs/2026-10-10-headless-rendering-design.md`.

## How it fits

- `EngineBuilder::build_headless` → `Engine::new(.., headless: true)` → `init_gpu(WgpuRenderer::new_offscreen(..))`, which enters the start scene.
- `step()` calls the same `Engine::frame` as the window loop with `Some(fixed_timestep)` as the frame's real time, so each step runs exactly one fixed update and one render.
- Input is queued on `Input` (`InputEvent`) the way window events are, and applied when the next frame begins; `tap_key` queues the release after the step that saw the press.

## Gotchas

- One process-wide device (`shared_gpu`): concurrent device creation deadlocks on Windows. `tests/common/gpu.rs` reuses it.
- Headless mode disables audio, gamepads, hot reload, dev windows, user preferences, `RYO_CAPTURE_*` and `RYO_INPUT`, and forces FPS/timing/stats overlays off. Engine debug keys (F1-F5, F9, F10, F12) are inert headless, so injected F-keys reach the game like any other key. Keep new wall-clock-dependent features out of headless frames the same way.
- Tests in this module reach engine internals (`HeadlessEngine::engine`, `Engine::sprite_gpu`); pixel-level checks belong in `tests/e2e/`.
