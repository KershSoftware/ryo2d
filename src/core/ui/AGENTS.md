# AGENTS.md - UI Module

## Architecture: Hybrid Mode

The UI system combines two paradigms for game HUDs, menus, and interactive elements:

### Immediate Mode (`immediate/`)
- Stateless per-frame drawing (buttons, labels, progress bars) via `UiContext` in `render_ui()`
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

`render_ui()` draws into a separate UI buffer that is composited **last**, above trails, GPU sprites, lighting, and post-processing. Engine stats/FPS overlays and the dev console are drawn into the same `UiContext` after the scene, so they appear on top. Clicks/Escape consumed by UI are propagated to `InputState`, so they don't leak into game input.

API usage is covered by the `ryo-ui` skill.

## Notes for AI Agents

1. **State ownership**: retained widget state is owned by the caller, not `UiContext`.
2. **Screen space**: all coordinates are in screen pixels.
3. **Alpha blending**: colors with alpha < 255 blend with the background.
4. **Live code is in `immediate/` and `retained/`**: `ui/font.rs` and `ui/context.rs` are not declared in `mod.rs` and are not compiled.
