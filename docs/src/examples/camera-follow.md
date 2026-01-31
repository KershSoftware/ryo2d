# Camera Follow Example

A player that moves with WASD while the camera smoothly follows.

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

impl Game for MyGame {
    fn init(&mut self, ctx: &mut GameContext) {
        // Create player
        let player = ctx.world.spawn();
        ctx.world.add_component(player, Transform::new(400.0, 300.0));
        ctx.world.add_component(player, Velocity::zero());
        
        let sprite = Sprite::solid_rect(32, 32, 0x40, 0xc0, 0xff, 0xff);
        ctx.world.add_component(player, Renderable::sprite(sprite).with_z_order(10));
        
        self.player = Some(player);

        // Create background grid for visual reference
        for row in 0..20 {
            for col in 0..20 {
                let marker = ctx.world.spawn();
                let x = col as f32 * 100.0 - 500.0;
                let y = row as f32 * 100.0 - 500.0;
                ctx.world.add_component(marker, Transform::new(x, y));
                
                let color = if (row + col) % 2 == 0 { 0x60 } else { 0x40 };
                let sprite = Sprite::solid_rect(16, 16, color, color, color, 0xff);
                ctx.world.add_component(marker, Renderable::sprite(sprite));
            }
        }

        // Center camera on player
        ctx.camera.set_position(400.0, 300.0);
    }

    fn update(&mut self, ctx: &mut GameContext, _dt: f32) {
        // Player movement
        if let Some(player) = self.player {
            if let Some(mut velocity) = ctx.world.get_component_mut::<Velocity>(player) {
                let speed = 200.0;
                velocity.dx = 0.0;
                velocity.dy = 0.0;

                if ctx.input.key_down(KeyCode::KeyW) { velocity.dy = -speed; }
                if ctx.input.key_down(KeyCode::KeyS) { velocity.dy = speed; }
                if ctx.input.key_down(KeyCode::KeyA) { velocity.dx = -speed; }
                if ctx.input.key_down(KeyCode::KeyD) { velocity.dx = speed; }
            }

            // Camera follows player smoothly
            if let Some(transform) = ctx.world.get_component::<Transform>(player) {
                let center_x = transform.x + 16.0;
                let center_y = transform.y + 16.0;
                ctx.camera.follow(center_x, center_y, 0.1);
            }
        }

        // Scroll wheel zoom
        let scroll = ctx.input.scroll_delta();
        if scroll.1 != 0.0 {
            let new_zoom = ctx.camera.zoom + scroll.1 * 0.1;
            ctx.camera.set_zoom(new_zoom);
        }
    }
}

fn main() {
    EngineBuilder::new()
        .title("Camera Follow")
        .resolution(800, 600)
        .fps(60)
        .with_game(MyGame::new())
        .build()
        .run()
        .unwrap();
}
```

## Key Points

- **`camera.follow(x, y, smoothness)`** - Smoothly moves camera toward target
  - `0.1` = smooth/slow
  - `1.0` = instant snap
- **Background grid** - Shows camera movement relative to world
- **Scroll wheel** - Zoom in/out to see more of the world
