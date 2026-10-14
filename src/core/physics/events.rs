//! What the simulation reports, sent after each step: read them with
//! `ctx.messages::<T>()` (`MessageReader<T>` in systems). A collider with no
//! body is its own body. On `CollisionStopped` either entity may already be
//! despawned. The order of `a` and `b` is unspecified.

use crate::core::ecs::{Entity, Message, Vec2};

/// Where two colliders first touched: a world point and the contact normal,
/// pointing from `a` to `b`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Contact {
  pub point: Vec2,
  pub normal: Vec2,
}

/// Two colliders started touching, or overlapping for a trigger.
#[derive(Message, Debug, Clone, Copy, PartialEq)]
pub struct CollisionStarted {
  pub a: Entity,
  pub b: Entity,
  pub body_a: Entity,
  pub body_b: Entity,
  /// Either side is a trigger
  pub trigger: bool,
  /// The first contact; `None` for triggers
  pub contact: Option<Contact>,
}

/// Two colliders stopped touching (or one of them went away).
#[derive(Message, Debug, Clone, Copy, PartialEq)]
pub struct CollisionStopped {
  pub a: Entity,
  pub b: Entity,
  pub body_a: Entity,
  pub body_b: Entity,
  pub trigger: bool,
}

/// A contact pushing harder than a collider's `with_force_events` threshold,
/// sent every step it does: breakables, impact sounds.
#[derive(Message, Debug, Clone, Copy, PartialEq)]
pub struct ContactForce {
  pub a: Entity,
  pub b: Entity,
  pub body_a: Entity,
  pub body_b: Entity,
  /// The total contact force (mass·px/s²)
  pub force: f32,
  /// The direction of the strongest contact force
  pub direction: Vec2,
  /// True on the first step above the threshold
  pub started: bool,
}

/// A joint pulled harder than its `with_break_force` and was removed (its
/// `Joint` component too).
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct JointBroken {
  pub joint: Entity,
  pub a: Entity,
  pub b: Entity,
}

macro_rules! pair_helpers {
  ($($message:ty),*) => {$(
    impl $message {
      /// Whether `entity` is either side's collider or body.
      pub fn involves(&self, entity: Entity) -> bool {
        [self.a, self.b, self.body_a, self.body_b].contains(&entity)
      }

      /// The other side's body, given one side's collider or body.
      pub fn other(&self, entity: Entity) -> Option<Entity> {
        if entity == self.a || entity == self.body_a {
          Some(self.body_b)
        } else if entity == self.b || entity == self.body_b {
          Some(self.body_a)
        } else {
          None
        }
      }

      /// The other side's collider, given one side's collider or body.
      pub fn other_collider(&self, entity: Entity) -> Option<Entity> {
        if entity == self.a || entity == self.body_a {
          Some(self.b)
        } else if entity == self.b || entity == self.body_b {
          Some(self.a)
        } else {
          None
        }
      }
    }
  )*};
}

pair_helpers!(CollisionStarted, CollisionStopped, ContactForce);
