//! Particle component
//!
//! Individual particle data for the CPU-simulated particle system.

use crate::core::ui::Color;

/// A single particle in the particle system.
///
/// Particles are spawned by [`ParticleEmitter`] and simulated on the CPU.
/// Visual properties (size, color) are interpolated over the particle's lifetime.
///
/// # Example
///
/// ```ignore
/// // Particles are typically created by emitters, not manually
/// let particle = Particle {
///     lifetime: 0.0,
///     max_lifetime: 2.0,
///     velocity: Vec2::new(0.0, -50.0),
///     ..Default::default()
/// };
/// ```
#[derive(Debug, Clone)]
pub struct Particle {
  // =========================================================================
  // Lifetime
  // =========================================================================
  /// Current age in seconds (0.0 = just spawned)
  pub lifetime: f32,
  /// Maximum lifetime in seconds before particle dies
  pub max_lifetime: f32,

  // =========================================================================
  // Motion
  // =========================================================================
  /// Current velocity in pixels per second
  pub velocity_x: f32,
  pub velocity_y: f32,
  /// Angular velocity in radians per second
  pub angular_velocity: f32,

  // =========================================================================
  // Visual (interpolated over lifetime)
  // =========================================================================
  /// Current size (interpolated between start_size and end_size)
  pub size: f32,
  /// Size at spawn
  pub start_size: f32,
  /// Size at death
  pub end_size: f32,
  /// Current color (interpolated between start_color and end_color)
  pub color: Color,
  /// Color at spawn
  pub start_color: Color,
  /// Color at death
  pub end_color: Color,
  /// Current rotation in radians
  pub rotation: f32,

  // =========================================================================
  // Physics
  // =========================================================================
  /// Mass for attractor calculations (default 1.0)
  pub mass: f32,
  /// Velocity damping per second (0.0 = no damping, 1.0 = stops immediately)
  pub drag: f32,
  /// Multiplier for gravity (0.0 = no gravity, 1.0 = full gravity)
  pub gravity_scale: f32,

  // =========================================================================
  // Features
  // =========================================================================
  /// Whether this particle has a trail attached
  pub has_trail: bool,
  /// Layer bits from the emitter (`ParticleEmitter::with_layer`), matched against attractor masks
  pub collision_layer: u32,
  /// Current animation frame (for animated particles)
  pub frame: u32,
}

impl Default for Particle {
  fn default() -> Self {
    Self {
      lifetime: 0.0,
      max_lifetime: 1.0,
      velocity_x: 0.0,
      velocity_y: 0.0,
      angular_velocity: 0.0,
      size: 16.0,
      start_size: 16.0,
      end_size: 16.0,
      color: Color::WHITE,
      start_color: Color::WHITE,
      end_color: Color::TRANSPARENT,
      rotation: 0.0,
      mass: 1.0,
      drag: 0.0,
      gravity_scale: 0.0,
      has_trail: false,
      collision_layer: 0,
      frame: 0,
    }
  }
}

impl Particle {
  /// Create a new particle with default values.
  pub fn new() -> Self {
    Self::default()
  }

  /// Get the normalized lifetime (0.0 = spawned, 1.0 = about to die).
  #[inline]
  pub fn lifetime_ratio(&self) -> f32 {
    if self.max_lifetime <= 0.0 {
      1.0
    } else {
      (self.lifetime / self.max_lifetime).clamp(0.0, 1.0)
    }
  }

  /// Check if the particle is dead (lifetime exceeded).
  #[inline]
  pub fn is_dead(&self) -> bool {
    self.lifetime >= self.max_lifetime
  }

  /// Update interpolated visual properties based on current lifetime.
  ///
  /// Call this after updating lifetime to refresh size and color.
  pub fn update_visuals(&mut self) {
    let t = self.lifetime_ratio();

    // Interpolate size
    self.size = lerp(self.start_size, self.end_size, t);

    // Interpolate color
    self.color = Color::new(
      lerp(self.start_color.r as f32, self.end_color.r as f32, t) as u8,
      lerp(self.start_color.g as f32, self.end_color.g as f32, t) as u8,
      lerp(self.start_color.b as f32, self.end_color.b as f32, t) as u8,
      lerp(self.start_color.a as f32, self.end_color.a as f32, t) as u8,
    );
  }

  /// Apply physics simulation for one time step.
  ///
  /// Updates velocity (drag, gravity) and rotation (angular velocity).
  /// Transform position should be updated separately by the caller.
  pub fn simulate(&mut self, dt: f32, gravity_x: f32, gravity_y: f32) {
    // Apply gravity
    self.velocity_x += gravity_x * self.gravity_scale * dt;
    self.velocity_y += gravity_y * self.gravity_scale * dt;

    // Apply drag (velocity *= 1 - drag * dt)
    if self.drag > 0.0 {
      let drag_factor = (1.0 - self.drag * dt).max(0.0);
      self.velocity_x *= drag_factor;
      self.velocity_y *= drag_factor;
    }

    // Apply angular velocity
    self.rotation += self.angular_velocity * dt;
  }
}

/// Linear interpolation helper
#[inline]
fn lerp(a: f32, b: f32, t: f32) -> f32 {
  a + (b - a) * t
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn particle_default() {
    let p = Particle::default();
    assert_eq!(p.lifetime, 0.0);
    assert_eq!(p.max_lifetime, 1.0);
    assert!(!p.is_dead());
  }

  #[test]
  fn particle_lifetime_ratio() {
    let mut p = Particle {
      max_lifetime: 2.0,
      ..Default::default()
    };

    p.lifetime = 0.0;
    assert!((p.lifetime_ratio() - 0.0).abs() < 0.001);

    p.lifetime = 1.0;
    assert!((p.lifetime_ratio() - 0.5).abs() < 0.001);

    p.lifetime = 2.0;
    assert!((p.lifetime_ratio() - 1.0).abs() < 0.001);
    assert!(p.is_dead());
  }

  #[test]
  fn particle_visual_interpolation() {
    let mut p = Particle {
      max_lifetime: 1.0,
      start_size: 10.0,
      end_size: 20.0,
      start_color: Color::WHITE,
      end_color: Color::new(255, 0, 0, 0),
      ..Default::default()
    };

    p.lifetime = 0.0;
    p.update_visuals();
    assert!((p.size - 10.0).abs() < 0.001);

    p.lifetime = 0.5;
    p.update_visuals();
    assert!((p.size - 15.0).abs() < 0.001);

    p.lifetime = 1.0;
    p.update_visuals();
    assert!((p.size - 20.0).abs() < 0.001);
  }

  #[test]
  fn particle_simulate_gravity() {
    let mut p = Particle {
      velocity_y: 0.0,
      gravity_scale: 1.0,
      ..Default::default()
    };

    p.simulate(1.0, 0.0, 100.0);
    assert!((p.velocity_y - 100.0).abs() < 0.001);
  }

  #[test]
  fn particle_simulate_drag() {
    let mut p = Particle {
      velocity_x: 100.0,
      drag: 0.5,
      ..Default::default()
    };

    p.simulate(1.0, 0.0, 0.0);
    assert!((p.velocity_x - 50.0).abs() < 0.001);
  }
}
