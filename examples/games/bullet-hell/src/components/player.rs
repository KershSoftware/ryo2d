//! Player component

use ryo2d::prelude::*;

use crate::constants::INVINCIBLE_DURATION;

/// The player's lives and the invincibility after a hit. Running, jumping
/// and wall slides are the engine's platformer preset (`PlatformerMovement`).
#[derive(Component, Debug, Clone, Copy)]
pub struct Player {
  pub lives: u32,
  /// Seconds of invincibility left after a hit
  pub invincible_timer: f32,
}

impl Player {
  pub fn new(lives: u32) -> Self {
    Self {
      lives,
      invincible_timer: 0.0,
    }
  }

  pub fn is_alive(&self) -> bool {
    self.lives > 0
  }

  pub fn take_damage(&mut self) {
    if self.invincible_timer <= 0.0 && self.lives > 0 {
      self.lives -= 1;
      self.invincible_timer = INVINCIBLE_DURATION;
    }
  }

  pub fn update(&mut self, dt: f32) {
    if self.invincible_timer > 0.0 {
      self.invincible_timer -= dt;
    }
  }

  pub fn is_invincible(&self) -> bool {
    self.invincible_timer > 0.0
  }
}
