//! Rapier2D physics integration
//!
//! This module wraps Rapier2D to provide physics simulation with collision response.
//! It maintains a mapping between ECS entities and Rapier physics bodies, handling
//! automatic synchronization of positions and velocities.
//!
//! ## Collision Events
//!
//! The physics world collects collision events during each `step()` call. Query them
//! with `get_collision_events()` or consume them with `drain_collision_events()`.
//!
//! ```ignore
//! for event in physics.drain_collision_events() {
//!     match event.event_type {
//!         CollisionEventType::Started => println!("Collision started!"),
//!         CollisionEventType::Stopped => println!("Collision ended!"),
//!     }
//! }
//! ```

use std::collections::HashMap;

use crossbeam_channel as channel;
use rapier2d::prelude::*;

use super::collision::{Collider, ColliderShape};
use super::components::RigidBody;
use crate::core::ecs::{Entity, Transform, Velocity, World};
use crate::core::graphics::GroupedCollider;

/// Pixels per meter conversion factor
/// Rapier works in meters, games typically work in pixels.
/// 100 pixels = 1 meter is a common convention.
const PIXELS_PER_METER: f32 = 100.0;

/// Type of collision event
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollisionEventType {
  /// Two colliders started touching
  Started,
  /// Two colliders stopped touching
  Stopped,
}

/// A collision event between two entities
///
/// Generated when colliders start or stop intersecting. Use `is_sensor_event()`
/// to check if this was a sensor/trigger collision (no physical response).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhysicsCollisionEvent {
  /// First entity in the collision pair
  pub entity_a: Entity,
  /// Second entity in the collision pair
  pub entity_b: Entity,
  /// Whether the collision started or stopped
  pub event_type: CollisionEventType,
  /// True if this collision involves a sensor/trigger collider
  pub is_sensor: bool,
}

impl PhysicsCollisionEvent {
  /// Check if this event involves a specific entity
  pub fn involves(&self, entity: Entity) -> bool {
    self.entity_a == entity || self.entity_b == entity
  }

  /// Get the other entity in the collision (given one participant)
  ///
  /// Returns `None` if the given entity is not part of this collision.
  pub fn other(&self, entity: Entity) -> Option<Entity> {
    if self.entity_a == entity {
      Some(self.entity_b)
    } else if self.entity_b == entity {
      Some(self.entity_a)
    } else {
      None
    }
  }

  /// Returns true if this is a collision start event.
  ///
  /// Use this to react when objects first touch.
  /// Most game logic (damage, scoring, death) should check this.
  #[inline]
  pub fn is_started(&self) -> bool {
    self.event_type == CollisionEventType::Started
  }

  /// Returns true if this is a collision end event.
  ///
  /// Use this to react when objects separate.
  /// Useful for "exit trigger" logic or resetting state.
  #[inline]
  pub fn is_stopped(&self) -> bool {
    self.event_type == CollisionEventType::Stopped
  }
}

/// Wrapper around Rapier2D physics simulation
///
/// This struct manages the physics world and provides methods for:
/// - Registering/unregistering entities with physics bodies
/// - Stepping the physics simulation
/// - Syncing positions between Rapier and the ECS world
/// - Collecting and querying collision events
pub struct RapierPhysicsWorld {
  // Rapier structures
  rigid_body_set: RigidBodySet,
  collider_set: ColliderSet,
  integration_parameters: IntegrationParameters,
  physics_pipeline: PhysicsPipeline,
  island_manager: IslandManager,
  broad_phase: DefaultBroadPhase,
  narrow_phase: NarrowPhase,
  impulse_joint_set: ImpulseJointSet,
  multibody_joint_set: MultibodyJointSet,
  ccd_solver: CCDSolver,

  // Mapping between ECS entities and Rapier handles
  entity_to_body: HashMap<Entity, RigidBodyHandle>,
  body_to_entity: HashMap<RigidBodyHandle, Entity>,
  entity_to_collider: HashMap<Entity, ColliderHandle>,
  collider_to_entity: HashMap<ColliderHandle, Entity>,

  // Collected events from the last step
  collision_events: Vec<PhysicsCollisionEvent>,
}

