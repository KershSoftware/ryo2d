# Asset Management

Ferrum2D provides a centralized asset management system for loading, caching, and hot-reloading game resources like sprites, textures, and audio files.

## Overview

The asset system provides:

- **Centralized loading** - All assets go through `AssetManager`
- **Automatic caching** - Load once, use everywhere
- **Reference counting** - Assets are freed when no longer used
- **Hot reloading** - Update assets at runtime during development
- **Type-safe handles** - `SpriteHandle` prevents use-after-free bugs

## Quick Start

```rust
use ferrum2d::prelude::*;

struct MyGame {
    player_sprite: Option<SpriteHandle>,
}

impl Game for MyGame {
    fn init(&mut self, ctx: &mut GameContext) {
        // Load a sprite through the asset manager
        match ctx.assets.load_sprite("assets/sprites/player.png") {
            Ok(sprite) => {
                self.player_sprite = Some(sprite);
            }
            Err(e) => {
                eprintln!("Failed to load sprite: {}", e);
            }
        }
    }

    fn update(&mut self, ctx: &mut GameContext, _dt: f32) {
        // Use the sprite...
    }
}
```

## AssetManager

The `AssetManager` is the central hub for all asset operations. It's available through `GameContext`:

```rust
fn init(&mut self, ctx: &mut GameContext) {
    // Access asset manager through context
    let sprite = ctx.assets.load_sprite("player.png")?;
}
```

### Base Path

The asset manager resolves paths relative to a base directory:

```rust
// Set base path for all asset loading
ctx.assets.set_base_path("assets");

// Now paths are relative to "assets/"
let sprite = ctx.assets.load_sprite("sprites/player.png")?;
// Actually loads: assets/sprites/player.png
```

You can also create an `AssetManager` with a specific base path:

```rust
let assets = AssetManager::with_base_path("game_data/resources");
```

### Loading Sprites

```rust
// Load a sprite - returns a lightweight handle
let player = ctx.assets.load_sprite("sprites/player.png")?;

// Same path returns cached version (no disk read!)
let player2 = ctx.assets.load_sprite("sprites/player.png")?;

// Both handles point to the same sprite data
assert_eq!(player.ref_count(), 2);
```

Supported image formats:
- PNG (recommended for pixel art)
- JPEG
- GIF (first frame only)
- BMP

### Creating Sprites Programmatically

For procedurally generated graphics:

```rust
// Create a solid color sprite
let red_square = Sprite::solid_rect(32, 32, 255, 0, 0, 255);

// Wrap in a handle (not cached, since it's generated)
let handle = ctx.assets.create_sprite(red_square, "generated/red_square");
```

### Cache Management

The asset manager tracks what's loaded and manages memory automatically:

```rust
// Check if a sprite is already loaded
if ctx.assets.is_sprite_cached("sprites/player.png") {
    println!("Player sprite is in cache!");
}

// Get cache statistics
let stats = ctx.assets.stats();
println!("Sprites cached: {}", stats.sprites_cached);
println!("Total references: {}", stats.sprites_total_refs);

// Count loaded sprites
let count = ctx.assets.sprite_count();
```

### Garbage Collection

Assets stay in memory as long as any handle references them. When handles are dropped, the asset becomes eligible for garbage collection:

```rust
// Remove sprites that have no external references
let freed = ctx.assets.gc_sprites();
println!("Freed {} unused sprites", freed);
```

**When to call GC:**
- After scene transitions
- When memory is constrained
- Periodically during loading screens

```rust
fn on_scene_change(&mut self, ctx: &mut GameContext) {
    // Clear our local handles
    self.sprites.clear();
    
    // Let asset manager clean up
    ctx.assets.gc_sprites();
}
```

### Clearing the Cache

For complete cache reset:

```rust
// Clear all cached sprites
ctx.assets.clear_sprite_cache();
```

> **Note:** Existing handles will still work, but reloading the same path will read from disk again.

## SpriteHandle

`SpriteHandle` is a lightweight, clone-friendly reference to sprite data.

### Why Handles?

Instead of passing around raw `Sprite` structs (which would copy all pixel data), we use handles:

```rust
// Bad: Copying pixel data is expensive!
// let sprite: Sprite = ...
// let copy = sprite.clone(); // Copies all pixels!

// Good: Handles are cheap to clone
let handle: SpriteHandle = ctx.assets.load_sprite("player.png")?;
let handle2 = handle.clone(); // Just increments reference count
```

### Handle Properties

```rust
let sprite = ctx.assets.load_sprite("player.png")?;

// Get dimensions
let width = sprite.width();   // e.g., 64
let height = sprite.height(); // e.g., 64

// Get the original path
let path = sprite.path(); // "assets/sprites/player.png"

// Check reference count (useful for debugging)
let refs = sprite.ref_count(); // How many handles exist
```

### Accessing Sprite Data

For rendering or other operations, you can access the underlying sprite:

```rust
// Option 1: Get a read guard (lock scope matters!)
let sprite_data = handle.sprite();
sprite_data.draw(frame, screen_width, screen_height, x, y);
// Lock released when sprite_data goes out of scope

// Option 2: Use with_sprite for explicit scoping (preferred)
handle.with_sprite(|sprite| {
    sprite.draw(frame, screen_width, screen_height, x, y);
});
// Lock is definitely released here
```

The `with_sprite()` method is preferred because it makes the lock scope explicit and ensures the lock is released promptly.

### Multiple Entities, One Sprite

Handles make sharing assets natural:

```rust
fn spawn_enemies(&mut self, ctx: &mut GameContext, count: usize) {
    // Load the sprite once
    let enemy_sprite = ctx.assets.load_sprite("enemy.png").unwrap();
    
    for i in 0..count {
        let entity = ctx.world.spawn();
        ctx.world.add_component(entity, Transform::new(i as f32 * 50.0, 100.0));
        
        // Clone is cheap - just increments ref count
        ctx.world.add_component(entity, SpriteRenderer {
            sprite: enemy_sprite.clone(),
            ..Default::default()
        });
    }
}
// One sprite in memory, many enemies on screen!
```

## Async Loading

For large assets or when you need to avoid frame hitches, use asynchronous loading. Assets load in a background thread while your game continues running.

### Basic Async Loading

```rust
struct MyGame {
    // Store async handles for assets being loaded
    boss_texture: Option<AsyncSpriteHandle>,
    // Store completed sprites
    boss_sprite: Option<SpriteHandle>,
}

impl Game for MyGame {
    fn init(&mut self, ctx: &mut GameContext) {
        // Request async load - returns immediately
        self.boss_texture = Some(ctx.assets.load_sprite_async("boss_4k.png"));
    }
    
    fn update(&mut self, ctx: &mut GameContext, _dt: f32) {
        // IMPORTANT: Process async loads each frame
        ctx.assets.process_async_loads();
        
        // Check if our asset is ready
        if let Some(pending) = &self.boss_texture {
            match pending.state() {
                LoadingState::Loading => {
                    // Still loading - show placeholder or loading indicator
                }
                LoadingState::Loaded => {
                    // Transfer to our sprite field
                    self.boss_sprite = pending.get();
                    self.boss_texture = None; // Done with async handle
                }
                LoadingState::Failed(reason) => {
                    eprintln!("Failed to load boss: {}", reason);
                    self.boss_texture = None;
                }
            }
        }
    }
}
```

### AsyncSpriteHandle API

The `AsyncSpriteHandle` provides methods to check loading status:

```rust
let pending = ctx.assets.load_sprite_async("large_texture.png");

// Check state
pending.is_loading()   // true while loading
pending.is_loaded()    // true on success
pending.is_failed()    // true on failure
pending.is_complete()  // true when done (success or failure)

// Get the loaded sprite (None if not ready)
let sprite: Option<SpriteHandle> = pending.get();

// Get sprite or fallback
let sprite = pending.get_or(&placeholder_sprite);

// Get detailed state
match pending.state() {
    LoadingState::Loading => { /* still loading */ }
    LoadingState::Loaded => { /* success */ }
    LoadingState::Failed(reason) => { /* error message */ }
}
```

### Cached Assets Load Instantly

If an asset is already cached, `load_sprite_async` returns immediately as loaded:

```rust
// First load goes through background thread
let first = ctx.assets.load_sprite_async("player.png");

// ... time passes, first completes ...
ctx.assets.process_async_loads();

// Second load returns immediately (cached!)
let second = ctx.assets.load_sprite_async("player.png");
assert!(second.is_loaded()); // Already done
```

### Pending Load Tracking

Monitor async loading status:

```rust
// How many assets are currently loading?
let pending = ctx.assets.pending_load_count();

// Are any loads in progress?
if ctx.assets.has_pending_loads() {
    println!("Loading {} assets...", pending);
}

// Full stats
let stats = ctx.assets.stats();
println!("Cached: {}, Loading: {}", stats.sprites_cached, stats.sprites_loading);
```

### When to Use Async vs Sync

| Use Case | Recommended |
|----------|-------------|
| Small sprites at init | Sync (`load_sprite`) |
| Large textures | Async |
| Level transitions | Sync (during loading screen) |
| Streaming open world | Async |
| UI elements | Sync at init |
| Dynamic content | Async |

**Sync loading** is simpler - use it when blocking is acceptable (init, loading screens).

**Async loading** prevents hitches - use for large assets or runtime loading.

## Hot Reloading

During development, you can modify asset files and see changes instantly without restarting the game.

### Enabling Hot Reload

```rust
fn init(&mut self, ctx: &mut GameContext) {
    // Watch directories for file changes
    if let Err(e) = ctx.assets.enable_hot_reload(vec!["assets/sprites", "assets/textures"]) {
        eprintln!("Hot reload not available: {}", e);
    }
    
    // Load your assets normally
    self.player = ctx.assets.load_sprite("assets/sprites/player.png").ok();
}
```

### Processing Changes

Call `process_reloads()` each frame to apply pending changes:

```rust
fn update(&mut self, ctx: &mut GameContext, _dt: f32) {
    // Check for and apply any asset changes
    let reloaded = ctx.assets.process_reloads();
    if reloaded > 0 {
        println!("Hot reloaded {} assets!", reloaded);
    }
    
    // ... rest of update
}
```

### How It Works

1. A background thread monitors watched directories using the `notify` crate
2. When a file changes, an event is queued
3. `process_reloads()` checks the queue and reloads modified assets
4. All handles pointing to that asset automatically see the new data

```text
┌─────────────────┐     ┌──────────────────┐     ┌─────────────────┐
│  File System    │────►│  Watcher Thread  │────►│  Main Thread    │
│  (file saved)   │     │  (background)    │     │  (process_      │
│                 │     │                  │     │   reloads)      │
└─────────────────┘     └──────────────────┘     └─────────────────┘
                             │                         ▲
                             │    mpsc channel         │
                             └─────────────────────────┘
```

### Watched Extensions

By default, hot reload watches for common asset types:
- **Images:** png, jpg, jpeg, gif, bmp
- **Audio:** wav, ogg, mp3, flac

### Disabling Hot Reload

For release builds or to stop watching:

```rust
// Check if hot reload is active
if ctx.assets.is_hot_reload_enabled() {
    ctx.assets.disable_hot_reload();
}
```

### Development Workflow

Hot reload enables rapid iteration:

1. Run your game in debug mode
2. Open sprite in image editor
3. Make changes and save
4. See changes instantly in-game!

```rust
// A typical development setup
fn init(&mut self, ctx: &mut GameContext) {
    // Only enable in debug builds
    #[cfg(debug_assertions)]
    {
        ctx.assets.enable_hot_reload(vec!["assets"]).ok();
    }
    
    // Load assets...
}
```

## Error Handling

Asset operations can fail. Handle errors gracefully:

```rust
use ferrum2d::prelude::*;

fn load_assets(&mut self, ctx: &mut GameContext) {
    match ctx.assets.load_sprite("player.png") {
        Ok(sprite) => {
            self.player_sprite = Some(sprite);
        }
        Err(AssetError::NotFound(path)) => {
            eprintln!("Asset not found: {:?}", path);
            // Maybe use a placeholder?
        }
        Err(AssetError::LoadError { path, reason }) => {
            eprintln!("Failed to load {:?}: {}", path, reason);
        }
        Err(e) => {
            eprintln!("Asset error: {}", e);
        }
    }
}
```

