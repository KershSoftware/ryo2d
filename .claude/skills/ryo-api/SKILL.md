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
    fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction;
    fn fixed_update(&mut self, ctx: &mut SceneContext) {}
    fn render_ui(&mut self, ui: &mut UiContext) -> SceneAction { SceneAction::Continue }
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
| `time` | `TimeInfo` | Timing values (fixed dt, scaled dt, etc.) |

### UiContext

Available in `render_ui`:

```rust
impl UiContext {
    // Text
    fn label(&mut self, text: &str, x: f32, y: f32, color: Color, bg: Option<Color>);
    fn label_shadow(&mut self, text: &str, x: f32, y: f32, color: Color, shadow: Color);
    
    // Shapes
    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: Color);
    fn rect_outline(&mut self, x: f32, y: f32, w: f32, h: f32, color: Color, thickness: f32);
    
    // Layout helpers
    fn screen_size(&self) -> (u32, u32);  // Returns (width, height) as u32
}

// Usage example:
let (sw, sh) = ui.screen_size();
let screen_w = sw as f32;  // Cast to f32 for positioning
let screen_h = sh as f32;
```

---

## Engine Builder

**Note:** Resolution, FPS, and other settings come from `Ryo.toml` by default.
Only use these methods to override the config file.

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
    fn all() -> &'static [DisplayMode];      // All variants for settings menus
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
    fn new(width: u32, height: u32) -> Self;

    // Common 16:9 resolutions
    fn common() -> Vec<Resolution>;  // Returns [1280x720, 1600x900, 1920x1080, 2560x1440, 3840x2160]
}
```

### WindowRequest

Request display changes at runtime (applied next frame):

```rust
// In SceneContext
impl SceneContext {
    fn request_display_mode(&mut self, mode: DisplayMode);
    fn request_resolution(&mut self, width: u32, height: u32);
}
```

**Usage:**
```rust
// Settings menu - change display mode
fn apply_settings(&mut self, ctx: &mut SceneContext) {
    ctx.request_display_mode(DisplayMode::BorderlessFullscreen);
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
    pub pivot_x: f32,   // Pivot X (0.0=left, 0.5=center, 1.0=right), defaults to 0.5
    pub pivot_y: f32,   // Pivot Y (0.0=top, 0.5=center, 1.0=bottom), defaults to 0.5
}

impl Transform {
    fn new(x: f32, y: f32) -> Self;              // z defaults to 0.0, pivot defaults to center
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
    fn with_pivot_center(self) -> Self;          // Pivot at (0.5, 0.5) - centered rotation
    fn with_pivot_top_center(self) -> Self;      // Pivot at (0.5, 0.0) - pendulums
    fn with_pivot_bottom_center(self) -> Self;   // Pivot at (0.5, 1.0) - standing objects
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
    pub z_order: i32,
    pub visible: bool,
    pub flip_x: bool,
    pub flip_y: bool,
    pub tint: Option<Color>,
}

impl Renderable {
    fn sprite(handle: SpriteHandle) -> Self;
    fn with_z_order(self, z: i32) -> Self;
    fn with_flip_x(self, flip: bool) -> Self;
    fn with_flip_y(self, flip: bool) -> Self;
    fn with_tint(self, color: Color) -> Self;
    fn hidden(self) -> Self;
}
```

### RenderLayer

```rust
pub enum RenderLayer {
    Background,  // z_order: -1000
    Default,     // z_order: 0
    Foreground,  // z_order: 1000
    UI,          // z_order: 10000
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
    pub body_type: RigidBodyType,
    pub gravity_scale: f32,
    pub linear_damping: f32,
    pub angular_damping: f32,
    pub fixed_rotation: bool,
}

impl RigidBody {
    fn dynamic() -> Self;      // Affected by forces
    fn kinematic() -> Self;    // Moved by code, affects others
    fn fixed() -> Self;        // Static, immovable
}
```

### Collider

```rust
pub struct Collider {
    pub shape: ColliderShape,
    pub is_sensor: bool,
    pub friction: f32,
    pub restitution: f32,
}

