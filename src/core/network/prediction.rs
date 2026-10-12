//! Client-side prediction for multiplayer games
//!
//! This module provides components and helpers for implementing client-side
//! prediction, where the client simulates locally and reconciles with server
//! corrections.
//!
//! # Overview
//!
//! Client-side prediction allows responsive gameplay by:
//! 1. Applying input locally (prediction)
//! 2. Receiving authoritative state from server
//! 3. Correcting local state when it differs (reconciliation)
//!
//! # Key Types
//!
//! - [`Predicted`] - Component marking entities for prediction
//! - [`PredictionHelper`] - Helper for handling position updates
//! - [`UpdateResult`] - Outcome of processing a server update
//!
//! # Example
//!
//! ```ignore
//! use ryo2d::prelude::*;
//!
//! // Mark entity as predicted when spawning
//! let entity = world.spawn((
//!     Transform::from_xy(0.0, 0.0),
//!     Predicted::new(client_id),
//! )).id();
//!
//! // Create prediction helper
//! let mut prediction = PredictionHelper::new(Some(local_client_id));
//!
//! // When receiving position update from server
//! match prediction.handle_position_update(&mut world, entity, &update) {
//!     UpdateResult::Applied => { /* Remote player, applied directly */ }
//!     UpdateResult::Spawned => { /* First update, entity visible now */ }
//!     UpdateResult::StoredForReconciliation => { /* Local player, will reconcile */ }
//! }
//!
//! // After respawn, force correction
//! prediction.force_correction(&mut world, entity, &update);
//! ```

use crate::core::ecs::Entity;
use renet::ClientId;
use serde::{Deserialize, Serialize};

use crate::core::ecs::{Component, Transform, Velocity, World};

/// Marker component for client-side predicted entities.
///
/// Attach this to entities that should use client-side prediction.
/// The prediction system uses this to determine:
/// - Whether to apply server updates directly or store for reconciliation
/// - Whether the entity has been spawned (visible) yet
/// - What sequence number was last acknowledged
#[derive(Component, Debug, Clone, Serialize, Deserialize)]
pub struct Predicted {
  /// The client that owns/controls this entity
  pub owner: ClientId,
  /// Whether the entity has received its first server update (is visible)
  spawned: bool,
  /// Last sequence number acknowledged by the server
  last_ack_sequence: u32,
  /// Whether to skip prediction on next update (force correction)
  force_next_correction: bool,
}

impl Predicted {
  /// Create a new Predicted component.
  ///
  /// The entity starts as not spawned, meaning it won't be visible
  /// until the first server update is received.
  pub fn new(owner: ClientId) -> Self {
    Self {
      owner,
      spawned: false,
      last_ack_sequence: 0,
      force_next_correction: false,
    }
  }

  /// Create a Predicted component that is already spawned.
  ///
  /// Use this when creating entities that should be immediately visible
  /// (e.g., the local player on the host).
  pub fn new_spawned(owner: ClientId) -> Self {
    Self {
      owner,
      spawned: true,
      last_ack_sequence: 0,
      force_next_correction: false,
    }
  }

  /// Check if this entity is owned by the given client.
  pub fn is_owned_by(&self, client_id: ClientId) -> bool {
    self.owner == client_id
  }

  /// Check if the entity has been spawned (received first server update).
  pub fn is_spawned(&self) -> bool {
    self.spawned
  }

  /// Mark the entity as spawned.
  pub fn mark_spawned(&mut self) {
    self.spawned = true;
  }

  /// Check if this entity can use prediction.
  ///
  /// Returns true if the entity is spawned and not forcing correction.
  pub fn can_predict(&self) -> bool {
    self.spawned && !self.force_next_correction
  }

  /// Get the last acknowledged sequence number.
  pub fn last_ack_sequence(&self) -> u32 {
    self.last_ack_sequence
  }

  /// Update the last acknowledged sequence number.
  pub fn acknowledge(&mut self, sequence: u32) {
    self.last_ack_sequence = sequence;
  }

