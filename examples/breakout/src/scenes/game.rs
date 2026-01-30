//! Game Scene
//!
//! The main gameplay scene where the breakout game is played.

use std::path::Path;
use std::time::Duration;

use ryo2d::prelude::*;

use super::super::components::{Ball, Block, DeathZone, Paddle, Wall};
use super::super::constants::*;
use super::super::levels::{level_path, load_level};
use super::super::save::save_high_score;
use super::{MenuScene, PauseScene};

/// Main gameplay scene
pub struct GameScene {
  // Game state
  score: u32,
  high_score: u32,
  lives: u32,
  level: u32,
  game_started: bool,
  game_over: bool,

  // Entity tracking
  paddle: Option<Entity>,
  ball: Option<Entity>,
  blocks: Vec<Entity>,
  walls: Vec<Entity>,
  death_zone: Option<Entity>,

  // Block count for win detection
  blocks_remaining: u32,

  // Wait for escape release after resuming from pause
  escape_was_released: bool,
}

impl GameScene {
  /// Create a new game scene
  pub fn new(high_score: u32) -> Self {
    Self {
      score: 0,
      high_score,
      lives: STARTING_LIVES,
      level: 1,
      game_started: false,
      game_over: false,
      paddle: None,
      ball: None,
      blocks: Vec::new(),
      walls: Vec::new(),
      death_zone: None,
      blocks_remaining: 0,
      escape_was_released: true,
    }
  }

  fn spawn_paddle(&mut self, ctx: &mut SceneContext) {
    let paddle = ctx.world.spawn((
      Transform::new(SCREEN_WIDTH as f32 / 2.0 - PADDLE_WIDTH / 2.0, PADDLE_Y),
      Velocity::zero(),
      RigidBody::kinematic(),
      Collider::box_collider(PADDLE_WIDTH, PADDLE_HEIGHT),
      SpriteRenderer::new(Sprite::solid_rect(
        PADDLE_WIDTH as u32,
        PADDLE_HEIGHT as u32,
        0x40,
        0xc0,
        0xff,
        0xff,
      ))
      .with_z_order(10),
      Paddle,
    ));
    self.paddle = Some(paddle);
  }

  fn spawn_ball(&mut self, ctx: &mut SceneContext) {
    // Ball spawns above the paddle, centered
    let ball_x = SCREEN_WIDTH as f32 / 2.0 - BALL_SIZE / 2.0;
    let ball_y = PADDLE_Y - BALL_SIZE - 10.0;

    let ball = ctx.world.spawn((
      Transform::new(ball_x, ball_y),
      Velocity::zero(),                                        // Frozen initially
      RigidBody::new().with_gravity_scale(0.0).with_drag(0.0), // No gravity
      Collider::circle_collider(BALL_SIZE / 2.0).with_offset(BALL_SIZE / 2.0, BALL_SIZE / 2.0),
      SpriteRenderer::new(Sprite::solid_rect(
        BALL_SIZE as u32,
        BALL_SIZE as u32,
        0xff,
        0xff,
        0xff,
        0xff,
      ))
      .with_z_order(15),
      Ball { frozen: true },
    ));
    self.ball = Some(ball);
    self.game_started = false;
  }

