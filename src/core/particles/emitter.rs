//! Particle emitter component
//!
//! Emitters spawn particles with configurable shapes, rates, and properties.

use crate::core::asset::SpriteHandle;
use crate::core::ecs::SpriteEffect;
use crate::core::ui::Color;

/// How particles are emitted from the emitter.
#[derive(Debug, Clone)]
pub enum EmissionMode {
  /// Emit particles continuously at a fixed rate.
  Continuous {
    /// Particles per second
    rate: f32,
  },
  /// Emit a single burst of particles (one-shot).
  Burst {
    /// Number of particles to emit
    count: u32,
  },
  /// Emit bursts at regular intervals.
  BurstRepeating {
    /// Particles per burst
    count: u32,
    /// Seconds between bursts
    interval: f32,
    /// Number of bursts (None = infinite)
    repeats: Option<u32>,
  },
}

impl Default for EmissionMode {
  fn default() -> Self {
    EmissionMode::Continuous { rate: 10.0 }
  }
}

/// Shape from which particles are spawned.
#[derive(Debug, Clone, Default)]
pub enum EmissionShape {
  /// Emit from a single point (the emitter's position).
  #[default]
  Point,
  /// Emit from a line segment.
  Line {
    /// Start offset from emitter position
    start_x: f32,
    start_y: f32,
    /// End offset from emitter position
    end_x: f32,
    end_y: f32,
  },
  /// Emit from a circle.
  Circle {
    /// Radius of the circle
    radius: f32,
    /// If true, emit only from the edge; if false, fill the circle
    hollow: bool,
  },
  /// Emit from a rectangle.
  Rectangle {
    /// Width of rectangle
    width: f32,
    /// Height of rectangle
    height: f32,
  },
  /// Emit in a cone pattern with directional velocity.
  Cone {
    /// Base direction in radians (0 = right, PI/2 = down)
    direction: f32,
    /// Spread angle in radians (full cone width)
    spread: f32,
    /// Minimum initial speed
    min_speed: f32,
    /// Maximum initial speed
    max_speed: f32,
  },
}

/// Configuration for particle trails.
#[derive(Debug, Clone)]
pub struct TrailConfig {
  /// Trail color (None = inherit from particle)
  pub color: Option<Color>,
  /// Trail width at head
  pub width: f32,
  /// Trail width at tail
  pub end_width: f32,
  /// Maximum number of trail points
  pub max_points: usize,
  /// Minimum distance between trail points
  pub min_distance: f32,
  /// Whether trail fades to transparent
  pub fade: bool,
}

impl Default for TrailConfig {
  fn default() -> Self {
    Self {
      color: None,
      width: 3.0,
      end_width: 0.0,
      max_points: 10,
      min_distance: 2.0,
      fade: true,
    }
  }
}

impl TrailConfig {
  /// Create a new trail config with specified width.
  pub fn new(width: f32) -> Self {
    Self {
      width,
      ..Default::default()
    }
  }

  /// Set trail color (None = inherit from particle).
  pub fn with_color(mut self, color: Color) -> Self {
    self.color = Some(color);
    self
  }

  /// Set trail end width for tapering.
  pub fn with_end_width(mut self, end_width: f32) -> Self {
    self.end_width = end_width;
    self
  }

  /// Set maximum trail points.
  pub fn with_max_points(mut self, max_points: usize) -> Self {
    self.max_points = max_points;
    self
  }

  /// Set minimum distance between points.
  pub fn with_min_distance(mut self, min_distance: f32) -> Self {
    self.min_distance = min_distance;
    self
  }

  /// Set whether trail fades.
  pub fn with_fade(mut self, fade: bool) -> Self {
    self.fade = fade;
    self
  }
}

/// Particle emitter component.
///
/// Attach to an entity with a Transform to create a particle effect at that location.
/// The emitter spawns particles according to its configuration.
///
/// # Example
///
/// ```ignore
/// use ryo2d::prelude::*;
///
/// // Simple fire effect
/// let fire_sprite = ctx.assets.load_sprite("particle.png");
/// ctx.world.spawn((
///     Transform::at(100.0, 100.0),
///     ParticleEmitter::fire(fire_sprite),
/// ));
///
/// // Custom emitter
/// ctx.world.spawn((
///     Transform::at(200.0, 200.0),
///     ParticleEmitter::new(sprite)
///         .with_mode(EmissionMode::Continuous { rate: 50.0 })
///         .with_shape(EmissionShape::Circle { radius: 10.0, hollow: false })
///         .with_lifetime(1.0, 2.0)
///         .with_color(Color::CYAN, Color::TRANSPARENT),
/// ));
/// ```
#[derive(Debug, Clone)]
pub struct ParticleEmitter {
  // =========================================================================
  // Core settings
  // =========================================================================
  /// Emission mode (continuous, burst, repeating burst)
  pub mode: EmissionMode,
  /// Emission shape
  pub shape: EmissionShape,
  /// Sprite for particles
  pub sprite: SpriteHandle,
  /// Whether the emitter is currently emitting
  pub enabled: bool,

