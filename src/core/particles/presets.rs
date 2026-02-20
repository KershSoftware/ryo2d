//! Built-in particle effect presets
//!
//! Ready-to-use emitter configurations for common effects.

use super::collision::CollisionConfig;
use super::emitter::{EmissionMode, EmissionShape, ParticleEmitter};
use crate::core::asset::SpriteHandle;
use crate::core::ecs::SpriteEffect;
use crate::core::ui::Color;

impl ParticleEmitter {
  // ===========================================================================
  // Fire Effects
  // ===========================================================================

  /// Create a fire effect.
  ///
  /// Orange-yellow particles rising upward with slight spread.
  pub fn fire(sprite: SpriteHandle) -> Self {
    Self::new(sprite)
      .with_mode(EmissionMode::Continuous { rate: 30.0 })
      .with_shape(EmissionShape::Cone {
        direction: -std::f32::consts::FRAC_PI_2, // Up
        spread: 0.5,
        min_speed: 30.0,
        max_speed: 60.0,
      })
      .with_lifetime(0.5, 1.0)
      .with_size(12.0, 20.0)
      .with_end_size(2.0, 6.0)
      .with_color(Color::new(255, 200, 50, 255), Color::new(255, 50, 0, 0))
      .with_drag(0.5)
      .with_effect(SpriteEffect::glow_with_intensity(
        Color::new(255, 150, 50, 255),
        4.0,
        0.8,
      ))
  }

  /// Create a small flame (torch, candle).
  pub fn flame_small(sprite: SpriteHandle) -> Self {
    Self::new(sprite)
      .with_mode(EmissionMode::Continuous { rate: 15.0 })
      .with_shape(EmissionShape::Cone {
        direction: -std::f32::consts::FRAC_PI_2,
        spread: 0.3,
        min_speed: 15.0,
        max_speed: 30.0,
      })
      .with_lifetime(0.3, 0.6)
      .with_size(6.0, 10.0)
      .with_end_size(1.0, 3.0)
      .with_color(Color::new(255, 220, 100, 255), Color::new(255, 100, 0, 0))
      .with_drag(0.3)
  }

  // ===========================================================================
  // Smoke Effects
  // ===========================================================================

  /// Create a smoke effect.
  ///
  /// Slow-expanding gray particles drifting upward.
  pub fn smoke(sprite: SpriteHandle) -> Self {
    Self::new(sprite)
      .with_mode(EmissionMode::Continuous { rate: 10.0 })
      .with_shape(EmissionShape::Circle {
        radius: 5.0,
        hollow: false,
      })
      .with_lifetime(2.0, 4.0)
      .with_size(8.0, 16.0)
      .with_end_size(30.0, 50.0)
      .with_color(Color::new(100, 100, 100, 180), Color::new(50, 50, 50, 0))
      .with_gravity(0.0, -10.0)
      .with_drag(0.8)
      .with_angular_velocity(-0.5, 0.5)
  }

  /// Create a thick smoke effect (fires, destruction).
  pub fn smoke_thick(sprite: SpriteHandle) -> Self {
    Self::new(sprite)
      .with_mode(EmissionMode::Continuous { rate: 20.0 })
      .with_shape(EmissionShape::Circle {
        radius: 10.0,
        hollow: false,
      })
      .with_lifetime(3.0, 5.0)
      .with_size(16.0, 24.0)
      .with_end_size(60.0, 100.0)
      .with_color(Color::new(60, 60, 60, 200), Color::new(30, 30, 30, 0))
      .with_gravity(0.0, -15.0)
      .with_drag(0.9)
      .with_angular_velocity(-0.3, 0.3)
  }

  // ===========================================================================
  // Explosion Effects
  // ===========================================================================

  /// Create an explosion effect (one-shot burst).
  ///
  /// Radial burst of particles with trails.
  pub fn explosion(sprite: SpriteHandle, count: u32) -> Self {
    Self::new(sprite)
      .with_mode(EmissionMode::Burst { count })
      .with_shape(EmissionShape::Circle {
        radius: 1.0,
        hollow: false,
      })
      .with_lifetime(0.3, 0.8)
      .with_size(8.0, 16.0)
      .with_end_size(2.0, 4.0)
      .with_speed(100.0, 300.0)
      .with_color(Color::new(255, 200, 100, 255), Color::new(255, 50, 0, 0))
      .with_drag(2.0)
      .with_effect(SpriteEffect::glow_with_intensity(
        Color::new(255, 150, 50, 255),
        3.0,
        0.6,
      ))
  }

