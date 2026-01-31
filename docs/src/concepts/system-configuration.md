# System Configuration

Ryo2D uses a 3-tier configuration system that cleanly separates developer defaults from user preferences:

```
system.json (user) > Ryo.toml [system] (developer) > Engine Defaults
```

## Configuration Precedence

| Source | Purpose | Who Controls It |
|--------|---------|-----------------|
| **Engine Defaults** | Hardcoded fallback | Engine developers |
| **Ryo.toml [system]** | Design-time defaults | Game developers |
| **system.json** | Runtime preferences | Players |

**Important:** `system.json` only overrides fields that are explicitly present. Missing fields fall back through the chain.

## Section Mapping

Not all `Ryo.toml` sections are user-facing:

| Section | Maps to system.json? | Purpose |
|---------|---------------------|---------|
| `[system]` | ✅ Yes | Display, audio defaults |
| `[info]` | ✅ Yes | `show_fps` overlay |
| `[project]` | ❌ No | Project metadata |
| `[paths]` | ❌ No | File locations |
| `[debug]` | ❌ No | Development debugging |
| `[env]` | ❌ No | Environment variables |

## Engine Defaults

When no configuration exists, the engine uses these defaults:

| Setting | Default Value |
|---------|---------------|
| Resolution | 1280 × 720 |
| VSync | `true` |
| Fullscreen | `false` |
| Master Volume | 0.8 |
| Music Volume | 1.0 |
| SFX Volume | 1.0 |
| Show FPS | `false` |

## Ryo.toml (Developer Config)

Developer-facing configuration for project structure and game defaults.

```toml
[project]
name = "My Game"

[paths]
save_data = "data"
assets = "assets"

# User-facing defaults (→ system.json)
[system]
resolution_width = 1280
resolution_height = 720
vsync = true
fullscreen = false
master_volume = 0.8
music_volume = 1.0
sfx_volume = 1.0

# User-facing info (→ system.json)
[info]
show_fps = false

# Developer-only (NOT saved to system.json)
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

| Field | Type | Engine Default | Description |
|-------|------|----------------|-------------|
| `master_volume` | `f32` | `0.8` | Master volume (0.0 - 1.0) |
| `music_volume` | `f32` | `1.0` | Music volume (0.0 - 1.0) |
| `sfx_volume` | `f32` | `1.0` | Sound effects volume (0.0 - 1.0) |
| `display.width` | `u32` | `1280` | Window width |
| `display.height` | `u32` | `720` | Window height |
| `display.fullscreen` | `bool` | `false` | Fullscreen mode |
| `display.vsync` | `bool` | `true` | VSync enabled (legacy field) |
| `vsync` | `bool?` | `None` | VSync preference (overrides display.vsync) |
| `show_fps` | `bool?` | `None` | Show FPS counter (engine override) |
| `settings` | `map` | `{}` | Custom key-value storage for game-specific settings |

**Note:** These are the engine defaults. Game developers can override them in `Ryo.toml [system]`, and players can override in `system.json`.

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

## Audio Volume Configuration

The audio system uses a hierarchical volume structure with three levels:

### Volume Hierarchy

```
Master Volume (0.0 - 1.0)
├── Music Volume (0.0 - 1.0)
│   └── Effective Music = master × music
└── SFX Volume (0.0 - 1.0)
    └── Effective SFX = master × sfx
```

**Master volume** scales all audio globally. **Music** and **SFX** volumes independently control their respective categories. Multiple sounds can play simultaneously in each category.

### Example: Volume Calculations

```json
{
  "master_volume": 0.8,  // 80% global volume
  "music_volume": 0.6,   // 60% of master for music
  "sfx_volume": 1.0      // 100% of master for sfx
}
```

Effective volumes:
- **Music plays at:** `0.8 × 0.6 = 0.48` (48%)
- **SFX plays at:** `0.8 × 1.0 = 0.8` (80%)

### Runtime API

Control volumes programmatically through `AudioManager`:

```rust
// Access audio through GameContext or SceneContext
if let Some(audio) = ctx.audio {
    // Master volume (affects everything)
    audio.set_master_volume(0.75);
    let master = audio.master_volume();
    
    // Music category (background music, ambient sounds)
    audio.set_music_volume(0.6);
    let music = audio.music_volume();
    
    // SFX category (sound effects, UI sounds)
    audio.set_sfx_volume(1.0);
    let sfx = audio.sfx_volume();
}
```

### Options Menu Example

```rust
struct AudioSettings {
    settings: SystemState,
}

