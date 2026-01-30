//! Bird component - the player character

use crate::constants::*;

/// Marks an entity as the player-controlled bird
#[derive(Debug, Clone, Copy)]
pub struct Bird {
  pub velocity_y: f32,
  pub is_alive: bool,
  pub has_started: bool,
}

impl Bird {
  pub fn new() -> Self {
    Self {
      velocity_y: 0.0,
      is_alive: true,
      has_started: false,
    }
  }

  pub fn flap(&mut self) {
    if self.is_alive {
      self.velocity_y = BIRD_FLAP_VELOCITY;
      self.has_started = true;
    }
  }

  pub fn apply_gravity(&mut self, dt: f32) {
    if self.has_started && self.is_alive {
      self.velocity_y += BIRD_GRAVITY * dt;
      self.velocity_y = self.velocity_y.min(BIRD_MAX_FALL_SPEED);
    }
  }

  pub fn kill(&mut self) {
    self.is_alive = false;
  }
}

impl Default for Bird {
  fn default() -> Self {
    Self::new()
  }
}
