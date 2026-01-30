//! Breakout - Complete Ryo2D Example Game
//!
//! This example demonstrates all major engine features:
//! - Scene management (MenuScene, GameScene, PauseScene)
//! - Immediate-mode UI system for menus and HUD
//! - ECS architecture for game entities
//! - Physics with Rapier2D
//! - Collision events for game logic
//! - Save system for high score persistence
//! - Audio for sound effects (paddle hit, block break, ball death)
//! - Level loading from external files
//!
//! Controls:
//! - Menu: Click "Start", Enter, or Space to begin
//! - Game: A/D or Arrow keys to move paddle
//! - Game: Any key or click to launch ball
//! - Pause: Escape to pause, Resume/Quit buttons

use std::fs;
use std::path::Path;
use std::time::Duration;

use ryo2d::prelude::*;

// ============================================================================
// Game Constants
// ============================================================================

const SCREEN_WIDTH: u32 = 1280;
const SCREEN_HEIGHT: u32 = 720;

const PADDLE_WIDTH: f32 = 120.0;
const PADDLE_HEIGHT: f32 = 16.0;
const PADDLE_Y: f32 = 650.0;
const PADDLE_SPEED: f32 = 600.0;

const BALL_SIZE: f32 = 12.0;
const BALL_SPEED: f32 = 400.0;

const BLOCK_WIDTH: f32 = 80.0;
const BLOCK_HEIGHT: f32 = 25.0;
const BLOCK_PADDING: f32 = 4.0;
const BLOCK_TOP_OFFSET: f32 = 60.0;
const BLOCK_LEFT_OFFSET: f32 = 120.0;

const WALL_THICKNESS: f32 = 20.0;

const STARTING_LIVES: u32 = 3;

// ============================================================================
// Components
// ============================================================================

/// Marker component for the paddle
#[derive(Debug, Clone)]
struct Paddle;

/// Marker component for the ball
#[derive(Debug, Clone)]
struct Ball {
  /// Whether the ball is currently frozen (waiting for launch)
  frozen: bool,
}

/// Marker component for blocks
#[derive(Debug, Clone)]
struct Block {
  /// Hits remaining before destruction
  hits: u32,
  /// Base color for this block
  color: BlockColor,
  /// Point value when destroyed
  points: u32,
}

/// Block colors
#[derive(Debug, Clone, Copy)]
enum BlockColor {
  Red,
  Orange,
  Yellow,
  Green,
  Blue,
  Purple,
  White,
  Silver,
}

impl BlockColor {
  fn to_rgba(self) -> (u8, u8, u8, u8) {
    match self {
      BlockColor::Red => (0xff, 0x44, 0x44, 0xff),
      BlockColor::Orange => (0xff, 0x88, 0x00, 0xff),
      BlockColor::Yellow => (0xff, 0xff, 0x00, 0xff),
      BlockColor::Green => (0x00, 0xcc, 0x44, 0xff),
      BlockColor::Blue => (0x44, 0x88, 0xff, 0xff),
      BlockColor::Purple => (0xaa, 0x44, 0xff, 0xff),
      BlockColor::White => (0xff, 0xff, 0xff, 0xff),
      BlockColor::Silver => (0xaa, 0xaa, 0xbb, 0xff),
    }
  }

  fn dimmed(self) -> (u8, u8, u8, u8) {
    let (r, g, b, a) = self.to_rgba();
    (
      (r as u16 * 2 / 3) as u8,
      (g as u16 * 2 / 3) as u8,
      (b as u16 * 2 / 3) as u8,
      a,
    )
  }

  fn from_char(c: char) -> Option<(BlockColor, u32, u32)> {
    // Returns (color, hits, points)
    match c {
      'R' => Some((BlockColor::Red, 1, 10)),
      'O' => Some((BlockColor::Orange, 1, 20)),
      'Y' => Some((BlockColor::Yellow, 1, 30)),
      'G' => Some((BlockColor::Green, 1, 40)),
      'B' => Some((BlockColor::Blue, 1, 50)),
      'P' => Some((BlockColor::Purple, 1, 60)),
      'W' => Some((BlockColor::White, 2, 100)),
      'S' => Some((BlockColor::Silver, 3, 200)),
      _ => None,
    }
  }
}

/// Marker for walls
#[derive(Debug, Clone)]
struct Wall;

/// Marker for the death zone (bottom of screen)
#[derive(Debug, Clone)]
struct DeathZone;

// ============================================================================
// Level Loading
// ============================================================================

