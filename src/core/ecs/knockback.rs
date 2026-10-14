//! Knockback system for directional force effects.
//!
//! The `Knockback` component applies decaying directional force to entities,
//! useful for hit reactions, explosions, and impact feedback.
//!
//! # Overview
//!
//! Knockback uses a strength-based decay system (similar to camera shake):
//! - Strength starts at 1.0 and decays over time
//! - Velocity is calculated as `direction * force * strength²` (squared for punchy feel)
//! - When strength reaches 0, knockback is complete
//!
//! # Example
//!
//! ```ignore
//! use ryo2d::prelude::*;
//!
//! // Apply knockback to an entity
//! Knockback::apply(ctx.world, enemy, Vec2::new(-1.0, 0.0), 400.0);
//!
//! // Or with custom decay rate
//! Knockback::apply_with_decay(ctx.world, enemy, Vec2::new(-1.0, 0.0), 400.0, 2.0);
//!
//! // React when it starts
//! for event in ctx.messages::<KnockbackStarted>() {
//!     // Play hit sound, spawn particles, etc.
//! }
//! ```
//!
//! # Physics Integration
//!
//! What the push does depends on the entity: a character
//! (`CharacterController`) is swept by it, so walls, floors and ceilings
//! stop it and the part running into them is dropped; a dynamic body takes
//! each hit as one change in velocity of its own, and physics slows it from
//! there; a kinematic body without a controller, or a fixed body, ignores
//! it (logged once); an entity without a body moves through its `Transform`.

use super::Vec2;
use super::kernel::{Component, Entity, World};

/// Component for applying decaying directional force to an entity.
///
/// Attach to an entity to push it in a direction. The force decays over time
/// using a strength-squared curve for a punchy impact feel.
///
/// # Example
///
/// ```ignore
/// // Create knockback pushing left at 400 pixels/sec
/// let kb = Knockback::new(Vec2::new(-1.0, 0.0), 400.0);
///
/// // Or from an angle (radians)
/// let kb = Knockback::from_angle(std::f32::consts::PI, 400.0);
///
/// // Customize decay rate
/// let kb = Knockback::new(Vec2::new(-1.0, 0.0), 400.0)
///     .with_decay_rate(8.0);  // Fast decay
/// ```
#[derive(Component, Debug, Clone)]
pub struct Knockback {
  /// Normalized direction X component
  direction_x: f32,
  /// Normalized direction Y component
  direction_y: f32,
  /// Force in pixels per second at full strength
  force: f32,
  /// Current strength (0.0-1.0), decays over time
  strength: f32,
  /// How fast strength decays per second (default: 4.0 = ~0.25s full decay)
  decay_rate: f32,
  /// Whether the knockback is currently active
  pub active: bool,
  /// True for one fixed step when knockback starts
  pub just_started: bool,
  /// True for one fixed step when knockback finishes
  pub just_finished: bool,
  /// Whether to accumulate with existing knockback (blend directions)
  pub accumulate: bool,
  /// The change in velocity not yet given to a dynamic body: each hit's own
  /// `direction * force`, taken by `update_knockback`
  pub(crate) pending: Vec2,
}

impl Default for Knockback {
  fn default() -> Self {
    Self {
      direction_x: 0.0,
      direction_y: 0.0,
      force: 0.0,
      strength: 0.0,
      decay_rate: Self::DEFAULT_DECAY_RATE,
      active: false,
      just_started: false,
      just_finished: false,
      accumulate: true,
      pending: Vec2::ZERO,
    }
  }
}

impl Knockback {
  /// Default decay rate (~0.25s full decay)
  pub const DEFAULT_DECAY_RATE: f32 = 4.0;

  /// Create a new knockback with the given direction and force.
  ///
  /// The direction will be normalized automatically.
  ///
  /// # Arguments
  ///
  /// * `direction` - Direction to push (will be normalized)
  /// * `force` - Force in pixels per second at full strength
  ///
  /// # Example
  ///
  /// ```ignore
  /// // Push left at 400 pixels/sec
  /// let kb = Knockback::new(Vec2::new(-1.0, 0.0), 400.0);
  ///
  /// // Push away from a point
  /// let dir = Vec2::new(target.x - source.x, target.y - source.y);
  /// let kb = Knockback::new(dir, 300.0);
  /// ```
  pub fn new(direction: Vec2, force: f32) -> Self {
    let (dir_x, dir_y) = normalize(direction.x, direction.y);
    Self {
      direction_x: dir_x,
      direction_y: dir_y,
      force,
      strength: 1.0,
      decay_rate: Self::DEFAULT_DECAY_RATE,
      active: true,
      just_started: true,
      just_finished: false,
      accumulate: true,
      pending: Vec2::new(dir_x, dir_y) * force,
    }
  }

