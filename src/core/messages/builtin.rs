//! The engine's own message types.

use crate::core::ecs::kernel::{Entity, Message};

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
