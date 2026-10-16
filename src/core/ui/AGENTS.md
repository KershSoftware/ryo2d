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

`Scene::ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) -> SceneAction` runs once per frame for the visible scenes, bottom-up, **before** the fixed steps (`Engine::ui_pass`, `engine/frame.rs`). Engine stats/FPS overlays and the dev console are drawn into the same `UiContext` after the scenes, so they appear on top.

- **Input**: `UiContext` reads a `UiInput` snapshot of `Input` (blocks and consumption carried over). What the UI consumes (`consume_click`, `consume_escape`, `consume_key`, `consume_scroll`, widget clicks, the wheel over a scrolling view and typed text) is applied to `Input` after the pass, so `update` and systems never see it: gameplay keys belong in `update` (`ctx.input`), keys the UI acts on in `ui` (`ui.input()`).
- **Time**: `ui.dt()` is real time, so UI animation runs through pause and slow motion.
- **Scene actions**: only the top scene's returned action counts (scenes below a transparent one draw, but their actions are dropped). It applies at once, before the fixed steps. If it brings a different scene to the top, the UI pass runs once more so the new scene's UI shows this frame.

API usage is covered by the `ryo-ui` skill.

## Drawing on the GPU

- **Recording.** `UiContext` draws nothing itself: every primitive records a `UiCommand` (`immediate/commands.rs`: `Rect`, `Sprite`, `Line`, `Text`) in UI units into the engine's `UiCommands`, with the clip and layer it was drawn under. A pass that runs twice in a frame (a scene switch returned from `ui`) clears the first pass's commands.
- **The UI pass.** At present time `render::ui::emit_ui` lays the commands into the 2D batch for the UI camera (`UI_CAMERA`, `CameraDraw::ui`): one z per command (layer, then submission), so the painter's order holds. The `ui` render pass draws that camera into the UI image: `Rgba8Unorm`, gamma-encoded colour premultiplied by alpha (`UI_FORMAT`, the `UI_GAMMA` override). The composite blends it over the frame in sRGB space, so a 50% black panel over white is 128, as on the web. Rects are analytic (`BatchShader::UiRect`: rounded corners, inner border, anti-aliased edges); nine-slices are nine sprite commands.
- **UI units and scale** (`render/ui.rs`, `UiSpace`). With the automatic scale (`ui_scale: None`, the default) UI units are screen units at any display scale; text is laid out at render pixels, so it is sharp on HiDPI displays. `EngineBuilder::ui_scale(Some(s))` / `ctx.request_ui_scale` make it `s` render pixels per UI unit, rects and text alike; mouse positions are converted to UI units (`UiInput::with_mouse_scale`). `UiResolution::Render` (default) draws the UI at the render size (pixel art: the low-res target, snapped like the world); `Window` draws it at window resolution.
- **Clipping** is a scissor per command (the clip in image pixels, rounded outward); the stack has no depth limit.
- **Text** goes through the text engine (`TextEngine`): measured, wrapped and laid out at the UI scale. The shorthand helpers (`draw_text`, `label`, ...) use the default UI style (`FontRole::Ui`, 14 px). The text input's click, caret, selection and scroll positions come from the engine (`byte_at`, `caret_x`, `selection_x`), never a fixed character width.
- **Fades go through opacity.** The layout cache keys on the style, colour included, so changing a style's colour every frame lays the text out again every frame. Widgets and helpers fade text through the text command's `opacity` (`UiCommand::Text { opacity }`), never through the style's colour. The shorthand helpers (`draw_text`, `label`, the wrapped and centred variants) do this for the colour they are given: they lay the text out opaque and carry the colour's alpha as opacity, so one cached layout serves every alpha. `draw_text_styled` lays its style out as given: moving that colour's alpha to opacity would fade the style's outline and shadow too.

- **`ui.text(..)`** is the full text call (`UiText`: markup, wrap, align, max lines, ellipsis, anchor, reveal, fade, opacity; `draw()` returns the size in UI units). The `draw_text*` family is the shorthand with the default style. A reveal with a fade wider than one cluster must be driven past the cluster count (`TextReveal::drawn_clusters`), or the last characters stay partly faded. A text's effect seed is a hash of the text, style and options, so it does not move when other commands are drawn before it.
- **Popups and layers.** Open dropdown lists record on the popup layer (1); `finish_dropdowns` is crate-internal and `SceneManager::ui_pass` runs it after each scene's `ui`. Layers order before submission order (`ui_z`); the console and overlays draw on layer 0 after the scenes and the popup layer still covers them.
- **Vertical centring** uses the font's real line height (`line_height`), never a fixed cell. Nine-slice insets clamp to the texture, and the corners shrink with a rect smaller than their sum.

## Notes for AI Agents

1. **State ownership**: retained widget state is owned by the caller, not `UiContext`.
2. **UI units**: all coordinates are UI units (screen units with the automatic scale), never render pixels.
3. **Alpha blending**: colours with alpha < 255 blend with what is below in sRGB space, as designers expect.
4. **The UI is never lit** and never post-processed.
