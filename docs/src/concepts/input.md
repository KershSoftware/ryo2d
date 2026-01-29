# Input Handling

ryo2d provides keyboard and mouse input through the `GameContext`.

## Keyboard

```rust
fn update(&mut self, ctx: &mut GameContext, dt: f32) {
    // Is key currently held down?
    if ctx.input.key_down(KeyCode::Space) {
        // Fires every frame while held
    }
    
    // Was key just pressed this frame?
    if ctx.input.key_pressed(KeyCode::Enter) {
        // Fires once on press
    }
    
    // Was key just released this frame?
    if ctx.input.key_released(KeyCode::Escape) {
        // Fires once on release
    }
}
```

## Mouse Buttons

```rust
// Left click held
if ctx.input.mouse_down(MouseButton::Left) { }

// Just clicked
if ctx.input.mouse_pressed(MouseButton::Left) { }

// Just released
if ctx.input.mouse_released(MouseButton::Left) { }
```

## Mouse Position

```rust
// Screen coordinates
let (x, y) = ctx.input.mouse_position();

// Movement since last frame
let (dx, dy) = ctx.input.mouse_delta();
```

## Scroll Wheel

```rust
let (scroll_x, scroll_y) = ctx.input.scroll_delta();

// Common use: zoom
if scroll_y != 0.0 {
    let new_zoom = ctx.camera.zoom + scroll_y * 0.1;
    ctx.camera.set_zoom(new_zoom);
}
```

## Key Codes

Import `KeyCode` from the prelude:

```rust
use ryo2d::prelude::*;  // Includes KeyCode

// Common keys
KeyCode::KeyW, KeyCode::KeyA, KeyCode::KeyS, KeyCode::KeyD
KeyCode::ArrowUp, KeyCode::ArrowDown, KeyCode::ArrowLeft, KeyCode::ArrowRight
KeyCode::Space, KeyCode::Enter, KeyCode::Escape
```
