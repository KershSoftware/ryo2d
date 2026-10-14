//! Rapier2D physics integration
//!
//! This module wraps Rapier2D to provide physics simulation with collision response.
//! It maps ECS entities to Rapier bodies and colliders; the physics systems
//! (`sync.rs`) register them, push game changes in, step, and read the
//! simulated bodies back.

use std::collections::HashMap;
use std::mem::discriminant;
use std::sync::{Mutex, PoisonError};

use glam::Mat2;
use rapier2d::prelude::{
  ActiveCollisionTypes, ActiveEvents, BroadPhaseBvh, CCDSolver, CoefficientCombineRule,
  ColliderBuilder, ColliderHandle, ColliderSet, CollisionEvent, ContactForceEvent, ContactPair,
  EventHandler, Group, ImpulseJointHandle, ImpulseJointSet, IntegrationParameters,
  InteractionGroups, InteractionTestMode, IslandManager, MultibodyJointSet, NarrowPhase,
  PhysicsPipeline, Pose, Real, RigidBodyActivation, RigidBodyBuilder, RigidBodyHandle,
  RigidBodySet, RigidBodyType, SoftBodySet, SoftBodyTearEvent,
};

use super::body::{BodyKind, RigidBody};
use super::collider::Collider;
use super::config::PhysicsConfig;
use super::convert::{from_rapier, pose, poses_differ, to_rapier};
use super::joint::{Joint, generic_joint, has_invalid_drive};
use super::shape;
use crate::core::ecs::{Entity, EntityHashMap, EntityHashSet, Resource, Tick, Vec2};

/// A collision event with its first contact, captured during the step.
struct Captured {
  event: CollisionEvent,
  contact: Option<(Vec2, Vec2)>,
}

/// Collects Rapier's events during a step; drained right after it.
#[derive(Default)]
struct EventCollector {
  collisions: Mutex<Vec<Captured>>,
  forces: Mutex<Vec<ContactForceEvent>>,
}

impl EventHandler for EventCollector {
  fn handle_collision_event(
    &self,
    _bodies: &RigidBodySet,
    colliders: &ColliderSet,
    event: CollisionEvent,
    contact_pair: Option<&ContactPair>,
  ) {
    let contact = contact_pair
      .filter(|_| event.started() && !event.sensor())
      .and_then(|pair| first_contact(colliders, pair, event.collider1()));
    self
      .collisions
      .lock()
      .unwrap_or_else(PoisonError::into_inner)
      .push(Captured { event, contact });
  }

  fn handle_contact_force_event(
    &self,
    dt: Real,
    _bodies: &RigidBodySet,
    _colliders: &ColliderSet,
    contact_pair: &ContactPair,
    total_force_magnitude: Real,
  ) {
    self
      .forces
      .lock()
      .unwrap_or_else(PoisonError::into_inner)
      .push(ContactForceEvent::from_contact_pair(
        dt,
        contact_pair,
        total_force_magnitude,
      ));
  }

  fn handle_soft_body_tear_event(&self, _soft_bodies: &SoftBodySet, _event: &SoftBodyTearEvent) {}
}

/// The deepest contact of `pair` as a world point, with the contact normal
/// pointing away from collider `from`. Rapier picks which collider of a pair
/// is first; its manifold normal points from `collider1` to `collider2`.
pub(crate) fn first_contact(
  colliders: &ColliderSet,
  pair: &ContactPair,
  from: ColliderHandle,
) -> Option<(Vec2, Vec2)> {
  let (manifold, contact) = pair.find_deepest_contact()?;
  let collider1 = colliders.get(pair.collider1)?;
  let point = from_rapier(*collider1.position() * contact.local_p1);
  let normal = from_rapier(manifold.data.normal);
  Some((
    point,
    if pair.collider1 == from {
      normal
    } else {
      -normal
    },
  ))
}

/// Where a collider sits and how its shape is mapped: relative to its body
/// when it has one, in the world otherwise.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Placement {
  pub(crate) translation: Vec2,
  pub(crate) rotation: f32,
  /// The linear map from the shape's own space into this frame (scale,
  /// mirroring, skew)
  pub(crate) map: Mat2,
}

impl Placement {
  fn is_finite(&self) -> bool {
    self.translation.is_finite() && self.rotation.is_finite() && self.map.is_finite()
  }

  fn pose(&self) -> Pose {
    pose(self.translation, self.rotation)
  }
}

/// Whether two shape maps differ enough to rebuild the shape.
fn maps_differ(a: Mat2, b: Mat2) -> bool {
  let tolerance = 1e-4 * a.abs().to_cols_array().into_iter().fold(1.0, f32::max);
  !a.abs_diff_eq(b, tolerance)
}

/// A collider's entity and its body's entity (its own when it has no body).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Owner {
  pub(crate) collider: Entity,
  pub(crate) body: Entity,
  /// Whether the collider is a trigger
  pub(crate) trigger: bool,
}

/// A body (or bare collider) entering or leaving a trigger in the last step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TriggerEvent {
  pub(crate) trigger: Entity,
  pub(crate) other: Entity,
  pub(crate) other_body: Entity,
  pub(crate) entered: bool,
}

pub(crate) struct BodyEntry {
  pub(crate) handle: RigidBodyHandle,
  /// The settings last applied, compared field by field on edits
  pub(crate) settings: RigidBody,
  /// Asleep after the last step
  asleep: bool,
  /// Fell asleep during the last step: Rapier zeroed its velocities, which
  /// the write-back passes on once
  fell_asleep: bool,
}

pub(crate) struct ColliderEntry {
  pub(crate) handle: ColliderHandle,
  /// The body it is attached to; `None` for a fixed collider on its own
  pub(crate) body: Option<Entity>,
  /// The component last applied
  pub(crate) collider: Collider,
  /// The pose last applied: relative to the body when attached, else world.
  /// Updated only when applied, so drift below the tolerance never adds up
  pub(crate) pose: (Vec2, f32),
  /// The shape map last applied
  pub(crate) map: Mat2,
}

pub(crate) struct JointEntry {
  /// `None` while inert (a body is missing)
  pub(crate) handle: Option<ImpulseJointHandle>,
  pub(crate) joint: Joint,
  /// The bodies' world poses when it was made: anchors stay where they were put
  pub(crate) poses: ((Vec2, f32), (Vec2, f32)),
  /// Whether its entity is enabled: a joint made or remade while it is
  /// disabled starts disabled
  enabled: bool,
}

/// A body the step moved, for the write-back.
pub(crate) struct BodyState {
  pub(crate) entity: Entity,
  pub(crate) translation: Vec2,
  pub(crate) rotation: f32,
  pub(crate) linvel: Vec2,
  pub(crate) angvel: f32,
  /// Dynamic bodies write their velocities back too
  pub(crate) dynamic: bool,
}

/// What the write-back gives a body, worked out before any of it lands.
pub(crate) struct WriteBack {
  pub(crate) entity: Entity,
  /// Its local translation and continuous rotation; `None` leaves its
  /// `Transform` as it is
  pub(crate) local: Option<(Vec2, f32)>,
  /// A dynamic body's velocities
  pub(crate) velocities: Option<(Vec2, f32)>,
}

/// A contact that started or stopped during the last step.
pub(crate) struct RawCollision {
  pub(crate) a: Owner,
  pub(crate) b: Owner,
  pub(crate) started: bool,
  /// World point and normal (pointing from `a` to `b`) of the first contact
  pub(crate) contact: Option<(Vec2, Vec2)>,
}

/// A contact force above a collider's threshold during the last step.
pub(crate) struct RawForce {
  pub(crate) a: Owner,
  pub(crate) b: Owner,
  pub(crate) force: f32,
  pub(crate) direction: Vec2,
  pub(crate) started: bool,
}

