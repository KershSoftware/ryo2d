# Retained Mode Widgets

While most of Ryo2D's UI is immediate-mode (call each frame, no state), some widgets need to track state across frames. These **retained-mode widgets** require you to store a state object and pass it each frame.

## When to Use Retained Mode

| Widget | Why Retained? |
|--------|--------------|
| **Text Input** | Cursor position, selection, blink timer, text buffer |
| **Scroll View** | Scroll offset, momentum, drag state |
| **Dropdown** | Open/closed state, selected index, keyboard navigation |

The pattern is simple: create a `*State` struct in your scene, pass it to the widget function each frame.

## Text Input

A text field with cursor, selection, and clipboard support.

### Basic Usage

```rust,ignore
struct MyScene {
    username: TextInputState,
}

impl MyScene {
    fn new() -> Self {
        Self {
            username: TextInputState::new(),
        }
    }
}

impl Scene for MyScene {
    fn render_ui(&mut self, ui: &mut UiContext, dt: f32) {
        let rect = Rect::new(100.0, 100.0, 200.0, 30.0);
        
        // Returns true if text changed this frame
        if ui.text_input(&mut self.username, rect, "Enter name...", dt) {
            println!("Text: {}", self.username.text());
        }
    }
}
```

### Features

- **Cursor navigation**: Arrow keys, Home/End
- **Selection**: Shift+arrows, Ctrl+A (select all)
- **Editing**: Type, backspace, delete
- **Clipboard**: Ctrl+C (copy), Ctrl+X (cut), Ctrl+V (paste)
- **IME support**: International input methods work automatically
- **Blinking cursor**: Visual feedback when focused

### State Methods

```rust,ignore
let state = TextInputState::new();
let state = TextInputState::with_text("initial value");

// Reading
state.text()           // Get current text
state.cursor()         // Cursor byte position
state.is_focused()     // Whether input has focus
state.has_selection()  // Whether text is selected
state.selected_text()  // Get selected text (if any)

// Modifying
state.set_text("new text");   // Replace all text
state.insert("text");         // Insert at cursor
state.select_all();           // Select all text
state.clear_selection();      // Deselect

// Clipboard
state.copy();   // Copy selection to clipboard
state.cut();    // Cut selection to clipboard
state.paste();  // Paste from clipboard
```

### Click Positioning

For mouse-based cursor positioning, use `set_cursor_char()`:

```rust,ignore
// Calculate character index from mouse position
let char_index = calculate_char_at_x(mouse_x, rect, state.text(), font);
state.set_cursor_char(char_index, input.key_down(KeyCode::ShiftLeft));
```

## Scroll View

A scrollable container for content larger than its visible area.

### Basic Usage

```rust,ignore
struct MyScene {
    scroll: ScrollViewState,
}

impl Scene for MyScene {
    fn render_ui(&mut self, ui: &mut UiContext, dt: f32) {
        let view_rect = Rect::new(20.0, 50.0, 300.0, 400.0);
        let content_height = 800.0; // Total height of all content
        
        ui.scroll_view(
            &mut self.scroll,
            view_rect,
            content_height,
            |ui, scroll_offset| {
                // Draw content - subtract scroll_offset from y positions
                for i in 0..20 {
                    let y = i as f32 * 35.0 - scroll_offset;
                    ui.draw_text(&format!("Item {}", i), 10.0, y, Color::WHITE);
                }
            }
        );
    }
}
```

### Features

- **Mouse wheel scrolling**: Scroll content with mouse wheel
- **Scrollbar**: Visual indicator and click-drag support
- **Momentum**: Smooth deceleration after scroll
- **Clipping**: Content outside the view is automatically clipped

### State Methods

```rust,ignore
let state = ScrollViewState::new();
let state = ScrollViewState::with_scroll(0.0, 100.0); // Start scrolled down

// Reading
state.scroll_y()    // Current vertical scroll offset

// Modifying
state.set_scroll(100.0);    // Jump to position
state.scroll_to_top();      // Scroll to top
state.scroll_to_bottom(content_height, view_height); // Scroll to bottom
```

## Dropdown

A select box for choosing from a list of options.

### Basic Usage

```rust,ignore
struct MyScene {
    difficulty: DropdownState,
}

impl Scene for MyScene {
    fn render_ui(&mut self, ui: &mut UiContext, dt: f32) {
        let options = ["Easy", "Medium", "Hard"];
        let rect = Rect::new(100.0, 100.0, 200.0, 30.0);
        
        // Returns true when selection changes
        if ui.dropdown(&mut self.difficulty, rect, &options) {
            let selected = options[self.difficulty.selected()];
            println!("Difficulty: {}", selected);
        }
    }
}
```

### Features

- **Click to open**: Click the dropdown to show options
- **Keyboard navigation**: Arrow keys to move, Enter to select, Escape to close
- **Click outside to close**: Clicking elsewhere dismisses the dropdown
- **Hover highlight**: Visual feedback on hovered option

### State Methods

```rust,ignore
let state = DropdownState::new();
let state = DropdownState::with_selected(1); // Start with "Medium" selected

// Reading
state.selected()    // Currently selected index
state.is_open()     // Whether dropdown list is visible

// Modifying
state.set_selected(2);  // Change selection
state.close();          // Close the dropdown
```

## Creating Custom Retained Widgets

The pattern for retained widgets is straightforward:

1. **Create a state struct** that holds all persistent data
2. **Implement methods** for reading and modifying state
3. **Add a render method** on `UiContext` that takes `&mut State`

```rust,ignore
// 1. State struct
pub struct MyWidgetState {
    value: f32,
    dragging: bool,
}

impl MyWidgetState {
    pub fn new() -> Self {
        Self { value: 0.0, dragging: false }
    }
    
    pub fn value(&self) -> f32 { self.value }
}

// 2. In your scene
struct MyScene {
    widget: MyWidgetState,
}

// 3. Usage in render_ui
fn render_ui(&mut self, ui: &mut UiContext, dt: f32) {
    // Handle input, update state, draw visuals
    let rect = Rect::new(100.0, 100.0, 200.0, 30.0);
    
    // Check mouse interaction
    if rect.contains(ui.mouse_pos()) && ui.mouse_pressed(MouseButton::Left) {
        self.widget.dragging = true;
    }
    
    // ... update and render logic
}
```

The key insight: retained widgets are just immediate-mode rendering with external state storage. There's no widget tree or lifecycle—you manage the state, the render function draws it.
