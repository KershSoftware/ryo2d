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
use crate::look::{spawn_ui_file, text_style};
use crate::save::save_high_score;

const HUD: &str = "assets/ui/hud.ui.ron";
const HUD_EMBEDDED: &str = include_str!("../../assets/ui/hud.ui.ron");

/// Main gameplay scene
pub struct GameScene {
  // Game state
  score: u32,
  high_score: u32,
  lives: u32,
  level: u32,
  game_started: bool,
  game_over: bool,
  game_won: bool,

  /// The HUD's tree, when its file loaded
  hud: Option<Entity>,

  // Entity tracking
  paddle: Option<Entity>,
  balls: Vec<Entity>,
  blocks: Vec<Entity>,
  walls: Vec<Entity>,
  death_zone: Option<Entity>,

  // Block count for win detection
  blocks_remaining: u32,

  // Multi-ball tracking (time-based)
  ball_counter: u32,
  multiball_blocks: Vec<Entity>,
  glow_time: f32,
  /// Time survived on current ball (resets on life lost)
  survival_time: f32,
  /// Cooldown after spawning a multiball block (prevents rapid spawning)
  multiball_cooldown: f32,

  // Heart block tracking (1000 point milestones)
  heart_blocks: Vec<Entity>,
  /// Last 1000-point milestone crossed (1000, 2000, 3000, etc.)
  last_heart_milestone: u32,

  // Paddle physics
  /// Current paddle velocity (with acceleration)
  paddle_velocity: f32,

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

  // "+1" life animation
  /// Time remaining for "+1" animation (0 = not showing)
  plus_one_timer: f32,

  // Background entity (full-screen solid color, z_order = -10)
  background: Option<Entity>,
}

impl GameScene {
  /// Create a new game scene, optionally with music from a previous scene for crossfading
  pub fn new(high_score: u32, prev_music: Option<MusicHandle>) -> Self {
    Self {
      score: 0,
      high_score,
      lives: STARTING_LIVES,
      level: 1,
      game_started: false,
      game_over: false,
      game_won: false,
      hud: None,
      paddle: None,
      balls: Vec::new(),
      blocks: Vec::new(),
      walls: Vec::new(),
      death_zone: None,
      blocks_remaining: 0,
      ball_counter: 0,
      multiball_blocks: Vec::new(),
      glow_time: 0.0,
      survival_time: 0.0,
      multiball_cooldown: 0.0,
      heart_blocks: Vec::new(),
      last_heart_milestone: 0,
      paddle_velocity: 0.0,
      needs_respawn: false,
      escape_was_released: true,
      music: None,
      prev_music,
      debug_mode: false,
      level_switch_q_released: true,
      level_switch_e_released: true,
      plus_one_timer: 0.0,
      background: None,
    }
  }

  /// Create a bordered rectangle sprite
  fn bordered_rect(
    width: u32,
    height: u32,
    r: u8,
    g: u8,
    b: u8,
    border_color: (u8, u8, u8),
  ) -> Sprite {
    let mut pixels = vec![0u8; (width * height * 4) as usize];
    let (br, bg, bb) = border_color;

    for y in 0..height {
      for x in 0..width {
        let idx = ((y * width + x) * 4) as usize;
        // Check if on border (1 pixel border)
        let is_border = x == 0 || x == width - 1 || y == 0 || y == height - 1;

        if is_border {
          pixels[idx] = br;
          pixels[idx + 1] = bg;
          pixels[idx + 2] = bb;
          pixels[idx + 3] = 255;
        } else {
          pixels[idx] = r;
          pixels[idx + 1] = g;
          pixels[idx + 2] = b;
          pixels[idx + 3] = 255;
        }
      }
    }

    Sprite::from_rgba(pixels, width, height)
  }

  /// Create a health block sprite (white background with red plus sign and black outline)
  fn health_block_sprite(width: u32, height: u32, intensity: f32) -> Sprite {
    let mut pixels = vec![0u8; (width * height * 4) as usize];
    let (bg_r, bg_g, bg_b) = (0xee, 0xee, 0xf8); // Light grayish-white background
    let (br, b_g, bb) = (0xcc, 0x33, 0x33); // Red border

    let cx = width / 2;
    let cy = height / 2;

    // Plus sign dimensions
    let plus_thickness = 5u32;
    let plus_arm_length = 8u32;
    let outline_size = 1u32;

    for y in 0..height {
      for x in 0..width {
        let idx = ((y * width + x) * 4) as usize;
        let is_border = x == 0 || x == width - 1 || y == 0 || y == height - 1;

        if is_border {
          pixels[idx] = br;
          pixels[idx + 1] = b_g;
          pixels[idx + 2] = bb;
          pixels[idx + 3] = 255;
        } else {
          // Check if pixel is part of plus sign or its outline
          let dx = (x as i32 - cx as i32).unsigned_abs();
          let dy = (y as i32 - cy as i32).unsigned_abs();

          let in_horizontal = dy <= plus_thickness / 2 && dx <= plus_arm_length;
          let in_vertical = dx <= plus_thickness / 2 && dy <= plus_arm_length;

          // Outline is slightly larger than the plus
          let in_h_outline =
            dy <= plus_thickness / 2 + outline_size && dx <= plus_arm_length + outline_size;
          let in_v_outline =
            dx <= plus_thickness / 2 + outline_size && dy <= plus_arm_length + outline_size;
          let is_outline = (in_h_outline || in_v_outline) && !(in_horizontal || in_vertical);

          if is_outline {
            // Black outline
            pixels[idx] = 0x20;
            pixels[idx + 1] = 0x20;
            pixels[idx + 2] = 0x20;
            pixels[idx + 3] = 255;
          } else if in_horizontal || in_vertical {
            // Red plus fading to white based on intensity
            // intensity 1.0 = full red, intensity 0.0 = white
            let red = 0xff;
            let green = (0x22 as f32 + (0xff - 0x22) as f32 * (1.0 - intensity)) as u8;
            let blue = (0x22 as f32 + (0xff - 0x22) as f32 * (1.0 - intensity)) as u8;
            pixels[idx] = red;
            pixels[idx + 1] = green;
            pixels[idx + 2] = blue;
            pixels[idx + 3] = 255;
          } else {
            // Light background
            pixels[idx] = bg_r;
            pixels[idx + 1] = bg_g;
            pixels[idx + 2] = bg_b;
            pixels[idx + 3] = 255;
          }
        }
      }
    }

    Sprite::from_rgba(pixels, width, height)
  }

