---
name: ryo-game-dev
description: Building games with Ryo2D engine - ECS, entities, scenes, input.
metadata:
  version: 1.0.0
---

# Ryo2D Game Developer

You help users build complete games using the Ryo2D engine. You understand the ECS architecture, the Scene trait lifecycle, and common game patterns.

**For complete API signatures, see the `ryo-api` skill.**

## Dev Build Performance

If dev builds run slowly (physics, particles, etc.), ensure `Cargo.toml` has:

```toml
[profile.dev.package."*"]
opt-level = 2
```

This optimizes dependencies while keeping your code fast to compile. New projects include this automatically; `ryo2d dev` warns if missing.

## Quick Reference

```rust
use ryo2d::prelude::*;

struct MyScene {
    player: Option<Entity>,
    score: u32,
}

impl Scene for MyScene {
    fn on_enter(&mut self, ctx: &mut SceneContext) {
        // IMPORTANT: Center camera so world coords match screen coords (0,0 = top-left)
        // Resolution comes from Ryo.toml, get it from camera viewport
        let (vw, vh) = ctx.camera.viewport_size();
        ctx.camera.set_position(vw / 2.0, vh / 2.0);

        // Load a sprite from assets folder
        let sprite = ctx.assets.load_sprite("player.png").unwrap();

        // Or create a colored rectangle
        let sprite = ctx.assets.create_sprite(
            Sprite::solid_rect(32, 32, 255, 0, 0, 255),
            "red-square"
        );

        // Spawn player entity at screen center (640, 360 for 1280x720)
        self.player = Some(ctx.world.spawn((
            Transform::new(vw / 2.0, vh / 2.0),
            Velocity::zero(),
            Renderable::sprite(sprite).with_z_order(10),
        )));
    }

    fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
        // Handle input, update game state
        if ctx.input.key_pressed(KeyCode::Escape) {
            return SceneAction::Quit;
        }
        SceneAction::Continue
    }

    fn render_ui(&mut self, ui: &mut UiContext) -> SceneAction {
        ui.label(&format!("Score: {}", self.score), 10.0, 10.0, Color::WHITE, None);
        SceneAction::Continue
    }
}

fn main() {
    // Resolution/FPS come from Ryo.toml - don't set .resolution() here!
    EngineBuilder::new()
        .title("My Game")
        .start_scene(MyScene { player: None, score: 0 })
        .build()
        .run()
        .unwrap();
}
```

## SceneContext Fields

| Field | Type | Purpose |
|-------|------|---------|
| `ctx.resources` | `&mut Resources` | Cross-scene shared state |
| `ctx.world` | `&mut World` | ECS world - spawn/query entities |
| `ctx.camera` | `&mut Camera` | Camera position, zoom, viewport |
| `ctx.input` | `&mut InputState` | Keyboard/mouse state |
| `ctx.assets` | `&mut AssetManager` | Load sprites, sounds |
| `ctx.audio` | `&mut Option<AudioManager>` | Play sounds/music |
| `ctx.effects` | `&mut EffectsManager` | Post-processing effects |
| `ctx.events` | `&mut Events` | Event bus for decoupled communication |

## Essential Patterns

### Loading Assets

```rust
// From file (relative to assets/ folder)
let sprite = ctx.assets.load_sprite("player.png").unwrap();
let sprite = ctx.assets.load_sprite("sprites/enemy.png").unwrap();

// Programmatic sprite
let bullet = ctx.assets.create_sprite(
    Sprite::solid_circle(8, 255, 255, 0, 255),  // Yellow circle
    "bullet"
);
```

### Spawning Entities

```rust
// Basic entity
let entity = ctx.world.spawn((
    Transform::new(x, y),
    Renderable::sprite(sprite_handle),
));

// With physics
let entity = ctx.world.spawn((
    Transform::new(x, y),
    Velocity::zero(),
    Renderable::sprite(sprite_handle),
    RigidBody::dynamic(),
    Collider::circle(16.0),
));

// Store for later reference
self.player = Some(entity);
```

### Input

```rust
// Keyboard
if ctx.input.key_pressed(KeyCode::Space) { /* just pressed */ }
if ctx.input.key_down(KeyCode::KeyW) { /* held down */ }
if ctx.input.key_released(KeyCode::Escape) { /* just released */ }

// Mouse
if ctx.input.mouse_pressed(MouseButton::Left) { /* clicked */ }
let (mx, my) = ctx.input.mouse_position();
let (wx, wy) = ctx.camera.screen_to_world(mx, my);
```

