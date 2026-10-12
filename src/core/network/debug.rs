//! Debug assertions for network programming
//!
//! This module provides debug-only assertions that help catch common
//! multiplayer networking bugs during development.
//!
//! All functions in this module are no-ops in release builds.
//!
//! # Available Assertions
//!
//! - [`warn_if_not_dirty`] - Warns if a component was modified but not marked dirty
//! - [`warn_default_input_field`] - Warns if an input field has its default value
//!
//! # Example
//!
//! ```ignore
//! use ryo2d::prelude::*;
//!
//! // After modifying a component, verify it was marked dirty
//! if let Some(mut transform) = world.get_mut::<Transform>(entity) {
//!     transform.x += 10.0;
//! }
//! // This will warn in debug builds if mark_dirty wasn't called
//! warn_if_not_dirty::<Transform>(entity, &replication);
//!
//! // Verify required input fields are set
//! let input = MyInput { facing_right: false, ..Default::default() };
//! warn_default_input_field(&input.facing_right, "facing_right");
//! ```

use crate::core::ecs::Entity;

use super::replication::{Replicated, ReplicationManager};

/// Warn if a component was potentially modified but not marked dirty.
///
/// In debug builds, this checks if the entity exists in the replication
/// manager and warns if the specified component is not in the dirty set.
///
/// This helps catch bugs where you modify a component but forget to call
/// `replication.mark_dirty::<T>(entity)`.
///
/// # Arguments
///
/// * `entity` - The entity that may have been modified
/// * `replication` - The replication manager to check
///
/// # Example
///
/// ```ignore
/// // After modifying a transform
/// if let Some(mut transform) = world.get_mut::<Transform>(entity) {
///     transform.x += 10.0;
/// }
/// replication.mark_dirty::<Transform>(entity);
///
/// // Verify it was marked (useful in complex code paths)
/// warn_if_not_dirty::<Transform>(entity, &replication);
/// ```
#[cfg(debug_assertions)]
pub fn warn_if_not_dirty<T: Replicated + 'static>(
  entity: Entity,
  replication: &ReplicationManager,
) {
  use std::any::type_name;

  let Some(network_id) = replication.get_network_id(entity) else {
    // Entity not networked, nothing to check
    return;
  };

  let component_id = T::component_id();
  let dirty_tracker = replication.dirty_tracker();

  // Check if this component is marked dirty for any client
  let is_dirty = dirty_tracker.is_component_dirty(network_id, component_id);

  if !is_dirty {
    let type_name = type_name::<T>();
    eprintln!(
      "[network debug] WARNING: Component `{}` on entity {:?} (NetworkId {}) \
             may have been modified but is not marked dirty. \
             Did you forget to call replication.mark_dirty::<{}>(entity)?",
      type_name,
      entity,
      network_id,
      type_name.rsplit("::").next().unwrap_or(type_name)
    );
  }
}

/// No-op in release builds.
#[cfg(not(debug_assertions))]
#[inline(always)]
pub fn warn_if_not_dirty<T: Replicated + 'static>(
  _entity: Entity,
  _replication: &ReplicationManager,
) {
}

/// Warn if an input field has its default value.
///
/// In debug builds, this warns when a field that should be explicitly set
/// still has its default value. This helps catch bugs where you forget
/// to populate required fields in input structs.
///
/// # Arguments
///
/// * `value` - Reference to the field value
/// * `field_name` - Name of the field for the warning message
///
/// # Example
///
/// ```ignore
/// let mut input = PlayerInput::default();
/// input.move_x = player.move_x;
/// // Oops, forgot to set facing_right!
///
/// // This will warn in debug builds
/// warn_default_input_field(&input.facing_right, "facing_right");
/// ```
#[cfg(debug_assertions)]
pub fn warn_default_input_field<T: Default + PartialEq>(value: &T, field_name: &str) {
  if *value == T::default() {
    eprintln!(
      "[network debug] WARNING: Input field `{}` has default value. \
             This may indicate a forgotten assignment. \
             Consider using the NetworkInput builder pattern for compile-time safety.",
      field_name
    );
  }
}

