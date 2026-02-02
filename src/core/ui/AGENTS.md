# UI Module - AGENTS.md

## Purpose

The `ui/` module provides a **hybrid UI system** combining immediate-mode and retained-mode approaches for game HUDs, menus, and interactive elements.

## Architecture: Hybrid Mode

This UI system uses **two complementary paradigms**:

### Immediate Mode (`immediate/`)
- Stateless per-frame drawing (buttons, labels, progress bars)
- You call draw functions each frame in `render_ui()`
- Input detection at draw time (buttons return `true` when clicked)
- Ideal for HUDs, debug overlays, dynamic UI

### Retained Mode (`retained/`)
- Stateful widgets with persistent state across frames
- Caller owns state structs (`DropdownState`, `TextInputState`, `ScrollViewState`)
- State includes cursor position, selection, scroll offset, focus
- Ideal for forms, text input, complex menus

## Module Structure

| File/Folder | Purpose |
|-------------|---------|
| `mod.rs` | Module root with documentation and re-exports |
| `types.rs` | Basic types: `Color`, `Rect`, `Anchor` |
| `font.rs` | `BitmapFont` for text rendering |
| `theme.rs` | `Theme` for consistent styling (colors, spacing) |
| `immediate/` | Stateless drawing API (see `immediate/AGENTS.md`) |
| `retained/` | Stateful widget state structs (see `retained/AGENTS.md`) |

## Key Types

### Color
RGBA color with common presets and hex constructor:
```rust
Color::WHITE
Color::hex(0xFF8000)   // RGB
Color::hex(0xFF800080) // RGBA
Color::rgba(255, 128, 0, 128)
```

### Rect
Screen-space rectangle with hit testing:
```rust
Rect::new(x, y, width, height)
rect.contains(mx, my)  // Point-in-rect test
rect.center()          // Returns (cx, cy)
```

### Anchor
9-point screen positioning for anchored elements:
```rust
Anchor::TopLeft, Anchor::Center, Anchor::BottomRight, ...
anchor.calculate_position(offset_x, offset_y, elem_w, elem_h, screen_w, screen_h)
```

### BitmapFont
Loads font atlases and renders text to the frame buffer:
```rust
let font = BitmapFont::load("font.png", 8, 8)?;
let font = create_debug_font();  // Built-in fallback
font.draw_text(frame, width, height, "Hello", x, y, Color::WHITE);
```

### UiContext
The main drawing API, passed to `render_ui()`:
```rust
fn render_ui(&mut self, ui: &mut UiContext) {
    ui.draw_text("Score: 100", 10.0, 10.0, Color::WHITE);
    ui.draw_rect(rect, Color::RED);
    ui.draw_progress_bar(rect, 0.5, Color::GREEN, Color::DARK_GRAY);
    
    if ui.button("Start", rect) {
        // Handle click
    }
    
    // Dropdown with retained state
    ui.dropdown(&mut self.dropdown, rect, &["A", "B", "C"]);
    ui.finish_dropdowns(); // Call at end for proper z-order
}
```

## Design Decisions

### Why Hybrid Mode?
- **Immediate for simple things**: Buttons, labels, progress bars - no state needed
- **Retained for complex things**: Text input needs cursor state, dropdowns need open/focus state
- **Best of both worlds**: Simple API for HUDs, proper state management for forms
- **Performance**: Stateless widgets are fast; stateful widgets only where needed

### Font System
- Uses bitmap fonts (image atlases with character grids)
- Built-in 8x8 debug font for development
- Users can load custom fonts for final games
- Characters are blitted with alpha blending

### Integration Point
The engine calls `game.render_ui(&mut UiContext)` **after** sprite rendering but **before** debug overlays. This ensures UI is always on top of game content.

## Notes for AI Agents

1. **Immediate vs Retained** - Use immediate for simple widgets, retained for stateful ones
2. **State ownership** - Retained widget state is owned by caller, not UiContext
3. **Screen space** - All coordinates are in screen pixels
4. **Alpha blending** - Colors with alpha < 255 blend with background
5. **See nested AGENTS.md** - Check `immediate/AGENTS.md` and `retained/AGENTS.md` for details