### Querying & Modifying Entities

```rust
// Get component from a stored entity
if let Some(transform) = ctx.world.get_component::<Transform>(entity) {
    let x = transform.x;  // Direct field access (not .position.x!)
    let y = transform.y;
}

// Modify component
if let Some(mut vel) = ctx.world.get_component_mut::<Velocity>(entity) {
    vel.dx = 100.0;  // Fields are dx, dy, dz (not x, y, z!)
    vel.dy = -50.0;
}

// Query all entities with components
// IMPORTANT: Returns flat tuple of components, NOT (entity, components)
for (transform, velocity) in ctx.world.query::<(&Transform, &Velocity)>().iter() {
    // Use transform.x, velocity.dx etc directly
}

// Query with mutable access
for (transform, velocity) in ctx.world.query::<(&Transform, &mut Velocity)>().iter() {
    velocity.dx = transform.x * 0.1;
}

// Query with optional
for (t, maybe_v) in ctx.world.query::<(&Transform, Option<&Velocity>)>().iter() {
    if let Some(vel) = maybe_v { /* has velocity */ }
}

// Remove entity
ctx.world.despawn(entity);
```

### Collision Handling

```rust
// In update()
for event in ctx.collision_events.iter() {
    if event.event_type == CollisionEventType::Started {
        if event.involves(self.player.unwrap()) {
            let other = event.other_entity(self.player.unwrap());
            
            if ctx.world.has_component::<Enemy>(other) {
                self.take_damage();
            } else if ctx.world.has_component::<Coin>(other) {
                self.score += 10;
                ctx.world.despawn(other);
            }
        }
    }
}
```

### Audio

```rust
// Sound effects
if let Some(audio) = ctx.audio.as_mut() {
    audio.play_sound("jump.wav").ok();
    audio.play_sound_with("explosion.wav", SoundOptions {
        volume: 0.8,
        pitch: 1.2,
        panning: 0.0,
    }).ok();
}

// Music
if let Some(audio) = ctx.audio.as_mut() {
    audio.play_music("bgm.ogg").ok();
    audio.set_music_volume(0.5);
}
```

### Camera

```rust
// Follow player
if let Some(t) = ctx.world.get_component::<Transform>(self.player.unwrap()) {
    ctx.camera.set_position(t.x, t.y);  // Direct x, y fields
}

// Zoom
ctx.camera.set_zoom(2.0);  // 2x zoom

// Convert coordinates
let (world_x, world_y) = ctx.camera.screen_to_world(screen_x, screen_y);
```

## Scene System

```rust
struct MenuScene;

impl Scene for MenuScene {
    fn on_enter(&mut self, ctx: &mut SceneContext) {
        // Spawn entities, configure camera, load sounds
        // (Sprite/asset loading can go in new() instead)
    }

    fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
        if ctx.input.key_pressed(KeyCode::Enter) {
            return SceneAction::Push(Box::new(GameScene::new()));
        }
        if ctx.input.key_pressed(KeyCode::Escape) {
            return SceneAction::Quit;
        }
        SceneAction::Continue
    }

    fn render_ui(&mut self, ui: &mut UiContext) -> SceneAction {
        // Get screen size (returns u32, cast to f32 for positioning)
        let (sw, sh) = ui.screen_size();
        let screen_w = sw as f32;
        let screen_h = sh as f32;
        
        ui.label("Press ENTER to start", screen_w / 2.0 - 80.0, screen_h / 2.0, Color::WHITE, None);
        SceneAction::Continue
    }
}

// SceneAction options:
// - Continue            Keep running
// - Push(scene)         Add scene on top (pause current)
// - Pop                 Return to previous scene
// - PopReturn(data)     Pop and return data to scene below (via on_resume)
// - Switch(scene)       Replace current scene (NOT "Replace"!)
// - Reset(scene)        Clear all scenes, push new one
// - Quit                Exit game
// All except Continue/Pop/Quit have "With" variants for transitions

// Automatic cleanup on Switch/Reset (no manual world.clear() needed!)
// Override cleanup_on_exit() to customize:
impl Scene for MyScene {
    fn cleanup_on_exit(&self) -> SceneCleanup {
        SceneCleanup::DEFAULT  // Resets world, camera, effects, lighting, time, events (not music)
    }
}
// SceneCleanup flags: NONE, WORLD, CAMERA, EFFECTS, LIGHTING, TIME, MUSIC, EVENTS, ALL, DEFAULT
// Combine with | and subtract with -
// Push/Pop do NOT run cleanup (paused scene keeps its state)
```

