# Save System

Ryo2D provides a comprehensive 3-tier save system with automatic compression. Each tier serves a different purpose, from engine settings to game-specific data.

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────┐
│                      SaveManager                            │
├─────────────────────────────────────────────────────────────┤
│  Tier 1: SystemState     │  Engine settings, audio, display │
│  Tier 2: WorldState      │  ECS snapshots, entity data      │
│  Tier 3: GameSaveData<T> │  Custom game data, progress      │
└─────────────────────────────────────────────────────────────┘
                           │
                           ▼
              ┌────────────────────────┐
              │   DEFLATE Compression  │
              │   + Version Header     │
              └────────────────────────┘
                           │
                           ▼
              ┌────────────────────────┐
              │    {workspace}/data/   │
              │  ├── system.ryo-system │
              │  ├── slot1.ryo-world   │
              │  └── slot1.ryo-save    │
              └────────────────────────┘
```

## The Three Tiers

### Tier 1: System State

Low-level engine settings that persist across sessions. This includes:

- **Audio levels**: Master, music, and SFX volumes
- **Display settings**: Resolution, fullscreen, VSync
- **Custom key-value storage**: For engine extensions

```rust
use ryo2d::prelude::*;

// Load existing or get defaults
let mut system = save_manager.load_system()?;

// Modify settings
system.master_volume = 0.8;
system.music_volume = 0.5;
system.display.fullscreen = true;
system.set_custom("language", "en");

// Save to disk
save_manager.save_system(&system)?;
```

System state is always saved to `system.ryo-system` - there's only one system state per game.

### Tier 2: World State

ECS world snapshots for saving/loading game worlds. Captures:

- **Entity snapshots**: All entities with their IDs
- **Component data**: Serializable component values
- **World metadata**: Scene name, level info, etc.

```rust
use ryo2d::prelude::*;

// Create a world snapshot
let mut world_state = WorldState::new();
world_state.set_metadata("scene", "dungeon_level_3");
world_state.set_metadata("checkpoint", "boss_room");

// Add entity data (typically done by iterating your ECS world)
world_state.add_entity(EntitySnapshot {
    id: 42,
    components: HashMap::new(), // Serialized component data
});

// Save to a named slot
save_manager.save_world("autosave", &world_state)?;

// Load it back
let loaded = save_manager.load_world("autosave")?;
println!("Scene: {:?}", loaded.get_metadata("scene"));
```

### Tier 3: Game Data

Generic, user-defined save data. This is where you put game-specific information:

- **Player progress**: Level, score, achievements
- **Inventory**: Items, equipment, currencies
- **Quest state**: Completed quests, active objectives
- **Any custom data**: Whatever your game needs

```rust
use ryo2d::prelude::*;
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize)]
struct PlayerProgress {
    level: u32,
    experience: u64,
    gold: u32,
    unlocked_abilities: Vec<String>,
    current_quest: Option<String>,
}

// Create your game data
let progress = PlayerProgress {
    level: 15,
    experience: 45000,
    gold: 1250,
    unlocked_abilities: vec!["fireball".into(), "shield".into()],
    current_quest: Some("defeat_dragon".into()),
};

// Save to a slot
save_manager.save_game("slot1", &progress)?;

// Load it back (includes metadata like timestamp)
let save: GameSaveData<PlayerProgress> = save_manager.load_game("slot1")?;
println!("Level: {}, saved at: {}", save.data.level, save.timestamp);
```

## Save Manager Setup

### From Workspace (Recommended)

The easiest way to create a `SaveManager` is from your workspace:

```rust
let workspace = Workspace::discover()?;
let save_manager = workspace.create_save_manager()?;
```

This uses the path configured in `ryo.toml`:

```toml
[paths]
save_data = "data"  # Default, saves go to {project}/data/
```

### Manual Creation

You can also create a `SaveManager` with any path:

```rust
let save_manager = SaveManager::new("./my_saves")?;
```

### Runtime Path Changes

Change the save directory at runtime:

```rust
// Change workspace config (affects future SaveManagers)
workspace.set_save_data_path("new_saves");

