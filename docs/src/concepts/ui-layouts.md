# UI Layout Containers

Ryo2D provides lightweight layout containers to simplify widget positioning. These are **rect generators** that calculate positions for you without managing widget state.

## Overview

| Container | Use Case |
|-----------|----------|
| `HBox` | Arrange widgets horizontally (toolbars, button rows) |
| `VBox` | Arrange widgets vertically (menus, settings panels) |
| `Stack` | Layer content within a bounded area (cards, modals) |

All containers work with the immediate-mode UI system. Call `.next()` to get a `Rect` for each widget.

## HBox - Horizontal Layout

Places widgets left-to-right with configurable spacing:

```rust,ignore
fn render_ui(&mut self, ui: &mut UiContext) {
    // Create a row starting at (20, 100) with 8px spacing
    let mut row = HBox::new(20.0, 100.0, 8.0);

    // Each .next() returns a Rect and advances the position
    ui.button("File", row.next(60.0, 30.0));
    ui.button("Edit", row.next(60.0, 30.0));
    ui.button("View", row.next(60.0, 30.0));

    // Mix different widget sizes
    ui.draw_text("Search:", row.next(50.0, 20.0).x, row.y() + 6.0, Color::WHITE);
    // ... text input would go here
}
```

### Vertical Alignment

Use `.with_valign()` to align items when they have different heights:

```rust,ignore
let mut row = HBox::new(20.0, 100.0, 10.0).with_valign(VAlign::Center);

// These will be vertically centered relative to each other
ui.checkbox(row.next(20.0, 20.0), &mut checked, None);  // 20px tall
ui.button("Apply", row.next(80.0, 30.0));               // 30px tall
ui.slider(row.next(100.0, 8.0), &mut value, 0.0, 1.0);  // 8px tall
```

### Tracking Bounds

HBox tracks the total area used:

```rust,ignore
let mut row = HBox::new(20.0, 100.0, 8.0);
ui.button("A", row.next(80.0, 30.0));
ui.button("B", row.next(80.0, 30.0));

// Draw a border around all buttons
let bounds = row.bounds();  // Returns Rect covering all items
ui.draw_rect_outline(bounds.expand(4.0), Color::WHITE, 1.0);

// Query current state
let total_width = row.width();
let max_height = row.height();
let next_x = row.current_x();
```

## VBox - Vertical Layout

Places widgets top-to-bottom:

```rust,ignore
let mut col = VBox::new(20.0, 100.0, 6.0);  // x, y, spacing

// Settings-style form
let r1 = col.next(300.0, 24.0);
ui.draw_text("Volume:", r1.x, r1.y + 4.0, Color::WHITE);
ui.slider(Rect::new(r1.x + 80.0, r1.y, 150.0, 8.0), &mut self.volume, 0.0, 1.0);

let r2 = col.next(300.0, 24.0);
ui.checkbox(Rect::new(r2.x, r2.y, 20.0, 20.0), &mut self.muted, Some("Mute Audio"));

// Buttons at bottom
col.next(300.0, 10.0);  // Spacer
ui.button("Save", col.next(100.0, 30.0));
ui.button("Cancel", col.next(100.0, 30.0));
```

### Horizontal Alignment

Use `.with_halign()` for items narrower than the allocated width:

```rust,ignore
let mut col = VBox::new(100.0, 50.0, 8.0).with_halign(HAlign::Center);

// Center-aligned buttons in a 200px wide column
ui.button("New Game", col.next(200.0, 35.0));
ui.button("Load Game", col.next(200.0, 35.0));
ui.button("Settings", col.next(200.0, 35.0));
```

## Stack - Layered Layout

Positions content within a fixed bounding rectangle. Perfect for cards, panels, and modals:

```rust,ignore
// Create a 300x200 panel
let stack = Stack::new(100.0, 100.0, 300.0, 200.0);

// Background fills the entire area
ui.draw_rect(stack.full(), Color::rgb(30, 30, 40));

// Content with 10px padding
let content = stack.padded(10.0);
ui.draw_text("Panel Title", content.x, content.y, Color::WHITE);

// Button aligned to bottom-right corner
let btn = stack.aligned(80.0, 30.0, HAlign::Right, VAlign::Bottom);
ui.button("OK", btn);
```

### Stack Methods

| Method | Returns |
|--------|---------|
| `.full()` | The entire bounding rect |
| `.padded(p)` | Rect inset by `p` on all sides |
| `.padded_xy(h, v)` | Rect inset by `h` horizontally, `v` vertically |
| `.centered(w, h)` | Centered rect of size `w` x `h` |
| `.aligned(w, h, halign, valign)` | Aligned rect within bounds |
| `.aligned_with_padding(w, h, ha, va, pad)` | Aligned with edge padding |
| `.split_horizontal(w)` | Returns `(left, right)` rects |
| `.split_vertical(h)` | Returns `(top, bottom)` rects |

### Building Card Layouts

```rust,ignore
let card = Stack::new(50.0, 50.0, 250.0, 180.0);

// Draw card background
ui.draw_panel(card.full(), Color::rgb(40, 40, 55), Some(Color::rgb(70, 70, 90)));

// Split into header and content
let (header, content) = card.split_vertical(35.0);
ui.draw_rect(header, Color::rgb(50, 50, 70));
ui.draw_text_centered("Card Title", header, Color::WHITE);

// Padded content area
let body = Stack::from_rect(content).padded(10.0);
ui.draw_text("Card content goes here...", body.x, body.y, Color::LIGHT_GRAY);

// Footer button
let btn = Stack::from_rect(content).aligned_with_padding(
    70.0, 25.0,
    HAlign::Right, VAlign::Bottom,
    8.0,
);
ui.button("Action", btn);
```