impl RapierPhysicsWorld {
  /// Create a new Rapier physics world
  pub fn new() -> Self {
    Self {
      rigid_body_set: RigidBodySet::new(),
      collider_set: ColliderSet::new(),
      integration_parameters: IntegrationParameters::default(),
      physics_pipeline: PhysicsPipeline::new(),
      island_manager: IslandManager::new(),
      broad_phase: DefaultBroadPhase::new(),
      narrow_phase: NarrowPhase::new(),
      impulse_joint_set: ImpulseJointSet::new(),
      multibody_joint_set: MultibodyJointSet::new(),
      ccd_solver: CCDSolver::new(),
      entity_to_body: HashMap::new(),
      body_to_entity: HashMap::new(),
      entity_to_collider: HashMap::new(),
      collider_to_entity: HashMap::new(),
      collision_events: Vec::new(),
    }
  }

  /// Clear all physics bodies and reset the physics world.
  ///
  /// Call this when resetting game state or switching scenes to ensure
  /// no stale physics bodies remain. This is more efficient than
  /// unregistering entities one by one.
  pub fn clear(&mut self) {
    // Clear all Rapier structures
    self.rigid_body_set = RigidBodySet::new();
    self.collider_set = ColliderSet::new();
    self.island_manager = IslandManager::new();
    self.broad_phase = DefaultBroadPhase::new();
    self.narrow_phase = NarrowPhase::new();
    self.impulse_joint_set = ImpulseJointSet::new();
    self.multibody_joint_set = MultibodyJointSet::new();

    // Clear entity mappings
    self.entity_to_body.clear();
    self.body_to_entity.clear();
    self.entity_to_collider.clear();
    self.collider_to_entity.clear();

    // Clear any pending collision events
    self.collision_events.clear();
  }

  /// Check if an entity is registered with the physics world
  pub fn is_registered(&self, entity: Entity) -> bool {
    self.entity_to_body.contains_key(&entity)
  }

