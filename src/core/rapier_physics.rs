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
use super::ecs::{Entity, Transform, Velocity, World};
use super::physics::RigidBody;

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

  /// Check if an entity is registered with the physics world
  pub fn is_registered(&self, entity: Entity) -> bool {
    self.entity_to_body.contains_key(&entity)
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
    let body = if rigid_body.is_kinematic {
      RigidBodyBuilder::kinematic_position_based()
        .translation(vector![x_meters, y_meters])
        .build()
    } else {
      let mut builder = RigidBodyBuilder::dynamic()
        .translation(vector![x_meters, y_meters])
        .gravity_scale(rigid_body.gravity_scale)
        .linear_damping(rigid_body.drag);

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
    let rapier_collider = match collider.shape {
      ColliderShape::Box { width, height } => {
        let half_w = (width / 2.0) / PIXELS_PER_METER;
        let half_h = (height / 2.0) / PIXELS_PER_METER;
        ColliderBuilder::cuboid(half_w, half_h)
          .translation(vector![offset_x_meters, offset_y_meters])
          .restitution(0.5) // Default bounciness
          .friction(0.3) // Default friction
          .sensor(collider.is_trigger)
          .active_events(ActiveEvents::COLLISION_EVENTS)
          .build()
      }
      ColliderShape::Circle { radius } => {
        let radius_meters = radius / PIXELS_PER_METER;
        ColliderBuilder::ball(radius_meters)
          .translation(vector![offset_x_meters, offset_y_meters])
          .restitution(0.7) // Circles bounce more
          .friction(0.3)
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
  use super::{
    super::{
      collision::Collider,
      ecs::{Transform, Velocity, World},
      physics::RigidBody,
    },
    *,
  };

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
}
