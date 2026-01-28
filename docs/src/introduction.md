# Ferrum2D

A simple, scriptable 2D game engine built in Rust.

## What is Ferrum2D?

Ferrum2D is a lightweight 2D game engine designed for simplicity and ease of use. It provides a clean API for building 2D games while leveraging Rust's safety and performance.

## Features

- **Simple API** - Implement the `Game` trait and you're ready to go
- **Entity-Component-System** - Flexible entity management with composable components
- **Camera System** - 2D camera with zoom, follow, and world-to-screen transforms
- **Sprite Rendering** - Load images, create solid colors, animated sprites
- **Input Handling** - Keyboard, mouse, and scroll wheel support
- **Fixed Timestep** - Consistent physics updates with variable rendering

## Quick Example

```rust
use ferrum2d::prelude::*;

struct MyGame;

impl Game for MyGame {
    fn init(&mut self, ctx: &mut GameContext) {
        let player = ctx.world.spawn();
        ctx.world.add_component(player, Transform::new(100.0, 100.0));
    }

    fn update(&mut self, ctx: &mut GameContext, dt: f32) {
        // Your game logic here
    }
}

fn main() {
    EngineBuilder::new()
        .title("My Game")
        .resolution(800, 600)
        .with_game(MyGame)
        .build()
        .run()
        .unwrap();
}
```

## Documentation

- **[Getting Started](./getting-started/quick-start.md)** - Installation and first steps
- **[Core Concepts](./concepts/game-loop.md)** - Understand how the engine works
- **[Examples](./examples/moving-player.md)** - Learn by example
- **[API Reference](api/ferrum2d/index.html)** - Complete API documentation
