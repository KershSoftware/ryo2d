# Ryo2D

A simple, scriptable 2D game engine built in Rust.

## What is Ryo2D?

Ryo2D is a lightweight 2D game engine designed for simplicity and ease of use. It provides a clean API for building 2D games while leveraging Rust's safety and performance.

## Features

- **Simple API** - Implement the `Scene` trait and you're ready to go
- **Entity-Component-System** - Flexible entity management with composable components
- **Camera System** - 2D camera with zoom, follow, and world-to-screen transforms
- **Sprite Rendering** - Load images, create solid colors, animated sprites
- **Input Handling** - Keyboard, mouse, and scroll wheel support
- **Fixed Timestep** - Consistent physics updates with variable rendering

## Quick Example

```rust
use ryo2d::prelude::*;

struct MyScene;

impl Scene for MyScene {
    fn on_enter(&mut self, ctx: &mut SceneContext) {
        ctx.world.spawn((Transform::new(100.0, 100.0),));
    }

    fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
        // Your game logic here
        SceneAction::Continue
    }
}

fn main() {
    EngineBuilder::new()
        .title("My Game")
        .resolution(800, 600)
        .start_scene(MyScene)
        .build()
        .run()
        .unwrap();
}
```

## Documentation

- **[Getting Started](./getting-started/quick-start.md)** - Installation and first steps
- **[Core Concepts](./concepts/game-loop.md)** - Understand how the engine works
- **[Examples](./examples/moving-player.md)** - Learn by example
- **[API Reference](api/ryo2d/index.html)** - Complete API documentation
