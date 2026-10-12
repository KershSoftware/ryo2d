//! The engine's own message types.

use std::path::PathBuf;

use crate::core::ecs::kernel::{Entity, Message};

/// Two colliders started touching (sensor or solid).
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct CollisionStarted {
  pub entity_a: Entity,
  pub entity_b: Entity,
}

impl CollisionStarted {
  pub fn involves(&self, entity: Entity) -> bool {
    self.entity_a == entity || self.entity_b == entity
  }

  /// The other entity in the pair, if `entity` is one of them.
  pub fn other(&self, entity: Entity) -> Option<Entity> {
    if self.entity_a == entity {
      Some(self.entity_b)
    } else if self.entity_b == entity {
      Some(self.entity_a)
    } else {
      None
    }
  }
}

/// Two colliders stopped touching.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct CollisionStopped {
  pub entity_a: Entity,
  pub entity_b: Entity,
}

impl CollisionStopped {
  pub fn involves(&self, entity: Entity) -> bool {
    self.entity_a == entity || self.entity_b == entity
  }

  /// The other entity in the pair, if `entity` is one of them.
  pub fn other(&self, entity: Entity) -> Option<Entity> {
    if self.entity_a == entity {
      Some(self.entity_b)
    } else if self.entity_b == entity {
      Some(self.entity_a)
    } else {
      None
    }
  }
}

/// A sprite, shader or other asset was reloaded from disk (dev builds).
#[derive(Message, Debug, Clone, PartialEq, Eq)]
pub struct AssetReloaded {
  pub path: PathBuf,
}

/// An asset failed to load or reload.
#[derive(Message, Debug, Clone, PartialEq, Eq)]
pub struct AssetLoadFailed {
  pub path: PathBuf,
  pub error: String,
}

/// A `Tween` began playing.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct TweenStarted {
  pub entity: Entity,
}

/// A `Tween` finished (every loop done).
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct TweenCompleted {
  pub entity: Entity,
}

/// A knockback impulse began.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct KnockbackStarted {
  pub entity: Entity,
}

/// A knockback impulse decayed to nothing.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct KnockbackFinished {
  pub entity: Entity,
}

/// A squash or stretch began.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct SquashStretchStarted {
  pub entity: Entity,
}

/// A squash or stretch recovered to the base scale.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct SquashStretchFinished {
  pub entity: Entity,
}
