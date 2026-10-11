---
name: ryo-api
description: Complete API reference for ryo2d::prelude - all types, methods, and signatures.
metadata:
  version: 1.0.0
---

# Ryo2D API Reference

Complete reference for everything exported from `ryo2d::prelude::*`. Use this when you need exact method signatures or want to know what's available.

## Import

```rust
use ryo2d::prelude::*;
```

---

## Core Trait

### Scene

The main trait for all games. Every game uses scenes:

```rust
pub trait Scene {
    fn on_enter(&mut self, ctx: &mut SceneContext) {}
    fn on_exit(&mut self, ctx: &mut SceneContext) {}
    fn on_pause(&mut self, ctx: &mut SceneContext) {}
    fn on_resume(&mut self, ctx: &mut SceneContext, returned: Option<Box<dyn Any>>) {}
    fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction;  // Fixed timestep
    fn render(&mut self, ctx: &mut SceneContext, alpha: f32) {}            // Custom rendering (alpha = interpolation)
    fn render_ui(&mut self, ui: &mut UiContext) -> SceneAction { SceneAction::Continue }
    fn render_gpu(&mut self, ctx: &mut GpuRenderContext) {}                // GPU-batched sprites; sorted by z with entities (no longer behind them automatically)
    fn is_transparent(&self) -> bool { false }     // Render scenes below this one too
    fn updates_below(&self) -> bool { false }      // Keep updating scenes below this one
    fn name(&self) -> &str { /* type name */ }
    fn is_physics_paused(&self) -> bool { false }
    fn cleanup_on_exit(&self) -> SceneCleanup { SceneCleanup::DEFAULT }
}
```

**Note:** `on_resume` receives an optional `Box<dyn Any>` containing data returned by a popped scene via `SceneAction::PopReturn`.

---

## Context Types

### SceneContext

Available in `Scene` trait methods:

| Field | Type | Description |
|-------|------|-------------|
| `resources` | `&mut Resources` | Typed resource map for cross-scene shared state |
| `world` | `&mut World` | ECS world for spawning/querying entities |
| `camera` | `&mut Camera` | Camera position, zoom, viewport |
| `input` | `&mut InputState` | Keyboard and mouse input |
| `assets` | `&mut AssetManager` | Load/create sprites and assets |
| `audio` | `&mut Option<AudioManager>` | Play sounds and music |
| `effects` | `&mut EffectsManager` | Post-processing effects |
| `lighting` | `&mut Option<LightingRenderer>` | 2D lighting (None if disabled) |
| `events` | `&mut Events` | Event bus for decoupled communication |
| `time_control` | `&mut TimeControl` | Time scale and hitstop |
| `time` | `TimeInfo` | Timing values (`fixed_dt`, `scaled_dt`, `real_dt`, `render_alpha`, `time_scale`) |
| `raycast` | `&mut Option<GpuRaycastRenderer>` | Pseudo-3D raycasting renderer (None if not initialized) |
| `tilemap` | `&mut TilemapManager` | Load, spawn and query tilemaps |
| `window_request` | `&mut WindowRequest` | Pending resolution / display mode changes |
| `scale_factor` | `f64` | Window DPI scale (1.0 standard, 2.0 HiDPI) |
| `display_mode` | `DisplayMode` | Current display mode |

Helpers: `screen_size() -> (f32, f32)`, `screen_width()`, `screen_height()`, `resource::<T>() -> Option<&T>`, `resource_mut::<T>()`, `resource_or_default::<T>() -> &mut T`.

### UiContext

Available in `render_ui`. Positions are screen pixels; shapes take a `Rect`:

```rust
impl UiContext {
    // Text
    fn draw_text(&mut self, text: &str, x: f32, y: f32, color: Color);
    fn draw_text_anchored(&mut self, text: &str, anchor: Anchor, offset_x: f32, offset_y: f32, color: Color);
    fn draw_text_centered(&mut self, text: &str, rect: Rect, color: Color);
    fn label(&mut self, text: &str, x: f32, y: f32, text_color: Color, bg_color: Option<Color>);
    fn text_size(&self, text: &str) -> (u32, u32);

    // Shapes
    fn draw_rect(&mut self, rect: Rect, color: Color);
    fn draw_rect_outline(&mut self, rect: Rect, color: Color, thickness: f32);
    fn draw_panel(&mut self, rect: Rect, bg_color: Color, border_color: Option<Color>);

    // Sprites
    fn draw_sprite_handle(&mut self, handle: &SpriteHandle, x: f32, y: f32);
    fn draw_sprite_handle_scaled(&mut self, handle: &SpriteHandle, x: f32, y: f32, scale: f32);

    // Widgets
    fn button(&mut self, text: &str, rect: Rect) -> bool;   // true when clicked
    fn slider(&mut self, rect: Rect, value: &mut f32, min: f32, max: f32) -> bool;
    fn wants_keyboard(&self) -> bool;   // a focused text input or an open dropdown was drawn this frame
    fn is_clicked(&self, rect: Rect) -> bool;
    fn is_hovered(&self, rect: Rect) -> bool;
    fn is_held(&self, rect: Rect) -> bool;

    // Layout helpers
    fn screen_size(&self) -> (u32, u32);  // Returns (width, height) as u32
}

// Usage example:
let (sw, sh) = ui.screen_size();
let screen_w = sw as f32;  // Cast to f32 for positioning
let screen_h = sh as f32;
ui.draw_rect(Rect::new(10.0, 10.0, 200.0, 20.0), Color::BLACK);
ui.draw_text("Score: 0", 14.0, 14.0, Color::WHITE);
```

See the `ryo-ui` skill for the full widget set (progress bars, dropdowns, scroll views, text input, themes).

---

## Engine Builder

**Note:** Resolution, FPS, and other settings come from `Ryo.toml` by default.
Only use these methods to override the config file. A saved player preference in system.json overrides a builder call when `with_user_preferences` includes it.

```rust
// Minimal - uses Ryo.toml configuration
EngineBuilder::new()
    .title("Game Title")              // Window title
    .start_scene(MenuScene::new())    // Initial scene
    .build()
    .run()
    .unwrap();

// Override Ryo.toml (only when needed)
EngineBuilder::new()
    .title("Game Title")
    .resolution(1280, 720)            // Override window size
    .target_fps(60)                   // Override target FPS
    .physics_hz(120)                  // Physics update rate
    .window_mode(WindowMode::Fixed)   // Window behavior
    .start_scene(GameScene::new())
    .build()
    .run()
    .unwrap();

// Register shared resources and declare console commands
EngineBuilder::new()
    .title("Game Title")
    .resource(GameSettings::default())  // Shared via ctx.resources
    .with_console_commands(|cmd| {
        cmd.declare("game:reset", "Reset the game");
        cmd.declare_verbose("game:spawn", "Spawn entity", "Usage: game:spawn <type>");
    })
    .start_scene(MenuScene::new())
    .build()
    .run()
    .unwrap();
```

Other builder methods: `background_fps(u32)`, `background_rendering(bool)`, `vsync(bool)`, `display_mode(DisplayMode)`, `max_width(u32)`, `max_height(u32)`, `ui_scale(Option<u32>)`, `icon(path)`, `show_colliders(bool)`, `show_fps(bool)`, `show_velocities(bool)`, `show_origins(bool)`, `with_user_preferences(filter)`. `run()` returns `EngineResult<()>`.

### WindowMode

```rust
pub enum WindowMode {
    Fixed,                    // Fixed size, no resizing
    ResizableAspectRatio,     // Maintains aspect ratio (default)
    ResizableIntegerScale,    // Pixel-perfect scaling
    ResizableStretch,         // Stretches to fill
}
```

### DisplayMode

Runtime display mode for window appearance:

```rust
pub enum DisplayMode {
    Windowed,               // Regular window with decorations
    ExclusiveFullscreen,    // Exclusive fullscreen (changes display resolution)
    BorderlessFullscreen,   // Borderless window covering full screen
}

impl DisplayMode {
    fn display_name(&self) -> &'static str;  // "Windowed", "Exclusive Fullscreen", "Borderless Fullscreen"
    fn all() -> &'static [DisplayMode];      // All variants, in declaration order (Windowed, Exclusive, Borderless)
}

// Common display modes for settings menus
const DISPLAY_MODES: &[DisplayMode] = &[
    DisplayMode::Windowed,
    DisplayMode::BorderlessFullscreen,
    DisplayMode::ExclusiveFullscreen,
];
```

### Resolution

Common resolution presets:

```rust
pub struct Resolution {
    pub width: u32,
    pub height: u32,
}

impl Resolution {
    const fn new(width: u32, height: u32) -> Self;
    fn display_name(&self) -> String;  // "1280x720"

    // Common resolutions
    fn common() -> &'static [Resolution];  // 1280x720, 1366x768, 1600x900, 1920x1080, 2560x1440, 3840x2160
}
```

### WindowRequest

Request display changes at runtime (applied next frame):

```rust
// In SceneContext
impl SceneContext {
    fn request_display_mode(&mut self, mode: DisplayMode);
    fn request_resolution(&mut self, width: u32, height: u32);
    fn request_display_change(&mut self, width: u32, height: u32, mode: DisplayMode);
    fn request_ui_scale(&mut self, scale: Option<u32>);  // None = auto-detect from DPI
}
```

**Usage:**
```rust
// Settings menu - change display mode
fn apply_settings(&mut self, ctx: &mut SceneContext) {
    ctx.request_display_mode(DisplayMode::BorderlessFullscreen);
}
```