## Nesting Layouts

Combine containers for complex UIs:

```rust,ignore
// Outer panel
let panel = Stack::new(20.0, 20.0, 400.0, 300.0);
ui.draw_panel(panel.full(), Color::rgb(30, 30, 40), Some(Color::rgb(60, 60, 80)));

// Content with padding
let content = panel.padded(15.0);

// Vertical layout inside
let mut col = VBox::new(content.x, content.y, 10.0);

// Each row is an HBox
let row1 = col.next(content.width, 30.0);
let mut r1 = HBox::new(row1.x, row1.y, 8.0);
ui.draw_text("Name:", r1.next(50.0, 20.0).x, row1.y + 6.0, Color::WHITE);
// ... text input widget

let row2 = col.next(content.width, 30.0);
let mut r2 = HBox::new(row2.x, row2.y, 8.0);
ui.button("Save", r2.next(80.0, 28.0));
ui.button("Cancel", r2.next(80.0, 28.0));
```

## Alignment Enums

```rust,ignore
// Horizontal alignment
HAlign::Left    // Align to left edge
HAlign::Center  // Center horizontally
HAlign::Right   // Align to right edge

// Vertical alignment
VAlign::Top     // Align to top edge
VAlign::Center  // Center vertically
VAlign::Bottom  // Align to bottom edge
```

## Clipping

The UI context supports **scissor rect clipping** for scroll regions and overflow handling. Use `push_clip()` and `pop_clip()` to manage a stack of clip rects:

```rust,ignore
// Create a scroll region
let scroll_area = Rect::new(10.0, 10.0, 200.0, 150.0);
ui.draw_panel(scroll_area, bg_color, Some(border_color));

// Push clip - all drawing is now clipped to this rect
ui.push_clip(scroll_area.shrink(5.0));

// Draw scrollable content
for (i, item) in items.iter().enumerate() {
    let y = 15.0 + i as f32 * 20.0 - scroll_offset;
    ui.draw_text(item, 15.0, y, Color::WHITE);  // Clipped!
}

// Restore previous clip state
ui.pop_clip();
```

### Clip Stack Methods

| Method | Description |
|--------|-------------|
| `push_clip(rect)` | Push a clip rect onto the stack |
| `pop_clip()` | Restore previous clip state |
| `current_clip()` | Get the effective clip rect (intersection of all) |
| `has_clip()` | Check if any clip is active |
| `clip_depth()` | Get number of clips on stack |
| `clear_clips()` | Remove all clips |
| `with_no_clip(f)` | Run closure without clips, then restore |

### Nested Clips

Nested clips compute the **intersection** of all stacked rects:

```rust,ignore
ui.push_clip(Rect::new(10.0, 10.0, 100.0, 100.0));  // Clip A
ui.push_clip(Rect::new(50.0, 50.0, 100.0, 100.0));  // Clip B

// Effective clip is the intersection: (50, 50) to (110, 110) → 60x60
ui.draw_rect(...);  // Only draws in the intersection

ui.pop_clip();  // Back to clip A only
ui.pop_clip();  // No clip
```

## Modal Overlays

For modal dialogs, use `draw_modal_overlay()` to dim the background:

```rust,ignore
if self.show_modal {
    // Dim everything behind the modal
    ui.draw_modal_overlay(Color::new(0, 0, 0, 180));

    // Draw the dialog
    let (w, h) = ui.screen_size();
    let dialog = Rect::centered_at(w as f32 / 2.0, h as f32 / 2.0, 300.0, 200.0);
    ui.draw_panel(dialog, Color::rgb(40, 40, 50), Some(Color::WHITE));
    ui.draw_text_centered("Confirm?", dialog, Color::WHITE);

    // Modal buttons
    if ui.button("OK", ok_rect) { self.show_modal = false; }
    if ui.button("Cancel", cancel_rect) { self.show_modal = false; }
}
```

The `draw_modal_overlay()` method temporarily clears all clips to draw over the entire screen, then restores the clip state.

## Z-Ordering

Z-ordering in immediate-mode UI is handled by **draw order**:

1. Draw background elements first
2. Draw foreground elements last
3. Modal overlays draw last (on top of everything)

```rust,ignore
fn render_ui(&mut self, ui: &mut UiContext) {
    // 1. Background
    ui.draw_rect(bg_rect, bg_color);

    // 2. Main content
    self.draw_main_ui(ui);

    // 3. Floating panels (on top of main content)
    self.draw_floating_panels(ui);

    // 4. Modal (on top of everything)
    if self.show_modal {
        ui.draw_modal_overlay(Color::new(0, 0, 0, 128));
        self.draw_modal_dialog(ui);
    }
}
```

## Tips

1. **Start simple** - Use raw `Rect::new()` for one-off elements, layouts for repeated patterns
2. **Track bounds** - Use `.bounds()` to draw backgrounds/borders after laying out content
3. **Stack for modals** - `Stack::from_rect(screen_rect).centered(w, h)` centers dialogs
4. **Spacing consistency** - Use the same spacing values across your UI for visual harmony
5. **VBox + HBox** - Combine for grid-like layouts without a full grid system
6. **Clip for scroll** - Always use `push_clip()`/`pop_clip()` for scrollable regions
7. **Draw order matters** - Draw modals and popups last to ensure they appear on top
