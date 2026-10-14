//! Main game scene with all gameplay logic

use crate::components::*;
use crate::save::save_best_time;
use crate::shared;
use crate::*;
use ryo2d::prelude::*;

use super::{GameOverScene, PauseScene};

mod combat;
mod enemies;
mod player;
mod spawning;

// Fixed design resolution - game logic always uses these values
// Display settings only affect window scaling, not gameplay
const DESIGN_WIDTH: f32 = 1920.0;
const DESIGN_HEIGHT: f32 = 1080.0;

/// Bundle type for pooled bullets
pub(super) type BulletBundle = (
  Transform,
  Renderable,
  Interpolated,
  RigidBody,
  Collider,
  Velocity,
  Bullet,
  Trail2D,
);

/// Marker component for nebula orb child entities
#[derive(Component, Debug, Clone)]
pub(super) struct NebulaOrbMarker {
  pub(super) parent: Entity,
  pub(super) index: usize,
}

pub struct GameScene {
  pub(super) player: Option<Entity>,
  pub(super) turrets: Vec<Entity>,
  pub(super) drones: Vec<Entity>,
  pub(super) nebulas: Vec<Entity>,
  pub(super) rockets: Vec<Entity>,
  pub(super) lasers: Vec<Entity>,
  pub(super) fading_lasers: Vec<Entity>,
  pub(super) bullets: Vec<Entity>,
  pub(super) particles: Vec<Entity>,
  pub(super) platforms: Vec<Entity>,
  pub(super) barriers: Vec<Entity>,
  pub(super) survival_time: f32,
  #[allow(dead_code)]
  pub(super) best_time: f32,
  pub(super) player_lives: u32,
  pub(super) player_invincible: bool,
  pub(super) player_sprite: SpriteHandle,
  pub(super) turret_sprite: SpriteHandle,
  pub(super) drone_sprite: SpriteHandle,
  pub(super) nebula_orb_sprite: SpriteHandle,
  pub(super) rocket_sprite: SpriteHandle,
  pub(super) bullet_sprite: SpriteHandle,
  pub(super) particle_sprite: SpriteHandle,
  pub(super) platform_sprite: SpriteHandle,
  pub(super) barrier_sprite: SpriteHandle,
  // Progression tracking
  pub(super) difficulty_level: u32,
  pub(super) time_since_last_progression: f32,
  pub(super) current_turret_speed: f32,
  pub(super) current_burst_size: u32,
  // Drone spawning
  pub(super) next_drone_spawn_time: f32,
  // Nebula spawning
  pub(super) next_nebula_spawn_time: f32,
  // Death effect
  pub(super) death_effect_timer: Option<f32>,
  // Screen dimensions (obtained from engine at runtime)
  pub(super) screen_width: f32,
  pub(super) screen_height: f32,
  // Object pool for bullets (high-frequency spawning)
  pub(super) bullet_pool: Option<Pool<BulletBundle>>,
}

impl Default for GameScene {
  fn default() -> Self {
    Self::new()
  }
}

