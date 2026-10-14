//! Gameplay messages.

use crate::core::ecs::{Entity, Message};

/// What a movement preset just did, for sounds, particles and squash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MovementEventKind {
  Jumped,
  AirJumped,
  WallJumped,
  Dashed,
  Landed,
}

/// A movement preset's character jumped, dashed or landed.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct MovementEvent {
  pub entity: Entity,
  pub kind: MovementEventKind,
}

/// A `Pickup` was collected by `by` (a body, or a bare collider).
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct PickedUp {
  pub pickup: Entity,
  pub by: Entity,
}

/// `entity` entered a `KillZone`.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Killed {
  pub entity: Entity,
  pub zone: Entity,
}

/// `entity`'s respawn point moved to a `Checkpoint`.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct CheckpointReached {
  pub checkpoint: Entity,
  pub entity: Entity,
}