impl Collider {
    fn circle(radius: f32) -> Self;
    fn rect(width: f32, height: f32) -> Self;
    fn capsule(half_height: f32, radius: f32) -> Self;
    fn sensor(self) -> Self;  // Triggers events but no physics
    fn with_friction(self, friction: f32) -> Self;
    fn with_restitution(self, bounce: f32) -> Self;
}
```

### ColliderShape

```rust
pub enum ColliderShape {
    Circle { radius: f32 },
    Rect { width: f32, height: f32 },
    Capsule { half_height: f32, radius: f32 },
}
```

### PhysicsCollisionEvent

```rust
pub struct PhysicsCollisionEvent {
    pub entity_a: Entity,
    pub entity_b: Entity,
    pub event_type: CollisionEventType,
}

impl PhysicsCollisionEvent {
    fn involves(&self, entity: Entity) -> bool;
    fn other_entity(&self, entity: Entity) -> Entity;
}

pub enum CollisionEventType {
    Started,
    Stopped,
}
```

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
    
    // Load from file
    fn load<P: AsRef<Path>>(path: P) -> SpriteResult<Self>;
    fn load_svg<P: AsRef<Path>>(path: P, scale: f32) -> SpriteResult<Self>;
    fn load_svg_fit<P: AsRef<Path>>(path: P, max_width: u32, max_height: u32) -> SpriteResult<Self>;
    
    // Properties
    fn width(&self) -> u32;
    fn height(&self) -> u32;
    fn data(&self) -> &[u8];
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
}
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
    fn is_failed(&self) -> bool;
    fn get(&self) -> Option<SpriteHandle>;
    fn get_or(&self, fallback: &SpriteHandle) -> SpriteHandle;
}

pub enum LoadingState {
    Loading,
    Loaded(SpriteHandle),
    Failed(String),
}
```

---

## Animation

### Animator

```rust
pub struct Animator {
    pub animations: HashMap<AnimationId, Animation>,
    pub current: Option<AnimationId>,
    pub frame: usize,
    pub timer: f32,
    pub state: PlaybackState,
}

impl Animator {
    fn new() -> Self;
    fn add(self, id: impl Into<AnimationId>, animation: Animation) -> Self;
    fn with_default(self, id: impl Into<AnimationId>) -> Self;
    fn play(&mut self, id: impl Into<AnimationId>);
    fn stop(&mut self);
    fn pause(&mut self);
    fn resume(&mut self);
    fn current_frame(&self) -> Option<SpriteHandle>;
}
```

### Animation

```rust
pub struct Animation {
    pub frames: Vec<SpriteHandle>,
    pub frame_duration: f32,
    pub loop_mode: LoopMode,
}

impl Animation {
    fn new(frames: Vec<SpriteHandle>, fps: f32) -> Self;
    fn with_loop_mode(self, mode: LoopMode) -> Self;
}

pub enum LoopMode {
    Once,
    Loop,
    PingPong,
}

pub enum PlaybackState {
    Playing,
    Paused,
    Stopped,
    Finished,
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

    pub duration: f32,      // Seconds
    pub delay: f32,         // Seconds before starting
    pub easing: Ease,       // Easing function
    pub loop_mode: TweenLoop,
    pub playing: bool,
    pub just_finished: bool,  // True for one frame on completion
}

impl Tween {
    // Constructors
    fn new(duration: f32) -> Self;
    fn to_position(x: f32, y: f32, duration: f32) -> Self;
    fn to_rotation(rotation: f32, duration: f32) -> Self;
    fn to_scale(scale_x: f32, scale_y: f32, duration: f32) -> Self;
    fn to_uniform_scale(scale: f32, duration: f32) -> Self;

    // Builder methods
    fn to_x(self, x: f32) -> Self;
    fn to_y(self, y: f32) -> Self;
    fn to_rotation_value(self, rotation: f32) -> Self;
    fn to_scale_x(self, scale_x: f32) -> Self;
    fn to_scale_y(self, scale_y: f32) -> Self;
    fn with_easing(self, easing: Ease) -> Self;
    fn with_delay(self, delay: f32) -> Self;
    fn with_loop(self, loop_mode: TweenLoop) -> Self;

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

### AnimationController

State machine for complex animation logic:

```rust
pub struct AnimationController {
    states: HashMap<String, AnimationState>,
    transitions: Vec<Transition>,
    current_state: String,
}

