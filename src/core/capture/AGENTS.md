# AGENTS.md - Frame Capture

Saves the final composed frame (scene + UI) as a PNG at game resolution. Spec: `docs/plans/2026-10-10-frame-capture-design.md`.

## Split

- `CaptureState` (engine) decides *when* and *where*: presented-frame counter, `RYO_CAPTURE_*` schedule, F12/console requests, background PNG writes, exit.
- `WgpuRenderer` only knows one pending path: `request_capture(path)` before a render call, `take_capture_result()` after. The capture pass reuses `draw_final_composite` with the identity uniform (no letterbox).
- Keep scheduling out of the renderer so a headless harness can drive the same API.

## Gotchas

- `read_texture_rgba` blocks (`Maintain::Wait`) and needs `COPY_SRC` plus an 8-bit RGBA/BGRA format; anything else is `CaptureError::UnsupportedFormat`.
- The capture texture uses the **surface** format (the final-composite pipelines target it), so readback must handle BGRA.
- "Frame N" counts presented frames only; a frame lost to `SurfaceError` is retried with the same number.
- Both present paths in `engine.rs` (`do_full_render_and_present` and `RedrawRequested`) must call `path_for_next_frame` before rendering and `on_frame_presented` after `Ok`.
- In-flight writes are joined in `ApplicationHandler::exiting` (not after `run_app`: on macOS, Cmd+Q exits the process without returning). Scheduled frames never presented are reported as failures there; with `RYO_CAPTURE_EXIT=1` any scheduled failure exits with code 1.