## EngineBuilder Options

```rust
// Minimal - uses Ryo.toml for resolution, fps, etc.
EngineBuilder::new()
    .title("My Game")
    .start_scene(MenuScene::new())
    .build()
    .run()
    .unwrap();

// With shared resources
EngineBuilder::new()
    .title("My Game")
    .resource(GameSettings::default())  // Accessible via ctx.resources
    .start_scene(MenuScene::new())
    .build()
    .run()
    .unwrap();

// Override Ryo.toml (only if needed for special cases)
EngineBuilder::new()
    .title("My Game")
    .resolution(1920, 1080)           // Override resolution
    .target_fps(120)                  // Override FPS
    .physics_hz(240)                  // Physics update rate
    .window_mode(WindowMode::ResizableAspectRatio)
    .start_scene(GameScene::new())
    .build()
    .run()
    .unwrap();
```

**Note:** Prefer using `Ryo.toml` for configuration. EngineBuilder methods override the config file. A saved player preference in system.json overrides a builder call when `with_user_preferences` includes it.

## Fixed Design Resolution Pattern

For games where gameplay depends on consistent dimensions (platformers, bullet-hell, etc.), use a **fixed design resolution** with scalable display:

```rust
// In main.rs - set fixed game resolution, allow display scaling
EngineBuilder::new()
    .title("My Game")
    .resolution(1920, 1080)  // Fixed design resolution - game logic uses this
    .window_mode(WindowMode::ResizableAspectRatio)  // Window can scale
    .target_fps(144)
    .start_scene(MenuScene::new())
    .build()
    .run()
    .unwrap();
```

**Key concept:** The game's internal resolution stays fixed at 1920x1080 (or whatever you choose). The window can be resized or fullscreened, and the engine automatically scales/letterboxes to maintain aspect ratio.

### Design Resolution Constants

Store fixed dimensions as constants for use throughout your game:

```rust
// constants.rs
const DESIGN_WIDTH: f32 = 1920.0;
const DESIGN_HEIGHT: f32 = 1080.0;

// Use in game logic - these never change regardless of window size
fn spawn_player(&self, ctx: &mut SceneContext) {
    ctx.world.spawn((
        Transform::new(DESIGN_WIDTH / 2.0, DESIGN_HEIGHT - 100.0),
        // ...
    ));
}

fn clamp_to_screen(&self, transform: &mut Transform) {
    transform.x = transform.x.clamp(0.0, DESIGN_WIDTH - PLAYER_SIZE);
    transform.y = transform.y.clamp(0.0, DESIGN_HEIGHT - PLAYER_SIZE);
}
```

### Display Mode vs Design Resolution

| Concept | Purpose |
|---------|---------|
| **Design Resolution** | Fixed internal game coordinates (e.g., 1920x1080) |
| **Display Mode** | How the game appears on screen (Windowed/Exclusive Fullscreen/Borderless Fullscreen) |
| **Window Size** | Actual window dimensions (can scale independently) |

The player can change display mode without affecting game logic:

```rust
// Request display mode change (settings menu)
ctx.request_display_mode(DisplayMode::BorderlessFullscreen);

// Game logic remains unchanged - always uses design resolution
let ground_y = DESIGN_HEIGHT - GROUND_OFFSET;
```

### WindowMode Options

| Mode | Behavior |
|------|----------|
| `Fixed` | Window cannot be resized |
| `ResizableAspectRatio` | Scales with letterboxing (recommended) |
| `ResizableIntegerScale` | Pixel-perfect scaling only |
| `ResizableStretch` | Stretches to fill (may distort) |

## Coordinate System

**CRITICAL:** The camera defaults to position (0, 0), which puts the screen center at world origin.

To use screen-style coordinates (0, 0 = top-left):

```rust
fn on_enter(&mut self, ctx: &mut SceneContext) {
    // Center camera so world coords match screen coords
    let (vw, vh) = ctx.camera.viewport_size();
    ctx.camera.set_position(vw / 2.0, vh / 2.0);
    
    // Now world (0, 0) = screen top-left
    // world (1280, 720) = screen bottom-right (for 1280x720)
}
```

### Screen Bounds Helper

For clamping entities to screen, **sprite origin is TOP-LEFT**:

