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

/// Two colliders started touching. Trigger overlaps are `TriggerEntered`.
#[derive(Message, Debug, Clone, Copy, PartialEq)]
pub struct CollisionStarted {
  pub a: Entity,
  pub b: Entity,
  pub body_a: Entity,
  pub body_b: Entity,
  /// The first contact
  pub contact: Option<Contact>,
}

/// Two colliders stopped touching (or one of them went away).
#[derive(Message, Debug, Clone, Copy, PartialEq)]
pub struct CollisionStopped {
  pub a: Entity,
  pub b: Entity,
  pub body_a: Entity,
  pub body_b: Entity,
}

/// A collider started overlapping a trigger (`Collider::as_trigger`). Sent
/// once per body: a body with several colliders enters on its first and
/// exits when its last one leaves. A body's trigger colliders (an aggro
/// radius) count apart from its solid ones, so a body with both enters once
/// with each kind (`other` says which). What can set a trigger off is the
/// usual layer and mask test. Two overlapping triggers each get a message.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct TriggerEntered {
  /// The trigger collider's entity
  pub trigger: Entity,
  /// The collider that entered
  pub other: Entity,
  /// Its body (the collider itself when it has none)
  pub other_body: Entity,
}

/// A body (or bare collider) left a trigger, or one of them went away:
/// either entity may already be despawned.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct TriggerExited {
  pub trigger: Entity,
  pub other: Entity,
  pub other_body: Entity,
}

macro_rules! trigger_helpers {
  ($($message:ty),*) => {$(
    impl $message {
      /// What entered or left: the other side's body, or its collider when it has none.
      pub fn actor(&self) -> Entity {
        self.other_body
      }
    }
  )*};
}

trigger_helpers!(TriggerEntered, TriggerExited);

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