  fn spawn_paddle(&mut self, ctx: &mut SceneContext) {
    let paddle = ctx
      .world
      .spawn((
        Transform::from_xy(SCREEN_WIDTH as f32 / 2.0, PADDLE_Y),
        Velocity::zero(),
        RigidBody::kinematic(),
        Collider::rect(PADDLE_WIDTH, PADDLE_HEIGHT)
          .with_restitution(1.0) // Perfect bounce
          .with_friction(0.0), // No friction
        Renderable::sprite(SpriteHandle::from_sprite(
          Self::bordered_rect(
            PADDLE_WIDTH as u32,
            PADDLE_HEIGHT as u32,
            0x40,
            0xc0,
            0xff,
            (0x20, 0x60, 0x80), // Darker blue border
          ),
          "paddle",
        ))
        .with_z_order(10),
        Interpolated,
        Paddle,
      ))
      .id();
    self.paddle = Some(paddle);
  }

  fn spawn_ball(&mut self, ctx: &mut SceneContext) {
    // Ball spawns centred, 10px above the paddle
    let ball_x = SCREEN_WIDTH as f32 / 2.0;
    let ball_y = PADDLE_Y - PADDLE_HEIGHT / 2.0 - 10.0 - BALL_SIZE / 2.0;

    // Use unique sprite ID for each ball to avoid caching issues
    self.ball_counter += 1;
    let sprite_id = format!("ball_{}", self.ball_counter);

    let ball = ctx
      .world
      .spawn((
        Transform::from_xy(ball_x, ball_y),
        Velocity::zero(), // Frozen initially
        RigidBody::dynamic()
          .with_ccd()
          .with_gravity_scale(0.0)
          .with_linear_damping(0.0)
          .lock_rotation(), // CCD prevents tunneling
        Collider::circle(BALL_SIZE / 2.0)
          .with_restitution(1.0) // Perfect bounce
          .with_friction(0.0), // No friction
        Renderable::sprite(SpriteHandle::from_sprite(
          Sprite::solid_circle(BALL_SIZE as u32, 0xff, 0xff, 0xff, 0xff),
          sprite_id,
        ))
        .with_z_order(15),
        Interpolated,
        Ball { frozen: true },
      ))
      .id();
    self.balls.push(ball);
    self.game_started = false;
  }

  /// Spawn an additional ball centred at a position (for multi-ball power-up)
  fn spawn_ball_at(&mut self, ctx: &mut SceneContext, x: f32, y: f32) {
    // Use unique sprite ID for each ball
    self.ball_counter += 1;
    let sprite_id = format!("ball_{}", self.ball_counter);

    // Random angle using multiple varying factors for better randomness
    // Combine glow_time, survival_time, score, and ball_counter for pseudo-random angle
    let random_seed = self.glow_time * 17.3 + self.survival_time * 7.1 + self.score as f32 * 3.7;
    let angle_offset = (random_seed.sin()) * 0.6; // -0.6 to 0.6 (~35 degrees each way)

    let vel_x = BALL_SPEED * angle_offset;
    let vel_y = -(BALL_SPEED * BALL_SPEED - vel_x * vel_x).sqrt(); // Maintain constant speed

    let ball = ctx
      .world
      .spawn((
        Transform::from_xy(x, y),
        Velocity::new(vel_x, vel_y), // Moving upward with random angle
        RigidBody::dynamic()
          .with_ccd()
          .with_gravity_scale(0.0)
          .with_linear_damping(0.0)
          .lock_rotation(), // CCD prevents tunneling
        Collider::circle(BALL_SIZE / 2.0)
          .with_restitution(1.0)
          .with_friction(0.0),
        Renderable::sprite(SpriteHandle::from_sprite(
          Sprite::solid_circle(BALL_SIZE as u32, 0xff, 0xff, 0xff, 0xff),
          sprite_id,
        ))
        .with_z_order(15),
        Interpolated,
        Ball { frozen: false },
      ))
      .id();
    self.balls.push(ball);
  }

