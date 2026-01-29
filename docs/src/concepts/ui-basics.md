# UI Basics

Ryo2D includes a simple **immediate-mode UI system** for game HUDs, health bars, score displays, and debug overlays. This chapter explains how to use it effectively.

## Overview

The UI system uses an **immediate-mode** design pattern:
- You call draw functions every frame in `render_ui()`
- No widget trees or persistent state to manage
- Buttons return `true` when clicked that frame

This approach is perfect for game UIs that change frequently and need to be simple.

## The UiContext

Your game receives a `UiContext` in the `render_ui` callback:

```rust,ignore
fn render_ui(&mut self, ui: &mut UiContext) {
    // Draw UI elements here
}
```

The `UiContext` holds:
- A reference to the frame buffer
- Current input state (for click detection)
- A bitmap font for text rendering

## Drawing Text

Use `draw_text()` for simple text at screen coordinates:

```rust,ignore
ui.draw_text("Hello, World!", 10.0, 10.0, Color::WHITE);
ui.draw_text(&format!("Score: {}", self.score), 10.0, 30.0, Color::YELLOW);
```

For anchored text that stays in a screen corner:

```rust,ignore
// Top-right corner, 10px offset
ui.draw_text_anchored("Lives: 3", Anchor::TopRight, -10.0, 10.0, Color::WHITE);

// Centered on screen
ui.draw_text_anchored("PAUSED", Anchor::Center, 0.0, 0.0, Color::WHITE);
```

## Drawing Shapes

### Rectangles

```rust,ignore
// Solid filled rectangle
ui.draw_rect(Rect::new(10.0, 10.0, 100.0, 20.0), Color::BLUE);

// Rectangle outline only
ui.draw_rect_outline(Rect::new(10.0, 40.0, 100.0, 20.0), Color::RED, 2.0);

// Panel with background and border
ui.draw_panel(
    Rect::new(10.0, 70.0, 100.0, 20.0),
    Color::DARK_GRAY,
    Some(Color::WHITE),
);
```

### Progress Bars

Perfect for health, mana, loading, or cooldown displays:

```rust,ignore
// Simple progress bar (50% filled)
ui.draw_progress_bar(
    Rect::new(10.0, 100.0, 200.0, 20.0),
    0.5,  // 50% progress
    Color::GREEN,      // Fill color
    Color::DARK_GRAY,  // Background color
);

// With border
ui.draw_progress_bar_with_border(
    Rect::new(10.0, 130.0, 200.0, 20.0),
    self.health / 100.0,
    Color::RED,
    Color::DARK_GRAY,
    Color::WHITE,
);
```

## Interactive Buttons

The `button()` method draws a button and returns `true` when clicked:

```rust,ignore
if ui.button("Start Game", Rect::new(300.0, 200.0, 120.0, 40.0)) {
    // Player clicked the button this frame
    self.start_game();
}
```

For custom-styled buttons:

```rust,ignore
if ui.button_styled(
    "Attack",
    Rect::new(10.0, 10.0, 100.0, 40.0),
    Color::rgb(60, 0, 0),      // Normal background
    Color::rgb(100, 0, 0),     // Hover background  
    Color::rgb(40, 0, 0),      // Pressed background
    Color::WHITE,              // Text color
) {
    self.attack();
}
```

## Checking Input

For custom interaction beyond buttons:

```rust,ignore
let rect = Rect::new(100.0, 100.0, 50.0, 50.0);

if ui.is_hovered(rect) {
    // Mouse is over this area
    ui.draw_rect(rect, Color::YELLOW);
}

if ui.is_clicked(rect) {
    // Mouse button just pressed over this area
}

if ui.is_held(rect) {
    // Mouse button held down over this area
}
```

## Colors

The `Color` type supports common presets and custom values:

```rust,ignore
// Presets
Color::WHITE, Color::BLACK, Color::RED, Color::GREEN, Color::BLUE
Color::YELLOW, Color::CYAN, Color::MAGENTA
Color::GRAY, Color::DARK_GRAY, Color::LIGHT_GRAY

// Custom RGB
Color::rgb(255, 128, 0)  // Orange

// Custom RGBA (with transparency)
Color::rgba(255, 128, 0, 128)  // 50% transparent orange

// From hex
Color::hex(0xFF8000)     // RGB orange
Color::hex(0xFF800080)   // RGBA with alpha

// Modify alpha
Color::RED.with_alpha(128)  // 50% transparent red
```

## Rectangles

The `Rect` type represents screen-space rectangles:

```rust,ignore
let r = Rect::new(x, y, width, height);

// Properties
r.right()  // x + width
r.bottom() // y + height
r.center() // (cx, cy)

// Modification
r.shrink(padding)  // Inset by padding
r.expand(padding)  // Outset by padding

// Hit testing
r.contains(mouse_x, mouse_y)
```

## Anchoring

The `Anchor` enum positions elements relative to screen edges:

```rust,ignore
// Nine anchor points
Anchor::TopLeft     Anchor::TopCenter     Anchor::TopRight
Anchor::CenterLeft  Anchor::Center        Anchor::CenterRight
Anchor::BottomLeft  Anchor::BottomCenter  Anchor::BottomRight
```

Use with `draw_text_anchored()` or calculate positions manually:

```rust,ignore
let (x, y) = Anchor::BottomRight.calculate_position(
    -10.0, -10.0,  // Offset from anchor
    100.0, 20.0,   // Element size
    screen_width as f32,
    screen_height as f32,
);
```

## Custom Fonts

The engine includes a built-in 8x8 debug font. You can load custom bitmap fonts:

```rust,ignore
// Font atlas: image with character grid
let font = BitmapFont::load("assets/my_font.png", 16, 16)?;

// Character dimensions: 16x16 pixels
// Default range: ASCII 32-126 (space through tilde)
```

Font atlases should be a grid of characters in row-major order starting from ASCII 32 (space).

## Complete Example

```rust,ignore
fn render_ui(&mut self, ui: &mut UiContext) {
    // Health bar in top-left
    ui.draw_progress_bar_with_border(
        Rect::new(10.0, 10.0, 200.0, 20.0),
        self.health / 100.0,
        Color::RED,
        Color::DARK_GRAY,
        Color::WHITE,
    );
    
    // Score in top-right
    ui.draw_text_anchored(
        &format!("Score: {}", self.score),
        Anchor::TopRight,
        -10.0, 10.0,
        Color::YELLOW,
    );
    
    // Pause button
    if ui.button("||", Rect::new(10.0, 40.0, 30.0, 30.0)) {
        self.paused = true;
    }
    
    // Pause overlay
    if self.paused {
        // Dim background
        let (w, h) = ui.screen_size();
        ui.draw_rect(
            Rect::new(0.0, 0.0, w as f32, h as f32),
            Color::rgba(0, 0, 0, 128),
        );
        
        // Centered text
        ui.draw_text_anchored("PAUSED", Anchor::Center, 0.0, -20.0, Color::WHITE);
        
        // Resume button
        if ui.button("Resume", Rect::new(340.0, 290.0, 120.0, 40.0)) {
            self.paused = false;
        }
    }
}
```

## Tips

1. **Draw order matters** - Later draws appear on top
2. **Screen coordinates** - (0, 0) is top-left, Y increases downward
3. **Alpha blending** - Semi-transparent colors blend with what's behind
4. **No clipping** - Drawing outside the screen is safe but wasteful
5. **Per-frame calls** - All UI is redrawn every frame; conditional logic is fine
