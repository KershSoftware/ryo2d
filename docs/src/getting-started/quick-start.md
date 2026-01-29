# Quick Start

## Installation

Add Ryo2D to your `Cargo.toml`:

```toml
[dependencies]
ryo2d = { git = "https://github.com/Kersheh/ryo2d" }
```

## Minimal Example

Create a new Rust project and add this to `src/main.rs`:

```rust
use ryo2d::prelude::*;

struct MyGame;

impl Game for MyGame {
    fn init(&mut self, _ctx: &mut GameContext) {
        println!("Game initialized!");
    }

    fn update(&mut self, _ctx: &mut GameContext, _dt: f32) {
        // Game logic runs here
    }
}

fn main() {
    EngineBuilder::new()
        .title("My First Game")
        .resolution(800, 600)
        .with_game(MyGame)
        .build()
        .run()
        .unwrap();
}
```

Run with:

```bash
cargo run
```

You should see an 800x600 window with a dark blue background!

## Project Configuration (Optional)

For larger projects, create a `ryo.toml` to configure paths and project settings:

```toml
[project]
name = "My First Game"

[paths]
save_data = "data"
assets = "assets"
```

See [Project Configuration](./configuration.md) for full details.

## Next Steps

- [Project Configuration](./configuration.md) - Configure your project structure
- [Your First Game](./first-game.md) - Add a player you can move around
- [Core Concepts](../concepts/game-loop.md) - Understand the engine architecture