  /// Time-based multiball spawn: longer survival = higher chance
  /// Probability increases from 0% at 0s to ~15% at 30s survival
  fn try_spawn_multiball_block(&mut self, ctx: &mut SceneContext) {
    // Don't spawn if on cooldown or no blocks available
    if self.multiball_cooldown > 0.0 || self.blocks.is_empty() {
      return;
    }

    // Calculate spawn chance based on survival time
    // Starts at 0, reaches ~15% chance per block hit at 30s survival
    let survival_factor = (self.survival_time / MULTIBALL_SURVIVAL_MAX).min(1.0);
    let spawn_chance = MULTIBALL_BASE_CHANCE * survival_factor;

    // Random check (use score as pseudo-random seed for determinism)
    let pseudo_random = ((self.score as f32 * 7.13 + self.survival_time * 3.17).sin() + 1.0) / 2.0;
    if pseudo_random > spawn_chance {
      return;
    }

    // Find a block that isn't already special AND has only 1 hit remaining
    // (avoid multi-hit blocks becoming multiball, which would be confusing)
    let available_blocks: Vec<Entity> = self
      .blocks
      .iter()
      .filter(|&&b| {
        ctx
          .world
          .get::<Block>(b)
          .map(|block| !block.is_multiball && !block.is_heart && block.hits == 1)
          .unwrap_or(false)
      })
      .copied()
      .collect();

    if available_blocks.is_empty() {
      return;
    }

    // Pick a block (use score for variety)
    let idx = (self.score as usize) % available_blocks.len();
    let block = available_blocks[idx];

    // Mark as multiball - ONLY proceed if we successfully set the flag
    let flag_set = if let Some(mut block_comp) = ctx.world.get_mut::<Block>(block) {
      block_comp.is_multiball = true;
      true
    } else {
      false
    };

    // Only change visuals and track if the flag was actually set
    if flag_set {
      // Change sprite to gold with border (will be animated with glow pulse each frame)
      if let Some(mut renderable) = ctx.world.get_mut::<Renderable>(block) {
        renderable.sprite = SpriteHandle::from_sprite(
          Self::bordered_rect(
            BLOCK_WIDTH as u32,
            BLOCK_HEIGHT as u32,
            0xff,
            0xd7,
            0x00,
            (0x80, 0x6b, 0x00), // Dark gold border
          ),
          format!("multiball_block_{}", self.ball_counter),
        );
      }

      // Track this block for color pulse animation
      self.multiball_blocks.push(block);

      // Set cooldown (5 seconds before another multiball can spawn)
      self.multiball_cooldown = MULTIBALL_COOLDOWN;
    }
  }

  /// Update animated pulse effect on multiball blocks
  fn update_multiball_glow(&mut self, ctx: &mut SceneContext, dt: f32) {
    self.glow_time += dt;

    // Faster, more dramatic color pulse: oscillate between gold and bright white
    let pulse = (self.glow_time * MULTIBALL_GLOW_SPEED).sin();
    let brightness = 0.5 + pulse * 0.5; // 0.0 to 1.0

    // Interpolate between deep gold (0xff, 0xaa, 0x00) and bright white (0xff, 0xff, 0xff)
    let r = 0xff;
    let g = (0xaa as f32 + (0xff - 0xaa) as f32 * brightness) as u8;
    let b = (0x00 as f32 + 0xff as f32 * brightness) as u8;

    // Update sprite color on all multiball blocks (use index for unique sprite IDs)
    // Use darker border based on brightness
    let border_g = (0x55 as f32 + (0x80 - 0x55) as f32 * brightness) as u8;
    let border_b = (0x00 as f32 + 0x80 as f32 * brightness) as u8;
    for (i, &block) in self.multiball_blocks.iter().enumerate() {
      if let Some(mut renderable) = ctx.world.get_mut::<Renderable>(block) {
        renderable.sprite = SpriteHandle::from_sprite(
          Self::bordered_rect(
            BLOCK_WIDTH as u32,
            BLOCK_HEIGHT as u32,
            r,
            g,
            b,
            (0x80, border_g, border_b),
          ),
          format!("multiball_glow_{}", i),
        );
      }
    }

    // Pulse health blocks with smooth fade in/out
    let fade_cycle = (self.glow_time * HEART_FADE_SPEED).sin();
    let plus_alpha = 0.5 + fade_cycle * 0.5; // 0.0 to 1.0

    for (i, &block) in self.heart_blocks.iter().enumerate() {
      if let Some(mut renderable) = ctx.world.get_mut::<Renderable>(block) {
        renderable.sprite = SpriteHandle::from_sprite(
          Self::health_block_sprite(BLOCK_WIDTH as u32, BLOCK_HEIGHT as u32, plus_alpha),
          format!("health_glow_{}", i),
        );
      }
    }
  }

