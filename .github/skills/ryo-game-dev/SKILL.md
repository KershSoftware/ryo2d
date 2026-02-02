---
name: ryo-game-dev
description: Building games with Ryo2D engine - ECS, entities, scenes, input.
metadata:
  version: 1.0.0
---

# Ryo2D Game Developer

You help users build complete games using the Ryo2D engine. You understand the ECS architecture, the Game/Scene trait lifecycle, and common game patterns.

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

struct MyGame {
    player: Option<Entity>,
    score: u32,
}

impl Game for MyGame {
    fn init(&mut self, ctx: &mut GameContext) {
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

    fn update(&mut self, ctx: &mut GameContext, dt: f32) {
        // Handle input, update game state
        if ctx.input.key_pressed(KeyCode::Escape) {
            // Exit game
        }
    }

    fn render_ui(&mut self, ui: &mut UiContext) {
        ui.label(&format!("Score: {}", self.score), 10.0, 10.0, Color::WHITE, None);
    }
}

fn main() {
    // Resolution/FPS come from Ryo.toml - don't set .resolution() here!
    EngineBuilder::new()
        .title("My Game")
        .with_game(MyGame { player: None, score: 0 })
        .build()
        .run()
        .unwrap();
}
```

## Game vs Scene

| Use | When |
|-----|------|
| `Game` trait | Single-screen games, simple projects |
| `Scene` trait | Menu systems, multiple game states, level transitions |

## GameContext Fields

| Field | Type | Purpose |
|-------|------|---------|
| `ctx.world` | `&mut World` | ECS world - spawn/query entities |
| `ctx.camera` | `&mut Camera` | Camera position, zoom, viewport |
| `ctx.input` | `&InputState` | Keyboard/mouse state |
| `ctx.assets` | `&mut AssetManager` | Load sprites, sounds |
| `ctx.audio` | `&mut Option<AudioManager>` | Play sounds/music |
| `ctx.effects` | `&mut EffectsManager` | Post-processing effects |
| `ctx.collision_events` | `&[PhysicsCollisionEvent]` | Physics collisions |

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
        // Setup when scene becomes active
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

    fn render_ui(&mut self, ui: &mut UiContext) {
        // Get screen size (returns u32, cast to f32 for positioning)
        let (sw, sh) = ui.screen_size();
        let screen_w = sw as f32;
        let screen_h = sh as f32;
        
        ui.label("Press ENTER to start", screen_w / 2.0 - 80.0, screen_h / 2.0, Color::WHITE, None);
    }
}

// SceneAction options:
// - Continue            Keep running
// - Push(scene)         Add scene on top (pause current)
// - Pop                 Return to previous scene  
// - Switch(scene)       Replace current scene (NOT "Replace"!)
// - Reset(scene)        Clear all scenes, push new one
// - Quit                Exit game
// All except Continue/Pop/Quit have "With" variants for transitions
```

## EngineBuilder Options

```rust
// Minimal - uses Ryo.toml for resolution, fps, etc.
EngineBuilder::new()
    .title("My Game")
    .with_game(MyGame::default())  // OR
    .with_scene(MenuScene)         // for multi-scene
    .build()
    .run()
    .unwrap();

// Override Ryo.toml (only if needed for special cases)
EngineBuilder::new()
    .title("My Game")
    .resolution(1920, 1080)           // Override resolution
    .fps(120)                         // Override FPS
    .physics_hz(240)                  // Physics update rate
    .window_mode(WindowMode::ResizableAspectRatio)
    .with_game(MyGame::default())
    .build()
    .run()
    .unwrap();
```

**Note:** Prefer using `Ryo.toml` for configuration. EngineBuilder methods override the config file.

## Fixed Design Resolution Pattern

For games where gameplay depends on consistent dimensions (platformers, bullet-hell, etc.), use a **fixed design resolution** with scalable display:

```rust
// In main.rs - set fixed game resolution, allow display scaling
EngineBuilder::new()
    .title("My Game")
    .resolution(1920, 1080)  // Fixed design resolution - game logic uses this
    .window_mode(WindowMode::ResizableAspectRatio)  // Window can scale
    .fps(144)
    .with_scene(MenuScene::new())
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
| **Display Mode** | How the game appears on screen (Windowed/Fullscreen/Borderless) |
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
// In your Scene or Game struct, store screen bounds
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
| `Parent` / `Children` | Entity hierarchy |