### Render pipeline

`SceneContext::render_pipeline` is the engine's `RenderPipeline`. Toggle built-in passes (`pass_ids::*`) or insert game passes; changes persist across scenes. See `ryo-renderer` for the pass order and the `RenderPass` trait.

```rust
let _ = ctx.render_pipeline.set_enabled(pass_ids::POST_PROCESS, false);
// Passes persist across scenes: add once (or `remove("my_pass")` in on_exit)
if !ctx.render_pipeline.contains("my_pass") {
    let _ = ctx.render_pipeline.insert_before(pass_ids::COMPOSITE, MyPass);
}
```

---

## Components

### Transform

```rust
pub struct Transform {
    pub x: f32,         // X position
    pub y: f32,         // Y position
    pub z: f32,         // Depth (for parallax/layering), defaults to 0.0
    pub rotation: f32,  // Radians
    pub scale_x: f32,   // X scale
    pub scale_y: f32,   // Y scale
    pub pivot_x: f32,   // Pivot X (0.0=left, 0.5=center, 1.0=right), defaults to 0.0
    pub pivot_y: f32,   // Pivot Y (0.0=top, 0.5=center, 1.0=bottom), defaults to 0.0
    pub offset_x: f32,  // Pixel offset applied after pivot, defaults to 0.0
    pub offset_y: f32,
}

impl Transform {
    fn new(x: f32, y: f32) -> Self;              // z = 0.0, pivot = (0, 0) top-left
    fn new_3d(x: f32, y: f32, z: f32) -> Self;   // Explicit z
    fn from_vec2(pos: Vec2) -> Self;
    fn from_vec3(pos: Vec3) -> Self;
    fn as_vec2(&self) -> Vec2;                   // Returns Vec2(x, y)
    fn as_vec3(&self) -> Vec3;                   // Returns Vec3(x, y, z)
    fn origin() -> Self;                         // (0, 0, 0)
    fn with_rotation(self, rotation: f32) -> Self;
    fn with_scale(self, scale_x: f32, scale_y: f32) -> Self;
    fn with_uniform_scale(self, scale: f32) -> Self;
    fn with_pivot(self, pivot_x: f32, pivot_y: f32) -> Self;  // Custom pivot point
    fn with_anchor(self, anchor: Anchor) -> Self;  // Pivot from an Anchor (e.g. Anchor::BottomCenter)
    fn with_offset(self, offset_x: f32, offset_y: f32) -> Self;  // Pixel offset after pivot
    fn with_pivot_center(self) -> Self;          // Pivot at (0.5, 0.5) - centered rotation
    fn with_pivot_top_center(self) -> Self;      // Pivot at (0.5, 0.0) - pendulums
    fn with_pivot_bottom_center(self) -> Self;   // Pivot at (0.5, 1.0) - standing objects
    // Also: with_pivot_top_left / top_right / center_left / center_right / bottom_left / bottom_right
}
```

**Pivot Point Examples:**
```rust
// Pendulum - rotates from top-center connection point
Transform::new(x, y).with_pivot_top_center().with_rotation(angle)

// Door hinge - rotates from left edge
Transform::new(x, y).with_pivot(0.0, 0.5).with_rotation(angle)

// Center rotation - spin around center
Transform::new(x, y).with_pivot_center().with_rotation(angle)

// Default - pivot at (0.0, 0.0) top-left for traditional sprite positioning
Transform::new(x, y).with_rotation(angle)
```

### Velocity

```rust
pub struct Velocity {
    pub dx: f32,  // X velocity (not x!)
    pub dy: f32,  // Y velocity (not y!)
    pub dz: f32,  // Z velocity, defaults to 0.0
}

impl Velocity {
    fn new(dx: f32, dy: f32) -> Self;            // dz defaults to 0.0
    fn new_3d(dx: f32, dy: f32, dz: f32) -> Self;
    fn from_vec2(vel: Vec2) -> Self;
    fn from_vec3(vel: Vec3) -> Self;
    fn as_vec2(&self) -> Vec2;                   // Returns Vec2(dx, dy)
    fn as_vec3(&self) -> Vec3;                   // Returns Vec3(dx, dy, dz)
    fn zero() -> Self;                           // (0, 0, 0)
}
```

### Renderable

```rust
pub struct Renderable {
    pub sprite: SpriteHandle,
    pub style: RenderStyle,       // Single (default) or Tiled
    pub mode: RenderMode,         // Auto (default), Cpu, Gpu
    pub visible: bool,
    pub z_order: i32,
    pub flip_x: bool,
    pub flip_y: bool,
    pub alpha: f32,               // 1.0 = opaque
    pub tint: Color,              // WHITE = no tint
    pub shader: Option<SpriteShader>,
    pub drop_shadow: Option<DropShadow>,
    pub blob_shadow: Option<BlobShadow>,
    pub contact_shadow: Option<ContactShadow>,
    pub interpolate: bool,
}

impl Renderable {
    fn sprite(handle: SpriteHandle) -> Self;
    fn tiled(sprite: SpriteHandle, direction: TileDirection, target_size: f32) -> Self;
    fn with_visible(self, visible: bool) -> Self;   // with_visible(false) to hide
    fn with_z_order(self, z: i32) -> Self;
    fn with_mode(self, mode: RenderMode) -> Self;
    fn with_flip_x(self, flip: bool) -> Self;
    fn with_flip_y(self, flip: bool) -> Self;
    fn with_alpha(self, alpha: f32) -> Self;
    fn with_tint(self, color: Color) -> Self;
    fn with_shader(self, shader: SpriteShader) -> Self;
    fn with_drop_shadow(self, shadow: DropShadow) -> Self;
    fn with_blob_shadow(self, shadow: BlobShadow) -> Self;
    fn with_contact_shadow(self, shadow: ContactShadow) -> Self;
    fn interpolated(self) -> Self;                  // Smooth rendering between fixed updates
    fn with_interpolation(self, enabled: bool) -> Self;
}
```

### RenderLayer

```rust
pub enum RenderLayer {
    Background,  // z_order: -1000
    Default,     // z_order: 0
    Foreground,  // z_order: 1000
    UI,          // z_order: 2000
}

impl RenderLayer {
    fn as_z_order(self) -> i32;
}
```

### Parent / Children (Hierarchy)

```rust
pub struct Parent(pub Entity);
pub struct Children(pub Vec<Entity>);
```

---

## Physics Components

### RigidBody

```rust
pub struct RigidBody {
    pub mass: f32,
    pub gravity_scale: f32,
    pub drag: f32,
    pub is_kinematic: bool,
    pub ccd_enabled: bool,
}

impl RigidBody {
    fn new() -> Self;          // Dynamic: affected by forces (also Default)
    fn kinematic() -> Self;    // Moved by code, affects others
    fn with_mass(self, mass: f32) -> Self;
    fn with_gravity_scale(self, scale: f32) -> Self;
    fn with_drag(self, drag: f32) -> Self;
    fn with_ccd(self) -> Self;  // Continuous collision detection for fast bodies
}
```

A static (immovable) body is just a `Collider` without a `RigidBody`.

### PhysicsConfig

```rust
pub struct PhysicsConfig {
    pub gravity: (f32, f32),  // Default (0.0, 980.0): +y is down
}

impl PhysicsConfig {
    fn new() -> Self;
    fn with_gravity(gx: f32, gy: f32) -> Self;
    fn zero_gravity() -> Self;
}
```

### Collider

```rust
pub struct Collider {
    pub shape: ColliderShape,
    pub offset_x: f32,
    pub offset_y: f32,
    pub is_trigger: bool,
    pub layer: u32,        // Default 1
    pub mask: u32,         // Default u32::MAX
    pub restitution: f32,  // Default 0.5
    pub friction: f32,     // Default 0.3
    pub center_in_bounds: Option<(f32, f32)>,
}

impl Collider {
    fn box_collider(width: f32, height: f32) -> Self;
    fn circle_collider(radius: f32) -> Self;
    fn centered_in(self, width: f32, height: f32) -> Self;  // Center on a sprite of this size
    fn with_offset(self, x: f32, y: f32) -> Self;
    fn as_trigger(self) -> Self;  // Triggers events but no physical response
    fn with_layer(self, layer: u32) -> Self;
    fn with_mask(self, mask: u32) -> Self;
    fn with_restitution(self, bounce: f32) -> Self;
    fn with_friction(self, friction: f32) -> Self;
    fn should_collide_with(&self, other: &Collider) -> bool;
}
```

### ColliderShape

```rust
pub enum ColliderShape {
    Box { width: f32, height: f32 },
    Circle { radius: f32 },
}
```

### PhysicsCollisionEvent

```rust
pub struct PhysicsCollisionEvent {
    pub entity_a: Entity,
    pub entity_b: Entity,
    pub event_type: CollisionEventType,
    pub is_sensor: bool,
}

impl PhysicsCollisionEvent {
    fn involves(&self, entity: Entity) -> bool;
    fn other(&self, entity: Entity) -> Option<Entity>;  // The other entity, if `entity` is part of this event
    fn is_started(&self) -> bool;
    fn is_stopped(&self) -> bool;
}

pub enum CollisionEventType {
    Started,
    Stopped,
}
```

Scenes receive these through the event bus as `CollisionStarted` / `CollisionStopped { entity_a, entity_b }` (both have `involves(entity)` and `other(entity) -> Option<Entity>`): `for e in ctx.events.read::<CollisionStarted>() { ... }`.

---

## Sprites & Assets

### Sprite

