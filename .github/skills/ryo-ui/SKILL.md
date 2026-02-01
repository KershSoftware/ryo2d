---
name: ryo-ui
description: Immediate-mode UI for HUDs, menus, and overlays.
metadata:
  version: 1.0.0
---

# Ryo2D UI Specialist

You help users build game UI using the immediate-mode system.

## Immediate Mode

Call draw functions every frame - no persistent widget state:

```rust
fn render_ui(&mut self, ui: &mut UiContext) {
    ui.draw_text("Score: 100", 10.0, 10.0, Color::WHITE);
}
```

## Color

```rust
Color::WHITE, Color::BLACK, Color::RED, Color::GREEN, Color::BLUE
Color::hex(0xFF8000)        // RGB
Color::hex(0xFF800080)      // RGBA
Color::rgba(255, 128, 0, 128)
```

## Rect

```rust
let rect = Rect::new(x, y, width, height);
rect.contains(mouse_x, mouse_y)  // Hit testing
rect.center()                     // (cx, cy)
```

## Anchor

```rust
Anchor::TopLeft, Anchor::TopCenter, Anchor::TopRight
Anchor::CenterLeft, Anchor::Center, Anchor::CenterRight
Anchor::BottomLeft, Anchor::BottomCenter, Anchor::BottomRight
```

## Drawing

### Text

```rust
ui.draw_text("Hello", x, y, Color::WHITE);
ui.draw_text(&format!("Score: {}", self.score), 10.0, 10.0, Color::YELLOW);
ui.draw_text_anchored("FPS: 60", Anchor::TopRight, -10.0, 10.0, Color::GREEN);
```

### Rectangles

```rust
ui.draw_rect(Rect::new(10.0, 10.0, 100.0, 50.0), Color::RED);
ui.draw_rect(rect, Color::rgba(0, 0, 0, 128));  // Semi-transparent
```

### Progress Bars

```rust
ui.draw_progress_bar(
    Rect::new(10.0, 10.0, 200.0, 20.0),
    self.health / 100.0,  // 0.0 to 1.0
    Color::RED,            // Foreground
    Color::DARK_GRAY,      // Background
);
```

### Buttons

```rust
if ui.button("Start Game", Rect::new(100.0, 200.0, 150.0, 40.0)) {
    self.start_game();
}
```

## Common Patterns

### HUD Layout

```rust
fn render_ui(&mut self, ui: &mut UiContext) {
    // Top-left: Health
    ui.draw_text("HP", 10.0, 10.0, Color::WHITE);
    ui.draw_progress_bar(
        Rect::new(40.0, 10.0, 150.0, 20.0),
        self.health / self.max_health,
        Color::RED, Color::DARK_GRAY,
    );

    // Top-right: Score
    ui.draw_text_anchored(
        &format!("Score: {}", self.score),
        Anchor::TopRight, -10.0, 10.0, Color::YELLOW,
    );
}
```

### Pause Menu

```rust
fn render_ui(&mut self, ui: &mut UiContext) {
    if !self.paused { return; }

    let (sw, sh) = ui.screen_size();

    // Darken background
    ui.draw_rect(Rect::new(0.0, 0.0, sw, sh), Color::rgba(0, 0, 0, 180));

    // Title
    ui.draw_text_anchored("PAUSED", Anchor::Center, 0.0, -60.0, Color::WHITE);

    // Buttons
    let cx = sw / 2.0 - 75.0;
    if ui.button("Resume", Rect::new(cx, sh / 2.0, 150.0, 40.0)) {
        self.paused = false;
    }
    if ui.button("Quit", Rect::new(cx, sh / 2.0 + 50.0, 150.0, 40.0)) {
        self.quit = true;
    }
}
```

### Debug Overlay

```rust
if self.debug_mode {
    ui.draw_rect(Rect::new(5.0, 5.0, 200.0, 80.0), Color::rgba(0, 0, 0, 150));
    ui.draw_text(&format!("Entities: {}", self.count), 10.0, 10.0, Color::GREEN);
    ui.draw_text(&format!("Pos: ({:.0}, {:.0})", self.px, self.py), 10.0, 30.0, Color::GREEN);
}
```

## Tips

- UI is in screen space (pixels), not world space
- Call draw functions every frame
- Buttons return `true` on click
- Use `Anchor` for responsive positioning
- Layer UI by draw order (first = behind)
- Use semi-transparent backgrounds for readability