  /// Iterate over all registered entities
  ///
  /// Useful for cleanup when checking which entities have been despawned.
  pub fn registered_entities(&self) -> impl Iterator<Item = Entity> + '_ {
    self.entity_to_body.keys().copied()
  }

  /// Register an ECS entity with the physics world
  ///
  /// Call this when spawning entities with RigidBody + Collider + Transform.
  /// If the entity is already registered, this is a no-op.
  pub fn register_entity(
    &mut self,
    entity: Entity,
    transform: &Transform,
    velocity: Option<&Velocity>,
    rigid_body: &RigidBody,
    collider: &Collider,
  ) {
    // Skip if already registered
    if self.entity_to_body.contains_key(&entity) {
      return;
    }

    // Convert position from pixels to meters
    let x_meters = transform.x / PIXELS_PER_METER;
    let y_meters = transform.y / PIXELS_PER_METER;

    // Create Rapier rigid body
    // Lock rotation for all bodies - this is a 2D engine and we don't sync rotation
    let body = if rigid_body.is_kinematic {
      RigidBodyBuilder::kinematic_position_based()
        .translation(vector![x_meters, y_meters])
        .locked_axes(LockedAxes::ROTATION_LOCKED)
        .build()
    } else {
      let mut builder = RigidBodyBuilder::dynamic()
        .translation(vector![x_meters, y_meters])
        .gravity_scale(rigid_body.gravity_scale)
        .linear_damping(rigid_body.drag)
        .locked_axes(LockedAxes::ROTATION_LOCKED)
        .ccd_enabled(rigid_body.ccd_enabled);

      // Set initial velocity if present
      if let Some(vel) = velocity {
        let vx_meters = vel.dx / PIXELS_PER_METER;
        let vy_meters = vel.dy / PIXELS_PER_METER;
        builder = builder.linvel(vector![vx_meters, vy_meters]);
      }

      builder.build()
    };

    let body_handle = self.rigid_body_set.insert(body);

    // Calculate collider offset in meters
    // For box colliders, also account for the fact that Rapier centers boxes
    // but our sprites use top-left positioning
    let (offset_x_meters, offset_y_meters) = match collider.shape {
      ColliderShape::Box { width, height } => {
        // Add half-size to convert from top-left to center positioning
        let center_offset_x = (collider.offset_x + width / 2.0) / PIXELS_PER_METER;
        let center_offset_y = (collider.offset_y + height / 2.0) / PIXELS_PER_METER;
        (center_offset_x, center_offset_y)
      }
      ColliderShape::Circle { .. } => {
        // Circles are already centered, just apply the offset
        let offset_x = collider.offset_x / PIXELS_PER_METER;
        let offset_y = collider.offset_y / PIXELS_PER_METER;
        (offset_x, offset_y)
      }
    };

    // Create Rapier collider based on shape
    // All colliders get COLLISION_EVENTS enabled to support event queries
    // NOTE: Collider dimensions should be the FINAL world-space sizes
    //       (already scaled by the game if needed)
    let rapier_collider = match collider.shape {
      ColliderShape::Box { width, height } => {
        let half_w = (width / 2.0) / PIXELS_PER_METER;
        let half_h = (height / 2.0) / PIXELS_PER_METER;
        ColliderBuilder::cuboid(half_w, half_h)
          .translation(vector![offset_x_meters, offset_y_meters])
          .restitution(collider.restitution)
          .friction(collider.friction)
          .sensor(collider.is_trigger)
          .active_events(ActiveEvents::COLLISION_EVENTS)
          .build()
      }
      ColliderShape::Circle { radius } => {
        let radius_meters = radius / PIXELS_PER_METER;
        ColliderBuilder::ball(radius_meters)
          .translation(vector![offset_x_meters, offset_y_meters])
          .restitution(collider.restitution)
          .friction(collider.friction)
          .sensor(collider.is_trigger)
          .active_events(ActiveEvents::COLLISION_EVENTS)
          .build()
      }
    };

    let collider_handle =
      self
        .collider_set
        .insert_with_parent(rapier_collider, body_handle, &mut self.rigid_body_set);

    // Store mappings
    self.entity_to_body.insert(entity, body_handle);
    self.body_to_entity.insert(body_handle, entity);
    self.entity_to_collider.insert(entity, collider_handle);
    self.collider_to_entity.insert(collider_handle, entity);
  }

  /// Register an ECS entity with a grouped collider (for composite tiles)
  ///
  /// Similar to `register_entity` but uses a `GroupedCollider` instead of `Collider`.
  /// GroupedCollider is a simple box that covers an entire composite tile.
  pub fn register_grouped_entity(
    &mut self,
    entity: Entity,
    transform: &Transform,
    velocity: Option<&Velocity>,
    rigid_body: &RigidBody,
    grouped: &GroupedCollider,
  ) {
    // Skip if already registered
    if self.entity_to_body.contains_key(&entity) {
      return;
    }

    // Convert position from pixels to meters
    let x_meters = transform.x / PIXELS_PER_METER;
    let y_meters = transform.y / PIXELS_PER_METER;

    // Create Rapier rigid body
    let body = if rigid_body.is_kinematic {
      RigidBodyBuilder::kinematic_position_based()
        .translation(vector![x_meters, y_meters])
        .locked_axes(LockedAxes::ROTATION_LOCKED)
        .build()
    } else {
      let mut builder = RigidBodyBuilder::dynamic()
        .translation(vector![x_meters, y_meters])
        .gravity_scale(rigid_body.gravity_scale)
        .linear_damping(rigid_body.drag)
        .locked_axes(LockedAxes::ROTATION_LOCKED)
        .ccd_enabled(rigid_body.ccd_enabled);

      if let Some(vel) = velocity {
        let vx_meters = vel.dx / PIXELS_PER_METER;
        let vy_meters = vel.dy / PIXELS_PER_METER;
        builder = builder.linvel(vector![vx_meters, vy_meters]);
      }

      builder.build()
    };

    let body_handle = self.rigid_body_set.insert(body);

    // GroupedCollider uses top-left positioning like regular box colliders
    // Convert offset + half-size to center position
    let half_w = grouped.width / 2.0;
    let half_h = grouped.height / 2.0;
    let center_offset_x = (grouped.offset_x + half_w) / PIXELS_PER_METER;
    let center_offset_y = (grouped.offset_y + half_h) / PIXELS_PER_METER;

    // Create box collider for the grouped area
    let half_w_meters = half_w / PIXELS_PER_METER;
    let half_h_meters = half_h / PIXELS_PER_METER;

    let rapier_collider = ColliderBuilder::cuboid(half_w_meters, half_h_meters)
      .translation(vector![center_offset_x, center_offset_y])
      .restitution(grouped.restitution)
      .friction(grouped.friction)
      .sensor(grouped.is_trigger)
      .active_events(ActiveEvents::COLLISION_EVENTS)
      .build();

    let collider_handle =
      self
        .collider_set
        .insert_with_parent(rapier_collider, body_handle, &mut self.rigid_body_set);

    // Store mappings
    self.entity_to_body.insert(entity, body_handle);
    self.body_to_entity.insert(body_handle, entity);
    self.entity_to_collider.insert(entity, collider_handle);
    self.collider_to_entity.insert(collider_handle, entity);
  }

  /// Remove an entity from the physics world
  pub fn unregister_entity(&mut self, entity: Entity) {
    if let Some(body_handle) = self.entity_to_body.remove(&entity) {
      self.body_to_entity.remove(&body_handle);
      if let Some(collider_handle) = self.entity_to_collider.remove(&entity) {
        self.collider_to_entity.remove(&collider_handle);
      }
      self.rigid_body_set.remove(
        body_handle,
        &mut self.island_manager,
        &mut self.collider_set,
        &mut self.impulse_joint_set,
        &mut self.multibody_joint_set,
        true,
      );
    }
  }

  /// Step the physics simulation
  ///
  /// `gravity` is in pixels per second squared (e.g., (0.0, 980.0) for Earth-like gravity)
  ///
  /// Collision events are collected during this step and can be queried
  /// with `get_collision_events()` or `drain_collision_events()`.
  pub fn step(&mut self, gravity: (f32, f32)) {
    // Clear previous events
    self.collision_events.clear();

    let gravity_meters = vector![gravity.0 / PIXELS_PER_METER, gravity.1 / PIXELS_PER_METER];

    // Create fresh channels for this step using crossbeam (required by Rapier)
    let (collision_send, collision_recv) = channel::unbounded();
    let (contact_force_send, contact_force_recv) = channel::unbounded();
    let event_handler = ChannelEventCollector::new(collision_send, contact_force_send);

    self.physics_pipeline.step(
      &gravity_meters,
      &self.integration_parameters,
      &mut self.island_manager,
      &mut self.broad_phase,
      &mut self.narrow_phase,
      &mut self.rigid_body_set,
      &mut self.collider_set,
      &mut self.impulse_joint_set,
      &mut self.multibody_joint_set,
      &mut self.ccd_solver,
      None,
      &(),
      &event_handler,
    );

    // Collect and convert Rapier events to our event type
    while let Ok(event) = collision_recv.try_recv() {
      let (collider1, collider2, started) = match event {
        CollisionEvent::Started(c1, c2, _flags) => (c1, c2, true),
        CollisionEvent::Stopped(c1, c2, _flags) => (c1, c2, false),
      };

      // Look up entities from collider handles
      if let (Some(&entity_a), Some(&entity_b)) = (
        self.collider_to_entity.get(&collider1),
        self.collider_to_entity.get(&collider2),
      ) {
        // Check if either collider is a sensor
        let is_sensor = self
          .collider_set
          .get(collider1)
          .is_some_and(|c| c.is_sensor())
          || self
            .collider_set
            .get(collider2)
            .is_some_and(|c| c.is_sensor());

        self.collision_events.push(PhysicsCollisionEvent {
          entity_a,
          entity_b,
          event_type: if started {
            CollisionEventType::Started
          } else {
            CollisionEventType::Stopped
          },
          is_sensor,
        });
      }
    }

    // Contact force events channel is dropped here - no need to drain
    drop(contact_force_recv);
  }

  /// Sync Rapier body positions and velocities back to ECS components
  ///
  /// Call this after `step()` to update Transform and Velocity components
  pub fn sync_to_ecs(&self, world: &mut World) {
    for (&entity, &body_handle) in &self.entity_to_body {
      if let Some(body) = self.rigid_body_set.get(body_handle) {
        // Skip kinematic bodies (they are controlled by game code)
        if body.is_kinematic() {
          continue;
        }

        let pos = body.translation();
        let vel = body.linvel();

        // Convert from meters back to pixels
        let x_pixels = pos.x * PIXELS_PER_METER;
        let y_pixels = pos.y * PIXELS_PER_METER;
        let vx_pixels = vel.x * PIXELS_PER_METER;
        let vy_pixels = vel.y * PIXELS_PER_METER;

        // Update Transform
        if let Some(mut transform) = world.get_component_mut::<Transform>(entity) {
          transform.x = x_pixels;
          transform.y = y_pixels;
        }

        // Update Velocity
        if let Some(mut velocity) = world.get_component_mut::<Velocity>(entity) {
          velocity.dx = vx_pixels;
          velocity.dy = vy_pixels;
        }
      }
    }
  }

  /// Sync ECS positions to Rapier (for kinematic bodies moved by game code)
  ///
  /// Call this before `step()` to ensure kinematic bodies are updated
  pub fn sync_kinematic_from_ecs(&mut self, world: &World) {
    for (&entity, &body_handle) in &self.entity_to_body {
      if let Some(body) = self.rigid_body_set.get_mut(body_handle)
        && body.is_kinematic()
        && let Some(transform) = world.get_component::<Transform>(entity)
      {
        let x_meters = transform.x / PIXELS_PER_METER;
        let y_meters = transform.y / PIXELS_PER_METER;
        body.set_next_kinematic_translation(vector![x_meters, y_meters]);
      }
    }
  }

  /// Sync dynamic body positions from ECS to Rapier (for teleportation/reset)
  ///
  /// Call this before `step()` if game code modifies Transform of dynamic bodies directly.
  /// This teleports the body to the new position without physics interpolation.
  pub fn sync_dynamic_from_ecs(&mut self, world: &World) {
    for (&entity, &body_handle) in &self.entity_to_body {
      if let Some(body) = self.rigid_body_set.get_mut(body_handle)
        && body.is_dynamic()
        && let Some(transform) = world.get_component::<Transform>(entity)
      {
        let x_meters = transform.x / PIXELS_PER_METER;
        let y_meters = transform.y / PIXELS_PER_METER;
        body.set_translation(vector![x_meters, y_meters], true);
      }
    }
  }

  /// Sync velocity changes from ECS to Rapier (for impulses applied in game code)
  ///
  /// Call this before `step()` if game code modifies Velocity directly
  pub fn sync_velocities_from_ecs(&mut self, world: &World) {
    for (&entity, &body_handle) in &self.entity_to_body {
      if let Some(body) = self.rigid_body_set.get_mut(body_handle)
        && !body.is_kinematic()
        && let Some(velocity) = world.get_component::<Velocity>(entity)
      {
        let vx_meters = velocity.dx / PIXELS_PER_METER;
        let vy_meters = velocity.dy / PIXELS_PER_METER;
        body.set_linvel(vector![vx_meters, vy_meters], true);
      }
    }
  }

  /// Get collision events from the last physics step
  ///
  /// Returns a slice of all collision events that occurred. Events are
  /// cleared at the start of each `step()` call.
  ///
  /// # Example
  ///
  /// ```ignore
  /// for event in physics.get_collision_events() {
  ///     if event.involves(player_entity) {
  ///         match event.event_type {
  ///             CollisionEventType::Started => println!("Player hit something!"),
  ///             CollisionEventType::Stopped => println!("Player separated"),
  ///         }
  ///     }
  /// }
  /// ```
  pub fn get_collision_events(&self) -> &[PhysicsCollisionEvent] {
    &self.collision_events
  }

  /// Take ownership of collision events, clearing the internal buffer
  ///
  /// Prefer `get_collision_events()` if you don't need ownership.
  pub fn drain_collision_events(&mut self) -> Vec<PhysicsCollisionEvent> {
    std::mem::take(&mut self.collision_events)
  }

  /// Get collision events involving a specific entity
  pub fn get_collision_events_for(
    &self,
    entity: Entity,
  ) -> impl Iterator<Item = &PhysicsCollisionEvent> {
    self
      .collision_events
      .iter()
      .filter(move |e| e.involves(entity))
  }

  /// Check if an entity had any collision this frame
  pub fn has_collision(&self, entity: Entity) -> bool {
    self.collision_events.iter().any(|e| e.involves(entity))
  }

  /// Check if two entities are currently intersecting using Rapier's narrow phase
  ///
  /// Unlike collision events which only fire on state change, this checks
  /// the current intersection status directly.
  pub fn are_intersecting(&self, entity_a: Entity, entity_b: Entity) -> bool {
    let Some(&collider_a) = self.entity_to_collider.get(&entity_a) else {
      return false;
    };
    let Some(&collider_b) = self.entity_to_collider.get(&entity_b) else {
      return false;
    };

    // Check if there's a contact pair in the narrow phase
    self
      .narrow_phase
      .contact_pair(collider_a, collider_b)
      .is_some()
  }

  /// Check if an entity is currently intersecting with any other entity
  ///
  /// Returns a list of entities the given entity is currently touching.
  pub fn get_current_intersections(&self, entity: Entity) -> Vec<Entity> {
    let Some(&collider_handle) = self.entity_to_collider.get(&entity) else {
      return Vec::new();
    };

    let mut intersecting = Vec::new();

    // Iterate all contact pairs involving this collider
    for contact_pair in self.narrow_phase.contact_pairs_with(collider_handle) {
      let other_handle = if contact_pair.collider1 == collider_handle {
        contact_pair.collider2
      } else {
        contact_pair.collider1
      };

      if let Some(&other_entity) = self.collider_to_entity.get(&other_handle) {
        // Check if there are actual contact points (not just AABB overlap)
        if contact_pair.has_any_active_contact {
          intersecting.push(other_entity);
        }
      }
    }

    intersecting
  }

  /// Get the number of collision events from the last step
  pub fn collision_event_count(&self) -> usize {
    self.collision_events.len()
  }

  /// Get the number of registered physics bodies
  pub fn body_count(&self) -> usize {
    self.rigid_body_set.len()
  }
}