  /// Try to spawn a heart block when crossing 1000 point milestones
  fn try_spawn_heart_block(&mut self, ctx: &mut SceneContext) {
    // Calculate current milestone (1000, 2000, 3000, etc.)
    let current_milestone = (self.score / 1000) * 1000;

    // Check if we crossed a new milestone
    if current_milestone > self.last_heart_milestone && current_milestone > 0 {
      self.last_heart_milestone = current_milestone;

      // Find a block that isn't already special (not multiball or heart)
      let available_blocks: Vec<Entity> = self
        .blocks
        .iter()
        .filter(|&&b| {
          ctx
            .world
            .get::<Block>(b)
            .map(|block| !block.is_multiball && !block.is_heart)
            .unwrap_or(false)
        })
        .copied()
        .collect();

      if available_blocks.is_empty() {
        return;
      }

      // Pick a random block based on glow_time for variety
      let idx = ((self.glow_time * 13.7).sin().abs() * available_blocks.len() as f32) as usize
        % available_blocks.len();
      let block = available_blocks[idx];

      // Mark as heart block
      if let Some(mut block_comp) = ctx.world.get_mut::<Block>(block) {
        block_comp.is_heart = true;
      }

      // Change sprite to heart block
      if let Some(mut renderable) = ctx.world.get_mut::<Renderable>(block) {
        renderable.sprite = SpriteHandle::from_sprite(
          Self::health_block_sprite(BLOCK_WIDTH as u32, BLOCK_HEIGHT as u32, 1.0),
          format!("heart_block_{}", current_milestone),
        );
      }

      // Track this block for pulse animation
      self.heart_blocks.push(block);
    }
  }

  fn spawn_walls(&mut self, ctx: &mut SceneContext) {
    // Left wall
    let left = ctx
      .world
      .spawn((
        Transform::from_xy(-WALL_THICKNESS / 2.0, SCREEN_HEIGHT as f32 / 2.0),
        RigidBody::fixed(),
        Collider::rect(WALL_THICKNESS, SCREEN_HEIGHT as f32)
          .with_restitution(1.0) // Perfect bounce
          .with_friction(0.0), // No friction
        Renderable::sprite(SpriteHandle::from_sprite(
          Sprite::solid_rect(WALL_THICKNESS as u32, SCREEN_HEIGHT, 0x30, 0x30, 0x40, 0xff),
          "wall_left",
        ))
        .with_z_order(0),
        Wall,
      ))
      .id();
    self.walls.push(left);

    // Right wall
    let right = ctx
      .world
      .spawn((
        Transform::from_xy(
          SCREEN_WIDTH as f32 + WALL_THICKNESS / 2.0,
          SCREEN_HEIGHT as f32 / 2.0,
        ),
        RigidBody::fixed(),
        Collider::rect(WALL_THICKNESS, SCREEN_HEIGHT as f32)
          .with_restitution(1.0) // Perfect bounce
          .with_friction(0.0), // No friction
        Renderable::sprite(SpriteHandle::from_sprite(
          Sprite::solid_rect(WALL_THICKNESS as u32, SCREEN_HEIGHT, 0x30, 0x30, 0x40, 0xff),
          "wall_right",
        ))
        .with_z_order(0),
        Wall,
      ))
      .id();
    self.walls.push(right);

    // Top wall (ceiling)
    let ceiling = ctx
      .world
      .spawn((
        Transform::from_xy(SCREEN_WIDTH as f32 / 2.0, -WALL_THICKNESS / 2.0),
        RigidBody::fixed(),
        Collider::rect(SCREEN_WIDTH as f32, WALL_THICKNESS)
          .with_restitution(1.0) // Perfect bounce
          .with_friction(0.0), // No friction
        Renderable::sprite(SpriteHandle::from_sprite(
          Sprite::solid_rect(SCREEN_WIDTH, WALL_THICKNESS as u32, 0x30, 0x30, 0x40, 0xff),
          "wall_ceiling",
        ))
        .with_z_order(0),
        Wall,
      ))
      .id();
    self.walls.push(ceiling);

    // Death zone: from just below the screen, and far wider than it, since
    // the side walls end at the bottom edge and a ball leaving near a corner
    // heads sideways past them before it is off the bottom
    let death = ctx
      .world
      .spawn((
        Transform::from_xy(
          SCREEN_WIDTH as f32 / 2.0,
          SCREEN_HEIGHT as f32 + BALL_SIZE + 50.0,
        ),
        RigidBody::fixed(),
        Collider::rect(SCREEN_WIDTH as f32 * 3.0, 100.0).as_trigger(),
        DeathZone,
      ))
      .id();
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
      let (r, g, b, _a) = color.to_rgba();
      // Darker border color (half brightness)
      let border = (r / 3, g / 3, b / 3);

      let block = ctx
        .world
        .spawn((
          Transform::from_xy(x, y),
          RigidBody::fixed(),
          Collider::rect(BLOCK_WIDTH, BLOCK_HEIGHT)
            .with_restitution(1.0) // Perfect bounce
            .with_friction(0.0), // No friction
          Renderable::sprite(SpriteHandle::from_sprite(
            Self::bordered_rect(BLOCK_WIDTH as u32, BLOCK_HEIGHT as u32, r, g, b, border),
            format!("block_{}_{}", x as u32, y as u32),
          ))
          .with_z_order(5),
          Block::new(color, hits, points),
        ))
        .id();
      self.blocks.push(block);
    }

