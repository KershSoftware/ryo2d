# Save Game Example

This example demonstrates a complete save/load system for a simple RPG-style game with multiple save slots.

## Complete Example

```rust
use ryo2d::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// =============================================================================
// Game Data Structures
// =============================================================================

#[derive(Serialize, Deserialize, Clone)]
struct PlayerData {
    name: String,
    level: u32,
    experience: u64,
    health: u32,
    max_health: u32,
    position: (f32, f32),
}

#[derive(Serialize, Deserialize, Clone)]
struct InventoryItem {
    id: String,
    quantity: u32,
}

#[derive(Serialize, Deserialize, Clone)]
struct QuestProgress {
    quest_id: String,
    objectives_completed: Vec<String>,
    is_complete: bool,
}

#[derive(Serialize, Deserialize)]
struct GameProgress {
    save_version: u32,
    player: PlayerData,
    inventory: Vec<InventoryItem>,
    quests: HashMap<String, QuestProgress>,
    current_scene: String,
    total_play_time: u64,
    enemies_defeated: u32,
}

impl Default for GameProgress {
    fn default() -> Self {
        GameProgress {
            save_version: 1,
            player: PlayerData {
                name: "Hero".to_string(),
                level: 1,
                experience: 0,
                health: 100,
                max_health: 100,
                position: (0.0, 0.0),
            },
            inventory: vec![
                InventoryItem { id: "health_potion".into(), quantity: 3 },
            ],
            quests: HashMap::new(),
            current_scene: "starting_village".to_string(),
            total_play_time: 0,
            enemies_defeated: 0,
        }
    }
}

// =============================================================================
// Save System Wrapper
// =============================================================================

struct GameSaveSystem {
    save_manager: SaveManager,
    current_slot: Option<String>,
    play_time_seconds: u64,
}

impl GameSaveSystem {
    fn new(workspace: &Workspace) -> Result<Self, EngineError> {
        Ok(GameSaveSystem {
            save_manager: workspace.create_save_manager()?,
            current_slot: None,
            play_time_seconds: 0,
        })
    }

    /// Save game to the specified slot
    fn save_game(&self, slot: &str, progress: &GameProgress) -> SaveResult<()> {
        let save = GameSaveData::new(progress)
            .with_name(&format!("Lv.{} {}", progress.player.level, progress.player.name))
            .with_play_time(progress.total_play_time);

        self.save_manager.save_game_with_info(slot, save)
    }

    /// Load game from a slot
    fn load_game(&mut self, slot: &str) -> SaveResult<GameProgress> {
        let save: GameSaveData<GameProgress> = self.save_manager.load_game(slot)?;
        self.current_slot = Some(slot.to_string());
        self.play_time_seconds = save.play_time;
        Ok(save.data)
    }

    /// Quick save to current slot (or "quicksave" if no slot loaded)
    fn quick_save(&self, progress: &GameProgress) -> SaveResult<()> {
        let slot = self.current_slot.as_deref().unwrap_or("quicksave");
        self.save_game(slot, progress)
    }

    /// Autosave (separate from manual saves)
    fn autosave(&self, progress: &GameProgress) -> SaveResult<()> {
        self.save_game("autosave", progress)
    }

    /// Get all available save slots with their metadata
    fn list_saves(&self) -> SaveResult<Vec<SaveSlotInfo>> {
        let slots = self.save_manager.list_game_saves()?;
        let mut infos = Vec::new();

        for slot in slots {
            if let Ok(save) = self.save_manager.load_game::<GameProgress>(&slot) {
                infos.push(SaveSlotInfo {
                    slot_name: slot,
                    player_name: save.data.player.name,
                    level: save.data.player.level,
                    scene: save.data.current_scene,
                    play_time: save.play_time,
                    timestamp: save.timestamp,
                    display_name: save.name,
                });
            }
        }

        // Sort by timestamp, newest first
        infos.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        Ok(infos)
    }

    /// Delete a save slot
    fn delete_save(&self, slot: &str) -> SaveResult<()> {
        self.save_manager.delete_game(slot)
    }

    /// Check if any saves exist
    fn has_any_saves(&self) -> SaveResult<bool> {
        Ok(!self.save_manager.list_game_saves()?.is_empty())
    }
}

struct SaveSlotInfo {
    slot_name: String,
    player_name: String,
    level: u32,
    scene: String,
    play_time: u64,
    timestamp: u64,
    display_name: Option<String>,
}

impl SaveSlotInfo {
    fn format_play_time(&self) -> String {
        let hours = self.play_time / 3600;
        let minutes = (self.play_time % 3600) / 60;
        format!("{}:{:02}", hours, minutes)
    }
}

// =============================================================================
// Settings System (Tier 1)
// =============================================================================

fn save_settings(save_manager: &SaveManager) -> SaveResult<()> {
    let mut system = save_manager.load_system()?;

    // Update settings from your game's options menu
    system.master_volume = 0.8;
    system.music_volume = 0.6;
    system.sfx_volume = 1.0;
    system.display.width = 1920;
    system.display.height = 1080;
    system.display.fullscreen = true;
    system.set_custom("difficulty", "normal");
    system.set_custom("subtitles", "on");

    save_manager.save_system(&system)
}

fn load_settings(save_manager: &SaveManager) -> SaveResult<SystemState> {
    // Returns defaults if no settings file exists
    save_manager.load_system()
}

// =============================================================================
// Game Integration
// =============================================================================

struct MyRPGGame {
    save_system: Option<GameSaveSystem>,
    progress: GameProgress,
    session_start: std::time::Instant,
    last_autosave: std::time::Instant,
}

impl Scene for MyRPGGame {
    fn on_enter(&mut self, ctx: &mut SceneContext) {
        // Initialize save system from workspace
        if let Ok(workspace) = Workspace::discover() {
            if let Ok(save_system) = GameSaveSystem::new(&workspace) {
                // Load settings
                if let Ok(settings) = load_settings(&save_system.save_manager) {
                    // Apply settings to your audio system, etc.
                    println!("Master volume: {}", settings.master_volume);
                }
                self.save_system = Some(save_system);
            }
        }

        self.session_start = std::time::Instant::now();
        self.last_autosave = std::time::Instant::now();
    }

    fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
        // Update play time
        self.progress.total_play_time = self.session_start.elapsed().as_secs();

        // F5 = Quick Save
        if ctx.input.key_pressed(KeyCode::F5) {
            if let Some(ref save_system) = self.save_system {
                match save_system.quick_save(&self.progress) {
                    Ok(_) => println!("Quick saved!"),
                    Err(e) => println!("Quick save failed: {}", e),
                }
            }
        }

        // F9 = Quick Load
        if ctx.input.key_pressed(KeyCode::F9) {
            if let Some(ref mut save_system) = self.save_system {
                match save_system.load_game("quicksave") {
                    Ok(progress) => {
                        self.progress = progress;
                        println!("Quick loaded!");
                    }
                    Err(e) => println!("Quick load failed: {}", e),
                }
            }
        }

        // Autosave every 5 minutes
        if self.last_autosave.elapsed().as_secs() > 300 {
            if let Some(ref save_system) = self.save_system {
                let _ = save_system.autosave(&self.progress);
                self.last_autosave = std::time::Instant::now();
            }
        }
        SceneAction::Continue
    }
}

// =============================================================================
// Save/Load Menu UI (Conceptual)
// =============================================================================

fn render_save_menu(save_system: &GameSaveSystem) {
    println!("=== Save Slots ===");

    match save_system.list_saves() {
        Ok(saves) => {
            for (i, save) in saves.iter().enumerate() {
                let name = save.display_name.as_deref().unwrap_or(&save.slot_name);
                println!(
                    "[{}] {} - Lv.{} {} - {} - {}",
                    i + 1,
                    save.slot_name,
                    save.level,
                    save.player_name,
                    save.scene,
                    save.format_play_time()
                );
            }

            if saves.is_empty() {
                println!("No saves found");
            }
        }
        Err(e) => println!("Error loading saves: {}", e),
    }
}

fn main() {
    // Example usage in a real game would use EngineBuilder
    println!("See the code for a complete save system example!");
}
```