  fn spawn_walls(&mut self, ctx: &mut SceneContext) {
    // Left wall
    let left = ctx.world.spawn((
      Transform::new(-WALL_THICKNESS, 0.0),
      RigidBody::kinematic(),
      Collider::box_collider(WALL_THICKNESS, SCREEN_HEIGHT as f32),
      SpriteRenderer::new(Sprite::solid_rect(
        WALL_THICKNESS as u32,
        SCREEN_HEIGHT,
        0x30,
        0x30,
        0x40,
        0xff,
      ))
      .with_z_order(0),
      Wall,
    ));
    self.walls.push(left);

    // Right wall
    let right = ctx.world.spawn((
      Transform::new(SCREEN_WIDTH as f32, 0.0),
      RigidBody::kinematic(),
      Collider::box_collider(WALL_THICKNESS, SCREEN_HEIGHT as f32),
      SpriteRenderer::new(Sprite::solid_rect(
        WALL_THICKNESS as u32,
        SCREEN_HEIGHT,
        0x30,
        0x30,
        0x40,
        0xff,
      ))
      .with_z_order(0),
      Wall,
    ));
    self.walls.push(right);

    // Top wall (ceiling)
    let ceiling = ctx.world.spawn((
      Transform::new(0.0, -WALL_THICKNESS),
      RigidBody::kinematic(),
      Collider::box_collider(SCREEN_WIDTH as f32, WALL_THICKNESS),
      SpriteRenderer::new(Sprite::solid_rect(
        SCREEN_WIDTH,
        WALL_THICKNESS as u32,
        0x30,
        0x30,
        0x40,
        0xff,
      ))
      .with_z_order(0),
      Wall,
    ));
    self.walls.push(ceiling);

    // Death zone (below screen)
    let death = ctx.world.spawn((
      Transform::new(0.0, SCREEN_HEIGHT as f32 + 50.0),
      RigidBody::kinematic(),
      Collider::box_collider(SCREEN_WIDTH as f32, 100.0).as_trigger(),
      DeathZone,
    ));
    self.death_zone = Some(death);
  }

  fn spawn_blocks(&mut self, ctx: &mut SceneContext) {
    // Load level file
    let path = level_path(self.level);
    let level_data = match load_level(Path::new(&path)) {
      Ok(data) => data,
      Err(e) => {
        eprintln!("Failed to load level {}: {}", self.level, e);
        // Fallback: loop back to level 1
        match load_level(Path::new(&level_path(1))) {
          Ok(data) => data,
          Err(_) => return, // No levels available
        }
      }
    };

    for (x, y, color, hits, points) in level_data.blocks {
      let (r, g, b, a) = color.to_rgba();

      let block = ctx.world.spawn((
        Transform::new(x, y),
        RigidBody::kinematic(),
        Collider::box_collider(BLOCK_WIDTH, BLOCK_HEIGHT),
        SpriteRenderer::new(Sprite::solid_rect(
          BLOCK_WIDTH as u32,
          BLOCK_HEIGHT as u32,
          r,
          g,
          b,
          a,
        ))
        .with_z_order(5),
        Block::new(color, hits, points),
      ));
      self.blocks.push(block);
    }

    self.blocks_remaining = self.blocks.len() as u32;
  }

  fn launch_ball(&mut self, ctx: &mut SceneContext) {
    if let Some(ball) = self.ball
      && let Some(mut ball_comp) = ctx.world.get_component_mut::<Ball>(ball)
      && ball_comp.frozen
    {
      ball_comp.frozen = false;

      // Set initial velocity - slightly angled upward
      if let Some(mut vel) = ctx.world.get_component_mut::<Velocity>(ball) {
        // Angle varies based on score for variety
        let angle_offset = (self.score as f32 * 0.1).sin() * 0.5;
        vel.dx = BALL_SPEED * angle_offset;
        vel.dy = -BALL_SPEED;
      }

      self.game_started = true;
    }
  }

  fn reset_ball(&mut self, ctx: &mut SceneContext) {
    // Remove old ball
    if let Some(ball) = self.ball.take() {
      let _ = ctx.world.despawn(ball);
    }

    // Reset paddle to center
    if let Some(paddle) = self.paddle
      && let Some(mut transform) = ctx.world.get_component_mut::<Transform>(paddle)
    {
      transform.x = SCREEN_WIDTH as f32 / 2.0 - PADDLE_WIDTH / 2.0;
    }

    // Spawn new ball
    self.spawn_ball(ctx);
  }

  fn handle_ball_death(&mut self, ctx: &mut SceneContext) {
    // Play death sound
    if let Some(audio) = ctx.audio.as_mut() {
      let _ = audio.play_sound_with_volume("ball_death", 0.5);
    }

    self.lives = self.lives.saturating_sub(1);

    if self.lives == 0 {
      // Game over - update high score
      if self.score > self.high_score {
        self.high_score = self.score;
        save_high_score(self.score);
      }
    } else {
      // Reset ball for next attempt
      self.reset_ball(ctx);
    }
  }