/// A push queued for the next step.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Push {
  Impulse(Vec2),
  ImpulseAt(Vec2, Vec2),
  AngularImpulse(f32),
  Force(Vec2),
  ForceAt(Vec2, Vec2),
  Torque(f32),
}

impl Push {
  fn is_finite(self) -> bool {
    match self {
      Push::Impulse(v) | Push::Force(v) => v.is_finite(),
      Push::ImpulseAt(v, p) | Push::ForceAt(v, p) => v.is_finite() && p.is_finite(),
      Push::AngularImpulse(t) | Push::Torque(t) => t.is_finite(),
    }
  }
}

/// The simulation. Read and steer it with `ctx.physics()` and
/// `ctx.physics_mut()` (`Res<Physics>`/`ResMut<Physics>` in systems).
#[derive(Resource)]
pub struct Physics {
  pub(crate) bodies: RigidBodySet,
  pub(crate) colliders: ColliderSet,
  pub(crate) joints: ImpulseJointSet,
  pub(crate) multibody_joints: MultibodyJointSet,
  pub(crate) soft_bodies: SoftBodySet,
  pub(crate) islands: IslandManager,
  pub(crate) broad_phase: BroadPhaseBvh,
  pub(crate) narrow_phase: NarrowPhase,
  ccd_solver: CCDSolver,
  pipeline: PhysicsPipeline,
  pub(crate) params: IntegrationParameters,
  gravity: Vec2,
  collector: EventCollector,
  pub(crate) body_entries: EntityHashMap<BodyEntry>,
  pub(crate) collider_entries: EntityHashMap<ColliderEntry>,
  pub(crate) owners: HashMap<ColliderHandle, Owner>,
  pub(crate) joint_entries: EntityHashMap<JointEntry>,
  joint_of: HashMap<ImpulseJointHandle, Entity>,
  /// Joint entities re-enabled since the last step: `sync_joints` brings
  /// them up to date (or makes them) once bodies are registered and moved
  pub(crate) resync_joints: EntityHashSet,
  /// Colliders whose body is not simulated yet; retried every step
  pub(crate) waiting: EntityHashSet,
  /// Bodies refused for a non-finite pose; retried every step
  pub(crate) refused: EntityHashSet,
  /// Entities that lost their `RigidBody` or their parent since the last
  /// step: the colliders at and below them may belong to another body now
  pub(crate) relink: EntityHashSet,
  /// `sync_colliders`' work list and walk stack, kept between steps so the
  /// hot path does not allocate
  pub(crate) sync_queue: Vec<Entity>,
  pub(crate) walk_stack: Vec<(Entity, usize)>,
  /// `read_back`'s work list, kept for the same reason
  pub(crate) write_backs: Vec<WriteBack>,
  /// Owners of colliders removed since the last step: Rapier reports their
  /// `Stopped` events during the next one
  departed: HashMap<ColliderHandle, Owner>,
  /// The change tick `read_back` last ran at. A `Transform`, `Velocity` or
  /// `AngularVelocity` last changed at this tick holds what the last step
  /// left in Rapier, unless the body is in `recheck`: `push_changes` and
  /// `sync_colliders` skip a root body and its own collider in that state
  /// without comparing (`push_changes` has the rules)
  pub(crate) write_tick: Option<Tick>,
  /// The change tick `track_disabled` last ran at: in the step's chain, just
  /// before its propagation. A `Transform` changed after it is not in the
  /// step's world poses
  pub(crate) chain_tick: Option<Tick>,
  /// Bodies the next push and collider sync compare by value whatever their
  /// tick: bodies that lost their parent (their local `Transform` became
  /// their world pose), and bodies whose `Transform` the game changed after
  /// the step's propagation and the write-back then stamped with its tick
  /// (their scale may be new). Cleared by `push_changes`
  pub(crate) recheck: EntityHashSet,
  /// Bodies and colliders the sync compared with Rapier, for tests
  #[cfg(test)]
  pub(crate) compared: usize,
  /// Bodies the game moved through `Transform` this step
  teleported: EntityHashSet,
  /// Kinematic bodies the engine moves by velocity this step
  driven: EntityHashSet,
  warned: EntityHashSet,
  /// Warnings logged, for tests
  #[cfg(test)]
  pub(crate) warnings: usize,
  /// Forces and impulses waiting for the next step
  queued: Vec<(Entity, Push)>,
  /// Bodies holding one-step forces, cleared after the step
  forced: Vec<RigidBodyHandle>,
  /// The last step's started and stopped contacts
  pub(crate) collisions: Vec<RawCollision>,
  /// Per (trigger collider, body inside it), how many of that body's
  /// colliders overlap the trigger: entered at 1, exited at 0
  trigger_overlaps: HashMap<(Entity, Entity), u32>,
  /// What each trigger holds now, in entry order (`TriggerContents`)
  pub(crate) trigger_contents: EntityHashMap<Vec<Entity>>,
  /// Triggers whose contents changed in the last step
  pub(crate) changed_triggers: EntityHashSet,
  /// The last step's trigger entries and exits
  pub(crate) trigger_events: Vec<TriggerEvent>,
  /// The last step's contact forces
  pub(crate) forces: Vec<RawForce>,
  /// Joints the last step broke: (joint, a, b)
  pub(crate) broken: Vec<(Entity, Entity, Entity)>,
}

fn body_type(kind: BodyKind) -> RigidBodyType {
  match kind {
    BodyKind::Dynamic => RigidBodyType::Dynamic,
    BodyKind::Fixed => RigidBodyType::Fixed,
    BodyKind::Kinematic => RigidBodyType::KinematicPositionBased,
  }
}

fn finite_or(value: f32, fallback: f32) -> f32 {
  if value.is_finite() { value } else { fallback }
}

fn body_builder(settings: &RigidBody) -> RigidBodyBuilder {
  let builder = RigidBodyBuilder::new(body_type(settings.kind))
    .gravity_scale(finite_or(settings.gravity_scale, 1.0))
    .linear_damping(finite_or(settings.linear_damping, 0.0).max(0.0))
    .angular_damping(finite_or(settings.angular_damping, 0.0).max(0.0))
    .ccd_enabled(settings.ccd)
    .can_sleep(settings.can_sleep)
    .dominance_group(settings.dominance);
  if settings.lock_rotation {
    builder.lock_rotations()
  } else {
    builder
  }
}

fn groups(collider: &Collider) -> InteractionGroups {
  InteractionGroups::new(
    Group::from(collider.layer),
    Group::from(collider.mask),
    InteractionTestMode::And,
  )
}

fn events(collider: &Collider) -> ActiveEvents {
  if collider.force_threshold.is_some() {
    ActiveEvents::COLLISION_EVENTS | ActiveEvents::CONTACT_FORCE_EVENTS
  } else {
    ActiveEvents::COLLISION_EVENTS
  }
}

fn active_types() -> ActiveCollisionTypes {
  ActiveCollisionTypes::all().difference(ActiveCollisionTypes::FIXED_FIXED)
}

