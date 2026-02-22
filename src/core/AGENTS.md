# AGENTS.md - Core Engine Modules

## Overview

This folder contains all core engine functionality. Everything here is "part of the engine" - as opposed to `src/main.rs` which is just a demo.

## Module Structure

| Module | Purpose |
|--------|---------|
| `mod.rs` | Public exports for core modules |
| `engine.rs` | `Engine` struct, game loop, window management |
| `error.rs` | Unified error types (`EngineError`, `AssetError`, `SaveError`, etc.) |
| `game.rs` | `GpuRenderContext`, `TimeControl`, `TimeInfo`, `WindowRequest` |
| `scene.rs` | Scene management, transitions, scene stack, automatic cleanup on Switch/Reset |
| `workspace.rs` | Project discovery, `ryo.toml` configuration, path management |
| `debug.rs` | Debug visualization (colliders, velocities, origins, FPS) and drawing primitives |
| `profiler.rs` | Puffin profiling macros (`profile_function!`, `profile_scope!`) |
| `profiler_window.rs` | Standalone profiler window with real-time flamegraphs |
| `ecs/` | Entity-Component-System using hecs, object pooling (see `ecs/AGENTS.md`) |
| `events/` | Event bus for decoupled inter-system communication (see `events/AGENTS.md`) |
| `animation.rs` | Sprite animation with `Animation`, `Animator`, `AnimationController` |
| `asset.rs` | `AssetManager` and `SpriteHandle` for resource loading/caching |
| `audio.rs` | `AudioManager`, sound effects, and music playback (Kira) |
| `camera.rs` | 2D camera with follow, zoom, coordinate transforms |
| `collision.rs` | AABB/Circle shapes, `Collider` component, collision detection |
| `console/` | Developer console with two-phase command system (see skill: `ryo-console`) |
| `hot_reload.rs` | Background filesystem watcher for dev-time asset hot reloading |
| `input.rs` | Keyboard, mouse, scroll, and gamepad input state tracking (gilrs) |
| `cursor.rs` | Mouse cursor customization (style, visibility, confinement) |
| `network/` | Client-server networking, replication, and RPCs (see `network/AGENTS.md`) |
| `physics.rs` | `RigidBody`, `Acceleration`, gravity, drag (config) |
| `rapier_physics.rs` | Rapier2D integration, physics world, collision events |
| `renderer.rs` | wgpu-based software renderer with letterbox/scaling support |
| `save.rs` | 3-tier save system with compression (system/world/game data) |
| `spatial.rs` | `SpatialHash` for broad-phase, `PhysicsWorld` integration |
| `sprite.rs` | `Sprite` and `AnimatedSprite` for 2D rendering |
| `sprite_gpu.rs` | GPU sprite batching system with per-sprite shaders and effects |
| `parallax.rs` | Parallax scrolling background system with layered sprites |
| `tile.rs` | Tiled rendering (`TiledSprite`, `CompositeTile`) and `GroupedCollider` |
| `tilemap/` | Tilemap system: grid storage, chunks, collision, Tiled import, auto-tiling (see `tilemap/AGENTS.md`) |
| `shaders/` | WGSL shader files for GPU rendering (sprite effects, post-processing) |
| `lighting/` | 2D lighting system with point lights, spotlights, and shadows (see `lighting/AGENTS.md`) |
| `ui/` | Immediate-mode UI system for HUDs and overlays (see `ui/AGENTS.md`) |

## Import Patterns

Within `core/`, modules reference each other using `super::`:

```rust
// Inside core/engine.rs
use super::asset::AssetManager;
use super::camera::Camera;
use super::input::InputState;
```

From outside `core/` (like from `lib.rs`), use `crate::core::`:

```rust
// Inside lib.rs
pub use crate::core::engine::EngineBuilder;
pub use crate::core::game::{GpuRenderContext, TimeControl, TimeInfo, WindowRequest};
pub use crate::core::asset::{AssetManager, SpriteHandle};
```

## Design Philosophy