  fn handle_block_hit(&mut self, ctx: &mut SceneContext, block: Entity) {
    // Bounce the ball off the block
    if let Some(ball) = self.ball
      && let (Some(ball_transform), Some(block_transform)) = (
        ctx.world.get_component::<Transform>(ball),
        ctx.world.get_component::<Transform>(block),
      )
    {
      // Determine bounce direction based on collision position
      let ball_cx = ball_transform.x + BALL_SIZE / 2.0;
      let ball_cy = ball_transform.y + BALL_SIZE / 2.0;
      let block_cx = block_transform.x + BLOCK_WIDTH / 2.0;
      let block_cy = block_transform.y + BLOCK_HEIGHT / 2.0;

      // Calculate overlap to determine which side was hit
      let dx = (ball_cx - block_cx).abs();
      let dy = (ball_cy - block_cy).abs();
      let overlap_x = (BALL_SIZE / 2.0 + BLOCK_WIDTH / 2.0) - dx;
      let overlap_y = (BALL_SIZE / 2.0 + BLOCK_HEIGHT / 2.0) - dy;

      if let Some(mut vel) = ctx.world.get_component_mut::<Velocity>(ball) {
        // Reflect based on which axis has less penetration
        if overlap_x < overlap_y {
          vel.dx = -vel.dx; // Side hit - reflect horizontally
        } else {
          vel.dy = -vel.dy; // Top/bottom hit - reflect vertically
        }
      }
    }

    // Extract data from block component
    let block_info = {
      if let Some(mut block_comp) = ctx.world.get_component_mut::<Block>(block) {
        block_comp.hits = block_comp.hits.saturating_sub(1);
        Some((block_comp.points, block_comp.hits, block_comp.color))
      } else {
        None
      }
    };

    if let Some((points, remaining_hits, color)) = block_info {
      if remaining_hits == 0 {
        // Play block break sound
        if let Some(audio) = ctx.audio.as_mut() {
          let _ = audio.play_sound_with_volume("block_break", 0.15);
        }

        // Destroy block
        self.score += points;
        self.blocks_remaining = self.blocks_remaining.saturating_sub(1);
        self.blocks.retain(|&b| b != block);
        let _ = ctx.world.despawn(block);

        // Update high score
        if self.score > self.high_score {
          self.high_score = self.score;
        }
      } else {
        // Update block appearance to show damage
        let (r, g, b, a) = color.dimmed();
        if let Some(mut sprite) = ctx.world.get_component_mut::<SpriteRenderer>(block) {
          sprite.sprite = Sprite::solid_rect(BLOCK_WIDTH as u32, BLOCK_HEIGHT as u32, r, g, b, a);
        }
      }
    }
  }

  fn check_level_complete(&mut self, ctx: &mut SceneContext) -> bool {
    if self.blocks_remaining == 0 {
      // Level complete! Save high score and advance
      save_high_score(self.high_score);
      self.level += 1;

      // Clear blocks and spawn new level
      ctx.world.clear();
      self.paddle = None;
      self.ball = None;
      self.blocks.clear();
      self.walls.clear();
      self.death_zone = None;

      true
    } else {
      false
    }
  }
}

