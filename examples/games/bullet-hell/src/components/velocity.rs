//! Velocity component for entities that move

#[derive(Debug, Clone, Copy)]
pub struct PlayerVelocity {
  pub x: f32,
  pub y: f32,
}

impl PlayerVelocity {
  pub fn new(x: f32, y: f32) -> Self {
    Self { x, y }
  }

  pub fn zero() -> Self {
    Self { x: 0.0, y: 0.0 }
  }
}
