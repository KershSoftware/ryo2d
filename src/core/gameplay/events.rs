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
