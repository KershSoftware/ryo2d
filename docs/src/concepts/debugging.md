# Debugging

Ryo2D provides built-in debug visualization tools to help you develop and troubleshoot your games. All debug features can be enabled via `Ryo.toml` configuration or programmatically through the `EngineBuilder`.

## Quick Start

Add a `[debug]` section to your `Ryo.toml`:

```toml
[debug]
show_colliders = true
show_fps = true
show_frame_timing = false
show_velocities = false
show_origins = false
```

Or enable debug features in code:

```rust,ignore
EngineBuilder::new()
    .title("My Game")
    .show_colliders(true)
    .show_fps(true)
    .show_frame_timing(false)
    .with_scene(GameScene::new())
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

**Note:** Values are smoothed using an exponential moving average for stable, readable display. The overlay also logs timing to console once per second for capture/analysis.

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
show_colliders = true

# Display FPS counter in bottom-left corner
show_fps = true

# Display detailed frame timing breakdown (replaces FPS when enabled)
show_frame_timing = false

# Draw velocity vectors (cyan arrows)
show_velocities = false

# Draw entity origin points (yellow crosshairs)
show_origins = false
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
    fn render_ui(&mut self, ui: &mut UiContext) {
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