/// Apply the material half of an edited `Collider` (everything but its
/// shape and placement): only fields that changed are written.
fn apply_material(
  rapier: &mut rapier2d::prelude::Collider,
  old: &Collider,
  new: &Collider,
  density: f32,
) {
  if old.friction != new.friction {
    rapier.set_friction(finite_or(new.friction, 0.5).max(0.0));
  }
  if old.restitution != new.restitution {
    rapier.set_restitution(finite_or(new.restitution, 0.0).max(0.0));
  }
  if old.density != new.density || old.mass != new.mass {
    match new.mass {
      Some(mass) => rapier.set_mass(finite_or(mass, 0.0).max(0.0)),
      None => rapier.set_density(density),
    }
  }
  if old.layer != new.layer || old.mask != new.mask {
    rapier.set_collision_groups(groups(new));
  }
  if old.is_trigger != new.is_trigger {
    rapier.set_sensor(new.is_trigger);
  }
  if old.force_threshold != new.force_threshold {
    rapier.set_active_events(events(new));
    rapier.set_contact_force_event_threshold(finite_or(new.force_threshold.unwrap_or(0.0), 0.0));
  }
}

impl Physics {
  /// An empty simulation at the default scale and gravity.
  pub fn new() -> Self {
    Self {
      bodies: RigidBodySet::new(),
      colliders: ColliderSet::new(),
      joints: ImpulseJointSet::new(),
      multibody_joints: MultibodyJointSet::new(),
      soft_bodies: SoftBodySet::new(),
      islands: IslandManager::new(),
      broad_phase: BroadPhaseBvh::new(),
      narrow_phase: NarrowPhase::new(),
      ccd_solver: CCDSolver::new(),
      pipeline: PhysicsPipeline::new(),
      params: IntegrationParameters {
        length_unit: PhysicsConfig::DEFAULT_PIXELS_PER_METER,
        ..IntegrationParameters::default()
      },
      gravity: Vec2::new(0.0, 980.0),
      collector: EventCollector::default(),
      body_entries: EntityHashMap::default(),
      collider_entries: EntityHashMap::default(),
      owners: HashMap::new(),
      trigger_overlaps: HashMap::new(),
      trigger_contents: EntityHashMap::default(),
      changed_triggers: EntityHashSet::default(),
      trigger_events: Vec::new(),
      joint_entries: EntityHashMap::default(),
      joint_of: HashMap::new(),
      resync_joints: EntityHashSet::default(),
      waiting: EntityHashSet::default(),
      refused: EntityHashSet::default(),
      relink: EntityHashSet::default(),
      sync_queue: Vec::new(),
      walk_stack: Vec::new(),
      write_backs: Vec::new(),
      departed: HashMap::new(),
      write_tick: None,
      chain_tick: None,
      recheck: EntityHashSet::default(),
      #[cfg(test)]
      compared: 0,
      teleported: EntityHashSet::default(),
      driven: EntityHashSet::default(),
      warned: EntityHashSet::default(),
      #[cfg(test)]
      warnings: 0,
      queued: Vec::new(),
      forced: Vec::new(),
      collisions: Vec::new(),
      forces: Vec::new(),
      broken: Vec::new(),
    }
  }

  /// Take the unit, solver iterations and gravity from the config.
  pub(crate) fn configure(&mut self, config: &PhysicsConfig) {
    self.params.length_unit = config.length_unit();
    self.params.num_solver_iterations = config.iterations();
    self.gravity = if config.gravity.is_finite() {
      config.gravity
    } else {
      Vec2::ZERO
    };
  }

  /// Rapier's density for `density` per square metre.
  pub(crate) fn rapier_density(&self, density: f32) -> f32 {
    let unit = self.params.length_unit;
    density / (unit * unit)
  }

  /// Forget the warnings logged for an entity that has no body, collider
  /// or joint left (despawned included). Warnings are once per entity while
  /// it keeps them: taking a collider out of the simulation and putting it
  /// back (a zero scale, say) does not log again.
  pub(crate) fn forget_warnings(&mut self, entity: Entity) {
    self.warned.remove(&entity);
  }

  pub(crate) fn warn_once(&mut self, entity: Entity, what: &str) {
    if self.warned.insert(entity) {
      #[cfg(test)]
      {
        self.warnings += 1;
      }
      crate::engine_warn!(module: "Physics", "{entity}: {what}");
    }
  }

  pub(crate) fn add_body(
    &mut self,
    entity: Entity,
    translation: Vec2,
    rotation: f32,
    linvel: Vec2,
    angvel: f32,
    settings: &RigidBody,
  ) {
    if self.body_entries.contains_key(&entity) {
      return;
    }
    if !(translation.is_finite() && rotation.is_finite()) {
      self.warn_once(
        entity,
        "a body with a non-finite Transform is not simulated until it is finite",
      );
      self.refused.insert(entity);
      return;
    }
    let linvel = if linvel.is_finite() {
      linvel
    } else {
      Vec2::ZERO
    };
    let handle = self.bodies.insert(
      body_builder(settings)
        .pose(pose(translation, rotation))
        .linvel(to_rapier(linvel))
        .angvel(finite_or(angvel, 0.0)),
    );
    self.body_entries.insert(
      entity,
      BodyEntry {
        handle,
        settings: settings.clone(),
        asleep: false,
        fell_asleep: false,
      },
    );
  }

  /// Apply an edited `RigidBody` in place: only fields that changed are written.
  pub(crate) fn update_body(&mut self, entity: Entity, settings: &RigidBody) {
    let Some(entry) = self.body_entries.get_mut(&entity) else {
      return;
    };
    let Some(body) = self.bodies.get_mut(entry.handle) else {
      return;
    };
    let old = &entry.settings;
    if old.kind != settings.kind {
      body.set_body_type(body_type(settings.kind), true);
    }
    if old.gravity_scale != settings.gravity_scale {
      body.set_gravity_scale(finite_or(settings.gravity_scale, 1.0), true);
    }
    if old.linear_damping != settings.linear_damping {
      body.set_linear_damping(finite_or(settings.linear_damping, 0.0).max(0.0));
    }
    if old.angular_damping != settings.angular_damping {
      body.set_angular_damping(finite_or(settings.angular_damping, 0.0).max(0.0));
    }
    if old.lock_rotation != settings.lock_rotation {
      body.lock_rotations(settings.lock_rotation, true);
    }
    if old.ccd != settings.ccd {
      body.enable_ccd(settings.ccd);
    }
    if old.can_sleep != settings.can_sleep {
      *body.activation_mut() = if settings.can_sleep {
        RigidBodyActivation::active()
      } else {
        RigidBodyActivation::cannot_sleep()
      };
    }
    if old.dominance != settings.dominance {
      body.set_dominance_group(settings.dominance);
    }
    entry.settings = settings.clone();
  }

  /// Remove a body. Its colliders stay, parentless: fixed colliders until
  /// they join another body. Its joints go inert (Rapier removes them with
  /// the body) until both their bodies are simulated again.
  pub(crate) fn remove_body(&mut self, entity: Entity) {
    self.refused.remove(&entity);
    let Some(entry) = self.body_entries.remove(&entity) else {
      return;
    };
    let attached: Vec<ColliderHandle> = self
      .bodies
      .get(entry.handle)
      .map(|body| body.colliders().to_vec())
      .unwrap_or_default();
    let joints: Vec<ImpulseJointHandle> = self
      .joints
      .attached_joints(entry.handle)
      .map(|(_, _, handle, _)| handle)
      .collect();
    for handle in joints {
      if let Some(joint_entity) = self.joint_of.remove(&handle)
        && let Some(joint) = self.joint_entries.get_mut(&joint_entity)
      {
        joint.handle = None;
      }
    }
    self.bodies.remove(
      entry.handle,
      &mut self.islands,
      &mut self.colliders,
      &mut self.joints,
      &mut self.multibody_joints,
      &mut self.soft_bodies,
      false,
    );
    for handle in attached {
      let Some(owner) = self.owners.get_mut(&handle) else {
        continue;
      };
      owner.body = owner.collider;
      let world = self.colliders.get(handle).map(|collider| {
        (
          from_rapier(collider.translation()),
          collider.rotation().angle(),
        )
      });
      if let Some(entry) = self.collider_entries.get_mut(&owner.collider) {
        entry.body = None;
        // Where it stays: its pose is in the world now
        if let Some(world) = world {
          entry.pose = world;
        }
      }
    }
  }