  /// Force the next update to be a correction (skip prediction).
  ///
  /// Use this after respawn or other events where the server state
  /// must be applied without client-side modification.
  pub fn force_correction(&mut self) {
    self.force_next_correction = true;
  }

  /// Clear the force correction flag.
  pub fn clear_force_correction(&mut self) {
    self.force_next_correction = false;
  }

  /// Check if next update should force correction.
  pub fn should_force_correction(&self) -> bool {
    self.force_next_correction
  }
}

/// Result of handling a server position update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateResult {
  /// Update was applied directly (remote entity or forced correction).
  Applied,
  /// This was the first update; entity is now spawned/visible.
  Spawned,
  /// Update stored for later reconciliation (local predicted entity).
  StoredForReconciliation,
  /// Entity not found or update skipped.
  Skipped,
}

/// Position update from the server.
///
/// This is a common format for server position updates.
/// Games can customize the fields as needed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerPositionUpdate {
  /// Client ID of the player
  pub client_id: ClientId,
  /// X position
  pub x: f32,
  /// Y position
  pub y: f32,
  /// Input sequence number this state corresponds to
  pub sequence: u32,
  /// Optional velocity for interpolation
  pub velocity_x: Option<f32>,
  pub velocity_y: Option<f32>,
}

impl ServerPositionUpdate {
  /// Create a new position update.
  pub fn new(client_id: ClientId, x: f32, y: f32, sequence: u32) -> Self {
    Self {
      client_id,
      x,
      y,
      sequence,
      velocity_x: None,
      velocity_y: None,
    }
  }

  /// Create a position update with velocity.
  pub fn with_velocity(mut self, vx: f32, vy: f32) -> Self {
    self.velocity_x = Some(vx);
    self.velocity_y = Some(vy);
    self
  }
}

/// Helper for managing client-side prediction.
///
/// This struct provides methods for correctly handling server updates
/// based on whether the entity is local (predicted) or remote.
pub struct PredictionHelper {
  /// The local client's ID (None if server or spectator)
  local_client_id: Option<ClientId>,
  /// Correction threshold - if difference exceeds this, snap instead of interpolate
  pub snap_threshold: f32,
}

impl PredictionHelper {
  /// Create a new prediction helper.
  ///
  /// Pass `None` for `local_client_id` if running as server or spectator.
  pub fn new(local_client_id: Option<ClientId>) -> Self {
    Self {
      local_client_id,
      snap_threshold: 10.0, // Default: snap if >10 units off
    }
  }

  /// Create a prediction helper with a custom snap threshold.
  pub fn with_snap_threshold(local_client_id: Option<ClientId>, threshold: f32) -> Self {
    Self {
      local_client_id,
      snap_threshold: threshold,
    }
  }

  /// Check if an entity is locally controlled.
  pub fn is_local(&self, owner: ClientId) -> bool {
    self.local_client_id == Some(owner)
  }

