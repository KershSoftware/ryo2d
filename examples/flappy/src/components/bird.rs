//! Bird component - the player character

use ryo2d::prelude::Sprite;

use crate::constants::*;

/// Holds all bird sprite states
pub struct BirdSprites {
  pub flap_up: Sprite,
  pub flap_down: Sprite,
  pub hurt_1: Sprite,
  pub hurt_2: Sprite,
}

impl BirdSprites {
  pub fn load() -> Self {
    let size = BIRD_SIZE as u32;
    Self {
      flap_up: Sprite::load_svg_fit("assets/bird-flap-up.svg", size, size)
        .expect("Failed to load bird-flap-up.svg"),
      flap_down: Sprite::load_svg_fit("assets/bird-flap-down.svg", size, size)
        .expect("Failed to load bird-flap-down.svg"),
      hurt_1: Sprite::load_svg_fit("assets/bird-hurt-1.svg", size, size)
        .expect("Failed to load bird-hurt-1.svg"),
      hurt_2: Sprite::load_svg_fit("assets/bird-hurt-2.svg", size, size)
        .expect("Failed to load bird-hurt-2.svg"),
    }
  }

  /// Get the appropriate sprite based on bird state
  pub fn get_sprite(&self, velocity_y: f32, is_alive: bool, hurt_frame: u32) -> &Sprite {
    if !is_alive {
      // Alternate between hurt frames
      if hurt_frame % 2 == 0 {
        &self.hurt_1
      } else {
        &self.hurt_2
      }
    } else if velocity_y < 0.0 {
      &self.flap_up
    } else {
      &self.flap_down
    }
  }
}

/// Marks an entity as the player-controlled bird
#[derive(Debug, Clone, Copy)]
pub struct Bird {
  pub velocity_y: f32,
  pub is_alive: bool,
  pub has_started: bool,
  pub hurt_timer: f32,
  pub hurt_frame: u32,
}

impl Bird {
  pub fn new() -> Self {
    Self {
      velocity_y: 0.0,
      is_alive: true,
      has_started: false,
      hurt_timer: 0.0,
      hurt_frame: 0,
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
