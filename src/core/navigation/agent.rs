//! Navigation agent components
//!
//! Provides `NavAgent` and `NavTarget` components for entity pathfinding.

use super::path::{Path, PathSmoothing};
use crate::core::ecs::Entity;
use crate::core::state_machine::StateMachine;
use std::hash::Hash;

/// State of a navigation agent
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum NavAgentState {
  /// Agent has no target and is stationary
  #[default]
  Idle,
  /// Computing a path to target
  Computing,
  /// Moving along a computed path
  Moving,
  /// Arrived at destination
  Arrived,
  /// Failed to find or follow path
  Failed,
}

impl NavAgentState {
  /// Check if the agent is actively navigating
  pub fn is_active(&self) -> bool {
    matches!(self, NavAgentState::Computing | NavAgentState::Moving)
  }

  /// Check if navigation has ended (success or failure)
  pub fn is_finished(&self) -> bool {
    matches!(self, NavAgentState::Arrived | NavAgentState::Failed)
  }
}

/// Creates a properly configured state machine for navigation agent states.
///
/// Transition graph:
/// ```text
/// ┌───────┐          ┌───────────┐
/// │ Idle  │─────────>│ Computing │
/// └───┬───┘          └─────┬─────┘
///     ^                    │
///     │         ┌─────────>v──────────┐
///     │         │      ┌───────┐      │
///     │         │      │Moving │      │
///     │         │      └───┬───┘      │
///     │         │          │          │
///     │    ┌────┴───┐  ┌───v────┐     │
///     └────│ Failed │  │Arrived │     │
///          └────────┘  └────────┘     │
///                ^                    │
///                └────────────────────┘
/// ```
fn create_nav_state_machine() -> StateMachine<NavAgentState> {
  use NavAgentState::*;

  StateMachine::new(Idle)
    // Normal navigation flow
    .with_transition(Idle, Computing) // Start pathfinding
    .with_transition(Computing, Moving) // Path found
    .with_transition(Computing, Failed) // No path found
    .with_transition(Moving, Arrived) // Reached destination
    .with_transition(Moving, Failed) // Path blocked
    // Reset transitions (can always go back to idle)
    .with_transition(Arrived, Idle) // Ready for new target
    .with_transition(Failed, Idle) // Ready for retry
    // Allow re-computation from these states
    .with_transition(Arrived, Computing) // New target while at destination
    .with_transition(Failed, Computing) // Retry pathfinding
    // Allow immediate pathfinding from idle
    .with_transition(Idle, Moving) // Direct path set (skip computing)
}

/// Navigation agent component
///
/// Attach to entities that need pathfinding. The navigation system will
/// automatically update position based on the computed path.
///
/// # State Machine
///
/// The agent uses an internal state machine with validated transitions:
///
/// - `Idle` → `Computing` → `Moving` → `Arrived`
/// - Any state can transition to `Idle` (via `stop()`)
/// - `Computing` and `Moving` can transition to `Failed`
///
/// Invalid transitions are ignored, ensuring consistent agent behavior.
///
/// # Example
///
/// ```ignore
/// use ryo2d::prelude::*;
///
/// // Spawn an entity with navigation
/// ctx.world.spawn((
///     Transform::new(100.0, 100.0),
///     NavAgent::new(100.0),  // 100 pixels/second
/// ));
///
/// // Set a destination
/// ctx.world.add_component(entity, NavTarget::position(500.0, 400.0));
///
/// // Check agent state
/// if agent.state() == NavAgentState::Arrived {
///     println!("Agent reached destination!");
/// }
/// ```
#[derive(Debug, Clone)]
pub struct NavAgent {
  /// Current path being followed
  pub current_path: Option<Path>,
  /// State machine managing navigation state with validated transitions
  state_machine: StateMachine<NavAgentState>,
  /// Movement speed in pixels per second
  pub speed: f32,
  /// Distance from waypoint to consider "arrived" (default: 4.0)
  pub arrival_threshold: f32,
  /// Whether to avoid other nav agents
  pub avoid_agents: bool,
  /// Agent collision radius for avoidance
  pub radius: f32,
  /// Path smoothing to apply
  pub path_smoothing: PathSmoothing,
  /// Request ID for time-sliced pathfinding
  pub(crate) pathfinding_request: Option<u64>,
}

