//! Ferrum2D Demo - Scene-based Breakout Physics Showcase
//!
//! This demo showcases:
//! - Scene management (menu → gameplay → pause)
//! - Rapier2D physics integration
//! - Collision events for game logic
//!
//! Controls:
//! - Menu: Enter to start, Escape to quit
//! - Game: A/D to move paddle, Space to reset, Escape for pause
//! - Pause: Enter to resume, Escape to menu

use std::time::Duration;

use ferrum2d::prelude::*;

// ============================================================================
// Menu Scene
// ============================================================================

struct MenuScene {
  title_entity: Option<Entity>,
}

impl MenuScene {
  fn new() -> Self {
    Self { title_entity: None }
  }
}

impl Scene for MenuScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    // Clear any existing entities
    ctx.world.clear();

    // Create simple "title" text (using colored rectangles as placeholder)
    // In a real game, you'd use a text rendering system

    // "BREAKOUT" title bar
    self.title_entity = Some(ctx.world.spawn((
      Transform::new(440.0, 200.0),
      SpriteRenderer::new(Sprite::solid_rect(400, 80, 0x40, 0xc0, 0xff, 0xff)).with_z_order(10),
    )));

    // "Press ENTER" prompt
    ctx.world.spawn((
      Transform::new(490.0, 400.0),
      SpriteRenderer::new(Sprite::solid_rect(300, 40, 0x80, 0x80, 0x80, 0xff)).with_z_order(10),
    ));

    // Decorative elements
    for i in 0..6 {
      let colors = [
        (0xff, 0x60, 0x60),
        (0xff, 0xff, 0x60),
        (0x60, 0xff, 0x60),
        (0x60, 0xff, 0xff),
        (0x60, 0x60, 0xff),
        (0xff, 0x60, 0xff),
      ];
      let (r, g, b) = colors[i];
      ctx.world.spawn((
        Transform::new(300.0 + i as f32 * 120.0, 550.0),
        SpriteRenderer::new(Sprite::solid_rect(40, 40, r, g, b, 0xff)).with_z_order(5),
      ));
    }

    ctx.camera.set_position(640.0, 360.0);

    println!("=== Ferrum2D Breakout Demo ===");
    println!("Press ENTER to start, ESCAPE to quit");
  }

  fn on_exit(&mut self, ctx: &mut SceneContext) {
    // Clean up menu entities
    ctx.world.clear();
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    if ctx.input.key_pressed(KeyCode::Enter) {
      // Use a fade transition when starting the game
      return SceneAction::SwitchWith(
        Box::new(GameplayScene::new()),
        Transition::fade(Duration::from_millis(300)),
      );
    }

    if ctx.input.key_pressed(KeyCode::Escape) {
      return SceneAction::Quit;
    }

    SceneAction::Continue
  }

  fn name(&self) -> &str {
    "MainMenu"
  }
}

// ============================================================================
// Gameplay Scene
// ============================================================================

struct GameplayScene {
  paddle: Option<Entity>,
  balls: Vec<Entity>,
  walls: Vec<Entity>,
  score: u32,
}

impl GameplayScene {
  fn new() -> Self {
    Self {
      paddle: None,
      balls: Vec::new(),
      walls: Vec::new(),
      score: 0,
    }
  }

  fn spawn_walls(&mut self, ctx: &mut SceneContext) {
    // Floor
    let floor = ctx.world.spawn((
      Transform::new(0.0, 680.0),
      RigidBody::kinematic(),
      Collider::box_collider(1280.0, 40.0),
      SpriteRenderer::new(Sprite::solid_rect(1280, 40, 0x40, 0x40, 0x40, 0xff)).with_z_order(0),
    ));
    self.walls.push(floor);

    // Left wall
    let left = ctx.world.spawn((
      Transform::new(-40.0, 0.0),
      RigidBody::kinematic(),
      Collider::box_collider(40.0, 720.0),
      SpriteRenderer::new(Sprite::solid_rect(40, 720, 0x40, 0x40, 0x40, 0xff)).with_z_order(0),
    ));
    self.walls.push(left);

    // Right wall
    let right = ctx.world.spawn((
      Transform::new(1280.0, 0.0),
      RigidBody::kinematic(),
      Collider::box_collider(40.0, 720.0),
      SpriteRenderer::new(Sprite::solid_rect(40, 720, 0x40, 0x40, 0x40, 0xff)).with_z_order(0),
    ));
    self.walls.push(right);

    // Ceiling
    let ceiling = ctx.world.spawn((
      Transform::new(0.0, -40.0),
      RigidBody::kinematic(),
      Collider::box_collider(1280.0, 40.0),
      SpriteRenderer::new(Sprite::solid_rect(1280, 40, 0x40, 0x40, 0x40, 0xff)).with_z_order(0),
    ));
    self.walls.push(ceiling);
  }

