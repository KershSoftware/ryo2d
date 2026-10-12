//! Nebula enemy component - 3 balls floating around each other, fires MLRS rockets
//!
//! The nebula is a floating enemy made of 3 orbs that orbit around a center point.
//! They use pathfinding with steering-based avoidance to float smoothly.
//! Attacks with MLRS-style rockets that arc up then lock briefly on the player.

use ryo2d::prelude::*;

/// Spawn duration for nebula fade-in effect
pub const NEBULA_SPAWN_DURATION: f32 = 2.0;

/// Nebula states for behavior control
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NebulaState {
  /// Moving to a new position
  Moving,
  /// Pausing to prepare rockets
  Charging,
  /// Firing the rocket salvo
  Firing,
  /// Cooldown after firing
  Cooldown,
}

/// Individual orb data for the 3 floating balls
#[derive(Debug, Clone)]
pub struct NebulaOrb {
  /// Offset from center (oscillates)
  pub offset: Vec2,
  /// Phase for oscillation (radians)
  pub phase: f32,
  /// Orbit radius
  pub radius: f32,
  /// Orbit speed (radians per second)
  pub orbit_speed: f32,
}

impl NebulaOrb {
  pub fn new(index: usize) -> Self {
    let mut rng = rand::thread_rng();
    let base_phase = (index as f32) * std::f32::consts::TAU / 3.0;

    Self {
      offset: Vec2::ZERO,
      phase: base_phase + rng.gen_range(-0.3..0.3),
      radius: rng.gen_range(18.0..28.0),
      orbit_speed: rng.gen_range(1.5..2.5) * if rng.gen_bool(0.5) { 1.0 } else { -1.0 },
    }
  }

  /// Update orb position around center
  pub fn update(&mut self, dt: f32) {
    self.phase += self.orbit_speed * dt;
    self.offset = Vec2::new(
      self.phase.cos() * self.radius,
      self.phase.sin() * self.radius,
    );
  }

  /// Get world position given center
  pub fn position(&self, center: Vec2) -> Vec2 {
    center + self.offset
  }
}

#[derive(Component, Debug, Clone)]
pub struct Nebula {
  /// Current behavior state
  pub state: NebulaState,
  /// Movement velocity
  pub velocity: Vec2,
  /// Target position for current move
  pub target: Vec2,
  /// Timer for current state
  pub state_timer: f32,
  /// The 3 floating orbs
  pub orbs: [NebulaOrb; 3],
  /// Rockets remaining in current salvo
  pub rockets_remaining: u32,
  /// Timer between rockets in salvo
  pub rocket_timer: f32,
  /// Spawn timer for fade-in effect (0 = fully spawned)
  pub spawn_timer: f32,
  /// Avoidance radius for steering
  pub radius: f32,
}

/// How many rockets per salvo
pub const NEBULA_ROCKETS_PER_SALVO: u32 = 3;
/// Time between rockets in a salvo
pub const NEBULA_ROCKET_INTERVAL: f32 = 0.25;

impl Nebula {
  pub fn new(start_x: f32, start_y: f32) -> Self {
    let mut rng = rand::thread_rng();
    Self {
      state: NebulaState::Cooldown,
      velocity: Vec2::ZERO,
      target: Vec2::new(start_x, start_y),
      state_timer: rng.gen_range(1.0..2.0),
      orbs: [NebulaOrb::new(0), NebulaOrb::new(1), NebulaOrb::new(2)],
      rockets_remaining: 0,
      rocket_timer: 0.0,
      spawn_timer: NEBULA_SPAWN_DURATION,
      radius: 40.0, // Larger radius for the nebula cluster
    }
  }

  /// Check if nebula is still spawning (fading in)
  pub fn is_spawning(&self) -> bool {
    self.spawn_timer > 0.0
  }

  /// Update spawn timer, returns true if just finished spawning
  pub fn update_spawn(&mut self, dt: f32) -> bool {
    if self.spawn_timer > 0.0 {
      self.spawn_timer -= dt;
      if self.spawn_timer <= 0.0 {
        self.spawn_timer = 0.0;
        return true;
      }
    }
    false
  }

  /// Get spawn progress (0.0 = just started, 1.0 = fully spawned)
  pub fn spawn_progress(&self) -> f32 {
    1.0 - (self.spawn_timer / NEBULA_SPAWN_DURATION).clamp(0.0, 1.0)
  }

  /// Update orb positions
  pub fn update_orbs(&mut self, dt: f32) {
    for orb in &mut self.orbs {
      orb.update(dt);
    }
  }

  /// Pick a new random target position to move to
  pub fn pick_new_target(&mut self, current: Vec2, screen_w: f32, screen_h: f32) {
    let mut rng = rand::thread_rng();

    // Move in upper half of screen
    let margin = 100.0;
    let min_x = margin;
    let max_x = screen_w - margin;
    let min_y = margin;
    let max_y = screen_h * 0.5; // Stay in upper half

    // Pick random target
    self.target.x = rng.gen_range(min_x..max_x);
    self.target.y = rng.gen_range(min_y..max_y);

    // Ensure minimum distance
    let min_distance = 120.0;
    let diff = self.target - current;
    let dist = diff.length();

    if dist < min_distance && dist > 0.1 {
      let scale = min_distance / dist;
      self.target =
        (current + diff * scale).clamp(Vec2::new(min_x, min_y), Vec2::new(max_x, max_y));
    }
  }

  /// Calculate velocity to reach target (base, before steering)
  pub fn calculate_base_velocity(&self, current: Vec2, speed: f32) -> Vec2 {
    let diff = self.target - current;
    let dist = diff.length();

    if dist > 5.0 {
      (diff / dist) * speed
    } else {
      Vec2::ZERO
    }
  }

  /// Check if nebula has reached its target
  pub fn reached_target(&self, current: Vec2) -> bool {
    (self.target - current).length() < 15.0
  }

  /// Get the topmost orb position for rocket spawning
  pub fn top_orb_position(&self, center: Vec2) -> Vec2 {
    self
      .orbs
      .iter()
      .map(|orb| orb.position(center))
      .min_by(|a, b| a.y.partial_cmp(&b.y).unwrap())
      .unwrap_or(center)
  }
}
