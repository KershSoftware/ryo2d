//! Rigid bodies: how physics moves an entity.

use crate::core::ecs::{Component, Transform, Velocity};

/// How a body moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BodyKind {
  /// Simulated: gravity, contacts, forces and joints move it.
  #[default]
  Dynamic,
  /// Static: never moves by itself. Writing its `Transform` teleports it,
  /// but it carries nothing. A `Collider` with no body is fixed too.
  Fixed,
  /// Moved only by the game and never pushed. Write its `Transform` and it
  /// travels there over the step (carrying what rides it), or set its
  /// `Velocity`/`AngularVelocity` and the engine moves it; when both are
  /// written in one step the `Transform` wins. Moving its parent counts as
  /// a `Transform` write: under a parent the game moves, its velocity is not
  /// applied that step.
  Kinematic,
}

/// Makes an entity a physics body, simulated with the `Collider` on the same
/// entity; its `Transform` is written back from the simulation.
///
/// Settings that do not apply to a kind are ignored: gravity, damping, CCD
/// and dominance only matter to dynamic bodies. Mass comes from the colliders.
/// Edits apply in place on the next step without resetting contacts or sleep.
#[derive(Component, Debug, Clone, PartialEq)]
#[non_exhaustive]
#[require(Transform, Velocity, AngularVelocity)]
pub struct RigidBody {
  pub kind: BodyKind,
  /// Multiplier on `PhysicsConfig::gravity` (0 floats, 2 falls twice as fast).
  pub gravity_scale: f32,
  /// How fast linear velocity decays, per second.
  pub linear_damping: f32,
  /// How fast spin decays, per second.
  pub angular_damping: f32,
  /// Never rotate: contacts and torques leave `Transform.rotation` alone.
  pub lock_rotation: bool,
  /// Continuous collision detection: fast bodies do not tunnel through thin ones.
  pub ccd: bool,
  /// Whether the body may fall asleep at rest (asleep it costs nothing).
  pub can_sleep: bool,
  /// A higher dominance pushes lower ones and is never pushed back by them.
  pub dominance: i8,
}

impl RigidBody {
  fn of_kind(kind: BodyKind) -> Self {
    Self {
      kind,
      gravity_scale: 1.0,
      linear_damping: 0.0,
      angular_damping: 0.0,
      lock_rotation: false,
      ccd: false,
      can_sleep: true,
      dominance: 0,
    }
  }

  /// A simulated body. It rotates unless `lock_rotation()` is called.
  pub fn dynamic() -> Self {
    Self::of_kind(BodyKind::Dynamic)
  }

  /// A static body: walls, floors, structures grouping several colliders.
  pub fn fixed() -> Self {
    Self::of_kind(BodyKind::Fixed)
  }

  /// A body only the game moves: platforms, doors, characters with their own movement.
  pub fn kinematic() -> Self {
    Self::of_kind(BodyKind::Kinematic)
  }

  /// Multiply gravity by `scale` (0 floats).
  pub fn with_gravity_scale(mut self, scale: f32) -> Self {
    self.gravity_scale = scale;
    self
  }

  /// Slow its motion by `damping` per second.
  pub fn with_linear_damping(mut self, damping: f32) -> Self {
    self.linear_damping = damping;
    self
  }

  /// Slow its spin by `damping` per second.
  pub fn with_angular_damping(mut self, damping: f32) -> Self {
    self.angular_damping = damping;
    self
  }

  /// Never rotate (characters, upright crates).
  pub fn lock_rotation(mut self) -> Self {
    self.lock_rotation = true;
    self
  }

  /// Continuous collision detection, for fast bodies that must not tunnel.
  pub fn with_ccd(mut self) -> Self {
    self.ccd = true;
    self
  }

  /// Stay awake at rest (a sleeping body costs nothing; an awake one does).
  pub fn never_sleep(mut self) -> Self {
    self.can_sleep = false;
    self
  }

  /// Push bodies of lower dominance without being pushed back.
  pub fn with_dominance(mut self, dominance: i8) -> Self {
    self.dominance = dominance;
    self
  }
}

impl Default for RigidBody {
  fn default() -> Self {
    Self::dynamic()
  }
}

/// Spin in radians per second, with the same sign as `Transform.rotation`.
/// The engine writes it back from awake dynamic bodies; set it to spin one.
#[derive(Component, Debug, Clone, Copy, PartialEq, Default)]
pub struct AngularVelocity(pub f32);

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn kinds_and_defaults() {
    let body = RigidBody::dynamic();
    assert_eq!(body.kind, BodyKind::Dynamic);
    assert_eq!(body.gravity_scale, 1.0);
    assert!(body.can_sleep && !body.lock_rotation && !body.ccd);
    assert_eq!(RigidBody::fixed().kind, BodyKind::Fixed);
    assert_eq!(RigidBody::kinematic().kind, BodyKind::Kinematic);
    assert_eq!(RigidBody::default(), RigidBody::dynamic());
  }

  #[test]
  fn builders() {
    let body = RigidBody::dynamic()
      .with_gravity_scale(0.5)
      .with_linear_damping(0.1)
      .with_angular_damping(0.2)
      .lock_rotation()
      .with_ccd()
      .never_sleep()
      .with_dominance(3);
    assert_eq!(body.gravity_scale, 0.5);
    assert_eq!(body.linear_damping, 0.1);
    assert_eq!(body.angular_damping, 0.2);
    assert!(body.lock_rotation && body.ccd && !body.can_sleep);
    assert_eq!(body.dominance, 3);
  }

  #[test]
  fn a_body_brings_its_motion_components() {
    let mut world = crate::core::ecs::World::new();
    let e = world.spawn(RigidBody::dynamic()).id();
    assert!(world.get::<Transform>(e).is_some());
    assert_eq!(world.get::<Velocity>(e), Some(&Velocity::zero()));
    assert_eq!(world.get::<AngularVelocity>(e), Some(&AngularVelocity(0.0)));
  }
}