impl NavAgent {
  /// Create a new navigation agent
  ///
  /// # Arguments
  /// * `speed` - Movement speed in pixels per second
  pub fn new(speed: f32) -> Self {
    NavAgent {
      current_path: None,
      state_machine: create_nav_state_machine(),
      speed: speed.max(0.0),
      arrival_threshold: 4.0,
      avoid_agents: false,
      radius: 8.0,
      path_smoothing: PathSmoothing::None,
      pathfinding_request: None,
    }
  }

  /// Set arrival threshold (distance to consider "at" a waypoint)
  pub fn with_arrival_threshold(mut self, threshold: f32) -> Self {
    self.arrival_threshold = threshold.max(0.1);
    self
  }

  /// Enable agent avoidance with specified radius
  pub fn with_avoidance(mut self, radius: f32) -> Self {
    self.avoid_agents = true;
    self.radius = radius.max(1.0);
    self
  }

  /// Set path smoothing method
  pub fn with_smoothing(mut self, smoothing: PathSmoothing) -> Self {
    self.path_smoothing = smoothing;
    self
  }

  /// Get the current navigation state.
  pub fn state(&self) -> NavAgentState {
    *self.state_machine.current()
  }

  /// Get the previous navigation state, if any transition has occurred.
  pub fn previous_state(&self) -> Option<NavAgentState> {
    self.state_machine.previous().copied()
  }

  /// Check if the agent just entered its current state (true for one frame after transition).
  pub fn just_entered_state(&self) -> bool {
    self.state_machine.just_entered
  }

  /// Check if the agent just exited its previous state (true for one frame after transition).
  pub fn just_exited_state(&self) -> bool {
    self.state_machine.just_exited
  }

  /// Get the time spent in the current state (in seconds).
  pub fn time_in_state(&self) -> f32 {
    self.state_machine.time_in_state()
  }

  /// Check if the agent can transition to a specific state.
  pub fn can_transition_to(&self, state: NavAgentState) -> bool {
    self.state_machine.can_transition_to(&state)
  }

  /// Check if agent has an active path
  pub fn has_path(&self) -> bool {
    self
      .current_path
      .as_ref()
      .is_some_and(|p| p.is_valid() && !p.is_complete())
  }

  /// Get the current destination (if navigating)
  pub fn destination(&self) -> Option<(f32, f32)> {
    self
      .current_path
      .as_ref()
      .and_then(|p| p.destination())
      .map(|n| (n.x, n.y))
  }

  /// Get remaining path length
  pub fn remaining_distance(&self) -> f32 {
    self
      .current_path
      .as_ref()
      .map(|p| p.remaining_length())
      .unwrap_or(0.0)
  }

  /// Cancel current navigation and return to Idle state.
  ///
  /// This always succeeds regardless of current state.
  pub fn stop(&mut self) {
    self.current_path = None;
    // Force transition to Idle (always valid from any state)
    self
      .state_machine
      .force_transition(NavAgentState::Idle)
      .ok();
    self.pathfinding_request = None;
  }

  /// Begin computing a path to a target.
  ///
  /// Returns `true` if the transition was valid, `false` otherwise.
  pub fn start_computing(&mut self) -> bool {
    self
      .state_machine
      .transition_to(NavAgentState::Computing)
      .is_ok()
  }