```rust
pub struct Sprite {
    data: Vec<u8>,   // RGBA pixel data
    width: u32,
    height: u32,
}

impl Sprite {
    // Create from data
    fn from_rgba(data: Vec<u8>, width: u32, height: u32) -> Self;
    
    // Solid shapes
    fn solid_rect(width: u32, height: u32, r: u8, g: u8, b: u8, a: u8) -> Self;
    fn solid_circle(diameter: u32, r: u8, g: u8, b: u8, a: u8) -> Self;
    fn solid_ellipse(width: u32, height: u32, r: u8, g: u8, b: u8, a: u8) -> Self;
    
    // Load from file
    fn load<P: AsRef<Path>>(path: P) -> SpriteResult<Self>;
    fn load_svg<P: AsRef<Path>>(path: P, scale: f32) -> SpriteResult<Self>;
    fn load_svg_fit<P: AsRef<Path>>(path: P, max_width: u32, max_height: u32) -> SpriteResult<Self>;
    
    // Properties
    fn width(&self) -> u32;
    fn height(&self) -> u32;
    fn data(&self) -> &[u8];
    fn scaled(&self, scale: f32) -> Self;
}
```

### SpriteHandle

Reference-counted handle to a loaded sprite. Clone freely.

### AssetManager

```rust
impl AssetManager {
    // Synchronous loading (blocks until complete)
    fn load_sprite<P: AsRef<Path>>(&mut self, path: P) -> AssetResult<SpriteHandle>;

    // Single async load
    fn load_sprite_async<P: AsRef<Path>>(&mut self, path: P) -> AsyncSpriteHandle;

    // Batch async loading (for loading screens with progress)
    fn start_loading<P, I>(&mut self, paths: I);  // Queue multiple assets
    fn process_async_loads(&mut self) -> usize;   // Process completed loads (call each frame)
    fn loading_progress(&self) -> LoadingProgress; // Get current progress
    fn is_loading_complete(&self) -> bool;        // All queued assets loaded?
    fn reset_loading_progress(&mut self);         // Reset for new batch

    // Creating sprites from data
    fn create_sprite<P: AsRef<Path>>(&self, sprite: Sprite, name: P) -> SpriteHandle;

    // Cache management
    fn cache_sprite_handle(&mut self, handle: SpriteHandle); // Pre-cache a loaded sprite
    fn is_sprite_cached<P: AsRef<Path>>(&self, path: P) -> bool;
    fn sprite_count(&self) -> usize;
    fn gc_sprites(&mut self) -> usize;
    fn clear_sprite_cache(&mut self);
    fn stats(&self) -> AssetStats;

    // Hot reload
    fn enable_hot_reload<P, I>(&mut self, watch_paths: I) -> AssetResult<()>;
    fn disable_hot_reload(&mut self);
    fn is_hot_reload_enabled(&self) -> bool;
    fn process_reloads(&mut self) -> usize;

    // WGSL shaders (cached by path; hot reload updates the handle)
    fn load_shader<P: AsRef<Path>>(&mut self, path: P) -> AssetResult<ShaderHandle>;
}

// ShaderHandle::from_source(id, wgsl) -> ShaderHandle; handle.replace_source(wgsl)
// SpriteShader::custom(handle).with_param("amount", 0.4)   // params by name: f32, Vec2/3/4, Color (sRGB/255; fills a vec3 or vec4)
// Material::custom(handle).with_param(..)                  // wins over Renderable::shader
// GpuRenderContext::add_custom_sprite(texture_id, &shader, &params, instance, z_order)
// Templates and colour-space rules: ryo-renderer skill, "Custom Shaders"
```

### LoadingProgress

Progress tracking for batch asset loading:

```rust
pub struct LoadingProgress {
    pub total: usize,      // Total assets in batch
    pub completed: usize,  // Assets finished (success or fail)
    pub succeeded: usize,  // Successfully loaded
    pub failed: usize,     // Failed to load
    pub current: Option<String>,  // Path of asset being loaded
    pub failures: Vec<String>,    // Paths that failed
}

impl LoadingProgress {
    fn percentage(&self) -> f32;      // 0.0 to 1.0
    fn current_asset(&self) -> &str;  // Filename of current asset (or "")
    fn is_loading(&self) -> bool;
    fn is_complete(&self) -> bool;
    fn has_failures(&self) -> bool;
}
```

**Loading Screen Example:**

```rust
// In Ready state - start loading
ctx.assets.start_loading(vec![
    "assets/player.png",
    "assets/enemies.png",
    "assets/background.png",
]);

// In Loading state - process and check progress
fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    // Process completed loads each frame
    ctx.assets.process_async_loads();

    // Get progress for UI
    let progress = ctx.assets.loading_progress();
    self.progress_bar_fill = progress.percentage();
    self.current_asset_text = progress.current_asset().to_string();

    // Check completion
    if ctx.assets.is_loading_complete() {
        return SceneAction::Switch(Box::new(GameScene::new()));
    }
    SceneAction::Continue
}
```

### AsyncSpriteHandle

```rust
impl AsyncSpriteHandle {
    fn state(&self) -> LoadingState;
    fn is_loading(&self) -> bool;
    fn is_loaded(&self) -> bool;
    fn is_complete(&self) -> bool;  // Loaded or failed
    fn is_failed(&self) -> bool;
    fn get(&self) -> Option<SpriteHandle>;
    fn get_or(&self, fallback: &SpriteHandle) -> SpriteHandle;
}

pub enum LoadingState {
    Loading,
    Loaded,
    Failed(String),
}
```

---

## Animation

### Animator

Simple component that plays one animation; the engine advances it each frame.

```rust
impl Animator {
    fn new(animation: Animation) -> Self;
    fn set_animation(&mut self, animation: Animation);
    fn play(&mut self);
    fn pause(&mut self);
    fn stop(&mut self);
    fn reset(&mut self);
    fn set_speed(&mut self, speed: f32);
    fn speed(&self) -> f32;
    fn is_playing(&self) -> bool;
    fn just_finished(&self) -> bool;
    fn current_sprite(&self) -> Option<&SpriteHandle>;
    fn current_frame_index(&self) -> usize;
    fn animation_name(&self) -> &str;
    fn update(&mut self, dt: f32);
}
```

### Animation

```rust
impl Animation {
    fn new(name: impl Into<String>, frames: Vec<SpriteHandle>, frame_duration: f32) -> Self;  // seconds per frame
    fn with_frame_durations(name: impl Into<String>, frames: Vec<SpriteHandle>, durations: Vec<f32>) -> Self;
    fn with_loop_mode(self, mode: LoopMode) -> Self;
    fn name(&self) -> &str;
    fn duration(&self) -> f32;
    fn frame_count(&self) -> usize;
    fn frame(&self, index: usize) -> Option<&SpriteHandle>;
    fn loop_mode(&self) -> LoopMode;
}

pub enum LoopMode {
    Once,
    Loop,        // Default
    PingPong,
    Times(u32),
}

pub enum PlaybackState {
    Playing,
    Paused,
    Stopped,
}
```

### Tween

Component for animating Transform properties over time:

```rust
pub struct Tween {
    // Target values (None = don't animate)
    pub target_x: Option<f32>,
    pub target_y: Option<f32>,
    pub target_rotation: Option<f32>,
    pub target_scale_x: Option<f32>,
    pub target_scale_y: Option<f32>,
    pub target_alpha: Option<f32>,   // Animates Renderable::alpha
    pub target_tint: Option<Color>,  // Animates Renderable::tint

    pub duration: f32,      // Seconds
    pub delay: f32,         // Seconds before starting
    pub easing: Ease,       // Easing function
    pub loop_mode: TweenLoop,
    pub speed: f32,           // Playback speed multiplier (default 1.0)
    pub playing: bool,
    pub just_started: bool,   // True for one frame when it begins
    pub just_finished: bool,  // True for one frame on completion
}

impl Tween {
    // Constructors
    fn new(duration: f32) -> Self;
    fn to_position(x: f32, y: f32, duration: f32) -> Self;
    fn to_rotation(rotation: f32, duration: f32) -> Self;
    fn to_scale(scale_x: f32, scale_y: f32, duration: f32) -> Self;
    fn to_uniform_scale(scale: f32, duration: f32) -> Self;
    fn fade_to(alpha: f32, duration: f32) -> Self;
    fn fade_out(duration: f32) -> Self;
    fn fade_in(duration: f32) -> Self;
    fn tint_to(tint: Color, duration: f32) -> Self;

    // Builder methods
    fn to_x(self, x: f32) -> Self;
    fn to_y(self, y: f32) -> Self;
    fn to_rotation_value(self, rotation: f32) -> Self;
    fn to_scale_x(self, scale_x: f32) -> Self;
    fn to_scale_y(self, scale_y: f32) -> Self;
    fn to_alpha(self, alpha: f32) -> Self;
    fn to_tint(self, tint: Color) -> Self;
    fn with_easing(self, easing: Ease) -> Self;
    fn with_delay(self, delay: f32) -> Self;
    fn with_loop(self, loop_mode: TweenLoop) -> Self;
    fn with_speed(self, speed: f32) -> Self;
    fn on_start(self, cb: impl Fn(f32) + Send + Sync + 'static) -> Self;
    fn on_update(self, cb: impl Fn(f32) + Send + Sync + 'static) -> Self;
    fn on_complete(self, cb: impl Fn(f32) + Send + Sync + 'static) -> Self;

    // Control
    fn pause(&mut self);
    fn resume(&mut self);
    fn stop(&mut self);        // Reset to beginning
    fn restart(&mut self);     // Stop + play

    // State
    fn is_finished(&self) -> bool;
    fn progress(&self) -> f32;  // 0.0 to 1.0
}
```