// Or change an existing SaveManager directly
save_manager.set_save_dir("./different_location")?;
```

## Save Metadata

`GameSaveData<T>` includes useful metadata:

```rust
// Create save with metadata
let save = GameSaveData::new(&my_data)
    .with_name("Before Boss Fight")
    .with_play_time(7200)  // 2 hours in seconds
    .with_metadata("difficulty", "hard")
    .with_metadata("chapter", "3");

save_manager.save_game_with_info("slot1", save)?;

// Access metadata when loading
let loaded: GameSaveData<MyData> = save_manager.load_game("slot1")?;
println!("Save name: {:?}", loaded.name);
println!("Play time: {} seconds", loaded.play_time);
println!("Saved at: {}", loaded.timestamp);  // Unix timestamp
```

## Slot Management

### Listing Saves

```rust
// List all game saves
let game_saves = save_manager.list_game_saves()?;
for slot in &game_saves {
    println!("Found save: {}", slot);
}

// List all world saves
let world_saves = save_manager.list_world_saves()?;
```

### Checking Existence

```rust
if save_manager.has_game_save("slot1") {
    let save = save_manager.load_game("slot1")?;
    // Continue from save
} else {
    // Start new game
}
```

### Deleting Saves

```rust
save_manager.delete_game("slot1")?;
save_manager.delete_world("autosave")?;
```

## Compression

All saves are automatically compressed using DEFLATE. You can adjust the compression level:

```rust
let mut save_manager = SaveManager::new("./saves")?;

// Compression levels:
// 0: No compression (fastest, largest files)
// 1-3: Fast compression
// 4-6: Balanced (default is 6)
// 7-9: Best compression (slowest, smallest files)
save_manager.set_compression_level(9);
```

For manual compression of arbitrary data:

```rust
use ryo2d::prelude::{compress_data, decompress_data};

let original = b"Some data to compress";
let compressed = compress_data(original)?;
let decompressed = decompress_data(&compressed)?;
assert_eq!(original.as_slice(), decompressed.as_slice());
```

## File Format

Save files use a binary format:

| Section | Size | Description |
|---------|------|-------------|
| Magic | 4 bytes | `RYO2` identifier |
| Version | 4 bytes | Format version (little-endian u32) |
| Data | Variable | DEFLATE-compressed RON data |

This allows the engine to:
- Quickly identify valid save files
- Reject incompatible versions with clear error messages
- Efficiently compress the data

## Error Handling

Save operations return `SaveResult<T>`, which uses `SaveError`:

```rust
use ryo2d::prelude::*;

match save_manager.load_game::<MyData>("slot1") {
    Ok(save) => {
        // Use the save
    }
    Err(SaveError::NotFound(path)) => {
        println!("Save not found: {:?}", path);
    }
    Err(SaveError::VersionMismatch { expected, found }) => {
        println!("Save version {} incompatible (need {})", found, expected);
    }
    Err(SaveError::Deserialization(msg)) => {
        println!("Corrupted save: {}", msg);
    }
    Err(e) => {
        println!("Save error: {}", e);
    }
}
```

## Best Practices

### 1. Use Meaningful Slot Names

```rust
// Good
save_manager.save_game("autosave", &data)?;
save_manager.save_game("quicksave", &data)?;
save_manager.save_game("before_boss", &data)?;

// Bad - slot names can't contain path separators
// save_manager.save_game("saves/slot1", &data)?;  // Error!
```

### 2. Version Your Save Data

Include a version field in your save structs:

```rust
#[derive(Serialize, Deserialize)]
struct MyGameSave {
    version: u32,  // Increment when format changes
    // ... other fields
}
```

### 3. Separate Concerns

Use the appropriate tier for each type of data:

- **SystemState**: Settings the player can change in options menu
- **WorldState**: The game world that needs to be reconstructed
- **GameSaveData**: Progress, inventory, stats

### 4. Autosave Regularly

```rust
// In your game update loop
if should_autosave {
    save_manager.save_game("autosave", &game_state)?;
    save_manager.save_world("autosave", &world_snapshot)?;
}
```

### 5. Handle Missing Saves Gracefully

```rust
let system = save_manager.load_system()?;  // Returns defaults if no save

// For game saves, check existence first
if save_manager.has_game_save("slot1") {
    // Load and continue
} else {
    // Start fresh
}
```
