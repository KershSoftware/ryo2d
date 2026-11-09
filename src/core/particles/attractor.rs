//! Particle attractor component
//!
//! Attractors influence particle motion, creating effects like gravity wells,
//! vortices, and turbulence.

use glam::Vec2;

use super::noise::noise_force;
use crate::core::ecs::Component;

/// Type of attractor force.
#[derive(Debug, Clone, Copy)]
pub enum AttractorType {
  /// Point attractor - pulls/pushes particles toward/away from a point.
  Point {
    /// Strength of attraction (negative = repel)
    strength: f32,
    /// Maximum radius of effect
    radius: f32,
    /// Falloff exponent (1.0 = linear, 2.0 = inverse square)
    falloff: f32,
  },
  /// Line attractor - pulls particles toward a line segment.
  Line {
    /// Start offset from attractor position
    start_x: f32,
    start_y: f32,
    /// End offset from attractor position
    end_x: f32,
    end_y: f32,
    /// Strength of attraction
    strength: f32,
    /// Maximum radius of effect
    radius: f32,
  },
  /// Area attractor - applies uniform force within bounds.
  Area {
    /// Width of area (centered on position)
    width: f32,
    /// Height of area (centered on position)
    height: f32,
    /// Force direction and magnitude
    force_x: f32,
    force_y: f32,
  },
  /// Vortex - swirls particles around the attractor.
  Vortex {
    /// Tangential force strength (positive = counter-clockwise)
    strength: f32,
    /// Maximum radius of effect
    radius: f32,
    /// Inward pull strength (0 = pure rotation, positive = spiral in)
    inward_pull: f32,
  },
  /// Turbulence - applies noise-based chaotic motion.
  Turbulence {
    /// Strength of turbulent force
    strength: f32,
    /// Scale of noise pattern (larger = bigger swirls)
    scale: f32,
    /// Animation speed of turbulence
    speed: f32,
  },
}

/// Particle attractor component.
///
/// Attach to an entity with a Transform to create a force field that affects particles.
/// Multiple attractors can affect the same particles (forces are combined).
///
/// # Example
///
/// ```ignore
/// use ryo2d::prelude::*;
///
/// // Create a gravity well at mouse position
/// let attractor = ctx.world.spawn((
///     Transform::from_xy(mouse_x, mouse_y),
///     ParticleAttractor::point(200.0, 100.0),
/// )).id();
/// ```
#[derive(Component, Debug, Clone)]
pub struct ParticleAttractor {
  /// Type and parameters of the attractor
  pub attractor_type: AttractorType,
  /// Bitmask of particle layers this attractor affects (0 = all)
  pub layer_mask: u32,
  /// Whether the attractor is currently active
  pub enabled: bool,
  /// Internal: time accumulator for turbulence animation
  pub(crate) time: f32,
}

impl ParticleAttractor {
  /// Create a new attractor with the given type.
  pub fn new(attractor_type: AttractorType) -> Self {
    Self {
      attractor_type,
      layer_mask: 0,
      enabled: true,
      time: 0.0,
    }
  }

  // =========================================================================
  // Factory methods
  // =========================================================================

  /// Create a point attractor (gravity well).
  ///
  /// # Arguments
  /// * `strength` - Force strength (positive = attract, negative = repel)
  /// * `radius` - Maximum effect radius
  pub fn point(strength: f32, radius: f32) -> Self {
    Self::new(AttractorType::Point {
      strength,
      radius,
      falloff: 1.0,
    })
  }

  /// Create a point attractor with inverse-square falloff (realistic gravity).
  pub fn gravity_well(strength: f32, radius: f32) -> Self {
    Self::new(AttractorType::Point {
      strength,
      radius,
      falloff: 2.0,
    })
  }

  /// Create a line attractor.
  pub fn line(
    start_x: f32,
    start_y: f32,
    end_x: f32,
    end_y: f32,
    strength: f32,
    radius: f32,
  ) -> Self {
    Self::new(AttractorType::Line {
      start_x,
      start_y,
      end_x,
      end_y,
      strength,
      radius,
    })
  }

  /// Create an area attractor (uniform directional force).
  ///
  /// # Arguments
  /// * `width` - Width of the force area (centered on position)
  /// * `height` - Height of the force area (centered on position)
  /// * `force_x` - Horizontal force component
  /// * `force_y` - Vertical force component
  pub fn area(width: f32, height: f32, force_x: f32, force_y: f32) -> Self {
    Self::new(AttractorType::Area {
      width,
      height,
      force_x,
      force_y,
    })
  }

  /// Create a vortex attractor (swirling force).
  ///
  /// # Arguments
  /// * `strength` - Tangential force (positive = counter-clockwise)
  /// * `radius` - Maximum effect radius
  pub fn vortex(strength: f32, radius: f32) -> Self {
    Self::new(AttractorType::Vortex {
      strength,
      radius,
      inward_pull: 0.3,
    })
  }

