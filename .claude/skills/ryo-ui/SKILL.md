---
name: ryo-ui
description: Build game UI with immediate-mode and retained-mode widgets including buttons, sliders, checkboxes, text inputs, dropdowns, scroll views, and layout containers. Use when creating HUDs, menus, settings screens, or any in-game interface.
metadata:
  version: 2.0.0
---

# Ryo2D UI Specialist

Build game UI using the hybrid immediate/retained mode system.

## Architecture

```text
┌─────────────────────────────────────────────────┐
│   Retained Mode (stateful widgets)              │
│   TextInputState, DropdownState, ScrollViewState│
├─────────────────────────────────────────────────┤
│   Layout Containers                             │
│   HBox, VBox, Stack                             │
├─────────────────────────────────────────────────┤
│   Immediate Mode (stateless per-frame)          │
│   UiContext drawing API                         │
├─────────────────────────────────────────────────┤
│   Theme System (hot-reload in debug)            │
└─────────────────────────────────────────────────┘
```

## Core Types

### Color

```rust
Color::WHITE, Color::BLACK, Color::RED, Color::GREEN, Color::BLUE
Color::hex(0xFF8000)          // RGB
Color::hex(0xFF800080)        // RGBA
Color::rgb(255, 128, 0)
Color::new(255, 128, 0, 128)   // RGBA
color.with_alpha(128)         // u8 alpha (0-255), returns new color
```

### Rect

```rust
Rect::new(x, y, width, height)
Rect::centered_at(cx, cy, width, height)
rect.contains(px, py)         // Hit testing
rect.center()                 // (cx, cy)
rect.shrink(padding)          // Inset all edges
rect.expand(padding)          // Expand all edges
```

### Anchor

```rust
Anchor::TopLeft, Anchor::TopCenter, Anchor::TopRight
Anchor::CenterLeft, Anchor::Center, Anchor::CenterRight
Anchor::BottomLeft, Anchor::BottomCenter, Anchor::BottomRight
```

---

## Theme System

Centralized styling with optional hot-reload:

```rust
let theme = Theme::default();   // or Theme::dark()
let theme = Theme::light();
let theme = Theme::load("assets/theme.ron")?;   // RON file

// Hot-reload from RON file (created with defaults if missing; reloads in debug builds only)
let hot = HotReloadTheme::new("assets/theme.ron")?;
hot.poll_reload();
let theme = hot.get();
```

`UiContext::with_theme(..)` builds a context that uses a theme. The `UiContext` passed to `render_ui()` has no theme set, so `draw_themed_*` there use default colors. All widget styles configurable:
`ButtonStyle`, `PanelStyle`, `ProgressBarStyle`, `SliderStyle`, `CheckboxStyle`, 
`ToggleStyle`, `TextInputStyle`, `LabelStyle`, `ScrollbarStyle`, `TooltipStyle`

---

## Immediate Mode Drawing

### Text

```rust
ui.draw_text("Hello", x, y, Color::WHITE);
ui.draw_text_anchored("Score: 100", Anchor::TopRight, -10.0, 10.0, Color::YELLOW);
ui.draw_text_centered("Title", rect, Color::WHITE);
ui.draw_text_wrapped("Long text...", x, y, max_width, Color::WHITE);  // max_width: u32
ui.draw_text_wrapped_centered("Long text...", rect, Color::WHITE);

// Text measurement
let (w, h) = ui.text_size("Hello");
let lines = ui.wrap_text("Long text...", max_width);  // Vec<String>
let (w, h) = ui.wrapped_text_size("Long text...", max_width);
```

### Rectangles

```rust
ui.draw_rect(rect, Color::RED);
ui.draw_rect_outline(rect, Color::WHITE, 2.0);
ui.draw_panel(rect, Color::DARK_GRAY, Some(Color::WHITE));
ui.draw_themed_panel(rect);  // Uses theme
```

### Progress Bars

```rust
ui.draw_progress_bar(rect, 0.75, Color::RED, Color::DARK_GRAY);
ui.draw_progress_bar_with_border(rect, 0.75, fg, bg, border);
ui.draw_themed_progress_bar(rect, 0.75);  // Uses theme
```

### Sprites

```rust
ui.draw_sprite(&sprite, x, y);
ui.draw_sprite_scaled(&sprite, x, y, scale_x, scale_y);
ui.draw_sprite_handle(&handle, x, y);
ui.draw_sprite_handle_scaled(&handle, x, y, scale_x, scale_y);
```

---

## Interactive Widgets

### Button