**Engine Owns the Loop** - User code never calls render or update directly. The `Scene` trait provides callbacks that the engine invokes at the right time.

**ECS for Entity Management** - Game objects are entities with attached components. The engine provides built-in components (`Transform`, `Velocity`, `SpriteRenderer`, `RigidBody`, `Collider`) and users can add their own.

**Assets via Handles** - Assets are loaded through `AssetManager` and accessed via lightweight handles (`SpriteHandle`). Handles use `Arc` for shared ownership, enabling multiple entities to reference the same asset data.

**Minimal Public Surface** - Only types users need are exposed through `lib.rs::prelude`. Internal implementation details stay private.

## System Dependencies

```
┌─────────────────────────────────────────────────────────────┐
│                       SceneContext                           │
│  ┌───────────┐ ┌─────────┐ ┌────────┐ ┌────────────┐       │
│  │ Resources │ │  World  │ │ Camera │ │ InputState │       │
│  │           │ │  (ECS)  │ │        │ │            │       │
│  └───────────┘ └─────────┘ └────────┘ └────────────┘       │
│  ┌──────────────────┐ ┌────────────────────────────────┐    │
│  │  AssetManager    │ │  Events, Effects, Lighting ... │    │
│  └──────────────────┘ └────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────────┐
│                     Engine Systems                          │
│  Animation → Physics → Collision → Movement → Rendering     │
└─────────────────────────────────────────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────────┐
│                      WgpuRenderer                           │
│  ┌──────────────┐ ┌────────────┐ ┌────────────────────────┐ │
│  │ Pixel Buffer │ │ GPU Texture│ │ Fullscreen Quad Shader │ │
│  │ (CPU RGBA)   │ │ (wgpu)     │ │ (WGSL)                 │ │
│  └──────────────┘ └────────────┘ └────────────────────────┘ │
└─────────────────────────────────────────────────────────────┘
```

## Animation System

The animation system (`animation.rs`) provides three APIs for different use cases:

### 1. Animation - Frame Sequence Definition
Defines a sequence of sprite frames with timing:
- Uniform or per-frame durations
- Loop modes: `Once`, `Loop`, `PingPong`, `Times(n)`
- Used as building blocks for animators

### 2. Animator - Simple ECS Component
For basic looping animations (coins spinning, flames flickering):
- Attach to entities with `SpriteRenderer`
- Engine automatically updates frames each tick
- Simple playback control (play/pause/stop/speed)

### 3. AnimationController - State Machine
For characters with multiple animation states:
- Manages multiple named animations
- State transitions (idle → walk → attack)
- Transition-after-finish for one-shot animations
- Current state tracking

**Integration:** The engine updates `Animator` and `AnimationController` components automatically each frame, and syncs their current sprite to the entity's `SpriteRenderer` component. Game code just sets states or swaps animations.

**Example Use Case (Flappy Bird):**
- `AnimationController` with "flap" (looping) and "hurt" (looping) states
- Game switches to "hurt" state on death
- Engine handles frame timing and sprite updates

## Tiled Rendering System

The tiled rendering system (`tile.rs`) provides components for repeating sprites and composite multi-part tiles.

### TiledSprite - Simple Repeating Tiles
Repeats a single sprite in one direction to fill a target size:
- Directions: `Up`, `Down`, `Left`, `Right`
- Automatically calculates tile count from `target_size / sprite_size`
- Useful for simple patterns like grass, walls, or ladders

### CompositeTile - Multi-Part Tiles
Combines multiple parts (static and tiled) into a single entity:
- Each `TilePart` can be static (drawn once) or tiled (repeated)
- Parts support `flip_x` and `flip_y` for sprite orientation
- Used for complex structures like pipes (cap + tiled body)

### GroupedCollider - Single Collision Box
Provides a single collision box covering an entire composite tile:
- More efficient than per-part colliders
- Auto-calculated from composite bounds via `GroupedCollider::from_composite()`
- Integrates with Rapier physics via `register_grouped_entity()`
- Supported by debug visualization (`show_colliders`)