  /// Create a debris explosion (physical particles).
  pub fn explosion_debris(sprite: SpriteHandle, count: u32) -> Self {
    Self::new(sprite)
      .with_mode(EmissionMode::Burst { count })
      .with_shape(EmissionShape::Circle {
        radius: 5.0,
        hollow: false,
      })
      .with_lifetime(1.0, 2.0)
      .with_size(4.0, 8.0)
      .with_end_size(4.0, 8.0)
      .with_speed(50.0, 150.0)
      .with_color(Color::new(150, 120, 80, 255), Color::new(80, 60, 40, 0))
      .with_gravity(0.0, 200.0)
      .with_drag(0.5)
      .with_rotation(0.0, std::f32::consts::TAU)
      .with_angular_velocity(-5.0, 5.0)
      .with_collision(CollisionConfig::bouncy(2.0, 0.6))
  }

  // ===========================================================================
  // Magic Effects
  // ===========================================================================

  /// Create a magic sparkle effect.
  ///
  /// Glowing particles that wander and fade.
  pub fn magic(sprite: SpriteHandle) -> Self {
    Self::new(sprite)
      .with_mode(EmissionMode::Continuous { rate: 20.0 })
      .with_shape(EmissionShape::Circle {
        radius: 15.0,
        hollow: false,
      })
      .with_lifetime(1.0, 2.0)
      .with_size(4.0, 8.0)
      .with_end_size(1.0, 2.0)
      .with_color(Color::new(150, 100, 255, 255), Color::new(50, 200, 255, 0))
      .with_speed(10.0, 30.0)
      .with_drag(1.0)
      .with_effect(SpriteEffect::glow_with_intensity(
        Color::new(100, 150, 255, 255),
        6.0,
        1.0,
      ))
  }

  /// Create a healing/buff effect.
  pub fn magic_heal(sprite: SpriteHandle) -> Self {
    Self::new(sprite)
      .with_mode(EmissionMode::Continuous { rate: 15.0 })
      .with_shape(EmissionShape::Circle {
        radius: 20.0,
        hollow: true,
      })
      .with_lifetime(0.8, 1.5)
      .with_size(6.0, 10.0)
      .with_end_size(2.0, 4.0)
      .with_color(Color::new(100, 255, 150, 255), Color::new(200, 255, 200, 0))
      .with_gravity(0.0, -30.0)
      .with_drag(0.5)
      .with_effect(SpriteEffect::glow_with_intensity(
        Color::new(150, 255, 150, 255),
        4.0,
        0.8,
      ))
  }

  // ===========================================================================
  // Weather Effects
  // ===========================================================================

  /// Create a rain effect.
  ///
  /// Fast downward streaks.
  pub fn rain(sprite: SpriteHandle) -> Self {
    Self::new(sprite)
      .with_mode(EmissionMode::Continuous { rate: 100.0 })
      .with_shape(EmissionShape::Line {
        start_x: -200.0,
        start_y: -50.0,
        end_x: 200.0,
        end_y: -50.0,
      })
      .with_lifetime(0.5, 1.0)
      .with_size(2.0, 3.0)
      .with_end_size(2.0, 3.0)
      .with_color(
        Color::new(180, 200, 255, 200),
        Color::new(180, 200, 255, 100),
      )
      .with_gravity(50.0, 800.0) // Slight angle + fast fall
  }

  /// Create a snow effect.
  ///
  /// Gentle floating descent with wobble.
  pub fn snow(sprite: SpriteHandle) -> Self {
    Self::new(sprite)
      .with_mode(EmissionMode::Continuous { rate: 30.0 })
      .with_shape(EmissionShape::Line {
        start_x: -200.0,
        start_y: -50.0,
        end_x: 200.0,
        end_y: -50.0,
      })
      .with_lifetime(3.0, 6.0)
      .with_size(3.0, 6.0)
      .with_end_size(3.0, 6.0)
      .with_color(Color::WHITE, Color::new(255, 255, 255, 100))
      .with_gravity(0.0, 20.0)
      .with_drag(0.5)
      .with_angular_velocity(-1.0, 1.0)
  }

  // ===========================================================================
  // Impact Effects
  // ===========================================================================

  /// Create a sparks effect.
  ///
  /// Bright bouncing particles.
  pub fn sparks(sprite: SpriteHandle) -> Self {
    Self::new(sprite)
      .with_mode(EmissionMode::Burst { count: 20 })
      .with_shape(EmissionShape::Cone {
        direction: -std::f32::consts::FRAC_PI_2,
        spread: std::f32::consts::PI,
        min_speed: 50.0,
        max_speed: 150.0,
      })
      .with_lifetime(0.5, 1.5)
      .with_size(2.0, 4.0)
      .with_end_size(1.0, 2.0)
      .with_color(Color::new(255, 255, 200, 255), Color::new(255, 150, 50, 0))
      .with_gravity(0.0, 300.0)
      .with_drag(0.3)
      .with_collision(CollisionConfig::bouncy(1.0, 0.7))
  }