### Ease

Easing functions for interpolation:

```rust
pub enum Ease {
    Linear,
    // Quad
    InQuad, OutQuad, InOutQuad,
    // Cubic
    InCubic, OutCubic, InOutCubic,
    // Quart
    InQuart, OutQuart, InOutQuart,
    // Sine
    InSine, OutSine, InOutSine,
    // Expo
    InExpo, OutExpo, InOutExpo,
    // Back (overshoot)
    InBack, OutBack, InOutBack,
    // Elastic
    InElastic, OutElastic, InOutElastic,
    // Bounce
    InBounce, OutBounce, InOutBounce,
}

impl Ease {
    fn apply(&self, t: f32) -> f32;  // t in [0,1], returns eased value
}
```

### TweenLoop

```rust
pub enum TweenLoop {
    Once,           // Play once, stop
    Loop,           // Repeat forever
    PingPong,       // Forward then backward
    Times(u32),     // Repeat n times
}
```

### TweenCallback

```rust
pub type TweenCallback = Arc<dyn Fn(f32) + Send + Sync>;
```

Callbacks receive eased progress (0.0–1.0). `on_start` gets `0.0`, `on_complete` gets `1.0`.

### Lerp

Trait for generic interpolation:

```rust
pub trait Lerp: Clone + Send + Sync + 'static {
    fn lerp(start: &Self, end: &Self, t: f32) -> Self;
}
```

Built-in impls: `f32`, `(f32, f32)`, `Vec2`, `Color`, `Rect`

### PropertyTween\<T: Lerp\>

Generic component for interpolating any `Lerp` type:

```rust
pub struct PropertyTween<T: Lerp> { /* internal state */ }

impl<T: Lerp> PropertyTween<T> {
    // Constructor
    fn new(start: T, end: T, duration: f32) -> Self;

    // Builder methods (mirrors Tween)
    fn with_easing(self, easing: Ease) -> Self;
    fn with_delay(self, delay: f32) -> Self;
    fn with_loop(self, loop_mode: TweenLoop) -> Self;
    fn with_speed(self, speed: f32) -> Self;
    fn on_start(self, cb: impl Fn(f32) + Send + Sync + 'static) -> Self;
    fn on_update(self, cb: impl Fn(f32) + Send + Sync + 'static) -> Self;
    fn on_complete(self, cb: impl Fn(f32) + Send + Sync + 'static) -> Self;

    // Reading state
    fn value(&self) -> T;           // Current interpolated value
    fn progress(&self) -> f32;      // Raw 0.0-1.0
    fn playing(&self) -> bool;
    fn is_finished(&self) -> bool;
    fn just_started(&self) -> bool;
    fn just_finished(&self) -> bool;

    // Runtime control
    fn pause(&mut self);
    fn resume(&mut self);
    fn stop(&mut self);             // Freezes at current value
    fn restart(&mut self);
    fn retarget(&mut self, new_end: T);  // Change destination mid-flight

    // Manual update (for custom Lerp types not auto-driven)
    fn update(&mut self, dt: f32);
}
```

Auto-driven types: `PropertyTween<f32>`, `PropertyTween<Vec2>`, `PropertyTween<Color>`. Others need manual `update(dt)`.

### TweenSequence

Component that plays `Tween`s one after another:

```rust
TweenSequence::new()
    .then(Tween::to_position(100.0, 0.0, 0.5))
    .then(Tween::fade_out(0.3))
    .with_loop();   // Optional: repeat the whole sequence
// Also: then_all(iter), with_speed(f32), pause/resume/stop/restart, is_finished(), len(), current_index()
```

### AnimationController

Component that switches between named animations (`AnimationId` is `&'static str`):

```rust
impl AnimationController {
    fn new() -> Self;
    fn add_animation(&mut self, id: AnimationId, animation: Animation);
    fn set_state(&mut self, state: AnimationId);
    fn transition_after_finish(&mut self, state: AnimationId);  // Switch when the current animation finishes
    fn current_state(&self) -> Option<AnimationId>;
    fn just_finished(&self) -> bool;
    fn current_sprite(&self) -> Option<&SpriteHandle>;
    fn play(&mut self);
    fn pause(&mut self);
    fn reset(&mut self);
    fn set_speed(&mut self, speed: f32);
    fn update(&mut self, dt: f32);
}
```

`ValidatedAnimator` is the same idea with an explicit transition graph (`new(initial_anim, initial_id)`, `with_animation`, `with_transition(from, to)`, `with_bidirectional`, `with_transition_from_any`, `set_animation(id) -> bool`, `force_animation(id)`); see the `ryo-animation` skill.

---

## Audio

### AudioManager

Reached via `ctx.audio: &mut Option<AudioManager>` (`None` if no audio device).

```rust
impl AudioManager {
    // Sound effects (short, can overlap): load once by name, then play by name
    fn load_sound(&mut self, name: &str, path: impl AsRef<Path>) -> AudioResult<()>;
    fn play_sound(&mut self, name: &str) -> AudioResult<SoundHandle>;
    fn play_sound_with(&mut self, name: &str, options: SoundOptions) -> AudioResult<SoundHandle>;
    fn has_sound(&self, name: &str) -> bool;
    fn unload_sound(&mut self, name: &str);
    fn clear_sounds(&mut self);

    // Music (streamed from file)
    fn play_music(&mut self, path: impl AsRef<Path>) -> AudioResult<MusicHandle>;
    fn play_music_with(&mut self, path: impl AsRef<Path>, options: MusicOptions) -> AudioResult<MusicHandle>;
    fn play_music_once(&mut self, path: impl AsRef<Path>, options: MusicOptions) -> AudioResult<bool>;  // No-op if music already playing
    fn crossfade_music(&mut self, current: &mut MusicHandle, new_path: impl AsRef<Path>, fade_duration: Duration) -> AudioResult<MusicHandle>;
    fn is_music_playing(&self) -> bool;
    fn current_music(&mut self) -> Option<&mut MusicHandle>;
    fn stop_music(&mut self);
    fn stop_music_fade(&mut self, duration: Duration);

    // Volume control (0.0 - 1.0)
    fn set_master_volume(&mut self, volume: f32);
    fn set_music_volume(&mut self, volume: f32);
    fn set_sfx_volume(&mut self, volume: f32);
    fn master_volume(&self) -> f32;
    fn music_volume(&self) -> f32;
    fn sfx_volume(&self) -> f32;
}
```

`SoundHandle`: `stop`, `pause`, `resume`, `set_volume`, `set_panning`, `set_playback_rate`, `is_playing`, `is_finished`.
`MusicHandle`: `stop`, `pause`, `resume`, `set_volume`, `set_panning`, `set_playback_rate`, `is_playing`, `fade_to(volume, Duration)`, `fade_out(Duration)`, `fade_out_and_stop(Duration)`.

### SoundOptions / MusicOptions

```rust
pub struct SoundOptions {
    pub volume: f32,         // 0.0 - 1.0 (default 1.0)
    pub panning: f32,        // -1.0 left, 0.0 center, 1.0 right
    pub playback_rate: f32,  // 1.0 = normal
}
// Builders: SoundOptions::new().volume(0.5).panning(-1.0).playback_rate(1.2)

pub struct MusicOptions {
    pub volume: f32,
    pub panning: f32,
    pub playback_rate: f32,
    pub looping: bool,                       // Default true
    pub fade_in: Option<Duration>,
    pub fade_out_on_drop: Option<Duration>,
}
// Builders: MusicOptions::new().volume(0.6).looping(false).fade_in(d).fade_out_on_drop(d).once()
```

---

## Input

### InputState

```rust
impl InputState {
    // Keyboard
    fn key_pressed(&self, key: KeyCode) -> bool;   // Just pressed this frame
    fn key_down(&self, key: KeyCode) -> bool;      // Currently held
    fn key_released(&self, key: KeyCode) -> bool;  // Just released this frame
    
    // Mouse
    fn mouse_pressed(&self, button: MouseButton) -> bool;
    fn mouse_down(&self, button: MouseButton) -> bool;
    fn mouse_released(&self, button: MouseButton) -> bool;
    fn mouse_position(&self) -> (f32, f32);       // Screen coordinates
    fn mouse_delta(&self) -> (f32, f32);          // Movement since last frame
    fn scroll_delta(&self) -> (f32, f32);         // Mouse wheel (x, y)
    fn mouse_x(&self) -> f32;
    fn mouse_y(&self) -> f32;

    // Consuming (prevents double-handling when stacking UI/scenes)
    fn key_pressed_and_consume(&mut self, key: KeyCode) -> bool;
    fn mouse_pressed_and_consume(&mut self, button: MouseButton) -> bool;

    // Text entry
    fn text_input(&self) -> &str;                 // Characters typed this frame

    // Gamepad (see the ryo-controller skill)
    fn gamepad_connected(&self) -> bool;
    fn gamepad_button_down(&self, button: GamepadButton) -> bool;
    fn gamepad_button_pressed(&self, button: GamepadButton) -> bool;
    fn left_stick(&self) -> (f32, f32);           // Deadzone applied
    fn right_stick(&self) -> (f32, f32);
}
```

### KeyCode (from winit)

