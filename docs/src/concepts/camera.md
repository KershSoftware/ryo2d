# Camera System

The camera controls what portion of the game world is visible on screen.

## Basics

The camera has:
- **Position** - Where in the world the camera is centered
- **Zoom** - Scale factor (1.0 = normal, 2.0 = zoomed in, 0.5 = zoomed out)
- **Viewport** - Screen dimensions

```rust
// Set camera position
ctx.camera.set_position(100.0, 200.0);

// Set zoom level
ctx.camera.set_zoom(1.5);
```

## Following an Entity

Smooth camera follow with lerp:

```rust
fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    if let Some(player) = self.player {
        if let Some(transform) = ctx.world.get_component::<Transform>(player) {
            // 0.1 = smoothness (lower = smoother, higher = snappier)
            ctx.camera.follow(transform.x, transform.y, 0.1);
        }
    }
    SceneAction::Continue
}
```

## Coordinate Transforms

Convert between world and screen coordinates:

```rust
// World position → Screen position
let (screen_x, screen_y) = ctx.camera.world_to_screen(world_x, world_y);

// Screen position → World position (for mouse clicks)
let (world_x, world_y) = ctx.camera.screen_to_world(mouse_x, mouse_y);
```

## Visibility Culling

Check if something is visible:

```rust
// Is this rectangle visible on screen?
if ctx.camera.is_visible(x, y, width, height) {
    // Render it
}

// Get the visible world bounds
let bounds = ctx.camera.visible_bounds();
// bounds.min_x, bounds.min_y, bounds.max_x, bounds.max_y
```

## Zoom with Scroll Wheel

```rust
let scroll = ctx.input.scroll_delta();
if scroll.1 != 0.0 {
    let new_zoom = ctx.camera.zoom + scroll.1 * 0.1;
    ctx.camera.set_zoom(new_zoom);  // Clamped to 0.1 - 10.0
}
```