### CompositeTileBuilder - Fluent API
Ergonomic builder for creating composite tiles with colliders:

```rust
let (composite, collider) = CompositeTileBuilder::new()
    .cap_flipped_y(cap_sprite, 0.0, 0.0)        // Flipped cap at top
    .body_tiled(body_sprite, 0.0, 24.0, TileDirection::Down, 128.0)
    .z_order(5)
    .build_with_collider();

world.spawn((
    Transform::new(x, y),
    RigidBody::kinematic(),
    composite,
    collider,
));
```

**Integration:** The engine renders `TiledSprite` and `CompositeTile` components automatically after `SpriteRenderer`, respecting z-order. Physics registration for `GroupedCollider` is automatic like standard `Collider`.

**Example Use Case (Flappy Bird Pipes):**
- Top pipe: body tiles downward, cap at bottom flipped vertically
- Bottom pipe: cap at top (normal), body tiles downward
- Single `GroupedCollider` covers entire pipe for collision

## Parallax Background System

The parallax background system (`parallax.rs`) provides layered scrolling backgrounds that create depth and movement.

### Architecture
A system-based approach (not ECS components) because:
- Parallax layers are a cohesive visual system, not independent entities
- More efficient render-time tiling vs spawning duplicate entities
- Cleaner API for game developers

### Key Types

**TileMode** - Controls horizontal tiling:
- `None` - Single sprite, no tiling (static sky)
- `Horizontal` - Tile to fill screen width
- `InfiniteScroll` - Seamless endless scrolling

**ParallaxLayer** - Individual layer definition:
- `sprite` - SpriteHandle for this layer
- `scroll_speed` - 0.0 (static) to 1.0 (camera speed)
- `z_order` - Lower values drawn first (behind)
- `y_offset` - Vertical position offset
- `tile_mode` - Horizontal tiling behavior

**ParallaxBackground** - Manages multiple layers with rendering

### Usage Pattern

Games use the builder API and render via `render_gpu()`:

```rust
// In scene init:
let (screen_w, screen_h) = ctx.screen_size();
self.background = ParallaxBackground::builder(screen_w as u32, screen_h as u32)
    .static_layer(sky_sprite, -100)              // Doesn't move
    .layer(mountains_sprite, 0.2, -80)           // 20% camera speed
    .layer(dunes_mid_sprite, 0.5, -60)           // 50% camera speed
    .layer(dunes_front_sprite, 0.8, -40)         // 80% camera speed
    .build();

// In render_gpu callback:
fn render_gpu(&mut self, gpu: &mut GpuRenderContext) {
    self.background.render(gpu);
}
```

### Parallax Offset Calculation
```rust
// scroll_speed: 0.0 = static, 1.0 = moves with camera
let parallax_offset_x = camera.x * layer.scroll_speed;
```

### Horizontal Tiling (InfiniteScroll mode)
```rust
let tiles_needed = (screen_width / sprite_width).ceil() as i32 + 2;
let base_offset = parallax_offset_x % sprite_width;
// Draw tiles from -sprite_width to cover seamless scrolling
```

**Integration:** Parallax layers render via the GPU path in `render_gpu()`, which draws behind all game entities.

## GPU Sprite Rendering System

The GPU sprite rendering system (`sprite_gpu.rs`) provides high-performance batched rendering for scenarios where CPU software rendering is too slow.

### When to Use GPU Rendering
- **Particle systems** with 1000+ sprites
- **Bullet hell games** with hundreds of projectiles
- **Effects-heavy scenes** requiring per-sprite shaders
- **Any scenario** where CPU blitting becomes a bottleneck

### Architecture
The GPU path bypasses the CPU pixel buffer entirely:
- Sprites rendered as textured quads directly on GPU
- Automatic batching by texture and effect type
- Instance buffer for per-sprite transforms
- Support for built-in effects (outline, glow, flash, etc.)

### Usage Pattern
Games implement `render_gpu()` callback instead of relying on automatic `SpriteRenderer` rendering:

```rust
fn render_gpu(&mut self, gpu: &mut GpuRenderContext) {
    // Upload sprite textures (cached by ID)
    for sprite in &self.sprites {
        gpu.upload_sprite(sprite);
    }
    
    // Add instances to batch collector
    for entity in gpu.world.entities_with::<Particle>() {
        let transform = gpu.world.get_component::<Transform>(entity)?;
        let instance = SpriteInstance::new()
            .with_transform(x, y, rotation, scale_x, scale_y);
        gpu.batch.add(sprite.id(), has_effects, instance, z_order);
    }
}
```

### BatchCollector - Automatic Grouping
Sprites are automatically grouped by:
- **Texture ID** - Sprites with same texture batched together
- **Effect status** - Effects/no-effects use different pipelines

This minimizes GPU state changes and draw calls.

### Synchronization Fix
**Critical:** Instance data is pre-uploaded to GPU buffer **before** starting the render pass. This prevents race conditions where the GPU reads from a buffer that's still being written to.

**Implementation:** Each batch's data is written to a different offset in the instance buffer, then the render pass uses `buffer.slice(offset..end)` to point at the correct region.

### Built-in Shader Effects
Sprites can use built-in effects via `SpriteEffect`:
- **Outline** - Colored border around sprite
- **Glow** - Soft radial glow
- **Flash** - Color flash overlay
- **Dissolve** - Fade-out effect
- **Distortion** - Wave/ripple effects

Effects are specified per-instance via `SpriteInstance::with_effect()`.

**Integration:** GPU rendering happens after CPU rendering but before post-processing. The engine automatically routes sprites through GPU path when `render_gpu()` populates the batch collector.

## Camera Shake System

The camera shake system (`camera.rs`) provides trauma-based screen shake for impact feedback.

### Architecture
- **Trauma-based**: Shake intensity is `trauma²` for better feel
- **Smooth noise**: Layered sine waves for organic movement (no external dependencies)
- **Auto-integrated**: Shake offset applied automatically in `world_to_screen()`
- **Real-time decay**: Uses unscaled dt so shake works during slow-mo

### Key Types

**CameraShake** (internal struct on Camera):
- `trauma` - Current trauma level (0.0 to 1.0)
- `max_offset` - Maximum pixel displacement (default: 10.0)
- `decay_rate` - Trauma decay per second (default: 1.0)

### Usage Pattern

```rust
// Trigger shake on impacts
ctx.camera.add_trauma(0.3);  // Light hit
ctx.camera.add_trauma(0.7);  // Explosion

// Configure shake feel
ctx.camera.configure_shake(20.0, 0.5);  // max_offset, decay_rate

// Query state
let trauma = ctx.camera.trauma();
let (offset_x, offset_y) = ctx.camera.shake_offset();
```

### How It Works
1. Game calls `add_trauma(amount)` - trauma accumulates (clamped to 1.0)
2. Engine calls `update_shake(real_dt)` each frame
3. Intensity = trauma² is calculated
4. Smooth noise generates offset within `±max_offset * intensity`
5. Trauma decays by `decay_rate * dt`
6. `world_to_screen()` adds shake offset automatically

## Time Scale & Hitstop System

The time control system (`engine.rs`) provides slow-motion and freeze-frame effects for game juice.

### Architecture
- **Time scale**: Multiplier applied to game dt (0.0=frozen, 1.0=normal, 2.0=fast)
- **Hitstop**: Temporary freeze that auto-expires after duration
- **Selective**: Affects game logic, physics, animations, tweens
- **Real-time exceptions**: Camera shake, UI, and input use unscaled time

### Key Types

**TimeControl** (shared between engine and contexts):
- `time_scale` - Speed multiplier (default: 1.0)
- `hitstop_remaining` - Seconds until hitstop ends (default: 0.0)

### Usage Pattern

```rust
// Slow-motion effects
ctx.set_time_scale(0.3);  // 30% speed for dramatic moments
ctx.set_time_scale(1.0);  // Return to normal
ctx.set_time_scale(2.0);  // Fast-forward

// Hitstop (freeze frame)
ctx.hitstop(0.05);  // 50ms freeze on hit
ctx.hitstop(0.1);   // 100ms freeze on critical

// Query state
let scale = ctx.time_scale();
let frozen = ctx.is_hitstop_active();
```