```rust
if ui.button("Start Game", Rect::new(100.0, 200.0, 150.0, 40.0)) {
    self.start_game();
}

// With custom colors
if ui.button_styled("Custom", rect, normal_bg, hover_bg, pressed_bg, text_color) { ... }
```

### Slider

```rust
if ui.slider(rect, &mut self.volume, 0.0, 100.0) {
    // Value changed
}
```

### Checkbox

```rust
if ui.checkbox(rect, &mut self.enabled, Some("Enable Sound")) {
    // Toggled
}
```

### Toggle Switch

```rust
if ui.toggle(rect, &mut self.on) {
    // Toggled
}
```

---

## Retained Mode Widgets

Stateful widgets that persist across frames.

### Text Input

```rust
// In your struct
username: TextInputState,

// In new()
username: TextInputState::with_text("default"),

// In render_ui()
if ui.text_input(&mut self.username, rect, "Username", ui.dt()) {
    // Text changed
}

// Access the text
let text = self.username.text();
```

Features: cursor, selection, clipboard (Ctrl+C/V/X), Shift+arrows, Home/End

### Dropdown

```rust
// In your struct
difficulty: DropdownState,

// In new()
difficulty: DropdownState::with_selected(1),

// In render_ui()
let options = &["Easy", "Normal", "Hard"];
if ui.dropdown(&mut self.difficulty, rect, options) {
    let selected_idx = self.difficulty.selected();
}

// IMPORTANT: Call at end of render_ui to draw open dropdowns on top
ui.finish_dropdowns();
```

### Scroll View

```rust
// In your struct
scroll: ScrollViewState,

// Vertical scrolling
ui.scroll_view(&mut self.scroll, visible_rect, content_height, ui.dt(), |ui, offset| {
    for (i, item) in items.iter().enumerate() {
        let y = i as f32 * 30.0 - offset;
        ui.draw_text(item, 10.0, y, Color::WHITE);
    }
});

// Horizontal scrolling
ui.scroll_view_horizontal(&mut self.scroll, rect, content_width, ui.dt(), |ui, offset| {
    // Draw content with x offset
});
```

---

## Layout Containers

### HBox (Horizontal Layout)

```rust
let mut hbox = HBox::new(start_x, start_y, spacing).with_valign(VAlign::Center);

// next(width, height) returns the Rect for the next widget
ui.button("One", hbox.next(80.0, 30.0));
ui.button("Two", hbox.next(80.0, 30.0));

// Get total bounds
let bounds = hbox.bounds();
```

### VBox (Vertical Layout)

```rust
let mut vbox = VBox::new(start_x, start_y, spacing).with_halign(HAlign::Center);

// next(width, height) returns the Rect for the next widget
ui.button("Option 1", vbox.next(200.0, 30.0));
```

### Stack (Overlay/Layered)

```rust
let (sw, sh) = ui.screen_size();
let stack = Stack::new(0.0, 0.0, sw as f32, sh as f32);  // or Stack::from_rect(rect)

// Split into two rects
let (header, body) = stack.split_vertical(50.0);       // top height
let (sidebar, content) = stack.split_horizontal(200.0); // left width

// Centered element
let button_rect = stack.centered(200.0, 50.0);

// Aligned element
let panel = stack.aligned(150.0, 100.0, HAlign::Right, VAlign::Top);

// Padded region
let padded = stack.padded(20.0);
```

---

## Clipping

Restrict rendering to a rectangular region:

```rust
ui.push_clip(Rect::new(10.0, 10.0, 200.0, 100.0));
// Everything drawn here is clipped to that rect
ui.draw_text("This will be clipped", 5.0, 5.0, Color::WHITE);
ui.pop_clip();

// Nested clips intersect automatically
ui.push_clip(outer_rect);
ui.push_clip(inner_rect);  // Uses intersection of both
ui.pop_clip();
ui.pop_clip();

// Temporarily bypass clipping (for dropdowns, tooltips)
ui.with_no_clip(|ui| {
    ui.draw_rect(fullscreen_rect, Color::BLACK);
});

// Query clip state
if ui.has_clip() { ... }
let depth = ui.clip_depth();
let current = ui.current_clip();
```

---

## Focus Management

### Tab Navigation

```rust
// For text inputs and dropdowns
ui.handle_tab_navigation(&mut [
    &mut self.username,
    &mut self.password,
    &mut self.email,
]);

// For dropdowns only
ui.handle_dropdown_tab_navigation(&mut [
    &mut self.dropdown1,
    &mut self.dropdown2,
]);
```