struct LevelData {
  blocks: Vec<(f32, f32, BlockColor, u32, u32)>, // x, y, color, hits, points
}

fn load_level(path: &Path) -> Result<LevelData, String> {
  let content = fs::read_to_string(path).map_err(|e| format!("Failed to read level: {}", e))?;

  let mut blocks = Vec::new();
  let mut current_row = 0usize;

  for line in content.lines() {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
      continue;
    }

    for (col, c) in line.chars().enumerate() {
      if c == '.' {
        continue;
      }

      if let Some((color, hits, points)) = BlockColor::from_char(c) {
        let x = BLOCK_LEFT_OFFSET + col as f32 * (BLOCK_WIDTH + BLOCK_PADDING);
        let y = BLOCK_TOP_OFFSET + current_row as f32 * (BLOCK_HEIGHT + BLOCK_PADDING);
        blocks.push((x, y, color, hits, points));
      }
    }
    current_row += 1;
  }

  Ok(LevelData { blocks })
}

// ============================================================================
// High Score Persistence
// ============================================================================

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct HighScoreData {
  high_score: u32,
}

fn load_high_score() -> u32 {
  // Try to load from save file
  if let Ok(data) = SaveManager::new(Path::new("examples/breakout/data"))
    .and_then(|sm| sm.load_game::<HighScoreData>("highscore"))
  {
    return data.data.high_score;
  }
  0
}

fn save_high_score(score: u32) {
  if let Ok(save_manager) = SaveManager::new(Path::new("examples/breakout/data")) {
    let data = HighScoreData { high_score: score };
    let _ = save_manager.save_game("highscore", &data);
  }
}

// ============================================================================
// Menu Scene
// ============================================================================

struct MenuScene {
  high_score: u32,
  // Pending action from button click (set in render_ui, consumed in update)
  start_clicked: bool,
  // Wait for escape to be released before accepting input
  escape_was_released: bool,
}

impl MenuScene {
  fn new() -> Self {
    Self {
      high_score: load_high_score(),
      start_clicked: false,
      escape_was_released: false,
    }
  }
}

impl Scene for MenuScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.world.clear();
    ctx
      .camera
      .set_position(SCREEN_WIDTH as f32 / 2.0, SCREEN_HEIGHT as f32 / 2.0);
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    // Wait for Escape to be released before accepting new Escape presses
    if !self.escape_was_released && !ctx.input.key_down(KeyCode::Escape) {
      self.escape_was_released = true;
    }

    // Start game on Enter, Space, or button click
    if ctx.input.key_pressed(KeyCode::Enter)
      || ctx.input.key_pressed(KeyCode::Space)
      || self.start_clicked
    {
      self.start_clicked = false;
      return SceneAction::SwitchWith(
        Box::new(GameScene::new(self.high_score)),
        Transition::fade(Duration::from_millis(300)),
      );
    }

    // ESC to open quit confirmation (only after released once)
    if self.escape_was_released && ctx.input.key_pressed(KeyCode::Escape) {
      self.escape_was_released = false; // Reset for when we return from overlay
      return SceneAction::Push(Box::new(QuitConfirmScene::new()));
    }

    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) {
    let (screen_w, screen_h) = ui.screen_size();

    // Background
    ui.draw_rect(
      Rect::new(0.0, 0.0, screen_w as f32, screen_h as f32),
      Color::rgb(20, 20, 35),
    );

    // Title
    ui.draw_text_anchored("BREAKOUT", Anchor::TopCenter, 0.0, 100.0, Color::WHITE);

    // Subtitle
    ui.draw_text_anchored(
      "A Ryo2D Engine Demo",
      Anchor::TopCenter,
      0.0,
      130.0,
      Color::GRAY,
    );

    // High score
    let high_score_text = format!("High Score: {}", self.high_score);
    ui.draw_text_anchored(
      &high_score_text,
      Anchor::TopCenter,
      0.0,
      180.0,
      Color::YELLOW,
    );

    // Decorative blocks
    let colors = [
      Color::hex(0xff4444),
      Color::hex(0xff8800),
      Color::hex(0xffff00),
      Color::hex(0x00cc44),
      Color::hex(0x4488ff),
      Color::hex(0xaa44ff),
    ];

    for (i, color) in colors.iter().enumerate() {
      let x = screen_w as f32 / 2.0 - 180.0 + i as f32 * 65.0;
      ui.draw_rect(Rect::new(x, 250.0, 60.0, 25.0), *color);
    }

    // Start button area
    let button_rect = Rect::new(screen_w as f32 / 2.0 - 100.0, 350.0, 200.0, 50.0);

    if ui.button("START GAME", button_rect) {
      self.start_clicked = true;
    }

    // Instructions
    ui.draw_text_anchored(
      "Press ENTER or SPACE to Start",
      Anchor::TopCenter,
      0.0,
      450.0,
      Color::LIGHT_GRAY,
    );

    ui.draw_text_anchored(
      "ESC to Quit",
      Anchor::BottomCenter,
      0.0,
      -30.0,
      Color::DARK_GRAY,
    );
  }

  fn name(&self) -> &str {
    "MenuScene"
  }
}

