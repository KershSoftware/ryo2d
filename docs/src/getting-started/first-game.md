# Your First Game

In this tutorial, we'll create a simple game with a player that moves around the screen.

## Architecture

Every Ryo2D game uses the `Scene` trait. Scenes represent distinct states of your game (menu, gameplay, pause, etc.). Even simple single-screen games use a scene.

For more complex scene management (push, pop, transitions), see [Scene Management](../concepts/scenes.md).

## Setting Up the Game Struct

```rust
use ryo2d::prelude::*;

struct MyScene {
    player: Option<Entity>,
}

impl MyScene {
    fn new() -> Self {
        MyScene { player: None }
    }
}
```

## Spawning a Player

In `on_enter()`, we create an entity and attach components:

```rust
impl Scene for MyScene {
    fn on_enter(&mut self, ctx: &mut SceneContext) {
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

    fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
        // Coming next...
        SceneAction::Continue
    }
}
```

## Handling Input

Now let's make the player move with WASD:

```rust
fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
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
    SceneAction::Continue
}
```

The engine automatically applies velocity to transform each frame!

## Running the Game

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    EngineBuilder::new()
        .title("My First Game")
        .resolution(800, 600)
        .start_scene(MyScene::new())
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

### Multi-Scene Games

For games with menus, pause screens, and level transitions, scenes can push/pop other scenes:

```rust
EngineBuilder::new()
    .start_scene(MenuScene::new())
    .build()
    .run()?;
```
