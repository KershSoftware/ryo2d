# AGENTS.md - Frame Capture

Saves the final composed frame (scene + UI) as a PNG at game resolution. Spec: `docs/specs/2026-10-10-frame-capture-design.md`.

## Split

- `CaptureState` (engine) decides *when* and *where*: presented-frame counter, `RYO_CAPTURE_*` schedule, F12/console requests, background PNG writes, exit.
- `WgpuRenderer` only knows one pending path: `request_capture(path)` before a render call, `take_capture_result()` after. The `capture` pass reuses `Compositor::draw` with the identity uniform (no letterbox).
- Keep scheduling out of the renderer so a headless harness can drive the same API.

## Gotchas

- `read_texture_rgba` blocks (`Maintain::Wait`) and needs `COPY_SRC` plus an 8-bit RGBA/BGRA format; anything else is `CaptureError::UnsupportedFormat`.
- The capture texture uses the **surface** format (the final-composite pipelines target it), so readback must handle BGRA.
- "Frame N" counts presented frames only; a frame lost to `SurfaceError` is retried with the same number.
- There is one present path, `Engine::present_frame` (`engine/extract.rs`), for window and headless runs: it calls `path_for_next_frame` before rendering and `on_frame_presented` only when the frame was presented. Keep it that way if you add another.
- In-flight writes are joined in `ApplicationHandler::exiting` (`engine/window.rs`; not after `run_app`: on macOS, Cmd+Q exits the process without returning). Scheduled frames never presented are reported as failures there; with `RYO_CAPTURE_EXIT=1` any scheduled failure exits with code 1.

## Scripted runs

- `RYO_INPUT` (`src/core/input_script.rs`) taps or holds keys and mouse buttons, and moves the mouse (game coordinates), at presented frames, counted by `CaptureState::presented_frames`. `Engine::frame` queues the frame's input (`before_frame`) before `First` publishes it, and releases a tap (`after_frame`) once a fixed update has seen its press.
- `RYO_HEADLESS` (`capture/config.rs`) makes `EngineBuilder::build` build like `HeadlessEngine` (no audio, gamepads, preferences, wall-clock overlays) but keep `RYO_CAPTURE_*` and `RYO_INPUT`; `Engine::run` then steps offscreen at the fixed timestep until the scene quits or `RYO_CAPTURE_EXIT` is met. Any invalid `RYO_*` value makes `run` exit with code 2 first: a scripted run with a typo would otherwise capture the wrong thing or never end.
- Both are read in dev builds only (`cfg!(debug_assertions)`); release builds warn and ignore them. `HeadlessEngine` tests ignore all of these and drive input with `tap_key`.