  fn add_collider(
    &mut self,
    entity: Entity,
    body: Option<(Entity, RigidBodyHandle)>,
    placement: Placement,
    collider: &Collider,
  ) {
    if !placement.is_finite() {
      self.warn_once(
        entity,
        "a collider with a non-finite transform is not simulated",
      );
      return;
    }
    let Some(builder) = self.collider_builder(collider, placement.map) else {
      self.warn_once(
        entity,
        "a degenerate collider shape (zero scale included) is not simulated",
      );
      return;
    };
    let builder = builder.position(placement.pose());
    let handle = match body {
      Some((_, parent)) => self
        .colliders
        .insert_with_parent(builder, parent, &mut self.bodies),
      None => self.colliders.insert(builder),
    };
    let body = body.map(|(body, _)| body);
    self.collider_entries.insert(
      entity,
      ColliderEntry {
        handle,
        body,
        collider: collider.clone(),
        pose: (placement.translation, placement.rotation),
        map: placement.map,
      },
    );
    self.owners.insert(
      handle,
      Owner {
        collider: entity,
        body: body.unwrap_or(entity),
        trigger: collider.is_trigger,
      },
    );
  }

  /// Rapier's collider for `collider` with `map` applied to its shape, or
  /// `None` when the shape is degenerate.
  fn collider_builder(&self, collider: &Collider, map: Mat2) -> Option<ColliderBuilder> {
    let shape = shape::build(&collider.shape, map)?;
    let builder = ColliderBuilder::new(shape)
      .friction(finite_or(collider.friction, 0.5).max(0.0))
      .friction_combine_rule(CoefficientCombineRule::GeometricMean)
      .restitution(finite_or(collider.restitution, 0.0).max(0.0))
      .restitution_combine_rule(CoefficientCombineRule::Max)
      .sensor(collider.is_trigger)
      .collision_groups(groups(collider))
      .active_collision_types(active_types())
      .active_events(events(collider))
      .contact_force_event_threshold(finite_or(collider.force_threshold.unwrap_or(0.0), 0.0));
    Some(match collider.mass {
      Some(mass) => builder.mass(finite_or(mass, 0.0).max(0.0)),
      None => builder.density(self.rapier_density(finite_or(collider.density, 1.0).max(0.0))),
    })
  }

  /// Bring a collider in line with its component, its body and its
  /// placement: register it, move it to another body (or none), rebuild its
  /// shape when the shape or scale changed, move it, and apply material
  /// edits. A collider whose body is not simulated yet waits out of the
  /// simulation (retried every step), leaving any body it was on; a shape
  /// that becomes degenerate leaves the simulation until it is valid again.
  pub(crate) fn sync_collider(
    &mut self,
    entity: Entity,
    body: Option<Entity>,
    placement: Placement,
    collider: &Collider,
  ) {
    let parent = match body {
      Some(body) => match self.body_entries.get(&body) {
        Some(entry) => Some((body, entry.handle)),
        None => {
          self.remove_collider(entity);
          self.waiting.insert(entity);
          return;
        }
      },
      None => None,
    };
    self.waiting.remove(&entity);
    let Some(entry) = self.collider_entries.get(&entity) else {
      self.add_collider(entity, parent, placement, collider);
      if let Some(entry) = self.collider_entries.get(&entity) {
        self.follow_body(entry.handle);
      }
      return;
    };
    let moved = poses_differ(entry.pose, (placement.translation, placement.rotation));
    let reshape = entry.collider.shape != collider.shape || maps_differ(entry.map, placement.map);
    let reparent = entry.body != body;
    let follows = self.follows_body(entry.handle);
    if !moved && !reshape && !reparent && follows && entry.collider == *collider {
      return;
    }
    if !placement.is_finite() {
      self.warn_once(
        entity,
        "a collider with a non-finite transform is not moved",
      );
      return;
    }
    let (handle, old) = (entry.handle, entry.collider.clone());
    if reshape {
      let Some(shape) = shape::build(&collider.shape, placement.map) else {
        self.remove_collider(entity);
        self.warn_once(
          entity,
          "a degenerate collider shape (zero scale included) is not simulated",
        );
        return;
      };
      if let Some(rapier) = self.colliders.get_mut(handle) {
        rapier.set_shape(shape);
      }
    }
    if reparent {
      self
        .colliders
        .set_parent(handle, parent.map(|(_, handle)| handle), &mut self.bodies);
      if let Some(owner) = self.owners.get_mut(&handle) {
        owner.body = body.unwrap_or(entity);
      }
    }
    if old.is_trigger != collider.is_trigger
      && let Some(owner) = self.owners.get_mut(&handle)
    {
      owner.trigger = collider.is_trigger;
    }
    let density = self.rapier_density(finite_or(collider.density, 1.0).max(0.0));
    if let Some(rapier) = self.colliders.get_mut(handle) {
      if moved || reparent {
        if parent.is_some() {
          rapier.set_position_wrt_parent(placement.pose());
        } else {
          rapier.set_position(placement.pose());
        }
      }
      apply_material(rapier, &old, collider, density);
    }
    if let Some(entry) = self.collider_entries.get_mut(&entity) {
      entry.body = body;
      entry.collider = collider.clone();
      if moved || reparent {
        entry.pose = (placement.translation, placement.rotation);
      }
      if reshape {
        entry.map = placement.map;
      }
    }
    self.follow_body(handle);
  }

  /// Whether a collider is enabled exactly when its body is (always, with
  /// no body).
  fn follows_body(&self, handle: ColliderHandle) -> bool {
    self
      .colliders
      .get(handle)
      .is_none_or(|collider| collider.is_enabled() == self.parent_enabled(collider))
  }

  /// Whether a collider's body is enabled (true without one).
  fn parent_enabled(&self, collider: &rapier2d::prelude::Collider) -> bool {
    collider
      .parent()
      .and_then(|parent| self.bodies.get(parent))
      .is_none_or(|body| body.is_enabled())
  }

  /// Rapier passes a body's enabled state to the colliders on it when the
  /// state changes, not to colliders attached or taken off later: bring a
  /// collider whose own entity is enabled in line with its body.
  fn follow_body(&mut self, handle: ColliderHandle) {
    let Some(collider) = self.colliders.get(handle) else {
      return;
    };
    let (enabled, parent) = (collider.is_enabled(), collider.parent());
    match (enabled, self.parent_enabled(collider)) {
      // Attached to a disabled body: toggling the body has Rapier disable
      // its colliders, as for any body turned off
      (true, false) => {
        if let Some(body) = parent.and_then(|parent| self.bodies.get_mut(parent)) {
          body.set_enabled(true);
          body.set_enabled(false);
        }
      }
      // Left disabled by a disabled body it was taken off
      (false, true) => {
        if let Some(collider) = self.colliders.get_mut(handle) {
          collider.set_enabled(false);
          collider.set_enabled(true);
        }
      }
      _ => {}
    }
  }

  /// A collider's world pose, for tests.
  #[cfg(test)]
  pub(crate) fn collider_pose(&self, entity: Entity) -> Option<(Vec2, f32)> {
    let collider = self
      .colliders
      .get(self.collider_entries.get(&entity)?.handle)?;
    Some((
      from_rapier(collider.translation()),
      collider.rotation().angle(),
    ))
  }