impl AudioSettings {
    fn adjust_master(&mut self, delta: f32) {
        self.settings.master_volume = 
            (self.settings.master_volume + delta).clamp(0.0, 1.0);
    }
    
    fn adjust_music(&mut self, delta: f32) {
        self.settings.music_volume = 
            (self.settings.music_volume + delta).clamp(0.0, 1.0);
    }
    
    fn adjust_sfx(&mut self, delta: f32) {
        self.settings.sfx_volume = 
            (self.settings.sfx_volume + delta).clamp(0.0, 1.0);
    }
    
    fn apply_to_audio_manager(&self, audio: &mut AudioManager) {
        audio.set_master_volume(self.settings.master_volume);
        audio.set_music_volume(self.settings.music_volume);
        audio.set_sfx_volume(self.settings.sfx_volume);
    }
}
```

### Volume Categories

**Music** - Used by:
- Background music (`play_music()`)
- Ambient loops
- Dynamic music systems

**SFX** - Used by:
- Sound effects (`play_sound()`)
- UI feedback sounds
- One-shot audio events

Both categories support multiple simultaneous sounds with independent volume control.

## Field Mapping: Ryo.toml → system.json

The `[system]` and `[info]` sections map directly to `system.json` fields:

| Ryo.toml Section | Ryo.toml Field | system.json Field |
|------------------|----------------|-------------------|
| `[system]` | `resolution_width` | `display.width` |
| `[system]` | `resolution_height` | `display.height` |
| `[system]` | `vsync` | `display.vsync` / `vsync` |
| `[system]` | `fullscreen` | `display.fullscreen` |
| `[system]` | `master_volume` | `master_volume` |
| `[system]` | `music_volume` | `music_volume` |
| `[system]` | `sfx_volume` | `sfx_volume` |
| `[info]` | `show_fps` | `show_fps` |

**Not mapped:** `[project]`, `[paths]`, `[debug]`, `[env]` are developer-only and never written to `system.json`.

## Runtime Screen Size API

During gameplay, access the current screen dimensions through the context:

```rust
impl Scene for MyScene {
    fn update(&mut self, ctx: &mut SceneContext) {
        // Get current screen dimensions
        let (width, height) = ctx.screen_size();
        let w = ctx.screen_width();
        let h = ctx.screen_height();
        
        // Use for dynamic positioning
        let center = (w / 2.0, h / 2.0);
    }
    
    fn render_ui(&mut self, ui: &mut UiContext) {
        // Also available in UI rendering
        let (w, h) = ui.screen_size();
    }
}
```

**Best practice:** Set your design resolution in `Ryo.toml [system]`, then use the runtime API for positioning. This allows users to customize resolution in `system.json` without breaking your game layout.

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

    fn adjust_master_volume(&mut self, delta: f32) {
        self.settings.master_volume = 
            (self.settings.master_volume + delta).clamp(0.0, 1.0);
    }

    fn adjust_music_volume(&mut self, delta: f32) {
        self.settings.music_volume = 
            (self.settings.music_volume + delta).clamp(0.0, 1.0);
    }

    fn adjust_sfx_volume(&mut self, delta: f32) {
        self.settings.sfx_volume = 
            (self.settings.sfx_volume + delta).clamp(0.0, 1.0);
    }

    fn apply_audio(&self, audio: &mut Option<AudioManager>) {
        if let Some(audio_manager) = audio {
            audio_manager.set_master_volume(self.settings.master_volume);
            audio_manager.set_music_volume(self.settings.music_volume);
            audio_manager.set_sfx_volume(self.settings.sfx_volume);
        }
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
- User's audio volumes to be applied to the AudioManager on startup

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
