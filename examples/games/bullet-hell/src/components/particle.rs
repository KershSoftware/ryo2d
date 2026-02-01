//! Particle component for explosion effects

#[derive(Debug, Clone, Copy)]
pub struct Particle {
  pub velocity_x: f32,
  pub velocity_y: f32,
  pub lifetime: f32,
  pub max_lifetime: f32,
}

impl Particle {
  pub fn new(velocity_x: f32, velocity_y: f32, lifetime: f32) -> Self {
    Self {
      velocity_x,
      velocity_y,
      lifetime,
      max_lifetime: lifetime,
    }
  }

  pub fn is_alive(&self) -> bool {
    self.lifetime > 0.0
  }

  pub fn alpha(&self) -> f32 {
    self.lifetime / self.max_lifetime
  }
}