  pub(crate) fn remove_collider(&mut self, entity: Entity) {
    self.trigger_contents.remove(&entity);
    let Some(entry) = self.collider_entries.remove(&entity) else {
      return;
    };
    if let Some(owner) = self.owners.remove(&entry.handle) {
      self.departed.insert(entry.handle, owner);
    }
    self.colliders.remove(
      entry.handle,
      &mut self.islands,
      &mut self.bodies,
      &mut self.soft_bodies,
      true,
    );
  }

  /// Count a trigger overlap starting or stopping, once per body: the body
  /// enters at its first collider inside and exits when its last one leaves.
  fn record_trigger(&mut self, a: Owner, b: Owner, started: bool) {
    for (trigger, other) in [(a, b), (b, a)] {
      if !trigger.trigger {
        continue;
      }
      let key = (trigger.collider, other.body);
      let count = self.trigger_overlaps.entry(key).or_insert(0);
      let changed = if started {
        *count += 1;
        *count == 1
      } else {
        *count = count.saturating_sub(1);
        *count == 0
      };
      if !started && *count == 0 {
        self.trigger_overlaps.remove(&key);
      }
      if !changed {
        continue;
      }
      let contents = self.trigger_contents.entry(trigger.collider).or_default();
      if started {
        contents.push(other.body);
      } else {
        contents.retain(|&inside| inside != other.body);
      }
      self.changed_triggers.insert(trigger.collider);
      self.trigger_events.push(TriggerEvent {
        trigger: trigger.collider,
        other: other.collider,
        other_body: other.body,
        entered: started,
      });
    }
  }

  /// Disable or re-enable an entity's body, collider and joint in Rapier;
  /// their velocities, contacts and joints survive.
  pub(crate) fn set_enabled(&mut self, entity: Entity, enabled: bool) {
    if let Some(entry) = self.body_entries.get(&entity)
      && let Some(body) = self.bodies.get_mut(entry.handle)
    {
      body.set_enabled(enabled);
    }
    if let Some(entry) = self.collider_entries.get(&entity)
      && let Some(collider) = self.colliders.get_mut(entry.handle)
    {
      collider.set_enabled(enabled);
    }
    self.set_joint_enabled(entity, enabled);
  }

  /// Disable or re-enable the joint on an entity; an inert one is made
  /// that way when its bodies come.
  pub(crate) fn set_joint_enabled(&mut self, entity: Entity, enabled: bool) {
    if let Some(entry) = self.joint_entries.get_mut(&entity) {
      entry.enabled = enabled;
      if let Some(handle) = entry.handle
        && let Some(joint) = self.joints.get_mut(handle, true)
      {
        joint.data.set_enabled(enabled);
      }
    }
  }

  /// Register a new or edited joint. A new break force is only recorded
  /// (Rapier's joint and its bodies' sleep are untouched); motors, limits,
  /// collisions and the kind's own values (a winch's rope length) change in
  /// place, keeping the anchors where they were put; new bodies, a new kind
  /// or new anchors remake it. An inert joint just records the edit:
  /// `retry_inert_joints` makes it once its bodies are simulated.
  pub(crate) fn sync_joint(&mut self, entity: Entity, joint: &Joint) {
    if has_invalid_drive(joint) {
      self.warn_once(entity, "a joint's non-finite limits or motor are ignored");
    }
    let Some(entry) = self.joint_entries.get_mut(&entity) else {
      self.insert_joint(entity, joint, true);
      return;
    };
    if entry.joint == *joint {
      return;
    }
    let Some(handle) = entry.handle else {
      entry.joint = joint.clone();
      return;
    };
    let mut kept = entry.joint.clone();
    kept.break_force = joint.break_force;
    if kept == *joint {
      entry.joint = joint.clone();
      return;
    }
    let same_frame = entry.joint.a == joint.a
      && entry.joint.b == joint.b
      && entry.joint.anchor == joint.anchor
      && discriminant(&entry.joint.kind) == discriminant(&joint.kind);
    let enabled = entry.enabled;
    if same_frame
      && let Some(mut data) = generic_joint(joint, entry.poses.0, entry.poses.1)
      && let Some(rapier) = self.joints.get_mut(handle, true)
    {
      data.set_enabled(enabled);
      rapier.data = data;
      entry.joint = joint.clone();
      return;
    }
    // Remade: a warning already logged for it stays logged
    self.remove_joint(entity);
    self.insert_joint(entity, joint, enabled);
  }

  fn insert_joint(&mut self, entity: Entity, joint: &Joint, enabled: bool) {
    let made = (joint.a != joint.b)
      .then(|| {
        let a = self.body_entries.get(&joint.a)?.handle;
        let b = self.body_entries.get(&joint.b)?.handle;
        let poses = (
          self.body_world_pose(joint.a)?,
          self.body_world_pose(joint.b)?,
        );
        Some((a, b, poses, generic_joint(joint, poses.0, poses.1)?))
      })
      .flatten();
    let Some((a, b, poses, mut data)) = made else {
      self.warn_once(
        entity,
        "a joint needs two different simulated bodies and sane values; it waits",
      );
      self.joint_entries.insert(
        entity,
        JointEntry {
          handle: None,
          joint: joint.clone(),
          poses: ((Vec2::ZERO, 0.0), (Vec2::ZERO, 0.0)),
          enabled,
        },
      );
      return;
    };
    data.set_enabled(enabled);
    let handle = self.joints.insert(a, b, data, true);
    self.joint_of.insert(handle, entity);
    self.joint_entries.insert(
      entity,
      JointEntry {
        handle: Some(handle),
        joint: joint.clone(),
        poses,
        enabled,
      },
    );
  }

  /// Try inert joints again: their bodies may be simulated now.
  pub(crate) fn retry_inert_joints(&mut self) {
    let ready: Vec<(Entity, Joint, bool)> = self
      .joint_entries
      .iter()
      .filter(|(_, entry)| {
        entry.handle.is_none() && self.has_body(entry.joint.a) && self.has_body(entry.joint.b)
      })
      .map(|(&entity, entry)| (entity, entry.joint.clone(), entry.enabled))
      .collect();
    for (entity, joint, enabled) in ready {
      self.joint_entries.remove(&entity);
      self.insert_joint(entity, &joint, enabled);
    }
  }

  /// Take a joint out of Rapier and forget it.
  pub(crate) fn remove_joint(&mut self, entity: Entity) {
    if let Some(entry) = self.joint_entries.remove(&entity)
      && let Some(handle) = entry.handle
    {
      self.joint_of.remove(&handle);
      self.joints.remove(handle, true);
    }
  }

  /// Joints simulated now (inert ones left out), for tests.
  #[cfg(test)]
  pub(crate) fn joint_count(&self) -> usize {
    self
      .joint_entries
      .values()
      .filter(|entry| entry.handle.is_some())
      .count()
  }

  fn has_body(&self, entity: Entity) -> bool {
    self.body_entries.contains_key(&entity)
  }

  /// Where Rapier holds a registered body.
  fn body_world_pose(&self, entity: Entity) -> Option<(Vec2, f32)> {
    let body = self.bodies.get(self.body_entries.get(&entity)?.handle)?;
    Some((from_rapier(body.translation()), body.rotation().angle()))
  }

  /// Whether `linvel`/`angvel` are what the simulation gave the entity's
  /// dynamic body (the write-back, zero once it sleeps), not the game's input.
  pub(crate) fn is_simulated_velocity(&self, entity: Entity, linvel: Vec2, angvel: f32) -> bool {
    let Some(body) = self
      .body_entries
      .get(&entity)
      .and_then(|entry| self.bodies.get(entry.handle))
    else {
      return false;
    };
    body.is_dynamic() && from_rapier(body.linvel()) == linvel && body.angvel() == angvel
  }