Common keys:
- Letters: `KeyCode::KeyA` through `KeyCode::KeyZ`
- Numbers: `KeyCode::Digit0` through `KeyCode::Digit9`
- Arrows: `KeyCode::ArrowUp`, `ArrowDown`, `ArrowLeft`, `ArrowRight`
- Special: `KeyCode::Space`, `Escape`, `Enter`, `Tab`, `Backspace`
- Modifiers: `KeyCode::ShiftLeft`, `ControlLeft`, `AltLeft`

### MouseButton (from winit)

```rust
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Back,
    Forward,
    Other(u16),
}
```

---

## Camera

**CRITICAL:** `position` is the center of the view. The engine's `ctx.camera` starts at half the viewport size,
so world (0, 0) is the screen top-left (screen-style coordinates). A camera built with `Camera::new` starts at (0, 0),
which puts the world origin at screen center. Scenes that want a centered origin must call `set_position(0.0, 0.0)`.

```rust
pub struct Camera {
    pub x: f32,      // View center in world space
    pub y: f32,
    pub zoom: f32,   // 1.0 = normal (set_zoom clamps to 0.1..=10.0)
    // viewport size is private: use viewport_size()
}

impl Camera {
    fn new(viewport_width: u32, viewport_height: u32) -> Self;
    fn centered_at(x: f32, y: f32, viewport_width: u32, viewport_height: u32) -> Self;

    fn set_position(&mut self, x: f32, y: f32);
    fn set_zoom(&mut self, zoom: f32);
    fn viewport_size(&self) -> (f32, f32);        // (width, height)

    fn screen_to_world(&self, screen_x: f32, screen_y: f32) -> (f32, f32);
    fn world_to_screen(&self, world_x: f32, world_y: f32) -> (f32, f32);

    fn visible_bounds(&self) -> CameraBounds;     // World-space AABB { left, right, top, bottom }
    fn is_visible(&self, world_x: f32, world_y: f32, width: f32, height: f32) -> bool;

    fn follow(&mut self, target_x: f32, target_y: f32, lerp_factor: f32);
    fn snap_to(&mut self, target_x: f32, target_y: f32);
    fn clamp_to_bounds(&mut self, world_min_x: f32, world_min_y: f32, world_max_x: f32, world_max_y: f32);
    fn reset(&mut self);                          // Position to viewport center, zoom 1.0, clear shake; keep viewport

    // Shake (trauma-based)
    fn add_trauma(&mut self, amount: f32);        // 0.0-1.0, accumulates
    fn configure_shake(&mut self, max_offset: f32, decay_rate: f32);
    fn trauma(&self) -> f32;
    fn reset_trauma(&mut self);
}

// Centered-origin setup (world (0,0) = screen center):
ctx.camera.set_position(0.0, 0.0);

// Screen-style coordinates (default in scenes; 0,0 = top-left):
let (vw, vh) = ctx.camera.viewport_size();
ctx.camera.set_position(vw / 2.0, vh / 2.0);
```

---

## World (ECS)

```rust
impl World {
    // Spawning (components are a tuple; hecs::DynamicBundle)
    fn spawn(&mut self, components: impl DynamicBundle) -> Entity;
    fn spawn_empty(&mut self) -> Entity;
    fn despawn(&mut self, entity: Entity) -> Result<(), NoSuchEntity>;
    fn clear(&mut self);                                   // Remove all entities

    // Component access (returns hecs::Ref / RefMut guards that deref to &T / &mut T)
    fn get<T: Component>(&self, entity: Entity) -> Option<Ref<'_, T>>;
    fn get_mut<T: Component>(&self, entity: Entity) -> Option<RefMut<'_, T>>;  // Takes &self
    fn has<T: Component>(&self, entity: Entity) -> bool;
    fn insert_one<T: Component>(&mut self, entity: Entity, component: T) -> Result<(), NoSuchEntity>;
    fn insert(&mut self, entity: Entity, components: impl DynamicBundle) -> Result<(), NoSuchEntity>;
    fn remove_one<T: Component>(&mut self, entity: Entity) -> Result<T, ComponentError>;
    // Aliases: get_component / get_component_mut (deprecated), has_component, add_component

    // Querying
    fn query<Q: Query>(&self) -> QueryBorrow<'_, Q>;       // call .iter()
    fn query_mut<Q: Query>(&mut self) -> QueryMut<'_, Q>;  // is itself an iterator
    fn entities_with<T: Component>(&self) -> Vec<Entity>;
    fn entities(&self) -> Vec<Entity>;

    // Entity checks
    fn contains(&self, entity: Entity) -> bool;            // Is this entity alive?
    fn len(&self) -> u32;                                  // Entity count
    fn is_empty(&self) -> bool;

    // Hierarchy
    fn spawn_child(&mut self, parent: Entity, components: impl DynamicBundle + Send + Sync) -> Entity;
    fn set_parent(&mut self, child: Entity, new_parent: Entity) -> Result<(), NoSuchEntity>;
    fn unparent(&mut self, child: Entity) -> Result<(), NoSuchEntity>;
    fn despawn_recursive(&mut self, entity: Entity) -> Result<(), NoSuchEntity>;
    fn parent(&self, entity: Entity) -> Option<Entity>;
    fn children(&self, entity: Entity) -> Vec<Entity>;
}
```

### Query Examples

**IMPORTANT:** Query iteration returns components directly, NOT `(entity, components)`.
The query type must match the destructuring pattern exactly. To get entity IDs, include `Entity` in the query tuple: `query::<(Entity, &Transform)>()`.

```rust
// Single component - note: just (transform,) not (entity, transform)
for (transform,) in ctx.world.query::<(&Transform,)>().iter() { }

// Multiple components - flat tuple, no entity
for (transform, velocity) in ctx.world.query::<(&Transform, &Velocity)>().iter() { }

// Mutable access - use &mut in query type
for (transform, velocity) in ctx.world.query::<(&Transform, &mut Velocity)>().iter() {
    velocity.dx = 100.0;  // Can modify
}

// Optional components
for (transform, maybe_vel) in ctx.world.query::<(&Transform, Option<&Velocity>)>().iter() {
    if let Some(vel) = maybe_vel { /* has velocity */ }
}

// Entity IDs: include Entity in the query tuple
for (entity, transform) in ctx.world.query::<(Entity, &Transform)>().iter() { }

// To get a component from a stored entity, use get / get_mut:
if let Some(transform) = ctx.world.get::<Transform>(self.player_entity) {
    // Access transform.x, transform.y, etc.
}
```

---

## Scene Management

### SceneAction

```rust
pub enum SceneAction {
    Continue,                          // Keep running current scene (default)
    Push(Box<dyn Scene>),              // Push new scene on stack (pauses current)
    PushWith(Box<dyn Scene>, Transition),
    Pop,                               // Return to previous scene
    PopWith(Transition),
    Switch(Box<dyn Scene>),            // Replace current scene (NOT Replace!)
    SwitchWith(Box<dyn Scene>, Transition),
    Reset(Box<dyn Scene>),             // Clear all scenes, push new one
    ResetWith(Box<dyn Scene>, Transition),
    PopReturn(Box<dyn Any>),           // Pop and return data to scene below
    PopReturnWith(Box<dyn Any>, Transition),
    Quit,                              // Exit the game
}

// In on_resume, unwrap data sent by PopReturn:
// if let Some(score) = downcast_return::<u32>(returned) { ... }
pub fn downcast_return<T: Any>(returned: Option<Box<dyn Any>>) -> Option<T>;
```

### SceneCleanup

```rust
// Bitflags controlling automatic state reset on Switch/Reset
pub struct SceneCleanup(u16);

impl SceneCleanup {
    const NONE: Self;      // No cleanup
    const WORLD: Self;     // world.clear()
    const CAMERA: Self;    // Reset position/zoom/shake (viewport preserved)
    const EFFECTS: Self;   // Clear post-processing effects
    const LIGHTING: Self;  // Reset ambient light and atmosphere to defaults
    const TIME: Self;      // Reset time scale and hitstop
    const MUSIC: Self;     // Stop music
    const EVENTS: Self;    // Clear event bus
    const RAYCAST: Self;   // Disable raycast renderer and drop its world map
    const ALL: Self;       // All flags
    const DEFAULT: Self;   // All except MUSIC

    fn contains(self, other: Self) -> bool;
    fn is_empty(self) -> bool;
}

// Supports BitOr (|), BitAnd (&), Not (!), Sub (-)
// Example: SceneCleanup::DEFAULT - SceneCleanup::WORLD
```

### Transition

```rust
pub enum Transition {
    None,
    Fade { duration: Duration, color: [u8; 4] },   // RGBA; duration is per half (out, then in)
    Slide { duration: Duration, direction: SlideDirection },
}

impl Transition {
    fn fade(duration: Duration) -> Self;                          // Fade through black
    fn fade_white(duration: Duration) -> Self;
    fn fade_color(duration: Duration, r: u8, g: u8, b: u8) -> Self;
    fn slide(duration: Duration, direction: SlideDirection) -> Self;
    fn duration(&self) -> Duration;                               // Total (fade = out + in)
}

pub enum SlideDirection {
    Left,
    Right,
    Up,
    Down,
}
```

---

## UI Types

### Color