impl AnimationController {
    fn new(initial_state: &str) -> Self;
    fn add_state(self, name: &str, animation: Animation) -> Self;
    fn add_transition(self, from: &str, to: &str, condition: TransitionCondition) -> Self;
    fn set_trigger(&mut self, trigger: &str);
    fn set_bool(&mut self, name: &str, value: bool);
    fn set_float(&mut self, name: &str, value: f32);
}
```

---

## Audio

### AudioManager

```rust
impl AudioManager {
    // Sound effects (short, can overlap)
    fn play_sound<P: AsRef<Path>>(&mut self, path: P) -> AudioResult<SoundHandle>;
    fn play_sound_with<P: AsRef<Path>>(&mut self, path: P, options: SoundOptions) -> AudioResult<SoundHandle>;
    
    // Music (long, one at a time per track)
    fn play_music<P: AsRef<Path>>(&mut self, path: P) -> AudioResult<MusicHandle>;
    fn play_music_with<P: AsRef<Path>>(&mut self, path: P, options: MusicOptions) -> AudioResult<MusicHandle>;
    
    // Volume control
    fn set_master_volume(&mut self, volume: f32);
    fn set_music_volume(&mut self, volume: f32);
    fn set_sfx_volume(&mut self, volume: f32);
    fn master_volume(&self) -> f32;
    fn music_volume(&self) -> f32;
    fn sfx_volume(&self) -> f32;
    
    // Music control
    fn stop_music(&mut self);
    fn pause_music(&mut self);
    fn resume_music(&mut self);
}
```

### SoundOptions / MusicOptions

```rust
pub struct SoundOptions {
    pub volume: f32,      // 0.0 - 1.0
    pub pitch: f32,       // 1.0 = normal
    pub panning: f32,     // -1.0 left, 0.0 center, 1.0 right
}

pub struct MusicOptions {
    pub volume: f32,
    pub looping: bool,
    pub fade_in: Option<f32>,  // Seconds
}
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
    fn scroll_delta(&self) -> f32;                // Mouse wheel
}
```

### KeyCode (from winit)

Common keys:
- Letters: `KeyCode::KeyA` through `KeyCode::KeyZ`
- Numbers: `KeyCode::Digit0` through `KeyCode::Digit9`
- Arrows: `KeyCode::ArrowUp`, `ArrowDown`, `ArrowLeft`, `ArrowRight`
- Special: `KeyCode::Space`, `Escape`, `Enter`, `Tab`, `Backspace`
- Modifiers: `KeyCode::ShiftLeft`, `ControlLeft`, `AltLeft`

### MouseButton

```rust
pub enum MouseButton {
    Left,
    Right,
    Middle,
}
```

---

## Camera

**CRITICAL:** Camera defaults to position (0, 0), which centers the screen on world origin.
For screen-style coordinates (0,0 = top-left), center the camera at half the viewport size.

```rust
impl Camera {
    fn new(viewport_width: u32, viewport_height: u32) -> Self;
    fn centered_at(x: f32, y: f32, viewport_width: u32, viewport_height: u32) -> Self;
    
    fn set_position(&mut self, x: f32, y: f32);
    fn position(&self) -> (f32, f32);
    fn set_zoom(&mut self, zoom: f32);
    fn zoom(&self) -> f32;
    fn viewport_size(&self) -> (f32, f32);        // (width, height)
    
    fn screen_to_world(&self, screen_x: f32, screen_y: f32) -> (f32, f32);
    fn world_to_screen(&self, world_x: f32, world_y: f32) -> (f32, f32);
    
    fn visible_bounds(&self) -> CameraBounds;     // World-space AABB
    fn is_visible(&self, x: f32, y: f32, w: f32, h: f32) -> bool;
    
    fn follow(&mut self, target_x: f32, target_y: f32, lerp_factor: f32);
    fn snap_to(&mut self, target_x: f32, target_y: f32);
    fn clamp_to_bounds(&mut self, min_x: f32, min_y: f32, max_x: f32, max_y: f32);
    fn reset(&mut self);                                      // Reset position/zoom/shake, keep viewport
}