### Focus Ring

```rust
if self.username.is_focused() {
    ui.draw_focus_ring(rect, Color::BLUE, 2.0, 2.0);  // thickness, offset
}
ui.draw_focus_ring_themed(rect);  // Uses theme colors
```

### Focus Trap (for modals)

```rust
let mut trap = FocusTrap::new(&mut [
    &mut self.field1,
    &mut self.field2,
]);
trap.focus_first();  // Focus first widget
trap.focus_next();   // Tab forward
trap.focus_prev();   // Tab backward

// Check if any widget is capturing input
if trap.is_capturing() { ... }
```

---

## Modal Dialogs

```rust
if self.show_modal {
    // Dim background and clear clips
    ui.draw_modal_overlay(Color::new(0, 0, 0, 180));
    
    // Draw modal content
    let modal_rect = Rect::centered_at(sw/2.0, sh/2.0, 300.0, 200.0);
    ui.draw_themed_panel(modal_rect);
    ui.draw_text_centered("Confirm?", modal_rect, Color::WHITE);
    
    if ui.button("OK", ok_rect) { self.show_modal = false; }
    if ui.button("Cancel", cancel_rect) { self.show_modal = false; }
}
```

---

## Interaction Helpers

```rust
ui.is_hovered(rect)           // Mouse over rect
ui.is_clicked(rect)           // Left click this frame
ui.is_held(rect)              // Left button held down
ui.escape_consumed()          // Escape was pressed (e.g., by dropdown)
ui.click_consumed()           // Click was handled by a widget
ui.point_in_clip(x, y)        // Point within current clip
```

---

## Common Patterns

### HUD with Health/Score

```rust
fn render_ui(&mut self, ui: &mut UiContext) -> SceneAction {
    // Health bar top-left
    ui.draw_themed_progress_bar(
        Rect::new(10.0, 10.0, 200.0, 20.0),
        self.health / self.max_health,
    );
    
    // Score top-right
    ui.draw_text_anchored(
        &format!("Score: {}", self.score),
        Anchor::TopRight, -10.0, 10.0, Color::YELLOW,
    );

    SceneAction::Continue
}
```

### Pause Menu with VBox

```rust
if self.paused {
    let (sw, sh) = ui.screen_size();
    ui.draw_modal_overlay(Color::new(0, 0, 0, 180));
    
    let mut vbox = VBox::new(sw as f32 / 2.0 - 75.0, sh as f32 / 2.0 - 60.0, 10.0);
    
    ui.draw_text_centered("PAUSED", vbox.next(150.0, 40.0), Color::WHITE);
    
    if ui.button("Resume", vbox.next(150.0, 40.0)) {
        self.paused = false;
    }
    if ui.button("Settings", vbox.next(150.0, 40.0)) {
        self.show_settings = true;
    }
    if ui.button("Quit", vbox.next(150.0, 40.0)) {
        self.quit = true;
    }
}
```

### Settings Form

```rust
fn render_ui(&mut self, ui: &mut UiContext) -> SceneAction {
    let mut vbox = VBox::new(50.0, 50.0, 15.0);
    
    // Text inputs
    let y = vbox.next(200.0, 30.0).y;
    ui.draw_text("Username:", 50.0, y + 5.0, Color::WHITE);
    ui.text_input(&mut self.username, Rect::new(150.0, y, 200.0, 30.0), "", ui.dt());
    
    let y = vbox.next(200.0, 30.0).y;
    ui.draw_text("Volume:", 50.0, y + 5.0, Color::WHITE);
    ui.slider(Rect::new(150.0, y, 200.0, 20.0), &mut self.volume, 0.0, 100.0);
    
    let y = vbox.next(200.0, 30.0).y;
    ui.draw_text("Difficulty:", 50.0, y + 5.0, Color::WHITE);
    ui.dropdown(&mut self.difficulty, Rect::new(150.0, y, 150.0, 30.0), 
        &["Easy", "Normal", "Hard"]);
    
    // Tab navigation
    ui.handle_tab_navigation(&mut [&mut self.username]);
    
    // IMPORTANT: Draw dropdowns last
    ui.finish_dropdowns();

    SceneAction::Continue
}
```

---

## Tips

- UI is in screen space (pixels), not world space
- Call `finish_dropdowns()` at end of `render_ui()` when using dropdowns
- Use `draw_modal_overlay()` for modal backgrounds (clears clip stack)
- Theme widgets use theme styles automatically
- Retained widgets need state structs persisted across frames
- Tab navigation works with any `Focusable` widget

