# AGENTS.md - Headless Testing

`ryo2d::testing` has two harnesses: `HeadlessEngine`, the whole engine on an offscreen GPU, and `UiHarness`, UI frames with no GPU at all (see UiHarness below). The module is native only.

`HeadlessEngine` (`ryo2d::testing`) runs the real engine without a window: an offscreen `WgpuRenderer`, one `Engine::frame` per `step`, pixels via `frame()`. Spec: `docs/specs/2026-10-10-headless-rendering-design.md`.

## How it fits

- `HeadlessEngine::new(builder)` → `EngineBuilder::build_headless` → `Engine::new(.., headless: true)` → `init_gpu(WgpuRenderer::new_offscreen(..))` on the `shared_gpu()` device, which inserts the `Gpu` resource and enters the start scene. `new_or_skip` returns `None` (printing a skip line) when there is no GPU.
- `step()` calls the same `Engine::frame` as the window loop with one fixed timestep as the frame's real time: the full frame runs (`First`, the UI pass, the fixed steps, `Update`, `PostUpdate`, render, `Last`), and at game speed 1 that is exactly one fixed step. A paused or hitstopped game clock runs none and slow motion runs fewer, as in a windowed run.
- Input is queued on `Input` (`InputEvent`) the way window events are and published when the next frame begins. `tap_key` queues the release after the step that saw the press. The UI pass runs before the fixed step, so a scene that records input in `update()` and draws it in `ui()` shows the previous step's value; probe with `ui.input()` in `ui()` when the frame must show the current one.
- `world()` / `world_mut()` reach the ECS between steps; `frame()` reads the last presented frame (scene + UI) at game resolution.
- **UI focus.** `ui_focused()` is the focus the last step resolved; `ui_focusables()` is `nav::nav_targets` over the frame just drawn (what the D-pad can land on in the top scene: focusable, enabled, shown, in a scope, not `nav_skip`, inside the innermost open modal or trap, with rects in UI units); `focus_ui(id)` is a `request_focus` the next step's resolve applies. Between frames `UiMemory::drawn` reads this frame's records (the resolves, inside a frame, read last frame's), so the list matches what `frame()` shows; the trap is the one the last resolve kept on `modal_stack`, so read it a frame or two after a modal opens or closes. The ui demo's `tests/acceptance.rs` walks its four acceptance pages (Settings on each tab and with a list open, Dialogue before and after Skip) and the Menus dialogs this way.

## UiHarness

`UiHarness` (`ui.rs`) is the engine's UI frame with no window, GPU or world: a `UiMemory`, a `LayoutEngine`, a `TextEngine` synced to the built-in fonts and the `UiCommands`, at a UI size of the caller's choosing, scale 1, the dark theme (`with_theme` for another). Games unit-test UI functions with it, and `benches/ui.rs` measures UI frames with it.

- **One `frame` is one engine UI pass**, 1/60 s after the last: `begin_frame` on the memory and the layout, `resolve_pointer`, `apply_scroll_requests`, `route_wheel`, `resolve_focus` (with the theme's timings), then the closure as the top scene (scene 1) between `begin_scene` and `end_scene`, then `end_frame` on the memory, the layout and the text cache. The closure's value is returned.
- **Scripted input lasts one frame**: `move_to` (the pointer stays), `press` (held until `release`), `release`, `wheel(dx, dy)` in lines, and `action(name)` for the nine `ui.*` actions, set straight on the frame's `NavInput` (no `Input`, no bindings). Another name warns once (`warn_once`, keyed by the name) and does nothing.
- **Reads**: `focused()` is `Interaction::focused` after the frame (it is always the top scene); `command_count()` the commands the last frame recorded.
- **A frame that records nothing** rewinds the memory (`UiMemory::rewind_frame`), so a read (`ui.scroll_offset(id)`) between frames leaves the UI as the last drawn frame left it; the crate's `UiDriver` relies on it.
- **`UiDriver`** (`src/core/ui/test_util.rs`, tests only) holds a `UiHarness` and derefs to it (`d.memory`, `d.commands`), adding the crate-only script (keys, typed text, IME, Shift, pad buttons, held directions, the stick, the device, the scene, prompts) and reads (sounds, the navigation outcome, captured rebinds). `UiHarness` keeps those fields `pub(crate)` for it.

## Gotchas

- One process-wide device (`shared_gpu`): concurrent device creation deadlocks on Windows. `tests/common/gpu.rs` reuses it.
- Headless mode has no audio device (`ctx.audio` works, but its `Result` methods return `AudioError::Unavailable`), no real gamepads (`press_pad`, `tap_pad`, `set_stick` drive a virtual pad), hot reload, dev windows or user preferences, ignores `RYO_CAPTURE_*` and `RYO_INPUT`, and forces the FPS, timing and stats overlays off. Engine debug keys (F1-F5, F9, F10, F12) are inert, so injected F-keys reach the game like any other key; the console key (`` ` ``) still toggles the console when `[debug] dev_console` is on (the default in debug builds). Keep new wall-clock-dependent features out of headless frames the same way.
- Particle emitters seeded from entropy differ run to run; use `ParticleSystem::with_seed` when comparing frames exactly.
- Tests in this module reach engine internals (`HeadlessEngine::engine`, `Engine::sprite_gpu`); pixel-level checks belong in `tests/e2e/`.