// Screen-style coordinates setup (0,0 = top-left):
let (vw, vh) = ctx.camera.viewport_size();
ctx.camera.set_position(vw / 2.0, vh / 2.0);
// Now world (0, 0) = screen top-left
// world (vw, vh) = screen bottom-right
```

---

## World (ECS)

```rust
impl World {
    // Spawning
    fn spawn<C: ComponentBundle>(&mut self, components: C) -> Entity;
    fn despawn(&mut self, entity: Entity) -> bool;
    
    // Component access
    fn get_component<T: Component>(&self, entity: Entity) -> Option<&T>;
    fn get_component_mut<T: Component>(&mut self, entity: Entity) -> Option<&mut T>;
    fn has_component<T: Component>(&self, entity: Entity) -> bool;
    fn add_component<T: Component>(&mut self, entity: Entity, component: T);
    fn remove_component<T: Component>(&mut self, entity: Entity);
    
    // Querying
    fn query<Q: Query>(&self) -> QueryIter<Q>;
    fn query_mut<Q: Query>(&mut self) -> QueryIterMut<Q>;
    
    // Entity checks
    fn is_alive(&self, entity: Entity) -> bool;
    fn entity_count(&self) -> usize;
}
```

### Query Examples

**IMPORTANT:** Query iteration returns components directly, NOT `(entity, components)`.
The query type must match the destructuring pattern exactly.

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

// To get entity with components, use get_component on a stored entity:
if let Some(transform) = ctx.world.get_component::<Transform>(self.player_entity) {
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
    Fade { duration: f32, color: Color },
    Slide { duration: f32, direction: SlideDirection },
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
    
    fn new(r: u8, g: u8, b: u8, a: u8) -> Self;
    fn rgb(r: u8, g: u8, b: u8) -> Self;
    fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self;
    fn from_hex(hex: u32) -> Self;
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
    fn receive(&mut self, client_id: ClientId, channel: Channel) -> Option<Vec<u8>>;
    fn clients(&self) -> impl Iterator<Item = ClientId>;
    fn disconnect_client(&mut self, client_id: ClientId);
}

// Player-hosted server (host plays while serving)
impl ListenServer {
    fn new(config: NetworkServerConfig) -> Result<Self, NetworkError>;
    fn update(&mut self, dt: Duration);
    fn events(&mut self) -> impl Iterator<Item = NetworkEvent>;
    fn send(&mut self, client_id: ClientId, channel: Channel, data: &[u8]);
    fn broadcast(&mut self, channel: Channel, data: &[u8]);
    fn receive(&mut self, client_id: ClientId, channel: Channel) -> Option<Vec<u8>>;
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
    fn disconnect(&mut self);
}
```

### Configuration

```rust
pub struct NetworkServerConfig {
    pub port: u16,
    pub max_clients: usize,
    pub protocol_id: u64,
}

pub struct NetworkClientConfig {
    pub server_addr: SocketAddr,
    pub protocol_id: u64,
}

pub enum Channel {
    Unreliable,        // Best-effort, no ordering
    ReliableOrdered,   // Guaranteed delivery + order
}

pub type ClientId = u64;
```

### Events

```rust
pub enum NetworkEvent {
    Connected,
    Disconnected { reason: DisconnectReason },
    ClientConnected { client_id: ClientId },
    ClientDisconnected { client_id: ClientId, reason: DisconnectReason },
}

pub enum DisconnectReason {
    Requested,
    Timeout,
    Kicked,
    ServerFull,
    ProtocolMismatch,
}
```

### Replication

```rust
impl ReplicationManager {
    fn new_server() -> Self;
    fn new_client() -> Self;
    fn register<T: Replicated>(&mut self);
    fn spawn_networked(&mut self, world: &mut World, builder: NetworkEntityBuilder) -> Entity;
    fn despawn_networked(&mut self, world: &mut World, entity: Entity);
    fn mark_dirty<T: Replicated>(&mut self, entity: Entity);
    fn broadcast_updates(&mut self, world: &World, server: &mut impl ServerLike);
    fn send_full_state(&mut self, world: &World, server: &mut impl ServerLike, client: ClientId);
    fn process_incoming(&mut self, world: &mut World, client: &mut NetworkClient);
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
    fn from_rpc<T: Rpc>(rpc: &T, sender: ClientId, target: RpcTarget) -> Result<Self, NetworkError>;
    fn from_bytes(data: &[u8]) -> Result<Self, NetworkError>;
    fn to_bytes(&self) -> Result<Vec<u8>, NetworkError>;
    fn into_rpc<T: Rpc>(self) -> Result<ReceivedRpc<T>, NetworkError>;
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
}
```

