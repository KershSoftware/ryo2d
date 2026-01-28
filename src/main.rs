//! Demo game showcasing Ferrum2D engine features

use ferrum2d::prelude::*;

/// Demo game showcasing the engine's features
struct DemoGame {
  player: Option<Entity>,
}

impl DemoGame {
  fn new() -> Self {
    DemoGame { player: None }
  }
}

impl Game for DemoGame {
  fn init(&mut self, ctx: &mut GameContext) {
    // Create a player entity with components
    let player = ctx.world.spawn();

    // Add Transform component (position in world)
    ctx
      .world
      .add_component(player, Transform::new(400.0, 300.0));

    // Add Velocity component (for movement)
    ctx.world.add_component(player, Velocity::zero());

    // Add SpriteRenderer component (visual representation)
    // Player draws on top of background (z_order = 10)
    let player_sprite = Sprite::solid_rect(32, 32, 0x40, 0xc0, 0xff, 0xff);
    ctx
      .world
      .add_component(player, SpriteRenderer::new(player_sprite).with_z_order(10));

    self.player = Some(player);

    // Create some background markers so zoom is visible
    // Background layer (z_order = 0, drawn first)
    for row in 0..10 {
      for col in 0..10 {
        let marker = ctx.world.spawn();
        let x = col as f32 * 100.0;
        let y = row as f32 * 100.0;
        ctx.world.add_component(marker, Transform::new(x, y));
        // Alternating colors for a grid pattern
        let color = if (row + col) % 2 == 0 {
          (0x80, 0x60, 0x40) // brown
        } else {
          (0x40, 0x60, 0x80) // blue-gray
        };
        let marker_sprite = Sprite::solid_rect(16, 16, color.0, color.1, color.2, 0xff);
        // z_order = 0 (default, drawn behind player)
        ctx
          .world
          .add_component(marker, SpriteRenderer::new(marker_sprite));
      }
    }

    // Center camera on player start position
    ctx.camera.set_position(400.0, 300.0);
  }

  fn update(&mut self, ctx: &mut GameContext, _dt: f32) {
    // === Player Input System ===
    if let Some(player) = self.player
      && let Some(mut velocity) = ctx.world.get_component_mut::<Velocity>(player)
    {
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

    // === Camera Follow System ===
    if let Some(player) = self.player
      && let Some(transform) = ctx.world.get_component::<Transform>(player)
    {
      // Center camera on player (with smooth follow)
      let center_x = transform.x + 16.0; // Center of 32x32 sprite
      let center_y = transform.y + 16.0;
      ctx.camera.follow(center_x, center_y, 0.1);
    }

    // === Zoom Controls ===
    let scroll = ctx.input.scroll_delta();
    if scroll.1 != 0.0 {
      let new_zoom = ctx.camera.zoom + scroll.1 * 0.1;
      ctx.camera.set_zoom(new_zoom);
    }
  }
}

fn main() {
  let engine = EngineBuilder::new()
    .title("Ferrum2D Demo")
    .resolution(1280, 720)
    .fps(60)
    .with_game(DemoGame::new())
    .build();

  if let Err(e) = engine.run() {
    eprintln!("Engine error: {}", e);
  }
}
