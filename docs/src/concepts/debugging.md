# Debugging

Ryo2D provides built-in debug visualization tools to help you develop and troubleshoot your games. All debug features can be enabled via `Ryo.toml` configuration or toggled at runtime with keyboard shortcuts.

## Runtime Debug Keys

During development, press these keys to toggle debug visualizations without restarting:

| Key | Action |
|-----|--------|
| **F1** | Cycle overlay: Off → FPS → Frame Timing → Full Stats → Off |
| **F2** | Toggle collision boundaries (green outlines) |
| **F3** | Toggle velocity vectors (cyan arrows) |
| **F4** | Toggle origin points (yellow crosshairs) |
| **F5** | Toggle borderless window mode |
| **F9** | Toggle Logger window |
| **F10** | Toggle Profiler window (requires `profiler` feature) |

To disable these keys (e.g., for a shipped game), add `disable_debug_keys = true` to your `Ryo.toml`.

## Logger Window (F9)

The Logger window provides a real-time view of engine and game logs directly in your game. Press **F9** to toggle it.

### Features

- **Real-time log streaming** - See logs as they happen
- **Log level filtering** - Filter by Error, Warn, Info, Debug, Trace
- **Module filtering** - Filter logs by module name (e.g., "Physics", "Audio")
- **Text selection** - Click and drag to select text
- **Copy support** - Ctrl+C to copy selected text, right-click to copy entire line
- **ISO timestamps** - Each entry shows timestamp and elapsed time since start
- **Auto-scroll** - Automatically scrolls to new entries (toggleable)
- **Catch-up** - Opening the logger late still shows all previous log entries

### Log Levels

| Level | Color | Use For |
|-------|-------|---------|
| **Error** | Red | Critical failures that need immediate attention |
| **Warn** | Yellow | Potential issues that don't stop execution |
| **Info** | White | General information about game state |
| **Debug** | Cyan | Detailed debugging information |
| **Trace** | Gray | Very verbose tracing (disabled by default) |

### Filtering Logs

Click the level buttons at the top of the logger window to show/hide specific log levels. Active filters are highlighted.

Use the module filter to focus on specific systems:
- Type a module name to filter (e.g., "Physics")
- Clear the filter to see all modules

### Logging from Your Game

Use the logging macros from the prelude:

```rust,ignore
use ryo2d::prelude::*;

fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    // Standard logging
    engine_info!("Player position: {:?}", player_pos);
    engine_warn!("Health low: {}", health);
    engine_error!("Failed to load asset");

    // Module-tagged logging (shows in module column)
    engine_info!(module: "Combat", "Damage dealt: {}", damage);
    engine_debug!(module: "AI", "Enemy state: {:?}", state);
    SceneAction::Continue
}
```

### Tips

- Keep the logger open during development to catch warnings early
- Use module tags to organize logs from different game systems
- The logger has minimal performance impact when open
- Logs persist in memory, so opening the logger late won't miss earlier messages

## Quick Start

Add a `[debug]` section to your `Ryo.toml`:

```toml
[debug]
show_colliders = true
show_velocities = false
show_origins = false
show_frame_timing = false
disable_debug_keys = false  # Allow F1-F4 debug toggles

[info]
show_fps = true
```

Or enable debug features in code:

```rust,ignore
EngineBuilder::new()
    .title("My Game")
    .show_colliders(true)
    .show_fps(true)
    .show_frame_timing(false)
    .start_scene(GameScene::new())
    .build()
    .run()?;
```

**Note:** Debug settings from `Ryo.toml` are **automatically loaded** during `build()` if the file exists. Any values set programmatically will override the config file settings.

## Available Debug Options

### `show_colliders`

Renders bright green outlines around all entities with `Collider` components.

**When to use:**
- Verifying collision boundaries match your sprites
- Debugging "why isn't collision working" issues
- Checking collider offsets are correct

**What it shows:**
- Box colliders: Green rectangle outline
- Circle colliders: Green circle outline
- Accounts for camera position and zoom

### `show_fps`

Displays a frames-per-second counter in the bottom-left corner.

**When to use:**
- Quick performance check
- Verifying target FPS is being met

**What it shows:**
- Current smoothed FPS value with a semi-transparent background

### `show_frame_timing`

Displays a detailed frame timing breakdown overlay in the bottom-left corner. When enabled, this replaces the simple FPS counter with more detailed information.

**When to use:**
- Detailed performance profiling
- Identifying which phase of the game loop is slow
- Diagnosing frame drops and stuttering

**What it shows:**
- **Update**: Time spent in game logic and physics (ms)
- **Render**: Time spent preparing the frame for display (ms)
- **Present**: Time spent submitting to GPU and effects pipeline (ms)
- **Total**: Combined frame time with smoothed FPS

**Note:** Values are smoothed using an exponential moving average for stable, readable display.

### `show_stats`

Displays entity and rendering statistics above the frame timing overlay.

**When to use:**
- Monitoring entity count during gameplay
- Tracking draw call efficiency
- Debugging entity lifecycle issues

**What it shows:**
- **Entities**: Number of active entities in the ECS world
- **Sprites**: Number of sprites submitted for rendering
- **Batches**: Number of GPU draw calls (lower is better)

**Tip:** Press F1 to cycle to "Full Stats" mode which shows both stats and frame timing together.

### `show_velocities`

Renders cyan arrows on entities with `Velocity` components, showing direction and magnitude of movement.

**When to use:**
- Debugging physics-based movement
- Verifying impulses are applied correctly
- Understanding entity motion patterns