  /// Create knockback from an angle (in radians) and force.
  ///
  /// Angle 0 points right (+X), PI/2 points down (+Y).
  ///
  /// # Arguments
  ///
  /// * `angle` - Direction angle in radians
  /// * `force` - Force in pixels per second at full strength
  ///
  /// # Example
  ///
  /// ```ignore
  /// use std::f32::consts::PI;
  ///
  /// // Push right
  /// let kb = Knockback::from_angle(0.0, 400.0);
  ///
  /// // Push up
  /// let kb = Knockback::from_angle(-PI / 2.0, 400.0);
  /// ```
  pub fn from_angle(angle: f32, force: f32) -> Self {
    Self {
      direction_x: angle.cos(),
      direction_y: angle.sin(),
      force,
      strength: 1.0,
      decay_rate: Self::DEFAULT_DECAY_RATE,
      active: true,
      just_started: true,
      just_finished: false,
      accumulate: true,
      pending: Vec2::new(angle.cos(), angle.sin()) * force,
    }
  }

  /// Set a custom decay rate.
  ///
  /// Higher values = faster decay. Default is 4.0 (~0.25s full decay).
  ///
  /// # Examples
  ///
  /// - `2.0` = slow decay (~0.5s)
  /// - `4.0` = normal decay (~0.25s)
  /// - `8.0` = fast decay (~0.125s)
  pub fn with_decay_rate(mut self, rate: f32) -> Self {
    self.decay_rate = rate.max(0.1);
    self
  }

  /// Set whether this knockback should accumulate with existing knockback.
  ///
  /// When true (default), applying knockback to an entity that already has
  /// knockback will blend the directions and add forces.
  ///
  /// When false, new knockback replaces existing knockback entirely.
  pub fn with_accumulate(mut self, accumulate: bool) -> Self {
    self.accumulate = accumulate;
    self
  }

  /// Get the current velocity from this knockback.
  ///
  /// Returns `(velocity_x, velocity_y)` in pixels per second.
  /// Uses strength² for a punchy, impact feel.
  pub fn current_velocity(&self) -> (f32, f32) {
    if !self.active || self.strength <= 0.0 {
      return (0.0, 0.0);
    }

    // Squared intensity for punchy feel (same as camera shake)
    let intensity = self.strength * self.strength;
    let vel_x = self.direction_x * self.force * intensity;
    let vel_y = self.direction_y * self.force * intensity;

    (vel_x, vel_y)
  }

  /// Get the current strength (0.0 to 1.0).
  pub fn strength(&self) -> f32 {
    self.strength
  }

  /// Get the force in pixels per second.
  pub fn force(&self) -> f32 {
    self.force
  }

  /// Get the normalized direction as a Vec2.
  pub fn direction(&self) -> Vec2 {
    Vec2::new(self.direction_x, self.direction_y)
  }

  /// Get the decay rate.
  pub fn decay_rate(&self) -> f32 {
    self.decay_rate
  }

  /// Check if the knockback has completely finished.
  pub fn is_finished(&self) -> bool {
    !self.active || self.strength <= 0.0
  }

  /// Knock `entity` back with the default decay rate, adding a `Knockback`
  /// if it has none. Does nothing if the entity does not exist.
  pub fn apply(world: &mut World, entity: Entity, direction: Vec2, force: f32) {
    Self::apply_with_decay(world, entity, direction, force, Self::DEFAULT_DECAY_RATE);
  }

  /// Knock `entity` back with a custom decay rate (higher recovers faster).
  pub fn apply_with_decay(
    world: &mut World,
    entity: Entity,
    direction: Vec2,
    force: f32,
    decay_rate: f32,
  ) {
    let Ok(mut entity) = world.get_entity_mut(entity) else {
      return;
    };
    if let Some(mut knockback) = entity.get_mut::<Knockback>() {
      knockback.push_with_decay(direction, force, decay_rate);
    } else {
      entity.insert(Knockback::new(direction, force).with_decay_rate(decay_rate));
    }
  }