```rust
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    const WHITE: Color;
    const BLACK: Color;
    const RED: Color;
    const GREEN: Color;
    const BLUE: Color;
    const YELLOW: Color;
    const CYAN: Color;
    const MAGENTA: Color;
    const TRANSPARENT: Color;
    const GRAY: Color;
    const DARK_GRAY: Color;
    const LIGHT_GRAY: Color;
    // UI presets: HEALTH_RED, MANA_BLUE, STAMINA_GREEN, XP_GOLD, PANEL_BG, PANEL_BORDER

    const fn new(r: u8, g: u8, b: u8, a: u8) -> Self;
    const fn rgb(r: u8, g: u8, b: u8) -> Self;                  // Opaque
    const fn hex(value: u32) -> Self;                           // 0xRRGGBB (opaque) or 0xRRGGBBAA
    const fn with_alpha(self, a: u8) -> Self;
    const fn to_array(self) -> [u8; 4];
}
```

### Rect

```rust
pub struct Rect { pub x: f32, pub y: f32, pub width: f32, pub height: f32 }

impl Rect {
    const fn new(x: f32, y: f32, width: f32, height: f32) -> Self;
    fn centered_at(cx: f32, cy: f32, width: f32, height: f32) -> Self;
    fn right(&self) -> f32;
    fn bottom(&self) -> f32;
    fn center(&self) -> (f32, f32);
    fn contains(&self, px: f32, py: f32) -> bool;
    fn shrink(&self, padding: f32) -> Self;
    fn expand(&self, padding: f32) -> Self;
}
```

### Anchor

```rust
pub enum Anchor {
    TopLeft,
    TopCenter,
    TopRight,
    CenterLeft,
    Center,
    CenterRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
}

impl Anchor {
    fn to_pivot(self) -> (f32, f32);  // e.g. BottomCenter -> (0.5, 1.0)
}
```

---

## Post-Processing

### EffectsManager

```rust
impl EffectsManager {
    fn update(&mut self, dt: f32);
    fn active_effects(&self) -> impl Iterator<Item = &PostProcessEffect>;
    fn has_active_effects(&self) -> bool;
    fn clear(&mut self);
    fn time(&self) -> f32;

    // Fade
    fn set_fade(&mut self, progress: f32);                        // 0.0-1.0
    fn set_fade_color(&mut self, progress: f32, color: Color);

    // Color tint
    fn set_color_tint(&mut self, color: Color, intensity: f32);
    fn set_sepia(&mut self, intensity: f32);

    // Vignette
    fn set_vignette(&mut self, intensity: f32);
    fn set_vignette_custom(&mut self, intensity: f32, radius: f32, softness: f32);

    // Screen flash (timed, auto-decays)
    fn flash(&mut self, color: Color, duration: Duration);
    fn flash_secs(&mut self, color: Color, duration_secs: f32);
    fn flash_white(&mut self, duration: Duration);
    fn flash_red(&mut self, duration: Duration);

    // CRT
    fn enable_crt(&mut self);
    fn enable_crt_custom(&mut self, scanline_intensity: f32, curvature: f32, rgb_offset: f32);
    fn disable_crt(&mut self);

    // Chromatic aberration
    fn set_chromatic_aberration(&mut self, offset: f32);
    fn disable_chromatic_aberration(&mut self);

    // God rays
    fn set_god_rays(&mut self, intensity: f32, light_positions: Vec<(f32, f32)>);
    fn set_god_rays_custom(&mut self, intensity: f32, samples: u32, decay: f32, density: f32, threshold: f32, light_positions: Vec<(f32, f32)>);
    fn update_god_rays_lights(&mut self, light_positions: Vec<(f32, f32)>);
    fn disable_god_rays(&mut self);
    fn has_god_rays(&self) -> bool;

    // Ambient occlusion
    fn set_ambient_occlusion(&mut self, intensity: f32, radius: f32);
    fn set_ambient_occlusion_custom(&mut self, intensity: f32, radius: f32, bias: f32);
    fn disable_ambient_occlusion(&mut self);
    fn has_ambient_occlusion(&self) -> bool;

    // Bloom
    fn set_bloom(&mut self, intensity: f32);                      // 0.0-5.0
    fn set_bloom_custom(&mut self, threshold: f32, intensity: f32, radius: f32, blur_passes: u32);
    fn disable_bloom(&mut self);
    fn has_bloom(&self) -> bool;

    // Custom WGSL effects (fn effect(in: EffectIn, params: Params) -> vec4<f32>); run after built-ins, in insertion order
    fn add_custom(&mut self, name: impl Into<String>, shader: ShaderHandle) -> Result<(), EffectError>;
    fn set_custom_param(&mut self, name: &str, param: &str, value: impl Into<ParamValue>) -> bool;
    fn set_custom_enabled(&mut self, name: &str, enabled: bool) -> bool;
    fn remove_custom(&mut self, name: &str) -> bool;
    fn has_custom(&self, name: &str) -> bool;
}
```

### PostProcessEffect

```rust
pub enum PostProcessEffect {
    Fade { progress: f32, color: Color },
    ColorTint { color: Color, intensity: f32 },
    Vignette { intensity: f32, radius: f32, softness: f32 },
    ScreenFlash { color: Color, intensity: f32 },
    Crt { scanline_intensity: f32, curvature: f32, rgb_offset: f32 },
    ChromaticAberration { offset: f32, radial: bool },
    GodRays { intensity: f32, samples: u32, decay: f32, density: f32, weight: f32, threshold: f32, light_positions: Vec<(f32, f32)> },
    AmbientOcclusion { intensity: f32, radius: f32, bias: f32 },
    Bloom { threshold: f32, intensity: f32, radius: f32, blur_passes: u32 },
}
```

---

## Error Types

All error types implement `std::error::Error`:

```rust
pub type EngineResult<T> = Result<T, EngineError>;
pub type AssetResult<T> = Result<T, AssetError>;
pub type AudioResult<T> = Result<T, AudioError>;
pub type SpriteResult<T> = Result<T, SpriteError>;
pub type SaveResult<T> = Result<T, SaveError>;
pub type NetworkResult<T> = Result<T, NetworkError>;
// Also: HotReloadResult, ReplicationResult, InventoryResult
```

---

## Networking

### Server Types

```rust
// Dedicated server (no local player)
impl NetworkServer {
    fn new(config: NetworkServerConfig) -> Result<Self, NetworkError>;
    fn update(&mut self, dt: Duration);
    fn events(&mut self) -> impl Iterator<Item = NetworkEvent>;
    fn send(&mut self, client_id: ClientId, channel: Channel, data: &[u8]);
    fn broadcast(&mut self, channel: Channel, data: &[u8]);
    fn broadcast_except(&mut self, exclude: ClientId, channel: Channel, data: &[u8]);
    fn receive(&mut self, client_id: ClientId, channel: Channel) -> Option<Vec<u8>>;
    fn clients(&self) -> impl Iterator<Item = ClientId>;
    fn disconnect(&mut self, client_id: ClientId);
    fn is_running(&self) -> bool;
    fn connected_count(&self) -> usize;
}

// Player-hosted server (host plays while serving)
impl ListenServer {
    fn new(config: NetworkServerConfig) -> Result<Self, NetworkError>;
    fn update(&mut self, dt: Duration);
    fn events(&mut self) -> impl Iterator<Item = NetworkEvent>;
    fn send(&mut self, client_id: ClientId, channel: Channel, data: &[u8]);
    fn broadcast(&mut self, channel: Channel, data: &[u8]);
    fn broadcast_except(&mut self, exclude: ClientId, channel: Channel, data: &[u8]);
    fn receive(&mut self, client_id: ClientId, channel: Channel) -> Option<Vec<u8>>;
    fn disconnect(&mut self, client_id: ClientId);
    fn host_id(&self) -> ClientId;           // Host's virtual ID (always 0)
    fn is_host(&self, id: ClientId) -> bool;
    fn all_clients(&self) -> impl Iterator<Item = ClientId>;     // Host + remotes
    fn remote_clients(&self) -> impl Iterator<Item = ClientId>;  // Only remotes
    fn player_count(&self) -> usize;         // Includes host
}

// Client connecting to a server
impl NetworkClient {
    fn connect(config: NetworkClientConfig) -> Result<Self, NetworkError>;
    fn update(&mut self, dt: Duration);
    fn events(&mut self) -> impl Iterator<Item = NetworkEvent>;
    fn send(&mut self, channel: Channel, data: &[u8]);
    fn receive(&mut self, channel: Channel) -> Option<Vec<u8>>;
    fn client_id(&self) -> ClientId;
    fn is_connected(&self) -> bool;
    fn is_connecting(&self) -> bool;
    fn is_disconnected(&self) -> bool;
    fn disconnect(&mut self);
    fn disconnect_reason(&self) -> Option<String>;
}

// Shared by NetworkServer and ListenServer (used by ReplicationManager)
pub trait ServerLike {
    fn send(&mut self, client_id: ClientId, channel: Channel, data: &[u8]);
    fn broadcast(&mut self, channel: Channel, data: &[u8]);
    fn receive(&mut self, client_id: ClientId, channel: Channel) -> Option<Vec<u8>>;
    fn clients(&self) -> Box<dyn Iterator<Item = ClientId> + '_>;
    fn is_host(&self, client_id: ClientId) -> bool;
}

// Client-side counterpart (implemented by NetworkClient; taken by process_incoming)
pub trait ClientLike {
    fn send(&mut self, channel: Channel, data: &[u8]);
    fn receive(&mut self, channel: Channel) -> Option<Vec<u8>>;
}
```

### Configuration

