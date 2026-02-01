//! Bullet component

#[derive(Debug, Clone, Copy)]
pub struct Bullet {
  pub velocity_x: f32,
  pub velocity_y: f32,
}

impl Bullet {
  pub fn new(velocity_x: f32, velocity_y: f32) -> Self {
    Self { velocity_x, velocity_y }
  }
}
