//! Navigation agent components
//!
//! Provides `NavAgent` and `NavTarget` components for entity pathfinding.

use super::path::{Path, PathSmoothing};
use crate::core::ecs::Entity;

/// State of a navigation agent
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
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

/// Navigation agent component
///
/// Attach to entities that need pathfinding. The navigation system will
/// automatically update position based on the computed path.
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
/// ```
#[derive(Debug, Clone)]
pub struct NavAgent {
    /// Current path being followed
    pub current_path: Option<Path>,
    /// Current navigation state
    pub state: NavAgentState,
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
            state: NavAgentState::Idle,
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

    /// Check if agent has an active path
    pub fn has_path(&self) -> bool {
        self.current_path
            .as_ref()
            .is_some_and(|p| p.is_valid() && !p.is_complete())
    }

    /// Get the current destination (if navigating)
    pub fn destination(&self) -> Option<(f32, f32)> {
        self.current_path
            .as_ref()
            .and_then(|p| p.destination())
            .map(|n| (n.x, n.y))
    }

    /// Get remaining path length
    pub fn remaining_distance(&self) -> f32 {
        self.current_path
            .as_ref()
            .map(|p| p.remaining_length())
            .unwrap_or(0.0)
    }

    /// Cancel current navigation
    pub fn stop(&mut self) {
        self.current_path = None;
        self.state = NavAgentState::Idle;
        self.pathfinding_request = None;
    }

    /// Set a new path to follow
    pub fn set_path(&mut self, path: Path) {
        self.current_path = Some(path);
        self.state = NavAgentState::Moving;
        self.pathfinding_request = None;
    }

    /// Mark navigation as failed
    pub fn fail(&mut self) {
        self.current_path = None;
        self.state = NavAgentState::Failed;
        self.pathfinding_request = None;
    }

    /// Mark navigation as arrived
    pub fn arrive(&mut self) {
        self.state = NavAgentState::Arrived;
        self.pathfinding_request = None;
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
        assert_eq!(agent.state, NavAgentState::Idle);
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
        agent.state = NavAgentState::Moving;

        agent.stop();

        assert_eq!(agent.state, NavAgentState::Idle);
        assert!(agent.current_path.is_none());
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
}