  fn spawn_paddle(&mut self, ctx: &mut SceneContext) {
    let paddle_width = 120.0;
    let paddle_height = 16.0;

    let paddle = ctx.world.spawn((
      Transform::new(580.0, 620.0),
      Velocity::zero(),
      RigidBody::kinematic(),
      Collider::box_collider(paddle_width, paddle_height),
      SpriteRenderer::new(Sprite::solid_rect(
        paddle_width as u32,
        paddle_height as u32,
        0x40,
        0xc0,
        0xff,
        0xff,
      ))
      .with_z_order(10),
    ));
    self.paddle = Some(paddle);
  }

  fn spawn_balls(&mut self, ctx: &mut SceneContext) {
    let colors = [
      (0xff, 0x60, 0x60),
      (0x60, 0xff, 0x60),
      (0x60, 0x60, 0xff),
      (0xff, 0xff, 0x60),
      (0xff, 0x60, 0xff),
      (0x60, 0xff, 0xff),
    ];

    for i in 0..12 {
      let x = 150.0 + (i % 6) as f32 * 160.0;
      let y = 80.0 + (i / 6) as f32 * 80.0;
      let (r, g, b) = colors[i % colors.len()];
      let vx = ((i as f32 * 47.0) % 200.0) - 100.0;
      let vy = 50.0 + (i as f32 * 23.0) % 100.0;

      let ball_size = 20;
      let ball_radius = ball_size as f32 / 2.0;

      let ball = ctx.world.spawn((
        Transform::new(x, y),
        Velocity::new(vx, vy),
        RigidBody::new().with_gravity_scale(0.5).with_drag(0.01),
        Collider::circle_collider(ball_radius).with_offset(ball_radius, ball_radius),
        SpriteRenderer::new(Sprite::solid_rect(ball_size, ball_size, r, g, b, 0xff))
          .with_z_order(5),
      ));

      self.balls.push(ball);
    }
  }

  fn reset_balls(&mut self, ctx: &mut SceneContext) {
    for (i, &ball) in self.balls.iter().enumerate() {
      let x = 150.0 + (i % 6) as f32 * 160.0;
      let y = 80.0 + (i / 6) as f32 * 80.0;
      let vx = ((i as f32 * 47.0) % 200.0) - 100.0;
      let vy = 50.0 + (i as f32 * 23.0) % 100.0;

      if let Some(mut transform) = ctx.world.get_component_mut::<Transform>(ball) {
        transform.x = x;
        transform.y = y;
      }
      if let Some(mut vel) = ctx.world.get_component_mut::<Velocity>(ball) {
        vel.dx = vx;
        vel.dy = vy;
      }
    }
    self.score = 0;
  }

  fn spawn_background(&mut self, ctx: &mut SceneContext) {
    for row in 0..8 {
      for col in 0..14 {
        let x = col as f32 * 100.0;
        let y = row as f32 * 100.0;
        let color = if (row + col) % 2 == 0 {
          (0x25, 0x25, 0x30)
        } else {
          (0x2a, 0x2a, 0x35)
        };
        ctx.world.spawn((
          Transform::new(x, y),
          SpriteRenderer::new(Sprite::solid_rect(16, 16, color.0, color.1, color.2, 0xff))
            .with_z_order(-10),
        ));
      }
    }
  }
}

