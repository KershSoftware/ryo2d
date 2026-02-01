# Focus & Keyboard Navigation

Ryo2D provides a lightweight focus system for keyboard navigation in UIs. This enables Tab/Shift+Tab navigation between widgets, focus rings for visual feedback, and focus traps for modal dialogs.

## Overview

| Feature | Description |
|---------|-------------|
| **Tab Navigation** | Move between widgets with Tab/Shift+Tab |
| **Focus Ring** | Visual indicator showing which widget has focus |
| **Focus Trap** | Contain Tab navigation within a modal dialog |
| **Focusable Trait** | Common interface for focusable widgets |

## The Focusable Trait

Any widget that can receive keyboard focus implements the `Focusable` trait:

```rust,ignore
pub trait Focusable {
    fn is_focused(&self) -> bool;
    fn set_focused(&mut self, focused: bool);
    fn is_capturing(&self) -> bool { false }
    fn on_focus(&mut self) {}
    fn on_blur(&mut self) {}
}
```

Built-in implementations:
- `TextInputState` - Text input fields
- `DropdownState` - Dropdown selectors

## Tab Navigation

Use `handle_tab_navigation()` to enable Tab/Shift+Tab navigation across different widget types:

```rust,ignore
struct MyScene {
    username: TextInputState,
    password: TextInputState,
    role: DropdownState,
}

fn render_ui(&mut self, ui: &mut UiContext, dt: f32) {
    // Draw widgets
    ui.text_input(&mut self.username, rect1, "Username", dt);
    ui.text_input(&mut self.password, rect2, "Password", dt);
    ui.dropdown(&mut self.role, rect3, &["User", "Admin", "Guest"]);
    ui.finish_dropdowns();

    // Enable Tab navigation across all widgets
    ui.handle_tab_navigation(&mut [
        &mut self.username,
        &mut self.password,
        &mut self.role,
    ]);
}
```

**Behavior:**
- **Tab**: Move to next widget (wraps at end)
- **Shift+Tab**: Move to previous widget (wraps at start)
- Navigation pauses when a widget is "capturing" (e.g., open dropdown handles arrow keys)

## Focus Rings

Draw visual focus indicators with `draw_focus_ring()`:

```rust,ignore
// Manual focus ring with custom styling
if self.submit_focused {
    ui.draw_focus_ring(
        button_rect,
        Color::rgb(100, 150, 255),  // Color
        2.0,                         // Thickness
        2.0,                         // Offset from widget
    );
}
ui.button("Submit", button_rect);

// Or use theme colors automatically
if self.username.is_focused() {
    ui.draw_focus_ring_themed(username_rect);
}
```

**Parameters:**
- `rect`: Widget bounds
- `color`: Ring color (typically a bright accent)
- `thickness`: Ring width in pixels
- `offset`: Gap between widget and ring

## Focus Trap for Modals

When displaying modal dialogs, use `FocusTrap` to keep Tab navigation within the modal:

```rust,ignore
use ryo2d::prelude::*;

struct ConfirmDialog {
    message_input: TextInputState,
    confirm_focused: bool,
    cancel_focused: bool,
}

impl ConfirmDialog {
    fn render(&mut self, ui: &mut UiContext, dt: f32) {
        // Draw modal overlay
        ui.draw_modal_overlay(Color::rgba(0, 0, 0, 180));

        // Draw dialog panel
        let dialog_rect = Rect::new(200.0, 150.0, 400.0, 200.0);
        ui.draw_panel(dialog_rect, Color::rgb(40, 40, 50), Some(Color::WHITE));

        // Draw widgets
        ui.text_input(&mut self.message_input, input_rect, "Enter message", dt);
        
        // Focus rings
        if self.message_input.is_focused() {
            ui.draw_focus_ring_themed(input_rect);
        }

        // Create focus trap for the dialog
        // This ensures Tab only cycles within these widgets
        let mut trap = FocusTrap::new(&mut [
            &mut self.message_input,
            // Add button focusable states here
        ]);

        // If nothing focused, focus first widget
        if trap.focused_index().is_none() {
            trap.focus_first();
        }
    }
}
```

### FocusTrap Methods

```rust,ignore
let mut trap = FocusTrap::new(&mut widgets);

trap.focused_index()  // Get index of focused widget (if any)
trap.focus_first()    // Focus the first widget
trap.focus_last()     // Focus the last widget
trap.focus_next()     // Move focus forward (wrapping)
trap.focus_prev()     // Move focus backward (wrapping)
trap.is_capturing()   // Check if any widget is capturing input
```

## Dropdown-Specific Navigation

The existing `handle_dropdown_tab_navigation()` is still available for dropdown-only navigation with additional features like Space/Enter to open:

```rust,ignore
// For dropdowns only, with Space/Enter support
ui.handle_dropdown_tab_navigation(&mut [
    &mut self.dropdown1,
    &mut self.dropdown2,
]);

// For mixed widget types
ui.handle_tab_navigation(&mut [
    &mut self.text_input,
    &mut self.dropdown,
]);
```

## Creating Custom Focusable Widgets

Implement `Focusable` for your own widgets:

```rust,ignore
struct CustomButton {
    focused: bool,
    label: String,
}

impl Focusable for CustomButton {
    fn is_focused(&self) -> bool {
        self.focused
    }

    fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    // Optional: block Tab navigation when widget needs exclusive input
    fn is_capturing(&self) -> bool {
        false
    }

    // Optional: called when widget gains focus
    fn on_focus(&mut self) {
        // Play sound, start animation, etc.
    }

    // Optional: called when widget loses focus
    fn on_blur(&mut self) {
        // Cleanup, validation, etc.
    }
}
```

## Best Practices

1. **Always provide visual feedback** - Draw focus rings so users know which widget is active

2. **Use logical tab order** - Order widgets in `handle_tab_navigation()` as users would expect (top-to-bottom, left-to-right)

3. **Trap modal focus** - Use `FocusTrap` for dialogs to prevent Tab from escaping

4. **Handle Enter/Space** - Focused buttons should respond to Enter/Space as activation

5. **Consider initial focus** - When a screen loads, set focus on the first interactive element:

```rust,ignore
fn on_enter(&mut self) {
    self.first_input.set_focused(true);
}
```

## Example: Login Form

```rust,ignore
struct LoginScene {
    username: TextInputState,
    password: TextInputState,
    remember_me: bool,
    submit_focused: bool,
}

fn render_ui(&mut self, ui: &mut UiContext, dt: f32) {
    // Username field
    let username_rect = Rect::new(100.0, 100.0, 200.0, 30.0);
    if self.username.is_focused() {
        ui.draw_focus_ring_themed(username_rect);
    }
    ui.text_input(&mut self.username, username_rect, "Username", dt);

    // Password field
    let password_rect = Rect::new(100.0, 140.0, 200.0, 30.0);
    if self.password.is_focused() {
        ui.draw_focus_ring_themed(password_rect);
    }
    ui.text_input(&mut self.password, password_rect, "Password", dt);

    // Remember me checkbox
    let checkbox_rect = Rect::new(100.0, 180.0, 20.0, 20.0);
    ui.checkbox(checkbox_rect, &mut self.remember_me, Some("Remember me"));

    // Submit button
    let submit_rect = Rect::new(100.0, 220.0, 100.0, 35.0);
    if self.submit_focused {
        ui.draw_focus_ring_themed(submit_rect);
    }
    if ui.button("Login", submit_rect) {
        self.submit_login();
    }

    // Tab navigation
    ui.handle_tab_navigation(&mut [
        &mut self.username,
        &mut self.password,
    ]);
}
```
