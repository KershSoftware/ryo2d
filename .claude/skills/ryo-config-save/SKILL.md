---
name: ryo-config-save
description: Project configuration (Ryo.toml), user settings (system.json), and the 3-tier save system.
metadata:
  version: 1.0.0
---

# Ryo2D Configuration and Save System

You help game developers configure their projects and implement save/load functionality.

## Configuration Hierarchy

**Runtime code is always king.** Explicit `EngineBuilder` calls and runtime API methods always take precedence over configuration files.

| File | Purpose | Who Edits | Format |
|------|---------|-----------|--------|
| `Ryo.toml` | Developer defaults, debug flags | Game developer | TOML |
| `data/system.json` | User preferences | Player | JSON |

### Without `with_system_state_overrides()`

**Priority:** `code` > `Ryo.toml [system]` > `engine defaults`

system.json is **not loaded** unless you call `with_system_state_overrides()`.

### With `with_system_state_overrides(filter)`

**Priority:** `code` > `system.json (filtered)` > `Ryo.toml [system]` > `engine defaults`

Use `SystemFieldFilter` to control which fields are loaded from system.json.

## Ryo.toml Structure

```toml
[project]
name = "My Game"
title = "My Awesome Game"  # Window title (defaults to name)

[paths]
entry_point = "src/main.rs"
save_data = "data"
assets = "assets"

[system]  # Defaults for system.json (user-facing)
resolution_width = 1920
resolution_height = 1080
vsync = true
display_mode = "windowed"  # "windowed", "exclusive_fullscreen", or "borderless_fullscreen"
master_volume = 1.0
music_volume = 0.8
sfx_volume = 1.0
show_fps = false

[debug]  # Dev-only flags (NOT in system.json)
show_colliders = false
show_velocities = false
show_origins = false
show_frame_timing = false

[env]  # Game-specific variables
debug_mode = false
difficulty = "normal"
```

### Section Mapping

| Section | Goes to system.json | Purpose |
|---------|---------------------|---------|
| `[project]` | No | Project metadata (name, title, version) |
| `[paths]` | No | Directory structure |
| `[system]` | Yes | User-facing defaults (resolution, audio, show_fps) |
| `[debug]` | No | Developer debugging |
| `[env]` | No | Game variables |

## system.json

Created automatically from `[system]` section when missing (title from `[info]`).
Players can edit this file directly.

```json
{
  "master_volume": 0.8,
  "music_volume": 1.0,
  "sfx_volume": 1.0,
  "display": {
    "width": 1920,
    "height": 1080,
    "display_mode": "windowed",
    "vsync": true
  },
  "show_fps": false,
  "settings": {}
}
```

### DisplayMode Values

| Value | Description |
|-------|-------------|
| `"windowed"` | Regular window with decorations |
| `"exclusive_fullscreen"` | Exclusive fullscreen (changes display resolution) |
| `"borderless_fullscreen"` | Borderless window covering full screen |

## Loading Configuration

```rust
use ryo2d::prelude::*;

// Discover workspace (finds Ryo.toml)
let workspace = Workspace::discover()?;

// Access developer config
let config = workspace.config();
println!("Game: {}", config.name);
println!("Assets: {:?}", config.assets);

// Access system defaults from [system] section
let defaults = &config.system;
println!("Default resolution: {}x{}", 
    defaults.resolution_width, 
    defaults.resolution_height);

// Access debug flags from [debug] section
if config.debug.show_colliders {
    // Enable debug rendering
}

// Access env variables from [env] section
if let Some(difficulty) = config.env.get("difficulty") {
    println!("Difficulty: {}", difficulty);
}
```

## 3-Tier Save System

| Tier | Purpose | File | Format |
|------|---------|------|--------|
| 1 | System State | `system.json` | JSON (editable) |
| 2 | World State | `*.ryo-world` | Compressed binary |
| 3 | Game Data | `*.ryo-save` | Compressed binary |

### Save Directory Structure

```
{workspace}/data/
+-- system.json          # Tier 1: User settings
+-- slot1.ryo-world      # Tier 2: World snapshot
+-- slot1.ryo-save       # Tier 3: Game data
+-- slot2.ryo-world
+-- slot2.ryo-save
```

## Tier 1: System State

User settings that persist across sessions.