    self.blocks_remaining = self.blocks.len() as u32;
  }

  /// Clear all level entities (blocks, walls, ball, paddle)
  fn clear_level(&mut self, ctx: &mut SceneContext) {
    // Despawn all blocks
    for block in self.blocks.drain(..) {
      ctx.world.despawn(block);
    }

    // Despawn walls
    for wall in self.walls.drain(..) {
      ctx.world.despawn(wall);
    }

    // Despawn all balls
    for ball in self.balls.drain(..) {
      ctx.world.despawn(ball);
    }

    // Despawn paddle
    if let Some(paddle) = self.paddle.take() {
      ctx.world.despawn(paddle);
    }

    // Despawn death zone
    if let Some(death_zone) = self.death_zone.take() {
      ctx.world.despawn(death_zone);
    }

    self.blocks_remaining = 0;
    self.multiball_blocks.clear();
    self.heart_blocks.clear();
    self.game_started = false;
    // Keep survival_time across levels (reward for not losing lives)
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
      if let Some(mut ball_comp) = ctx.world.get_mut::<Ball>(ball)
        && ball_comp.frozen
      {
        ball_comp.frozen = false;

        // Set initial velocity - angle influenced by paddle movement
        if let Some(mut vel) = ctx.world.get_mut::<Velocity>(ball) {
          // Paddle velocity influences horizontal launch angle
          // Normalize paddle velocity to -1..1 range, then apply influence factor
          let paddle_influence = (self.paddle_velocity / PADDLE_SPEED) * PADDLE_LAUNCH_INFLUENCE;

          // Calculate launch velocity with paddle influence
          // Max horizontal component is ~70% of ball speed to ensure upward motion
          let max_horizontal = BALL_SPEED * BALL_MAX_HORIZONTAL_RATIO;
          let horizontal = paddle_influence * max_horizontal;

          // Calculate vertical component to maintain constant total speed
          let vertical = -(BALL_SPEED * BALL_SPEED - horizontal * horizontal).sqrt();

          vel.dx = horizontal;
          vel.dy = vertical;
        }

        self.game_started = true;
        break;
      }
    }
  }

  fn reset_ball(&mut self, ctx: &mut SceneContext) {
    // Remove all existing balls
    for ball in self.balls.drain(..) {
      ctx.world.despawn(ball);
    }

    // Reset paddle to center and stop its velocity
    self.paddle_velocity = 0.0;
    if let Some(paddle) = self.paddle
      && let Some(mut transform) = ctx.world.get_mut::<Transform>(paddle)
    {
      transform.translation.x = SCREEN_WIDTH as f32 / 2.0;
    }

    // Spawn new ball
    self.spawn_ball(ctx);
  }

  fn handle_ball_death(&mut self, ctx: &mut SceneContext) {
    // Play death sound
    let _ = ctx
      .audio
      .play_sound_with("ball_death", SoundOptions::new().volume(0.5));

    self.lives = self.lives.saturating_sub(1);

    // Reset survival time (multiball chance resets on life lost)
    self.survival_time = 0.0;
    self.multiball_cooldown = 0.0;

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

  /// Score and break a block the ball hit. Rapier has already bounced the
  /// ball off it (restitution 1.0); reflecting it here as well would send
  /// it back into the bricks.
  fn handle_block_hit(&mut self, ctx: &mut SceneContext, block: Entity) {
    // Get block position FIRST (before any mutation) for multiball spawn:
    // the new ball's centre sits just below the block
    let block_position = ctx.world.get::<Transform>(block).map(|t| {
      (
        t.translation.x,
        t.translation.y + BLOCK_HEIGHT / 2.0 + BALL_SIZE / 2.0,
      )
    });

    // Extract data from block component
    let block_info = {
      if let Some(mut block_comp) = ctx.world.get_mut::<Block>(block) {
        block_comp.hits = block_comp.hits.saturating_sub(1);
        Some((
          block_comp.points,
          block_comp.hits,
          block_comp.color,
          block_comp.is_multiball,
          block_comp.is_heart,
        ))
      } else {
        None
      }
    };

    if let Some((points, remaining_hits, color, is_multiball, is_heart)) = block_info {
      if remaining_hits == 0 {
        // Play block break sound
        let _ = ctx
          .audio
          .play_sound_with("block_break", SoundOptions::new().volume(0.15));

        // Grant extra life if this was a health block
        if is_heart {
          self.lives += 1;
          // Trigger "+1" fly-by animation
          self.plus_one_timer = PLUS_ONE_DURATION;
        }

        // Destroy block
        self.score += points;
        self.blocks_remaining = self.blocks_remaining.saturating_sub(1);
        self.blocks.retain(|&b| b != block);
        self.multiball_blocks.retain(|&b| b != block);
        self.heart_blocks.retain(|&b| b != block);
        ctx.world.despawn(block);

        // Spawn extra ball if this was a multiball block
        // Use position captured BEFORE despawning
        if is_multiball && let Some((x, y)) = block_position {
          self.spawn_ball_at(ctx, x, y);
        }

        // Update high score
        if self.score > self.high_score {
          self.high_score = self.score;
        }

        // Check if we should spawn special blocks
        self.try_spawn_multiball_block(ctx);
        self.try_spawn_heart_block(ctx);
      } else {
        // Update block appearance to show damage
        let (r, g, b, _a) = color.dimmed();
        let border = (r / 3, g / 3, b / 3);
        if let Some(mut renderable) = ctx.world.get_mut::<Renderable>(block) {
          renderable.sprite = SpriteHandle::from_sprite(
            Self::bordered_rect(BLOCK_WIDTH as u32, BLOCK_HEIGHT as u32, r, g, b, border),
            "block_damaged",
          );
        }
      }
    }
  }

  fn check_level_complete(&mut self, ctx: &mut SceneContext) -> bool {
    if self.blocks_remaining == 0 {
      // Level complete! Save high score
      save_high_score(self.high_score);

      // Check if player beat all levels
      if self.level >= MAX_LEVEL {
        self.game_won = true;
        return true;
      }

      // Advance to next level
      self.level += 1;

      // Clear the level's entities (the background stays)
      self.clear_level(ctx);

      // Reset multiball cooldown for new level (keep survival time as reward)
      self.multiball_cooldown = 0.0;

      true
    } else {
      false
    }
  }
}