impl GameScene {
  pub fn new() -> Self {
    // Load sprites
    // Heroic gold/orange player color with black outline
    let player_sprite =
      Sprite::solid_rect(PLAYER_SIZE as u32, PLAYER_SIZE as u32, 255, 180, 50, 255);
    let turret_sprite = Sprite::load("assets/turret.png").expect("Failed to load turret sprite");
    // Create a small round bullet programmatically (bright orange-red)
    let bullet_sprite = Sprite::solid_circle(BULLET_SIZE as u32, 255, 100, 50, 255);
    let particle_sprite =
      Sprite::load("assets/particle.png").expect("Failed to load particle sprite");
    let platform_sprite =
      Sprite::load("assets/platform.png").expect("Failed to load platform sprite");
    // Create barrier sprite programmatically (dark gray rectangle)
    let barrier_sprite =
      Sprite::solid_rect(BARRIER_WIDTH as u32, BARRIER_HEIGHT as u32, 60, 60, 70, 255);
    // Create drone sprite programmatically (purple color)
    let drone_sprite = Sprite::solid_rect(DRONE_SIZE as u32, DRONE_SIZE as u32, 150, 80, 200, 255);
    // Create nebula orb sprite (forest green)
    let nebula_orb_sprite = Sprite::solid_circle(NEBULA_ORB_SIZE as u32, 45, 120, 50, 255);
    // Create rocket sprite (orange/red)
    let rocket_sprite = Sprite::solid_rect(
      ROCKET_SIZE as u32,
      (ROCKET_SIZE * 0.6) as u32,
      255,
      120,
      50,
      255,
    );

    Self {
      player: None,
      turrets: Vec::new(),
      drones: Vec::new(),
      nebulas: Vec::new(),
      rockets: Vec::new(),
      lasers: Vec::new(),
      fading_lasers: Vec::new(),
      bullets: Vec::new(),
      particles: Vec::new(),
      platforms: Vec::new(),
      barriers: Vec::new(),
      survival_time: 0.0,
      best_time: save::load_best_time(),
      player_lives: PLAYER_MAX_LIVES,
      player_invincible: false,
      player_sprite: SpriteHandle::from_sprite(player_sprite, "player.png"),
      turret_sprite: SpriteHandle::from_sprite(turret_sprite, "turret.png"),
      drone_sprite: SpriteHandle::from_sprite(drone_sprite, "drone"),
      nebula_orb_sprite: SpriteHandle::from_sprite(nebula_orb_sprite, "nebula_orb"),
      rocket_sprite: SpriteHandle::from_sprite(rocket_sprite, "rocket"),
      bullet_sprite: SpriteHandle::from_sprite(bullet_sprite, "bullet.png"),
      particle_sprite: SpriteHandle::from_sprite(particle_sprite, "particle.png"),
      platform_sprite: SpriteHandle::from_sprite(platform_sprite, "platform.png"),
      barrier_sprite: SpriteHandle::from_sprite(barrier_sprite, "barrier"),
      // Progression starts at level 0
      difficulty_level: 0,
      time_since_last_progression: 0.0,
      current_turret_speed: TURRET_MOVE_SPEED,
      current_burst_size: TURRET_BURST_SIZE,
      // Drone spawning - first drone at 30 seconds
      next_drone_spawn_time: DRONE_FIRST_SPAWN_TIME,
      // Nebula spawning - spawn immediately for testing (normally NEBULA_FIRST_SPAWN_TIME)
      next_nebula_spawn_time: 1.0,
      // Death effect not active
      death_effect_timer: None,
      // Screen dimensions - fixed design resolution for consistent gameplay
      screen_width: DESIGN_WIDTH,
      screen_height: DESIGN_HEIGHT,
      // Bullet pool initialized in on_enter
      bullet_pool: None,
    }
  }
}

