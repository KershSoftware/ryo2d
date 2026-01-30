# System Configuration

Ryo2D supports two types of configuration files with different purposes:

- **`Ryo.toml`** - Developer configuration (paths, defaults, project metadata)
- **`system.json`** - Runtime user settings (display, audio, keybindings)

## Configuration Precedence

When both files exist and define the same setting:

```
system.json > Ryo.toml > Engine Defaults
```

**Important:** `system.json` only overrides when explicitly present. Missing fields fall back to `Ryo.toml` or engine defaults.

## Ryo.toml (Developer Config)

Developer-facing configuration for project structure and game defaults.

```toml
[project]
name = "My Game"

[paths]
save_data = "data"
assets = "assets"

[window]
width = 800
height = 600
title = "My Game"
mode = "ResizableAspectRatio"
max_width = 1920
max_height = 1080

[debug]
show_fps = false
show_colliders = false
```

See [Project Configuration](../getting-started/configuration.md) for full `Ryo.toml` documentation.

## system.json (User Settings)

Runtime user preferences that can be modified by the game's options menu or by users manually editing the file.

Located at: `{save_dir}/system.json`

```json
{
  "master_volume": 0.75,
  "music_volume": 0.6,
  "sfx_volume": 0.9,
  "display": {
    "width": 1920,
    "height": 1080,
    "fullscreen": true,
    "vsync": true
  },
  "vsync": true,
  "show_fps": false,
  "settings": {
    "language": "en",
    "difficulty": "normal"
  }
}
```

### SystemState Fields

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `master_volume` | `f32` | `1.0` | Master volume (0.0 - 1.0) |
| `music_volume` | `f32` | `0.8` | Music volume (0.0 - 1.0) |
| `sfx_volume` | `f32` | `1.0` | Sound effects volume (0.0 - 1.0) |
| `display.width` | `u32` | `800` | Window width |
| `display.height` | `u32` | `600` | Window height |
| `display.fullscreen` | `bool` | `false` | Fullscreen mode |
| `display.vsync` | `bool` | `true` | VSync enabled (legacy field) |
| `vsync` | `bool?` | `None` | VSync preference (overrides display.vsync) |
| `show_fps` | `bool?` | `None` | Show FPS counter (engine override) |
| `settings` | `map` | `{}` | Custom key-value storage for game-specific settings |

## VSync Configuration

The `vsync` field controls the rendering present mode with smart fallbacks:

| Value | Behavior | Fallback Chain |
|-------|----------|----------------|
| Not set | Best performance + no tearing | Mailbox → Immediate |
| `true` | Traditional VSync | Mailbox → Fifo |
| `false` | Maximum performance (may tear) | Immediate |

**Mailbox** is preferred because it eliminates tearing without blocking like Fifo, but not all systems support it (notably, some browsers don't support Mailbox).

### Example: Force VSync Off for Maximum FPS

```json
{
  "vsync": false
}
```

This uses `Immediate` present mode, which may cause screen tearing but provides the lowest latency.

## Field Mapping Differences

Not all fields have 1-to-1 mappings between `Ryo.toml` and `system.json`:

| Setting | Ryo.toml | system.json |
|---------|----------|-------------|
| FPS display | `[debug] show_fps` | `show_fps` (root level) |
| VSync | N/A | `vsync` |
| Window size | `[window] width/height` | `display.width/height` |
| Audio levels | N/A | `master_volume`, `music_volume`, `sfx_volume` |

## Loading System State

```rust
use ryo2d::prelude::*;

fn load_user_settings(save_manager: &SaveManager) -> SaveResult<SystemState> {
    // Loads from {save_dir}/system.json
    // Returns default SystemState if file doesn't exist
    save_manager.load_system()
}
```

## Saving System State

```rust
use ryo2d::prelude::*;

fn save_user_settings(save_manager: &SaveManager, settings: &SystemState) -> SaveResult<()> {
    // Saves to {save_dir}/system.json as human-readable JSON
    save_manager.save_system(settings)
}
```

## Options Menu Example

```rust
use ryo2d::prelude::*;

struct OptionsMenu {
    settings: SystemState,
}

impl OptionsMenu {
    fn new(save_manager: &SaveManager) -> Self {
        let settings = save_manager.load_system().unwrap_or_default();
        Self { settings }
    }

    fn toggle_vsync(&mut self) {
        // Toggle between Some(true), Some(false), and None
        self.settings.vsync = match self.settings.vsync {
            None => Some(true),        // Default → On
            Some(true) => Some(false), // On → Off
            Some(false) => None,       // Off → Default
        };
    }

    fn adjust_volume(&mut self, delta: f32) {
        self.settings.master_volume = 
            (self.settings.master_volume + delta).clamp(0.0, 1.0);
    }

    fn save(&self, save_manager: &SaveManager) -> SaveResult<()> {
        save_manager.save_system(&self.settings)
    }
}
```

## Custom Game Settings

Use the `settings` HashMap for game-specific configuration:

```rust
// Save custom setting
settings.set_setting("difficulty", "hard");
settings.set_setting("language", "ja");

// Load custom setting
if let Some(difficulty) = settings.get_setting("difficulty") {
    println!("Difficulty: {}", difficulty);
}
```

## Engine Override Behavior

When you use `with_system_state_overrides()` on the engine builder, it loads `system.json` and applies overrides:

```rust
EngineBuilder::new()
    .resolution(800, 600)
    .with_system_state_overrides()  // Loads system.json
    .with_game(MyGame)
    .build()
```

This allows:
- User's `vsync` preference to control present mode
- User's `show_fps` to override `Ryo.toml` debug settings
- User's display settings to override window configuration

## Best Practices

1. **Use `Ryo.toml` for developer defaults** - Project paths, default window size, debug flags during development
2. **Use `system.json` for user preferences** - Audio levels, display settings, keybindings
3. **Always provide defaults** - Don't require users to have `system.json`
4. **Document custom settings** - If you use the `settings` HashMap, document the keys
5. **Validate user input** - When loading `system.json`, clamp values to valid ranges

## See Also

- [Project Configuration](../getting-started/configuration.md) - `Ryo.toml` reference
- [Save System](./save-system.md) - Other save tiers (World State, Game Data)
- [Debugging](./debugging.md) - Debug configuration options
