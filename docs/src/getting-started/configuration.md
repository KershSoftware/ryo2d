# Project Configuration

Ryo2D uses a `Ryo.toml` configuration file to define project settings. This file is optional but recommended for organizing your game project.

## Configuration Hierarchy

Ryo2D uses a 3-tier configuration system:

```
system.json (user prefs) > Ryo.toml [system] (dev defaults) > Engine Defaults
```

| Source | Purpose | Location |
|--------|---------|----------|
| **Engine defaults** | Hardcoded fallback values | Built into engine |
| **Ryo.toml [system]** | Developer-specified defaults | Project root |
| **system.json** | User preferences (options menu) | `{save_dir}/system.json` |

**Key insight:** `[system]` and `[info]` sections define what goes into `system.json`. Other sections (`[project]`, `[paths]`, `[debug]`, `[env]`) are developer-only and never saved to user config.

## The Ryo.toml File

Create a `Ryo.toml` file in your project root:

```toml
[project]
name = "My Awesome Game"

[paths]
entry_point = "src/main.rs"
save_data = "data"
assets = "assets"

# User-facing defaults (saved to system.json)
[system]
resolution_width = 1280
resolution_height = 720
vsync = true
fullscreen = false
master_volume = 0.8
music_volume = 1.0
sfx_volume = 1.0

# User-facing info display (saved to system.json)
[info]
show_fps = false

# Developer-only debug settings (NOT saved to system.json)
[debug]
show_colliders = false
show_velocities = false
show_origins = false
```

## Section Reference

### [project] — Developer Only

| Field | Type | Description |
|-------|------|-------------|
| `name` | String | Display name for your project |

### [paths] — Developer Only

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `entry_point` | String | Auto-detected | Path to main.rs (relative to project root) |
| `save_data` | String | `"data"` | Directory for save files and persistent data |
| `assets` | String | `"assets"` | Directory for game assets (sprites, sounds, etc.) |

### [system] — User-Facing Defaults

These fields are used as defaults when creating `system.json`. Users can override them via an options menu.

| Field | Type | Engine Default | Description |
|-------|------|----------------|-------------|
| `resolution_width` | u32 | `1280` | Window/screen width |
| `resolution_height` | u32 | `720` | Window/screen height |
| `vsync` | bool | `true` | VSync enabled |
| `fullscreen` | bool | `false` | Fullscreen mode |
| `master_volume` | f32 | `0.8` | Master volume (0.0 - 1.0) |
| `music_volume` | f32 | `1.0` | Music volume (0.0 - 1.0) |
| `sfx_volume` | f32 | `1.0` | SFX volume (0.0 - 1.0) |

### [info] — User-Facing Display

| Field | Type | Engine Default | Description |
|-------|------|----------------|-------------|
| `show_fps` | bool | `false` | Show FPS counter overlay |

### [debug] — Developer Only

These are for development only and are **not** saved to `system.json`.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `show_colliders` | bool | `false` | Render physics collider outlines |
| `show_velocities` | bool | `false` | Render velocity vectors |
| `show_origins` | bool | `false` | Render entity origin points |

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
    
    // Resolution comes from system.json > Ryo.toml > engine defaults
    // No need to hardcode .resolution() in code
    EngineBuilder::new()
        .title(&config.name)
        .with_system_state_overrides()  // Loads user prefs from system.json
        .with_game(MyGame { workspace })
        .build()
        .run()
        .unwrap();
}
```

## Runtime Screen Size API

During gameplay, access the current screen dimensions through the context:

```rust
impl Scene for MyScene {
    fn update(&mut self, ctx: &mut SceneContext) {
        // Get current screen dimensions
        let (width, height) = ctx.screen_size();
        
        // Or individual dimensions
        let w = ctx.screen_width();
        let h = ctx.screen_height();
        
        // Use for positioning, spawning, etc.
        let center_x = w / 2.0;
        let center_y = h / 2.0;
    }
    
    fn render_ui(&mut self, ui: &mut UiContext) {
        // Also available in UI context
        let (w, h) = ui.screen_size();
    }
}
```

**Best practice:** Define your design resolution in `Ryo.toml [system]`, then use runtime API for positioning. This keeps resolution configuration in one place and allows users to customize it via `system.json`.

## Example Project Structure

```
my-game/
├── Ryo.toml           # Project configuration
├── Cargo.toml         # Rust dependencies
├── src/
│   └── main.rs        # Game entry point
├── assets/
│   ├── sprites/
│   │   └── player.png
│   └── sounds/
│       └── jump.wav
└── data/              # Created automatically for save files
    └── system.json    # User preferences (auto-created)
```

## Multiple Examples

For projects with multiple games or demos, place each in the `examples/` directory with its own `Ryo.toml`:

```
my-engine-project/
├── Cargo.toml
├── src/
│   └── lib.rs         # Shared library code
└── examples/
    ├── breakout/
    │   ├── main.rs
    │   └── Ryo.toml   # name = "Breakout Demo"
    └── asteroids/
        ├── main.rs
        └── Ryo.toml   # name = "Asteroids Clone"
```

Run examples with:

```bash
cargo run --example breakout
cargo run --example asteroids
```

## See Also

- [System Configuration](../concepts/system-configuration.md) - Detailed system.json reference
- [Your First Game](./first-game.md) - Build a simple game
- [Asset Management](../concepts/assets.md) - Load sprites and sounds