  /// Push again, optionally blending with an active knockback.
  ///
  /// If `self.accumulate` is true and knockback is active, this blends
  /// the new direction with the existing one and adds forces.
  /// Otherwise, it replaces the existing knockback entirely.
  pub fn push(&mut self, direction: Vec2, force: f32) {
    let (new_dir_x, new_dir_y) = normalize(direction.x, direction.y);
    self.pending += Vec2::new(new_dir_x, new_dir_y) * force;

    if self.accumulate && self.active && self.strength > 0.0 {
      // Blend directions weighted by force
      let existing_weight = self.force * self.strength;
      let new_weight = force;
      let total_weight = existing_weight + new_weight;

      if total_weight > 0.0 {
        let blended_x =
          (self.direction_x * existing_weight + new_dir_x * new_weight) / total_weight;
        let blended_y =
          (self.direction_y * existing_weight + new_dir_y * new_weight) / total_weight;
        let (norm_x, norm_y) = normalize(blended_x, blended_y);
        self.direction_x = norm_x;
        self.direction_y = norm_y;
      }

      // Add forces (capped at reasonable maximum)
      self.force = (self.force + force).min(2000.0);

      // Reset strength to give the new hit impact
      self.strength = 1.0;
    } else {
      // Replace existing knockback
      self.direction_x = new_dir_x;
      self.direction_y = new_dir_y;
      self.force = force;
      self.strength = 1.0;
    }

    self.active = true;
    self.just_started = true;
    self.just_finished = false;
  }

  /// Push with a specific decay rate.
  pub fn push_with_decay(&mut self, direction: Vec2, force: f32, decay_rate: f32) {
    self.push(direction, force);
    self.decay_rate = decay_rate.max(0.1);
  }

  /// Update the knockback state (decay strength over time).
  ///
  /// Called by the engine each fixed step.
  pub fn update(&mut self, dt: f32) {
    self.just_started = false;
    self.just_finished = false;

    if !self.active {
      return;
    }

    // Decay strength
    self.strength = (self.strength - self.decay_rate * dt).max(0.0);

    // Check if finished
    if self.strength <= 0.0 {
      self.active = false;
      self.just_finished = true;
    }
  }

  /// Stop the knockback immediately.
  pub fn stop(&mut self) {
    if self.active {
      self.active = false;
      self.strength = 0.0;
      self.just_finished = true;
    }
  }

  /// Drop the part of the push running into a surface whose normal (out of
  /// the surface) is `normal`; a push straight into it stops.
  pub(crate) fn clip(&mut self, normal: Vec2) {
    if !self.active {
      return;
    }
    let (vx, vy) = self.current_velocity();
    let push = Vec2::new(vx, vy);
    let into = push.dot(normal);
    if into >= 0.0 {
      return;
    }
    let rest = push - normal * into;
    match rest.try_normalize() {
      Some(direction) if rest.length() > 1e-3 => {
        self.force *= rest.length() / push.length();
        self.direction_x = direction.x;
        self.direction_y = direction.y;
      }
      _ => self.stop(),
    }
  }

  /// Restart the knockback with current direction and force.
  pub fn restart(&mut self) {
    self.strength = 1.0;
    self.active = true;
    self.just_started = true;
    self.just_finished = false;
    self.pending += self.direction() * self.force;
  }
}

