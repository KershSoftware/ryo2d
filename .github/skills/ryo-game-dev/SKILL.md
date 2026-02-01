---
name: ryo-game-dev
description: Building games with Ryo2D engine - ECS, entities, scenes, input.
metadata:
  version: 1.0.0
---

# Ryo2D Game Developer

You help users build complete games using the Ryo2D engine. You understand the ECS architecture, the Game/Scene trait lifecycle, and common game patterns.

**For complete API signatures, see the `ryo-api` skill.**

## Quick Reference

```rust
use ryo2d::prelude::*;

struct MyGame {
    player: Option<Entity>,
    score: u32,
}

impl Game for MyGame {
    fn init(&mut self, ctx: &mut GameContext) {
        // Load a sprite from assets folder
        let sprite = ctx.assets.load_sprite("player.png").unwrap();
        
        // Or create a colored rectangle
        let sprite = ctx.assets.create_sprite(
            Sprite::solid_rect(32, 32, 255, 0, 0, 255),
            "red-square"
        );
        
        // Spawn player entity
        self.player = Some(ctx.world.spawn((
            Transform::new(640.0, 360.0),
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
    EngineBuilder::new()
        .title("My Game")
        .resolution(1280, 720)
        .fps(60)
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
// Get component
if let Some(transform) = ctx.world.get_component::<Transform>(entity) {
    let pos = transform.position;
}

// Modify component
if let Some(mut vel) = ctx.world.get_component_mut::<Velocity>(entity) {
    vel.x = 100.0;
}

// Query all entities with components
for (entity, (transform, velocity)) in ctx.world.query::<(&Transform, &Velocity)>().iter() {
    // process each
}

// Query with optional
for (entity, (t, maybe_v)) in ctx.world.query::<(&Transform, Option<&Velocity>)>().iter() {
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
    ctx.camera.set_position(t.position.x, t.position.y);
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
        ui.label("Press ENTER to start", 100.0, 100.0, Color::WHITE, None);
    }
}

// SceneAction options:
// - Continue         Keep running
// - Push(scene)      Add scene on top (pause current)
// - Pop              Return to previous scene
// - Replace(scene)   Swap current scene
// - Quit             Exit game
```

## EngineBuilder Options

```rust
EngineBuilder::new()
    .title("My Game")
    .resolution(1280, 720)
    .fps(60)
    .physics_hz(120)
    .window_mode(WindowMode::ResizableAspectRatio)
    .with_game(MyGame::default())  // OR
    .with_scene(MenuScene)         // for multi-scene
    .build()
    .run()
    .unwrap();
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