**What it shows:**
- Cyan lines from entity origin in the direction of velocity
- Line length proportional to velocity magnitude
- Only renders for entities that are actually moving

### `show_origins`

Renders small yellow crosshairs at each entity's `Transform` position.

**When to use:**
- Debugging sprite positioning and anchoring
- Verifying spawn locations
- Understanding coordinate systems

**What it shows:**
- Small yellow + at the exact (x, y) position of each entity

## Configuration Reference

### Ryo.toml

```toml
[debug]
# Draw collision boundaries (green outlines)
show_colliders = false

# Draw velocity vectors (cyan arrows)
show_velocities = false

# Draw entity origin points (yellow crosshairs)
show_origins = false

# Display detailed frame timing breakdown
show_frame_timing = false

# Display entity/sprite/batch statistics
show_stats = false

# Enable flamegraph profiler window (requires --features profiler or `ryo2d dev --profiler`)
enable_profiler = false

# Disable F1-F4 debug key toggles (for release builds)
disable_debug_keys = false

[info]
# Display FPS counter in bottom-left corner
show_fps = true
```

## Game-Specific Configuration: `[env]`

The `[env]` section provides game-specific configuration that the engine passes through without interpreting. Use this for custom flags and settings unique to your game.

### Example

```toml
[env]
debug_scene = true
starting_level = 3
difficulty = "hard"
```

### Accessing Env Values

```rust,ignore
use ryo2d::prelude::*;

fn main() {
    let workspace = Workspace::discover().expect("No workspace found");
    let config = workspace.config();
    
    // Boolean values
    if config.env_bool("debug_scene").unwrap_or(false) {
        // Start with debug scene
    }
    
    // Integer values
    let level = config.env_int("starting_level").unwrap_or(1);
    
    // String values
    let difficulty = config.env_str("difficulty").unwrap_or("normal");
    
    // Float values
    let speed = config.env_float("speed_multiplier").unwrap_or(1.0);
    
    // Check existence
    if config.has_env("custom_flag") {
        // Handle custom flag
    }
}
```

### Use Cases

- **Debug scene toggle**: Start with a test scene instead of the main menu
- **Level skip**: Jump to a specific level during development
- **Feature flags**: Enable/disable experimental features
- **Difficulty overrides**: Force a specific difficulty for testing
- **Asset paths**: Custom asset directories for testing

### EngineBuilder Methods

| Method | Description |
|--------|-------------|
| `.show_colliders(bool)` | Enable/disable collider visualization |
| `.show_fps(bool)` | Enable/disable FPS counter |
| `.show_frame_timing(bool)` | Enable/disable detailed frame timing overlay |
| `.show_velocities(bool)` | Enable/disable velocity vectors |
| `.show_origins(bool)` | Enable/disable origin crosshairs |

**Note:** Debug settings from `Ryo.toml` are automatically loaded. These methods override config file values.

### Console Warnings

In debug builds, Ryo2D prints helpful performance warnings to the console:

**Slow Frame Warnings** (>33ms frames):
```
[SLOW FRAME] Total: 45.2ms | Update: 38.1ms | Render: 5.3ms | Present: 1.8ms
```

**Scene Load Warnings** (>5ms scene transitions):
```
[Scene] on_enter took 127ms
```

These warnings only appear in debug builds and cannot be disabled.

## Debug Colors

The debug system uses consistent colors for different visualizations:

| Visualization | Color | RGB Value |
|--------------|-------|-----------|
| Colliders | Bright Green | `#00FF00` |
| Velocities | Cyan | `#00FFFF` |
| Origins | Yellow | `#FFFF00` |

## Performance Considerations

Debug visualization has minimal performance impact, but keep in mind:

- **Collider rendering** iterates all entities with `Transform + Collider`
- **Velocity rendering** iterates all entities with `Transform + Velocity`
- **Origin rendering** iterates all entities with `Transform`

For games with thousands of entities, you may want to disable debug rendering in performance-critical scenarios.

## Best Practices

1. **Enable in development, disable in release**
   
   Keep debug options in `Ryo.toml` during development. When releasing, either remove the `[debug]` section or set all values to `false`.

2. **Use `show_colliders` early**
   
   Enable collider visualization when first implementing physics. Many collision bugs are immediately obvious when you can see the actual collision boundaries.

3. **FPS monitoring**
   
   Enable `show_fps` during development to catch performance regressions early.

4. **Combine visualizations**
   
   When debugging complex physics issues, enable multiple options simultaneously:
   
   ```toml
   [debug]
   show_colliders = true
   show_velocities = true
   show_origins = true
   ```

## Custom Debug Rendering

For game-specific debug visualization, you can use the debug module's drawing primitives in your `render_ui` method:

```rust,ignore
use ryo2d::core::debug;

impl Scene for GameScene {
    fn render_ui(&mut self, ui: &mut UiContext) -> SceneAction {
        // Your custom debug rendering here
        // The engine's debug module provides low-level drawing functions
    }
}
```

## Example: Debugging Collision Issues

When a collision isn't working as expected:

1. Enable collider visualization:
   ```toml
   [debug]
   show_colliders = true
   ```

2. Run your game and observe:
   - Are the colliders where you expect them?
   - Are they the right size?
   - Do they overlap when collision should occur?

3. Check for common issues:
   - Collider offset not matching sprite anchor
   - Collider size too small/large
   - Wrong collision layers configured
   - Missing `RigidBody` component (physics not running)

## See Also

- [Physics System](./physics.md) - For collision detection and response
- [ECS Architecture](./ecs.md) - Understanding components and entities
- [Project Configuration](../getting-started/configuration.md) - Full Ryo.toml reference
