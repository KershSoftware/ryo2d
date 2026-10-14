//! Rapier2D physics integration
//!
//! This module wraps Rapier2D to provide physics simulation with collision response.
//! It maps ECS entities to Rapier bodies; the physics systems
//! (`sync.rs`) register bodies, push game changes in, step, and
//! read positions back.
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

use std::sync::{Mutex, PoisonError};

use rapier2d::prelude::{
  ActiveEvents, BroadPhaseBvh, CCDSolver, ColliderBuilder, ColliderHandle, ColliderSet,
  CollisionEvent, ContactPair, EventHandler, ImpulseJointSet, IntegrationParameters, IslandManager,
  LockedAxes, MultibodyJointSet, NarrowPhase, PhysicsPipeline, Real, RigidBodyBuilder,
  RigidBodyHandle, RigidBodySet, RigidBodyType, SoftBodySet, SoftBodyTearEvent,
};

use super::collision::{Collider, ColliderShape};
use super::components::RigidBody;
use super::config::PhysicsConfig;
use super::convert::{from_rapier, to_rapier};
use crate::core::ecs::{Entity, Resource, Transform, Vec2, Velocity};
use crate::core::graphics::GroupedCollider;

/// Collects Rapier's events during a step; drained right after it.
#[derive(Default)]
struct EventCollector {
  collisions: Mutex<Vec<CollisionEvent>>,
}

impl EventHandler for EventCollector {
  fn handle_collision_event(
    &self,
    _bodies: &RigidBodySet,
    _colliders: &ColliderSet,
    event: CollisionEvent,
    _contact_pair: Option<&ContactPair>,
  ) {
    self
      .collisions
      .lock()
      .unwrap_or_else(PoisonError::into_inner)
      .push(event);
  }

  fn handle_contact_force_event(
    &self,
    _dt: Real,
    _bodies: &RigidBodySet,
    _colliders: &ColliderSet,
    _contact_pair: &ContactPair,
    _total_force_magnitude: Real,
  ) {
  }

  fn handle_soft_body_tear_event(&self, _soft_bodies: &SoftBodySet, _event: &SoftBodyTearEvent) {}
}

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
/// - Pushing game writes in and reading simulated positions out
/// - Collecting and querying collision events
#[derive(Resource)]
pub struct Physics {
  // Rapier structures
  rigid_body_set: RigidBodySet,
  collider_set: ColliderSet,
  integration_parameters: IntegrationParameters,
  physics_pipeline: PhysicsPipeline,
  island_manager: IslandManager,
  broad_phase: BroadPhaseBvh,
  narrow_phase: NarrowPhase,
  impulse_joint_set: ImpulseJointSet,
  multibody_joint_set: MultibodyJointSet,
  soft_bodies: SoftBodySet,
  ccd_solver: CCDSolver,
  collector: EventCollector,
  /// Pixels per second squared, from `PhysicsConfig`.
  gravity: Vec2,

  // Mapping between ECS entities and Rapier handles
  entity_to_body: HashMap<Entity, RigidBodyHandle>,
  body_to_entity: HashMap<RigidBodyHandle, Entity>,
  entity_to_collider: HashMap<Entity, ColliderHandle>,
  collider_to_entity: HashMap<ColliderHandle, Entity>,

  // Collected events from the last step
  collision_events: Vec<PhysicsCollisionEvent>,
}