impl Scene for GameScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.world.clear();
    ctx
      .camera
      .set_position(SCREEN_WIDTH as f32 / 2.0, SCREEN_HEIGHT as f32 / 2.0);

    // Load audio files (gracefully handle missing files)
    if let Some(audio) = ctx.audio.as_mut() {
      let _ = audio.load_sound("paddle_hit", "examples/breakout/assets/audio/bounce.wav");
      let _ = audio.load_sound("block_break", "examples/breakout/assets/audio/break.wav");
      let _ = audio.load_sound("ball_death", "examples/breakout/assets/audio/life-lost.wav");
    }

    self.spawn_walls(ctx);
    self.spawn_paddle(ctx);
    self.spawn_ball(ctx);
    self.spawn_blocks(ctx);
  }

  fn on_exit(&mut self, _ctx: &mut SceneContext) {
    // Save high score on exit
    save_high_score(self.high_score);
  }

  fn on_pause(&mut self, _ctx: &mut SceneContext) {
    // Pause logic handled by PauseScene
  }

  fn on_resume(&mut self, _ctx: &mut SceneContext) {
    // Wait for escape release before allowing pause again
    self.escape_was_released = false;
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    // Track escape key release (needed after resuming from pause)
    if !self.escape_was_released && !ctx.input.key_down(KeyCode::Escape) {
      self.escape_was_released = true;
    }

    // Pause (only if escape was released since last pause)
    if self.escape_was_released && ctx.input.key_pressed(KeyCode::Escape) {
      self.escape_was_released = false;
      return SceneAction::Push(Box::new(PauseScene::new(self.score, self.high_score)));
    }

    // Check for game over
    if self.lives == 0 {
      if !self.game_over {
        // First frame of game over - save score and set flag
        self.game_over = true;
        if self.score > self.high_score {
          self.high_score = self.score;
          save_high_score(self.score);
        }
      } else {
        // Wait for any input to return to menu
        let any_key_pressed = ctx.input.key_pressed(KeyCode::Space)
          || ctx.input.key_pressed(KeyCode::Enter)
          || ctx.input.mouse_pressed(MouseButton::Left)
          || ctx.input.key_pressed(KeyCode::Escape);

        if any_key_pressed {
          return SceneAction::SwitchWith(
            Box::new(MenuScene::new()),
            Transition::fade(Duration::from_millis(500)),
          );
        }
      }
      return SceneAction::Continue;
    }

    // Check for level complete
    if self.check_level_complete(ctx) {
      // Re-enter to spawn new level
      self.spawn_walls(ctx);
      self.spawn_paddle(ctx);
      self.spawn_ball(ctx);
      self.spawn_blocks(ctx);
      return SceneAction::Continue;
    }

    // Launch ball on any key or click
    if !self.game_started {
      let any_key_pressed = ctx.input.key_pressed(KeyCode::Space)
        || ctx.input.key_pressed(KeyCode::Enter)
        || ctx.input.mouse_pressed(MouseButton::Left)
        || ctx.input.key_pressed(KeyCode::KeyW)
        || ctx.input.key_pressed(KeyCode::ArrowUp);

      if any_key_pressed {
        self.launch_ball(ctx);
      }
    }

    // Paddle movement
    if let Some(paddle) = self.paddle
      && let Some(mut transform) = ctx.world.get_component_mut::<Transform>(paddle)
    {
      if ctx.input.key_down(KeyCode::KeyA) || ctx.input.key_down(KeyCode::ArrowLeft) {
        transform.x -= PADDLE_SPEED * dt;
      }
      if ctx.input.key_down(KeyCode::KeyD) || ctx.input.key_down(KeyCode::ArrowRight) {
        transform.x += PADDLE_SPEED * dt;
      }

      // Clamp to screen bounds
      transform.x = transform.x.clamp(0.0, SCREEN_WIDTH as f32 - PADDLE_WIDTH);
    }

    // Ball physics - maintain constant speed
    if let Some(ball) = self.ball
      && let Some(ball_comp) = ctx.world.get_component::<Ball>(ball)
      && !ball_comp.frozen
      && let Some(mut vel) = ctx.world.get_component_mut::<Velocity>(ball)
    {
      // Normalize velocity to maintain constant speed
      let speed = (vel.dx * vel.dx + vel.dy * vel.dy).sqrt();
      if speed > 0.0 && (speed - BALL_SPEED).abs() > 1.0 {
        vel.dx = vel.dx / speed * BALL_SPEED;
        vel.dy = vel.dy / speed * BALL_SPEED;
      }

      // Ensure ball doesn't get stuck going horizontally
      if vel.dy.abs() < 50.0 {
        vel.dy = if vel.dy >= 0.0 { 50.0 } else { -50.0 };
        let speed = (vel.dx * vel.dx + vel.dy * vel.dy).sqrt();
        vel.dx = vel.dx / speed * BALL_SPEED;
        vel.dy = vel.dy / speed * BALL_SPEED;
      }
    }

    // Process collision events
    for event in ctx.collision_events {
      if event.event_type != CollisionEventType::Started {
        continue;
      }

      // Check for ball collisions
      if let Some(ball) = self.ball
        && event.involves(ball)
        && let Some(other) = event.other(ball)
      {
        // Ball hit death zone
        if self.death_zone == Some(other) {
          self.handle_ball_death(ctx);
          continue;
        }

        // Ball hit paddle - add spin based on hit position
        if self.paddle == Some(other) {
          // Play paddle hit sound
          if let Some(audio) = ctx.audio.as_mut() {
            let _ = audio.play_sound_with_volume("paddle_hit", 0.15);
          }

          if let (Some(ball_transform), Some(paddle_transform)) = (
            ctx.world.get_component::<Transform>(ball),
            ctx.world.get_component::<Transform>(other),
          ) {
            let ball_center = ball_transform.x + BALL_SIZE / 2.0;
            let paddle_center = paddle_transform.x + PADDLE_WIDTH / 2.0;
            let offset = (ball_center - paddle_center) / (PADDLE_WIDTH / 2.0);

            if let Some(mut vel) = ctx.world.get_component_mut::<Velocity>(ball) {
              // Reflect and add spin
              vel.dy = -vel.dy.abs(); // Always go up
              vel.dx += offset * 200.0; // Add horizontal spin

              // Normalize speed
              let speed = (vel.dx * vel.dx + vel.dy * vel.dy).sqrt();
              vel.dx = vel.dx / speed * BALL_SPEED;
              vel.dy = vel.dy / speed * BALL_SPEED;
            }
          }
          continue;
        }

        // Ball hit block
        if self.blocks.contains(&other) {
          self.handle_block_hit(ctx, other);
          continue;
        }
      }
    }

    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) {
    let (screen_w, _screen_h) = ui.screen_size();

    // Score (top right)
    let score_text = format!("Score: {}", self.score);
    ui.draw_text_anchored(&score_text, Anchor::TopRight, -20.0, 10.0, Color::WHITE);

    // High score
    let high_text = format!("High: {}", self.high_score);
    ui.draw_text_anchored(&high_text, Anchor::TopRight, -20.0, 25.0, Color::YELLOW);

    // Lives (top left)
    let lives_text = format!("Lives: {}", self.lives);
    ui.draw_text_anchored(&lives_text, Anchor::TopLeft, 20.0, 10.0, Color::WHITE);

    // Level
    let level_text = format!("Level: {}", self.level);
    ui.draw_text_anchored(&level_text, Anchor::TopLeft, 20.0, 25.0, Color::CYAN);

    // "Press any key to launch" prompt
    if !self.game_started {
      ui.draw_text_anchored(
        "Press any key to launch!",
        Anchor::Center,
        0.0,
        100.0,
        Color::WHITE,
      );
    }

    // Game over overlay
    if self.lives == 0 {
      ui.draw_rect(
        Rect::new(0.0, 0.0, screen_w as f32, 720.0),
        Color::new(0, 0, 0, 180),
      );
      ui.draw_text_anchored("GAME OVER", Anchor::Center, 0.0, -50.0, Color::RED);
      let final_score = format!("Final Score: {}", self.score);
      ui.draw_text_anchored(&final_score, Anchor::Center, 0.0, -10.0, Color::WHITE);
      ui.draw_text_anchored(
        "Press any key to continue",
        Anchor::Center,
        0.0,
        30.0,
        Color::LIGHT_GRAY,
      );
    }
  }

  fn name(&self) -> &str {
    "GameScene"
  }
}