  /// Start pushing a step's changes.
  pub(crate) fn begin_push(&mut self) {
    self.teleported.clear();
    self.driven.clear();
  }

  /// Teleport a body of any kind to the pose, if that differs from Rapier's
  /// (a re-enabled body does not travel to where it was put while disabled).
  pub(crate) fn place_body(&mut self, entity: Entity, translation: Vec2, rotation: f32) {
    if !(translation.is_finite() && rotation.is_finite()) {
      self.warn_once(entity, "a non-finite Transform is not pushed to physics");
      return;
    }
    let Some(handle) = self.handle_if_moved(entity, translation, rotation) else {
      return;
    };
    if let Some(body) = self.bodies.get_mut(handle) {
      body.set_position(pose(translation, rotation), true);
    }
  }

  /// A body's handle when the pose differs from Rapier's. Read through
  /// `get`: `get_mut` marks a body modified, and Rapier then reprocesses it
  /// in the next step even when nothing was written.
  fn handle_if_moved(
    &self,
    entity: Entity,
    translation: Vec2,
    rotation: f32,
  ) -> Option<RigidBodyHandle> {
    let handle = self.body_entries.get(&entity)?.handle;
    let body = self.bodies.get(handle)?;
    let current = (from_rapier(body.translation()), body.rotation().angle());
    poses_differ(current, (translation, rotation)).then_some(handle)
  }

  /// Put a body where the game moved it, if that differs from Rapier's pose:
  /// a kinematic body travels there over the step, any other body teleports
  /// and wakes.
  pub(crate) fn move_body(&mut self, entity: Entity, translation: Vec2, rotation: f32) {
    let Some(handle) = self.handle_if_moved(entity, translation, rotation) else {
      return;
    };
    let Some(body) = self.bodies.get_mut(handle) else {
      return;
    };
    if body.is_kinematic() {
      body.set_next_kinematic_position(pose(translation, rotation));
    } else {
      body.set_position(pose(translation, rotation), true);
    }
    self.teleported.insert(entity);
  }

  /// Give a dynamic body the game's velocities where they differ from
  /// Rapier's (compared through `get`, as for `handle_if_moved`).
  pub(crate) fn push_velocity(&mut self, entity: Entity, linvel: Vec2, angvel: f32) {
    if !linvel.is_finite() || !angvel.is_finite() {
      return;
    }
    let Some(handle) = self.body_entries.get(&entity).map(|entry| entry.handle) else {
      return;
    };
    let Some(body) = self.bodies.get(handle) else {
      return;
    };
    let new_linvel = from_rapier(body.linvel()) != linvel;
    let new_angvel = body.angvel() != angvel;
    if !body.is_dynamic() || !(new_linvel || new_angvel) {
      return;
    }
    let Some(body) = self.bodies.get_mut(handle) else {
      return;
    };
    if new_linvel {
      body.set_linvel(to_rapier(linvel), true);
    }
    if new_angvel {
      body.set_angvel(angvel, true);
    }
  }

  /// Move a kinematic body by its velocity over the step, unless the game
  /// moved it through `Transform` this step.
  pub(crate) fn drive_kinematic(&mut self, entity: Entity, linvel: Vec2, angvel: f32, dt: f32) {
    if self.teleported.contains(&entity) || !linvel.is_finite() || !angvel.is_finite() {
      return;
    }
    let Some(entry) = self.body_entries.get(&entity) else {
      return;
    };
    let Some(body) = self.bodies.get_mut(entry.handle) else {
      return;
    };
    if !body.is_kinematic() {
      return;
    }
    let translation = from_rapier(body.translation()) + linvel * dt;
    let rotation = body.rotation().angle() + angvel * dt;
    body.set_next_kinematic_position(pose(translation, rotation));
    self.driven.insert(entity);
  }

  pub(crate) fn queue(&mut self, body: Entity, push: Push) {
    if push.is_finite() {
      self.queued.push((body, push));
    }
  }

  /// Apply the queued pushes to dynamic bodies; one-step forces are
  /// remembered so `step` can clear them.
  pub(crate) fn apply_queued(&mut self) {
    let mut queued = std::mem::take(&mut self.queued);
    for (entity, push) in queued.drain(..) {
      let Some(entry) = self.body_entries.get(&entity) else {
        continue;
      };
      let Some(body) = self.bodies.get_mut(entry.handle) else {
        continue;
      };
      if !body.is_dynamic() {
        continue;
      }
      match push {
        Push::Impulse(impulse) => body.apply_impulse(to_rapier(impulse), true),
        Push::ImpulseAt(impulse, point) => {
          body.apply_impulse_at_point(to_rapier(impulse), to_rapier(point), true)
        }
        Push::AngularImpulse(impulse) => body.apply_torque_impulse(impulse, true),
        Push::Force(force) => {
          body.add_force(to_rapier(force), true);
          self.forced.push(entry.handle);
        }
        Push::ForceAt(force, point) => {
          body.add_force_at_point(to_rapier(force), to_rapier(point), true);
          self.forced.push(entry.handle);
        }
        Push::Torque(torque) => {
          body.add_torque(torque, true);
          self.forced.push(entry.handle);
        }
      }
    }
    self.queued = queued;
  }

  /// Advance by `dt` seconds, then resolve the step's contacts to entities.
  pub(crate) fn step(&mut self, dt: f32) {
    self.params.dt = dt;
    self.pipeline.step(
      to_rapier(self.gravity),
      &self.params,
      &mut self.islands,
      &mut self.broad_phase,
      &mut self.narrow_phase,
      &mut self.bodies,
      &mut self.colliders,
      &mut self.joints,
      &mut self.multibody_joints,
      &mut self.soft_bodies,
      &mut self.ccd_solver,
      &(),
      &self.collector,
    );
    for handle in self.forced.drain(..) {
      if let Some(body) = self.bodies.get_mut(handle) {
        body.reset_forces(false);
        body.reset_torques(false);
      }
    }
    self.collisions.clear();
    self.trigger_events.clear();
    self.forces.clear();
    let events = self
      .collector
      .collisions
      .get_mut()
      .unwrap_or_else(PoisonError::into_inner);
    let mut sensors: Vec<(Owner, Owner, bool)> = Vec::new();
    for Captured { event, contact } in events.drain(..) {
      let owner = |handle| {
        self
          .owners
          .get(&handle)
          .or_else(|| self.departed.get(&handle))
          .copied()
      };
      if let (Some(a), Some(b)) = (owner(event.collider1()), owner(event.collider2())) {
        if event.sensor() {
          sensors.push((a, b, event.started()));
        } else {
          self.collisions.push(RawCollision {
            a,
            b,
            started: event.started(),
            contact,
          });
        }
      }
    }
    for (a, b, started) in sensors {
      self.record_trigger(a, b, started);
    }
    let events = self
      .collector
      .forces
      .get_mut()
      .unwrap_or_else(PoisonError::into_inner);
    for event in events.drain(..) {
      let owner = |handle| self.owners.get(&handle).copied();
      if let (Some(a), Some(b)) = (owner(event.collider1), owner(event.collider2)) {
        self.forces.push(RawForce {
          a,
          b,
          force: event.total_force_magnitude,
          direction: from_rapier(event.max_force_direction),
          started: event.started,
        });
      }
    }
    self.departed.clear();
    self.break_joints(dt);
    for entry in self.body_entries.values_mut() {
      let asleep = self
        .bodies
        .get(entry.handle)
        .is_some_and(|body| body.is_sleeping());
      entry.fell_asleep = asleep && !entry.asleep;
      entry.asleep = asleep;
    }
  }

