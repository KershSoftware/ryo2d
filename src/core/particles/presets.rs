//! Built-in particle effect presets: [`ParticleEffect`]s to place as they
//! are, or to start from and change with the `with_*` builders.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use super::collision::CollisionConfig;
use super::effect::{Burst, EmitShape, ParticleEffect};
use super::value::{Curve, Gradient};
use crate::core::asset::Handle;
use crate::core::ecs::SpriteEffect;
use crate::core::graphics::Image;
use crate::core::ui::Color;

const UP: f32 = -FRAC_PI_2;
const DOWN: f32 = FRAC_PI_2;

fn circle(radius: f32, hollow: bool) -> EmitShape {
  EmitShape::Circle {
    radius,
    arc_deg: 360.0,
    thickness: if hollow { 0.0 } else { 1.0 },
  }
}

impl ParticleEffect {
  /// A cone of particles, with the angles given in radians.
  fn cone(self, direction: f32, spread: f32, speed: (f32, f32)) -> Self {
    self
      .with_direction(direction.to_degrees(), spread.to_degrees())
      .with_speed(speed)
  }

  /// Tints from `start` to `end` over a particle's life.
  fn fade(self, start: Color, end: Color) -> Self {
    self.with_color_over_life(Gradient::new([(0.0, start), (1.0, end)]))
  }

  /// Starts at `start` size and ends at `end` size (both as ranges).
  fn sized(self, start: (f32, f32), end: (f32, f32)) -> Self {
    let (mid_start, mid_end) = ((start.0 + start.1) / 2.0, (end.0 + end.1) / 2.0);
    let ratio = if mid_start == 0.0 {
      1.0
    } else {
      mid_end / mid_start
    };
    self
      .with_size(start)
      .with_size_over_life(Curve::new([(0.0, 1.0), (1.0, ratio)]))
  }

  /// One burst of `count` at the start, played once.
  fn one_shot(self, count: impl Into<super::value::ValueRange>) -> Self {
    self
      .with_rate(0.0)
      .with_burst(Burst::at(0.0, count))
      .with_looping(false)
  }

  /// Orange-yellow flames rising upward.
  pub fn fire(image: Handle<Image>) -> Self {
    Self::new(image)
      .with_rate(30.0)
      .cone(UP, 0.5, (30.0, 60.0))
      .with_lifetime((0.5, 1.0))
      .sized((12.0, 20.0), (2.0, 6.0))
      .fade(Color::new(255, 200, 50, 255), Color::new(255, 50, 0, 0))
      .with_drag(0.5)
      .with_effect(SpriteEffect::glow_with_intensity(
        Color::new(255, 150, 50, 255),
        4.0,
        0.8,
      ))
  }

  /// A small flame (torch, candle).
  pub fn flame_small(image: Handle<Image>) -> Self {
    Self::new(image)
      .with_rate(15.0)
      .cone(UP, 0.3, (15.0, 30.0))
      .with_lifetime((0.3, 0.6))
      .sized((6.0, 10.0), (1.0, 3.0))
      .fade(Color::new(255, 220, 100, 255), Color::new(255, 100, 0, 0))
      .with_drag(0.3)
  }

  /// Slow-expanding gray smoke drifting upward.
  pub fn smoke(image: Handle<Image>) -> Self {
    Self::new(image)
      .with_rate(10.0)
      .with_shape(circle(5.0, false))
      .outward()
      .with_lifetime((2.0, 4.0))
      .sized((8.0, 16.0), (30.0, 50.0))
      .fade(Color::new(100, 100, 100, 180), Color::new(50, 50, 50, 0))
      .with_gravity(0.0, -10.0)
      .with_drag(0.8)
      .with_angular_velocity((-0.5f32.to_degrees(), 0.5f32.to_degrees()))
  }

  /// Thick smoke for fires and destruction.
  pub fn smoke_thick(image: Handle<Image>) -> Self {
    Self::new(image)
      .with_rate(20.0)
      .with_shape(circle(10.0, false))
      .outward()
      .with_lifetime((3.0, 5.0))
      .sized((16.0, 24.0), (60.0, 100.0))
      .fade(Color::new(60, 60, 60, 200), Color::new(30, 30, 30, 0))
      .with_gravity(0.0, -15.0)
      .with_drag(0.9)
      .with_angular_velocity((-0.3f32.to_degrees(), 0.3f32.to_degrees()))
  }

