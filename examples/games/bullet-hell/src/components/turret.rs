//! Turret component for wall-mounted guns

use crate::constants::TURRET_BURST_SIZE;
use rand::Rng;

#[derive(Debug, Clone, Copy)]
pub struct Turret {
  pub side: TurretSide,
  pub move_direction: f32, // 1.0 for down, -1.0 for up
  pub shoot_timer: f32,
  pub burst_count: u32,
  pub burst_timer: f32,
  pub max_burst_size: u32, // Updated by progression system
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
    }
  }

  pub fn reverse_direction(&mut self) {
    self.move_direction *= -1.0;
  }
}