impl Scene for GameplayScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    self.spawn_background(ctx);
    self.spawn_walls(ctx);
    self.spawn_paddle(ctx);
    self.spawn_balls(ctx);

    ctx.camera.set_position(640.0, 360.0);

    println!("Game started! Score: 0");
    println!("Controls: A/D to move, Space to reset, Escape to pause");
  }

  fn on_exit(&mut self, ctx: &mut SceneContext) {
    println!("Final score: {}", self.score);
    ctx.world.clear();
  }

  fn on_pause(&mut self, _ctx: &mut SceneContext) {
    println!("Game paused");
  }

  fn on_resume(&mut self, _ctx: &mut SceneContext) {
    println!("Game resumed");
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    // Escape to pause
    if ctx.input.key_pressed(KeyCode::Escape) {
      return SceneAction::Push(Box::new(PauseScene::new(self.score)));
    }

    // Paddle movement
    if let Some(paddle) = self.paddle
      && let Some(mut transform) = ctx.world.get_component_mut::<Transform>(paddle)
    {
      let speed = 500.0;

      if ctx.input.key_down(KeyCode::KeyA) || ctx.input.key_down(KeyCode::ArrowLeft) {
        transform.x -= speed * dt;
      }
      if ctx.input.key_down(KeyCode::KeyD) || ctx.input.key_down(KeyCode::ArrowRight) {
        transform.x += speed * dt;
      }

      transform.x = transform.x.clamp(0.0, 1280.0 - 120.0);
    }

    // Collision events for scoring
    for event in ctx.collision_events {
      if event.event_type != CollisionEventType::Started {
        continue;
      }

      if let Some(paddle) = self.paddle
        && event.involves(paddle)
        && let Some(ball) = event.other(paddle)
        && self.balls.contains(&ball)
      {
        self.score += 1;
      }
    }

    // Reset balls
    if ctx.input.key_pressed(KeyCode::Space) {
      self.reset_balls(ctx);
      println!("Balls reset! Score was: {}", self.score);
    }

    // Zoom
    let scroll = ctx.input.scroll_delta();
    if scroll.1 != 0.0 {
      ctx.camera.set_zoom(ctx.camera.zoom + scroll.1 * 0.1);
    }

    SceneAction::Continue
  }

  fn name(&self) -> &str {
    "Gameplay"
  }
}

// ============================================================================
// Pause Scene (Overlay)
// ============================================================================

struct PauseScene {
  score_at_pause: u32,
}

impl PauseScene {
  fn new(score: u32) -> Self {
    Self {
      score_at_pause: score,
    }
  }
}

impl Scene for PauseScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    // Add a semi-transparent overlay (dark rectangle)
    ctx.world.spawn((
      Transform::new(440.0, 280.0),
      SpriteRenderer::new(Sprite::solid_rect(400, 160, 0x20, 0x20, 0x20, 0xcc)).with_z_order(100),
    ));

    // "PAUSED" text placeholder
    ctx.world.spawn((
      Transform::new(540.0, 320.0),
      SpriteRenderer::new(Sprite::solid_rect(200, 40, 0xff, 0xff, 0xff, 0xff)).with_z_order(101),
    ));

    println!("=== PAUSED ===");
    println!("Score: {}", self.score_at_pause);
    println!("Press ENTER to resume, ESCAPE to return to menu");
  }

  fn on_exit(&mut self, _ctx: &mut SceneContext) {
    // Note: We don't clear the world here because we want to preserve
    // the game state when resuming. The pause overlay entities will
    // be cleaned up when the gameplay scene exits.
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    if ctx.input.key_pressed(KeyCode::Enter) {
      return SceneAction::Pop;
    }

    if ctx.input.key_pressed(KeyCode::Escape) {
      // Pop pause, then switch to menu with fade transition
      return SceneAction::ResetWith(
        Box::new(MenuScene::new()),
        Transition::fade(Duration::from_millis(200)),
      );
    }

    SceneAction::Continue
  }

  fn is_transparent(&self) -> bool {
    true // Render gameplay scene below
  }

  fn name(&self) -> &str {
    "PauseMenu"
  }
}

// ============================================================================
// Main
// ============================================================================

fn main() {
  // Use with_scene() for scene-based games
  let engine = EngineBuilder::new()
    .title("Ferrum2D - Scene Demo")
    .resolution(1280, 720)
    .fps(60)
    .with_scene(MenuScene::new())
    .build();

  if let Err(e) = engine.run() {
    eprintln!("Engine error: {}", e);
  }
}
