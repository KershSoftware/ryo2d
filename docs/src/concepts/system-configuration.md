# System Configuration

Ryo2D uses a layered configuration system that separates developer defaults from user preferences, with saved player preferences applied last.

## Configuration Precedence

Startup settings resolve in one order, each layer overriding only what it sets: engine defaults → `Ryo.toml` (dev) or the values `ryo2d build` embedded from it (release) → `EngineBuilder` calls → saved player preferences in `system.json` (only keys present in the file and in the `with_user_preferences` filter). Without `with_user_preferences`, `system.json` is never read. A new `system.json` is seeded from the resolved settings.

```
Engine defaults < Ryo.toml / embedded values < EngineBuilder calls < system.json (filtered)
```

| Source | Purpose | Who Controls It |
|--------|---------|-----------------|
| **Builder calls** | Explicit `EngineBuilder` calls | Game developers (code) |
| **system.json** | Runtime preferences (when loaded) | Players |
| **Ryo.toml [system]** | Design-time defaults | Game developers (config) |
| **Engine Defaults** | Hardcoded fallback | Engine developers |

**Important:** `system.json` is only loaded when `with_user_preferences()` is called. Keys missing from system.json are not preferences; the builder, Ryo.toml and defaults apply.

## Section Mapping

Not all `Ryo.toml` sections are user-facing:

| Section | Maps to system.json? | Purpose |
|---------|---------------------|---------|
| `[system]` | ✅ Yes | Display, audio, show_fps defaults |
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
| Display Mode | `"windowed"` |
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
resolution = "1280x720"
vsync = true
display_mode = "windowed"
master_volume = 0.8
music_volume = 1.0
sfx_volume = 1.0
show_fps = false
console = false

# Developer-only (NOT saved to system.json)
[debug]
show_colliders = false
show_velocities = false
show_origins = false
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
    "display_mode": "windowed",
    "vsync": true
  },
  "vsync": true,
  "show_fps": false,
  "env": {
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
| `display.display_mode` | `string` | `"windowed"` | Display mode (`windowed`, `exclusive_fullscreen`, `borderless_fullscreen`) |
| `display.vsync` | `bool` | `true` | VSync enabled |
| `vsync` | `bool?` | `None` | VSync preference (overrides display.vsync) |
| `show_fps` | `bool?` | `None` | Show FPS counter (engine override) |
| `env` | `map` | `{}` | Custom key-value storage for game-specific environment variables |

**Note:** These are the engine defaults. Game developers can override them in `Ryo.toml [system]`, and players can override in `system.json`.

## VSync Configuration

The `vsync` field controls the rendering present mode with smart fallbacks:

| Value | Behavior | Fallback Chain |
|-------|----------|----------------|
| Not set | On (`true`) | Mailbox → Fifo |
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
// Access audio through SceneContext or SceneContext
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

The `[system]` section maps directly to `system.json` fields:

| Ryo.toml Field | system.json Field |
|----------------|-------------------|
| `resolution` | `display.resolution` |
| `vsync` | `display.vsync` / `vsync` |
| `display_mode` | `display.display_mode` |
| `master_volume` | `master_volume` |
| `music_volume` | `music_volume` |
| `sfx_volume` | `sfx_volume` |
| `show_fps` | `show_fps` |

**Not mapped:** `[project]`, `[paths]`, `[debug]`, `[env]` are developer-only and never written to `system.json`.

## Runtime Screen Size API

During gameplay, access the current screen dimensions through the context:

```rust
impl Scene for MyScene {
    fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
        // Get current screen dimensions
        let (width, height) = ctx.screen_size();
        let w = ctx.screen_width();
        let h = ctx.screen_height();
        
        // Use for dynamic positioning
        let center = (w / 2.0, h / 2.0);
        SceneAction::Continue
    }
    
    fn render_ui(&mut self, ui: &mut UiContext) -> SceneAction {
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

## Custom Game Env Variables

Use the `env` HashMap for game-specific configuration:

```rust
use serde_json::json;

// Save custom env variable
settings.set_env("difficulty", json!("hard"));
settings.set_env("language", json!("ja"));

// Load with typed getters
if let Some(difficulty) = settings.get_env_str("difficulty") {
    println!("Difficulty: {}", difficulty);
}
if let Some(debug) = settings.get_env_bool("debug_mode") {
    println!("Debug: {}", debug);
}
```

## Engine Override Behavior

When you use `with_user_preferences()` on the engine builder, it loads `system.json` and applies overrides. You can control which fields are loaded using a `UserPreferenceFilter`.

### Loading All Fields

```rust
use ryo2d::prelude::*;

EngineBuilder::new()
    .resolution(800, 600)
    .with_user_preferences(UserPreferenceFilter::all())
    .start_scene(MyScene::new())
    .build()
```

This allows:
- User's `vsync` preference to control present mode
- User's `show_fps` to override `Ryo.toml` debug settings
- User's display settings to override window configuration
- User's audio volumes to be applied to the AudioManager on startup

### Filtering Fields

For games with a fixed design resolution (like pixel art games), you can filter out resolution while still loading other user preferences:

```rust
use ryo2d::prelude::*;

// Load audio and display mode, but NOT resolution
EngineBuilder::new()
    .resolution(320, 240)  // Fixed pixel art resolution
    .with_user_preferences([
        UserPreference::MasterVolume,
        UserPreference::MusicVolume,
        UserPreference::SfxVolume,
        UserPreference::DisplayMode,
        UserPreference::Vsync,
        UserPreference::ShowFps,
    ])
    .start_scene(MyScene::new())
    .build()
```

### Available Fields

| Field | Description |
|-------|-------------|
| `UserPreference::MasterVolume` | Master volume (0.0 - 1.0) |
| `UserPreference::MusicVolume` | Music volume (0.0 - 1.0) |
| `UserPreference::SfxVolume` | Sound effects volume (0.0 - 1.0) |
| `UserPreference::Resolution` | Display resolution (width + height together) |
| `UserPreference::DisplayMode` | Windowed, exclusive fullscreen, or borderless fullscreen |
| `UserPreference::Vsync` | VSync preference |
| `UserPreference::ShowFps` | Show FPS counter |

### Filter Methods

| Method | Description |
|--------|-------------|
| `UserPreferenceFilter::all()` | All fields enabled |
| `UserPreferenceFilter::none()` | No fields - no saved preference overrides the game's settings |
| `UserPreferenceFilter::only([...])` | Only specified fields |
| `filter.contains(field)` | Check if a field is enabled |

## Best Practices

1. **Use `Ryo.toml` for developer defaults** - Project paths, default window size, debug flags during development
2. **Use `system.json` for user preferences** - Audio levels, display settings, keybindings
3. **Always provide defaults** - Don't require users to have `system.json`
4. **Document custom env vars** - If you use the `env` HashMap, document the keys
5. **Validate user input** - When loading `system.json`, clamp values to valid ranges

## See Also

- [Project Configuration](../getting-started/configuration.md) - `Ryo.toml` reference
- [Save System](./save-system.md) - Other save tiers (World State, Game Data)
- [Debugging](./debugging.md) - Debug configuration options
