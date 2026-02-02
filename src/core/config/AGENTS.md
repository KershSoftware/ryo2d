# AGENTS.md - Config Module

## Overview

The `config/` module handles project configuration, save/load systems, and debug visualization.

## File Structure

| File | Purpose |
|------|---------|
| `mod.rs` | Module exports and re-exports |
| `workspace.rs` | `Workspace`, `WorkspaceConfig` - Ryo.toml parsing |
| `save.rs` | `SaveManager` - 3-tier save system with compression |
| `debug.rs` | `DebugConfig`, debug visualization functions |

## Key Types

### Workspace

Project discovery and configuration:

```rust
// Auto-discover from current directory
let workspace = Workspace::discover()?;

// Or from explicit path
let workspace = Workspace::from_path("./my_game")?;

// Access configuration
let name = workspace.name();
let assets_dir = workspace.assets_dir();
let config = workspace.config();

// System defaults from [system] section
let defaults = &config.system;
println!("Resolution: {}x{}", defaults.resolution_width, defaults.resolution_height);
```

### WorkspaceConfig

Parsed `Ryo.toml` structure:

```toml
[project]
name = "My Game"

[paths]
entry_point = "src/main.rs"
assets = "assets"
save_data = "data"

[system]  # Defaults for system.json
resolution_width = 1920
resolution_height = 1080
vsync = true
fullscreen = false

[debug]  # Developer flags
show_colliders = false
show_fps = true

[info]
show_fps = true
```

### SaveManager

3-tier save system:

```rust
// Create from workspace
let save_manager = workspace.create_save_manager()?;

// 1. System state (settings, preferences)
let system = save_manager.load_or_create_system(&defaults)?;
save_manager.save_system(&system)?;

// 2. World state (ECS snapshot)
let world_state = WorldState::from_world(&world);
save_manager.save_world("slot1", &world_state)?;
let loaded = save_manager.load_world::<MyComponent>("slot1")?;

// 3. Game state (custom data)
let game_data: GameSaveData<MyState> = GameSaveData {
    metadata: SaveMetadata::new(),
    data: my_state,
};
save_manager.save_game("slot1", &game_data)?;
```

**Save tiers:**
| Tier | File | Purpose |
|------|------|---------|
| System | `system.json` | User preferences, display settings |
| World | `world_*.sav` | Entity snapshots, compressed |
| Game | `game_*.sav` | Custom game state, compressed |

### DebugConfig

Debug visualization flags:

```rust
let debug = DebugConfig {
    show_colliders: true,
    show_fps: true,
    show_frame_timing: false,
    show_velocities: false,
    show_origins: false,
};
```

### Debug Visualization Functions

Render debug overlays:

```rust
// Render collider outlines
render_colliders(frame, width, height, &world, &camera);

// Render velocity vectors
render_velocities(frame, width, height, &world, &camera);

// Render entity origins
render_origins(frame, width, height, &world, &camera);

// Render FPS counter
render_fps_counter(&mut ui, fps, screen_height);

// Render frame timing breakdown
render_frame_timing(&mut ui, &timings, screen_height);
```

## Import Patterns

From within this module:
```rust
// Files in config/ reference each other via super::
// But typically they're independent
```

From other core modules:
```rust
use crate::core::config::{Workspace, SaveManager, DebugConfig};
use crate::core::config::{render_colliders, render_fps_counter};
```

From game code via prelude:
```rust
use ryo2d::prelude::*;
// Workspace, SaveManager, etc. available
```

## Configuration Hierarchy

```
Engine Defaults
      ▼
Ryo.toml [system] section
      ▼
system.json (user preferences)
      ▼
Runtime overrides
```

**Priority:** Later values override earlier ones.

## Notes for AI Agents

1. **Workspace is auto-discovered** - Engine finds Ryo.toml automatically
2. **Save files are compressed** - Uses zstd compression for world/game saves
3. **Debug config comes from Ryo.toml** - [debug] section controls visualization
4. **Two DebugConfig structs exist** - One in workspace.rs (TOML parsing), one in debug.rs (runtime)
5. **System.json is user-facing** - Don't commit it; it's for player preferences
