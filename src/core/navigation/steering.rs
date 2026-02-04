//! Steering behaviors for agent movement
//!
//! Standalone steering functions that can be combined with pathfinding
//! or used independently for flocking, avoidance, and smooth movement.
//!
//! # Example
//!
//! ```ignore
//! use ryo2d::prelude::*;
//!
//! // Collect nearby agents
//! let neighbors: Vec<(Vec2, f32)> = other_agents
//!     .iter()
//!     .map(|(pos, radius)| (*pos, *radius))
//!     .collect();
//!
//! // Calculate avoidance force
//! let avoid = steering::separation(my_pos, my_radius, &neighbors, 100.0);
//!
//! // Apply to movement
//! let final_velocity = path_direction + avoid;
//! transform.x += final_velocity.x * dt;
//! transform.y += final_velocity.y * dt;
//! ```

use crate::core::ecs::Vec2;

/// Calculate separation steering to avoid nearby agents.
///
/// Returns a force vector pointing away from nearby agents, weighted by
/// how close they are. Agents within overlapping radii produce stronger forces.
///
/// # Arguments
///
/// * `position` - Current position of this agent
/// * `radius` - Collision/avoidance radius of this agent
/// * `neighbors` - List of (position, radius) for nearby agents
/// * `strength` - Force multiplier (higher = stronger avoidance)
///
/// # Returns
///
/// A steering force vector. Will be zero if no neighbors are nearby.
///
/// # Example
///
/// ```ignore
/// let neighbors = vec![
///     (Vec2::new(105.0, 100.0), 8.0),  // Agent to the right
///     (Vec2::new(100.0, 90.0), 8.0),   // Agent below
/// ];
///
/// let avoid = steering::separation(
///     Vec2::new(100.0, 100.0),
///     8.0,
///     &neighbors,
///     50.0,
/// );
/// // avoid will point up-left, away from both neighbors
/// ```
pub fn separation(position: Vec2, radius: f32, neighbors: &[(Vec2, f32)], strength: f32) -> Vec2 {
  let mut force = Vec2::ZERO;

  for &(neighbor_pos, neighbor_radius) in neighbors {
    let diff = position - neighbor_pos;
    let dist = diff.length();

    // Skip if same position (self or exact overlap)
    if dist < 0.001 {
      continue;
    }

    // Combined radii = minimum comfortable distance
    let min_dist = radius + neighbor_radius;

    // Only apply force if within interaction range (2x the combined radii)
    let interaction_range = min_dist * 2.0;
    if dist < interaction_range {
      // Normalize direction
      let dir = diff / dist;

      // Weight by proximity: stronger when closer
      // At dist=0, weight approaches infinity (clamped)
      // At dist=interaction_range, weight=0
      let overlap = interaction_range - dist;
      let weight = (overlap / interaction_range).min(1.0);

      // Quadratic falloff for smoother behavior
      force += dir * weight * weight * strength;
    }
  }

  force
}

/// Calculate seek steering toward a target position.
///
/// Returns a velocity vector pointing toward the target at the specified speed.
/// Does not slow down near the target (use [`arrive`] for that).
///
/// # Arguments
///
/// * `position` - Current position
/// * `target` - Target position to seek
/// * `max_speed` - Maximum movement speed
///
/// # Returns
///
/// A velocity vector toward the target, or zero if already at target.
pub fn seek(position: Vec2, target: Vec2, max_speed: f32) -> Vec2 {
  let desired = target - position;
  let dist = desired.length();

  if dist < 0.001 {
    return Vec2::ZERO;
  }

  (desired / dist) * max_speed
}

/// Calculate flee steering away from a threat position.
///
/// Opposite of [`seek`] - returns a velocity vector pointing away from the threat.
///
/// # Arguments
///
/// * `position` - Current position
/// * `threat` - Position to flee from
/// * `max_speed` - Maximum movement speed
///
/// # Returns
///
/// A velocity vector away from the threat, or zero if at the same position.
pub fn flee(position: Vec2, threat: Vec2, max_speed: f32) -> Vec2 {
  let desired = position - threat;
  let dist = desired.length();

  if dist < 0.001 {
    return Vec2::ZERO;
  }

  (desired / dist) * max_speed
}

/// Calculate arrive steering that slows down near the target.
///
/// Like [`seek`], but gradually reduces speed when within `slow_radius`
/// of the target, coming to a stop at the target position.
///
/// # Arguments
///
/// * `position` - Current position
/// * `target` - Target position to arrive at
/// * `max_speed` - Maximum movement speed
/// * `slow_radius` - Distance at which to start slowing down
///
/// # Returns
///
/// A velocity vector that smoothly approaches zero at the target.
pub fn arrive(position: Vec2, target: Vec2, max_speed: f32, slow_radius: f32) -> Vec2 {
  let desired = target - position;
  let dist = desired.length();

  if dist < 0.001 {
    return Vec2::ZERO;
  }

  let dir = desired / dist;

  // Scale speed based on distance when inside slow radius
  let speed = if dist < slow_radius {
    max_speed * (dist / slow_radius)
  } else {
    max_speed
  };

  dir * speed
}

