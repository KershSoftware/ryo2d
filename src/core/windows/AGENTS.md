# AGENTS.md - Dev Windows

Development tool windows (profiler, logger) and guidance for adding new ones.

## Architecture Overview

Dev windows are standalone winit windows that share GPU resources with the main game renderer:

- **Isolated rendering**: each window renders independently with its own surface and a throttled frame rate (`render_throttle_ms`)
- **Live data sharing**: windows read engine state via shared references and message channels
- **Focus-aware throttling**: the main game only throttles to 30 FPS when ALL windows (game + dev tools) lose focus

All dev windows receive `Arc<wgpu::Device>` / `Arc<wgpu::Queue>` from the game renderer. Each creates its own `wgpu::Surface` and draws into a CPU pixel buffer (`DevWindowRenderer`).

## Opening Windows

- **Logger**: `[debug] logger_window = true` opens it at startup. F9 toggles it.
- **Profiler**: requires the `profiler` cargo feature. `[debug] enable_profiler = true` only enables puffin collection and does NOT auto-open the window. F10 toggles it. `ryo2d dev -p` sets `RYO_PROFILER_AUTO_OPEN` to open it at startup.
- F9/F10 only work when `[debug] dev_window_keys` is true (default in debug builds only).
- Usage and scopes are covered by the `ryo-profiler` skill. Logging macros are in `core/dev_log.rs`.

## Adding a New Dev Window

Use `logger_window.rs` as the template (`DevWindowRenderer` + `DevWindowConfig`). Gate `render()` on `should_render()` and call `mark_rendered()`, so the throttle is respected. Finish with `present()`.

Wire it into `engine.rs` (mirror `logger_window` at each site):

1. `Engine` fields: `Option<MyWindow>`, a `my_window_focused` flag, and a `pending_open_*` flag if it has a hotkey.
2. Create it in `resumed()` (config-driven) and/or `about_to_wait()` (pending hotkey requests). Creation needs the `ActiveEventLoop`, so key handlers only set the pending flag.
3. `window_event()`: route by `window_id()`. Handle `CloseRequested` (drop the window, don't exit), `RedrawRequested` → `render()`, and `Focused` → update the focus flag. Forward the rest to `handle_event()`.
4. Include the focus flag in the throttle check and the focused-window `request_redraw()` at the end of `about_to_wait()`.
