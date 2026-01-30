# Window Mode Implementation Summary

## What Was Added

### 1. WindowMode Enum
Four resize behaviors for different game types:

- **Fixed** - No resize/maximize
- **ResizableAspectRatio** (default) - Scale with letterboxing
- **ResizableIntegerScale** - Pixel-perfect integer scaling
- **ResizableStretch** - Fill window (may distort)

### 2. Engine Integration

**EngineConfig:**
- Added `window_mode: WindowMode` field
- Defaults to `ResizableAspectRatio`

**EngineBuilder:**
- New `.window_mode(WindowMode)` method
- Window `resizable` flag controlled by mode
- Handles `WindowEvent::Resized` appropriately

**Resize Behavior:**
- **Fixed:** Window not resizable at all
- **AspectRatio/Integer:** Pixels handles scaling automatically via `resize_surface()`
- **Stretch:** Updates internal buffer size and camera viewport

### 3. API Exports

Added to prelude:
```rust
pub use crate::core::engine::WindowMode;
```

### 4. Documentation

Created comprehensive guide at `docs/src/concepts/window-modes.md`:
- Explains each mode with examples
- Performance notes
- Best practices
- Common patterns

### 5. Tests

Added 3 new tests:
- `window_mode_defaults_to_resizable_aspect_ratio()`
- `engine_config_includes_window_mode()`
- `engine_builder_sets_window_mode()`

## How It Works

The `pixels` crate already handles scaling your fixed-size pixel buffer to fit the window. Our WindowMode enum just configures:

1. Whether the window is resizable
2. How to handle resize events
3. For stretch mode, whether to resize the buffer itself

## Usage Example

```rust
// Default: resizable with aspect ratio
EngineBuilder::new()
    .resolution(800, 600)
    .with_scene(MenuScene::new())
    .build()
    .run()?;

// Fixed window
EngineBuilder::new()
    .resolution(640, 480)
    .window_mode(WindowMode::Fixed)
    .with_scene(GameScene::new())
    .build()
    .run()?;

// Integer scaling for pixel-perfect
EngineBuilder::new()
    .resolution(320, 240)
    .window_mode(WindowMode::ResizableIntegerScale)
    .with_scene(GameScene::new())
    .build()
    .run()?;
```

## Benefits

✅ Game developers have control over resize behavior  
✅ Default is sensible (aspect ratio preservation)  
✅ Supports multiple game styles (modern, retro, casual)  
✅ No performance cost - GPU handles scaling  
✅ Game code unaffected - always uses consistent coordinates  
✅ Well documented with examples

## Next Steps

Future enhancements could include:
- Fullscreen toggle support
- Runtime window mode switching
- Config file integration (read from Ryo.toml)
- Custom scaling algorithms (e.g., xBR, HQx for pixel art)