  /// Set a new path to follow, transitioning to Moving state.
  ///
  /// This will transition from Computing or Idle to Moving.
  pub fn set_path(&mut self, path: Path) {
    self.current_path = Some(path);
    // Try normal transition first, fall back to force if needed
    if self
      .state_machine
      .transition_to(NavAgentState::Moving)
      .is_err()
    {
      self
        .state_machine
        .force_transition(NavAgentState::Moving)
        .ok();
    }
    self.pathfinding_request = None;
  }

  /// Mark navigation as failed.
  ///
  /// Returns `true` if the transition was valid (from Computing or Moving).
  pub fn fail(&mut self) -> bool {
    self.current_path = None;
    self.pathfinding_request = None;
    self
      .state_machine
      .transition_to(NavAgentState::Failed)
      .is_ok()
  }

  /// Mark navigation as arrived at destination.
  ///
  /// Returns `true` if the transition was valid (from Moving).
  pub fn arrive(&mut self) -> bool {
    self.pathfinding_request = None;
    self
      .state_machine
      .transition_to(NavAgentState::Arrived)
      .is_ok()
  }

  /// Update the state machine timing (call once per frame).
  ///
  /// This updates `time_in_state` and clears `just_entered`/`just_exited` flags.
  pub fn update(&mut self, dt: f32) {
    self.state_machine.update(dt);
  }

  /// Get access to the underlying state machine for advanced usage.
  pub fn state_machine(&self) -> &StateMachine<NavAgentState> {
    &self.state_machine
  }
}

impl Default for NavAgent {
  fn default() -> Self {
    Self::new(100.0) // Default 100 px/s
  }
}

/// Navigation target component
///
/// Attach to an entity with `NavAgent` to set its destination.
/// The navigation system will compute a path and start movement.
///
/// # Example
///
/// ```ignore
/// use ryo2d::prelude::*;
///
/// // Move to a fixed position
/// ctx.world.add_component(entity, NavTarget::position(500.0, 400.0));
///
/// // Follow another entity
/// ctx.world.add_component(entity, NavTarget::follow(target_entity));
/// ```
#[derive(Debug, Clone)]
pub struct NavTarget {
  /// Target X coordinate
  pub x: f32,
  /// Target Y coordinate
  pub y: f32,
  /// Whether to continuously update path as target moves
  pub dynamic_target: bool,
  /// Entity to follow (if any)
  pub follow_entity: Option<Entity>,
  /// Re-path if target moves more than this distance
  pub repath_threshold: f32,
  /// Generation of the path request (for deduplication)
  #[allow(dead_code)]
  pub(crate) request_generation: u64,
}

impl NavTarget {
  /// Create a target at a fixed position
  pub fn position(x: f32, y: f32) -> Self {
    NavTarget {
      x,
      y,
      dynamic_target: false,
      follow_entity: None,
      repath_threshold: 32.0,
      request_generation: 0,
    }
  }

  /// Create a target that follows another entity
  pub fn follow(entity: Entity) -> Self {
    NavTarget {
      x: 0.0,
      y: 0.0,
      dynamic_target: true,
      follow_entity: Some(entity),
      repath_threshold: 32.0,
      request_generation: 0,
    }
  }

  /// Set as dynamic target (will re-path periodically)
  pub fn with_dynamic(mut self, dynamic: bool) -> Self {
    self.dynamic_target = dynamic;
    self
  }

  /// Set re-path threshold for dynamic targets
  pub fn with_repath_threshold(mut self, threshold: f32) -> Self {
    self.repath_threshold = threshold.max(1.0);
    self
  }

  /// Update target position (used by navigation system for follow targets)
  pub fn update_position(&mut self, x: f32, y: f32) {
    self.x = x;
    self.y = y;
  }

