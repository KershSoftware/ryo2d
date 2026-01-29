# Project Configuration

Ryo2D uses a `ryo.toml` configuration file to define project settings. This file is optional but recommended for organizing your game project.

## The ryo.toml File

Create a `ryo.toml` file in your project root:

```toml
[project]
name = "My Awesome Game"

[paths]
entry_point = "src/main.rs"
save_data = "data"
assets = "assets"
```

### Configuration Options

#### [project]

| Field | Type | Description |
|-------|------|-------------|
| `name` | String | Display name for your project |

#### [paths]

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `entry_point` | String | Auto-detected | Path to main.rs (relative to project root) |
| `save_data` | String | `"data"` | Directory for save files and persistent data |
| `assets` | String | `"assets"` | Directory for game assets (sprites, sounds, etc.) |

## Entry Point Detection

If you don't specify an `entry_point`, Ryo2D will look for:

1. `./main.rs` (project root)
2. `./src/main.rs` (standard Rust layout)

## Workspace Discovery

The engine automatically discovers your workspace by looking for `ryo.toml` in the current directory and up to 3 parent directories. This is useful when running from subdirectories.

```rust
use ryo2d::prelude::*;

fn main() {
    // Discover workspace from current directory
    let workspace = Workspace::discover().expect("No workspace found");
    
    println!("Project: {}", workspace.config().name);
    println!("Root: {:?}", workspace.root());
    println!("Save data: {:?}", workspace.save_data_dir());
}
```

## Programmatic Access

You can access workspace configuration at runtime:

```rust
use ryo2d::prelude::*;

fn main() {
    let workspace = Workspace::discover().unwrap();
    let config = workspace.config();
    
    // Access configured paths
    let save_dir = workspace.save_data_dir();
    let assets_dir = workspace.root().join(&config.assets);
    
    // Use in your game
    EngineBuilder::new()
        .title(&config.name)
        .resolution(800, 600)
        .with_game(MyGame { workspace })
        .build()
        .run()
        .unwrap();
}
```

## Example Project Structure

```
my-game/
├── ryo.toml           # Project configuration
├── Cargo.toml         # Rust dependencies
├── src/
│   └── main.rs        # Game entry point
├── assets/
│   ├── sprites/
│   │   └── player.png
│   └── sounds/
│       └── jump.wav
└── data/              # Created automatically for save files
    └── save.json
```

## Multiple Examples

For projects with multiple games or demos, place each in the `examples/` directory with its own `ryo.toml`:

```
my-engine-project/
├── Cargo.toml
├── src/
│   └── lib.rs         # Shared library code
└── examples/
    ├── breakout/
    │   ├── main.rs
    │   └── ryo.toml   # name = "Breakout Demo"
    └── asteroids/
        ├── main.rs
        └── ryo.toml   # name = "Asteroids Clone"
```

Run examples with:

```bash
cargo run --example breakout
cargo run --example asteroids
```

## Next Steps

- [Your First Game](./first-game.md) - Build a simple game
- [Asset Management](../concepts/assets.md) - Load sprites and sounds
