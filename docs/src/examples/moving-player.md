# Moving Player Example

A complete example of a player that moves with WASD keys.

```rust
use ferrum2d::prelude::*;

struct MyGame {
    player: Option<Entity>,
}

impl MyGame {
    fn new() -> Self {
        MyGame { player: None }
    }
}

impl Game for MyGame {
    fn init(&mut self, ctx: &mut GameContext) {
        // Create player entity
        let player = ctx.world.spawn();
        
        // Position at center of screen
        ctx.world.add_component(player, Transform::new(400.0, 300.0));
        
        // Enable movement
        ctx.world.add_component(player, Velocity::zero());
        
        // Visual: 32x32 cyan square
        let sprite = Sprite::solid_rect(32, 32, 0x40, 0xc0, 0xff, 0xff);
        ctx.world.add_component(player, SpriteRenderer::new(sprite));
        
        self.player = Some(player);
    }

    fn update(&mut self, ctx: &mut GameContext, _dt: f32) {
        if let Some(player) = self.player {
            if let Some(mut velocity) = ctx.world.get_component_mut::<Velocity>(player) {
                let speed = 200.0;
                velocity.dx = 0.0;
                velocity.dy = 0.0;

                if ctx.input.key_down(KeyCode::KeyW) || ctx.input.key_down(KeyCode::ArrowUp) {
                    velocity.dy = -speed;
                }
                if ctx.input.key_down(KeyCode::KeyS) || ctx.input.key_down(KeyCode::ArrowDown) {
                    velocity.dy = speed;
                }
                if ctx.input.key_down(KeyCode::KeyA) || ctx.input.key_down(KeyCode::ArrowLeft) {
                    velocity.dx = -speed;
                }
                if ctx.input.key_down(KeyCode::KeyD) || ctx.input.key_down(KeyCode::ArrowRight) {
                    velocity.dx = speed;
                }
            }
        }
    }
}

fn main() {
    EngineBuilder::new()
        .title("Moving Player")
        .resolution(800, 600)
        .fps(60)
        .with_game(MyGame::new())
        .build()
        .run()
        .unwrap();
}
```

## How It Works

1. **`init()`** - Creates a player entity with Transform, Velocity, and SpriteRenderer
2. **`update()`** - Reads keyboard input and sets velocity
3. **Movement System** - The engine automatically applies velocity to transform

The player moves at 200 pixels per second in any direction.