```rust
pub struct NetworkServerConfig {
    pub port: u16,
    pub max_clients: usize,
    pub protocol_id: u64,
    pub transport: TransportConfig,   // Udp (default) or WebRtc (matchbox feature)
}

pub struct NetworkClientConfig {
    pub server_addr: Option<SocketAddr>,
    pub protocol_id: u64,
    pub transport: TransportConfig,
}

impl NetworkClientConfig {
    fn udp(server_addr: SocketAddr, protocol_id: u64) -> Self;
}

pub enum Channel {
    Unreliable,          // Best-effort, no ordering
    ReliableOrdered,     // Guaranteed delivery + order
    ReliableUnordered,   // Guaranteed delivery, any order
    ReplicationReliable,   // Reserved for ReplicationManager - don't use for game messages
    ReplicationUnreliable, // Reserved for ReplicationManager - don't use for game messages
}

pub type ClientId = u64;   // renet::ClientId
pub const LOCAL_HOST_ID: ClientId = 0;
```

### Events

```rust
pub enum NetworkEvent {
    Connected,                                                  // Client side
    Disconnected { reason: String },                            // Client side
    ClientConnected { client_id: ClientId },                    // Server side
    ClientDisconnected { client_id: ClientId, reason: String }, // Server side
}

// Used by the ECS events DisconnectedFromServer / ClientDisconnected
pub enum DisconnectReason {
    Unknown,
    ServerClosed,
    Timeout,
    Kicked,
}
```

### Replication

```rust
impl ReplicationManager {
    fn new_server() -> Self;
    fn new_client() -> Self;
    fn register<T: Replicated>(&mut self);
    fn spawn_networked(&mut self, world: &mut World, builder: NetworkEntityBuilder) -> Entity;
    fn despawn_networked(&mut self, world: &mut World, entity: Entity) -> Option<NetworkId>;
    fn mark_dirty<T: Replicated>(&mut self, entity: Entity);
    fn broadcast_updates(&mut self, world: &World, server: &mut impl ServerLike);
    fn send_full_state(&mut self, world: &World, server: &mut impl ServerLike, client: ClientId);
    fn process_incoming(&mut self, world: &mut World, client: &mut impl ClientLike);  // also sends acks
    fn client_acked_generation(&self, client_id: ClientId) -> Option<u64>;
    fn set_refresh_interval(&mut self, interval: u64);  // broadcasts between full refreshes; default 300, 0 disables; refresh is spread over broadcasts in MAX_REFRESH_BYTES messages, changes go in MAX_BATCH_BYTES batches
    fn get_entity(&self, network_id: NetworkId) -> Option<Entity>;
}

impl NetworkEntityBuilder {
    fn new() -> Self;
    fn server_owned(self) -> Self;
    fn client_owned(self, client_id: ClientId) -> Self;
    fn with_component<T: Replicated>(self, component: T) -> Self;
}

// Trait for replicatable components
pub trait Replicated: Component + Serialize + DeserializeOwned + Clone {
    fn component_id() -> ComponentTypeId;
    fn component_name() -> &'static str;
    fn replication_priority() -> u8 { 1 }  // Higher = more frequent
}
```

### RPCs

```rust
// Trait for RPC types
pub trait Rpc: Serialize + DeserializeOwned + Clone + Send + Sync + 'static {
    fn rpc_id() -> RpcTypeId;
    fn rpc_name() -> &'static str;
}

pub enum RpcTarget {
    Server,
    Client(ClientId),
    Broadcast,
    BroadcastExcept(ClientId),
}

impl SerializedRpc {
    fn from_rpc<T: Rpc>(rpc: &T, sender: ClientId, target: RpcTarget) -> Result<Self, serde_json::Error>;
    fn from_bytes(data: &[u8]) -> Result<Self, serde_json::Error>;
    fn to_bytes(&self) -> Result<Vec<u8>, serde_json::Error>;
    fn into_rpc<T: Rpc>(self) -> Result<ReceivedRpc<T>, serde_json::Error>;
}

pub struct ReceivedRpc<T: Rpc> {
    pub payload: T,
    pub sender: ClientId,
}
```

### Errors

```rust
pub enum NetworkError {
    InitError(String),
    ConnectionError(String),
    SendError(String),
    ReceiveError(String),
    TransportError(String),
    NotConnected,
    NotRunning,
}
```

---

## Navigation & Pathfinding

### NavAgent

```rust
pub struct NavAgent {
    pub current_path: Option<Path>,
    pub speed: f32,
    pub arrival_threshold: f32,
    pub avoid_agents: bool,
    pub radius: f32,
    pub path_smoothing: PathSmoothing,
}

impl NavAgent {
    fn new(speed: f32) -> Self;
    fn with_arrival_threshold(self, threshold: f32) -> Self;
    fn with_avoidance(self, radius: f32) -> Self;  // Enables avoid_agents
    fn with_smoothing(self, smoothing: PathSmoothing) -> Self;
    fn state(&self) -> NavAgentState;
    fn has_path(&self) -> bool;
    fn destination(&self) -> Option<(f32, f32)>;
    fn remaining_distance(&self) -> f32;
    fn set_path(&mut self, path: Path);
    fn stop(&mut self);
    fn arrive(&mut self) -> bool;
    fn fail(&mut self) -> bool;
}
```

### NavAgentState

```rust
pub enum NavAgentState {
    Idle,       // No target
    Computing,  // Finding path
    Moving,     // Following path
    Arrived,    // Reached destination
    Failed,     // Could not find/follow path
}

impl NavAgentState {
    fn is_active(&self) -> bool;    // Computing or Moving
    fn is_finished(&self) -> bool;  // Arrived or Failed
}
```

### NavTarget

```rust
pub struct NavTarget {
    pub x: f32,
    pub y: f32,
    pub dynamic_target: bool,         // Re-path as the target moves
    pub follow_entity: Option<Entity>,
    pub repath_threshold: f32,        // Re-path if the target moved farther than this
}

impl NavTarget {
    fn position(x: f32, y: f32) -> Self;
    fn follow(entity: Entity) -> Self;
    fn with_dynamic(self, dynamic: bool) -> Self;
    fn with_repath_threshold(self, threshold: f32) -> Self;
}
```

### Steering Module

Standalone steering behaviors for agent movement:

```rust
pub mod steering {
    /// Push away from nearby agents
    fn separation(
        position: Vec2,
        radius: f32,
        neighbors: &[(Vec2, f32)],  // (pos, radius)
        strength: f32,
    ) -> Vec2;

    /// Move toward a target at max speed
    fn seek(position: Vec2, target: Vec2, max_speed: f32) -> Vec2;

    /// Move away from a threat at max speed
    fn flee(position: Vec2, threat: Vec2, max_speed: f32) -> Vec2;

    /// Seek but slow down when within slow_radius
    fn arrive(
        position: Vec2,
        target: Vec2,
        max_speed: f32,
        slow_radius: f32,
    ) -> Vec2;

    /// Move toward center of mass of neighbors
    fn cohesion(
        position: Vec2,
        neighbors: &[(Vec2, f32)],  // (pos, radius)
        max_speed: f32,
    ) -> Vec2;

    /// Match average velocity of neighbors
    fn alignment(
        neighbors: &[(Vec2, Vec2)],  // (pos, velocity)
        max_speed: f32,
    ) -> Vec2;
}
```

### NavigationGrid

```rust
pub struct NavigationGrid { /* ... */ }

impl NavigationGrid {
    fn new(world_width: f32, world_height: f32, cell_size: f32) -> Self;
    fn world_to_grid(&self, world_x: f32, world_y: f32) -> GridCoord;
    fn grid_to_world(&self, coord: GridCoord) -> (f32, f32);
    fn is_walkable(&self, coord: GridCoord) -> bool;
    fn get(&self, coord: GridCoord) -> CellState;
    fn set(&mut self, coord: GridCoord, state: CellState);   // CellState: Walkable | Blocked | Difficult(cost)
    fn block_rect(&mut self, min_x: f32, min_y: f32, max_x: f32, max_y: f32);  // World-space corners
    fn line_of_sight(&self, from: GridCoord, to: GridCoord) -> bool;
}
```

### Pathfinder

```rust
pub struct Pathfinder { /* ... */ }

impl Pathfinder {
    fn new() -> Self;
    fn with_config(config: PathfinderConfig) -> Self;
    fn find_path(&self, grid: &NavigationGrid, start_x: f32, start_y: f32, goal_x: f32, goal_y: f32) -> PathResult;
    fn start_pathfinding(&mut self, grid: &NavigationGrid, start_x: f32, start_y: f32, goal_x: f32, goal_y: f32) -> (u64, PathResult);  // For time-slicing
    fn continue_pathfinding(&mut self, grid: &NavigationGrid, request_id: u64) -> PathResult;
}

pub enum PathResult {
    Found(Path),
    NoPath,
    InvalidEndpoints,
    InProgress,
    TooExpensive,
}
```

---

## Math Types (from glam)

```rust
pub struct Vec2 { pub x: f32, pub y: f32 }
pub struct Vec3 { pub x: f32, pub y: f32, pub z: f32 }
pub struct Vec4 { pub x: f32, pub y: f32, pub z: f32, pub w: f32 }
pub struct Mat4 { /* 4x4 matrix */ }
```

Common operations:
```rust
let v = Vec2::new(1.0, 2.0);
let v = Vec2::ZERO;
let v = Vec2::ONE;
let len = v.length();
let n = v.normalize();
let d = v1.dot(v2);
let v = v1 + v2;
let v = v1 * 2.0;
```

---

## Particle System

### ParticleEmitter