/// Normalize a 2D vector, returning (0, 0) for zero-length vectors.
fn normalize(x: f32, y: f32) -> (f32, f32) {
  let len_sq = x * x + y * y;
  if len_sq < 1e-10 {
    return (0.0, 0.0);
  }
  let len = len_sq.sqrt();
  (x / len, y / len)
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::f32::consts::PI;

  #[test]
  fn new_creates_active_knockback() {
    let kb = Knockback::new(Vec2::new(1.0, 0.0), 400.0);
    assert!(kb.active);
    assert!(kb.just_started);
    assert!(!kb.just_finished);
    assert_eq!(kb.strength(), 1.0);
    assert_eq!(kb.force(), 400.0);
  }

  #[test]
  fn direction_is_normalized() {
    let kb = Knockback::new(Vec2::new(3.0, 4.0), 100.0);
    let dir = kb.direction();
    let len = (dir.x * dir.x + dir.y * dir.y).sqrt();
    assert!((len - 1.0).abs() < 0.001, "Direction should be normalized");
  }

  #[test]
  fn from_angle_works() {
    let kb = Knockback::from_angle(0.0, 100.0);
    assert!((kb.direction().x - 1.0).abs() < 0.001);
    assert!(kb.direction().y.abs() < 0.001);

    let kb = Knockback::from_angle(PI / 2.0, 100.0);
    assert!(kb.direction().x.abs() < 0.001);
    assert!((kb.direction().y - 1.0).abs() < 0.001);
  }

  #[test]
  fn current_velocity_uses_squared_intensity() {
    let kb = Knockback::new(Vec2::new(1.0, 0.0), 400.0);
    let (vx, vy) = kb.current_velocity();
    // At strength 1.0, intensity = 1.0, so velocity = force
    assert_eq!(vx, 400.0);
    assert_eq!(vy, 0.0);

    let mut kb = Knockback::new(Vec2::new(1.0, 0.0), 400.0);
    kb.strength = 0.5;
    let (vx, _) = kb.current_velocity();
    // At strength 0.5, intensity = 0.25
    assert!((vx - 100.0).abs() < 0.001);
  }

  #[test]
  fn update_decays_strength() {
    let mut kb = Knockback::new(Vec2::new(1.0, 0.0), 400.0).with_decay_rate(4.0);

    kb.update(0.1);
    assert!(!kb.just_started); // Cleared after update
    assert!((kb.strength() - 0.6).abs() < 0.001); // 1.0 - 4.0 * 0.1

    kb.update(0.1);
    assert!((kb.strength() - 0.2).abs() < 0.001);
  }

  #[test]
  fn knockback_finishes_when_strength_zero() {
    let mut kb = Knockback::new(Vec2::new(1.0, 0.0), 400.0).with_decay_rate(4.0);

    // Run until finished
    for _ in 0..10 {
      kb.update(0.1);
    }

    assert!(!kb.active);
    assert!(kb.is_finished());
  }

  #[test]
  fn just_finished_flag_set_correctly() {
    let mut kb = Knockback::new(Vec2::new(1.0, 0.0), 400.0).with_decay_rate(10.0);

    kb.update(0.05);
    assert!(!kb.just_finished);

    // Force strength to near zero
    kb.strength = 0.01;
    kb.update(0.1);
    assert!(kb.just_finished);

    // Next frame, flag should be cleared
    kb.update(0.1);
    assert!(!kb.just_finished);
  }

  #[test]
  fn apply_accumulates_when_enabled() {
    let mut kb = Knockback::new(Vec2::new(1.0, 0.0), 100.0);

    // Apply force in different direction
    kb.push(Vec2::new(0.0, 1.0), 100.0);

    // Direction should be blended (roughly 45 degrees)
    let dir = kb.direction();
    assert!(dir.x > 0.3 && dir.x < 0.9);
    assert!(dir.y > 0.3 && dir.y < 0.9);
  }

  #[test]
  fn apply_replaces_when_accumulate_disabled() {
    let mut kb = Knockback::new(Vec2::new(1.0, 0.0), 100.0).with_accumulate(false);

    kb.push(Vec2::new(0.0, 1.0), 200.0);

    // Direction should be replaced
    let dir = kb.direction();
    assert!(dir.x.abs() < 0.001);
    assert!((dir.y - 1.0).abs() < 0.001);
    assert_eq!(kb.force(), 200.0);
  }

  #[test]
  fn stop_deactivates_knockback() {
    let mut kb = Knockback::new(Vec2::new(1.0, 0.0), 400.0);
    assert!(kb.active);

    kb.stop();

    assert!(!kb.active);
    assert!(kb.just_finished);
    assert_eq!(kb.strength(), 0.0);
  }

  #[test]
  fn restart_reactivates_knockback() {
    let mut kb = Knockback::new(Vec2::new(1.0, 0.0), 400.0);
    kb.stop();

    kb.restart();

    assert!(kb.active);
    assert!(kb.just_started);
    assert_eq!(kb.strength(), 1.0);
  }

  #[test]
  fn zero_direction_handled() {
    let kb = Knockback::new(Vec2::new(0.0, 0.0), 100.0);
    let (vx, vy) = kb.current_velocity();
    // Should return zero velocity, not NaN
    assert!(vx.is_finite());
    assert!(vy.is_finite());
  }

  #[test]
  fn custom_decay_rate() {
    let kb = Knockback::new(Vec2::new(1.0, 0.0), 400.0).with_decay_rate(8.0);
    assert_eq!(kb.decay_rate(), 8.0);
  }

  #[test]
  fn decay_rate_has_minimum() {
    let kb = Knockback::new(Vec2::new(1.0, 0.0), 400.0).with_decay_rate(0.0);
    assert!(kb.decay_rate() >= 0.1);
  }

  #[test]
  fn clipping_drops_the_part_running_into_a_surface() {
    let mut kb = Knockback::new(Vec2::new(1.0, 1.0), 100.0);
    kb.clip(Vec2::NEG_X); // a wall on the right, its normal pointing left
    let (vx, vy) = kb.current_velocity();
    assert!(vx.abs() < 1e-4 && vy > 0.0, "({vx}, {vy})");
    let mut head_on = Knockback::new(Vec2::X, 100.0);
    head_on.clip(Vec2::NEG_X);
    assert!(head_on.is_finished());
  }
}