impl Physics {
  /// Create a new Rapier physics world
  pub fn new() -> Self {
    let integration_parameters = IntegrationParameters {
      length_unit: PhysicsConfig::DEFAULT_PIXELS_PER_METER,
      ..IntegrationParameters::default()
    };
    Self {
      rigid_body_set: RigidBodySet::new(),
      collider_set: ColliderSet::new(),
      integration_parameters,
      physics_pipeline: PhysicsPipeline::new(),
      island_manager: IslandManager::new(),
      broad_phase: BroadPhaseBvh::new(),
      narrow_phase: NarrowPhase::new(),
      impulse_joint_set: ImpulseJointSet::new(),
      multibody_joint_set: MultibodyJointSet::new(),
      soft_bodies: SoftBodySet::new(),
      ccd_solver: CCDSolver::new(),
      collector: EventCollector::default(),
      gravity: Vec2::new(0.0, 980.0),
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
    self.broad_phase = BroadPhaseBvh::new();
    self.narrow_phase = NarrowPhase::new();
    self.impulse_joint_set = ImpulseJointSet::new();
    self.multibody_joint_set = MultibodyJointSet::new();
    self.soft_bodies = SoftBodySet::new();

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

    let body = Self::build_body(transform, velocity, rigid_body);
    let body_handle = self.rigid_body_set.insert(body);

    // The shape is centred on the entity's origin plus the collider's offset
    let offset = collider.offset;

    // Create Rapier collider based on shape
    // All colliders get COLLISION_EVENTS enabled to support event queries
    // NOTE: Collider dimensions should be the FINAL world-space sizes
    //       (already scaled by the game if needed)
    let rapier_collider = match collider.shape {
      ColliderShape::Box { width, height } => ColliderBuilder::cuboid(width / 2.0, height / 2.0),
      ColliderShape::Circle { radius } => ColliderBuilder::ball(radius),
    }
    .translation(to_rapier(offset))
    .density(self.rapier_density(1.0))
    .restitution(collider.restitution)
    .friction(collider.friction)
    .sensor(collider.is_trigger)
    .active_events(ActiveEvents::COLLISION_EVENTS)
    .build();

    let collider_handle =
      self
        .collider_set
        .insert_with_parent(rapier_collider, body_handle, &mut self.rigid_body_set);
    self.insert_mappings(entity, body_handle, collider_handle);
  }

  /// Bring a registered body's settings in line with its `RigidBody`: body
  /// type, gravity scale, drag and CCD. Only values that differ are written,
  /// so an unchanged body keeps its contacts and stays asleep.
  pub fn apply_body_settings(&mut self, entity: Entity, rigid_body: &RigidBody) {
    let Some(handle) = self.entity_to_body.get(&entity).copied() else {
      return;
    };
    let Some(body) = self.rigid_body_set.get_mut(handle) else {
      return;
    };
    let body_type = if rigid_body.is_kinematic {
      RigidBodyType::KinematicPositionBased
    } else {
      RigidBodyType::Dynamic
    };
    if body.body_type() != body_type {
      body.set_body_type(body_type, true);
    }
    if !rigid_body.is_kinematic {
      if body.gravity_scale() != rigid_body.gravity_scale {
        body.set_gravity_scale(rigid_body.gravity_scale, true);
      }
      if body.linear_damping() != rigid_body.drag {
        body.set_linear_damping(rigid_body.drag);
      }
      if body.is_ccd_enabled() != rigid_body.ccd_enabled {
        body.enable_ccd(rigid_body.ccd_enabled);
      }
    }
  }

  /// A Rapier body at the entity's position. Rotation is locked: bodies do
  /// not rotate, and the engine does not sync rotation.
  fn build_body(
    transform: &Transform,
    velocity: Option<&Velocity>,
    rigid_body: &RigidBody,
  ) -> rapier2d::dynamics::RigidBody {
    let position = to_rapier(transform.translation);
    if rigid_body.is_kinematic {
      RigidBodyBuilder::kinematic_position_based()
        .translation(position)
        .locked_axes(LockedAxes::ROTATION_LOCKED)
        .build()
    } else {
      let mut builder = RigidBodyBuilder::dynamic()
        .translation(position)
        .gravity_scale(rigid_body.gravity_scale)
        .linear_damping(rigid_body.drag)
        .locked_axes(LockedAxes::ROTATION_LOCKED)
        .ccd_enabled(rigid_body.ccd_enabled);

      // Set initial velocity if present
      if let Some(vel) = velocity {
        builder = builder.linvel(to_rapier(Vec2::new(vel.dx, vel.dy)));
      }

      builder.build()
    }
  }

  fn insert_mappings(
    &mut self,
    entity: Entity,
    body_handle: RigidBodyHandle,
    collider_handle: ColliderHandle,
  ) {
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

    let body = Self::build_body(transform, velocity, rigid_body);
    let body_handle = self.rigid_body_set.insert(body);

    // The box spans `offset` to `offset + size` from the entity's origin, the
    // way composite tile parts are laid out
    let size = Vec2::new(grouped.width, grouped.height);
    let center = Vec2::new(grouped.offset_x, grouped.offset_y) + size / 2.0;
    let half = size / 2.0;
    let rapier_collider = ColliderBuilder::cuboid(half.x, half.y)
      .translation(to_rapier(center))
      .density(self.rapier_density(1.0))
      .restitution(grouped.restitution)
      .friction(grouped.friction)
      .sensor(grouped.is_trigger)
      .active_events(ActiveEvents::COLLISION_EVENTS)
      .build();

    let collider_handle =
      self
        .collider_set
        .insert_with_parent(rapier_collider, body_handle, &mut self.rigid_body_set);
    self.insert_mappings(entity, body_handle, collider_handle);
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
        &mut self.soft_bodies,
        true,
      );
    }
  }

  /// Take the unit, solver iterations and gravity from the config.
  pub(crate) fn configure(&mut self, config: &PhysicsConfig) {
    self.integration_parameters.length_unit = config.length_unit();
    self.integration_parameters.num_solver_iterations = config.iterations();
    self.gravity = if config.gravity.is_finite() {
      config.gravity
    } else {
      Vec2::ZERO
    };
  }

  /// Rapier's density for `density` per square metre.
  pub(crate) fn rapier_density(&self, density: f32) -> f32 {
    let unit = self.integration_parameters.length_unit;
    density / (unit * unit)
  }

  /// Advance the simulation by `dt` seconds (the fixed timestep).
  ///
  /// Collision events are collected during this step and can be queried
  /// with `get_collision_events()` or `drain_collision_events()`.
  pub fn step(&mut self, dt: f32) {
    // Clear previous events
    self.collision_events.clear();
    self.integration_parameters.dt = dt;

    self.physics_pipeline.step(
      to_rapier(self.gravity),
      &self.integration_parameters,
      &mut self.island_manager,
      &mut self.broad_phase,
      &mut self.narrow_phase,
      &mut self.rigid_body_set,
      &mut self.collider_set,
      &mut self.impulse_joint_set,
      &mut self.multibody_joint_set,
      &mut self.soft_bodies,
      &mut self.ccd_solver,
      &(),
      &self.collector,
    );

    // Convert Rapier's events to ours; the drained Vec keeps its capacity
    let collisions = self
      .collector
      .collisions
      .get_mut()
      .unwrap_or_else(PoisonError::into_inner);
    for event in collisions.drain(..) {
      let (collider1, collider2, started) = (event.collider1(), event.collider2(), event.started());

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
  }

  fn body_mut(&mut self, entity: Entity) -> Option<&mut rapier2d::dynamics::RigidBody> {
    let handle = *self.entity_to_body.get(&entity)?;
    self.rigid_body_set.get_mut(handle)
  }

  fn body(&self, entity: Entity) -> Option<&rapier2d::dynamics::RigidBody> {
    self.rigid_body_set.get(*self.entity_to_body.get(&entity)?)
  }

  /// Put the body where the game moved it: a kinematic body moves there over
  /// the next step, any other body teleports (and wakes).
  pub fn set_transform(&mut self, entity: Entity, transform: &Transform) {
    let Some(body) = self.body_mut(entity) else {
      return;
    };
    let p = to_rapier(transform.translation);
    if body.is_kinematic() {
      body.set_next_kinematic_translation(p);
    } else {
      body.set_translation(p, true);
    }
  }

  /// Move a kinematic body to `transform` over the next step; other bodies
  /// are left alone.
  pub fn set_kinematic_target(&mut self, entity: Entity, transform: &Transform) {
    let Some(body) = self.body_mut(entity) else {
      return;
    };
    if body.is_kinematic() {
      body.set_next_kinematic_translation(to_rapier(transform.translation));
    }
  }

  /// Give a dynamic body the game's velocity (pixels per second), waking it.
  pub fn set_velocity(&mut self, entity: Entity, velocity: &Velocity) {
    if let Some(body) = self.body_mut(entity)
      && !body.is_kinematic()
    {
      body.set_linvel(to_rapier(Vec2::new(velocity.dx, velocity.dy)), true);
    }
  }

  /// Awake dynamic bodies: (entity, position px, velocity px/s).
  pub fn read_back(&self) -> impl Iterator<Item = (Entity, Vec2, Vec2)> + '_ {
    self.entity_to_body.iter().filter_map(|(&entity, &handle)| {
      let body = self.rigid_body_set.get(handle)?;
      if body.is_kinematic() || body.is_sleeping() {
        return None;
      }
      Some((
        entity,
        from_rapier(body.translation()),
        from_rapier(body.linvel()),
      ))
    })
  }

  /// The entity's body velocity in pixels per second, if it is registered.
  pub fn body_velocity(&self, entity: Entity) -> Option<Vec2> {
    Some(from_rapier(self.body(entity)?.linvel()))
  }

  /// Whether the entity's body is asleep (at rest, skipped by the solver).
  pub fn is_sleeping(&self, entity: Entity) -> bool {
    self.body(entity).is_some_and(|body| body.is_sleeping())
  }

  /// The entity's body position in pixels, if it is registered.
  pub fn body_position(&self, entity: Entity) -> Option<Vec2> {
    Some(from_rapier(self.body(entity)?.translation()))
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
        if contact_pair.has_any_active_contact() {
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

  /// The entity's body mass, if it is registered.
  #[allow(dead_code)] // the characterization tests use it; later tasks too
  pub(crate) fn body_mass(&self, entity: Entity) -> Option<f32> {
    Some(self.body(entity)?.mass())
  }

  /// Get the number of registered physics bodies
  pub fn body_count(&self) -> usize {
    self.rigid_body_set.len()
  }
}

impl Default for Physics {
  fn default() -> Self {
    Self::new()
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ecs::{Transform, Velocity};
  use crate::core::physics::{Collider, RigidBody};

  /// Register an entity directly (the physics systems do this in the engine).
  fn add(
    physics: &mut Physics,
    entity: Entity,
    x: f32,
    y: f32,
    velocity: Option<Velocity>,
    body: RigidBody,
    collider: Collider,
  ) {
    physics.register_entity(
      entity,
      &Transform::from_xy(x, y),
      velocity.as_ref(),
      &body,
      &collider,
    );
  }

  fn entity(index: u32) -> Entity {
    Entity::from_raw_u32(index).expect("a valid test index")
  }

  fn collider_center(physics: &Physics, entity: Entity) -> Vec2 {
    let handle = physics.entity_to_collider[&entity];
    from_rapier(physics.collider_set[handle].position().translation)
  }

  #[test]
  fn a_100_px_box_weighs_one() {
    let mut world = Physics::new();
    let e = entity(1);
    world.register_entity(
      e,
      &Transform::from_xy(0.0, 0.0),
      None,
      &RigidBody::new(),
      &Collider::box_collider(100.0, 100.0),
    );
    let mass = world.body_mass(e).unwrap();
    assert!((mass - 1.0).abs() < 1e-3, "{mass}");
  }

  #[test]
  fn positions_are_pixels() {
    let mut world = Physics::new();
    let e = entity(1);
    world.register_entity(
      e,
      &Transform::from_xy(250.0, -40.0),
      None,
      &RigidBody::kinematic(),
      &Collider::circle_collider(5.0),
    );
    assert_eq!(world.body_position(e), Some(Vec2::new(250.0, -40.0)));
  }

  #[test]
  fn rapier_world_creates() {
    let physics = Physics::new();
    assert_eq!(physics.body_count(), 0);
  }

  #[test]
  fn register_and_unregister_entity() {
    let mut physics = Physics::new();
    let e = entity(1);
    add(
      &mut physics,
      e,
      100.0,
      100.0,
      Some(Velocity::zero()),
      RigidBody::new(),
      Collider::box_collider(32.0, 32.0),
    );
    assert!(physics.is_registered(e));
    assert_eq!(physics.body_count(), 1);
    physics.unregister_entity(e);
    assert_eq!(physics.body_count(), 0);
    assert!(!physics.is_registered(e));
  }

  #[test]
  fn step_applies_gravity() {
    let mut physics = Physics::new();
    let e = entity(1);
    add(
      &mut physics,
      e,
      100.0,
      100.0,
      None,
      RigidBody::new(),
      Collider::box_collider(32.0, 32.0),
    );
    physics.step(1.0 / 60.0);
    let (moved, position, velocity) = physics.read_back().next().unwrap();
    assert_eq!(moved, e);
    assert!(position.y > 100.0, "Entity should fall due to gravity");
    assert!(velocity.y > 0.0);
  }

  #[test]
  fn kinematic_body_not_affected_by_gravity_nor_read_back() {
    let mut physics = Physics::new();
    let e = entity(1);
    add(
      &mut physics,
      e,
      100.0,
      100.0,
      None,
      RigidBody::kinematic(),
      Collider::box_collider(32.0, 32.0),
    );
    physics.step(1.0 / 60.0);
    assert_eq!(physics.body_position(e), Some(Vec2::new(100.0, 100.0)));
    assert_eq!(physics.read_back().count(), 0);
  }

  #[test]
  fn collision_response_and_events() {
    let mut physics = Physics::new();
    let (ball, floor) = (entity(1), entity(2));
    add(
      &mut physics,
      ball,
      100.0,
      80.0,
      Some(Velocity::new(0.0, 100.0)),
      RigidBody::new(),
      Collider::circle_collider(10.0),
    );
    add(
      &mut physics,
      floor,
      50.0,
      100.0,
      None,
      RigidBody::kinematic(),
      Collider::box_collider(100.0, 20.0),
    );

    let mut collision_detected = false;
    for _ in 0..60 {
      physics.step(1.0 / 60.0);
      for event in physics.get_collision_events() {
        if event.is_started() && event.involves(ball) && event.involves(floor) {
          collision_detected = true;
          assert_eq!(event.other(ball), Some(floor));
          assert_eq!(event.other(floor), Some(ball));
        }
      }
    }
    assert!(
      collision_detected,
      "Expected collision event between ball and floor"
    );
    let y = physics.body_position(ball).unwrap().y;
    assert!(
      y < 100.0,
      "Ball should rest on the floor, not fall through: y = {y}"
    );
  }

  #[test]
  fn set_transform_teleports_and_set_velocity_pushes() {
    let mut physics = Physics::new();
    let e = entity(1);
    add(
      &mut physics,
      e,
      0.0,
      0.0,
      None,
      RigidBody::new().with_gravity_scale(0.0),
      Collider::circle_collider(4.0),
    );
    physics.set_transform(e, &Transform::from_xy(300.0, -20.0));
    assert_eq!(physics.body_position(e), Some(Vec2::new(300.0, -20.0)));
    physics.set_velocity(e, &Velocity::new(60.0, 0.0));
    physics.step(0.5);
    let x = physics.body_position(e).unwrap().x;
    assert!((x - 330.0).abs() < 0.5, "half a second at 60 px/s: {x}");
  }

  #[test]
  fn clear_resets_physics_world() {
    let mut physics = Physics::new();
    let (e1, e2, e3) = (entity(1), entity(2), entity(3));
    add(
      &mut physics,
      e1,
      100.0,
      100.0,
      None,
      RigidBody::new(),
      Collider::box_collider(32.0, 32.0),
    );
    add(
      &mut physics,
      e2,
      200.0,
      200.0,
      None,
      RigidBody::kinematic(),
      Collider::circle_collider(16.0),
    );
    assert_eq!(physics.body_count(), 2);
    physics.clear();
    assert_eq!(physics.body_count(), 0);
    assert!(!physics.is_registered(e1));
    assert!(!physics.is_registered(e2));
    add(
      &mut physics,
      e3,
      50.0,
      50.0,
      None,
      RigidBody::new(),
      Collider::box_collider(10.0, 10.0),
    );
    assert_eq!(physics.body_count(), 1);
    assert!(physics.is_registered(e3));
  }

  #[test]
  fn collision_event_is_started_is_stopped() {
    let started_event = PhysicsCollisionEvent {
      entity_a: Entity::PLACEHOLDER,
      entity_b: Entity::PLACEHOLDER,
      event_type: CollisionEventType::Started,
      is_sensor: false,
    };
    assert!(started_event.is_started());
    assert!(!started_event.is_stopped());

    let stopped_event = PhysicsCollisionEvent {
      event_type: CollisionEventType::Stopped,
      ..started_event
    };
    assert!(!stopped_event.is_started());
    assert!(stopped_event.is_stopped());
  }

  #[test]
  fn a_collider_is_centred_on_the_entity() {
    let mut physics = Physics::new();
    let e = entity(1);
    add(
      &mut physics,
      e,
      100.0,
      100.0,
      None,
      RigidBody::new(),
      Collider::circle_collider(20.0),
    );
    assert!((collider_center(&physics, e) - Vec2::new(100.0, 100.0)).length() < 1e-3);
  }

  #[test]
  fn a_collider_offset_moves_it_from_the_entity_centre() {
    let mut physics = Physics::new();
    let e = entity(1);
    add(
      &mut physics,
      e,
      100.0,
      100.0,
      None,
      RigidBody::new(),
      Collider::circle_collider(20.0).with_offset(32.0, -8.0),
    );
    assert!((collider_center(&physics, e) - Vec2::new(132.0, 92.0)).length() < 1e-3);
  }

  #[test]
  fn a_grouped_collider_spans_its_offset_and_size() {
    let mut physics = Physics::new();
    let e = entity(1);
    let grouped = GroupedCollider::new(40.0, 20.0).with_offset(-20.0, 0.0);
    physics.register_grouped_entity(
      e,
      &Transform::from_xy(100.0, 100.0),
      None,
      &RigidBody::kinematic(),
      &grouped,
    );
    assert!((collider_center(&physics, e) - Vec2::new(100.0, 110.0)).length() < 1e-3);
  }
}