  // =========================================================================
  // Lifetime (randomized per particle)
  // =========================================================================
  /// Minimum particle lifetime in seconds
  pub lifetime_min: f32,
  /// Maximum particle lifetime in seconds
  pub lifetime_max: f32,

  // =========================================================================
  // Size (randomized per particle, interpolated over lifetime)
  // =========================================================================
  /// Minimum starting size
  pub size_min: f32,
  /// Maximum starting size
  pub size_max: f32,
  /// Minimum ending size
  pub size_end_min: f32,
  /// Maximum ending size
  pub size_end_max: f32,

  // =========================================================================
  // Color (interpolated over lifetime)
  // =========================================================================
  /// Color at spawn
  pub color_start: Color,
  /// Color at death
  pub color_end: Color,

  // =========================================================================
  // Speed (randomized per particle, for non-Cone shapes)
  // =========================================================================
  /// Minimum initial speed
  pub speed_min: f32,
  /// Maximum initial speed
  pub speed_max: f32,

  // =========================================================================
  // Rotation
  // =========================================================================
  /// Minimum initial rotation in radians
  pub rotation_min: f32,
  /// Maximum initial rotation in radians
  pub rotation_max: f32,
  /// Minimum angular velocity in radians/second
  pub angular_velocity_min: f32,
  /// Maximum angular velocity in radians/second
  pub angular_velocity_max: f32,

  // =========================================================================
  // Physics
  // =========================================================================
  /// Gravity applied to particles (x, y)
  pub gravity: (f32, f32),
  /// Velocity damping (0-1, per second)
  pub drag: f32,

  // =========================================================================
  // Rendering
  // =========================================================================
  /// Z-order for sorting
  pub z_order: i32,
  /// Optional sprite effect (glow, dissolve, etc.)
  pub effect: Option<SpriteEffect>,

  // =========================================================================
  // Features
  // =========================================================================
  /// Optional trail configuration
  pub trail: Option<TrailConfig>,
  /// Optional collision configuration (imported from collision.rs)
  pub collision: Option<super::collision::CollisionConfig>,
  /// Maximum particles this emitter can have alive at once
  pub max_particles: u32,

  // =========================================================================
  // Internal state
  // =========================================================================
  /// Accumulator for continuous emission
  pub(crate) emission_accumulator: f32,
  /// Time since last burst (for repeating bursts)
  pub(crate) burst_timer: f32,
  /// Bursts remaining (for repeating bursts)
  pub(crate) bursts_remaining: Option<u32>,
  /// Whether a one-shot burst has fired
  pub(crate) burst_fired: bool,
  /// Current particle count (tracked by system)
  pub(crate) particle_count: u32,
}

impl ParticleEmitter {
  /// Create a new particle emitter with the given sprite.
  pub fn new(sprite: SpriteHandle) -> Self {
    Self {
      mode: EmissionMode::default(),
      shape: EmissionShape::default(),
      sprite,
      enabled: true,
      lifetime_min: 1.0,
      lifetime_max: 2.0,
      size_min: 8.0,
      size_max: 16.0,
      size_end_min: 0.0,
      size_end_max: 0.0,
      color_start: Color::WHITE,
      color_end: Color::TRANSPARENT,
      speed_min: 0.0,
      speed_max: 50.0,
      rotation_min: 0.0,
      rotation_max: 0.0,
      angular_velocity_min: 0.0,
      angular_velocity_max: 0.0,
      gravity: (0.0, 0.0),
      drag: 0.0,
      z_order: 0,
      effect: None,
      trail: None,
      collision: None,
      max_particles: 1000,
      emission_accumulator: 0.0,
      burst_timer: 0.0,
      bursts_remaining: None,
      burst_fired: false,
      particle_count: 0,
    }
  }

  // =========================================================================
  // Builder methods
  // =========================================================================

  /// Set emission mode.
  pub fn with_mode(mut self, mode: EmissionMode) -> Self {
    self.mode = mode;
    // Reset burst state for repeating bursts
    if let EmissionMode::BurstRepeating { repeats, .. } = &self.mode {
      self.bursts_remaining = *repeats;
    }
    self
  }

  /// Set emission shape.
  pub fn with_shape(mut self, shape: EmissionShape) -> Self {
    self.shape = shape;
    self
  }

  /// Set particle lifetime range.
  pub fn with_lifetime(mut self, min: f32, max: f32) -> Self {
    self.lifetime_min = min.max(0.01);
    self.lifetime_max = max.max(min);
    self
  }

  /// Set particle starting size range.
  pub fn with_size(mut self, min: f32, max: f32) -> Self {
    self.size_min = min.max(0.1);
    self.size_max = max.max(min);
    self
  }

  /// Set particle ending size range.
  pub fn with_end_size(mut self, min: f32, max: f32) -> Self {
    self.size_end_min = min.max(0.0);
    self.size_end_max = max.max(min);
    self
  }

