---
name: Ryo2D Game Developer
description: Building games with Ryo2D engine - ECS, entities, scenes, input.
version: 1.0.0
trigger:
  - "create game"
  - "spawn entity"
  - "player movement"
  - "game logic"
---

# Ryo2D Game Developer

You help users build complete games using the Ryo2D engine. You understand the ECS architecture, the Game trait lifecycle, and common game patterns.

## Resources

- **Engine Architecture**: `src/core/AGENTS.md`
- **ECS Patterns**: `src/core/ecs/AGENTS.md`
- **Public API**: `ryo2d::prelude::*`
- **Examples**: `examples/games/breakout/`, `examples/games/flappy/`

## The Game Trait

Every Ryo2D game implements the `Game` trait:

```rust
use ryo2d::prelude::*;

struct MyGame {
    player: Option<Entity>,
    score: u32,
}

impl Game for MyGame {
    fn init(&mut self, ctx: &mut GameContext) {
        // Called once at startup - spawn entities, load assets
    }

    fn update(&mut self, ctx: &mut GameContext, dt: f32) {
        // Called every fixed timestep - game logic, input
    }

    fn render_ui(&mut self, ui: &mut UiContext) {
        // Called after sprites - draw HUD elements
    }
}
```

## GameContext

| Field | Type | Purpose |
|-------|------|---------|
| `ctx.world` | `&mut World` | ECS world - spawn/query entities |
| `ctx.camera` | `&mut Camera` | Camera position, zoom, viewport |
| `ctx.input` | `&InputState` | Keyboard/mouse state |
| `ctx.assets` | `&mut AssetManager` | Load sprites, sounds |
| `ctx.audio` | `&mut Option<AudioManager>` | Play sounds/music |
| `ctx.effects` | `&mut EffectsManager` | Post-processing effects |
| `ctx.collision_events` | `&[PhysicsCollisionEvent]` | Physics collisions |

## Spawning Entities

```rust
let player = ctx.world.spawn((
    Transform::new(100.0, 200.0),
    Velocity::new(0.0, 0.0),
    Renderable::sprite(sprite_handle),
    RigidBody::dynamic(),
    Collider::circle(16.0),
));
self.player = Some(player);
```

## Input Handling

```rust
// Keyboard
if ctx.input.key_pressed(KeyCode::Space) { /* just pressed */ }
if ctx.input.key_down(KeyCode::KeyW) { /* held down */ }
if ctx.input.key_released(KeyCode::Escape) { /* just released */ }

// Mouse
if ctx.input.mouse_pressed(MouseButton::Left) { /* clicked */ }
let (mx, my) = ctx.input.mouse_position();

// World coordinates (camera-adjusted)
let world_pos = ctx.camera.screen_to_world(mx, my);
```

## Querying Entities

```rust
// Get single component
if let Some(transform) = ctx.world.get_component::<Transform>(entity) {
    println!("Position: {:?}", transform.position);
}

// Mutate component
if let Some(mut vel) = ctx.world.get_component_mut::<Velocity>(entity) {
    vel.x += 10.0;
}

// Query multiple components
for (entity, (transform, velocity)) in ctx.world.query::<(&Transform, &Velocity)>().iter() {
    // access both components
}
```

## Scene Management

```rust
struct MenuScene;

impl Scene for MenuScene {
    fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
        if ctx.input.key_pressed(KeyCode::Enter) {
            return SceneAction::Push(Box::new(GameScene::new()));
        }
        SceneAction::Continue
    }
}

// In main.rs - use with_scene for multi-scene games
EngineBuilder::new()
    .with_scene(MenuScene)
    .build()
    .run()
```

SceneAction options: `Continue`, `Push(scene)`, `Pop`, `Replace(scene)`, `Quit`

## Common Patterns

### Player Movement

```rust
fn update(&mut self, ctx: &mut GameContext, dt: f32) {
    let speed = 200.0;
    let mut dx = 0.0;
    let mut dy = 0.0;

    if ctx.input.key_down(KeyCode::KeyW) { dy -= 1.0; }
    if ctx.input.key_down(KeyCode::KeyS) { dy += 1.0; }
    if ctx.input.key_down(KeyCode::KeyA) { dx -= 1.0; }
    if ctx.input.key_down(KeyCode::KeyD) { dx += 1.0; }

    // Normalize diagonal
    let len = (dx * dx + dy * dy).sqrt();
    if len > 0.0 { dx /= len; dy /= len; }

    if let Some(mut vel) = ctx.world.get_component_mut::<Velocity>(self.player.unwrap()) {
        vel.x = dx * speed;
        vel.y = dy * speed;
    }
}
```

### Collision Response

```rust
for event in ctx.collisions_started() {
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
```

### Camera Follow

```rust
if let Some(transform) = ctx.world.get_component::<Transform>(self.player.unwrap()) {
    ctx.camera.set_position(transform.position.x, transform.position.y);
}
```

## EngineBuilder

```rust
EngineBuilder::new()
    .title("My Game")
    .resolution(1280, 720)
    .fps(60)
    .physics_hz(120)
    .window_mode(WindowMode::ResizableAspectRatio)
    .with_game(MyGame::default())
    .build()
    .run()
    .unwrap();
```

WindowMode: `Fixed`, `ResizableAspectRatio` (default), `ResizableIntegerScale`, `ResizableStretch`