```rust
pub struct ParticleEmitter {
    pub mode: EmissionMode,
    pub shape: EmissionShape,
    pub sprite: SpriteHandle,
    pub enabled: bool,
    pub lifetime_min: f32,
    pub lifetime_max: f32,
    pub size_min: f32,
    pub size_max: f32,
    pub size_end_min: f32,
    pub size_end_max: f32,
    pub color_start: Color,
    pub color_end: Color,
    pub speed_min: f32,
    pub speed_max: f32,
    pub rotation_min: f32,
    pub rotation_max: f32,
    pub angular_velocity_min: f32,
    pub angular_velocity_max: f32,
    pub gravity: (f32, f32),
    pub drag: f32,
    pub z_order: i32,
    pub effect: Option<SpriteEffect>,
    pub collision: Option<CollisionConfig>,
    pub layer: u32,                 // Attractor layer (matched against ParticleAttractor::layer_mask)
    pub max_particles: u32,
}

impl ParticleEmitter {
    // Constructors
    fn new(sprite: SpriteHandle) -> Self;

    // Presets
    fn fire(sprite: SpriteHandle) -> Self;
    fn flame_small(sprite: SpriteHandle) -> Self;
    fn smoke(sprite: SpriteHandle) -> Self;
    fn smoke_thick(sprite: SpriteHandle) -> Self;
    fn explosion(sprite: SpriteHandle, count: u32) -> Self;
    fn explosion_debris(sprite: SpriteHandle, count: u32) -> Self;
    fn magic(sprite: SpriteHandle) -> Self;
    fn magic_heal(sprite: SpriteHandle) -> Self;
    fn rain(sprite: SpriteHandle) -> Self;
    fn snow(sprite: SpriteHandle) -> Self;
    fn sparks(sprite: SpriteHandle) -> Self;
    fn dust(sprite: SpriteHandle) -> Self;
    fn confetti(sprite: SpriteHandle) -> Self;
    fn bubbles(sprite: SpriteHandle) -> Self;
    fn comet_trail(sprite: SpriteHandle) -> Self;
    fn exhaust(sprite: SpriteHandle) -> Self;

    // Builder methods
    fn with_mode(self, mode: EmissionMode) -> Self;
    fn with_shape(self, shape: EmissionShape) -> Self;
    fn with_lifetime(self, min: f32, max: f32) -> Self;
    fn with_size(self, min: f32, max: f32) -> Self;
    fn with_end_size(self, min: f32, max: f32) -> Self;
    fn with_color(self, start: Color, end: Color) -> Self;
    fn with_speed(self, min: f32, max: f32) -> Self;
    fn with_rotation(self, min: f32, max: f32) -> Self;
    fn with_angular_velocity(self, min: f32, max: f32) -> Self;
    fn with_gravity(self, x: f32, y: f32) -> Self;
    fn with_drag(self, drag: f32) -> Self;
    fn with_z_order(self, z: i32) -> Self;
    fn with_effect(self, effect: SpriteEffect) -> Self;
    fn with_layer(self, layer: u32) -> Self;
    fn with_collision(self, config: CollisionConfig) -> Self;
    fn with_max_particles(self, max: u32) -> Self;
    fn one_shot(self) -> Self;

    // Runtime control
    fn set_enabled(&mut self, enabled: bool);
    fn reset(&mut self);
    fn burst(&mut self, count: u32);
    fn is_finished(&self) -> bool;
}
```

### EmissionMode

```rust
pub enum EmissionMode {
    Continuous { rate: f32 },                          // Particles per second
    Burst { count: u32 },                              // Single burst
    BurstRepeating { count: u32, interval: f32, repeats: Option<u32> },
}
```

### EmissionShape

```rust
pub enum EmissionShape {
    Point,
    Line { start_x: f32, start_y: f32, end_x: f32, end_y: f32 },
    Circle { radius: f32, hollow: bool },
    Rectangle { width: f32, height: f32 },
    Cone { direction: f32, spread: f32, min_speed: f32, max_speed: f32 },
}
```

### ParticleAttractor

```rust
pub struct ParticleAttractor {
    pub attractor_type: AttractorType,
    pub layer_mask: u32,
    pub enabled: bool,
}

impl ParticleAttractor {
    fn new(attractor_type: AttractorType) -> Self;
    fn point(strength: f32, radius: f32) -> Self;
    fn gravity_well(strength: f32, radius: f32) -> Self;
    fn line(start_x: f32, start_y: f32, end_x: f32, end_y: f32, strength: f32, radius: f32) -> Self;
    fn area(width: f32, height: f32, force_x: f32, force_y: f32) -> Self;
    fn vortex(strength: f32, radius: f32) -> Self;
    fn spiral(strength: f32, radius: f32, inward_pull: f32) -> Self;
    fn turbulence(strength: f32, scale: f32, speed: f32) -> Self;
    fn with_layer_mask(self, mask: u32) -> Self;
    fn with_enabled(self, enabled: bool) -> Self;
    fn calculate_force(&self, attractor_x: f32, attractor_y: f32, particle_x: f32, particle_y: f32, particle_layer: u32) -> (f32, f32);
}
```

### AttractorType

```rust
pub enum AttractorType {
    Point { strength: f32, radius: f32, falloff: f32 },
    Line { start_x: f32, start_y: f32, end_x: f32, end_y: f32, strength: f32, radius: f32 },
    Area { width: f32, height: f32, force_x: f32, force_y: f32 },
    Vortex { strength: f32, radius: f32, inward_pull: f32 },
    Turbulence { strength: f32, scale: f32, speed: f32 },
}
```

### CollisionConfig

```rust
// Particles collide with Transform + Collider entities
// (triggers and the particle's own emitter entity are ignored)
pub struct CollisionConfig {
    pub radius: f32,                  // particle circle radius
    pub collision_mask: u32,          // matched against Collider::layer (default u32::MAX)
    pub response: CollisionResponse,  // default Die
    pub send_events: bool,            // default false; set by with_events()
}

impl CollisionConfig {
    fn new(radius: f32) -> Self;      // Die, all layers, no events
    fn die_on_hit(radius: f32) -> Self;
    fn bouncy(radius: f32, restitution: f32) -> Self;
    fn sticky(radius: f32) -> Self;
    fn with_mask(self, mask: u32) -> Self;
    fn with_response(self, response: CollisionResponse) -> Self;
    fn with_events(self) -> Self;     // emit ParticleCollision on impact for any response
}

// Minimum impact speed along the normal (px/s) for a bounce event; in ryo2d::prelude
pub const MIN_BOUNCE_EVENT_SPEED: f32 = 20.0;

pub enum CollisionResponse {
    Die,                                 // default; removed on contact
    Bounce { restitution_percent: u8 },  // build with CollisionResponse::bounce(0.7)
    Stick,                               // frozen until its lifetime ends
    Event,                               // passes through; always emits on not-touching -> touching
}

impl CollisionResponse {
    fn bounce(restitution: f32) -> Self;
    fn restitution(&self) -> f32;
}
```

### ParticleCollision Event

```rust
// Event response: always. Die/Stick: once, with with_events().
// Bounce: on not-touching -> touching at >= MIN_BOUNCE_EVENT_SPEED, with with_events().
// Touching is per particle: moving straight into an overlapping/adjacent collider doesn't re-notify.
pub struct ParticleCollision {
    pub emitter_entity: Entity,
    pub collider_entity: Entity,
    pub position_x: f32,   // particle position at contact
    pub position_y: f32,
    pub normal_x: f32,     // unit surface normal, pointing at the particle
    pub normal_y: f32,
    pub velocity_x: f32,   // velocity before the response
    pub velocity_y: f32,
}
```

### Particle

Per-particle state (managed by `ParticleSystem`; you rarely touch it directly):

```rust
pub struct Particle {
    pub lifetime: f32,
    pub max_lifetime: f32,
    pub velocity_x: f32,
    pub velocity_y: f32,
    pub angular_velocity: f32,
    pub rotation: f32,
    pub size: f32,
    pub start_size: f32,
    pub end_size: f32,
    pub color: Color,
    pub start_color: Color,
    pub end_color: Color,
    pub mass: f32,
    pub drag: f32,
    pub gravity_scale: f32,
    pub has_trail: bool,
    pub collision_layer: u32,
    pub frame: u32,
}

impl Particle {
    fn lifetime_ratio(&self) -> f32;
    fn is_dead(&self) -> bool;
}
```

`ParticleSystem::new()` / `ParticleSystem::with_seed(seed: u64)` (reproducible particles) drive emitters; also `particle_count()`, `clear()`, `clear_emitter(entity)`.

---

## Testing (`ryo2d::testing`)

| Item | Signature / purpose |
|------|---------------------|
| `HeadlessEngine::new(builder)` | `-> Result<HeadlessEngine, HeadlessError>` — engine without a window |
| `HeadlessEngine::new_or_skip(builder)` | `-> Option<HeadlessEngine>` — `None` when no GPU (tests `return` early) |
| `step()` / `step_frames(n)` | `-> Result<StepOutcome, HeadlessError>` — one fixed update + render per step |
| `frame()` | `-> Result<RgbaImage, HeadlessError>` — last frame at game resolution |
| `frame_count()` | presented frames |
| `world()` / `world_mut()` | ECS world between steps |
| `press_key` / `release_key` / `tap_key` | keyboard input (`KeyCode`) |
| `press_mouse` / `release_mouse` / `mouse_move(x, y)` | mouse input (game coordinates) |
| `StepOutcome` | `Running` \| `Quit` |
| `HeadlessError` | `NoGpu` \| `Render(String)` \| `Capture(CaptureError)` |