// ============================================================================
// Game Scene
// ============================================================================

struct GameScene {
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
  fn new(high_score: u32) -> Self {
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
    let level_path = format!("examples/breakout/levels/level{}.txt", self.level);
    let level_data = match load_level(Path::new(&level_path)) {
      Ok(data) => data,
      Err(e) => {
        eprintln!("Failed to load level {}: {}", self.level, e);
        // Fallback: loop back to level 1
        match load_level(Path::new("examples/breakout/levels/level1.txt")) {
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
        Block {
          hits,
          color,
          points,
        },
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
        // Random angle between -30 and 30 degrees from vertical
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
      // Game over - update high score and return to menu
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

// ============================================================================
// Pause Scene
// ============================================================================

struct PauseScene {
  score: u32,
  high_score: u32,
  // Wait for escape to be released before accepting input
  escape_was_released: bool,
  // Pending actions from button clicks (set in render_ui, consumed in update)
  resume_clicked: bool,
  quit_clicked: bool,
  quit_desktop_clicked: bool,
}

impl PauseScene {
  fn new(score: u32, high_score: u32) -> Self {
    Self {
      score,
      high_score,
      escape_was_released: false,
      resume_clicked: false,
      quit_clicked: false,
      quit_desktop_clicked: false,
    }
  }
}

impl Scene for PauseScene {
  fn on_enter(&mut self, _ctx: &mut SceneContext) {
    // Pause overlay - no world changes needed
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    // Wait for Escape to be released before accepting new Escape presses
    // This prevents the same keypress that opened the menu from closing it
    if !self.escape_was_released && !ctx.input.key_down(KeyCode::Escape) {
      self.escape_was_released = true;
      // Still check for button clicks and Enter key while waiting
    }

    // Check for button clicks from previous render_ui
    if self.resume_clicked {
      self.resume_clicked = false;
      return SceneAction::Pop;
    }

    if self.quit_clicked {
      self.quit_clicked = false;
      save_high_score(self.high_score);
      return SceneAction::ResetWith(
        Box::new(MenuScene::new()),
        Transition::fade(Duration::from_millis(300)),
      );
    }

    if self.quit_desktop_clicked {
      self.quit_desktop_clicked = false;
      save_high_score(self.high_score);
      return SceneAction::Quit;
    }

    // Resume on Enter (always) or Escape (only after released once)
    if ctx.input.key_pressed(KeyCode::Enter) {
      return SceneAction::Pop;
    }
    if self.escape_was_released && ctx.input.key_pressed(KeyCode::Escape) {
      return SceneAction::Pop;
    }

    // Return to menu via Q key
    if ctx.input.key_pressed(KeyCode::KeyQ) {
      // Save high score before quitting
      save_high_score(self.high_score);
      return SceneAction::ResetWith(
        Box::new(MenuScene::new()),
        Transition::fade(Duration::from_millis(300)),
      );
    }

    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) {
    let (screen_w, screen_h) = ui.screen_size();

    // Dim overlay
    ui.draw_rect(
      Rect::new(0.0, 0.0, screen_w as f32, screen_h as f32),
      Color::new(0, 0, 0, 180),
    );

    // Pause panel
    let panel_w = 300.0;
    let panel_h = 310.0;
    let panel_x = (screen_w as f32 - panel_w) / 2.0;
    let panel_y = (screen_h as f32 - panel_h) / 2.0;

    ui.draw_panel(
      Rect::new(panel_x, panel_y, panel_w, panel_h),
      Color::rgb(30, 30, 50),
      Some(Color::WHITE),
    );

    // Title
    ui.draw_text_centered(
      "PAUSED",
      Rect::new(panel_x, panel_y + 20.0, panel_w, 30.0),
      Color::WHITE,
    );

    // Score
    let score_text = format!("Score: {}", self.score);
    ui.draw_text_centered(
      &score_text,
      Rect::new(panel_x, panel_y + 60.0, panel_w, 20.0),
      Color::YELLOW,
    );

    // Resume button
    let resume_rect = Rect::new(panel_x + 50.0, panel_y + 110.0, 200.0, 40.0);
    if ui.button("RESUME", resume_rect) {
      self.resume_clicked = true;
    }

    // Quit to menu button
    let quit_rect = Rect::new(panel_x + 50.0, panel_y + 160.0, 200.0, 40.0);
    if ui.button("QUIT TO MENU", quit_rect) {
      self.quit_clicked = true;
    }

    // Quit to desktop button
    let quit_desktop_rect = Rect::new(panel_x + 50.0, panel_y + 210.0, 200.0, 40.0);
    if ui.button("QUIT TO DESKTOP", quit_desktop_rect) {
      self.quit_desktop_clicked = true;
    }
  }

  fn is_transparent(&self) -> bool {
    true // Show game behind pause overlay
  }

  fn name(&self) -> &str {
    "PauseScene"
  }
}

// ============================================================================
// Quit Confirmation Scene
// ============================================================================

struct QuitConfirmScene {
  // Wait for escape to be released before accepting input
  escape_was_released: bool,
  // Pending actions from button clicks (set in render_ui, consumed in update)
  yes_clicked: bool,
  no_clicked: bool,
}

impl QuitConfirmScene {
  fn new() -> Self {
    Self {
      escape_was_released: false,
      yes_clicked: false,
      no_clicked: false,
    }
  }
}

impl Scene for QuitConfirmScene {
  fn on_enter(&mut self, _ctx: &mut SceneContext) {
    // Overlay - no world changes needed
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    // Wait for Escape to be released before accepting new Escape presses
    if !self.escape_was_released && !ctx.input.key_down(KeyCode::Escape) {
      self.escape_was_released = true;
    }

    // Check for button clicks from previous render_ui
    if self.yes_clicked {
      self.yes_clicked = false;
      return SceneAction::Quit;
    }

    if self.no_clicked {
      self.no_clicked = false;
      return SceneAction::Pop;
    }

    // N to cancel (always works)
    if ctx.input.key_pressed(KeyCode::KeyN) {
      return SceneAction::Pop;
    }

    // ESC to cancel (only after released once)
    if self.escape_was_released && ctx.input.key_pressed(KeyCode::Escape) {
      return SceneAction::Pop;
    }

    // Y to confirm quit
    if ctx.input.key_pressed(KeyCode::KeyY) {
      return SceneAction::Quit;
    }

    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) {
    let (screen_w, screen_h) = ui.screen_size();

    // Dim overlay
    ui.draw_rect(
      Rect::new(0.0, 0.0, screen_w as f32, screen_h as f32),
      Color::new(0, 0, 0, 180),
    );

    // Confirmation panel
    let panel_w = 350.0;
    let panel_h = 180.0;
    let panel_x = (screen_w as f32 - panel_w) / 2.0;
    let panel_y = (screen_h as f32 - panel_h) / 2.0;

    ui.draw_panel(
      Rect::new(panel_x, panel_y, panel_w, panel_h),
      Color::rgb(30, 30, 50),
      Some(Color::WHITE),
    );

    // Title
    ui.draw_text_centered(
      "Quit to Desktop?",
      Rect::new(panel_x, panel_y + 30.0, panel_w, 30.0),
      Color::WHITE,
    );

    // Yes button
    let yes_rect = Rect::new(panel_x + 30.0, panel_y + 100.0, 140.0, 40.0);
    if ui.button("YES (Y)", yes_rect) {
      self.yes_clicked = true;
    }

    // No button
    let no_rect = Rect::new(panel_x + 180.0, panel_y + 100.0, 140.0, 40.0);
    if ui.button("NO (N)", no_rect) {
      self.no_clicked = true;
    }
  }

  fn is_transparent(&self) -> bool {
    true // Show menu behind overlay
  }

  fn name(&self) -> &str {
    "QuitConfirmScene"
  }
}

// ============================================================================
// Main
// ============================================================================

fn main() {
  let engine = EngineBuilder::new()
    .title("Breakout - Ryo2D Demo")
    .resolution(SCREEN_WIDTH, SCREEN_HEIGHT)
    .fps(60)
    .physics_hz(120) // Higher physics rate for better collision detection
    .with_scene(MenuScene::new())
    .build();

  if let Err(e) = engine.run() {
    eprintln!("Engine error: {}", e);
  }
}
