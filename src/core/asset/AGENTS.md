# AGENTS.md - Asset Module

## Overview

The `asset/` module provides resource loading and caching for game assets. It handles sprite loading, hot reloading during development, and efficient asset sharing via handles.

## File Structure

| File | Purpose |
|------|---------|
| `mod.rs` | Module exports and re-exports |
| `manager.rs` | `AssetManager`, `SpriteHandle`, `AsyncSpriteHandle` |
| `hot_reload.rs` | `HotReloadWatcher` for dev-time file watching |

## Key Types

### SpriteHandle

A lightweight, clonable reference to loaded sprite data:

```rust
// Load a sprite
let player = assets.load_sprite("sprites/player.png")?;

// Handles are cheap to clone (Arc-backed)
let player2 = player.clone();

// Access sprite data
println!("Size: {}x{}", player.width(), player.height());
```

**Internals:** Uses `Arc<RwLock<Sprite>>` for shared ownership and hot-reload support.

### AssetManager

Central hub for loading and caching assets:

```rust
// In game init
let sprite = ctx.assets.load_sprite("player.png")?;

// Async batch loading
let handles = ctx.assets.load_sprites_async(&["a.png", "b.png", "c.png"]);
for handle in handles {
    if handle.is_ready() {
        let sprite = handle.get()?;
    }
}

// Hot reload (call each frame in dev mode)
ctx.assets.process_reloads();
```

**Features:**
- Automatic deduplication (same file = same handle)
- Path resolution relative to workspace assets directory
- Hot reload support for development iteration
- Async loading for batch operations

### HotReloadWatcher

Background filesystem watcher for dev-time iteration:

```rust
// Enable hot reload (typically done by engine)
assets.enable_hot_reload(vec![assets_dir])?;

// Process pending changes (call each frame)
assets.process_reloads();
```

**How it works:**
1. `notify` crate watches asset directories
2. File changes are sent via channel
3. `process_reloads()` updates sprite data in-place
4. All handles automatically see new data

## Import Patterns

From within this module:
```rust
use super::hot_reload::{HotReloadEvent, HotReloadWatcher};
```

From other core modules:
```rust
use crate::core::asset::{AssetManager, SpriteHandle};
```

From game code via prelude:
```rust
use ryo2d::prelude::*;
// AssetManager, SpriteHandle available
```

## Notes for AI Agents

1. **SpriteHandle is the primary asset type** - Other asset types (audio, etc.) have their own systems
2. **Hot reload is automatic in debug builds** - Engine enables it when workspace is detected
3. **Async loading is non-blocking** - Game continues while sprites load in background
4. **Sprites are shared** - Loading the same path twice returns the same handle
