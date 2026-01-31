# Asset Loading Patterns

This example demonstrates common patterns for managing game assets effectively.

## Basic Asset Loading

The simplest approach - load sprites during initialization:

```rust
use ryo2d::prelude::*;

struct BasicGame {
    player_sprite: Option<SpriteHandle>,
    enemy_sprite: Option<SpriteHandle>,
}

impl Game for BasicGame {
    fn init(&mut self, ctx: &mut GameContext) {
        // Load sprites and store handles
        self.player_sprite = ctx.assets.load_sprite("sprites/player.png").ok();
        self.enemy_sprite = ctx.assets.load_sprite("sprites/enemy.png").ok();
        
        // Create a player entity using the loaded sprite
        if let Some(sprite) = &self.player_sprite {
            let player = ctx.world.spawn();
            ctx.world.add_component(player, Transform::new(400.0, 300.0));
            ctx.world.add_component(player, Renderable::sprite(sprite.clone()));
        }
    }
    
    fn update(&mut self, ctx: &mut GameContext, _dt: f32) {
        // Game logic...
    }
}
```

## Sprite Registry Pattern

For games with many sprites, use a registry:

```rust
use ryo2d::prelude::*;
use std::collections::HashMap;

/// A registry of named sprites for easy access
struct SpriteRegistry {
    sprites: HashMap<String, SpriteHandle>,
}

impl SpriteRegistry {
    fn new() -> Self {
        SpriteRegistry {
            sprites: HashMap::new(),
        }
    }
    
    /// Load a sprite and register it with a name
    fn load(&mut self, assets: &mut AssetManager, name: &str, path: &str) -> AssetResult<()> {
        let handle = assets.load_sprite(path)?;
        self.sprites.insert(name.to_string(), handle);
        Ok(())
    }
    
    /// Get a sprite by name (clones the handle)
    fn get(&self, name: &str) -> Option<SpriteHandle> {
        self.sprites.get(name).cloned()
    }
    
    /// Check if a sprite is registered
    fn contains(&self, name: &str) -> bool {
        self.sprites.contains_key(name)
    }
}

struct RegistryGame {
    sprites: SpriteRegistry,
}

impl Game for RegistryGame {
    fn init(&mut self, ctx: &mut GameContext) {
        ctx.assets.set_base_path("assets");
        
        // Load all game sprites
        let sprite_list = [
            ("player", "sprites/player.png"),
            ("enemy_small", "sprites/enemy_small.png"),
            ("enemy_large", "sprites/enemy_large.png"),
            ("bullet", "sprites/bullet.png"),
            ("coin", "sprites/coin.png"),
            ("heart", "sprites/heart.png"),
            ("explosion", "sprites/explosion.png"),
        ];
        
        for (name, path) in sprite_list {
            if let Err(e) = self.sprites.load(ctx.assets, name, path) {
                eprintln!("Failed to load '{}': {}", name, e);
            }
        }
        
        // Spawn player using registry
        let player = ctx.world.spawn();
        ctx.world.add_component(player, Transform::new(400.0, 300.0));
        if let Some(sprite) = self.sprites.get("player") {
            ctx.world.add_component(player, Renderable::sprite(sprite));
        }
    }
    
    fn update(&mut self, ctx: &mut GameContext, _dt: f32) {
        // Spawn enemies using registry
        if ctx.input.key_pressed(KeyCode::KeyE) {
            let enemy = ctx.world.spawn();
            ctx.world.add_component(enemy, Transform::new(100.0, 100.0));
            
            if let Some(sprite) = self.sprites.get("enemy_small") {
                ctx.world.add_component(enemy, Renderable::sprite(sprite));
            }
        }
    }
}
```

## Fallback Sprites

Handle missing assets gracefully with placeholders:

```rust
use ryo2d::prelude::*;
use std::collections::HashMap;

struct RobustGame {
    sprites: HashMap<String, SpriteHandle>,
    placeholder: Option<SpriteHandle>,
}

impl RobustGame {
    fn new() -> Self {
        RobustGame {
            sprites: HashMap::new(),
            placeholder: None,
        }
    }
    
    /// Load sprite with fallback to placeholder
    fn load_or_placeholder(
        &mut self,
        assets: &mut AssetManager,
        name: &str,
        path: &str,
    ) {
        match assets.load_sprite(path) {
            Ok(sprite) => {
                self.sprites.insert(name.to_string(), sprite);
            }
            Err(e) => {
                eprintln!("Warning: Could not load '{}': {}", path, e);
                // Use placeholder if available
                if let Some(placeholder) = &self.placeholder {
                    self.sprites.insert(name.to_string(), placeholder.clone());
                }
            }
        }
    }
    
    /// Get a sprite, with fallback to placeholder
    fn get_sprite(&self, name: &str) -> Option<SpriteHandle> {
        self.sprites.get(name).cloned()
            .or_else(|| self.placeholder.clone())
    }
}

impl Game for RobustGame {
    fn init(&mut self, ctx: &mut GameContext) {
        // Create a magenta "missing texture" placeholder
        let placeholder_sprite = Sprite::solid_rect(32, 32, 255, 0, 255, 255);
        self.placeholder = Some(ctx.assets.create_sprite(
            placeholder_sprite,
            "placeholder"
        ));
        
        // Now load sprites - missing ones will use placeholder
        self.load_or_placeholder(ctx.assets, "player", "sprites/player.png");
        self.load_or_placeholder(ctx.assets, "enemy", "sprites/MISSING.png"); // Will use placeholder
        
        // Spawn entities - they'll render even with missing assets
        let player = ctx.world.spawn();
        ctx.world.add_component(player, Transform::new(400.0, 300.0));
        if let Some(sprite) = self.get_sprite("player") {
            ctx.world.add_component(player, Renderable::sprite(sprite));
        }
    }
    
    fn update(&mut self, ctx: &mut GameContext, _dt: f32) {}
}
```

## Level-Based Asset Loading

Load and unload assets per level for memory efficiency:

```rust
use ryo2d::prelude::*;
use std::collections::HashMap;

struct LevelAssets {
    sprites: HashMap<String, SpriteHandle>,
}

struct LevelGame {
    current_level: usize,
    assets: LevelAssets,
}

impl LevelGame {
    fn load_level(&mut self, ctx: &mut GameContext, level: usize) {
        // Clear current level assets
        self.assets.sprites.clear();
        
        // Garbage collect to free memory
        let freed = ctx.assets.gc_sprites();
        println!("Freed {} sprites from previous level", freed);
        
        // Load level-specific assets
        let level_path = format!("levels/level{}", level);
        ctx.assets.set_base_path(&level_path);
        
        // Common level sprites
        let sprites = ["tileset", "background", "enemies", "items"];
        for name in sprites {
            let path = format!("{}.png", name);
            if let Ok(sprite) = ctx.assets.load_sprite(&path) {
                self.assets.sprites.insert(name.to_string(), sprite);
            }
        }
        
        self.current_level = level;
        println!("Loaded level {} with {} sprites", 
            level, 
            self.assets.sprites.len()
        );
    }
}

impl Game for LevelGame {
    fn init(&mut self, ctx: &mut GameContext) {
        self.load_level(ctx, 1);
    }
    
    fn update(&mut self, ctx: &mut GameContext, _dt: f32) {
        // Level transitions
        if ctx.input.key_pressed(KeyCode::KeyN) {
            let next = self.current_level + 1;
            self.load_level(ctx, next);
        }
        
        if ctx.input.key_pressed(KeyCode::KeyP) && self.current_level > 1 {
            let prev = self.current_level - 1;
            self.load_level(ctx, prev);
        }
    }
}
```

## Hot Reload Development Setup

Optimal setup for development iteration:

```rust
use ryo2d::prelude::*;
use std::collections::HashMap;

struct DevGame {
    sprites: HashMap<String, SpriteHandle>,
    reload_count: usize,
}

impl Game for DevGame {
    fn init(&mut self, ctx: &mut GameContext) {
        ctx.assets.set_base_path("assets");
        
        // Enable hot reload in debug builds only
        #[cfg(debug_assertions)]
        {
            match ctx.assets.enable_hot_reload(vec![
                "assets/sprites",
                "assets/tilesets",
                "assets/ui",
            ]) {
                Ok(()) => {
                    println!("[Dev] Hot reload enabled - edit assets to see live updates!");
                }
                Err(e) => {
                    eprintln!("[Dev] Hot reload unavailable: {}", e);
                }
            }
        }
        
        // Load initial sprites
        self.load_sprites(ctx);
    }
    
    fn update(&mut self, ctx: &mut GameContext, _dt: f32) {
        // Process hot reloads
        let reloaded = ctx.assets.process_reloads();
        if reloaded > 0 {
            self.reload_count += reloaded;
            println!("[Dev] Reloaded {} assets (total: {})", reloaded, self.reload_count);
        }
        
        // Debug: Print cache stats
        if ctx.input.key_pressed(KeyCode::F1) {
            let stats = ctx.assets.stats();
            println!("[Debug] Asset Cache:");
            println!("  Sprites: {}", stats.sprites_cached);
            println!("  Total refs: {}", stats.sprites_total_refs);
            println!("  Hot reloads this session: {}", self.reload_count);
        }
        
        // Debug: Force reload all (for testing)
        if ctx.input.key_pressed(KeyCode::F5) {
            println!("[Dev] Force reloading all assets...");
            ctx.assets.clear_sprite_cache();
            self.sprites.clear();
            self.load_sprites(ctx);
        }
    }
}

impl DevGame {
    fn load_sprites(&mut self, ctx: &mut GameContext) {
        let to_load = [
            ("player", "sprites/player.png"),
            ("enemy", "sprites/enemy.png"),
            ("tile_grass", "tilesets/grass.png"),
            ("tile_stone", "tilesets/stone.png"),
            ("button", "ui/button.png"),
        ];
        
        for (name, path) in to_load {
            match ctx.assets.load_sprite(path) {
                Ok(sprite) => {
                    self.sprites.insert(name.to_string(), sprite);
                }
                Err(e) => {
                    eprintln!("Failed to load '{}': {}", name, e);
                }
            }
        }
    }
}
```

## Animated Sprite Loading

Loading sprite sheets for animations:

```rust
use ryo2d::prelude::*;

struct AnimatedGame {
    walk_animation: Option<AnimatedSprite>,
}

impl Game for AnimatedGame {
    fn init(&mut self, ctx: &mut GameContext) {
        // Load the sprite sheet
        match ctx.assets.load_sprite("sprites/player_walk.png") {
            Ok(sheet) => {
                // Access the raw sprite to create animation
                sheet.with_sprite(|sprite| {
                    // Assuming 4 frames, each 32x32, in a horizontal strip
                    self.walk_animation = Some(AnimatedSprite::new(
                        sprite.clone(),
                        32,      // frame width
                        32,      // frame height
                        4,       // frame count
                        0.15,    // seconds per frame
                    ));
                });
            }
            Err(e) => {
                eprintln!("Failed to load animation: {}", e);
            }
        }
    }
    
    fn update(&mut self, ctx: &mut GameContext, dt: f32) {
        // Update animation
        if let Some(anim) = &mut self.walk_animation {
            anim.update(dt);
        }
    }
}
```

## Preloading with Progress

Show loading progress for many assets:

```rust
use ryo2d::prelude::*;
use std::collections::HashMap;

struct LoadingGame {
    sprites: HashMap<String, SpriteHandle>,
    assets_to_load: Vec<(&'static str, &'static str)>,
    loading_index: usize,
    is_loading: bool,
}

impl LoadingGame {
    fn new() -> Self {
        LoadingGame {
            sprites: HashMap::new(),
            assets_to_load: vec![
                ("player", "sprites/player.png"),
                ("enemy1", "sprites/enemy1.png"),
                ("enemy2", "sprites/enemy2.png"),
                ("enemy3", "sprites/enemy3.png"),
                ("boss", "sprites/boss.png"),
                ("bullet", "sprites/bullet.png"),
                ("powerup", "sprites/powerup.png"),
                ("tileset", "sprites/tileset.png"),
                ("background", "sprites/background.png"),
                ("ui_health", "sprites/ui_health.png"),
            ],
            loading_index: 0,
            is_loading: true,
        }
    }
    
    fn loading_progress(&self) -> f32 {
        if self.assets_to_load.is_empty() {
            return 1.0;
        }
        self.loading_index as f32 / self.assets_to_load.len() as f32
    }
}

impl Game for LoadingGame {
    fn init(&mut self, ctx: &mut GameContext) {
        ctx.assets.set_base_path("assets");
    }
    
    fn update(&mut self, ctx: &mut GameContext, _dt: f32) {
        if self.is_loading {
            // Load one asset per frame to prevent hitching
            if self.loading_index < self.assets_to_load.len() {
                let (name, path) = self.assets_to_load[self.loading_index];
                
                match ctx.assets.load_sprite(path) {
                    Ok(sprite) => {
                        self.sprites.insert(name.to_string(), sprite);
                    }
                    Err(e) => {
                        eprintln!("Failed to load '{}': {}", name, e);
                    }
                }
                
                self.loading_index += 1;
                
                // Print progress
                let progress = self.loading_progress() * 100.0;
                println!("Loading... {:.0}%", progress);
            } else {
                println!("Loading complete!");
                self.is_loading = false;
            }
        } else {
            // Normal game logic
        }
    }
}
```

## Async Loading for Large Assets

For large textures or runtime loading without frame hitches:

```rust
use ryo2d::prelude::*;
use std::collections::HashMap;

struct AsyncGame {
    // Completed sprites
    sprites: HashMap<String, SpriteHandle>,
    // Assets currently loading
    pending: HashMap<String, AsyncSpriteHandle>,
    // Placeholder for loading assets
    placeholder: Option<SpriteHandle>,
}

impl AsyncGame {
    fn new() -> Self {
        AsyncGame {
            sprites: HashMap::new(),
            pending: HashMap::new(),
            placeholder: None,
        }
    }
    
    /// Request an async load if not already loaded or pending
    fn request_sprite(&mut self, assets: &mut AssetManager, name: &str, path: &str) {
        // Already have it?
        if self.sprites.contains_key(name) {
            return;
        }
        // Already loading?
        if self.pending.contains_key(name) {
            return;
        }
        // Request async load
        let handle = assets.load_sprite_async(path);
        self.pending.insert(name.to_string(), handle);
    }
    
    /// Get a sprite, or placeholder if still loading
    fn get_sprite(&self, name: &str) -> Option<SpriteHandle> {
        self.sprites.get(name).cloned()
            .or_else(|| self.placeholder.clone())
    }
}

impl Game for AsyncGame {
    fn init(&mut self, ctx: &mut GameContext) {
        // Create placeholder first
        let placeholder = Sprite::solid_rect(64, 64, 128, 128, 128, 255);
        self.placeholder = Some(ctx.assets.create_sprite(placeholder, "placeholder"));
        
        // Request async loads for large assets
        self.request_sprite(ctx.assets, "world_map", "textures/world_4k.png");
        self.request_sprite(ctx.assets, "boss", "sprites/boss_large.png");
        self.request_sprite(ctx.assets, "background", "backgrounds/parallax.png");
    }
    
    fn update(&mut self, ctx: &mut GameContext, _dt: f32) {
        // IMPORTANT: Process async loads each frame
        let completed = ctx.assets.process_async_loads();
        if completed > 0 {
            println!("Loaded {} assets", completed);
        }
        
        // Check pending loads and move completed ones to sprites
        let completed_names: Vec<String> = self.pending
            .iter()
            .filter(|(_, handle)| handle.is_complete())
            .map(|(name, _)| name.clone())
            .collect();
        
        for name in completed_names {
            if let Some(pending) = self.pending.remove(&name) {
                match pending.state() {
                    LoadingState::Loaded => {
                        if let Some(sprite) = pending.get() {
                            self.sprites.insert(name, sprite);
                        }
                    }
                    LoadingState::Failed(reason) => {
                        eprintln!("Failed to load '{}': {}", name, reason);
                    }
                    _ => {}
                }
            }
        }
        
        // Show loading status
        if ctx.assets.has_pending_loads() {
            let count = ctx.assets.pending_load_count();
            println!("Still loading {} assets...", count);
        }
    }
}
```