  /// Break the joints the step pulled past their break force. Rapier keeps a
  /// joint's linear impulse in three places: locked axes (revolute, fixed,
  /// a prismatic's cross axis) in `impulses`, limits (rope, a prismatic's
  /// range) and motors (spring, a prismatic's motor) on the axis itself. Each
  /// is the impulse of one solver substep (`dt / num_solver_iterations`).
  fn break_joints(&mut self, dt: f32) {
    self.broken.clear();
    let substep = dt / self.params.num_solver_iterations.max(1) as f32;
    if substep <= 0.0 {
      return;
    }
    for (&entity, entry) in &self.joint_entries {
      let (Some(handle), Some(limit), true) =
        (entry.handle, entry.joint.break_force, entry.enabled)
      else {
        continue;
      };
      let Some(joint) = self.joints.get(handle) else {
        continue;
      };
      let axis = |i: usize| joint.data.limits[i].impulse + joint.data.motors[i].impulse;
      let impulse = Vec2::new(joint.impulses.x + axis(0), joint.impulses.y + axis(1));
      if impulse.length() / substep > limit {
        self.broken.push((entity, entry.joint.a, entry.joint.b));
      }
    }
    let broken = std::mem::take(&mut self.broken);
    for &(entity, _, _) in &broken {
      self.remove_joint(entity);
    }
    self.broken = broken;
  }

