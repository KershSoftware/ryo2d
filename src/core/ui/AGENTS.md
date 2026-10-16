# AGENTS.md - UI Module

## Architecture: Hybrid Mode

The UI system combines two paradigms for game HUDs, menus, and interactive elements:

### Immediate Mode (`immediate/`)
- Stateless per-frame drawing (buttons, labels, progress bars) via `UiContext` in `Scene::ui`
- Input detection at draw time (buttons return `true` when clicked)
- Ideal for HUDs, debug overlays, dynamic UI

### Retained Mode (`retained/`)
- Caller-owned state structs (`DropdownState`, `TextInputState`, `ScrollViewState`) persisting across frames
- State includes cursor position, selection, scroll offset, focus
- Rendered through `UiContext` (`immediate/context.rs`)
- Ideal for forms, text input, complex menus

### Why Hybrid?
- **Immediate for simple things**: buttons, labels, progress bars need no state
- **Retained for complex things**: text input needs cursor state, dropdowns need open/focus state
- **Performance**: stateless widgets are fast. Stateful widgets are used only where needed.

## Integration Point

`Scene::ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) -> SceneAction` runs once per frame for the visible scenes, bottom-up, **before** the fixed steps (`Engine::ui_pass`, `engine/frame.rs`). Engine stats/FPS overlays and the dev console are drawn into the same `UiContext` after the scenes, on the overlay layer, so they appear on top of the scenes' pages and open lists alike.

- **Input**: `UiContext` reads a `UiInput` snapshot of `Input` (blocks and consumption carried over). What the UI consumes (`consume_click`, `consume_escape`, `consume_key`, `consume_scroll`, widget clicks, the wheel over a scrolling view and typed text) is applied to `Input` after the pass, so `update` and systems never see it: gameplay keys belong in `update` (`ctx.input`), keys the UI acts on in `ui` (`ui.input()`).
- **Time**: `ui.dt()` is real time, so UI animation runs through pause and slow motion.
- **Scene actions**: only the top scene's returned action counts (scenes below a transparent one draw, but their actions are dropped). It applies at once, before the fixed steps. If it brings a different scene to the top, the UI pass runs once more so the new scene's UI shows this frame.

API usage is covered by the `ryo-ui` skill.

## Layout and ids

Containers (`containers.rs`: `column`, `row`, `grid`, `stack`, `panel`) are laid out by the layout engine (`layout/`, the only code that names Taffy); `tree.rs` holds the subtree being built and its resolution.

- **Ids** (`id.rs`) come from call sites: a hash of the enclosing scope, the builder's `#[track_caller]` site and how often that site ran in the scope this frame. A helper called in a loop gets one id per call, and skipping an earlier call shifts the later ones; lists that reorder or filter use `.id(key)`, which replaces the count with the key. A container's id is the scope of what it contains, and `ui.scope(key, |ui| ..)` adds a keyed scope with no node, so a game's component salts its widgets' ids without touching layout.
- **Scenes are root scopes.** `SceneManager::ui_pass` brackets each scene's `ui` with `begin_scene(scope_id, is_top)` and `end_scene`; the overlays and the console are scene 0. The same code in two scenes makes two ids, and a scene's records and state carry its scope id, so they are forgotten when it exits (`SceneManager::take_exited`, drained by `Engine::draw_ui` into `UiMemory::forget_scene`).
- **Each top-level container lays out when its `.show()` returns.** Opening a container with none open starts a subtree (remembering its `at` rect and the clip in force); nested containers and leaves join it, and closing its root lays it out, places it by its anchor and offset in the UI (or at its `at` rect), snaps every rect to render pixels (`render::ui::snap_rect`) and resolves its draws. A leaf outside any container (`ui.text("..")` without `.at`) is a subtree of its own, laid out at once. So a top-level container's `Response` has this frame's rect; one inside another container has last frame's.
- **Text leaves** are measured by the text engine through the layout's measure callback, wrapped to the width they get (left-aligned when measured, so a centred text does not claim the whole row) and drawn at their unsnapped width, so snapping never wraps them. `UiText` draws itself when dropped: `ui.text("Hi");` is a statement.
- **Draws keep submission order through `order`.** Every draw takes `UiContext::next_order()` when it is recorded, inside a container or not; a container's draws reach `UiCommands` only when it is laid out, after absolute draws recorded later, and `emit_ui` orders by `ui_z(layer, order)`, never by position in the list.
- **Records are written for next frame's hit tests.** Each container writes a `Record` (rect, clip, layer, order, depth, scene, flags, focus scope, scroll ancestor, content size) into `UiMemory` when its subtree is laid out. Hit testing and navigation read last frame's records (`UiMemory::prev`), so input lands on what was drawn. The memory and layout engine keep one frame per engine frame: a rerun UI pass (a scene switched from `ui`) discards the first pass's records and ids with `UiMemory::restart_pass`.
- A container still open when a scene's `ui` ends (its contents panicked) is laid out by `end_scene`, warned once.

## Drawing on the GPU

- **Recording.** `UiContext` draws nothing itself: every primitive records a `UiCommand` (`Rect`, `Sprite`, `Line`, `Text`) in UI units into the engine's `UiCommands`. The commands, clips, text, fades, the typewriter and popups are described in `immediate/AGENTS.md`.
- **The UI pass.** At present time `render::ui::emit_ui` lays the commands into the 2D batch for the UI camera (`UI_CAMERA`, `CameraDraw::ui`): one z per command (layer, then the order it was recorded in), so the painter's order holds. The `ui` render pass draws that camera into the UI image: `Rgba8Unorm`, gamma-encoded colour premultiplied by alpha (`UI_FORMAT`, the `UI_GAMMA` override). The composite blends it over the frame in sRGB space, so a 50% black panel over white is 128, as on the web. Rects are analytic (`BatchShader::UiRect`: rounded corners, inner border, anti-aliased edges). Frame steps and shaders are in `src/core/render/AGENTS.md`.
- **UI units and scale** (`render/ui.rs`, `UiSpace`). With the automatic scale (`ui_scale: None`, the default) UI units are screen units at any display scale; text is laid out at render pixels, so it is sharp on HiDPI displays. `EngineBuilder::ui_scale(Some(s))` / `ctx.request_ui_scale` make it `s` render pixels per UI unit, rects and text alike; mouse positions are converted to UI units (`UiInput::with_mouse_scale`). `UiResolution::Render` (default) draws the UI at the render size (pixel art: the low-res target, snapped like the world); `Window` draws it at window resolution.
- **Clipping** is a scissor per command (the clip in image pixels, rounded outward); the stack has no depth limit.
- **Text** goes through the text engine (`src/core/text/AGENTS.md`): measured, wrapped and laid out at the UI scale. Fonts are the `Fonts` resource; the UI uses `FontRole::Ui`, and the console and overlays `FontRole::Mono`.

## Notes for AI Agents

1. **State ownership**: retained widget state is owned by the caller, not `UiContext`.
2. **UI units**: all coordinates are UI units (screen units with the automatic scale), never render pixels.
3. **Alpha blending**: colours with alpha < 255 blend with what is below in sRGB space, as designers expect.
4. **The UI is never lit** and never post-processed.
5. **Fade text through opacity**, never through a style's colour: colour is in the layout cache key, so a colour fade lays the text out again every frame (`ui.text(..).opacity(a)`).
