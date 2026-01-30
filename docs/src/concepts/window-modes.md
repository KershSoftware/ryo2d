# Window Modes

Ryo2D provides flexible window resize behavior to suit different game types and artistic styles.

## Overview

When users resize, maximize, or drag your game window, the engine needs to decide how to handle it. Different games have different requirements:

- **Pixel art games** want crisp pixels without blur
- **Casual games** want to fill the screen
- **Precise platformers** want consistent aspect ratios

Ryo2D's `WindowMode` enum gives you control over this behavior.

## Window Modes

### ResizableAspectRatio (Default)

The game viewport scales to fit the window while maintaining aspect ratio. Black bars (letterboxing/pillarboxing) fill unused space.

```rust
EngineBuilder::new()
    .title("My Game")
    .resolution(800, 600)
    .window_mode(WindowMode::ResizableAspectRatio) // default
    .with_scene(MenuScene::new())
    .build()
    .run()?;
```

**Best for:**
- Most 2D games
- Any game where aspect ratio matters
- Games with carefully designed compositions

**Example:** An 800×600 game in a 1920×1080 window will scale up to 1440×1080 with black bars on the sides.

**Constraining viewport size:**

You can limit how much the viewport scales with `max_width()` and `max_height()`:

```rust
EngineBuilder::new()
    .resolution(320, 240)
    .max_width(1280)   // Don't scale wider than 1280px
    .max_height(960)   // Don't scale taller than 960px
    .window_mode(WindowMode::ResizableAspectRatio)
    .with_scene(GameScene::new())
    .build()
    .run()?;
```

This prevents blur on large displays and maintains pixel density. Useful for pixel art games that look best at specific scales.

### Fixed

Window cannot be resized or maximized by the user.

```rust
EngineBuilder::new()
    .resolution(640, 480)
    .window_mode(WindowMode::Fixed)
    .with_scene(GameScene::new())
    .build()
    .run()?;
```

**Best for:**
- Game jam entries
- When you want complete control over presentation
- Retro games that ran at specific resolutions
- Demos and showcases

### ResizableIntegerScale

Game viewport scales in integer multiples (1×, 2×, 3×, 4×) only. Maintains perfect pixel crispness with letterboxing.

```rust
EngineBuilder::new()
    .resolution(320, 240)
    .window_mode(WindowMode::ResizableIntegerScale)
    .with_scene(GameScene::new())
    .build()
    .run()?;
```

**Best for:**
- Retro-styled pixel art games
- When pixel-perfect rendering is critical
- Games like Celeste, Enter the Gungeon

**Example:** A 320×240 game in a 1000×800 window will scale to 960×720 (3×) with black bars, not 1000×750 (3.125×).

**Constraining viewport size:**

Max width/height constraints work with integer scaling too:

```rust
EngineBuilder::new()
    .resolution(320, 240)
    .max_width(960)    // Allow up to 3× scaling
    .window_mode(WindowMode::ResizableIntegerScale)
    .with_scene(GameScene::new())
    .build()
    .run()?;
```

### ResizableStretch

Game viewport stretches to fill the entire window. **May distort aspect ratio.**

```rust
EngineBuilder::new()
    .resolution(800, 600)
    .window_mode(WindowMode::ResizableStretch)
    .with_scene(GameScene::new())
    .build()
    .run()?;
```

**Best for:**
- UI-heavy games where distortion isn't noticeable
- Some puzzle games
- Tools and editors built with Ryo2D

**Warning:** Can make circles look like ovals and squares look like rectangles.

## How It Works

Behind the scenes, Ryo2D uses the [pixels](https://github.com/parasyte/pixels) crate, which:

1. **Your game** renders to a fixed-size pixel buffer (e.g., 800×600)
2. **The engine** scales this buffer to fit the window
3. **GPU** handles the actual scaling using fast hardware

This separation means:
- Your game logic always works with consistent coordinates
- No need to handle different resolutions in your code
- Scaling is cheap and GPU-accelerated

## Choosing a Mode

| Your Game Type | Recommended Mode |
|----------------|------------------|
| Pixel art (retro style) | `ResizableIntegerScale` |
| Modern 2D with sprites | `ResizableAspectRatio` |
| Hand-drawn/vector art | `ResizableAspectRatio` |
| UI-heavy/tools | `ResizableStretch` |
| Very specific experience | `Fixed` |

## Common Patterns

### Let Users Choose

Read from config file:

```rust
let config = workspace.config();
let mode = match config.string("window.mode").as_deref() {
    Some("fixed") => WindowMode::Fixed,
    Some("integer") => WindowMode::ResizableIntegerScale,
    Some("stretch") => WindowMode::ResizableStretch,
    _ => WindowMode::ResizableAspectRatio, // default
};

EngineBuilder::new()
    .window_mode(mode)
    .with_scene(MenuScene::new())
    .build()
    .run()?;
```

### Start Fullscreen

Combine with fullscreen APIs (future feature):

```rust
EngineBuilder::new()
    .window_mode(WindowMode::ResizableAspectRatio)
    .fullscreen(true) // Not yet implemented
    .with_scene(MenuScene::new())
    .build()
    .run()?;
```

## Technical Details

### Performance

All modes have identical performance. The GPU scales the pixel buffer regardless of mode - only the math changes:

- **AspectRatio:** Calculate scale to fit
- **IntegerScale:** Round down to integer
- **Stretch:** Fill entire window
- **Fixed:** No scaling (1:1)

### Coordinate Systems

Window mode **does not affect** your game code. You always work in your chosen resolution:

```rust
// Your game is 800×600
let center_x = 400.0; // Always the center, regardless of window size
let center_y = 300.0;

world.spawn((
    Transform::new(center_x, center_y),
    SpriteRenderer::new(sprite),
));
```

### Input Mapping

Mouse/touch coordinates are automatically mapped to your game's resolution. A click at window position (1920, 1080) might map to game position (400, 300).

**Important:** Clicks in black bars (letterbox/pillarbox areas) are ignored and don't register as input. The engine only reports coordinates within the actual game viewport.

## See Also

- [Input Handling](./input.md) - How input scales with window modes
- [Camera System](./camera.md) - Advanced viewport control
- [Ryo.toml Reference](../getting-started/configuration.md) - Config file options