  /// Bodies the step moved: awake dynamic bodies (and those that just fell
  /// asleep, now still) and kinematic bodies driven by velocity.
  pub(crate) fn moved_bodies(&self) -> impl Iterator<Item = BodyState> + '_ {
    let state = |entity: Entity, body: &rapier2d::prelude::RigidBody, dynamic: bool| BodyState {
      entity,
      translation: from_rapier(body.translation()),
      rotation: body.rotation().angle(),
      linvel: from_rapier(body.linvel()),
      angvel: body.angvel(),
      dynamic,
    };
    let dynamic = self
      .body_entries
      .iter()
      .filter_map(move |(&entity, entry)| {
        let body = self.bodies.get(entry.handle)?;
        let moved = !body.is_sleeping() || entry.fell_asleep;
        (body.is_dynamic() && body.is_enabled() && moved).then(|| state(entity, body, true))
      });
    let driven = self.driven.iter().filter_map(move |&entity| {
      let body = self.bodies.get(self.body_entries.get(&entity)?.handle)?;
      Some(state(entity, body, false))
    });
    dynamic.chain(driven)
  }

  /// Where Rapier holds a body the write-back places after the last step:
  /// an enabled dynamic body that moved (`moved_bodies`) or, under a parent
  /// (`nested`), rests (`resting_body`), or a kinematic body the step drove
  /// by velocity. Every other body stays where its `Transform` puts it: a
  /// root body asleep since before the step is not written.
  pub(crate) fn written_pose(&self, entity: Entity, nested: bool) -> Option<(Vec2, f32)> {
    let entry = self.body_entries.get(&entity)?;
    let body = self.bodies.get(entry.handle)?;
    let moved = nested || !body.is_sleeping() || entry.fell_asleep;
    let written =
      (body.is_dynamic() && body.is_enabled() && moved) || self.driven.contains(&entity);
    written.then(|| (from_rapier(body.translation()), body.rotation().angle()))
  }

  /// A dynamic body asleep since before the last step, which `moved_bodies`
  /// leaves out, where Rapier holds it (no velocities to write back).
  pub(crate) fn resting_body(&self, entity: Entity) -> Option<BodyState> {
    let entry = self.body_entries.get(&entity)?;
    let body = self.bodies.get(entry.handle)?;
    let resting = body.is_sleeping() && !entry.fell_asleep;
    (body.is_dynamic() && body.is_enabled() && resting).then(|| BodyState {
      entity,
      translation: from_rapier(body.translation()),
      rotation: body.rotation().angle(),
      linvel: Vec2::ZERO,
      angvel: 0.0,
      dynamic: false,
    })
  }

  /// A body's world pose after the last step, for tests.
  #[cfg(test)]
  pub(crate) fn body_pose(&self, entity: Entity) -> Option<(Vec2, f32)> {
    self.body_world_pose(entity)
  }

  #[cfg(test)]
  pub(crate) fn body_count(&self) -> usize {
    self.body_entries.len()
  }

  #[cfg(test)]
  pub(crate) fn collider_count(&self) -> usize {
    self.collider_entries.len()
  }

  /// Entities with a warning logged, for tests.
  #[cfg(test)]
  pub(crate) fn warned_count(&self) -> usize {
    self.warned.len()
  }

  /// Whether the entity's body is asleep (at rest, costing nothing).
  pub fn is_sleeping(&self, entity: Entity) -> bool {
    self
      .body_entries
      .get(&entity)
      .and_then(|entry| self.bodies.get(entry.handle))
      .is_some_and(|body| body.is_sleeping())
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
  use crate::core::ecs::{GlobalTransform, Transform};
  use crate::core::physics::sync::placement;
  use std::f32::consts::FRAC_PI_2;

  fn entity(index: u32) -> Entity {
    Entity::from_raw_u32(index).expect("a valid test index")
  }

  fn collider_center(physics: &Physics, entity: Entity) -> Vec2 {
    physics
      .collider_pose(entity)
      .map(|(translation, _)| translation)
      .expect("a simulated collider")
  }

  /// The placement of `collider` on an unscaled entity at `at`, turned by
  /// `rotation`, relative to `body` when given.
  fn place(at: Vec2, rotation: f32, collider: &Collider, body: Option<(Vec2, f32)>) -> Placement {
    let transform = Transform::from_translation(at).with_rotation(rotation);
    placement(&GlobalTransform::from_transform(&transform), collider, body)
  }

  /// A collider without a body, on an entity at `at` turned by `rotation`.
  fn bare(physics: &mut Physics, e: Entity, at: Vec2, rotation: f32, collider: &Collider) {
    physics.sync_collider(e, None, place(at, rotation, collider, None), collider);
  }

  /// A body and its own collider at `at`.
  fn add(physics: &mut Physics, e: Entity, at: Vec2, body: RigidBody, collider: Collider) {
    physics.add_body(e, at, 0.0, Vec2::ZERO, 0.0, &body);
    let placement = place(at, 0.0, &collider, Some((at, 0.0)));
    physics.sync_collider(e, Some(e), placement, &collider);
  }

  #[test]
  fn a_100_px_box_weighs_one() {
    let mut physics = Physics::new();
    let e = entity(1);
    add(
      &mut physics,
      e,
      Vec2::ZERO,
      RigidBody::dynamic(),
      Collider::rect(100.0, 100.0),
    );
    let mass = physics.bodies[physics.body_entries[&e].handle].mass();
    assert!((mass - 1.0).abs() < 1e-3, "{mass}");
  }

  #[test]
  fn positions_are_pixels() {
    let mut physics = Physics::new();
    let e = entity(1);
    add(
      &mut physics,
      e,
      Vec2::new(250.0, -40.0),
      RigidBody::kinematic(),
      Collider::circle(5.0),
    );
    assert_eq!(physics.body_pose(e), Some((Vec2::new(250.0, -40.0), 0.0)));
  }

  #[test]
  fn a_collider_is_centred_on_the_entity_plus_its_offset() {
    let mut physics = Physics::new();
    let (centred, offset) = (entity(1), entity(2));
    let at = Vec2::new(100.0, 100.0);
    add(
      &mut physics,
      centred,
      at,
      RigidBody::dynamic(),
      Collider::circle(20.0),
    );
    add(
      &mut physics,
      offset,
      at,
      RigidBody::dynamic(),
      Collider::circle(20.0).with_offset(32.0, -8.0),
    );
    assert!((collider_center(&physics, centred) - at).length() < 1e-3);
    assert!((collider_center(&physics, offset) - Vec2::new(132.0, 92.0)).length() < 1e-3);
  }

  #[test]
  fn a_bare_collider_s_offset_turns_with_its_entity() {
    let mut physics = Physics::new();
    let e = entity(1);
    let collider = Collider::rect(4.0, 4.0).with_offset(10.0, 0.0);
    bare(
      &mut physics,
      e,
      Vec2::new(100.0, 100.0),
      FRAC_PI_2,
      &collider,
    );
    assert!((collider_center(&physics, e) - Vec2::new(100.0, 110.0)).length() < 1e-3);
    bare(&mut physics, e, Vec2::ZERO, 0.0, &collider);
    assert!((collider_center(&physics, e) - Vec2::new(10.0, 0.0)).length() < 1e-3);
  }

  #[test]
  fn a_collider_joins_a_body_added_later_and_stays_when_it_goes() {
    let mut physics = Physics::new();
    let e = entity(1);
    let collider = Collider::circle(4.0).with_offset(5.0, 0.0);
    bare(&mut physics, e, Vec2::ZERO, 0.0, &collider);
    let handle = physics.collider_entries[&e].handle;
    assert_eq!(
      physics.owners[&handle],
      Owner {
        collider: e,
        body: e,
        trigger: false
      }
    );

    let at = Vec2::new(50.0, 0.0);
    physics.add_body(e, at, 0.0, Vec2::ZERO, 0.0, &RigidBody::dynamic());
    physics.sync_collider(
      e,
      Some(e),
      place(at, 0.0, &collider, Some((at, 0.0))),
      &collider,
    );
    assert_eq!(physics.collider_entries[&e].body, Some(e));
    let body = physics.body_entries[&e].handle;
    assert_eq!(physics.colliders[handle].parent(), Some(body));
    assert!(
      physics.bodies[body].mass() > 0.0,
      "the collider gives it mass"
    );
    let local = physics.colliders[handle]
      .position_wrt_parent()
      .map(|pose| from_rapier(pose.translation));
    assert_eq!(local, Some(Vec2::new(5.0, 0.0)), "its offset from the body");

    physics.remove_body(e);
    assert_eq!(physics.collider_entries[&e].body, None);
    assert_eq!(physics.colliders[handle].parent(), None);
    assert_eq!((physics.body_count(), physics.collider_count()), (0, 1));
  }

  #[test]
  fn a_collider_moves_to_another_body_and_back_to_none() {
    let mut physics = Physics::new();
    let (a, b, part) = (entity(1), entity(2), entity(3));
    let collider = Collider::rect(100.0, 100.0);
    physics.add_body(a, Vec2::ZERO, 0.0, Vec2::ZERO, 0.0, &RigidBody::dynamic());
    let at = Vec2::new(200.0, 0.0);
    physics.add_body(b, at, 0.0, Vec2::ZERO, 0.0, &RigidBody::dynamic());
    physics.sync_collider(
      part,
      Some(a),
      place(Vec2::ZERO, 0.0, &collider, Some((Vec2::ZERO, 0.0))),
      &collider,
    );
    let handle = physics.collider_entries[&part].handle;
    physics.sync_collider(
      part,
      Some(b),
      place(at, 0.0, &collider, Some((at, 0.0))),
      &collider,
    );
    assert_eq!(
      physics.colliders[handle].parent(),
      Some(physics.body_entries[&b].handle)
    );
    assert_eq!(physics.owners[&handle].body, b);
    physics.sync_collider(part, None, place(at, 0.0, &collider, None), &collider);
    assert_eq!(physics.colliders[handle].parent(), None);
    assert_eq!(physics.owners[&handle].body, part);
    assert!((collider_center(&physics, part) - at).length() < 1e-3);
  }

  #[test]
  fn a_collider_whose_body_is_not_simulated_waits() {
    let mut physics = Physics::new();
    let (body, part) = (entity(1), entity(2));
    let collider = Collider::circle(4.0);
    let placement = place(Vec2::ZERO, 0.0, &collider, Some((Vec2::ZERO, 0.0)));
    physics.sync_collider(part, Some(body), placement, &collider);
    assert_eq!(physics.collider_count(), 0);
    assert!(physics.waiting.contains(&part));
    physics.add_body(
      body,
      Vec2::ZERO,
      0.0,
      Vec2::ZERO,
      0.0,
      &RigidBody::dynamic(),
    );
    physics.sync_collider(part, Some(body), placement, &collider);
    assert_eq!(physics.collider_count(), 1);
    assert!(physics.waiting.is_empty());
  }

  #[test]
  fn drift_below_the_tolerance_never_adds_up() {
    let mut physics = Physics::new();
    let e = entity(1);
    let collider = Collider::circle(4.0);
    for x in [0.0, 0.0006, 0.0012] {
      bare(&mut physics, e, Vec2::new(x, 0.0), 0.0, &collider);
    }
    // Each step was under the tolerance, the whole drift is not
    let x = collider_center(&physics, e).x;
    assert!((x - 0.0012).abs() < 1e-6, "{x}");
  }

  #[test]
  fn invalid_shapes_and_poses_are_skipped() {
    let mut physics = Physics::new();
    bare(
      &mut physics,
      entity(1),
      Vec2::ZERO,
      0.0,
      &Collider::rect(0.0, 4.0),
    );
    bare(
      &mut physics,
      entity(2),
      Vec2::ZERO,
      0.0,
      &Collider::circle(f32::NAN),
    );
    bare(
      &mut physics,
      entity(3),
      Vec2::new(f32::INFINITY, 0.0),
      0.0,
      &Collider::circle(4.0),
    );
    let flat = Collider::circle(4.0);
    let squashed = Placement {
      map: Mat2::from_diagonal(Vec2::new(1.0, 0.0)),
      ..place(Vec2::ZERO, 0.0, &flat, None)
    };
    physics.sync_collider(entity(5), None, squashed, &flat);
    physics.add_body(
      entity(4),
      Vec2::ZERO,
      f32::NAN,
      Vec2::ZERO,
      0.0,
      &RigidBody::dynamic(),
    );
    assert_eq!((physics.body_count(), physics.collider_count()), (0, 0));
  }

  #[test]
  fn a_non_finite_offset_or_rotation_never_reaches_rapier() {
    let mut physics = Physics::new();
    let circle = || Collider::circle(4.0);
    bare(
      &mut physics,
      entity(1),
      Vec2::ZERO,
      0.0,
      &circle().with_offset(f32::NAN, 0.0),
    );
    bare(
      &mut physics,
      entity(2),
      Vec2::ZERO,
      0.0,
      &circle().with_rotation(f32::INFINITY),
    );
    assert_eq!(physics.collider_count(), 0);
    // An edit to one is not applied: it stays where it was
    let edited = entity(3);
    bare(&mut physics, edited, Vec2::ZERO, 0.0, &circle());
    bare(
      &mut physics,
      edited,
      Vec2::ZERO,
      0.0,
      &circle().with_offset(0.0, f32::NAN),
    );
    assert_eq!(physics.collider_count(), 1);
    assert_eq!(collider_center(&physics, edited), Vec2::ZERO);
    // A bare collider's new offset is not placed from a non-finite Transform
    let moved = entity(4);
    bare(&mut physics, moved, Vec2::ZERO, 0.0, &circle());
    bare(
      &mut physics,
      moved,
      Vec2::new(f32::NAN, 0.0),
      0.0,
      &circle().with_offset(5.0, 0.0),
    );
    assert!(collider_center(&physics, moved).is_finite());
  }
}
