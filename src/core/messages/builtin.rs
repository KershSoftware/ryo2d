//! The engine's own message types.

use std::path::PathBuf;

use crate::core::ecs::kernel::{Entity, Message};

/// A sprite, shader or other asset was reloaded from disk (dev builds).
#[derive(Message, Debug, Clone, PartialEq, Eq)]
pub struct AssetReloaded {
  pub path: PathBuf,
}

/// An asset failed to hot reload (dev builds); the previous version stays in
/// use. Initial and async loads report failure through their own results.
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

/// A tween finished an iteration; for looping tweens this fires each cycle.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct TweenCompleted {
  pub entity: Entity,
  /// The tween is done (`Once`, or the last of `Times(n)`), not just one
  /// iteration of a loop
  pub finished: bool,
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