### Error Types

| Error | Cause |
|-------|-------|
| `NotFound(path)` | File doesn't exist |
| `LoadError { path, reason }` | File exists but couldn't be parsed |
| `TypeMismatch { path, expected }` | Wrong asset type |
| `WatchError { reason }` | Hot reload setup failed |

## Performance Tips

### Load During Init

Load assets during `init()` or loading screens, not during gameplay:

```rust
// Good: Load once during init
fn init(&mut self, ctx: &mut GameContext) {
    self.sprites.insert("player", ctx.assets.load_sprite("player.png")?);
    self.sprites.insert("enemy", ctx.assets.load_sprite("enemy.png")?);
}

fn update(&mut self, ctx: &mut GameContext, _dt: f32) {
    // Just use the cached handles
    let sprite = self.sprites.get("player").unwrap();
}
```

```rust
// Bad: Loading every frame!
fn update(&mut self, ctx: &mut GameContext, _dt: f32) {
    let sprite = ctx.assets.load_sprite("player.png")?; // Disk I/O check each frame!
}
```

Even with caching, `load_sprite()` checks the cache hash map each call. Store handles in your game struct.

### Clone Handles Freely

Handles are designed to be cloned:

```rust
// This is fine and cheap
let sprite = self.player_sprite.clone();
ctx.world.add_component(entity, SpriteRenderer { sprite, ..Default::default() });
```

### Batch Loading

For many assets, consider a loading phase:

```rust
fn load_level(&mut self, ctx: &mut GameContext, level: &str) -> AssetResult<()> {
    let sprite_files = ["ground.png", "wall.png", "spike.png", "coin.png", "door.png"];
    
    for file in sprite_files {
        let path = format!("levels/{}/{}", level, file);
        let handle = ctx.assets.load_sprite(&path)?;
        self.level_sprites.insert(file.to_string(), handle);
    }
    
    Ok(())
}
```

### GC Strategy

Don't GC every frame - it iterates all cached assets:

```rust
// Good: GC at natural transition points
fn on_level_complete(&mut self, ctx: &mut GameContext) {
    self.level_sprites.clear(); // Release our handles
    ctx.assets.gc_sprites();    // Clean up
    self.load_next_level(ctx);
}

// Bad: GC every frame
fn update(&mut self, ctx: &mut GameContext, _dt: f32) {
    ctx.assets.gc_sprites(); // Unnecessary work each frame!
}
```

## Architecture Deep Dive

Understanding the internals helps you use the system effectively.

### Reference Counting with Arc

`SpriteHandle` uses `Arc<RwLock<Sprite>>` internally:

```rust
// Simplified internal structure
pub struct SpriteHandle {
    inner: Arc<RwLock<Sprite>>,  // Shared, thread-safe reference
    path: Arc<PathBuf>,          // Original file path
}
```

- **`Arc`** (Atomic Reference Counted) allows multiple owners
- **`RwLock`** allows multiple readers or one writer (needed for hot reload)

### Cache Structure

The `AssetManager` maintains a hash map of loaded assets:

```rust
// Simplified internal structure
pub struct AssetManager {
    base_path: PathBuf,
    sprites: HashMap<PathBuf, SpriteHandle>,
    watcher: Option<HotReloadWatcher>,
}
```

When you call `load_sprite("player.png")`:
1. Path is resolved relative to base path
2. Check if path is in `sprites` hash map
3. If cached: clone the handle (cheap)
4. If not cached: load from disk, create handle, insert into cache

### Hot Reload Threading

The hot reload system uses a background thread:

```rust
// Main thread                    // Watcher thread
// -----------                    // --------------
enable_hot_reload() ──────────►  spawn watcher
                                  │
                                  ▼
                                  watch directories
                                  │
// game loop                      │ file changes
// ---------                      ▼
process_reloads() ◄────────────  send event via channel
    │
    ▼
  reload asset
  update RwLock
    │
    ▼
  all handles see new data
```

The `RwLock` is key: when `process_reloads()` updates sprite data, any code holding a read lock (via `with_sprite()` or `sprite()`) will see the new data on their next access.