  /// Set particle color gradient (start to end over lifetime).
  pub fn with_color(mut self, start: Color, end: Color) -> Self {
    self.color_start = start;
    self.color_end = end;
    self
  }

  /// Set particle initial speed range.
  pub fn with_speed(mut self, min: f32, max: f32) -> Self {
    self.speed_min = min.max(0.0);
    self.speed_max = max.max(min);
    self
  }

  /// Set particle rotation range (in radians).
  pub fn with_rotation(mut self, min: f32, max: f32) -> Self {
    self.rotation_min = min;
    self.rotation_max = max;
    self
  }

  /// Set particle angular velocity range (in radians/second).
  pub fn with_angular_velocity(mut self, min: f32, max: f32) -> Self {
    self.angular_velocity_min = min;
    self.angular_velocity_max = max;
    self
  }

  /// Set gravity applied to particles.
  pub fn with_gravity(mut self, x: f32, y: f32) -> Self {
    self.gravity = (x, y);
    self
  }

  /// Set velocity drag (0-1, damping per second).
  pub fn with_drag(mut self, drag: f32) -> Self {
    self.drag = drag.clamp(0.0, 1.0);
    self
  }

  /// Set z-order for rendering.
  pub fn with_z_order(mut self, z_order: i32) -> Self {
    self.z_order = z_order;
    self
  }

  /// Set sprite effect (glow, dissolve, etc.).
  pub fn with_effect(mut self, effect: SpriteEffect) -> Self {
    self.effect = Some(effect);
    self
  }

  /// Enable particle trails.
  pub fn with_trail(mut self, config: TrailConfig) -> Self {
    self.trail = Some(config);
    self
  }

  /// Set collision configuration.
  pub fn with_collision(mut self, config: super::collision::CollisionConfig) -> Self {
    self.collision = Some(config);
    self
  }

  /// Set maximum simultaneous particles.
  pub fn with_max_particles(mut self, max: u32) -> Self {
    self.max_particles = max.max(1);
    self
  }

  /// Convert to a one-shot emitter (emits once then auto-disables).
  pub fn one_shot(mut self) -> Self {
    self.mode = match self.mode {
      EmissionMode::Continuous { rate } => EmissionMode::Burst {
        count: (rate * 0.5).max(1.0) as u32,
      },
      EmissionMode::Burst { count } => EmissionMode::Burst { count },
      EmissionMode::BurstRepeating { count, .. } => EmissionMode::Burst { count },
    };
    self
  }

  // =========================================================================
  // Runtime control
  // =========================================================================

  /// Enable or disable the emitter.
  pub fn set_enabled(&mut self, enabled: bool) {
    self.enabled = enabled;
  }

  /// Reset burst state (allows one-shot to fire again).
  pub fn reset(&mut self) {
    self.burst_fired = false;
    self.emission_accumulator = 0.0;
    self.burst_timer = 0.0;
    if let EmissionMode::BurstRepeating { repeats, .. } = &self.mode {
      self.bursts_remaining = *repeats;
    }
  }

  /// Trigger a manual burst of particles.
  pub fn burst(&mut self, count: u32) {
    // This is handled by the particle system - we just set a flag
    self.emission_accumulator = count as f32;
  }

  /// Check if this emitter has finished (one-shot burst fired, no particles left).
  pub fn is_finished(&self) -> bool {
    match &self.mode {
      EmissionMode::Burst { .. } => self.burst_fired && self.particle_count == 0,
      EmissionMode::BurstRepeating { repeats: Some(0), .. } => self.particle_count == 0,
      _ => false,
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::graphics::Sprite;

  fn test_sprite() -> SpriteHandle {
    SpriteHandle::from_sprite(Sprite::solid_rect(8, 8, 255, 255, 255, 255), "test.png")
  }

  #[test]
  fn emitter_default() {
    let emitter = ParticleEmitter::new(test_sprite());
    assert!(emitter.enabled);
    assert!(matches!(emitter.mode, EmissionMode::Continuous { .. }));
    assert!(matches!(emitter.shape, EmissionShape::Point));
  }

  #[test]
  fn emitter_builder() {
    let emitter = ParticleEmitter::new(test_sprite())
      .with_lifetime(0.5, 1.5)
      .with_size(4.0, 8.0)
      .with_color(Color::RED, Color::TRANSPARENT)
      .with_gravity(0.0, 100.0)
      .with_drag(0.1);

    assert!((emitter.lifetime_min - 0.5).abs() < 0.001);
    assert!((emitter.lifetime_max - 1.5).abs() < 0.001);
    assert_eq!(emitter.color_start, Color::RED);
  }

  #[test]
  fn emitter_one_shot() {
    let emitter = ParticleEmitter::new(test_sprite())
      .with_mode(EmissionMode::Continuous { rate: 100.0 })
      .one_shot();

    assert!(matches!(emitter.mode, EmissionMode::Burst { .. }));
  }
}