  /// A radial burst of glowing particles, played once.
  pub fn explosion(image: Handle<Image>, count: u32) -> Self {
    Self::new(image)
      .one_shot(count as f32)
      .with_shape(circle(1.0, false))
      .outward()
      .with_lifetime((0.3, 0.8))
      .sized((8.0, 16.0), (2.0, 4.0))
      .with_speed((100.0, 300.0))
      .fade(Color::new(255, 200, 100, 255), Color::new(255, 50, 0, 0))
      .with_drag(2.0)
      .with_effect(SpriteEffect::glow_with_intensity(
        Color::new(255, 150, 50, 255),
        3.0,
        0.6,
      ))
  }

  /// A burst of physical debris that falls and bounces.
  pub fn explosion_debris(image: Handle<Image>, count: u32) -> Self {
    Self::new(image)
      .one_shot(count as f32)
      .with_shape(circle(5.0, false))
      .outward()
      .with_lifetime((1.0, 2.0))
      .sized((4.0, 8.0), (4.0, 8.0))
      .with_speed((50.0, 150.0))
      .fade(Color::new(150, 120, 80, 255), Color::new(80, 60, 40, 0))
      .with_gravity(0.0, 200.0)
      .with_drag(0.5)
      .with_rotation((0.0, TAU.to_degrees()))
      .with_angular_velocity((-5.0f32.to_degrees(), 5.0f32.to_degrees()))
      .with_collision(CollisionConfig::bouncy(2.0, 0.6))
  }

  /// Glowing sparkles that wander and fade.
  pub fn magic(image: Handle<Image>) -> Self {
    Self::new(image)
      .with_rate(20.0)
      .with_shape(circle(15.0, false))
      .outward()
      .with_lifetime((1.0, 2.0))
      .sized((4.0, 8.0), (1.0, 2.0))
      .fade(Color::new(150, 100, 255, 255), Color::new(50, 200, 255, 0))
      .with_speed((10.0, 30.0))
      .with_drag(1.0)
      .with_effect(SpriteEffect::glow_with_intensity(
        Color::new(100, 150, 255, 255),
        6.0,
        1.0,
      ))
  }

  /// A healing or buff ring that rises.
  pub fn magic_heal(image: Handle<Image>) -> Self {
    Self::new(image)
      .with_rate(15.0)
      .with_shape(circle(20.0, true))
      .outward()
      .with_lifetime((0.8, 1.5))
      .sized((6.0, 10.0), (2.0, 4.0))
      .fade(Color::new(100, 255, 150, 255), Color::new(200, 255, 200, 0))
      .with_gravity(0.0, -30.0)
      .with_drag(0.5)
      .with_effect(SpriteEffect::glow_with_intensity(
        Color::new(150, 255, 150, 255),
        4.0,
        0.8,
      ))
  }

  /// Fast downward streaks.
  pub fn rain(image: Handle<Image>) -> Self {
    Self::new(image)
      .with_rate(100.0)
      .with_shape(EmitShape::Line { length: 400.0 })
      .with_lifetime((0.5, 1.0))
      .sized((2.0, 3.0), (2.0, 3.0))
      .fade(
        Color::new(180, 200, 255, 200),
        Color::new(180, 200, 255, 100),
      )
      .with_gravity(50.0, 800.0)
  }

  /// Gentle floating descent.
  pub fn snow(image: Handle<Image>) -> Self {
    Self::new(image)
      .with_rate(30.0)
      .with_shape(EmitShape::Line { length: 400.0 })
      .with_lifetime((3.0, 6.0))
      .sized((3.0, 6.0), (3.0, 6.0))
      .fade(Color::WHITE, Color::new(255, 255, 255, 100))
      .with_gravity(0.0, 20.0)
      .with_drag(0.5)
      .with_angular_velocity((-1.0f32.to_degrees(), 1.0f32.to_degrees()))
  }

  /// Bright bouncing sparks, played once.
  pub fn sparks(image: Handle<Image>) -> Self {
    Self::new(image)
      .one_shot(20.0)
      .cone(UP, PI, (50.0, 150.0))
      .with_lifetime((0.5, 1.5))
      .sized((2.0, 4.0), (1.0, 2.0))
      .fade(Color::new(255, 255, 200, 255), Color::new(255, 150, 50, 0))
      .with_gravity(0.0, 300.0)
      .with_drag(0.3)
      .with_collision(CollisionConfig::bouncy(1.0, 0.7))
  }

  /// A puff of dust on impact, played once.
  pub fn dust(image: Handle<Image>) -> Self {
    Self::new(image)
      .one_shot(10.0)
      .cone(UP, PI * 0.8, (20.0, 50.0))
      .with_lifetime((0.3, 0.8))
      .sized((4.0, 8.0), (12.0, 20.0))
      .fade(Color::new(180, 160, 140, 150), Color::new(150, 130, 110, 0))
      .with_drag(3.0)
  }