### How It Works
1. Game calls `set_time_scale()` or `hitstop()` via context
2. Engine syncs to internal TimeState before each update
3. `scaled_fixed_dt()` returns `fixed_dt * time_scale` (or 0 during hitstop)
4. All systems receive scaled dt: animations, tweens, physics, game logic
5. Hitstop decays with real dt after all updates complete
6. Camera shake and UI continue at real-time speed

### Combining Effects
Time scale and hitstop work together:
- Hitstop takes priority (dt=0 regardless of time_scale)
- After hitstop ends, time_scale resumes
- Useful pattern: hitstop on hit, then slow-mo for kill cam

## Object Pooling System

The object pooling system (`ecs/pool.rs` and `ecs/active.rs`) provides pre-allocated entity pools for high-frequency spawning scenarios like bullets, particles, and enemies.

### Why Object Pooling?

Allocating/deallocating entities rapidly (100+ bullets per second) can cause:
- Memory allocation overhead
- GC pressure in some scenarios
- Frame rate stutters during heavy spawning

Object pools pre-allocate entities at load time and recycle them during gameplay.

### Architecture

- `Active`/`Inactive` marker components control entity state
- Engine automatically skips `Inactive` entities in: rendering, physics, animations, tweens
- `Pool<B>` is generic over component bundle type `B`
- World generation tracking detects scene clears automatically

### Key Types

**PoolConfig** - Configure pool behavior:
- `initial_capacity` - Pre-spawn count (default: 32)
- `max_capacity` - Hard limit (0 = unlimited)
- `expand_batch_size` - Entities to add when exhausted (default: 16)
- `exhausted_policy` - What to do when empty

**PoolExhaustedPolicy** - Handle empty pool:
- `Expand` - Spawn more entities (default)
- `ReturnNone` - Return `None` from `acquire()`
- `RecycleOldest` - Force-release oldest active entity

**PoolStats** - Monitor performance:
- `total_spawned`, `current_active`, `current_available`
- `peak_active`, `acquires`, `releases`, `expansions`

### Usage Pattern

```rust
// In scene struct
struct GameScene {
    bullet_pool: Pool<(Transform, Renderable, Collider, Bullet)>,
}

// Initialize in on_enter
fn on_enter(&mut self, ctx: &mut SceneContext) {
    let sprite = ctx.assets.load_sprite("bullet.png").unwrap();
    self.bullet_pool = Pool::new(
        &mut ctx.world,
        PoolConfig::default().with_capacity(200),
        move || (
            Transform::default(),
            Renderable::sprite(sprite.clone()),
            Collider::circle(4.0).as_trigger(),
            Bullet::default(),
        )
    );
}

// Acquire bullet (no allocation!)
fn fire(&mut self, ctx: &mut SceneContext, x: f32, y: f32) {
    if let Some(entity) = self.bullet_pool.acquire(&mut ctx.world) {
        if let Some(mut t) = ctx.world.get_component_mut::<Transform>(entity) {
            t.x = x;
            t.y = y;
        }
    }
}

// Release bullet (no deallocation!)
fn despawn_bullet(&mut self, ctx: &mut SceneContext, entity: Entity) {
    self.bullet_pool.release(&mut ctx.world, entity);
}
```

### GPU Rendering Note

When using `render_gpu()`, you must manually filter `Inactive` entities:

```rust
fn render_gpu(&mut self, gpu: &mut GpuRenderContext) {
    for entity in self.bullet_pool.active_iter() {
        if gpu.world.has::<Inactive>(entity) {
            continue;  // Skip inactive pooled entities
        }
        // ... render entity
    }
}
```

**Integration:** The engine handles `Inactive` filtering for CPU rendering, physics registration, animations, and tweens automatically. Only GPU rendering in `render_gpu()` requires manual filtering.

## Profiler System

