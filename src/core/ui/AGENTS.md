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

`Scene::ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) -> SceneAction` runs once per frame for the visible scenes, bottom-up, **before** the fixed steps (`Engine::ui_pass`, `engine/frame.rs`). It draws into a separate UI buffer that is composited **last**, above the world, lighting and post-processing. Engine stats/FPS overlays and the dev console are drawn into the same `UiContext` after the scenes, so they appear on top.

- **Input**: `UiContext` reads a `UiInput` snapshot of `Input` (blocks and consumption carried over). What the UI consumes (`consume_click`, `consume_escape`, `consume_key`, widget clicks and typed text) is applied to `Input` after the pass, so `update` and systems never see it: gameplay keys belong in `update` (`ctx.input`), keys the UI acts on in `ui` (`ui.input()`).
- **Time**: `ui.dt()` is real time, so UI animation runs through pause and slow motion.
- **Scene actions**: only the top scene's returned action counts (scenes below a transparent one draw, but their actions are dropped). It applies at once, before the fixed steps. If it brings a different scene to the top, the UI pass runs once more so the new scene's UI shows this frame.

API usage is covered by the `ryo-ui` skill.

## Notes for AI Agents

1. **State ownership**: retained widget state is owned by the caller, not `UiContext`.
2. **Screen space**: all coordinates are in screen pixels.
3. **Alpha blending**: colors with alpha < 255 blend with the background.
4. **The UI is never lit** and never post-processed.
