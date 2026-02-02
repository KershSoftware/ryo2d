//! Turret component for wall-mounted guns

use crate::constants::{TURRET_BURST_SIZE, TURRET_SPAWN_DURATION};
use rand::Rng;

#[derive(Debug, Clone, Copy)]
pub struct Turret {
  pub side: TurretSide,
  pub move_direction: f32, // 1.0 for down, -1.0 for up
  pub shoot_timer: f32,
  pub burst_count: u32,
  pub burst_timer: f32,
  pub max_burst_size: u32, // Updated by progression system
  pub spawn_timer: f32,    // Time remaining until fully spawned (0 = active)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurretSide {
  Left,
  Right,
}

impl Turret {
  pub fn new(side: TurretSide) -> Self {
    let mut rng = rand::thread_rng();
    Self {
      side,
      move_direction: if rng.gen_bool(0.5) { 1.0 } else { -1.0 },
      shoot_timer: rng.gen_range(0.5..2.0),
      burst_count: 0,
      burst_timer: 0.0,
      max_burst_size: TURRET_BURST_SIZE,
      spawn_timer: TURRET_SPAWN_DURATION,
    }
  }

  /// Create a turret that's already fully spawned (no fade-in)
  pub fn new_instant(side: TurretSide) -> Self {
    let mut turret = Self::new(side);
    turret.spawn_timer = 0.0;
    turret
  }

  pub fn reverse_direction(&mut self) {
    self.move_direction *= -1.0;
  }

  /// Check if turret is still spawning (fading in)
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
    1.0 - (self.spawn_timer / TURRET_SPAWN_DURATION).clamp(0.0, 1.0)
  }
}
