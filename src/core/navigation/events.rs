//! Navigation events
//!
//! Events emitted by the navigation system for game logic to react to.

use crate::core::ecs::Entity;

/// Emitted when an agent successfully reaches its destination
///
/// # Example
///
/// ```ignore
/// use ryo2d::prelude::*;
///
/// fn update(&mut self, ctx: &mut GameContext) {
///     for event in ctx.events.read::<PathCompleted>() {
///         if event.entity == self.enemy {
///             // Enemy reached player, deal damage
///             self.player_health -= 10;
///         }
///     }
/// }
/// ```
#[derive(Debug, Clone, Copy)]
pub struct PathCompleted {
  /// The entity that completed its path
  pub entity: Entity,
  /// Final X position
  pub x: f32,
  /// Final Y position
  pub y: f32,
}

impl PathCompleted {
  pub fn new(entity: Entity, x: f32, y: f32) -> Self {
    PathCompleted { entity, x, y }
  }
}

/// Reason why pathfinding failed
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathFailureReason {
  /// No valid path exists to the target
  NoPath,
  /// Start or goal position is blocked
  InvalidEndpoints,
  /// Path computation exceeded limits
  TooExpensive,
  /// Target entity no longer exists
  TargetLost,
  /// Grid changed during computation
  GridChanged,
}

/// Emitted when an agent fails to find or follow a path
///
/// # Example
///
/// ```ignore
/// use ryo2d::prelude::*;
///
/// fn update(&mut self, ctx: &mut GameContext) {
///     for event in ctx.events.read::<PathFailed>() {
///         match event.reason {
///             PathFailureReason::NoPath => {
///                 println!("Entity {:?} is stuck!", event.entity);
///             }
///             PathFailureReason::TargetLost => {
///                 // Re-acquire target
///             }
///             _ => {}
///         }
///     }
/// }
/// ```
#[derive(Debug, Clone, Copy)]
pub struct PathFailed {
  /// The entity that failed to path
  pub entity: Entity,
  /// Reason for failure
  pub reason: PathFailureReason,
  /// Attempted target X (if known)
  pub target_x: Option<f32>,
  /// Attempted target Y (if known)
  pub target_y: Option<f32>,
}

impl PathFailed {
  pub fn new(entity: Entity, reason: PathFailureReason) -> Self {
    PathFailed {
      entity,
      reason,
      target_x: None,
      target_y: None,
    }
  }

  pub fn with_target(mut self, x: f32, y: f32) -> Self {
    self.target_x = Some(x);
    self.target_y = Some(y);
    self
  }
}

/// Emitted when an agent's path becomes invalid due to grid changes
///
/// The navigation system will automatically attempt to recompute the path,
/// but game code can listen for this to implement custom behavior.
///
/// # Example
///
/// ```ignore
/// use ryo2d::prelude::*;
///
/// fn update(&mut self, ctx: &mut GameContext) {
///     for event in ctx.events.read::<PathInvalidated>() {
///         // Maybe show a "recalculating" indicator
///         self.show_path_indicator(event.entity, PathIndicator::Recalculating);
///     }
/// }
/// ```
#[derive(Debug, Clone, Copy)]
pub struct PathInvalidated {
  /// The entity whose path was invalidated
  pub entity: Entity,
  /// Whether the system will attempt to recompute
  pub will_recompute: bool,
}

impl PathInvalidated {
  pub fn new(entity: Entity, will_recompute: bool) -> Self {
    PathInvalidated {
      entity,
      will_recompute,
    }
  }
}

/// Emitted when an agent starts computing a new path
///
/// Useful for showing loading indicators or disabling certain actions
/// while pathfinding is in progress.
#[derive(Debug, Clone, Copy)]
pub struct PathStarted {
  /// The entity that started pathfinding
  pub entity: Entity,
  /// Target X position
  pub target_x: f32,
  /// Target Y position
  pub target_y: f32,
}

impl PathStarted {
  pub fn new(entity: Entity, target_x: f32, target_y: f32) -> Self {
    PathStarted {
      entity,
      target_x,
      target_y,
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn path_completed_creation() {
    let mut world = crate::core::ecs::World::new();
    let entity = world.spawn(());

    let event = PathCompleted::new(entity, 100.0, 200.0);
    assert_eq!(event.entity, entity);
    assert_eq!(event.x, 100.0);
    assert_eq!(event.y, 200.0);
  }

  #[test]
  fn path_failed_with_target() {
    let mut world = crate::core::ecs::World::new();
    let entity = world.spawn(());

    let event = PathFailed::new(entity, PathFailureReason::NoPath).with_target(50.0, 75.0);

    assert_eq!(event.entity, entity);
    assert_eq!(event.reason, PathFailureReason::NoPath);
    assert_eq!(event.target_x, Some(50.0));
    assert_eq!(event.target_y, Some(75.0));
  }

  #[test]
  fn path_invalidated_creation() {
    let mut world = crate::core::ecs::World::new();
    let entity = world.spawn(());

    let event = PathInvalidated::new(entity, true);
    assert_eq!(event.entity, entity);
    assert!(event.will_recompute);
  }
}