  /// Check if target position has changed significantly
  pub fn needs_repath(&self, new_x: f32, new_y: f32) -> bool {
    let dx = self.x - new_x;
    let dy = self.y - new_y;
    (dx * dx + dy * dy).sqrt() > self.repath_threshold
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn nav_agent_creation() {
    let agent = NavAgent::new(150.0);
    assert_eq!(agent.speed, 150.0);
    assert_eq!(agent.state(), NavAgentState::Idle);
    assert!(!agent.has_path());
  }

  #[test]
  fn nav_agent_builders() {
    let agent = NavAgent::new(100.0)
      .with_arrival_threshold(8.0)
      .with_avoidance(12.0)
      .with_smoothing(PathSmoothing::LineOfSight);

    assert_eq!(agent.arrival_threshold, 8.0);
    assert!(agent.avoid_agents);
    assert_eq!(agent.radius, 12.0);
    assert_eq!(agent.path_smoothing, PathSmoothing::LineOfSight);
  }

  #[test]
  fn nav_agent_state_helpers() {
    assert!(!NavAgentState::Idle.is_active());
    assert!(NavAgentState::Computing.is_active());
    assert!(NavAgentState::Moving.is_active());
    assert!(!NavAgentState::Arrived.is_active());
    assert!(!NavAgentState::Failed.is_active());

    assert!(!NavAgentState::Idle.is_finished());
    assert!(!NavAgentState::Moving.is_finished());
    assert!(NavAgentState::Arrived.is_finished());
    assert!(NavAgentState::Failed.is_finished());
  }

  #[test]
  fn nav_agent_stop() {
    let mut agent = NavAgent::new(100.0);
    // Use set_path to transition to Moving (creates a minimal path)
    agent.set_path(Path::empty());

    agent.stop();

    assert_eq!(agent.state(), NavAgentState::Idle);
    assert!(agent.current_path.is_none());
  }

  #[test]
  fn nav_agent_state_transitions() {
    let mut agent = NavAgent::new(100.0);

    // Start idle
    assert_eq!(agent.state(), NavAgentState::Idle);

    // Can transition to Computing
    assert!(agent.start_computing());
    assert_eq!(agent.state(), NavAgentState::Computing);

    // Can fail from Computing
    assert!(agent.fail());
    assert_eq!(agent.state(), NavAgentState::Failed);

    // Can stop from Failed (goes to Idle)
    agent.stop();
    assert_eq!(agent.state(), NavAgentState::Idle);

    // Can set path to go directly to Moving
    agent.set_path(Path::empty());
    assert_eq!(agent.state(), NavAgentState::Moving);

    // Can arrive from Moving
    assert!(agent.arrive());
    assert_eq!(agent.state(), NavAgentState::Arrived);
  }

  #[test]
  fn nav_agent_invalid_transitions() {
    let mut agent = NavAgent::new(100.0);

    // Can't arrive from Idle (not a valid transition)
    assert!(!agent.arrive());
    assert_eq!(agent.state(), NavAgentState::Idle);

    // Can't fail from Idle
    assert!(!agent.fail());
    assert_eq!(agent.state(), NavAgentState::Idle);
  }

  #[test]
  fn nav_agent_time_tracking() {
    let mut agent = NavAgent::new(100.0);

    agent.update(0.5);
    assert!((agent.time_in_state() - 0.5).abs() < 0.001);

    agent.start_computing();
    assert_eq!(agent.time_in_state(), 0.0); // Reset on transition
  }

  #[test]
  fn nav_target_position() {
    let target = NavTarget::position(100.0, 200.0);
    assert_eq!(target.x, 100.0);
    assert_eq!(target.y, 200.0);
    assert!(!target.dynamic_target);
    assert!(target.follow_entity.is_none());
  }

  #[test]
  fn nav_target_follow() {
    let mut world = crate::core::ecs::World::new();
    let entity = world.spawn(());

    let target = NavTarget::follow(entity);
    assert!(target.dynamic_target);
    assert_eq!(target.follow_entity, Some(entity));
  }

  #[test]
  fn nav_target_repath() {
    let target = NavTarget::position(100.0, 100.0).with_repath_threshold(10.0);

    // Small movement - no repath needed
    assert!(!target.needs_repath(105.0, 105.0));

    // Large movement - repath needed
    assert!(target.needs_repath(150.0, 150.0));
  }

  #[test]
  fn nav_agent_default() {
    let agent = NavAgent::default();
    assert_eq!(agent.speed, 100.0);
  }

  // =========================================================================
  // Extensive State Machine Tests
  // =========================================================================

  #[test]
  fn nav_agent_full_navigation_cycle() {
    let mut agent = NavAgent::new(100.0);

    // Start idle
    assert_eq!(agent.state(), NavAgentState::Idle);

    // Begin computing path
    assert!(agent.start_computing());
    assert_eq!(agent.state(), NavAgentState::Computing);

    // Path found, start moving
    agent.set_path(Path::empty());
    assert_eq!(agent.state(), NavAgentState::Moving);

    // Arrive at destination
    assert!(agent.arrive());
    assert_eq!(agent.state(), NavAgentState::Arrived);

    // Ready for new target (back to idle)
    agent.stop();
    assert_eq!(agent.state(), NavAgentState::Idle);
  }

  #[test]
  fn nav_agent_computing_to_failed_to_retry() {
    let mut agent = NavAgent::new(100.0);

    // Start computing
    assert!(agent.start_computing());
    assert_eq!(agent.state(), NavAgentState::Computing);

    // No path found
    assert!(agent.fail());
    assert_eq!(agent.state(), NavAgentState::Failed);

    // Retry computing (Failed -> Computing is valid)
    assert!(agent.start_computing());
    assert_eq!(agent.state(), NavAgentState::Computing);

    // This time path found
    agent.set_path(Path::empty());
    assert_eq!(agent.state(), NavAgentState::Moving);
  }

  #[test]
  fn nav_agent_moving_blocked_recovery() {
    let mut agent = NavAgent::new(100.0);

    // Get to moving state
    agent.set_path(Path::empty());
    assert_eq!(agent.state(), NavAgentState::Moving);

    // Path gets blocked
    assert!(agent.fail());
    assert_eq!(agent.state(), NavAgentState::Failed);

    // Recover by going back to idle
    agent.stop();
    assert_eq!(agent.state(), NavAgentState::Idle);

    // Or recover by retrying
    agent.set_path(Path::empty());
    assert!(agent.fail());
    assert!(agent.start_computing());
    assert_eq!(agent.state(), NavAgentState::Computing);
  }

  #[test]
  fn nav_agent_arrived_to_new_target() {
    let mut agent = NavAgent::new(100.0);

    // Complete a navigation
    agent.set_path(Path::empty());
    agent.arrive();
    assert_eq!(agent.state(), NavAgentState::Arrived);

    // Immediately start computing for new target (Arrived -> Computing)
    assert!(agent.start_computing());
    assert_eq!(agent.state(), NavAgentState::Computing);
  }

  #[test]
  fn nav_agent_just_entered_flag() {
    let mut agent = NavAgent::new(100.0);

    // Just created - should be just_entered
    assert!(agent.just_entered_state());

    // After update, flag should be cleared
    agent.update(0.016);
    assert!(!agent.just_entered_state());

    // Transition to Computing
    agent.start_computing();
    assert!(agent.just_entered_state());

    // After update, cleared again
    agent.update(0.016);
    assert!(!agent.just_entered_state());
  }

  #[test]
  fn nav_agent_just_exited_flag() {
    let mut agent = NavAgent::new(100.0);

    // Just created - should not be just_exited
    assert!(!agent.just_exited_state());

    // Transition - should set just_exited
    agent.start_computing();
    assert!(agent.just_exited_state());

    // After update, cleared
    agent.update(0.016);
    assert!(!agent.just_exited_state());
  }

  #[test]
  fn nav_agent_previous_state_tracking() {
    let mut agent = NavAgent::new(100.0);

    // Initially no previous state
    assert!(agent.previous_state().is_none());

    // Transition to Computing
    agent.start_computing();
    assert_eq!(agent.previous_state(), Some(NavAgentState::Idle));

    // Transition to Moving
    agent.set_path(Path::empty());
    assert_eq!(agent.previous_state(), Some(NavAgentState::Computing));

    // Transition to Arrived
    agent.arrive();
    assert_eq!(agent.previous_state(), Some(NavAgentState::Moving));
  }

  #[test]
  fn nav_agent_can_transition_to_from_idle() {
    let agent = NavAgent::new(100.0);

    // From Idle, can go to Computing or Moving
    assert!(agent.can_transition_to(NavAgentState::Computing));
    assert!(agent.can_transition_to(NavAgentState::Moving));

    // Cannot go to Arrived or Failed from Idle
    assert!(!agent.can_transition_to(NavAgentState::Arrived));
    assert!(!agent.can_transition_to(NavAgentState::Failed));

    // Cannot transition to same state
    assert!(!agent.can_transition_to(NavAgentState::Idle));
  }

  #[test]
  fn nav_agent_can_transition_to_from_computing() {
    let mut agent = NavAgent::new(100.0);
    agent.start_computing();

    // From Computing, can go to Moving or Failed
    assert!(agent.can_transition_to(NavAgentState::Moving));
    assert!(agent.can_transition_to(NavAgentState::Failed));

    // Cannot go to Idle or Arrived from Computing
    assert!(!agent.can_transition_to(NavAgentState::Idle));
    assert!(!agent.can_transition_to(NavAgentState::Arrived));
  }

  #[test]
  fn nav_agent_can_transition_to_from_moving() {
    let mut agent = NavAgent::new(100.0);
    agent.set_path(Path::empty());

    // From Moving, can go to Arrived or Failed
    assert!(agent.can_transition_to(NavAgentState::Arrived));
    assert!(agent.can_transition_to(NavAgentState::Failed));

    // Cannot go to Idle or Computing from Moving
    assert!(!agent.can_transition_to(NavAgentState::Idle));
    assert!(!agent.can_transition_to(NavAgentState::Computing));
  }

  #[test]
  fn nav_agent_can_transition_to_from_arrived() {
    let mut agent = NavAgent::new(100.0);
    agent.set_path(Path::empty());
    agent.arrive();

    // From Arrived, can go to Idle or Computing
    assert!(agent.can_transition_to(NavAgentState::Idle));
    assert!(agent.can_transition_to(NavAgentState::Computing));

    // Cannot go to Moving or Failed from Arrived
    assert!(!agent.can_transition_to(NavAgentState::Moving));
    assert!(!agent.can_transition_to(NavAgentState::Failed));
  }

  #[test]
  fn nav_agent_can_transition_to_from_failed() {
    let mut agent = NavAgent::new(100.0);
    agent.start_computing();
    agent.fail();

    // From Failed, can go to Idle or Computing
    assert!(agent.can_transition_to(NavAgentState::Idle));
    assert!(agent.can_transition_to(NavAgentState::Computing));

    // Cannot go to Moving or Arrived from Failed
    assert!(!agent.can_transition_to(NavAgentState::Moving));
    assert!(!agent.can_transition_to(NavAgentState::Arrived));
  }

  #[test]
  fn nav_agent_stop_from_any_state() {
    // Test stop from Idle
    let mut agent = NavAgent::new(100.0);
    agent.stop();
    assert_eq!(agent.state(), NavAgentState::Idle);

    // Test stop from Computing
    let mut agent = NavAgent::new(100.0);
    agent.start_computing();
    agent.stop();
    assert_eq!(agent.state(), NavAgentState::Idle);

    // Test stop from Moving
    let mut agent = NavAgent::new(100.0);
    agent.set_path(Path::empty());
    agent.stop();
    assert_eq!(agent.state(), NavAgentState::Idle);

    // Test stop from Arrived
    let mut agent = NavAgent::new(100.0);
    agent.set_path(Path::empty());
    agent.arrive();
    agent.stop();
    assert_eq!(agent.state(), NavAgentState::Idle);

    // Test stop from Failed
    let mut agent = NavAgent::new(100.0);
    agent.start_computing();
    agent.fail();
    agent.stop();
    assert_eq!(agent.state(), NavAgentState::Idle);
  }

  #[test]
  fn nav_agent_time_accumulates_correctly() {
    let mut agent = NavAgent::new(100.0);

    agent.update(0.1);
    agent.update(0.2);
    agent.update(0.3);

    assert!((agent.time_in_state() - 0.6).abs() < 0.001);
  }

  #[test]
  fn nav_agent_time_resets_on_all_transitions() {
    let mut agent = NavAgent::new(100.0);

    // Accumulate time in Idle
    agent.update(1.0);
    assert!((agent.time_in_state() - 1.0).abs() < 0.001);

    // Transition to Computing - time should reset
    agent.start_computing();
    assert_eq!(agent.time_in_state(), 0.0);

    // Accumulate time in Computing
    agent.update(0.5);
    assert!((agent.time_in_state() - 0.5).abs() < 0.001);

    // Transition to Moving - time should reset
    agent.set_path(Path::empty());
    assert_eq!(agent.time_in_state(), 0.0);

    // Accumulate time in Moving
    agent.update(0.3);
    assert!((agent.time_in_state() - 0.3).abs() < 0.001);

    // Transition to Arrived - time should reset
    agent.arrive();
    assert_eq!(agent.time_in_state(), 0.0);
  }

  #[test]
  fn nav_agent_state_machine_access() {
    let agent = NavAgent::new(100.0);

    // Can access underlying state machine
    let sm = agent.state_machine();
    assert_eq!(sm.current(), &NavAgentState::Idle);
    assert!(sm.just_entered);
  }

  #[test]
  fn nav_agent_set_path_clears_pathfinding_request() {
    let mut agent = NavAgent::new(100.0);
    agent.pathfinding_request = Some(42);

    agent.set_path(Path::empty());

    assert!(agent.pathfinding_request.is_none());
  }

  #[test]
  fn nav_agent_fail_clears_path_and_request() {
    let mut agent = NavAgent::new(100.0);
    agent.set_path(Path::empty());
    agent.pathfinding_request = Some(42);

    agent.fail();

    assert!(agent.current_path.is_none());
    assert!(agent.pathfinding_request.is_none());
  }

  #[test]
  fn nav_agent_stop_clears_everything() {
    let mut agent = NavAgent::new(100.0);
    agent.set_path(Path::empty());
    agent.pathfinding_request = Some(42);

    agent.stop();

    assert!(agent.current_path.is_none());
    assert!(agent.pathfinding_request.is_none());
    assert_eq!(agent.state(), NavAgentState::Idle);
  }

  #[test]
  fn nav_agent_direct_path_from_idle() {
    let mut agent = NavAgent::new(100.0);

    // Can set path directly from Idle (skips Computing)
    agent.set_path(Path::empty());

    assert_eq!(agent.state(), NavAgentState::Moving);
    assert_eq!(agent.previous_state(), Some(NavAgentState::Idle));
  }

  #[test]
  fn nav_agent_flags_persist_until_update() {
    let mut agent = NavAgent::new(100.0);

    // Transition
    agent.start_computing();

    // Flags should be set
    assert!(agent.just_entered_state());
    assert!(agent.just_exited_state());

    // Check multiple times - still set until update
    assert!(agent.just_entered_state());
    assert!(agent.just_exited_state());

    // After update - cleared
    agent.update(0.016);
    assert!(!agent.just_entered_state());
    assert!(!agent.just_exited_state());
  }
}