```rust
// In your Scene struct, store screen bounds
struct GameScene {
    screen_w: f32,
    screen_h: f32,
    // ...
}

fn on_enter(&mut self, ctx: &mut SceneContext) {
    let (vw, vh) = ctx.camera.viewport_size();
    self.screen_w = vw;
    self.screen_h = vh;
    ctx.camera.set_position(vw / 2.0, vh / 2.0);
}

// Clamp player to screen (sprite origin is TOP-LEFT)
let sprite_w = 48.0;  // Full sprite width
let sprite_h = 32.0;  // Full sprite height

// Left/top edge: position >= 0
// Right/bottom edge: position <= screen - sprite_size
transform.x = transform.x.clamp(0.0, self.screen_w - sprite_w);
transform.y = transform.y.clamp(0.0, self.screen_h - sprite_h);
```

## Common Components

| Component | Purpose |
|-----------|---------|
| `Transform` | Position, rotation, scale |
| `Velocity` | Movement direction and speed |
| `Renderable` | Sprite and rendering options |
| `RigidBody` | Physics body (dynamic, kinematic, fixed) |
| `Collider` | Collision shape (circle, rect, capsule) |
| `Animator` | Sprite animation state |
| `Tween` | Smooth interpolation of Transform properties |
| `PropertyTween<T>` | Generic value tweening (f32, Vec2, Color, custom Lerp types) |
| `Parent` / `Children` | Entity hierarchy |
| `NavAgent` | Navigation and pathfinding |
| `Trail2D` | Motion trails behind entities |

## Navigation & Pathfinding

The navigation system provides A* pathfinding with manual integration for full control.

### Basic Setup

```rust
struct GameScene {
    nav_grid: NavigationGrid,
    pathfinder: Pathfinder,
}

fn on_enter(&mut self, ctx: &mut SceneContext) {
    // Build nav grid from world
    self.nav_grid = NavigationBuilder::new(800.0, 600.0)
        .with_cell_size(16.0)
        .build_from_world(&ctx.world);

    // Spawn navigating entity
    ctx.world.spawn((
        Transform::new(100.0, 100.0),
        NavAgent::new(100.0)  // 100 px/sec
            .with_smoothing(PathSmoothing::LineOfSight),
    ));
}
```

### Setting Destinations

```rust
// Fixed position
ctx.world.add_component(entity, NavTarget::position(500.0, 400.0));

// Follow another entity
ctx.world.add_component(entity, NavTarget::follow(player_entity));
```

### Agent Avoidance with Steering

Use the `steering` module for agents that avoid each other:

```rust
use ryo2d::prelude::steering;

fn move_agent(&self, world: &mut World, entity: Entity, dt: f32) {
    let (pos, radius) = /* get from entity */;
    let waypoint = /* from NavAgent path */;

    // Collect nearby agent positions
    let neighbors: Vec<(Vec2, f32)> = /* query other agents */;

    // Calculate forces
    let path_dir = (waypoint - pos).normalize_or_zero();
    let avoid = steering::separation(pos, radius, &neighbors, 100.0);

    // Blend and apply
    let velocity = (path_dir * speed + avoid).clamp_length_max(speed);
    transform.x += velocity.x * dt;
    transform.y += velocity.y * dt;
}
```

### Steering Functions

| Function | Purpose |
|----------|---------|
| `steering::separation()` | Push away from nearby agents |
| `steering::seek()` | Move toward a target |
| `steering::flee()` | Move away from a threat |
| `steering::arrive()` | Seek but slow near target |
| `steering::cohesion()` | Move toward group center |
| `steering::alignment()` | Match group velocity |

## Visual Tests

Test what your scenes draw with `ryo2d::testing::HeadlessEngine` — no window, deterministic steps, pixels out:

```rust
use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

#[test]
fn player_is_drawn_at_spawn() {
  let Some(mut engine) = HeadlessEngine::new_or_skip(
    EngineBuilder::new().resolution(320, 180).start_scene(Level::default()),
  ) else { return }; // no GPU: skip

  engine.step_frames(5).unwrap();
  engine.tap_key(KeyCode::Space); // press_key / release_key / press_mouse / mouse_move too
  engine.step_frames(10).unwrap();

  let frame = engine.frame().unwrap(); // RgbaImage, game resolution
  assert_eq!(frame.get_pixel(160, 90).0, [255, 0, 0, 255]);
}
```

- One fixed update per step; same scene + steps + input → identical frames (seed your RNG).
- Headless: no audio (`ctx.audio` is `None`), no gamepads, no user preferences; `Ryo.toml` is honoured.
- Particle emitters use an unseeded thread RNG, so don't compare frames containing particles byte-for-byte.
- Save a failing frame with `engine.frame().unwrap().save("failed.png")`.