  /// Tumbling confetti, played once.
  pub fn confetti(image: Handle<Image>) -> Self {
    Self::new(image)
      .one_shot(50.0)
      .cone(UP, PI * 0.6, (100.0, 200.0))
      .with_lifetime((2.0, 4.0))
      .sized((6.0, 10.0), (6.0, 10.0))
      .fade(Color::WHITE, Color::new(255, 255, 255, 100))
      .with_gravity(0.0, 100.0)
      .with_drag(0.5)
      .with_rotation((0.0, TAU.to_degrees()))
      .with_angular_velocity((-8.0f32.to_degrees(), 8.0f32.to_degrees()))
  }

  /// Bubbles that rise.
  pub fn bubbles(image: Handle<Image>) -> Self {
    Self::new(image)
      .with_rate(5.0)
      .with_shape(circle(20.0, false))
      .outward()
      .with_lifetime((2.0, 4.0))
      .sized((4.0, 12.0), (8.0, 16.0))
      .fade(
        Color::new(200, 220, 255, 150),
        Color::new(200, 220, 255, 50),
      )
      .with_gravity(0.0, -30.0)
      .with_drag(0.3)
  }

  /// A trail behind a comet or projectile.
  pub fn comet_trail(image: Handle<Image>) -> Self {
    Self::new(image)
      .with_rate(60.0)
      .with_lifetime((0.2, 0.4))
      .sized((8.0, 12.0), (2.0, 4.0))
      .fade(Color::new(255, 200, 100, 255), Color::new(255, 100, 0, 0))
      .with_speed((0.0, 10.0))
      .with_drag(0.5)
      .with_effect(SpriteEffect::glow_with_intensity(
        Color::new(255, 150, 50, 255),
        4.0,
        0.6,
      ))
  }

  /// Engine exhaust pointing down.
  pub fn exhaust(image: Handle<Image>) -> Self {
    Self::new(image)
      .with_rate(40.0)
      .cone(DOWN, 0.3, (50.0, 100.0))
      .with_lifetime((0.2, 0.5))
      .sized((6.0, 10.0), (12.0, 20.0))
      .fade(Color::new(255, 150, 50, 200), Color::new(100, 100, 100, 0))
      .with_drag(1.0)
  }
}

#[cfg(test)]
mod tests {
  use super::super::collision::CollisionResponse;
  use super::super::effect::ParticleEffect;
  use crate::core::asset::{Assets, Handle};
  use crate::core::graphics::Image;

  fn img() -> Handle<Image> {
    Assets::<Image>::new().add(Image::solid_rect(8, 8, 255, 255, 255, 255))
  }

  #[test]
  fn presets_keep_their_looks() {
    let fire = ParticleEffect::fire(img());
    assert_eq!(fire.rate, 30.0);
    assert!((fire.direction_deg + 90.0).abs() < 1e-3);
    assert!((fire.spread_deg - 28.648).abs() < 1e-2);
    assert_eq!(
      fire.color_over_life.keys().last().map(|k| k.color.a),
      Some(0)
    );

    let boom = ParticleEffect::explosion(img(), 40);
    assert_eq!(boom.bursts.len(), 1);
    assert_eq!(boom.bursts[0].count.min, 40.0);
    assert_eq!(boom.bursts[0].count.max, 40.0);
    assert!(!boom.looping);
    assert_eq!(boom.rate, 0.0);

    let sparks = ParticleEffect::sparks(img());
    let collision = sparks.collision.expect("sparks collide");
    assert_eq!(collision.response, CollisionResponse::bounce(0.7));
  }

  #[test]
  fn every_preset_is_valid() {
    let all = [
      ParticleEffect::fire(img()),
      ParticleEffect::flame_small(img()),
      ParticleEffect::smoke(img()),
      ParticleEffect::smoke_thick(img()),
      ParticleEffect::explosion(img(), 10),
      ParticleEffect::explosion_debris(img(), 10),
      ParticleEffect::magic(img()),
      ParticleEffect::magic_heal(img()),
      ParticleEffect::rain(img()),
      ParticleEffect::snow(img()),
      ParticleEffect::sparks(img()),
      ParticleEffect::dust(img()),
      ParticleEffect::confetti(img()),
      ParticleEffect::bubbles(img()),
      ParticleEffect::comet_trail(img()),
      ParticleEffect::exhaust(img()),
    ];
    for fx in all {
      assert!(fx.problems().is_empty(), "{:?}", fx.problems());
    }
  }
}
