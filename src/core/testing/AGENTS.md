# AGENTS.md - Headless Testing

`HeadlessEngine` runs the real engine without a window: offscreen `WgpuRenderer`, one fixed update per `step`, pixels via `frame()`. Spec: `docs/specs/2026-10-10-headless-rendering-design.md`.

## How it fits

- `EngineBuilder::build_headless` → `Engine::new(.., headless: true)` → `init_gpu(WgpuRenderer::new_offscreen(..))`.
- `step()` calls the same `Engine::step_frame` as the window loop, with `dt = Some(fixed_timestep)`; `TimeState::advance` makes that exactly one update.
- Input goes through the same `InputState::handle_*` methods winit events use.

## Gotchas

- One process-wide device (`shared_gpu`) — concurrent device creation deadlocks on Windows. `tests/common/gpu.rs` reuses it.
- Headless never calls `TimeState::tick_fps_counter`: it resets `frame_count` on the wall clock, and `frame_count` drives trail/lighting animation time.
- Headless mode disables audio, gamepads, hot reload, dev windows, user preferences, `RYO_CAPTURE_*` and `RYO_INPUT`, and forces FPS/timing/stats overlays off. Engine debug keys (F1-F5, F9, F10, F12) are inert headless, so injected F-keys reach the game like any other key. Keep new wall-clock-dependent features out of headless frames the same way.
- `tick_frame` (unfocused window) is still a separate frame path; it shares `present_frame` but not `step_frame`.