impl GameScene {
  /// Set the HUD's bindings (nothing is written for a value that has not
  /// changed)
  fn show_hud(&self, ctx: &mut SceneContext) {
    if let Some(hud) = self.hud {
      ctx.ui_set(hud, "score", self.score);
      ctx.ui_set(hud, "high", self.high_score);
      ctx.ui_set(hud, "lives", self.lives);
      ctx.ui_set(hud, "level", self.level);
    }
  }
}

impl Scene for GameScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    // Check for debug mode from workspace config
    if let Ok(workspace) = Workspace::discover() {
      self.debug_mode = workspace.config().env_bool("debug_scene").unwrap_or(false);
    }

    // Clear tracking lists from previous game (fix stale entity references)
    self.multiball_blocks.clear();
    self.heart_blocks.clear();
    self.survival_time = 0.0;
    self.multiball_cooldown = 0.0;
    self.last_heart_milestone = 0;

    ctx
      .camera()
      .set_position(SCREEN_WIDTH as f32 / 2.0, SCREEN_HEIGHT as f32 / 2.0);

    // Load audio files (gracefully handle missing files)
    let audio = &mut *ctx.audio;
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

    // Spawn background entity (solid color fill)
    // Color matches menu scene: RGB(20, 20, 35)
    let bg = ctx
      .world
      .spawn((
        Transform::from_xy(0.0, 0.0),
        Renderable::sprite(SpriteHandle::from_sprite(
          Sprite::solid_rect(SCREEN_WIDTH, SCREEN_HEIGHT, 20, 20, 35, 255),
          "game_background",
        ))
        .with_anchor(Anchor::TopLeft)
        .with_z_order(-10),
      ))
      .id();
    self.background = Some(bg);

    self.spawn_walls(ctx);
    self.spawn_paddle(ctx);
    self.spawn_ball(ctx);
    self.spawn_blocks(ctx);
    self.hud = spawn_ui_file(ctx, HUD, HUD_EMBEDDED);
    self.show_hud(ctx);
  }

  fn on_exit(&mut self, _ctx: &mut SceneContext) {
    // Save high score on exit
    save_high_score(self.high_score);
  }

  fn on_pause(&mut self, _ctx: &mut SceneContext) {
    // Pause logic handled by PauseScene
  }

  fn on_resume(&mut self, _ctx: &mut SceneContext, _returned: Option<Box<dyn std::any::Any>>) {
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

    // Update survival time (only when game has started and not paused/over)
    if self.game_started && self.lives > 0 {
      self.survival_time += dt;
    }

    // Update multiball cooldown
    if self.multiball_cooldown > 0.0 {
      self.multiball_cooldown = (self.multiball_cooldown - dt).max(0.0);
    }

    // Update "+1" animation timer
    if self.plus_one_timer > 0.0 {
      self.plus_one_timer = (self.plus_one_timer - dt).max(0.0);
    }

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

    // Track escape/Start key release (needed after resuming from pause)
    if !self.escape_was_released
      && !ctx.input.key_down(KeyCode::Escape)
      && !ctx.input.gamepad_button_down(GamepadButton::Start)
    {
      self.escape_was_released = true;
    }

    // Pause (only if escape/Start was released since last pause)
    // Use consume to prevent input from cascading to the pause overlay
    if self.escape_was_released
      && (ctx.input.key_pressed_and_consume(KeyCode::Escape)
        || ctx
          .input
          .gamepad_button_pressed_and_consume(GamepadButton::Start))
    {
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
        let any_input = ctx.input.key_pressed(KeyCode::Space)
          || ctx.input.key_pressed(KeyCode::Enter)
          || ctx.input.mouse_pressed(MouseButton::Left)
          || ctx.input.key_pressed(KeyCode::Escape)
          || ctx.input.gamepad_button_pressed(GamepadButton::South)
          || ctx.input.gamepad_button_pressed(GamepadButton::Start)
          || ctx.input.gamepad_button_pressed(GamepadButton::East);

        if any_input {
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
      // Check if player won the game
      if self.game_won {
        // Update high score one final time
        if self.score > self.high_score {
          self.high_score = self.score;
          save_high_score(self.score);
        }
        return SceneAction::Continue;
      }

      // Spawn the next level
      self.spawn_walls(ctx);
      self.spawn_paddle(ctx);
      self.spawn_ball(ctx);
      self.spawn_blocks(ctx);
      return SceneAction::Continue;
    }

    // Check for game won state (wait for input to return to menu)
    if self.game_won {
      let any_input = ctx.input.key_pressed(KeyCode::Space)
        || ctx.input.key_pressed(KeyCode::Enter)
        || ctx.input.mouse_pressed(MouseButton::Left)
        || ctx.input.key_pressed(KeyCode::Escape)
        || ctx.input.gamepad_button_pressed(GamepadButton::South)
        || ctx.input.gamepad_button_pressed(GamepadButton::Start)
        || ctx.input.gamepad_button_pressed(GamepadButton::East);

      if any_input {
        self.music = None;
        return SceneAction::SwitchWith(
          Box::new(MenuScene::new()),
          Transition::fade(Duration::from_millis(500)),
        );
      }
      return SceneAction::Continue;
    }

    // Launch ball on any key, click, or gamepad button
    if !self.game_started {
      let any_input = ctx.input.key_pressed(KeyCode::Space)
        || ctx.input.key_pressed(KeyCode::Enter)
        || ctx.input.mouse_pressed(MouseButton::Left)
        || ctx.input.key_pressed(KeyCode::KeyW)
        || ctx.input.key_pressed(KeyCode::ArrowUp)
        || ctx.input.gamepad_button_pressed(GamepadButton::South); // A button

      if any_input {
        self.launch_ball(ctx);
      }
    }

    // Paddle movement with acceleration (keyboard + D-pad + left stick)
    let stick_x = ctx.input.left_stick_x();
    let dpad_left = ctx.input.gamepad_button_down(GamepadButton::DPadLeft);
    let dpad_right = ctx.input.gamepad_button_down(GamepadButton::DPadRight);

    let move_left = ctx.input.key_down(KeyCode::KeyA)
      || ctx.input.key_down(KeyCode::ArrowLeft)
      || dpad_left
      || stick_x < -GAMEPAD_STICK_DEADZONE;

    let move_right = ctx.input.key_down(KeyCode::KeyD)
      || ctx.input.key_down(KeyCode::ArrowRight)
      || dpad_right
      || stick_x > GAMEPAD_STICK_DEADZONE;

    // Apply acceleration or deceleration
    if move_left && !move_right {
      self.paddle_velocity -= PADDLE_ACCELERATION * dt;
    } else if move_right && !move_left {
      self.paddle_velocity += PADDLE_ACCELERATION * dt;
    } else {
      // Decelerate towards zero when no input
      if self.paddle_velocity > 0.0 {
        self.paddle_velocity = (self.paddle_velocity - PADDLE_DECELERATION * dt).max(0.0);
      } else if self.paddle_velocity < 0.0 {
        self.paddle_velocity = (self.paddle_velocity + PADDLE_DECELERATION * dt).min(0.0);
      }
    }

    // Clamp velocity to max speed
    self.paddle_velocity = self.paddle_velocity.clamp(-PADDLE_SPEED, PADDLE_SPEED);

    // Apply velocity to paddle position
    if let Some(paddle) = self.paddle
      && let Some(mut transform) = ctx.world.get_mut::<Transform>(paddle)
    {
      transform.translation.x += self.paddle_velocity * dt;

      // Clamp to screen bounds and stop velocity at walls
      let min_x = PADDLE_WIDTH / 2.0;
      let max_x = SCREEN_WIDTH as f32 - PADDLE_WIDTH / 2.0;
      if transform.translation.x < min_x {
        transform.translation.x = min_x;
        self.paddle_velocity = 0.0;
      } else if transform.translation.x > max_x {
        transform.translation.x = max_x;
        self.paddle_velocity = 0.0;
      }
    }

    // Ball physics - maintain constant speed for all balls
    for &ball in &self.balls {
      if let Some(ball_comp) = ctx.world.get::<Ball>(ball) {
        if ball_comp.frozen {
          continue;
        }
      } else {
        continue;
      }

      if let Some(mut vel) = ctx.world.get_mut::<Velocity>(ball) {
        // Normalize velocity to maintain constant speed
        let speed = (vel.dx * vel.dx + vel.dy * vel.dy).sqrt();
        if speed > 0.0 && (speed - BALL_SPEED).abs() > 1.0 {
          vel.dx = vel.dx / speed * BALL_SPEED;
          vel.dy = vel.dy / speed * BALL_SPEED;
        }

        // Ensure ball doesn't get stuck going horizontally
        if vel.dy.abs() < BALL_MIN_VERTICAL_SPEED {
          vel.dy = if vel.dy >= 0.0 {
            BALL_MIN_VERTICAL_SPEED
          } else {
            -BALL_MIN_VERTICAL_SPEED
          };
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
      .messages::<CollisionStarted>()
      .into_iter()
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

    // Ball entered the death zone (a trigger, so not a collision)
    for entered in ctx.messages::<TriggerEntered>() {
      if self.death_zone == Some(entered.trigger) && self.balls.contains(&entered.actor()) {
        balls_to_remove.push(entered.actor());
      }
    }

    for (ball, other) in ball_collisions {
      // Ball hit paddle - add spin based on hit position
      if self.paddle == Some(other) {
        // Play paddle hit sound
        let _ = ctx
          .audio
          .play_sound_with("paddle_hit", SoundOptions::new().volume(0.15));

        if let (Some(ball_transform), Some(paddle_transform)) = (
          ctx.world.get::<Transform>(ball),
          ctx.world.get::<Transform>(other),
        ) {
          let offset =
            (ball_transform.translation.x - paddle_transform.translation.x) / (PADDLE_WIDTH / 2.0);

          if let Some(mut vel) = ctx.world.get_mut::<Velocity>(ball) {
            // Reflect and add spin
            vel.dy = -vel.dy.abs(); // Always go up
            vel.dx += offset * PADDLE_SPIN_EFFECT;

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
      ctx.world.despawn(ball);
    }

    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) -> SceneAction {
    let (screen_w, _screen_h) = ui.screen_size();

    // A saved HUD file that gave the root a new kind replaced its entity
    for reloaded in ctx.messages::<UiReloaded>() {
      if self.hud.is_some() && self.hud == reloaded.replaced {
        self.hud = Some(reloaded.root);
      }
    }
    self.show_hud(ctx);

    // "+1" fly-by animation when gaining a life
    if self.plus_one_timer > 0.0 {
      // Animation: starts at Lives position, moves right and fades out
      let progress = 1.0 - (self.plus_one_timer / PLUS_ONE_DURATION);
      let x_offset = 128.0 + progress * 60.0; // Starts past "Lives: N", moves right 60
      let y_offset = 10.0 - progress * 15.0; // Starts at 10, floats up to -5

      // Fade: full opacity in first third, then fade out
      let opacity = if progress < 0.3 {
        progress / 0.3 // Fade in
      } else {
        1.0 - (progress - 0.3) / 0.7 // Fade out
      };

      // Green "+1"
      ui.text("+1")
        .style(&text_style(Color::new(0x44, 0xff, 0x44, 255)))
        .opacity(opacity)
        .at(x_offset, y_offset);
    }

    // "Press any key to launch" prompt
    if !self.game_started {
      ui.draw_text_anchored_styled(
        "Press any key to launch!",
        Anchor::Center,
        0.0,
        100.0,
        &text_style(Color::WHITE),
      );
    }

    // Victory overlay
    if self.game_won {
      ui.draw_rect(
        Rect::new(0.0, 0.0, screen_w as f32, 720.0),
        Color::new(0, 0, 0, 180),
      );
      ui.draw_text_anchored_styled(
        "YOU WIN!",
        Anchor::Center,
        0.0,
        -70.0,
        &text_style(Color::YELLOW),
      );
      ui.draw_text_anchored_styled(
        "Congratulations!",
        Anchor::Center,
        0.0,
        -30.0,
        &text_style(Color::GREEN),
      );
      let final_score = format!("Final Score: {}", self.score);
      ui.draw_text_anchored_styled(
        &final_score,
        Anchor::Center,
        0.0,
        10.0,
        &text_style(Color::WHITE),
      );
      if self.score >= self.high_score {
        ui.draw_text_anchored_styled(
          "NEW HIGH SCORE!",
          Anchor::Center,
          0.0,
          50.0,
          &text_style(Color::CYAN),
        );
      }
      ui.draw_text_anchored_styled(
        "Press any key to continue",
        Anchor::Center,
        0.0,
        90.0,
        &text_style(Color::LIGHT_GRAY),
      );
    }

    // Game over overlay
    if self.lives == 0 {
      ui.draw_rect(
        Rect::new(0.0, 0.0, screen_w as f32, 720.0),
        Color::new(0, 0, 0, 180),
      );
      ui.draw_text_anchored_styled(
        "GAME OVER",
        Anchor::Center,
        0.0,
        -50.0,
        &text_style(Color::RED),
      );
      let final_score = format!("Final Score: {}", self.score);
      ui.draw_text_anchored_styled(
        &final_score,
        Anchor::Center,
        0.0,
        -10.0,
        &text_style(Color::WHITE),
      );
      ui.draw_text_anchored_styled(
        "Press any key to continue",
        Anchor::Center,
        0.0,
        30.0,
        &text_style(Color::LIGHT_GRAY),
      );
    }

    // Debug mode UI
    if self.debug_mode {
      ui.draw_text_anchored_styled(
        "[DEBUG MODE]",
        Anchor::BottomLeft,
        20.0,
        -10.0,
        &text_style(Color::new(255, 255, 0, 180)),
      );
      ui.draw_text_anchored_styled(
        "Q: Prev Level | E: Next Level",
        Anchor::BottomLeft,
        20.0,
        -25.0,
        &text_style(Color::new(200, 200, 200, 180)),
      );
    }
    SceneAction::Continue
  }

  fn name(&self) -> &str {
    "GameScene"
  }
}