impl Scene for GameScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    // Use fixed design resolution for consistent gameplay
    // Display settings only affect window scaling, not game logic
    self.screen_width = DESIGN_WIDTH;
    self.screen_height = DESIGN_HEIGHT;

    // Register handler for game:reset command (declared in MenuScene)
    // Handler is auto-cleaned when this scene exits
    ctx.console().register("game:reset", |_args, cmd_ctx| {
      cmd_ctx.console.print_system("Resetting game...");
      shared::request_reset();
      CommandResult::Ok
    });

    // Load sound effects
    let _ = ctx
      .audio
      .load_sound("enemy-spawn", "assets/enemy-spawn.wav");
    let _ = ctx.audio.load_sound("laser", "assets/laser.mp3");
    let _ = ctx.audio.load_sound("hurt", "assets/hurt.mp3");
    let _ = ctx.audio.load_sound("bullet", "assets/bullet.wav");

    // The backdrop carries on from the shared parallax time
    shared::spawn_background(ctx);

    // Reset state
    self.survival_time = 0.0;
    self.player = None;
    self.turrets.clear();
    self.drones.clear();
    self.lasers.clear();
    self.fading_lasers.clear();
    self.bullets.clear();
    self.particles.clear();
    self.platforms.clear();
    self.barriers.clear();

    // Configure camera shake for impactful hits (20px max offset, fast decay)
    ctx.camera().configure_shake(20.0, 1.5);

    // Spawn game entities
    self.spawn_platforms(ctx);
    self.spawn_barriers(ctx);
    self.spawn_turrets(ctx);
    // Drones additionally spawn progressively every 30 seconds
    self.spawn_drone(ctx);
    self.spawn_player(ctx);

    // Initialize bullet pool (200 bullets, spawned on first use)
    let bullet_sprite = self.bullet_sprite.clone();
    self.bullet_pool = Some(Pool::new(
      move || {
        (
          Transform::default(),
          Renderable::sprite(bullet_sprite.clone()).with_z_order(Z_BULLET),
          Interpolated,
          // CCD for fast-moving bullets, no gravity
          RigidBody::dynamic()
            .with_ccd()
            .with_gravity_scale(0.0)
            .lock_rotation(),
          Collider::circle(BULLET_SIZE / 2.0).as_trigger(),
          Velocity::zero(),
          Bullet::new(0.0, 0.0),
          Trail2D::new(Color::new(255, 140, 30, 230), 3.5)
            .with_z_order(Z_BULLET)
            .with_length(25)
            .with_taper(0.7)
            .with_fade(true)
            .with_min_distance(2.0),
        )
      },
      PoolConfig {
        initial_capacity: 200,
        max_capacity: 500,
        expand_batch_size: 50,
        exhausted_policy: PoolExhaustedPolicy::RecycleOldest,
      },
    ));

    // Center camera
    ctx
      .camera()
      .set_position(self.screen_width / 2.0, self.screen_height / 2.0);
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    // Check for reset request from console command
    if shared::take_reset_request() {
      return SceneAction::Switch(Box::new(GameScene::new()));
    }

    // Pause: push overlay scene
    if ctx.input.key_pressed_and_consume(KeyCode::Escape)
      || ctx
        .input
        .gamepad_button_pressed_and_consume(GamepadButton::Start)
    {
      return SceneAction::Push(Box::new(PauseScene::new()));
    }

    // The shared parallax time carries the backdrop into the next scene
    shared::add_parallax_time(dt);

    self.survival_time += dt;

    // Update progression
    self.update_progression(ctx, dt);

    // Update player state cache for UI
    if let Some(player_entity) = self.player
      && let Some(player) = ctx.world.get::<Player>(player_entity)
    {
      self.player_lives = player.lives;
      self.player_invincible = player.is_invincible();
    }

    // Handle death effect animation
    if let Some(timer) = self.death_effect_timer {
      let new_timer = timer - dt;

      // Animate chromatic aberration: quick pulse in and out
      const DEATH_EFFECT_DURATION: f32 = 0.6;
      let progress = 1.0 - (new_timer / DEATH_EFFECT_DURATION).clamp(0.0, 1.0);
      // Sine curve for smooth pulse: rises to peak at 0.5, falls back to 0
      let intensity = (progress * std::f32::consts::PI).sin();
      let strength = intensity * 0.015; // Max chromatic aberration strength
      ctx.effects().set_chromatic_aberration(strength);

      if new_timer <= 0.0 {
        // Effect complete, transition to game over
        ctx.effects().clear();
        save_best_time(self.survival_time);
        return SceneAction::Switch(Box::new(GameOverScene::new(self.survival_time)));
      }
      self.death_effect_timer = Some(new_timer);

      // During death effect, still render but don't update gameplay
      return SceneAction::Continue;
    }

    // Update barriers FIRST so player can ride them at correct position
    self.update_barriers(ctx, dt);
    self.update_player(ctx, dt);
    self.update_turrets(ctx, dt);
    self.update_drones(ctx, dt);
    self.update_nebulas(ctx, dt);
    self.update_rockets(ctx, dt);
    self.update_laser_tracers(ctx, dt);
    self.update_bullets(ctx, dt);
    self.update_particles(ctx, dt);

    // Check game over - start death effect instead of immediate transition
    if self.check_game_over(ctx) {
      const DEATH_EFFECT_DURATION: f32 = 0.6;
      self.death_effect_timer = Some(DEATH_EFFECT_DURATION);
      return SceneAction::Continue;
    }

    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    profile_scope!("ui");

    // Use cached player state
    let lives = self.player_lives;
    let invincible = self.player_invincible;

    // Lives - draw 3 hearts (red = alive, grey = lost)
    let heart_size = 44.0;
    let heart_spacing = 16.0;
    let hearts_x = 40.0;
    let hearts_y = 40.0;

    for i in 0..PLAYER_MAX_LIVES {
      let x = hearts_x + i as f32 * (heart_size + heart_spacing);
      let has_life = i < lives;
      self.draw_heart(ui, x, hearts_y, heart_size, has_life, invincible);
    }

    // Survival time - positioned below hearts
    let time_text = format!("Time: {:.1}s", self.survival_time);
    ui.label(
      &time_text,
      40.0,
      hearts_y + heart_size + 20.0,
      Color::WHITE,
      Some(Color::new(0, 0, 0, 150)),
    );

    // Hit indicator
    if invincible {
      ui.draw_text_anchored(
        "HIT!",
        Anchor::TopCenter,
        0.0,
        200.0,
        Color::rgb(255, 50, 50),
      );
    }

    SceneAction::Continue
  }
}