The profiler system (`profiler.rs` and `profiler_window.rs`) provides puffin-based hierarchical profiling with a built-in flamegraph viewer.

### Architecture

Two modules work together:

**`profiler.rs`** - Instrumentation layer:
- `profile_function!()` - Profiles entire function (uses function name)
- `profile_scope!("name")` - Profiles a named code block
- `init(enabled)` - Enables puffin scope collection
- `new_frame()` - Signals frame boundary to puffin
- Compiles to zero-cost no-ops when `profiler` feature is disabled

**`profiler_window.rs`** - Visualization layer:
- Standalone wgpu window with CPU-rendered flamegraph
- Registers as puffin sink to receive frame data
- Color-coded bars based on scope name hash
- Hover tooltips showing scope name and duration
- 10 FPS throttle for stable, readable display
- Setup instructions when no scopes detected

### Data Flow

```
Game Code                    Engine                    Profiler Window
    │                           │                            │
    │  profile_function!()      │                            │
    │  profile_scope!()    ────>│ puffin records scope       │
    │                           │        │                   │
    │                           │        ▼                   │
    │                           │ new_frame() ──────────────>│ FrameView sink
    │                           │                            │ receives frame
    │                           │                            │      │
    │                           │                            │      ▼
    │                           │                            │ collect_puffin_data()
    │                           │                            │ draw_flamegraph()
    │                           │                            │ present()
```

### Engine Integration

The engine manages profiler lifecycle in `engine.rs`:

```rust
// In resumed() - create profiler window
if profiler_enabled && renderer_ready {
    self.profiler_window = ProfilerWindow::new(...);
}

// In event handling - route events to profiler window
if event.window_id() == profiler_window.window_id() {
    profiler_window.handle_event(&event);
}

// At frame start
profiler::new_frame();
profiler_window.track_frame_time();

// After game render
profiler_window.render();
```

### Pre-Instrumented Scopes

The engine instruments these systems automatically:
- `update` - All game logic and fixed timestep work
- `render` - CPU sprite rendering and batch collection
- `present` - GPU submission and effects pipeline
- `physics_step` - Rapier2D physics simulation
- `scene_push` / `scene_switch` - Scene transitions

### Adding Profiler Scopes

For engine developers adding new systems:

```rust
use crate::profile_scope;

fn my_system(world: &mut World, dt: f32) {
    profile_scope!("my_system");

    for (entity, component) in world.query_mut::<&mut MyComponent>() {
        profile_scope!("process_entity");
        // ... work
    }
}
```

### Configuration

Via CLI:
```bash
ryo2d dev --profiler    # Enables profiler feature and opens window
```

Via Ryo.toml:
```toml
[debug]
enable_profiler = true
```

### Performance Considerations

- Profiler overhead is ~200-400ns per scope when enabled
- Profiler window renders at 10 FPS to avoid impacting game
- When `profiler` feature is disabled, all macros compile to nothing
- Separate focus tracking prevents game throttling when profiler is focused

## Cursor Customization System

The cursor system (`cursor.rs`) provides control over the mouse cursor appearance and behavior.

### Architecture

Cursor changes use a **deferred request pattern** to avoid borrow conflicts:
- Game code calls `ctx.set_cursor()`, `ctx.set_cursor_visible()`, etc.
- Requests are stored in `Engine::cursor_request`
- Engine applies changes in `about_to_wait()` after all updates complete

### CursorStyle

18 system cursor icons available:

| Style | Description |
|-------|-------------|
| `Default` | Standard arrow cursor |
| `Text` | I-beam for text input |
| `Crosshair` | Precise selection |
| `Pointer` | Hand for clickable elements |
| `Move` | Four-way arrows for dragging |
| `NotAllowed` | Forbidden/disabled indicator |
| `Grab` | Open hand (ready to grab) |
| `Grabbing` | Closed hand (dragging) |
| `Wait` | Busy spinner |
| `Progress` | Arrow with spinner |
| `Help` | Arrow with question mark |
| `Cell` | Table cell selection |
| `ZoomIn` / `ZoomOut` | Magnifying glass |
| `ResizeHorizontal` | Left-right resize |
| `ResizeVertical` | Up-down resize |
| `ResizeNeSw` / `ResizeNwSe` | Diagonal resize |