  /// Handle a server position update correctly.
  ///
  /// This method determines the correct action based on:
  /// - Whether this is a local or remote entity
  /// - Whether the entity has been spawned yet
  /// - Whether a correction is being forced
  ///
  /// # Returns
  ///
  /// - `Applied` - Update was applied directly to the entity
  /// - `Spawned` - First update, entity is now visible
  /// - `StoredForReconciliation` - Local entity, reconcile later
  /// - `Skipped` - Entity not found or update not applicable
  pub fn handle_position_update(
    &self,
    world: &mut World,
    entity: Entity,
    update: &ServerPositionUpdate,
  ) -> UpdateResult {
    // Get the Predicted component
    let Some(mut predicted) = world.get_mut::<Predicted>(entity) else {
      // No Predicted component - just apply update directly
      if let Some(mut transform) = world.get_mut::<Transform>(entity) {
        transform.translation = glam::Vec2::new(update.x, update.y);
      }
      return UpdateResult::Applied;
    };

    let is_local = self.is_local(predicted.owner);
    let was_spawned = predicted.spawned;
    let force_correction = predicted.force_next_correction;

    // First update - mark as spawned
    if !was_spawned {
      predicted.mark_spawned();
      predicted.acknowledge(update.sequence);

      // Apply the update
      if let Some(mut transform) = world.get_mut::<Transform>(entity) {
        transform.translation = glam::Vec2::new(update.x, update.y);
      }
      return UpdateResult::Spawned;
    }

    // Handle forced correction
    if force_correction {
      predicted.clear_force_correction();
      predicted.acknowledge(update.sequence);

      if let Some(mut transform) = world.get_mut::<Transform>(entity) {
        transform.translation = glam::Vec2::new(update.x, update.y);
      }
      return UpdateResult::Applied;
    }

    // Local entity with prediction
    if is_local {
      predicted.acknowledge(update.sequence);
      // Don't apply directly - reconciliation will handle it
      return UpdateResult::StoredForReconciliation;
    }

    // Remote entity - apply directly
    if let Some(mut transform) = world.get_mut::<Transform>(entity) {
      transform.translation = glam::Vec2::new(update.x, update.y);
    }
    UpdateResult::Applied
  }

  /// Force a server correction on an entity.
  ///
  /// Use this after respawn or other events where the server state
  /// must be applied without client-side prediction interference.
  pub fn force_correction(&self, world: &mut World, entity: Entity, update: &ServerPositionUpdate) {
    // Mark for forced correction
    if let Some(mut predicted) = world.get_mut::<Predicted>(entity) {
      predicted.force_correction();
    }

    // Apply the update
    self.handle_position_update(world, entity, update);
  }

  /// Apply a position update directly, bypassing prediction logic.
  ///
  /// Use this for spawning new entities or when you need direct control.
  pub fn apply_direct(&self, world: &mut World, entity: Entity, update: &ServerPositionUpdate) {
    if let Some(mut transform) = world.get_mut::<Transform>(entity) {
      transform.translation = glam::Vec2::new(update.x, update.y);
    }

    if let Some(mut predicted) = world.get_mut::<Predicted>(entity) {
      predicted.acknowledge(update.sequence);
      if !predicted.spawned {
        predicted.mark_spawned();
      }
    }
  }

  /// Check if reconciliation is needed for an entity.
  ///
  /// Returns true if the entity is local and has unacknowledged inputs.
  pub fn needs_reconciliation(&self, world: &World, entity: Entity, current_sequence: u32) -> bool {
    let Some(predicted) = world.get::<Predicted>(entity) else {
      return false;
    };

    self.is_local(predicted.owner) && current_sequence > predicted.last_ack_sequence
  }

  /// Get the local client ID.
  pub fn local_client_id(&self) -> Option<ClientId> {
    self.local_client_id
  }

  /// Set the local client ID.
  pub fn set_local_client_id(&mut self, client_id: Option<ClientId>) {
    self.local_client_id = client_id;
  }

  /// Dead-reckon all remote predicted entities by extrapolating their
  /// position using their current [`Velocity`].
  ///
  /// Call this each frame on the client to smooth remote player movement
  /// between server position updates. When the next update arrives,
  /// [`handle_position_update`](Self::handle_position_update) snaps to the
  /// authoritative position, correcting any accumulated drift.
  ///
  /// Only affects entities that have [`Predicted`], [`Transform`], and
  /// [`Velocity`] components and are **not** owned by the local client.
  ///
  /// Game-specific forces (gravity, friction, etc.) should be applied to
  /// remote entity velocities *before* calling this method.
  ///
  /// # Example
  ///
  /// ```ignore
  /// // Apply game-specific gravity to airborne remote entities first
  /// for &entity in &remote_entities {
  ///     if is_airborne(entity) {
  ///         vel.dy += GRAVITY * dt;
  ///     }
  /// }
  /// // Then let the engine extrapolate positions
  /// prediction.dead_reckon_remotes(&mut world, dt);
  /// ```
  pub fn dead_reckon_remotes(&self, world: &mut World, dt: f32) {
    let Some(local_id) = self.local_client_id else {
      return;
    };

    let mut remotes = world.query::<(&Predicted, &Velocity, &mut Transform)>();
    for (predicted, velocity, mut transform) in remotes.iter_mut(world) {
      if predicted.owner != local_id && predicted.spawned {
        transform.translation += velocity.as_vec2() * dt;
      }
    }
  }
}