  /// Create a spiral attractor (vortex with strong inward pull).
  ///
  /// # Arguments
  /// * `strength` - Tangential force
  /// * `radius` - Maximum effect radius
  /// * `inward_pull` - Inward pull strength
  pub fn spiral(strength: f32, radius: f32, inward_pull: f32) -> Self {
    Self::new(AttractorType::Vortex {
      strength,
      radius,
      inward_pull,
    })
  }

  /// Create a turbulence attractor (chaotic noise-based motion).
  ///
  /// # Arguments
  /// * `strength` - Force magnitude
  /// * `scale` - Noise scale (larger = bigger swirls)
  /// * `speed` - Animation speed
  pub fn turbulence(strength: f32, scale: f32, speed: f32) -> Self {
    Self::new(AttractorType::Turbulence {
      strength,
      scale,
      speed,
    })
  }

  // =========================================================================
  // Builder methods
  // =========================================================================

  /// Set the layer mask (0 = affect all particles).
  pub fn with_layer_mask(mut self, mask: u32) -> Self {
    self.layer_mask = mask;
    self
  }

  /// Set whether the attractor is enabled.
  pub fn with_enabled(mut self, enabled: bool) -> Self {
    self.enabled = enabled;
    self
  }

  // =========================================================================
  // Force calculation
  // =========================================================================

  /// Calculate the force this attractor applies to a particle.
  ///
  /// Returns (force_x, force_y) in pixels per second squared.
  pub fn calculate_force(
    &self,
    attractor_x: f32,
    attractor_y: f32,
    particle_x: f32,
    particle_y: f32,
    particle_layer: u32,
  ) -> (f32, f32) {
    if !self.enabled {
      return (0.0, 0.0);
    }

    // Check layer mask
    if self.layer_mask != 0 && (particle_layer & self.layer_mask) == 0 {
      return (0.0, 0.0);
    }

    self
      .attractor_type
      .force(self.time, attractor_x, attractor_y, particle_x, particle_y)
  }

  /// Update internal time (for turbulence animation).
  pub(crate) fn update(&mut self, dt: f32) {
    if let AttractorType::Turbulence { speed, .. } = &self.attractor_type {
      self.time += dt * speed;
    }
  }
}

impl AttractorType {
  /// The force an attractor of this type at `time` (its turbulence's)
  /// applies to a particle, whatever its layer: (force_x, force_y) in
  /// pixels per second squared.
  pub(crate) fn force(
    &self,
    time: f32,
    attractor_x: f32,
    attractor_y: f32,
    particle_x: f32,
    particle_y: f32,
  ) -> (f32, f32) {
    match self {
      AttractorType::Point {
        strength,
        radius,
        falloff,
      } => {
        let dx = attractor_x - particle_x;
        let dy = attractor_y - particle_y;
        let dist_sq = dx * dx + dy * dy;
        let dist = dist_sq.sqrt();

        if dist > *radius || dist < 0.001 {
          return (0.0, 0.0);
        }

        // Normalized direction
        let nx = dx / dist;
        let ny = dy / dist;

        // Force magnitude with falloff
        let t = 1.0 - (dist / radius);
        let magnitude = strength * t.powf(*falloff);

        (nx * magnitude, ny * magnitude)
      }

      AttractorType::Line {
        start_x,
        start_y,
        end_x,
        end_y,
        strength,
        radius,
      } => {
        // World-space line endpoints
        let lx1 = attractor_x + start_x;
        let ly1 = attractor_y + start_y;
        let lx2 = attractor_x + end_x;
        let ly2 = attractor_y + end_y;

        // Find closest point on line segment
        let (closest_x, closest_y) =
          closest_point_on_segment(particle_x, particle_y, lx1, ly1, lx2, ly2);

        let dx = closest_x - particle_x;
        let dy = closest_y - particle_y;
        let dist = (dx * dx + dy * dy).sqrt();

        if dist > *radius || dist < 0.001 {
          return (0.0, 0.0);
        }

        let nx = dx / dist;
        let ny = dy / dist;
        let t = 1.0 - (dist / radius);
        let magnitude = strength * t;

        (nx * magnitude, ny * magnitude)
      }

      AttractorType::Area {
        width,
        height,
        force_x,
        force_y,
      } => {
        // Check if particle is within bounds
        let half_w = width * 0.5;
        let half_h = height * 0.5;
        let dx = particle_x - attractor_x;
        let dy = particle_y - attractor_y;

        if dx.abs() <= half_w && dy.abs() <= half_h {
          (*force_x, *force_y)
        } else {
          (0.0, 0.0)
        }
      }

      AttractorType::Vortex {
        strength,
        radius,
        inward_pull,
      } => {
        let dx = particle_x - attractor_x;
        let dy = particle_y - attractor_y;
        let dist = (dx * dx + dy * dy).sqrt();

        if dist > *radius || dist < 0.001 {
          return (0.0, 0.0);
        }

        // Normalized direction from center
        let nx = dx / dist;
        let ny = dy / dist;

        // Tangent (perpendicular) - counter-clockwise
        let tx = -ny;
        let ty = nx;

        let t = 1.0 - (dist / radius);

        // Tangential force (rotation)
        let rot_magnitude = strength * t;

        // Radial force (inward pull)
        let rad_magnitude = -inward_pull * t;

        (
          tx * rot_magnitude + nx * rad_magnitude,
          ty * rot_magnitude + ny * rad_magnitude,
        )
      }

      AttractorType::Turbulence {
        strength,
        scale,
        speed: _,
      } => {
        let frequency = if scale.is_finite() && *scale > 0.0 {
          1.0 / scale
        } else {
          0.0
        };
        let force = noise_force(0, Vec2::new(particle_x, particle_y), time, frequency) * *strength;
        (force.x, force.y)
      }
    }
  }
}

