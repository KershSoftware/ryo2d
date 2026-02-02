# AGENTS.md - Immediate Mode UI

## Overview

This folder implements the **immediate-mode UI system** for Ryo2D. Every frame, you call draw functions to render UI elements - no widget state is persisted between frames. This is ideal for HUDs, debug overlays, and dynamic UI.

**Note:** Stateful/retained-mode widgets (`DropdownState`, `TextInputState`, `ScrollViewState`) are in `../retained/`. This folder contains only stateless drawing primitives and layout helpers.

## File Structure

| File | Purpose |
|------|---------|
| `mod.rs` | Public exports and module documentation |
| `context.rs` | `UiContext` - the main drawing API (renders both immediate and retained widgets) |
| `font.rs` | `BitmapFont` for text rendering, word wrapping |
| `layout.rs` | `HBox`, `VBox`, `Stack` stateless layout containers |

## Key Types

### UiContext

The central drawing context passed to `Scene::render_ui()`. Provides:

- **Drawing primitives**: `draw_rect`, `draw_panel`, `draw_text`, `draw_line`
- **Immediate widgets**: `button`, `slider`, `checkbox`, `toggle`, `progress_bar` (stateless)
- **Retained widgets**: `dropdown`, `text_input`, `scroll_view` (state structs from `retained/`)
- **Text functions**: `draw_text_wrapped`, `draw_text_centered`, `draw_text_anchored`
- **Clipping**: `push_clip`, `pop_clip`, `with_clip` for scissor regions
- **Interaction**: `is_clicked`, `is_hovered`, `is_held`

### BitmapFont

Monospace bitmap font with atlas-based rendering:

```rust
// Text measurement
let width = font.text_width("Hello");
let (w, h) = font.text_size("Line1\nLine2");

// Word wrapping
let lines = font.wrap_text("Long text here", max_width);
let (w, h) = font.wrapped_text_size("Long text", max_width);
```

### Layout Containers

```rust
// Horizontal layout
let mut row = HBox::new(x, y, spacing);
let rect1 = row.next(width, height);
let rect2 = row.next(width, height);

// Vertical layout  
let mut col = VBox::new(x, y, spacing);
let rect1 = col.next(width, height);
let rect2 = col.next(width, height);

// Stack (layered positioning)
let stack = Stack::new(x, y, width, height);
let centered = stack.centered(w, h);
let aligned = stack.aligned(w, h, HAlign::Left, VAlign::Top);
```

## Usage Patterns

### Basic Drawing

```rust
fn render_ui(&mut self, ui: &mut UiContext) {
    // Background panel
    ui.draw_panel(Rect::new(10.0, 10.0, 200.0, 100.0), 
                  Color::rgb(30, 30, 40), 
                  Some(Color::GRAY));
    
    // Text
    ui.draw_text("Score: 100", 20.0, 20.0, Color::WHITE);
    
    // Button (returns true when clicked)
    if ui.button("Click Me", Rect::new(20.0, 50.0, 100.0, 30.0)) {
        self.handle_click();
    }
}
```

### Word Wrapping

```rust
// Wrap text to fit width
ui.draw_text_wrapped(
    "This is a long piece of text that will wrap at word boundaries.",
    x, y,
    200,  // max width in pixels
    Color::WHITE
);

// Centered wrapped text
ui.draw_text_wrapped_centered(
    "Centered and wrapped",
    rect,
    Color::WHITE
);
```

### Clipping

```rust
// Scissor clipping for scroll regions
ui.push_clip(viewport_rect);
ui.draw_text("This will be clipped", x, y, Color::WHITE);
ui.pop_clip();

// Convenience method
ui.with_clip(rect, |ui| {
    // Everything here is clipped
});
```

### Layout Containers

```rust
// Toolbar with evenly spaced buttons
let mut row = HBox::new(10.0, 10.0, 8.0);
if ui.button("File", row.next(60.0, 30.0)) { /* ... */ }
if ui.button("Edit", row.next(60.0, 30.0)) { /* ... */ }
if ui.button("View", row.next(60.0, 30.0)) { /* ... */ }

// Vertical menu
let mut col = VBox::new(100.0, 100.0, 5.0);
for item in &menu_items {
    if ui.button(item, col.next(150.0, 30.0)) {
        self.select(item);
    }
}
```

## Design Principles

1. **Stateless** - No widget state persisted; caller manages all state
2. **Immediate** - Draw calls happen directly; no deferred rendering
3. **Composable** - Layout containers work with any widget
4. **Minimal allocations** - Hot paths avoid heap allocations

## Clipping Stack

The clip stack supports up to 16 nested clips. Clips intersect (nested clips are smaller than parents).

```rust
ui.push_clip(outer);  // depth 1
ui.push_clip(inner);  // depth 2, intersected with outer
// drawing here clipped to intersection
ui.pop_clip();        // depth 1
ui.pop_clip();        // depth 0 (no clipping)
```

## Testing

All modules have unit tests:
- `context.rs` - Clip stack, rect intersection
- `font.rs` - Text measurement, word wrapping edge cases
- `layout.rs` - HBox, VBox, Stack positioning

Run with: `cargo test --lib ui::immediate`
