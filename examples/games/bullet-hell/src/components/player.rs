//! Player component

use crate::constants::{INVINCIBLE_DURATION, WALL_JUMP_PUSH_DURATION, WALL_SLIDE_GRACE_DURATION};

/// Which wall the player is sliding on
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WallSide {
  Left,
  Right,
}

#[derive(Debug, Clone, Copy)]
pub struct Player {
  pub lives: u32,
  pub is_grounded: bool,
  pub invincible_timer: f32,          // Temporary invincibility after hit
  pub wall_sliding: Option<WallSide>, // Which wall player is sliding on (if any)
  pub can_wall_jump: bool, // Whether player can wall jump (reset on ground/new wall contact)
  pub wall_jump_cooldown: f32, // Timer preventing horizontal input override after wall jump
  pub wall_slide_grace: f32, // Grace period to still wall jump after releasing wall direction
  pub last_wall_side: Option<WallSide>, // Remember which wall we were on for grace period
}

impl Player {
  pub fn new(lives: u32) -> Self {
    Self {
      lives,
      is_grounded: false,
      invincible_timer: 0.0,
      wall_sliding: None,
      can_wall_jump: false,
      wall_jump_cooldown: 0.0,
      wall_slide_grace: 0.0,
      last_wall_side: None,
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
    if self.wall_jump_cooldown > 0.0 {
      self.wall_jump_cooldown -= dt;
    }
    if self.wall_slide_grace > 0.0 {
      self.wall_slide_grace -= dt;
      if self.wall_slide_grace <= 0.0 {
        // Grace expired - can no longer wall jump
        self.can_wall_jump = false;
        self.last_wall_side = None;
      }
    }
  }

  pub fn start_wall_jump(&mut self) {
    self.wall_jump_cooldown = WALL_JUMP_PUSH_DURATION;
    self.can_wall_jump = false;
    self.wall_slide_grace = 0.0;
    self.last_wall_side = None;
  }

  /// Start the grace period when player releases wall direction
  pub fn start_wall_slide_grace(&mut self, wall_side: WallSide) {
    self.wall_slide_grace = WALL_SLIDE_GRACE_DURATION;
    self.last_wall_side = Some(wall_side);
  }

  /// Check if player can wall jump (either actively sliding or in grace period)
  pub fn can_perform_wall_jump(&self) -> bool {
    self.can_wall_jump && (self.wall_sliding.is_some() || self.wall_slide_grace > 0.0)
  }

  /// Get the wall side for wall jump (current slide or grace period)
  pub fn wall_jump_side(&self) -> Option<WallSide> {
    self.wall_sliding.or(self.last_wall_side)
  }

  pub fn is_wall_jump_locked(&self) -> bool {
    self.wall_jump_cooldown > 0.0
  }

  pub fn is_invincible(&self) -> bool {
    self.invincible_timer > 0.0
  }
}