### Context Methods

```rust
// Change cursor style
ctx.set_cursor(CursorStyle::Crosshair);

// Control visibility
ctx.set_cursor_visible(false);  // Hide cursor
ctx.show_cursor();              // Show cursor
ctx.hide_cursor();              // Hide cursor

// Confine cursor to window
ctx.set_cursor_confined(true);  // Lock to window bounds
```

### Usage Patterns

**Hover-based cursor changes:**
```rust
fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    let (mx, my) = ctx.input.mouse_position();

    if self.button_rect.contains(mx, my) {
        ctx.set_cursor(CursorStyle::Pointer);
    } else if self.text_field.contains(mx, my) {
        ctx.set_cursor(CursorStyle::Text);
    } else {
        ctx.set_cursor(CursorStyle::Default);
    }

    SceneAction::Continue
}
```

**Drag operations:**
```rust
if ctx.input.mouse_pressed(MouseButton::Left) && self.is_draggable(mx, my) {
    self.dragging = true;
    ctx.set_cursor(CursorStyle::Grabbing);
} else if ctx.input.mouse_released(MouseButton::Left) {
    self.dragging = false;
    ctx.set_cursor(CursorStyle::Default);
}
```

**First-person/fullscreen games:**
```rust
fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.hide_cursor();
    ctx.set_cursor_confined(true);
}

fn on_exit(&mut self, ctx: &mut SceneContext) {
    ctx.show_cursor();
    ctx.set_cursor_confined(false);
}
```

### Demo

See `examples/demos/cursor/` for a complete example showcasing all cursor styles and features.

## Developer Console System

The developer console (`console/`) provides a Valve-style overlay for runtime debugging and command execution.

### Architecture

```
console/
  mod.rs           - ConsoleState, UI rendering, input handling
  commands.rs      - CommandRegistry, two-phase command system
  builtins.rs      - Built-in debug/system commands
```

### Two-Phase Command System

Game commands use a **declare-then-register** pattern:

1. **Declaration** (startup): Define command name, description, verbose help
2. **Registration** (scoped): Attach handler within a specific scope (scene, game level)
3. **Auto-cleanup**: Handlers removed when scope exits, command reverts to noop

This enables commands to appear in help immediately while only being active in appropriate contexts.

### Key Types

**ConsoleState** - UI state:
- Window position/size, drag state, focus
- Input buffer, cursor, command history
- Output lines with colors and timestamps
- Scroll offset, word wrapping

**CommandRegistry** - Command management:
- `builtins` - Engine commands (help, clear, debug:fps, etc.)
- `declared` - Game commands with optional scoped handlers
- `execute()` - Parse and run commands
- `clear_scope()` - Remove handlers for a scope

**DeclaredCommand** - Game command:
```rust
pub struct DeclaredCommand {
    pub name: String,
    pub description: String,
    pub verbose_description: Option<String>,
    pub handler: Option<(u64, CommandFn)>,  // (scope_id, handler)
}
```

### Integration Points

- **Engine**: Manages ConsoleState and CommandRegistry, handles toggle key (~)
- **SceneContext**: `declare_console_command()`, `register_console_command()`
- **Scene transitions**: Auto-calls `clear_scope()` when scenes exit

### Usage

```rust
// Declare in menu scene (available in help immediately)
ctx.declare_console_command("game:reset", "Reset game state", None);

// Register in game scene (becomes active)
ctx.register_console_command("game:reset", |args, cmd_ctx| {
    cmd_ctx.console.print_system("Resetting...");
    CommandResult::Ok
});
// Handler auto-removed when game scene exits
```

### Configuration

```toml
# Ryo.toml
[system]
console = true  # Enable in both debug and release builds

[project]
name = "My Game"
version = "1.0.0"  # Shown in system:version command
```

**See skill `ryo-console` for complete documentation.**