/// Calculate cohesion steering toward the center of a group.
///
/// Used in flocking behaviors to keep agents together.
///
/// # Arguments
///
/// * `position` - Current position
/// * `neighbors` - Positions of nearby agents (radii ignored)
/// * `max_speed` - Maximum movement speed
///
/// # Returns
///
/// A velocity vector toward the center of mass of neighbors,
/// or zero if no neighbors.
pub fn cohesion(position: Vec2, neighbors: &[(Vec2, f32)], max_speed: f32) -> Vec2 {
  if neighbors.is_empty() {
    return Vec2::ZERO;
  }

  // Calculate center of mass
  let mut center = Vec2::ZERO;
  for &(pos, _) in neighbors {
    center += pos;
  }
  center /= neighbors.len() as f32;

  seek(position, center, max_speed)
}

/// Calculate alignment steering to match velocity with nearby agents.
///
/// Used in flocking behaviors to make agents move in the same direction.
///
/// # Arguments
///
/// * `neighbors` - List of (position, velocity) for nearby agents
/// * `max_speed` - Maximum movement speed (to normalize result)
///
/// # Returns
///
/// A velocity vector representing the average heading of neighbors,
/// or zero if no neighbors.
pub fn alignment(neighbors: &[(Vec2, Vec2)], max_speed: f32) -> Vec2 {
  if neighbors.is_empty() {
    return Vec2::ZERO;
  }

  // Calculate average velocity
  let mut avg_velocity = Vec2::ZERO;
  for &(_, vel) in neighbors {
    avg_velocity += vel;
  }
  avg_velocity /= neighbors.len() as f32;

  // Normalize to max_speed
  let speed = avg_velocity.length();
  if speed < 0.001 {
    return Vec2::ZERO;
  }

  (avg_velocity / speed) * max_speed
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_separation_no_neighbors() {
    let force = separation(Vec2::new(100.0, 100.0), 8.0, &[], 50.0);
    assert_eq!(force, Vec2::ZERO);
  }

  #[test]
  fn test_separation_far_neighbor() {
    // Neighbor far outside interaction range
    let neighbors = vec![(Vec2::new(200.0, 100.0), 8.0)];
    let force = separation(Vec2::new(100.0, 100.0), 8.0, &neighbors, 50.0);
    assert_eq!(force, Vec2::ZERO);
  }

  #[test]
  fn test_separation_close_neighbor() {
    // Neighbor very close - should push away
    let neighbors = vec![(Vec2::new(105.0, 100.0), 8.0)];
    let force = separation(Vec2::new(100.0, 100.0), 8.0, &neighbors, 50.0);

    // Should point left (away from neighbor on right)
    assert!(force.x < 0.0);
    assert!(force.y.abs() < 0.001);
  }

  #[test]
  fn test_seek_basic() {
    let vel = seek(Vec2::new(0.0, 0.0), Vec2::new(100.0, 0.0), 50.0);
    assert!((vel.x - 50.0).abs() < 0.001);
    assert!(vel.y.abs() < 0.001);
  }

  #[test]
  fn test_seek_at_target() {
    let vel = seek(Vec2::new(100.0, 100.0), Vec2::new(100.0, 100.0), 50.0);
    assert_eq!(vel, Vec2::ZERO);
  }

  #[test]
  fn test_flee_basic() {
    let vel = flee(Vec2::new(0.0, 0.0), Vec2::new(100.0, 0.0), 50.0);
    // Should flee left (away from threat on right)
    assert!((vel.x - -50.0).abs() < 0.001);
    assert!(vel.y.abs() < 0.001);
  }

  #[test]
  fn test_arrive_outside_slow_radius() {
    let vel = arrive(Vec2::new(0.0, 0.0), Vec2::new(100.0, 0.0), 50.0, 20.0);
    // Outside slow radius, should be max speed
    assert!((vel.x - 50.0).abs() < 0.001);
  }

  #[test]
  fn test_arrive_inside_slow_radius() {
    let vel = arrive(Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), 50.0, 20.0);
    // 10 units away with slow_radius=20, should be half speed
    assert!((vel.x - 25.0).abs() < 0.001);
  }

  #[test]
  fn test_cohesion_no_neighbors() {
    let vel = cohesion(Vec2::new(0.0, 0.0), &[], 50.0);
    assert_eq!(vel, Vec2::ZERO);
  }

  #[test]
  fn test_cohesion_basic() {
    let neighbors = vec![
      (Vec2::new(100.0, 0.0), 8.0),
      (Vec2::new(100.0, 100.0), 8.0),
    ];
    let vel = cohesion(Vec2::new(0.0, 0.0), &neighbors, 50.0);
    // Center of mass is (100, 50), should seek toward it
    assert!(vel.x > 0.0);
    assert!(vel.y > 0.0);
  }

  #[test]
  fn test_alignment_no_neighbors() {
    let vel = alignment(&[], 50.0);
    assert_eq!(vel, Vec2::ZERO);
  }

  #[test]
  fn test_alignment_basic() {
    let neighbors = vec![
      (Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0)),
      (Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0)),
    ];
    let vel = alignment(&neighbors, 50.0);
    // All moving right, should align right at max_speed
    assert!((vel.x - 50.0).abs() < 0.001);
    assert!(vel.y.abs() < 0.001);
  }
}