## Key Patterns

### 1. Wrapper for Game-Specific Saves

Create a `GameSaveSystem` struct that wraps `SaveManager` and provides game-specific methods:

```rust
struct GameSaveSystem {
    save_manager: SaveManager,
    current_slot: Option<String>,
}

impl GameSaveSystem {
    fn quick_save(&self, progress: &GameProgress) -> SaveResult<()> {
        let slot = self.current_slot.as_deref().unwrap_or("quicksave");
        self.save_manager.save_game(slot, progress)
    }
}
```

### 2. Separate Settings from Progress

Use Tier 1 (`SystemState`) for options menu settings:

```rust
// Settings persist even without a save file
let settings = save_manager.load_system()?;  // Returns defaults if none

// Game progress requires explicit save
let progress = save_manager.load_game("slot1")?;
```

### 3. Include Version in Save Data

```rust
#[derive(Serialize, Deserialize)]
struct GameProgress {
    save_version: u32,  // Increment when format changes
    // ...
}
```

This lets you migrate old saves when your format changes.

### 4. Meaningful Save Metadata

```rust
let save = GameSaveData::new(progress)
    .with_name("Before Final Boss")  // User can see this
    .with_play_time(total_seconds)   // For display
    .with_metadata("chapter", "5");  // For filtering
```

### 5. Autosave vs Manual Saves

Keep autosaves separate from manual saves:

```rust
// Autosave goes to a dedicated slot
save_system.save_game("autosave", &progress)?;

// Manual saves go to slot1, slot2, etc.
save_system.save_game("slot1", &progress)?;
```

## Error Handling

Always handle save errors gracefully:

```rust
match save_system.save_game("slot1", &progress) {
    Ok(_) => {
        show_notification("Game saved!");
    }
    Err(SaveError::Io(e)) => {
        show_error(&format!("Couldn't save: {}", e));
    }
    Err(e) => {
        show_error(&format!("Save failed: {}", e));
    }
}
```

## Testing Save Systems

For testing, use a temporary directory:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_save_and_load() {
        let dir = tempdir().unwrap();
        let save_manager = SaveManager::new(dir.path()).unwrap();

        let progress = GameProgress::default();
        save_manager.save_game("test", &progress).unwrap();

        let loaded: GameSaveData<GameProgress> = 
            save_manager.load_game("test").unwrap();

        assert_eq!(loaded.data.player.level, 1);
    }
}
```