## Streaming World with Async

For open-world games that load chunks dynamically:

```rust
use ryo2d::prelude::*;
use std::collections::HashMap;

struct StreamingWorld {
    loaded_chunks: HashMap<(i32, i32), SpriteHandle>,
    loading_chunks: HashMap<(i32, i32), AsyncSpriteHandle>,
    player_chunk: (i32, i32),
    load_radius: i32,
}

impl StreamingWorld {
    fn chunk_path(x: i32, y: i32) -> String {
        format!("world/chunk_{}_{}.png", x, y)
    }
}

impl Game for StreamingWorld {
    fn init(&mut self, ctx: &mut GameContext) {
        self.player_chunk = (0, 0);
        self.load_radius = 2;
    }
    
    fn update(&mut self, ctx: &mut GameContext, _dt: f32) {
        // Process any pending chunk loads
        ctx.assets.process_async_loads();
        
        // Check for completed loads
        let completed: Vec<(i32, i32)> = self.loading_chunks
            .iter()
            .filter(|(_, h)| h.is_loaded())
            .map(|(pos, _)| *pos)
            .collect();
        
        for pos in completed {
            if let Some(pending) = self.loading_chunks.remove(&pos) {
                if let Some(sprite) = pending.get() {
                    self.loaded_chunks.insert(pos, sprite);
                }
            }
        }
        
        // Request chunks near player
        let (px, py) = self.player_chunk;
        for dx in -self.load_radius..=self.load_radius {
            for dy in -self.load_radius..=self.load_radius {
                let chunk_pos = (px + dx, py + dy);
                
                // Skip if already loaded or loading
                if self.loaded_chunks.contains_key(&chunk_pos) {
                    continue;
                }
                if self.loading_chunks.contains_key(&chunk_pos) {
                    continue;
                }
                
                // Request async load
                let path = Self::chunk_path(chunk_pos.0, chunk_pos.1);
                let handle = ctx.assets.load_sprite_async(&path);
                self.loading_chunks.insert(chunk_pos, handle);
            }
        }
        
        // Unload distant chunks
        let unload: Vec<(i32, i32)> = self.loaded_chunks
            .keys()
            .filter(|(x, y)| {
                (x - px).abs() > self.load_radius + 1 ||
                (y - py).abs() > self.load_radius + 1
            })
            .cloned()
            .collect();
        
        for pos in unload {
            self.loaded_chunks.remove(&pos);
        }
        
        // Periodically garbage collect
        ctx.assets.gc_sprites();
    }
}
```

## Key Takeaways

1. **Prefer sync loading during init** - Simpler and blocking is acceptable
2. **Use async for large assets** - Prevents frame hitches during gameplay
3. **Always call `process_async_loads()`** - Required for async handles to complete
4. **Store handles in your game struct** - Don't call `load_sprite` repeatedly
5. **Clone handles freely** - They're cheap reference-counted pointers
6. **Use GC at transitions** - Scene changes, level loads, etc.
7. **Handle errors gracefully** - Use placeholders for missing assets
8. **Enable hot reload for dev** - Wrap in `#[cfg(debug_assertions)]`
9. **One sprite, many entities** - Clone handles to share asset data
