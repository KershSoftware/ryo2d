//! Game Scene
//!
//! The main gameplay scene where the breakout game is played.

use std::path::Path;
use std::time::Duration;

use ryo2d::prelude::*;

use super::{MenuScene, PauseScene};
use crate::components::{Ball, Block, DeathZone, Paddle, Wall};
use crate::constants::*;
use crate::levels::{level_path, load_level};
use crate::save::save_high_score;

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
  balls: Vec<Entity>,
  blocks: Vec<Entity>,
  walls: Vec<Entity>,
  death_zone: Option<Entity>,

  // Block count for win detection
  blocks_remaining: u32,

  // Multi-ball tracking
  multiball_score_threshold: u32,
  ball_counter: u32,
  multiball_blocks: Vec<Entity>,
  glow_time: f32,

  // Deferred respawn flag (spawn ball next frame to avoid rendering issues)
  needs_respawn: bool,

  // Wait for escape release after resuming from pause
  escape_was_released: bool,

  // Background music
  music: Option<MusicHandle>,
  // Previous scene's music for crossfading
  prev_music: Option<MusicHandle>,

  // Debug mode flag
  debug_mode: bool,
  // Debounce level switching - wait for key release
  level_switch_q_released: bool,
  level_switch_e_released: bool,
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
      balls: Vec::new(),
      blocks: Vec::new(),
      walls: Vec::new(),
      death_zone: None,
      blocks_remaining: 0,
      multiball_score_threshold: 300,
      ball_counter: 0,
      multiball_blocks: Vec::new(),
      glow_time: 0.0,
      needs_respawn: false,
      escape_was_released: true,
      music: None,
      prev_music: None,
      debug_mode: false,
      level_switch_q_released: true,
      level_switch_e_released: true,
    }
  }

  /// Create a new game scene with music from previous scene for crossfading
  pub fn with_prev_music(high_score: u32, prev_music: Option<MusicHandle>) -> Self {
    Self {
      score: 0,
      high_score,
      lives: STARTING_LIVES,
      level: 1,
      game_started: false,
      game_over: false,
      paddle: None,
      balls: Vec::new(),
      blocks: Vec::new(),
      walls: Vec::new(),
      death_zone: None,
      blocks_remaining: 0,
      multiball_score_threshold: 300,
      ball_counter: 0,
      multiball_blocks: Vec::new(),
      glow_time: 0.0,
      needs_respawn: false,
      escape_was_released: true,
      music: None,
      prev_music,
      debug_mode: false,
      level_switch_q_released: true,
      level_switch_e_released: true,
    }
  }

  fn spawn_paddle(&mut self, ctx: &mut SceneContext) {
    let paddle = ctx.world.spawn((
      Transform::new(SCREEN_WIDTH as f32 / 2.0 - PADDLE_WIDTH / 2.0, PADDLE_Y),
      Velocity::zero(),
      RigidBody::kinematic(),
      Collider::box_collider(PADDLE_WIDTH, PADDLE_HEIGHT)
        .with_restitution(1.0) // Perfect bounce
        .with_friction(0.0), // No friction
      Renderable::sprite(SpriteHandle::from_sprite(
        Sprite::solid_rect(
          PADDLE_WIDTH as u32,
          PADDLE_HEIGHT as u32,
          0x40,
          0xc0,
          0xff,
          0xff,
        ),
        "paddle",
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

    // Use unique sprite ID for each ball to avoid caching issues
    self.ball_counter += 1;
    let sprite_id = format!("ball_{}", self.ball_counter);

    let ball = ctx.world.spawn((
      Transform::new(ball_x, ball_y),
      Velocity::zero(),                                        // Frozen initially
      RigidBody::new().with_gravity_scale(0.0).with_drag(0.0), // No gravity
      Collider::circle_collider(BALL_SIZE / 2.0)
        .with_offset(BALL_SIZE / 2.0, BALL_SIZE / 2.0)
        .with_restitution(1.0) // Perfect bounce
        .with_friction(0.0), // No friction
      Renderable::sprite(SpriteHandle::from_sprite(
        Sprite::solid_circle(BALL_SIZE as u32, 0xff, 0xff, 0xff, 0xff),
        sprite_id,
      ))
      .with_z_order(15),
      Ball { frozen: true },
    ));
    self.balls.push(ball);
    self.game_started = false;
  }

  /// Spawn an additional ball at a specific position (for multi-ball power-up)
  fn spawn_ball_at(&mut self, ctx: &mut SceneContext, x: f32, y: f32) {
    // Use unique sprite ID for each ball
    self.ball_counter += 1;
    let sprite_id = format!("ball_{}", self.ball_counter);

    let ball = ctx.world.spawn((
      Transform::new(x - BALL_SIZE / 2.0, y),
      Velocity::new(0.0, -BALL_SPEED), // Moving upward
      RigidBody::new().with_gravity_scale(0.0).with_drag(0.0),
      Collider::circle_collider(BALL_SIZE / 2.0)
        .with_offset(BALL_SIZE / 2.0, BALL_SIZE / 2.0)
        .with_restitution(1.0)
        .with_friction(0.0),
      Renderable::sprite(SpriteHandle::from_sprite(
        Sprite::solid_circle(BALL_SIZE as u32, 0xff, 0xff, 0xff, 0xff),
        sprite_id,
      ))
      .with_z_order(15),
      Ball { frozen: false },
    ));
    self.balls.push(ball);
  }

  /// Check if score crossed threshold and convert a random block to multi-ball
  fn try_spawn_multiball_block(&mut self, ctx: &mut SceneContext) {
    if self.score >= self.multiball_score_threshold && !self.blocks.is_empty() {
      // Pick a block based on score (deterministic but varied)
      let idx = (self.score as usize) % self.blocks.len();
      let block = self.blocks[idx];

      // Mark as multiball
      if let Some(mut block_comp) = ctx.world.get_component_mut::<Block>(block) {
        // Don't convert if already multiball
        if block_comp.is_multiball {
          // Try next threshold
          self.multiball_score_threshold += 300;
          return;
        }
        block_comp.is_multiball = true;
      } else {
        return; // Block doesn't exist, skip
      }

      // Change sprite to gold (will be animated each frame)
      if let Some(mut renderable) = ctx.world.get_component_mut::<Renderable>(block) {
        renderable.sprite = SpriteHandle::from_sprite(
          Sprite::solid_rect(BLOCK_WIDTH as u32, BLOCK_HEIGHT as u32, 0xff, 0xd7, 0x00, 0xff),
          "multiball_block",
        );
      }

      // Add a subtle elastic wiggle with slight rotation
      let _ = ctx.world.insert_one(
        block,
        Tween::new(0.25)
          .to_scale_x(1.02)
          .to_scale_y(1.02)
          .to_rotation_value(0.02)
          .with_easing(Ease::InOutElastic)
          .with_loop(TweenLoop::PingPong),
      );

      // Track this block for color pulse animation
      self.multiball_blocks.push(block);

      // Set next threshold
      self.multiball_score_threshold += 300;
    }
  }

  /// Update animated pulse effect on multiball blocks
  fn update_multiball_glow(&mut self, ctx: &mut SceneContext, dt: f32) {
    self.glow_time += dt;

    // Pulsing brightness: oscillate between gold and bright yellow/white
    let pulse = (self.glow_time * 5.0).sin(); // 5.0 = speed of pulse
    let brightness = 0.5 + pulse * 0.5; // 0.0 to 1.0

    // Interpolate between gold (0xff, 0xd7, 0x00) and bright white-gold (0xff, 0xff, 0xaa)
    let r = 0xff;
    let g = (0xd7 as f32 + (0xff - 0xd7) as f32 * brightness) as u8;
    let b = (0x00 as f32 + 0xaa as f32 * brightness) as u8;

    // Update sprite color on all multiball blocks
    for &block in &self.multiball_blocks {
      if let Some(mut renderable) = ctx.world.get_component_mut::<Renderable>(block) {
        renderable.sprite = SpriteHandle::from_sprite(
          Sprite::solid_rect(BLOCK_WIDTH as u32, BLOCK_HEIGHT as u32, r, g, b, 0xff),
          "multiball_block",
        );
      }
    }
  }

  fn spawn_walls(&mut self, ctx: &mut SceneContext) {
    // Left wall
    let left = ctx.world.spawn((
      Transform::new(-WALL_THICKNESS, 0.0),
      RigidBody::kinematic(),
      Collider::box_collider(WALL_THICKNESS, SCREEN_HEIGHT as f32)
        .with_restitution(1.0) // Perfect bounce
        .with_friction(0.0), // No friction
      Renderable::sprite(SpriteHandle::from_sprite(
        Sprite::solid_rect(WALL_THICKNESS as u32, SCREEN_HEIGHT, 0x30, 0x30, 0x40, 0xff),
        "wall_left",
      ))
      .with_z_order(0),
      Wall,
    ));
    self.walls.push(left);

    // Right wall
    let right = ctx.world.spawn((
      Transform::new(SCREEN_WIDTH as f32, 0.0),
      RigidBody::kinematic(),
      Collider::box_collider(WALL_THICKNESS, SCREEN_HEIGHT as f32)
        .with_restitution(1.0) // Perfect bounce
        .with_friction(0.0), // No friction
      Renderable::sprite(SpriteHandle::from_sprite(
        Sprite::solid_rect(WALL_THICKNESS as u32, SCREEN_HEIGHT, 0x30, 0x30, 0x40, 0xff),
        "wall_right",
      ))
      .with_z_order(0),
      Wall,
    ));
    self.walls.push(right);

    // Top wall (ceiling)
    let ceiling = ctx.world.spawn((
      Transform::new(0.0, -WALL_THICKNESS),
      RigidBody::kinematic(),
      Collider::box_collider(SCREEN_WIDTH as f32, WALL_THICKNESS)
        .with_restitution(1.0) // Perfect bounce
        .with_friction(0.0), // No friction
      Renderable::sprite(SpriteHandle::from_sprite(
        Sprite::solid_rect(SCREEN_WIDTH, WALL_THICKNESS as u32, 0x30, 0x30, 0x40, 0xff),
        "wall_ceiling",
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
        Collider::box_collider(BLOCK_WIDTH, BLOCK_HEIGHT)
          .with_restitution(1.0) // Perfect bounce
          .with_friction(0.0), // No friction
        Renderable::sprite(SpriteHandle::from_sprite(
          Sprite::solid_rect(BLOCK_WIDTH as u32, BLOCK_HEIGHT as u32, r, g, b, a),
          format!("block_{}_{}", x as u32, y as u32),
        ))
        .with_z_order(5),
        Block::new(color, hits, points),
      ));
      self.blocks.push(block);
    }

    self.blocks_remaining = self.blocks.len() as u32;
  }

  /// Clear all level entities (blocks, walls, ball, paddle)
  fn clear_level(&mut self, ctx: &mut SceneContext) {
    // Despawn all blocks
    for block in self.blocks.drain(..) {
      let _ = ctx.world.despawn(block);
    }

    // Despawn walls
    for wall in self.walls.drain(..) {
      let _ = ctx.world.despawn(wall);
    }

    // Despawn all balls
    for ball in self.balls.drain(..) {
      let _ = ctx.world.despawn(ball);
    }

    // Despawn paddle
    if let Some(paddle) = self.paddle.take() {
      let _ = ctx.world.despawn(paddle);
    }

    // Despawn death zone
    if let Some(death_zone) = self.death_zone.take() {
      let _ = ctx.world.despawn(death_zone);
    }

    self.blocks_remaining = 0;
    self.multiball_blocks.clear();
    self.game_started = false;
  }

  /// Load a specific level
  fn load_level(&mut self, ctx: &mut SceneContext, level: u32) {
    self.clear_level(ctx);
    self.level = level;
    self.spawn_walls(ctx);
    self.spawn_paddle(ctx);
    self.spawn_ball(ctx);
    self.spawn_blocks(ctx);
  }

  fn launch_ball(&mut self, ctx: &mut SceneContext) {
    // Launch the first frozen ball (should only be one at game start)
    for &ball in &self.balls {
      if let Some(mut ball_comp) = ctx.world.get_component_mut::<Ball>(ball)
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
        break;
      }
    }
  }

  fn reset_ball(&mut self, ctx: &mut SceneContext) {
    // Remove all existing balls
    for ball in self.balls.drain(..) {
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
      let _ = audio.play_sound_with("ball_death", SoundOptions::new().volume(0.5));
    }

    self.lives = self.lives.saturating_sub(1);

    if self.lives == 0 {
      // Game over - update high score
      if self.score > self.high_score {
        self.high_score = self.score;
        save_high_score(self.score);
      }
    } else {
      // Defer ball respawn to next frame to avoid rendering issues
      self.needs_respawn = true;
    }
  }

  fn handle_block_hit(&mut self, ctx: &mut SceneContext, ball: Entity, block: Entity) {
    // Bounce the ball off the block
    if let (Some(ball_transform), Some(block_transform)) = (
      ctx.world.get_component::<Transform>(ball),
      ctx.world.get_component::<Transform>(block),
    ) {
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
        Some((
          block_comp.points,
          block_comp.hits,
          block_comp.color,
          block_comp.is_multiball,
        ))
      } else {
        None
      }
    };

    if let Some((points, remaining_hits, color, is_multiball)) = block_info {
      if remaining_hits == 0 {
        // Play block break sound
        if let Some(audio) = ctx.audio.as_mut() {
          let _ = audio.play_sound_with("block_break", SoundOptions::new().volume(0.15));
        }

        // Get block position before despawning (for multiball spawn)
        let spawn_pos = if is_multiball {
          ctx
            .world
            .get_component::<Transform>(block)
            .map(|t| (t.x + BLOCK_WIDTH / 2.0, t.y + BLOCK_HEIGHT))
        } else {
          None
        };

        // Destroy block
        self.score += points;
        self.blocks_remaining = self.blocks_remaining.saturating_sub(1);
        self.blocks.retain(|&b| b != block);
        self.multiball_blocks.retain(|&b| b != block);
        let _ = ctx.world.despawn(block);

        // Spawn extra ball if this was a multiball block
        if let Some((x, y)) = spawn_pos {
          self.spawn_ball_at(ctx, x, y);
        }

        // Update high score
        if self.score > self.high_score {
          self.high_score = self.score;
        }

        // Check if we should spawn a new multiball block
        self.try_spawn_multiball_block(ctx);
      } else {
        // Update block appearance to show damage
        let (r, g, b, a) = color.dimmed();
        if let Some(mut renderable) = ctx.world.get_component_mut::<Renderable>(block) {
          renderable.sprite = SpriteHandle::from_sprite(
            Sprite::solid_rect(BLOCK_WIDTH as u32, BLOCK_HEIGHT as u32, r, g, b, a),
            "block_damaged",
          );
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
      self.balls.clear();
      self.blocks.clear();
      self.multiball_blocks.clear();
      self.walls.clear();
      self.death_zone = None;

      // Reset multiball threshold for new level
      self.multiball_score_threshold = self.score + 300;

      true
    } else {
      false
    }
  }
}

impl Scene for GameScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    // Check for debug mode from workspace config
    if let Ok(workspace) = Workspace::discover() {
      self.debug_mode = workspace.config().env_bool("debug_scene").unwrap_or(false);
    }

    ctx.world.clear();
    ctx
      .camera
      .set_position(SCREEN_WIDTH as f32 / 2.0, SCREEN_HEIGHT as f32 / 2.0);

    // Load audio files (gracefully handle missing files)
    if let Some(audio) = ctx.audio.as_mut() {
      let _ = audio.load_sound("paddle_hit", "assets/audio/bounce.wav");
      let _ = audio.load_sound("block_break", "assets/audio/break.wav");
      let _ = audio.load_sound("ball_death", "assets/audio/life-lost.wav");

      // Crossfade from previous music to game music if available
      if let Some(mut prev_music) = self.prev_music.take() {
        if let Ok(music) = audio.crossfade_music(
          &mut prev_music,
          "assets/audio/bgm.mp3",
          Duration::from_millis(1000),
        ) {
          self.music = Some(music);
        }
      } else {
        // No previous music, just start game music with auto-fade on scene exit
        if let Ok(music) = audio.play_music_with(
          "assets/audio/bgm.mp3",
          MusicOptions::new().fade_out_on_drop(Duration::from_millis(500)),
        ) {
          self.music = Some(music);
        }
      }
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
    // Handle deferred ball respawn (from previous frame's death)
    if self.needs_respawn {
      self.needs_respawn = false;
      self.reset_ball(ctx);
    }

    // Animate multiball block glow
    self.update_multiball_glow(ctx, dt);

    // Debug mode: Level switching with Q/E
    if self.debug_mode {
      // Track key releases for debouncing
      if !ctx.input.key_down(KeyCode::KeyQ) {
        self.level_switch_q_released = true;
      }
      if !ctx.input.key_down(KeyCode::KeyE) {
        self.level_switch_e_released = true;
      }

      // Only switch if key was previously released
      if ctx.input.key_pressed(KeyCode::KeyQ) && self.level_switch_q_released {
        self.level_switch_q_released = false;
        // Previous level (wrap to 10)
        let new_level = if self.level == 1 { 10 } else { self.level - 1 };
        self.load_level(ctx, new_level);
        return SceneAction::Continue;
      } else if ctx.input.key_pressed(KeyCode::KeyE) && self.level_switch_e_released {
        self.level_switch_e_released = false;
        // Next level (wrap to 1)
        let new_level = if self.level == 10 { 1 } else { self.level + 1 };
        self.load_level(ctx, new_level);
        return SceneAction::Continue;
      }
    }

    // Track escape key release (needed after resuming from pause)
    if !self.escape_was_released && !ctx.input.key_down(KeyCode::Escape) {
      self.escape_was_released = true;
    }

    // Pause (only if escape was released since last pause)
    if self.escape_was_released && ctx.input.key_pressed(KeyCode::Escape) {
      self.escape_was_released = false;
      return SceneAction::Push(Box::new(PauseScene::with_music(
        self.score,
        self.high_score,
        self.music.as_mut(),
      )));
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
          // Drop music handle (will fade out via fade_out_on_drop)
          // Don't pass to menu - let it stop cleanly
          self.music = None;
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

    // Ball physics - maintain constant speed for all balls
    for &ball in &self.balls {
      if let Some(ball_comp) = ctx.world.get_component::<Ball>(ball) {
        if ball_comp.frozen {
          continue;
        }
      } else {
        continue;
      }

      if let Some(mut vel) = ctx.world.get_component_mut::<Velocity>(ball) {
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
    }

    // Process collision events (only Started - when objects first touch)
    // Collect ball collision targets first to avoid borrow conflicts
    // Store (ball_entity, other_entity) pairs
    let ball_collisions: Vec<(Entity, Entity)> = ctx
      .events
      .read::<CollisionStarted>()
      .filter_map(|event| {
        // Find which ball was involved in this collision
        for &ball in &self.balls {
          if event.involves(ball) {
            return event.other(ball).map(|other| (ball, other));
          }
        }
        None
      })
      .collect();

    // Track balls that need to be removed (hit death zone)
    let mut balls_to_remove: Vec<Entity> = Vec::new();

    for (ball, other) in ball_collisions {
      // Ball hit death zone
      if self.death_zone == Some(other) {
        balls_to_remove.push(ball);
        continue;
      }

      // Ball hit paddle - add spin based on hit position
      if self.paddle == Some(other) {
        // Play paddle hit sound
        if let Some(audio) = ctx.audio.as_mut() {
          let _ = audio.play_sound_with("paddle_hit", SoundOptions::new().volume(0.15));
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
        self.handle_block_hit(ctx, ball, other);
        continue;
      }
    }

    // Remove dead balls from tracking (but don't despawn yet)
    for &ball in &balls_to_remove {
      self.balls.retain(|&b| b != ball);
    }

    // Check if all balls are lost
    if self.balls.is_empty() && self.game_started {
      self.handle_ball_death(ctx);
    }

    // Now despawn the dead ball entities (after new ball is spawned)
    for ball in balls_to_remove {
      let _ = ctx.world.despawn(ball);
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

    // Debug mode UI
    if self.debug_mode {
      ui.draw_text_anchored(
        "[DEBUG MODE]",
        Anchor::BottomLeft,
        20.0,
        -10.0,
        Color::new(255, 255, 0, 180),
      );
      ui.draw_text_anchored(
        "Q: Prev Level | E: Next Level",
        Anchor::BottomLeft,
        20.0,
        -25.0,
        Color::new(200, 200, 200, 180),
      );
    }
  }

  fn name(&self) -> &str {
    "GameScene"
  }
}
