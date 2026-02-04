//! Laser tracer component for sweeping laser effects
//!
//! When a drone fires, a tracer entity quickly moves from the drone to
//! the laser endpoint, leaving a fading trail and damaging the player on contact.

/// Component for laser tracer entities that sweep across the screen.
#[derive(Debug)]
pub struct LaserTracer {
  /// Target X position (endpoint of laser)
  pub target_x: f32,
  /// Target Y position
  pub target_y: f32,
  /// Speed in pixels per second
  pub speed: f32,
  /// Whether this tracer damages the player on contact
  pub damages: bool,
  /// Damage radius (how close player must be to take damage)
  pub damage_radius: f32,
}

impl LaserTracer {
  /// Create a new laser tracer
  pub fn new(target_x: f32, target_y: f32, speed: f32) -> Self {
    Self {
      target_x,
      target_y,
      speed,
      damages: false,
      damage_radius: 20.0,
    }
  }

  /// Create a damaging laser tracer
  pub fn damaging(target_x: f32, target_y: f32, speed: f32, damage_radius: f32) -> Self {
    Self {
      target_x,
      target_y,
      speed,
      damages: true,
      damage_radius,
    }
  }

  /// Check if the tracer has reached its target
  pub fn reached_target(&self, x: f32, y: f32) -> bool {
    let dx = self.target_x - x;
    let dy = self.target_y - y;
    let dist_sq = dx * dx + dy * dy;
    // Consider reached if within 5 pixels
    dist_sq < 25.0
  }
}