## Complete Example

Here's a complete game showing asset management patterns:

```rust
use ferrum2d::prelude::*;
use std::collections::HashMap;

struct AssetDemo {
    sprites: HashMap<String, SpriteHandle>,
    spawn_timer: f32,
}

impl AssetDemo {
    fn new() -> Self {
        AssetDemo {
            sprites: HashMap::new(),
            spawn_timer: 0.0,
        }
    }
}

impl Game for AssetDemo {
    fn init(&mut self, ctx: &mut GameContext) {
        // Set up base path
        ctx.assets.set_base_path("assets");
        
        // Enable hot reload in debug builds
        #[cfg(debug_assertions)]
        {
            if let Err(e) = ctx.assets.enable_hot_reload(vec!["assets/sprites"]) {
                eprintln!("Hot reload unavailable: {}", e);
            }
        }
        
        // Pre-load all game sprites
        let sprite_names = ["player", "enemy", "bullet", "coin"];
        for name in sprite_names {
            match ctx.assets.load_sprite(&format!("sprites/{}.png", name)) {
                Ok(sprite) => {
                    self.sprites.insert(name.to_string(), sprite);
                }
                Err(e) => {
                    eprintln!("Failed to load {}: {}", name, e);
                    // Create placeholder
                    let placeholder = Sprite::solid_rect(32, 32, 255, 0, 255, 255);
                    let handle = ctx.assets.create_sprite(placeholder, format!("placeholder/{}", name));
                    self.sprites.insert(name.to_string(), handle);
                }
            }
        }
        
        // Spawn initial player
        let player = ctx.world.spawn();
        ctx.world.add_component(player, Transform::new(400.0, 300.0));
        ctx.world.add_component(player, SpriteRenderer {
            sprite: self.sprites.get("player").unwrap().clone(),
            ..Default::default()
        });
    }
    
    fn update(&mut self, ctx: &mut GameContext, dt: f32) {
        // Process hot reloads
        let reloaded = ctx.assets.process_reloads();
        if reloaded > 0 {
            println!("[Dev] Reloaded {} sprites", reloaded);
        }
        
        // Spawn enemies periodically
        self.spawn_timer += dt;
        if self.spawn_timer > 2.0 {
            self.spawn_timer = 0.0;
            
            let enemy = ctx.world.spawn();
            ctx.world.add_component(enemy, Transform::new(
                rand::random::<f32>() * 800.0,
                rand::random::<f32>() * 600.0,
            ));
            ctx.world.add_component(enemy, SpriteRenderer {
                sprite: self.sprites.get("enemy").unwrap().clone(),
                ..Default::default()
            });
        }
        
        // Print cache stats on F1
        if ctx.input.key_pressed(KeyCode::F1) {
            let stats = ctx.assets.stats();
            println!("Cache: {} sprites, {} total refs", 
                stats.sprites_cached, 
                stats.sprites_total_refs
            );
        }
    }
}

fn main() {
    EngineBuilder::new()
        .title("Asset Management Demo")
        .resolution(800, 600)
        .with_game(AssetDemo::new())
        .build()
        .run()
        .unwrap();
}
```

## Summary

| Feature | API |
|---------|-----|
| Load sprite (sync) | `ctx.assets.load_sprite("path.png")` |
| Load sprite (async) | `ctx.assets.load_sprite_async("path.png")` |
| Process async loads | `ctx.assets.process_async_loads()` |
| Check pending count | `ctx.assets.pending_load_count()` |
| Create sprite | `ctx.assets.create_sprite(sprite, "name")` |
| Set base path | `ctx.assets.set_base_path("assets")` |
| Check cache | `ctx.assets.is_sprite_cached("path.png")` |
| Cache stats | `ctx.assets.stats()` |
| Free unused | `ctx.assets.gc_sprites()` |
| Clear cache | `ctx.assets.clear_sprite_cache()` |
| Enable hot reload | `ctx.assets.enable_hot_reload(paths)` |
| Process hot reload | `ctx.assets.process_reloads()` |
| Disable hot reload | `ctx.assets.disable_hot_reload()` |
