# Quick Start

## Installation

Add Ferrum2D to your `Cargo.toml`:

```toml
[dependencies]
ferrum2d = { git = "https://github.com/Kersheh/ferrum2d" }
```

## Minimal Example

Create a new Rust project and add this to `src/main.rs`:

```rust
use ferrum2d::prelude::*;

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

## Next Steps

- [Your First Game](./first-game.md) - Add a player you can move around
- [Core Concepts](../concepts/game-loop.md) - Understand the engine architecture