/// No-op in release builds.
#[cfg(not(debug_assertions))]
#[inline(always)]
pub fn warn_default_input_field<T: Default + PartialEq>(_value: &T, _field_name: &str) {}

/// Warn if attempting to send an RPC without proper initialization.
///
/// This helps catch cases where the network layer hasn't been properly
/// set up before attempting to send data.
#[cfg(debug_assertions)]
pub fn warn_if_not_connected(is_connected: bool, operation: &str) {
  if !is_connected {
    eprintln!(
      "[network debug] WARNING: Attempted to {} while not connected. \
             The operation will be silently dropped.",
      operation
    );
  }
}

/// No-op in release builds.
#[cfg(not(debug_assertions))]
#[inline(always)]
pub fn warn_if_not_connected(_is_connected: bool, _operation: &str) {}

/// Assert that a network operation is being called from the correct role.
///
/// This helps catch bugs where server-only code runs on a client or vice versa.
#[cfg(debug_assertions)]
pub fn debug_assert_role(expected_server: bool, actual_is_server: bool, operation: &str) {
  if expected_server != actual_is_server {
    let expected = if expected_server { "server" } else { "client" };
    let actual = if actual_is_server { "server" } else { "client" };
    eprintln!(
      "[network debug] WARNING: Operation `{}` expected to run on {} but running on {}",
      operation, expected, actual
    );
  }
}

/// No-op in release builds.
#[cfg(not(debug_assertions))]
#[inline(always)]
pub fn debug_assert_role(_expected_server: bool, _actual_is_server: bool, _operation: &str) {}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ecs::Transform;
  use crate::core::ecs::World;
  use crate::core::network::replication::{NetworkEntityBuilder, ReplicationManager};

  #[test]
  fn warn_if_not_dirty_no_panic_for_non_networked_entity() {
    let replication = ReplicationManager::new_server();
    let mut world = World::new();
    let entity = world.spawn((Transform::from_xy(0.0, 0.0),)).id();

    // Should not panic for non-networked entity
    warn_if_not_dirty::<Transform>(entity, &replication);
  }

  #[test]
  fn warn_if_not_dirty_with_dirty_component() {
    let mut replication = ReplicationManager::new_server();
    replication.register::<Transform>();
    let mut world = World::new();

    let builder = NetworkEntityBuilder::new().server_owned();
    let entity = replication.spawn_networked(&mut world, builder);

    // Add the transform component
    world
      .entity_mut(entity)
      .insert(Transform::from_xy(0.0, 0.0));

    // Mark it dirty
    replication.mark_dirty::<Transform>(entity);

    // Should not warn (component is dirty)
    warn_if_not_dirty::<Transform>(entity, &replication);
  }

  #[test]
  fn warn_default_input_field_with_default() {
    // Should print warning but not panic
    let value: bool = false;
    warn_default_input_field(&value, "test_field");
  }

  #[test]
  fn warn_default_input_field_with_non_default() {
    // Should not print warning
    let value: i32 = 42;
    warn_default_input_field(&value, "test_field");
  }

  #[test]
  fn warn_if_not_connected_when_disconnected() {
    // Should print warning but not panic
    warn_if_not_connected(false, "send message");
  }

  #[test]
  fn warn_if_not_connected_when_connected() {
    // Should not print warning
    warn_if_not_connected(true, "send message");
  }

  #[test]
  fn debug_assert_role_correct() {
    // Should not warn when roles match
    debug_assert_role(true, true, "spawn_entity");
    debug_assert_role(false, false, "process_input");
  }

  #[test]
  fn debug_assert_role_mismatch() {
    // Should warn but not panic when roles mismatch
    debug_assert_role(true, false, "spawn_entity");
  }
}