  /// Create a dust impact effect.
  pub fn dust(sprite: SpriteHandle) -> Self {
    Self::new(sprite)
      .with_mode(EmissionMode::Burst { count: 10 })
      .with_shape(EmissionShape::Cone {
        direction: -std::f32::consts::FRAC_PI_2,
        spread: std::f32::consts::PI * 0.8,
        min_speed: 20.0,
        max_speed: 50.0,
      })
      .with_lifetime(0.3, 0.8)
      .with_size(4.0, 8.0)
      .with_end_size(12.0, 20.0)
      .with_color(Color::new(180, 160, 140, 150), Color::new(150, 130, 110, 0))
      .with_drag(3.0)
  }

  // ===========================================================================
  // Celebration Effects
  // ===========================================================================

  /// Create a confetti effect.
  ///
  /// Multi-colored tumbling particles.
  pub fn confetti(sprite: SpriteHandle) -> Self {
    Self::new(sprite)
      .with_mode(EmissionMode::Burst { count: 50 })
      .with_shape(EmissionShape::Cone {
        direction: -std::f32::consts::FRAC_PI_2,
        spread: std::f32::consts::PI * 0.6,
        min_speed: 100.0,
        max_speed: 200.0,
      })
      .with_lifetime(2.0, 4.0)
      .with_size(6.0, 10.0)
      .with_end_size(6.0, 10.0)
      .with_color(Color::WHITE, Color::new(255, 255, 255, 100))
      .with_gravity(0.0, 100.0)
      .with_drag(0.5)
      .with_rotation(0.0, std::f32::consts::TAU)
      .with_angular_velocity(-8.0, 8.0)
  }

  /// Create a bubble effect.
  pub fn bubbles(sprite: SpriteHandle) -> Self {
    Self::new(sprite)
      .with_mode(EmissionMode::Continuous { rate: 5.0 })
      .with_shape(EmissionShape::Circle {
        radius: 20.0,
        hollow: false,
      })
      .with_lifetime(2.0, 4.0)
      .with_size(4.0, 12.0)
      .with_end_size(8.0, 16.0)
      .with_color(
        Color::new(200, 220, 255, 150),
        Color::new(200, 220, 255, 50),
      )
      .with_gravity(0.0, -30.0)
      .with_drag(0.3)
  }

  // ===========================================================================
  // Trail Effects
  // ===========================================================================

  /// Create a comet/projectile trail.
  pub fn comet_trail(sprite: SpriteHandle) -> Self {
    Self::new(sprite)
      .with_mode(EmissionMode::Continuous { rate: 60.0 })
      .with_shape(EmissionShape::Point)
      .with_lifetime(0.2, 0.4)
      .with_size(8.0, 12.0)
      .with_end_size(2.0, 4.0)
      .with_color(Color::new(255, 200, 100, 255), Color::new(255, 100, 0, 0))
      .with_speed(0.0, 10.0)
      .with_drag(0.5)
      .with_effect(SpriteEffect::glow_with_intensity(
        Color::new(255, 150, 50, 255),
        4.0,
        0.6,
      ))
  }

  /// Create an engine exhaust effect.
  pub fn exhaust(sprite: SpriteHandle) -> Self {
    Self::new(sprite)
      .with_mode(EmissionMode::Continuous { rate: 40.0 })
      .with_shape(EmissionShape::Cone {
        direction: std::f32::consts::FRAC_PI_2, // Down
        spread: 0.3,
        min_speed: 50.0,
        max_speed: 100.0,
      })
      .with_lifetime(0.2, 0.5)
      .with_size(6.0, 10.0)
      .with_end_size(12.0, 20.0)
      .with_color(Color::new(255, 150, 50, 200), Color::new(100, 100, 100, 0))
      .with_drag(1.0)
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
  fn fire_preset() {
    let emitter = ParticleEmitter::fire(test_sprite());
    assert!(emitter.enabled);
    assert!(matches!(emitter.mode, EmissionMode::Continuous { .. }));
    assert!(matches!(emitter.shape, EmissionShape::Cone { .. }));
  }

  #[test]
  fn explosion_preset() {
    let emitter = ParticleEmitter::explosion(test_sprite(), 30);
    assert!(matches!(emitter.mode, EmissionMode::Burst { count: 30 }));
  }

  #[test]
  fn sparks_preset() {
    let emitter = ParticleEmitter::sparks(test_sprite());
    assert!(emitter.collision.is_some());
  }
}
