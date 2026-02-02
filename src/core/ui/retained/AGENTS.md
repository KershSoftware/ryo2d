# AGENTS.md - Retained Mode UI

## Overview

This folder implements **retained-mode widgets** for Ryo2D. Unlike immediate-mode widgets that are stateless per-frame, retained widgets maintain persistent state across frames.

**Retained mode** means:
- Caller creates and stores a state struct (e.g., `DropdownState`)
- State persists across frames (selection, cursor, focus, scroll position)
- Widget methods mutate the state based on input
- Rendering happens through `UiContext` in `immediate/context.rs`

## File Structure

| File | Purpose |
|------|---------|
| `mod.rs` | Public exports and module documentation |
| `dropdown.rs` | `DropdownState` - Select from a list of options |
| `text_input.rs` | `TextInputState` - Text entry with cursor, selection |
| `scroll_view.rs` | `ScrollViewState` - Scrollable content areas |

## Key Types

### DropdownState

Dropdown/select widget for choosing from a list:

```rust
// Store in scene
difficulty: DropdownState,

// Initialize with unique ID (for Tab navigation)
difficulty: DropdownState::with_id(1),

// Render each frame
ui.dropdown(&mut self.difficulty, rect, &["Easy", "Medium", "Hard"]);

// Get selected value
let selected = self.difficulty.selected();
```

**State fields:**
- `selected` - Currently selected index
- `open` - Whether the dropdown list is visible
- `hover_index` - Keyboard navigation within open list
- `focused` - Whether focused for Tab navigation
- `just_closed` - Prevents re-open on same keypress

### TextInputState

Text entry with full editing support:

```rust
// Store in scene
username: TextInputState,

// Initialize
username: TextInputState::new(),
// or with initial text:
username: TextInputState::with_text("default"),

// Render each frame (dt for cursor blink)
if ui.text_input(&mut self.username, rect, "Enter name...", dt) {
    // Text changed
}

// Get text value
let text = self.username.text();
```

**State fields:**
- `text` - The text buffer
- `cursor` - Cursor byte position
- `selection_anchor` - Selection start (for shift-select)
- `focused` - Whether accepting input
- `blink_timer` / `cursor_visible` - Cursor blink animation

**Features:**
- Arrow key navigation with Ctrl for word jumps
- Shift+Arrow for selection
- Ctrl+A select all, Ctrl+C copy, Ctrl+V paste, Ctrl+X cut
- Home/End for line start/end
- Backspace, Delete with word deletion (Ctrl)

### ScrollViewState

Scrollable content container:

```rust
// Store in scene
log_scroll: ScrollViewState,

// Render with callback for content
ui.scroll_view(&mut self.log_scroll, rect, content_height, |ui, offset| {
    // Draw content with y positions adjusted by offset
    for (i, line) in lines.iter().enumerate() {
        let y = i as f32 * 20.0 - offset;
        ui.draw_text(line, 10.0, y, Color::WHITE);
    }
});
```

**State fields:**
- `scroll_x`, `scroll_y` - Current scroll offsets
- `velocity_x`, `velocity_y` - Momentum for smooth scrolling
- `dragging_scrollbar` - Whether scrollbar thumb is being dragged
- `dragging_content` - Whether content is being drag-scrolled

## Usage Pattern

All retained widgets follow the same pattern:

1. **Store state** in your scene/game struct
2. **Initialize** in constructor (usually `::new()` or `::with_id()`)
3. **Render** each frame by calling `ui.widget_name(&mut state, ...)`
4. **Query** the state for values (`.selected()`, `.text()`, etc.)

## Focus Management

For Tab navigation between widgets:

```rust
// At end of render_ui(), after all dropdowns are drawn:
ui.handle_dropdown_tab_navigation(&mut [
    &mut self.dropdown1,
    &mut self.dropdown2,
    &mut self.dropdown3,
]);

// Draw dropdown overlays on top of other UI
ui.finish_dropdowns();
```

## Testing

All modules have unit tests:
- `dropdown.rs` - Constructors, state management, focus, just_closed flag
- `text_input.rs` - Cursor navigation, selection, clipboard, unicode
- `scroll_view.rs` - Scroll bounds, clamping, momentum

Run with: `cargo test --lib ui::retained`

## Notes for AI Agents

1. **State is owned by caller** - These structs are stored in game/scene, not UiContext
2. **Rendering through UiContext** - Drawing happens in `immediate/context.rs`
3. **Input handling is per-widget** - Each widget handles its own input when rendered
4. **Test state logic** - Unit tests cover state manipulation, not rendering
