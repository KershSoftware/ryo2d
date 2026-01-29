# Error Handling

Ferrum2D provides a unified error handling system using idiomatic Rust patterns. All error types implement `std::error::Error` and provide helpful error messages for debugging.

## Error Type Hierarchy

The engine uses a layered error hierarchy:

```
EngineError (top-level)
├── Window(String)
├── Render(String)
├── Asset(AssetError)
├── Audio(AudioError)
├── Sprite(SpriteError)
└── Io(std::io::Error)

AssetError
├── NotFound(PathBuf)
├── LoadError { path, reason }
├── TypeMismatch { path, expected }
├── WatchError(String)
└── Sprite(SpriteError)

SpriteError
├── NotFound(PathBuf)
├── DecodeError { path, reason }
└── IoError { path, source }

AudioError
├── InitError(String)
├── LoadError(String)
├── PlayError(String)
└── NotFound(String)

HotReloadError
├── WatcherCreation(String)
└── WatchPath { path, reason }
```

## Result Type Aliases

Each subsystem has a convenient result type alias:

```rust
use ferrum2d::prelude::*;

// Engine-wide operations
fn run_game() -> EngineResult<()> { /* ... */ }

// Asset operations
fn load_assets(assets: &mut AssetManager) -> AssetResult<SpriteHandle> {
    assets.load_sprite("player.png")
}

// Audio operations
fn play_sound(audio: &mut AudioManager) -> AudioResult<SoundHandle> {
    audio.play("explosion")?
}

// Sprite operations
fn load_image() -> SpriteResult<Sprite> {
    Sprite::load("image.png")
}
```

## Handling Errors

### The `?` Operator

Errors automatically convert up the hierarchy, so you can use `?` freely:

```rust
use ferrum2d::prelude::*;

fn setup_game(ctx: &mut GameContext) -> EngineResult<()> {
    // AssetError converts to EngineError automatically
    let player = ctx.assets.load_sprite("player.png")?;
    
    // AudioError also converts to EngineError
    ctx.audio.load_sound("jump", "sounds/jump.wav")?;
    
    Ok(())
}
```

### Pattern Matching

For fine-grained error handling, match on specific variants:

```rust
use ferrum2d::prelude::*;

fn load_with_fallback(assets: &mut AssetManager) -> SpriteHandle {
    match assets.load_sprite("player.png") {
        Ok(sprite) => sprite,
        Err(AssetError::NotFound(_)) => {
            println!("Player sprite not found, using placeholder");
            assets.create_sprite(Sprite::solid_rect(32, 32, [255, 0, 255, 255]))
        }
        Err(e) => {
            println!("Failed to load player: {}", e);
            assets.create_sprite(Sprite::solid_rect(32, 32, [255, 0, 0, 255]))
        }
    }
}
```

### Displaying Errors

All error types implement `Display` for user-friendly messages:

```rust
match assets.load_sprite("missing.png") {
    Ok(sprite) => { /* use sprite */ }
    Err(e) => {
        // Prints: "Asset not found: missing.png"
        eprintln!("Error: {}", e);
    }
}
```

## Error Chaining

Errors preserve their source for debugging. Use the `source()` method to walk the chain:

```rust
use std::error::Error;

fn print_error_chain(err: &dyn Error) {
    eprintln!("Error: {}", err);
    let mut source = err.source();
    while let Some(cause) = source {
        eprintln!("  Caused by: {}", cause);
        source = cause.source();
    }
}
```

## Engine Run Errors

The `Engine::run()` method returns `EngineResult<()>`:

```rust
fn main() {
    let result = EngineBuilder::new()
        .title("My Game")
        .resolution(800, 600)
        .with_game(MyGame)
        .build()
        .run();

    if let Err(e) = result {
        eprintln!("Engine error: {}", e);
        std::process::exit(1);
    }
}
```

## Best Practices

### 1. Use Result Types in Game Code

Return `Result` from functions that can fail:

```rust
impl Game for MyGame {
    fn init(&mut self, ctx: &mut GameContext) {
        if let Err(e) = self.load_level(ctx) {
            eprintln!("Failed to load level: {}", e);
            // Handle gracefully or set error state
        }
    }
}

impl MyGame {
    fn load_level(&mut self, ctx: &mut GameContext) -> AssetResult<()> {
        self.tileset = ctx.assets.load_sprite("tiles.png")?;
        self.player = ctx.assets.load_sprite("player.png")?;
        Ok(())
    }
}
```

### 2. Provide Context with Error Messages

When propagating errors, add context:

```rust
fn load_level(path: &str, assets: &mut AssetManager) -> EngineResult<Level> {
    let tileset = assets.load_sprite(&format!("{}/tileset.png", path))
        .map_err(|e| EngineError::Asset(AssetError::LoadError {
            path: path.into(),
            reason: format!("Failed to load level tileset: {}", e),
        }))?;
    
    // ...
    Ok(level)
}
```

### 3. Log Errors in Development

During development, log errors for debugging:

```rust
fn play_sound_safe(audio: &mut AudioManager, name: &str) {
    if let Err(e) = audio.play(name) {
        #[cfg(debug_assertions)]
        eprintln!("[Audio] Failed to play '{}': {}", name, e);
    }
}
```

## Available Error Types in Prelude

All error types are exported from the prelude:

```rust
use ferrum2d::prelude::*;

// Error types
// - EngineError, AssetError, AudioError, SpriteError, HotReloadError

// Result aliases
// - EngineResult<T>, AssetResult<T>, AudioResult<T>, SpriteResult<T>, HotReloadResult<T>
```
