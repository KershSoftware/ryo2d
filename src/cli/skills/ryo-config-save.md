---
name: ryo-config-save
description: Project configuration (Ryo.toml), user settings (system.json), and the 3-tier save system.
metadata:
  version: 1.0.0
---

# Ryo2D Configuration and Save System

You help game developers configure their projects and implement save/load functionality.

## Configuration Hierarchy

Ryo2D uses a two-file configuration system:

| File | Purpose | Who Edits | Format |
|------|---------|-----------|--------|
| `Ryo.toml` | Developer defaults, debug flags | Game developer | TOML |
| `data/system.json` | User preferences | Player | JSON |

**Priority chain:** saved `system.json` preferences (only keys present, only with `with_user_preferences`) > `EngineBuilder` calls > `Ryo.toml` (dev) / embedded values (release) > engine defaults

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
resolution = "1920x1080"
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
| `[env]` | Seeds its `env` (with `with_user_preferences`) | Game variables; shipped with release builds |

## system.json

Created on first run from the game's resolved settings (builder > Ryo.toml > defaults), holding only the keys in the `with_user_preferences` filter (plus `env` when Ryo.toml has `[env]`); never created without `with_user_preferences`.
After the first run, `data/system.json` holds the filtered settings and overrides later builder or `[system]` changes - delete it in dev to re-seed.
Players can edit this file directly.

```json
{
  "master_volume": 0.8,
  "music_volume": 1.0,
  "sfx_volume": 1.0,
  "resolution": "1920x1080",
  "display_mode": "windowed",
  "vsync": true,
  "show_fps": false,
  "env": {}
}
```

### DisplayMode Values

| Value | Description |
|-------|-------------|
| `"windowed"` | Regular window with decorations |
| `"exclusive_fullscreen"` | Exclusive fullscreen (changes display resolution) |
| `"borderless_fullscreen"` | Borderless window covering full screen |

## Where saves live

| Build | Save folder (system.json, game and world saves) |
|---|---|
| Dev (`ryo2d dev`, `cargo run`) | `[paths] save_data` under the project (default `data/`) |
| Release (`ryo2d build`) | OS user data folder: `%APPDATA%`, `~/Library/Application Support` or `~/.local/share`, plus `[release] save_folder` (default `[project] name`) |
| Release with `[release] portable = true` | `data/` beside the game |

`Workspace::save_location()` returns the folder; the engine logs it at startup. `ryo2d run` launches the release bundle like a player would, so a non-portable game saves into your real OS folder.

- The save folder is where players' saves live: set `[release] save_folder` before your first release. Changing it - or `[project] name` while it's unset - moves to a new, empty folder.
- A default folder name from `[project] name` has `/` and characters Windows rejects replaced with `-`. An explicit `save_folder` is used as written (`Studio/Game` nests).
- Games built with older engine versions saved to `data/` beside the game; set `portable = true` to keep using those saves when shipping an update.
- The folder persists across `ryo2d build`s; delete its `system.json` to pick up new `[system]` defaults when testing with `ryo2d run`.
- Release builds read the Ryo.toml values embedded by `ryo2d build` (including `[env]`) and never a Ryo.toml on disk, so `Workspace::discover().config()` matches between `ryo2d run` and a shipped game.

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
println!("Default resolution: {:?}", defaults.resolution);  // Option<(u32, u32)>

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

// Modify and save (resolution is settings.width / settings.height)
let mut settings = save_manager.load_system()?;
settings.master_volume = 0.5;
settings.display_mode = DisplayMode::ExclusiveFullscreen;
save_manager.save_system(&settings)?;

// Custom game env variables (typed values)
settings.set_env("language", serde_json::json!("en"));
```

## Tier 2: World State

`WorldState` is a container of `EntitySnapshot`s (component data serialized to bytes, keyed by component name). The engine does not capture a `World` into it automatically: you fill it yourself, and rebuild entities from it after loading.

```rust
// use std::collections::HashMap;
// Build a snapshot (serialize the components you care about yourself)
let mut state = WorldState::new();
state.set_metadata("level", "forest");
state.add_entity(EntitySnapshot { id: 1, components: HashMap::new() });

// Save to slot
save_manager.save_world("autosave", &state)?;

// Load from slot
let loaded: WorldState = save_manager.load_world("autosave")?;
for snapshot in &loaded.entities {
    // Recreate the entity from snapshot.components
}

// List available world saves
let slots = save_manager.list_world_saves()?;
for slot in slots {
    println!("Found save: {}", slot);
}
```

**Note:** For most games, `save_game` with your own serializable struct (Tier 3) is simpler than world snapshots.

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

// Load from slot (returned in a GameSaveData wrapper: .data, .timestamp, .play_time, ...)
let loaded: PlayerProgress = save_manager.load_game("slot1")?.data;

// Delete a save
save_manager.delete_game("slot1")?;
```

## Best Practices

1. **Use `load_or_create_system`** - Handles the fallback chain correctly
2. **Keep system.json player-editable** - It's JSON for a reason
3. **Debug flags in [debug]** - Never expose to players
4. **Use [env] for game variables** - Easy to tweak during development; `ryo2d build` ships them, so set dev-only switches (e.g. `debug_scene`) to `false` before release
5. **World saves for checkpoints** - Quick snapshots of game state
6. **Game saves for progress** - Structured data you control

## Common Patterns

### Settings Menu

```rust
fn render_settings(&mut self, ui: &mut UiContext, save_manager: &SaveManager) {
    let mut settings = save_manager.load_system().unwrap_or_default();

    let track = Rect::new(100.0, 100.0, 200.0, 20.0);
    if ui.slider(track, &mut settings.master_volume, 0.0, 1.0) {
        save_manager.save_system(&settings).ok();
    }
}
```

### Display Mode Settings

For runtime display mode changes, use `WindowRequest`:

```rust
// In your scene/game
fn apply_display_mode(&mut self, ctx: &mut SceneContext, save_manager: &SaveManager, mode: DisplayMode) {
    // Request display mode change (applied after the current frame)
    ctx.request_display_mode(mode);

    // Save preference
    let mut settings = save_manager.load_system().unwrap_or_default();
    settings.display_mode = mode;
    save_manager.save_system(&settings).ok();
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
impl Scene for MyScene {
    fn update(&mut self, _ctx: &mut SceneContext, dt: f32) -> SceneAction {
        self.autosave_timer += dt;
        if self.autosave_timer >= 60.0 {
            self.autosave_timer = 0.0;
            if let Some(ref save_manager) = self.save_manager {
                save_manager.save_game("autosave", &self.progress).ok();
            }
        }
        SceneAction::Continue
    }
}
```

### Multiple Save Slots

`self.world` here is a `WorldState` you maintain (see Tier 2).

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
    self.progress = self.save_manager.load_game(&slot_name)?.data;
    Ok(())
}
```