/// Find the closest point on a line segment to a given point.
fn closest_point_on_segment(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> (f32, f32) {
  let abx = bx - ax;
  let aby = by - ay;
  let apx = px - ax;
  let apy = py - ay;

  let ab_sq = abx * abx + aby * aby;
  if ab_sq < 0.0001 {
    return (ax, ay);
  }

  let t = ((apx * abx + apy * aby) / ab_sq).clamp(0.0, 1.0);
  (ax + t * abx, ay + t * aby)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn point_attractor_attraction() {
    let attractor = ParticleAttractor::point(100.0, 50.0);

    // Particle at (10, 0), attractor at (0, 0)
    let (fx, fy) = attractor.calculate_force(0.0, 0.0, 10.0, 0.0, 0);

    // Force should point toward attractor (negative x)
    assert!(fx < 0.0);
    assert!(fy.abs() < 0.001);
  }

  #[test]
  fn point_attractor_repulsion() {
    let attractor = ParticleAttractor::point(-100.0, 50.0);

    let (fx, _fy) = attractor.calculate_force(0.0, 0.0, 10.0, 0.0, 0);

    // Force should point away (positive x)
    assert!(fx > 0.0);
  }

  #[test]
  fn point_attractor_out_of_range() {
    let attractor = ParticleAttractor::point(100.0, 50.0);

    // Particle at 100 units away, radius is 50
    let (fx, fy) = attractor.calculate_force(0.0, 0.0, 100.0, 0.0, 0);

    assert_eq!(fx, 0.0);
    assert_eq!(fy, 0.0);
  }

  #[test]
  fn layer_mask_filtering() {
    let attractor = ParticleAttractor::point(100.0, 50.0).with_layer_mask(0b0001);

    // Matching layer
    let (fx, _) = attractor.calculate_force(0.0, 0.0, 10.0, 0.0, 0b0001);
    assert!(fx != 0.0);

    // Non-matching layer
    let (fx, _) = attractor.calculate_force(0.0, 0.0, 10.0, 0.0, 0b0010);
    assert_eq!(fx, 0.0);
  }

  #[test]
  fn disabled_attractor() {
    let attractor = ParticleAttractor::point(100.0, 50.0).with_enabled(false);

    let (fx, fy) = attractor.calculate_force(0.0, 0.0, 10.0, 0.0, 0);
    assert_eq!(fx, 0.0);
    assert_eq!(fy, 0.0);
  }

  #[test]
  fn vortex_attractor() {
    let attractor = ParticleAttractor::vortex(100.0, 50.0);

    // Particle directly to the right of vortex center
    let (fx, fy) = attractor.calculate_force(0.0, 0.0, 10.0, 0.0, 0);

    // Tangential force should be perpendicular (mostly Y for particle on X axis)
    assert!(fy.abs() > fx.abs());
  }

  #[test]
  fn area_attractor() {
    let attractor = ParticleAttractor::area(100.0, 100.0, 30.0, 0.0);

    // Particle inside the area
    let (fx, fy) = attractor.calculate_force(0.0, 0.0, 10.0, 10.0, 0);
    assert!((fx - 30.0).abs() < 0.001);
    assert_eq!(fy, 0.0);

    // Particle outside the area
    let (fx, fy) = attractor.calculate_force(0.0, 0.0, 200.0, 0.0, 0);
    assert_eq!(fx, 0.0);
    assert_eq!(fy, 0.0);
  }

  #[test]
  fn turbulence_uses_smooth_noise() {
    let attractor = ParticleAttractor::turbulence(100.0, 50.0, 1.0);
    let at = |x: f32, time: f32| attractor.attractor_type.force(time, 0.0, 0.0, x, 20.0);
    // Smooth in space and in time, and bounded by the strength
    for i in 0..100 {
      let x = i as f32 * 3.7;
      let (fx, fy) = at(x, 0.5);
      assert!(fx.abs() <= 100.0 && fy.abs() <= 100.0);
      let (nx, ny) = at(x + 0.05, 0.5);
      assert!((fx - nx).abs() < 2.0 && (fy - ny).abs() < 2.0);
      let (tx, ty) = at(x, 0.5 + 0.001);
      assert!((fx - tx).abs() < 2.0 && (fy - ty).abs() < 2.0);
    }
    // It varies, and repeats
    assert_ne!(at(10.0, 0.0), at(80.0, 0.0));
    assert_eq!(at(10.0, 0.3), at(10.0, 0.3));
  }
}