---

## Navigation & Pathfinding

### NavAgent

```rust
pub struct NavAgent {
    pub current_path: Option<Path>,
    pub state: NavAgentState,
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
    fn has_path(&self) -> bool;
    fn destination(&self) -> Option<(f32, f32)>;
    fn set_path(&mut self, path: Path);
    fn stop(&mut self);
    fn arrive(&mut self);
    fn fail(&mut self);
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
    pub follow_entity: Option<Entity>,
}

impl NavTarget {
    fn position(x: f32, y: f32) -> Self;
    fn follow(entity: Entity) -> Self;
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
    fn new(width: f32, height: f32, cell_size: f32) -> Self;
    fn world_to_grid(&self, x: f32, y: f32) -> GridCoord;
    fn grid_to_world(&self, coord: GridCoord) -> (f32, f32);
    fn is_walkable(&self, coord: GridCoord) -> bool;
    fn set(&mut self, coord: GridCoord, state: CellState);
    fn block_rect(&mut self, x: f32, y: f32, w: f32, h: f32);
    fn line_of_sight(&self, from: GridCoord, to: GridCoord) -> bool;
}
```

### Pathfinder

```rust
pub struct Pathfinder { /* ... */ }

impl Pathfinder {
    fn new() -> Self;
    fn with_config(config: PathfinderConfig) -> Self;
    fn find_path(&mut self, grid: &NavigationGrid, x1: f32, y1: f32, x2: f32, y2: f32) -> PathResult;
    fn start_pathfinding(...) -> (u64, PathResult);  // For time-slicing
    fn continue_pathfinding(&mut self, grid: &NavigationGrid, request_id: u64) -> PathResult;
}

pub enum PathResult {
    Found(Path),
    NoPath,
    InvalidEndpoints,
    InProgress,
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
    pub trail: Option<TrailConfig>,
    pub collision: Option<CollisionConfig>,
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
    fn with_trail(self, config: TrailConfig) -> Self;
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
pub struct CollisionConfig {
    pub radius: f32,
    pub collision_mask: u32,
    pub response: CollisionResponse,
}

impl CollisionConfig {
    fn new(radius: f32, response: CollisionResponse) -> Self;
    fn bouncy(radius: f32, restitution: f32) -> Self;
    fn sticky(radius: f32) -> Self;
    fn deadly(radius: f32) -> Self;
    fn event(radius: f32) -> Self;
    fn with_mask(self, mask: u32) -> Self;
}

pub enum CollisionResponse {
    Die,
    Bounce { restitution: f32 },
    Stick,
    Event,
}
```

### TrailConfig (for particles)

```rust
pub struct TrailConfig {
    pub color: Option<Color>,
    pub width: f32,
    pub end_width: f32,
    pub max_points: usize,
    pub min_distance: f32,
    pub fade: bool,
}

impl TrailConfig {
    fn new(width: f32) -> Self;
    fn with_color(self, color: Color) -> Self;
    fn with_end_width(self, end_width: f32) -> Self;
    fn with_max_points(self, max_points: usize) -> Self;
    fn with_min_distance(self, min_distance: f32) -> Self;
    fn with_fade(self, fade: bool) -> Self;
}
```

### ParticleCollision Event

```rust
pub struct ParticleCollision {
    pub particle_entity: Entity,
    pub collider_entity: Entity,
    pub position: (f32, f32),
    pub normal: (f32, f32),
}
```

### Particle

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
    pub drag: f32,
    pub emitter_entity: Entity,
}

impl Particle {
    fn lifetime_ratio(&self) -> f32;
    fn is_dead(&self) -> bool;
}
```
