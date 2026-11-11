# AGENTS.md - Dev Windows

Development tool windows (profiler, logger) and guidance for adding new ones.

The whole module is a developer tool and native only: it is declared under `cfg(all(dev_tools, not(target_arch = "wasm32")))` (`core/mod.rs`), with the engine's dev window fields, and its windows' text (`text::CpuText`). A build without the tools, and the web, has none of it.

## Architecture Overview

Dev windows are standalone winit windows that share GPU resources with the main game renderer:

- **Isolated rendering**: each window renders independently with its own surface and a throttled frame rate (`render_throttle_ms`)
- **Live data sharing**: windows read engine state via shared references and message channels
- **Focus-aware throttling**: an unfocused game window gets no redraw events, so `about_to_wait` runs its frame through `unfocused_tick`; only when no window of ours (game or dev tool) has focus does it throttle to `EngineBuilder::background_fps` (0, the default, keeps `target_fps`) and skip presenting without `background_rendering`; the game keeps updating either way

All dev windows receive `Arc<wgpu::Device>` / `Arc<wgpu::Queue>` from the game renderer. Each creates its own `wgpu::Surface` and draws into a CPU pixel buffer (`DevWindowRenderer`).

## Text

The windows draw JetBrains Mono through `text::CpuText`: swash rasterizes the glyphs into the window's CPU pixel buffer at `FONT_PX` (12), and each glyph image is cached per character. It owns its own `Fonts` and `GlyphRasterizer`, since the windows live apart from the engine's registry. Layouts measure text in characters (`measure_text`, `CHAR_WIDTH`), never bytes, and keep a row pitch of `CHAR_HEIGHT + 2` (`CHAR_HEIGHT` is the font's ascent; descenders fit in the 2). Without the `default-fonts` feature nothing is drawn. `CHAR_WIDTH` and `CHAR_HEIGHT` are pinned to the font by a test; the dev_window tests also write a sample layout to `target/dev-window-text.png` for looking at, since the windows themselves need an OS window.

## Opening Windows

- **Logger**: `[debug] logger_window = true` opens it at startup. F9 toggles it.
- **Profiler**: requires the `profiler` cargo feature. `[debug] enable_profiler = true` only enables puffin collection and does NOT auto-open the window. F10 toggles it. `ryo2d dev -p` sets `RYO_PROFILER_AUTO_OPEN` to open it at startup.
- F9/F10 work while `[debug] dev_window_keys` is true, its default in a build with the tools (a build without them has no such keys: they are the game's).
- Usage and scopes are covered by the `ryo-profiler` skill. The log itself is `core/logging/` (its own module notes).

## Logger Window

- **Reading**: the window owns a `LogFeed` (`log_feed.rs`), its reading and filtering with no window, which the tests drive. The feed keeps the last sequence number it read and asks the ring for what came after (`logging::mirror_records` at `Trace`, the console mirror's reader), in its engine's log scope (`Engine::log_scope`, passed to `LoggerWindow::new`); it never reads by index or length, so it follows the log for a whole session. A poll with nothing new is one atomic read (`records_may_follow`). It starts from the oldest record the ring holds for the scope. `HeadlessEngine::log_feed` (doc-hidden) gives a test one with no window (`tests/e2e/dev_tools.rs`).
- **Its list** is bounded at `FEED_ROWS` (10,000, the ring's size: a feed that has fallen behind can take all the ring still has, and no more; a few megabytes). The oldest rows go first. Warnings and errors the ring dropped before the feed read them are counted (`missed`) and shown in the footer.
- **Rows**: `HH:MM:SS.mmm` (the record's time since the logger started), the frame (blank when none), the level letter, the origin letter (`E`ngine, `G`ame, `L`ibrary, from `logging::Origin`) and `target: message` (`row_text`, which a right-click or a selection copies).
- **Buttons**: the five levels (Trace off by default), `ENG`, `GAME`, `LIB` (libraries, off by default), `CLR` (empties the feed's list, not the ring, and the feed carries on after the newest record logged so far) and `BTM` (back to the newest row, following again). The filter applies to the rows kept, so turning a button on shows the earlier rows too.
- **Scrolling**: a wheel notch scrolls three rows; a trackpad's pixel delta is divided by the window's scale factor, then by the row pitch (`scroll_rows`). Both add to one carry, and the part of a row left is kept for the next delta, so a high-resolution wheel's tenths of a notch add up (`a_fractional_wheel_notch_adds_up`). A scrolled-up view stays on its rows as new ones arrive; scrolling back to the bottom follows again.

## Adding a New Dev Window

Use `logger_window.rs` as the template (`DevWindowRenderer` + `DevWindowConfig`). Gate `render()` on `should_render()` and call `mark_rendered()`, so the throttle is respected. Finish with `present()`.

Wire it into the engine (mirror `logger_window` at each site), every piece under `cfg(all(dev_tools, not(target_arch = "wasm32")))`:

1. `Engine` fields (`engine/mod.rs`): `Option<MyWindow>`, a `my_window_focused` flag, and a `pending_open_*` flag if it has a hotkey.
2. The hotkey in `handle_tool_keys` (`engine/frame.rs`), which consumes the press. Creation needs the `ActiveEventLoop`, so the key handler only sets the pending flag.
3. The dev window `impl Engine` block of `engine/window.rs` (its counterpart below it does nothing, for a build without the tools and the web): create the window in `open_dev_windows_at_start` (config-driven, from `finish_gpu_setup`) and/or `open_pending_dev_windows` (hotkey requests, from `about_to_wait`).
4. `dev_window_event` (first thing in `window_event()`): route by `window_id()`. Handle `CloseRequested` (drop the window, don't exit), `RedrawRequested` → `render()`, and `Focused` → update the focus flag. Forward the rest to `handle_event()`.
5. Its focus flag in `dev_window_focused` (the throttle check) and `redraw_focused_dev_windows`, and its `render()` in `render_dev_windows` (drawn while the game has focus, and from `unfocused_tick` when not throttled).