// Helper methods for UI rendering (not part of Scene trait)
impl GameScene {
  /// Draw a pixelated heart icon
  fn draw_heart(
    &self,
    ui: &mut UiContext,
    x: f32,
    y: f32,
    size: f32,
    filled: bool,
    flashing: bool,
  ) {
    // Colors - apply flash effect when hit
    let fill_color = if filled {
      if flashing {
        // Pulsing flash effect using survival_time for animation
        let pulse = (self.survival_time * 10.0).sin().abs();
        let base_r = 220u8;
        let base_g = 50u8;
        let base_b = 50u8;
        // Flash towards white
        let flash_r = base_r.saturating_add((pulse * 35.0) as u8);
        let flash_g = base_g.saturating_add((pulse * 200.0) as u8);
        let flash_b = base_b.saturating_add((pulse * 200.0) as u8);
        Color::rgb(flash_r, flash_g, flash_b)
      } else {
        Color::rgb(220, 50, 50) // Red for alive
      }
    } else {
      Color::rgb(140, 130, 120) // Beige/grey for lost
    };
    let border_color = Color::rgb(60, 50, 50); // Dark border

    // Scale factor (heart pattern is 9x8 base)
    let scale = size / 9.0;
    let px = scale; // Pixel size

    // Classic pixel heart pattern (9 wide, 8 tall)
    // 1 = filled, 0 = empty
    #[rustfmt::skip]
    let pattern: [[u8; 9]; 8] = [
      [0, 1, 1, 0, 0, 0, 1, 1, 0], // Row 0: two bumps
      [1, 1, 1, 1, 1, 1, 1, 1, 1], // Row 1
      [1, 1, 1, 1, 1, 1, 1, 1, 1], // Row 2
      [1, 1, 1, 1, 1, 1, 1, 1, 1], // Row 3
      [0, 1, 1, 1, 1, 1, 1, 1, 0], // Row 4
      [0, 0, 1, 1, 1, 1, 1, 0, 0], // Row 5
      [0, 0, 0, 1, 1, 1, 0, 0, 0], // Row 6
      [0, 0, 0, 0, 1, 0, 0, 0, 0], // Row 7
    ];

    // Draw border first (slightly larger)
    let border_offset = 1.0;
    for (row_idx, row) in pattern.iter().enumerate() {
      for (col_idx, &pixel) in row.iter().enumerate() {
        if pixel == 1 {
          let px_x = x + col_idx as f32 * px;
          let px_y = y + row_idx as f32 * px;
          // Draw border by checking if any neighbor is empty
          let has_empty_neighbor = Self::has_empty_neighbor(&pattern, row_idx, col_idx);
          if has_empty_neighbor {
            ui.draw_rect(
              Rect::new(
                px_x - border_offset,
                px_y - border_offset,
                px + border_offset * 2.0,
                px + border_offset * 2.0,
              ),
              border_color,
            );
          }
        }
      }
    }

    // Draw fill
    for (row_idx, row) in pattern.iter().enumerate() {
      for (col_idx, &pixel) in row.iter().enumerate() {
        if pixel == 1 {
          let px_x = x + col_idx as f32 * px;
          let px_y = y + row_idx as f32 * px;
          ui.draw_rect(Rect::new(px_x, px_y, px, px), fill_color);
        }
      }
    }
  }

  /// Apply damage to the player: flash, camera shake, and hurt sound.
  /// Returns true if damage was actually applied (player was not invincible).
  fn apply_player_damage(&mut self, ctx: &mut SceneContext) -> bool {
    let Some(player_e) = self.player else {
      return false;
    };
    let Some(mut player) = ctx.world.get_mut::<Player>(player_e) else {
      return false;
    };
    let was_invincible = player.is_invincible();
    player.take_damage();
    self.player_lives = player.lives;
    self.player_invincible = player.is_invincible();

    if !was_invincible {
      if let Some(mut renderable) = ctx.world.get_mut::<Renderable>(player_e) {
        renderable.shader = Some(SpriteShader::flash(Color::rgb(255, 100, 100), 1.0));
      }
      ctx.camera().add_trauma(0.5);
      let _ = ctx.audio.play_sound("hurt");
    }
    !was_invincible
  }

  /// Check if a pixel in the heart pattern has an empty neighbor
  fn has_empty_neighbor(pattern: &[[u8; 9]; 8], row: usize, col: usize) -> bool {
    let directions: [(i32, i32); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];
    for (dr, dc) in directions {
      let nr = row as i32 + dr;
      let nc = col as i32 + dc;
      if !(0..8).contains(&nr) || !(0..9).contains(&nc) {
        return true; // Edge of pattern counts as empty
      }
      if pattern[nr as usize][nc as usize] == 0 {
        return true;
      }
    }
    false
  }
}