```rust
let workspace = Workspace::discover()?;
let save_manager = workspace.create_save_manager()?;

// Load with proper fallback chain
let settings = save_manager.load_or_create_system(
    &workspace.config().system
)?;

// Modify and save
let mut settings = save_manager.load_system()?;
settings.master_volume = 0.5;
settings.display.display_mode = DisplayMode::ExclusiveFullscreen;
save_manager.save_system(&settings)?;

// Custom game settings (string key-value)
settings.settings.insert("language".into(), "en".into());
```

## Tier 2: World State

ECS world snapshots - entities and serializable components.

```rust
// Save world to slot
save_manager.save_world("autosave", &ctx.world)?;

// Load world from slot
let loaded_world = save_manager.load_world("autosave")?;
*ctx.world = loaded_world;

// List available world saves
let slots = save_manager.list_world_saves()?;
for slot in slots {
    println!("Found save: {}", slot);
}
```

**Note:** Only components implementing `Serialize` + `Deserialize` are saved.

## Tier 3: Game Data

Custom game-specific data (progress, unlocks, etc.).

```rust
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize)]
struct PlayerProgress {
    level: u32,
    score: u64,
    unlocked_items: Vec<String>,
}

let progress = PlayerProgress {
    level: 5,
    score: 12500,
    unlocked_items: vec!["sword".into(), "shield".into()],
};

// Save to slot (compressed)
save_manager.save_game("slot1", &progress)?;

// Load from slot
let loaded: PlayerProgress = save_manager.load_game("slot1")?;

// Delete a save
save_manager.delete_game("slot1")?;
```

## Best Practices

1. **Use `load_or_create_system`** - Handles the fallback chain correctly
2. **Keep system.json player-editable** - It's JSON for a reason
3. **Debug flags in [debug]** - Never expose to players
4. **Use [env] for game variables** - Easy to tweak during development
5. **World saves for checkpoints** - Quick snapshots of game state
6. **Game saves for progress** - Structured data you control

## Common Patterns

### Settings Menu

```rust
fn render_settings(&mut self, ui: &mut UiContext, save_manager: &SaveManager) {
    let mut settings = save_manager.load_system().unwrap_or_default();

    if ui.slider("Master Volume", &mut settings.master_volume, 0.0..=1.0) {
        save_manager.save_system(&settings).ok();
    }
}
```

### Display Mode Settings

For runtime display mode changes, use `WindowRequest`:

```rust
// In your scene/game
fn apply_display_mode(&mut self, ctx: &mut SceneContext, mode: DisplayMode) {
    // Request display mode change (applied next frame)
    ctx.request_display_mode(mode);

    // Save preference
    save_display_mode(mode);
}

// Display mode options for settings menu
const DISPLAY_MODES: &[DisplayMode] = &[
    DisplayMode::Windowed,
    DisplayMode::BorderlessFullscreen,
    DisplayMode::ExclusiveFullscreen,
];

fn display_mode_label(mode: DisplayMode) -> &'static str {
    match mode {
        DisplayMode::Windowed => "Windowed",
        DisplayMode::BorderlessFullscreen => "Borderless Fullscreen",
        DisplayMode::ExclusiveFullscreen => "Exclusive Fullscreen",
    }
}
```

### Autosave

```rust
impl Game for MyGame {
    fn update(&mut self, ctx: &mut GameContext, dt: f32) {
        self.autosave_timer += dt;
        if self.autosave_timer >= 60.0 {
            self.autosave_timer = 0.0;
            if let Some(ref save_manager) = self.save_manager {
                save_manager.save_world("autosave", &ctx.world).ok();
            }
        }
    }
}
```

### Multiple Save Slots

```rust
fn save_to_slot(&self, slot: u8) -> SaveResult<()> {
    let slot_name = format!("slot{}", slot);
    self.save_manager.save_world(&slot_name, &self.world)?;
    self.save_manager.save_game(&slot_name, &self.progress)?;
    Ok(())
}

fn load_from_slot(&mut self, slot: u8) -> SaveResult<()> {
    let slot_name = format!("slot{}", slot);
    self.world = self.save_manager.load_world(&slot_name)?;
    self.progress = self.save_manager.load_game(&slot_name)?;
    Ok(())
}
```
