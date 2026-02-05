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

### SystemField & SystemFieldFilter

Control which fields are loaded from `system.json` when using `with_system_state_overrides()`:

```rust
// SystemField enum variants
SystemField::MasterVolume   // Master volume (0.0 - 1.0)
SystemField::MusicVolume    // Music volume (0.0 - 1.0)
SystemField::SfxVolume      // SFX volume (0.0 - 1.0)
SystemField::Resolution     // Display resolution (width + height)
SystemField::DisplayMode    // Windowed/fullscreen/borderless
SystemField::Vsync          // VSync preference
SystemField::ShowFps        // Show FPS counter

// SystemFieldFilter methods
SystemFieldFilter::all()    // All fields (default, backward compatible)
SystemFieldFilter::none()   // No fields (pure Ryo.toml defaults)
SystemFieldFilter::only([]) // Only specified fields
filter.contains(field)      // Check if field is enabled
```

**Usage in EngineBuilder:**

```rust
// Load only audio settings from system.json
EngineBuilder::new()
    .with_system_state_overrides([
        SystemField::MasterVolume,
        SystemField::MusicVolume,
        SystemField::SfxVolume,
    ])
    .build();

// Load all fields (default behavior)
EngineBuilder::new()
    .with_system_state_overrides(SystemFieldFilter::all())
    .build();

// Load nothing from system.json
EngineBuilder::new()
    .with_system_state_overrides(SystemFieldFilter::none())
    .build();
```

**Use cases:**
- Fixed-resolution games: Filter out `Resolution` to keep design resolution from Ryo.toml
- Audio-only settings: Filter to only volume fields
- Pure Ryo.toml: Use `none()` to ignore system.json entirely

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

**Runtime code is always king.** Explicit `EngineBuilder` calls and runtime API methods always take precedence over configuration files.

### Without `with_system_state_overrides()`

```
Engine Defaults (lowest)
      ▼
Ryo.toml [system] section
      ▼
Runtime Code (highest)
```

If you don't call `with_system_state_overrides()`, system.json is **not loaded at all**. Only Ryo.toml and engine defaults are used.

### With `with_system_state_overrides(filter)`

```
Engine Defaults (lowest)
      ▼
Ryo.toml [system] section
      ▼
system.json (filtered fields)
      ▼
Runtime Code (highest)
```

When you call `with_system_state_overrides()`, the engine loads system.json and applies the filtered fields. You can control which fields are loaded via `SystemFieldFilter`.

### Examples

```rust
// WITHOUT overrides: code > Ryo.toml > defaults
// system.json is IGNORED, .resolution(800, 600) wins
EngineBuilder::new()
    .resolution(800, 600)
    .build();

// WITH overrides: code > system.json > Ryo.toml > defaults
// If user has resolution in system.json, it's used (unless code overrides)
EngineBuilder::new()
    .with_system_state_overrides(SystemFieldFilter::all())
    .build();

// Mixed: code overrides resolution, but user's audio settings are loaded
EngineBuilder::new()
    .resolution(800, 600)  // This wins over any config
    .with_system_state_overrides(SystemFieldFilter::all())
    .build();
```

**Key insight:** If a developer explicitly sets `.resolution(800, 600)` in code, that value is used regardless of what's in Ryo.toml or system.json.

## Notes for AI Agents

1. **Workspace is auto-discovered** - Engine finds Ryo.toml automatically
2. **Save files are compressed** - Uses zstd compression for world/game saves
3. **Debug config comes from Ryo.toml** - [debug] section controls visualization
4. **Two DebugConfig structs exist** - One in workspace.rs (TOML parsing), one in debug.rs (runtime)
5. **System.json is user-facing** - Don't commit it; it's for player preferences