impl Default for RapierPhysicsWorld {
  fn default() -> Self {
    Self::new()
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ecs::{Transform, Velocity, World};
  use crate::core::physics::{Collider, RigidBody};

  #[test]
  fn rapier_world_creates() {
    let physics = RapierPhysicsWorld::new();
    assert_eq!(physics.body_count(), 0);
  }

  #[test]
  fn register_entity() {
    let mut physics = RapierPhysicsWorld::new();
    let mut world = World::new();

    let entity = world.spawn((
      Transform::new(100.0, 100.0),
      Velocity::zero(),
      RigidBody::new(),
      Collider::box_collider(32.0, 32.0),
    ));

    // Register with physics
    {
      let transform = world.get_component::<Transform>(entity).unwrap();
      let velocity = world.get_component::<Velocity>(entity);
      let rb = world.get_component::<RigidBody>(entity).unwrap();
      let col = world.get_component::<Collider>(entity).unwrap();
      physics.register_entity(entity, &transform, velocity.as_deref(), &rb, &col);
    }

    assert!(physics.is_registered(entity));
    assert_eq!(physics.body_count(), 1);
  }

  #[test]
  fn unregister_entity() {
    let mut physics = RapierPhysicsWorld::new();
    let mut world = World::new();

    let entity = world.spawn((
      Transform::new(100.0, 100.0),
      RigidBody::new(),
      Collider::circle_collider(16.0),
    ));

    {
      let transform = world.get_component::<Transform>(entity).unwrap();
      let rb = world.get_component::<RigidBody>(entity).unwrap();
      let col = world.get_component::<Collider>(entity).unwrap();
      physics.register_entity(entity, &transform, None, &rb, &col);
    }

    assert_eq!(physics.body_count(), 1);
    physics.unregister_entity(entity);
    assert_eq!(physics.body_count(), 0);
    assert!(!physics.is_registered(entity));
  }

  #[test]
  fn step_applies_gravity() {
    let mut physics = RapierPhysicsWorld::new();
    let mut world = World::new();

    let entity = world.spawn((
      Transform::new(100.0, 100.0),
      Velocity::zero(),
      RigidBody::new(),
      Collider::box_collider(32.0, 32.0),
    ));

    {
      let transform = world.get_component::<Transform>(entity).unwrap();
      let velocity = world.get_component::<Velocity>(entity);
      let rb = world.get_component::<RigidBody>(entity).unwrap();
      let col = world.get_component::<Collider>(entity).unwrap();
      physics.register_entity(entity, &transform, velocity.as_deref(), &rb, &col);
    }

    // Step physics with gravity
    physics.step((0.0, 980.0));
    physics.sync_to_ecs(&mut world);

    // Entity should have fallen
    let transform = world.get_component::<Transform>(entity).unwrap();
    assert!(transform.y > 100.0, "Entity should fall due to gravity");
  }

  #[test]
  fn kinematic_body_not_affected_by_gravity() {
    let mut physics = RapierPhysicsWorld::new();
    let mut world = World::new();

    let entity = world.spawn((
      Transform::new(100.0, 100.0),
      RigidBody::kinematic(),
      Collider::box_collider(32.0, 32.0),
    ));

    {
      let transform = world.get_component::<Transform>(entity).unwrap();
      let rb = world.get_component::<RigidBody>(entity).unwrap();
      let col = world.get_component::<Collider>(entity).unwrap();
      physics.register_entity(entity, &transform, None, &rb, &col);
    }

    // Step physics with gravity
    physics.step((0.0, 980.0));
    physics.sync_to_ecs(&mut world);

    // Kinematic body should not move
    let transform = world.get_component::<Transform>(entity).unwrap();
    assert!(
      (transform.y - 100.0).abs() < 0.01,
      "Kinematic body should not fall"
    );
  }

  #[test]
  fn collision_response() {
    let mut physics = RapierPhysicsWorld::new();
    let mut world = World::new();

    // Falling ball
    let ball = world.spawn((
      Transform::new(100.0, 50.0),
      Velocity::zero(),
      RigidBody::new(),
      Collider::circle_collider(16.0),
    ));

    // Static floor (kinematic)
    let floor = world.spawn((
      Transform::new(100.0, 200.0),
      RigidBody::kinematic(),
      Collider::box_collider(200.0, 20.0),
    ));

    // Register both
    {
      let t = world.get_component::<Transform>(ball).unwrap();
      let v = world.get_component::<Velocity>(ball);
      let rb = world.get_component::<RigidBody>(ball).unwrap();
      let col = world.get_component::<Collider>(ball).unwrap();
      physics.register_entity(ball, &t, v.as_deref(), &rb, &col);
    }
    {
      let t = world.get_component::<Transform>(floor).unwrap();
      let rb = world.get_component::<RigidBody>(floor).unwrap();
      let col = world.get_component::<Collider>(floor).unwrap();
      physics.register_entity(floor, &t, None, &rb, &col);
    }

    // Step multiple times
    for _ in 0..60 {
      physics.step((0.0, 980.0));
    }
    physics.sync_to_ecs(&mut world);

    // Ball should have landed on floor (not fallen through)
    let ball_transform = world.get_component::<Transform>(ball).unwrap();
    assert!(
      ball_transform.y < 200.0,
      "Ball should be stopped by floor, not fall through. Y = {}",
      ball_transform.y
    );
  }

  #[test]
  fn collision_events_generated() {
    let mut physics = RapierPhysicsWorld::new();
    let mut world = World::new();

    // Ball starting above floor (will collide after physics step)
    let ball = world.spawn((
      Transform::new(100.0, 80.0), // Ball at y=80
      Velocity::new(0.0, 100.0),   // Moving downward
      RigidBody::new().with_gravity_scale(1.0),
      Collider::circle_collider(10.0),
    ));

    // Static floor at y=100
    let floor = world.spawn((
      Transform::new(50.0, 100.0),
      RigidBody::kinematic(),
      Collider::box_collider(100.0, 20.0),
    ));

    // Register both entities
    {
      let t = world.get_component::<Transform>(ball).unwrap();
      let v = world.get_component::<Velocity>(ball);
      let rb = world.get_component::<RigidBody>(ball).unwrap();
      let col = world.get_component::<Collider>(ball).unwrap();
      physics.register_entity(ball, &t, v.as_deref(), &rb, &col);
    }
    {
      let t = world.get_component::<Transform>(floor).unwrap();
      let rb = world.get_component::<RigidBody>(floor).unwrap();
      let col = world.get_component::<Collider>(floor).unwrap();
      physics.register_entity(floor, &t, None, &rb, &col);
    }

    // Step physics until collision happens
    let mut collision_detected = false;
    for _ in 0..30 {
      physics.step((0.0, 980.0));
      physics.sync_to_ecs(&mut world);

      // Check for collision events
      for event in physics.get_collision_events() {
        if event.event_type == CollisionEventType::Started
          && event.involves(ball)
          && event.involves(floor)
        {
          collision_detected = true;
          // Verify helper methods work
          assert_eq!(event.other(ball), Some(floor));
          assert_eq!(event.other(floor), Some(ball));
        }
      }
    }

    assert!(
      collision_detected,
      "Expected collision event between ball and floor"
    );

    // Step again to show events are cleared between steps
    physics.step((0.0, 980.0));
    // After a step with no new collisions starting, event count should reflect
    // only events from that step (could be 0 or continued contact)
  }

  #[test]
  fn clear_resets_physics_world() {
    let mut physics = RapierPhysicsWorld::new();
    let mut world = World::new();

    // Register some entities
    let e1 = world.spawn((
      Transform::new(100.0, 100.0),
      RigidBody::new(),
      Collider::box_collider(32.0, 32.0),
    ));
    let e2 = world.spawn((
      Transform::new(200.0, 200.0),
      RigidBody::kinematic(),
      Collider::circle_collider(16.0),
    ));

    {
      let t = world.get_component::<Transform>(e1).unwrap();
      let rb = world.get_component::<RigidBody>(e1).unwrap();
      let col = world.get_component::<Collider>(e1).unwrap();
      physics.register_entity(e1, &t, None, &rb, &col);
    }
    {
      let t = world.get_component::<Transform>(e2).unwrap();
      let rb = world.get_component::<RigidBody>(e2).unwrap();
      let col = world.get_component::<Collider>(e2).unwrap();
      physics.register_entity(e2, &t, None, &rb, &col);
    }

    assert_eq!(physics.body_count(), 2);
    assert!(physics.is_registered(e1));
    assert!(physics.is_registered(e2));

    // Clear the physics world
    physics.clear();

    // All bodies should be gone
    assert_eq!(physics.body_count(), 0);
    assert!(!physics.is_registered(e1));
    assert!(!physics.is_registered(e2));

    // Should be able to register new entities after clear
    let e3 = world.spawn((
      Transform::new(50.0, 50.0),
      RigidBody::new(),
      Collider::box_collider(10.0, 10.0),
    ));
    {
      let t = world.get_component::<Transform>(e3).unwrap();
      let rb = world.get_component::<RigidBody>(e3).unwrap();
      let col = world.get_component::<Collider>(e3).unwrap();
      physics.register_entity(e3, &t, None, &rb, &col);
    }
    assert_eq!(physics.body_count(), 1);
    assert!(physics.is_registered(e3));
  }

  #[test]
  fn collision_event_is_started_is_stopped() {
    // Test Started event
    let started_event = PhysicsCollisionEvent {
      entity_a: hecs::Entity::DANGLING,
      entity_b: hecs::Entity::DANGLING,
      event_type: CollisionEventType::Started,
      is_sensor: false,
    };
    assert!(started_event.is_started());
    assert!(!started_event.is_stopped());

    // Test Stopped event
    let stopped_event = PhysicsCollisionEvent {
      entity_a: hecs::Entity::DANGLING,
      entity_b: hecs::Entity::DANGLING,
      event_type: CollisionEventType::Stopped,
      is_sensor: false,
    };
    assert!(!stopped_event.is_started());
    assert!(stopped_event.is_stopped());
  }
}
