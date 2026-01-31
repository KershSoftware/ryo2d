# Your First Game

In this tutorial, we'll create a simple game with a player that moves around the screen.

## Choosing Your Architecture

Ryo2D offers two approaches for structuring your game:

| Approach | Best For | Entry Point |
|----------|----------|-------------|
| **Game trait** | Simple games, tutorials, prototypes | `with_game()` |
| **Scene trait** | Multi-screen games (menus, pause, levels) | `with_scene()` |

This tutorial uses the simpler `Game` trait. For scene-based architecture, see [Scene Management](../concepts/scenes.md).

## Setting Up the Game Struct

```rust
use ryo2d::prelude::*;

struct MyGame {
    player: Option<Entity>,
}

impl MyGame {
    fn new() -> Self {
        MyGame { player: None }
    }
}
```

## Spawning a Player

In `init()`, we create an entity and attach components:

```rust
impl Game for MyGame {
    fn init(&mut self, ctx: &mut GameContext) {
        // Create a player entity with all components
        let sprite = Sprite::solid_rect(32, 32, 0x40, 0xc0, 0xff, 0xff);
        let sprite_handle = SpriteHandle::from_sprite(sprite, "player");
        
        let player = ctx.world.spawn((
            Transform::new(400.0, 300.0),
            Velocity::zero(),
            Renderable::sprite(sprite_handle),
        ));
        
        self.player = Some(player);
    }
    
    fn update(&mut self, _ctx: &mut GameContext, _dt: f32) {
        // Coming next...
    }
}
```

## Handling Input

Now let's make the player move with WASD:

```rust
fn update(&mut self, ctx: &mut GameContext, _dt: f32) {
    if let Some(player) = self.player {
        if let Some(mut velocity) = ctx.world.get_component_mut::<Velocity>(player) {
            let speed = 200.0;
            velocity.dx = 0.0;
            velocity.dy = 0.0;

            if ctx.input.key_down(KeyCode::KeyW) {
                velocity.dy = -speed;
            }
            if ctx.input.key_down(KeyCode::KeyS) {
                velocity.dy = speed;
            }
            if ctx.input.key_down(KeyCode::KeyA) {
                velocity.dx = -speed;
            }
            if ctx.input.key_down(KeyCode::KeyD) {
                velocity.dx = speed;
            }
        }
    }
}
```

The engine automatically applies velocity to transform each frame!

## Running the Game

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    EngineBuilder::new()
        .title("My First Game")
        .resolution(800, 600)
        .with_game(MyGame::new())  // Use with_game() for Game trait
        .build()
        .run()?;
    Ok(())
}
```

## Complete Code

See the [Moving Player Example](../examples/moving-player.md) for the complete code.

## Next Steps

Once you're comfortable with the basics, consider:

- **[Scene Management](../concepts/scenes.md)** - For games with menus, pause screens, and multiple levels
- **[Physics System](../concepts/physics.md)** - For collision detection and physics simulation
- **[Asset Loading](../concepts/asset-loading.md)** - For loading sprites, sounds, and other resources

### Quick Comparison: Game vs Scene

```rust
// Simple approach: Game trait
EngineBuilder::new()
    .with_game(MyGame::new())
    .build()
    .run()?;

// Scene-based approach: for multi-screen games
EngineBuilder::new()
    .with_scene(MenuScene::new())  // Scenes can push/pop other scenes
    .build()
    .run()?;
```