impl Default for PredictionHelper {
  fn default() -> Self {
    Self::new(None)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn create_test_world_with_entity(owner: ClientId) -> (World, Entity) {
    let mut world = World::new();
    let entity = world
      .spawn((Transform::from_xy(0.0, 0.0), Predicted::new(owner)))
      .id();
    (world, entity)
  }

  #[test]
  fn predicted_new() {
    let pred = Predicted::new(1);
    assert_eq!(pred.owner, 1);
    assert!(!pred.is_spawned());
    assert_eq!(pred.last_ack_sequence(), 0);
    assert!(!pred.can_predict()); // Not spawned yet
  }

  #[test]
  fn predicted_new_spawned() {
    let pred = Predicted::new_spawned(1);
    assert!(pred.is_spawned());
    assert!(pred.can_predict());
  }

  #[test]
  fn predicted_force_correction() {
    let mut pred = Predicted::new_spawned(1);
    assert!(pred.can_predict());

    pred.force_correction();
    assert!(!pred.can_predict());
    assert!(pred.should_force_correction());

    pred.clear_force_correction();
    assert!(pred.can_predict());
  }

  #[test]
  fn first_update_spawns() {
    let (mut world, entity) = create_test_world_with_entity(1);
    let helper = PredictionHelper::new(Some(1));

    let update = ServerPositionUpdate::new(1, 100.0, 200.0, 1);
    let result = helper.handle_position_update(&mut world, entity, &update);

    assert_eq!(result, UpdateResult::Spawned);

    let transform = world.get::<Transform>(entity).unwrap();
    assert_eq!(transform.translation.x, 100.0);
    assert_eq!(transform.translation.y, 200.0);

    let predicted = world.get::<Predicted>(entity).unwrap();
    assert!(predicted.is_spawned());
  }

  #[test]
  fn local_entity_stores_for_reconciliation() {
    let (mut world, entity) = create_test_world_with_entity(1);
    let helper = PredictionHelper::new(Some(1));

    // First update to spawn
    let update1 = ServerPositionUpdate::new(1, 100.0, 200.0, 1);
    helper.handle_position_update(&mut world, entity, &update1);

    // Set local position (prediction)
    if let Some(mut t) = world.get_mut::<Transform>(entity) {
      t.translation.x = 150.0;
    }

    // Second update - should store for reconciliation
    let update2 = ServerPositionUpdate::new(1, 110.0, 200.0, 2);
    let result = helper.handle_position_update(&mut world, entity, &update2);

    assert_eq!(result, UpdateResult::StoredForReconciliation);

    // Position should NOT have changed (reconciliation handles it)
    let transform = world.get::<Transform>(entity).unwrap();
    assert_eq!(transform.translation.x, 150.0); // Still the predicted value
  }

  #[test]
  fn remote_entity_applies_directly() {
    let (mut world, entity) = create_test_world_with_entity(2); // Owner is 2
    let helper = PredictionHelper::new(Some(1)); // Local client is 1

    // First update to spawn
    let update1 = ServerPositionUpdate::new(2, 100.0, 200.0, 1);
    helper.handle_position_update(&mut world, entity, &update1);

    // Second update - should apply directly (remote entity)
    let update2 = ServerPositionUpdate::new(2, 150.0, 250.0, 2);
    let result = helper.handle_position_update(&mut world, entity, &update2);

    assert_eq!(result, UpdateResult::Applied);

    let transform = world.get::<Transform>(entity).unwrap();
    assert_eq!(transform.translation.x, 150.0);
    assert_eq!(transform.translation.y, 250.0);
  }

  #[test]
  fn force_correction_applies() {
    let (mut world, entity) = create_test_world_with_entity(1);
    let helper = PredictionHelper::new(Some(1));

    // Spawn
    let update1 = ServerPositionUpdate::new(1, 100.0, 200.0, 1);
    helper.handle_position_update(&mut world, entity, &update1);

    // Set local position
    if let Some(mut t) = world.get_mut::<Transform>(entity) {
      t.translation.x = 999.0;
    }

    // Force correction
    let update2 = ServerPositionUpdate::new(1, 50.0, 50.0, 5);
    helper.force_correction(&mut world, entity, &update2);

    // Position should be corrected
    let transform = world.get::<Transform>(entity).unwrap();
    assert_eq!(transform.translation.x, 50.0);
    assert_eq!(transform.translation.y, 50.0);
  }

  #[test]
  fn needs_reconciliation() {
    let (mut world, entity) = create_test_world_with_entity(1);
    let helper = PredictionHelper::new(Some(1));

    // Spawn with sequence 1
    let update = ServerPositionUpdate::new(1, 0.0, 0.0, 1);
    helper.handle_position_update(&mut world, entity, &update);

    // Should need reconciliation if current sequence > acked
    assert!(helper.needs_reconciliation(&world, entity, 5));
    assert!(!helper.needs_reconciliation(&world, entity, 1));
  }

  #[test]
  fn entity_without_predicted_applies_directly() {
    let mut world = World::new();
    let entity = world.spawn((Transform::from_xy(0.0, 0.0),)).id(); // No Predicted

    let helper = PredictionHelper::new(Some(1));
    let update = ServerPositionUpdate::new(1, 100.0, 200.0, 1);
    let result = helper.handle_position_update(&mut world, entity, &update);

    assert_eq!(result, UpdateResult::Applied);

    let transform = world.get::<Transform>(entity).unwrap();
    assert_eq!(transform.translation.x, 100.0);
  }

  #[test]
  fn dead_reckon_moves_remote_entity() {
    let mut world = World::new();
    // Remote entity (owner 2) with velocity
    let remote = world
      .spawn((
        Transform::from_xy(100.0, 200.0),
        Predicted::new_spawned(ClientId::from(2u64)),
        Velocity {
          dx: 60.0,
          dy: -120.0,
        },
      ))
      .id();
    // Local entity (owner 1) with velocity — should NOT move
    let local = world
      .spawn((
        Transform::from_xy(0.0, 0.0),
        Predicted::new_spawned(ClientId::from(1u64)),
        Velocity {
          dx: 999.0,
          dy: 999.0,
        },
      ))
      .id();

    let helper = PredictionHelper::new(Some(ClientId::from(1u64)));
    helper.dead_reckon_remotes(&mut world, 0.1); // 100ms step

    // Remote entity should have moved
    let rt = world.get::<Transform>(remote).unwrap();
    assert!((rt.translation.x - 106.0).abs() < 0.01);
    assert!((rt.translation.y - 188.0).abs() < 0.01);

    // Local entity should be untouched
    let lt = world.get::<Transform>(local).unwrap();
    assert_eq!(lt.translation.x, 0.0);
    assert_eq!(lt.translation.y, 0.0);
  }

  #[test]
  fn dead_reckon_skips_unspawned() {
    let mut world = World::new();
    // Remote entity that hasn't received first update (not spawned)
    let entity = world
      .spawn((
        Transform::from_xy(0.0, 0.0),
        Predicted::new(ClientId::from(2u64)), // new() = not spawned
        Velocity { dx: 100.0, dy: 0.0 },
      ))
      .id();

    let helper = PredictionHelper::new(Some(ClientId::from(1u64)));
    helper.dead_reckon_remotes(&mut world, 0.1);

    // Should not have moved — entity isn't spawned yet
    let t = world.get::<Transform>(entity).unwrap();
    assert_eq!(t.translation.x, 0.0);
  }
}
