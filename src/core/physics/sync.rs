//! Keep Rapier in step with the world. Each fixed step, in
//! `FixedSet::Physics` while `PhysicsPaused` is false: forget what went away,
//! track disabled entities, propagate transforms, register new bodies, bring
//! colliders in line, push the game's changes, bring joints in line, step,
//! write the simulated bodies back and turn contacts and broken joints into
//! messages.
//!
//! Bodies may sit anywhere in the hierarchy, and a collider belongs to the
//! nearest body at or above its entity. Physics reads world poses from
//! `GlobalTransform` and writes a body back into its local `Transform`
//! through its parent; a collider's shape follows its entity's world scale.
//!
//! A body's rotation is its local direction turned through its parent's
//! linear map (`body_rotation`): the entity's own scale, mirroring included,
//! stays with its shape. So a root body's rotation is its
//! `Transform.rotation`, and the write-back (`local_pose`) maps the simulated
//! direction back through the parent, which propagation turns into the same
//! world angle under any parent scale.

use glam::{Affine2, Mat2};

use super::body::{AngularVelocity, BodyKind, RigidBody};
use super::collider::Collider;
use super::config::PhysicsConfig;
use super::convert::angle_delta;
use super::events::{CollisionStarted, CollisionStopped, Contact, ContactForce, JointBroken};
use super::joint::Joint;
use super::world::{Physics, Placement, WriteBack};
use crate::core::ecs::propagate::propagate_transforms;
use crate::core::ecs::{
  Added, Allow, Changed, ChildOf, Children, Commands, DetectChanges, DetectChangesMut, Disabled,
  EcsSystemState, Entity, GlobalTransform, Has, IntoScheduleConfigs, MessageWriter, Mut, Or,
  ParamSet, Query, Ref, RemovedComponents, Res, ResMut, SystemChangeTick, Tick, Transform, Vec2,
  Velocity, With, World,
};
use crate::core::scene::PhysicsPaused;
use crate::core::schedule::{EngineSchedules, FixedSet, Phase};
use crate::core::time::Time;

/// Run condition: physics steps unless the scene stack paused it.
pub(crate) fn physics_enabled(paused: Res<PhysicsPaused>) -> bool {
  !paused.0
}

/// How deep a collider may sit below its body.
const MAX_DEPTH: usize = 64;

/// A body as `register_bodies` reads it; its velocities may be reset (see
/// `apply_edit`)
type BodyRow<'a> = (
  Entity,
  BodyPlace<'a>,
  &'a mut Velocity,
  &'a mut AngularVelocity,
  Ref<'a, RigidBody>,
);
/// What places a body in the world (see `body_pose`)
type BodyPlace<'a> = (&'a GlobalTransform, &'a Transform, Option<&'a ChildOf>);
/// New or edited bodies, and every body (for the refused ones)
type BodyRows<'a, 'w, 's> = (
  Query<'w, 's, BodyRow<'a>, Changed<RigidBody>>,
  Query<'w, 's, BodyRow<'a>>,
);
/// World transforms, disabled entities included
type Globals<'a, 'w, 's> = Query<'w, 's, &'a GlobalTransform, Allow<Disabled>>;
type HasPhysics = Or<(With<RigidBody>, With<Collider>, With<Joint>)>;
/// A body whose `Velocity` or `AngularVelocity` changed
type Pushed = (
  With<RigidBody>,
  Or<(Changed<Velocity>, Changed<AngularVelocity>)>,
);
/// A collider edited, moved, scaled or reparented
type Resync = (
  With<Collider>,
  Or<(
    Changed<Collider>,
    Changed<GlobalTransform>,
    Changed<ChildOf>,
  )>,
);
/// A collider `sync_colliders` was told about by change detection, with
/// what tells the write-back's own move from any other change
type ResyncRow<'a> = (
  Entity,
  Ref<'a, Collider>,
  Option<Ref<'a, Transform>>,
  Has<RigidBody>,
  Has<ChildOf>,
);
/// A body whose world pose changed, with what tells the write-back's own
/// move from any other (see `push_changes`)
type MovedRow<'a> = (
  Entity,
  &'a GlobalTransform,
  Ref<'a, Transform>,
  Option<&'a ChildOf>,
);
/// Entities whose subtree may hold colliders that change owner
type Relinkable = (Or<(With<Collider>, With<Children>)>, Allow<Disabled>);
/// An entity's local `Transform` and parent link
type LocalLink<'a> = (&'a mut Transform, Option<&'a ChildOf>);
/// What `read_back` reads and writes, fetched at a tick of its own
type ReadBack<'w, 's> = (
  ResMut<'w, Physics>,
  Query<'w, 's, LocalLink<'static>, Allow<Disabled>>,
  Query<'w, 's, Entity, (With<RigidBody>, With<ChildOf>)>,
  Query<'w, 's, (&'static mut Velocity, &'static mut AngularVelocity), With<RigidBody>>,
  SystemChangeTick,
);

/// A body's world rotation: its local rotation turned through its parent's
/// linear map. The entity's own scale, mirroring included, stays out (its
/// shape carries it), so a root body's rotation is its `Transform.rotation`.
/// Under a parent it is kept continuous near the sum of the angles: the sync
/// tolerance grows with an angle's magnitude (see `poses_differ`), which a
/// wrapped angle loses, and a body turned thousands of radians would
/// otherwise never sleep.
fn body_rotation(transform: &Transform, parent: Option<&GlobalTransform>) -> f32 {
  let Some(parent) = parent else {
    return transform.rotation;
  };
  let direction = parent.affine().matrix2 * Vec2::from_angle(transform.rotation);
  let near = parent.rotation() + transform.rotation;
  near + angle_delta(near, direction.y.atan2(direction.x))
}

/// A body's world pose: its translation and `body_rotation`. A parent
/// without a `GlobalTransform` places it from the identity, as propagation
/// does.
fn body_pose((global, transform, parent): BodyPlace, globals: &Globals) -> (Vec2, f32) {
  let parent = parent.and_then(|parent| globals.get(parent.parent()).ok());
  (global.translation(), body_rotation(transform, parent))
}

/// The local `Transform` (keeping `scale`) that puts a body at `pose` in the
/// world under a parent whose world transform is `parent` (the identity when
/// `None`): the inverse of `body_pose`. `None` when the parent's map cannot
/// be inverted (a zero scale) or the result is not finite.
fn local_pose(
  parent: Option<Affine2>,
  (translation, rotation): (Vec2, f32),
  scale: Vec2,
) -> Option<Transform> {
  let local = match parent {
    None => Transform {
      translation,
      rotation,
      scale,
    },
    Some(parent) => {
      if !parent.matrix2.determinant().is_normal() {
        return None;
      }
      let inverse = parent.inverse();
      let direction = inverse.matrix2 * Vec2::from_angle(rotation);
      Transform {
        translation: inverse.transform_point2(translation),
        rotation: direction.y.atan2(direction.x),
        scale,
      }
    }
  };
  (local.translation.is_finite() && local.rotation.is_finite()).then_some(local)
}

/// The nearest entity at or above `entity` with a `RigidBody`.
fn owning_body(
  entity: Entity,
  parents: &Query<&ChildOf, Allow<Disabled>>,
  bodies: &Query<BodyPlace, (With<RigidBody>, Allow<Disabled>)>,
) -> Option<Entity> {
  let mut current = entity;
  for _ in 0..MAX_DEPTH {
    if bodies.contains(current) {
      return Some(current);
    }
    current = parents.get(current).ok()?.parent();
  }
  None
}

/// Push `root` and everything below it onto `out`; `stack` is scratch space.
fn subtree(
  root: Entity,
  children: &Query<&Children, Allow<Disabled>>,
  stack: &mut Vec<(Entity, usize)>,
  out: &mut Vec<Entity>,
) {
  stack.clear();
  stack.push((root, 0));
  while let Some((entity, depth)) = stack.pop() {
    out.push(entity);
    if depth < MAX_DEPTH
      && let Ok(kids) = children.get(entity)
    {
      stack.extend(kids.iter().map(|&kid| (kid, depth + 1)));
    }
  }
}

/// A collider's placement: its frame is its entity's world transform, moved
/// by `offset` and turned by `rotation`; relative to `body` (that body's world
/// translation and rotation) when it has one.
pub(crate) fn placement(
  global: &GlobalTransform,
  collider: &Collider,
  body: Option<(Vec2, f32)>,
) -> Placement {
  let affine = global.affine();
  let offset = affine.matrix2 * collider.offset;
  let rotation = global.rotation() + collider.rotation;
  // Shape points: turned by the collider's rotation, mapped by the entity's
  // linear part, then turned back by the frame's rotation
  let map = Mat2::from_angle(-rotation) * affine.matrix2 * Mat2::from_angle(collider.rotation);
  match body {
    Some((translation, body_rotation)) => Placement {
      // The offset is added after the subtraction: on the body's own entity
      // the two translations cancel exactly, however far out it is
      translation: Vec2::from_angle(-body_rotation)
        .rotate(affine.translation - translation + offset),
      rotation: rotation - body_rotation,
      map,
    },
    None => Placement {
      translation: affine.translation + offset,
      rotation,
      map,
    },
  }
}

/// Forget joints, colliders and bodies whose components went away (despawn
/// included); a body's colliders that stay become fixed and its joints
/// inert. Entities that lost their `RigidBody` or their parent are
/// remembered for `sync_colliders`, which finds the colliders at and below
/// them a new owner, and bodies that lost their parent for `push_changes`.
/// An entity left without any of the three loses its logged warnings,
/// registered or not.
#[allow(clippy::too_many_arguments)] // one removal reader or query per input
pub(crate) fn forget_removed(
  mut physics: ResMut<Physics>,
  mut joints: RemovedComponents<Joint>,
  mut colliders: RemovedComponents<Collider>,
  mut bodies: RemovedComponents<RigidBody>,
  mut detached: RemovedComponents<ChildOf>,
  relinkable: Query<(), Relinkable>,
  kept: Query<(), (HasPhysics, Allow<Disabled>)>,
  has_body: Query<(), (With<RigidBody>, Allow<Disabled>)>,
) {
  for entity in joints.read() {
    physics.remove_joint(entity);
    if !kept.contains(entity) {
      physics.forget_warnings(entity);
    }
  }
  for entity in colliders.read() {
    physics.remove_collider(entity);
    if !kept.contains(entity) {
      physics.forget_warnings(entity);
    }
  }
  for entity in bodies.read() {
    physics.remove_body(entity);
    if !kept.contains(entity) {
      physics.forget_warnings(entity);
    }
    if relinkable.contains(entity) {
      physics.relink.insert(entity);
    }
  }
  for entity in detached.read() {
    if relinkable.contains(entity) {
      physics.relink.insert(entity);
    }
    if has_body.contains(entity) {
      physics.recheck.insert(entity);
    }
  }
}

/// Disable the simulation of disabled entities; re-enable (or register, if
/// they were spawned disabled) re-enabled ones. The steps skip a disabled
/// entity, so what the game changed meanwhile (`Transform`, `Velocity`,
/// `RigidBody`, `Collider`, `Joint`) is applied as it comes back: bodies
/// here, colliders by the next `sync_colliders` and joints by the next
/// `sync_joints`.
///
/// It also records its tick as `Physics::chain_tick`: in the physics chain
/// it runs just before propagation, so `read_back` can tell a `Transform`
/// changed after it. (Its run in `Last` is overwritten by the next step's
/// before `read_back` reads it.)
#[allow(clippy::too_many_arguments)] // one query or removal reader per input
pub(crate) fn track_disabled(
  mut physics: ResMut<Physics>,
  config: Res<PhysicsConfig>,
  disabled: Query<Entity, (Added<Disabled>, Allow<Disabled>, HasPhysics)>,
  mut enabled: RemovedComponents<Disabled>,
  mut locals: Query<LocalLink, Allow<Disabled>>,
  mut bodies: Query<(&mut Velocity, &mut AngularVelocity, Ref<RigidBody>)>,
  colliders: Query<(), With<Collider>>,
  joints: Query<(), With<Joint>>,
  ticks: SystemChangeTick,
) {
  physics.chain_tick = Some(ticks.this_run());
  // Density depends on the unit: configure before anything registers
  physics.configure(&config);
  for entity in &disabled {
    physics.set_enabled(entity, false);
  }
  for entity in enabled.read() {
    if joints.contains(entity) {
      // Made or brought up to date by `sync_joints`, after its bodies
      physics.set_joint_enabled(entity, true);
      physics.resync_joints.insert(entity);
    }
    let has_collider = colliders.contains(entity);
    if !(has_collider || bodies.contains(entity)) {
      continue;
    }
    // Propagation skips disabled entities, so one moved while disabled
    // comes back with a stale `GlobalTransform`: the body is placed from the
    // local transforms, and the changed mark has propagation refresh it (and
    // its subtree) before colliders sync
    let Some((parent, world)) = world_transform(entity, &locals) else {
      continue;
    };
    if let Ok((mut transform, _)) = locals.get_mut(entity) {
      transform.set_changed();
    }
    if let Ok((mut velocity, mut angular, body)) = bodies.get_mut(entity)
      && let Ok((transform, _)) = locals.get(entity)
    {
      let (translation, rotation) = (
        world.translation(),
        body_rotation(transform, parent.as_ref()),
      );
      physics.add_body(
        entity,
        translation,
        rotation,
        velocity.as_vec2(),
        angular.0,
        &body,
      );
      // Edits made while it was disabled; `register_bodies` applying a
      // recent one again changes nothing
      apply_edit(&mut physics, entity, &body, &mut velocity, &mut angular);
      physics.place_body(entity, translation, rotation);
      physics.push_velocity(entity, velocity.as_vec2(), angular.0);
    }
    if has_collider {
      // Registered, or brought up to date, by the next `sync_colliders`
      physics.waiting.insert(entity);
    }
    physics.set_enabled(entity, true);
  }
}

/// An entity's world transform and its parent's, composed from the local
/// `Transform`s up the hierarchy as propagation computes them (an ancestor
/// without a `Transform` places its children from the identity, so the
/// parent's is `None` there and at a root). `None` without a `Transform`.
fn world_transform(
  entity: Entity,
  locals: &Query<LocalLink, Allow<Disabled>>,
) -> Option<(Option<GlobalTransform>, GlobalTransform)> {
  let (transform, mut parent) = locals.get(entity).ok()?;
  let mut ancestors = Vec::new();
  for _ in 0..MAX_DEPTH {
    let Some(link) = parent else {
      break;
    };
    let Ok((transform, next)) = locals.get(link.parent()) else {
      break;
    };
    ancestors.push(*transform);
    parent = next;
  }
  let parent = (!ancestors.is_empty()).then(|| {
    ancestors
      .iter()
      .rev()
      .fold(GlobalTransform::IDENTITY, |global, transform| {
        global.mul_transform(transform)
      })
  });
  let world = parent
    .unwrap_or(GlobalTransform::IDENTITY)
    .mul_transform(transform);
  Some((parent, world))
}

/// Apply an edited `RigidBody` in place. A dynamic body made kinematic or
/// fixed stops: on those `Velocity` is the game's input, so the velocities
/// the simulation wrote back are cleared (a velocity the game set itself is
/// kept).
fn apply_edit(
  physics: &mut Physics,
  entity: Entity,
  body: &RigidBody,
  velocity: &mut Mut<Velocity>,
  angular: &mut Mut<AngularVelocity>,
) {
  if body.kind != BodyKind::Dynamic
    && physics.is_simulated_velocity(entity, velocity.as_vec2(), angular.0)
  {
    velocity.set_if_neq(Velocity::zero());
    angular.set_if_neq(AngularVelocity(0.0));
  }
  physics.update_body(entity, body);
}

/// Register new bodies at their world pose and apply edited ones in place.
/// A body refused for a non-finite pose is tried again every step until its
/// pose is finite.
pub(crate) fn register_bodies(
  mut physics: ResMut<Physics>,
  config: Res<PhysicsConfig>,
  mut bodies: ParamSet<BodyRows>,
  globals: Globals,
) {
  physics.configure(&config);
  let refused: Vec<Entity> = physics.refused.drain().collect();
  for (entity, place, mut velocity, mut angular, body) in &mut bodies.p0() {
    if body.is_added() {
      let (translation, rotation) = body_pose(place, &globals);
      physics.add_body(
        entity,
        translation,
        rotation,
        velocity.as_vec2(),
        angular.0,
        &body,
      );
    } else if body.is_changed() {
      apply_edit(&mut physics, entity, &body, &mut velocity, &mut angular);
    }
  }
  // Already registered (re-added or re-enabled meanwhile) is a no-op
  let bodies = bodies.p1();
  for entity in refused {
    if let Ok((entity, place, velocity, angular, body)) = bodies.get(entity) {
      let (translation, rotation) = body_pose(place, &globals);
      physics.add_body(
        entity,
        translation,
        rotation,
        velocity.as_vec2(),
        angular.0,
        &body,
      );
    }
  }
}

/// Bring every collider that changed, moved, scaled or changed owner in line
/// with the simulation. Owners change when a `RigidBody` appears or goes
/// above a collider, or when a collider or an ancestor is reparented.
///
/// A root body's own collider that only the write-back moved (its
/// `Transform` last changed at `Physics::write_tick`, its `Collider` not
/// since, the body not in `Physics::recheck`) is skipped: it moved with its
/// body, and its placement relative to the body depends only on the
/// entity's scale and the `Collider`. Every other source (waiting, new
/// bodies, reparenting, relinking) is visited.
#[allow(clippy::too_many_arguments)] // one query per input
pub(crate) fn sync_colliders(
  mut physics: ResMut<Physics>,
  changed: Query<ResyncRow, Resync>,
  new_bodies: Query<Entity, Added<RigidBody>>,
  reparented: Query<Entity, Changed<ChildOf>>,
  children: Query<&Children, Allow<Disabled>>,
  parents: Query<&ChildOf, Allow<Disabled>>,
  bodies: Query<BodyPlace, (With<RigidBody>, Allow<Disabled>)>,
  colliders: Query<(&GlobalTransform, &Collider)>,
  globals: Globals,
) {
  // Buffers kept between steps: this runs every step over every moving
  // collider
  let mut todo = std::mem::take(&mut physics.sync_queue);
  let mut stack = std::mem::take(&mut physics.walk_stack);
  let mut relink = std::mem::take(&mut physics.relink);
  todo.clear();
  let own = physics.write_tick;
  let recheck = &physics.recheck;
  let any_recheck = !recheck.is_empty();
  todo.extend(
    changed
      .iter()
      .filter(|(entity, collider, transform, body, child)| {
        let own_move = *body
          && !*child
          && !collider.is_changed()
          && transform
            .as_ref()
            .is_some_and(|transform| Some(transform.last_changed()) == own)
          && !(any_recheck && recheck.contains(entity));
        !own_move
      })
      .map(|(entity, ..)| entity),
  );
  // One query's entities are distinct; only other sources can repeat them
  let distinct = todo.len();
  todo.extend(physics.waiting.drain());
  for root in new_bodies
    .iter()
    .chain(reparented.iter())
    .chain(relink.drain())
  {
    subtree(root, &children, &mut stack, &mut todo);
  }
  if todo.len() > distinct {
    todo.sort_unstable();
    todo.dedup();
  }
  for &entity in &todo {
    let Ok((global, collider)) = colliders.get(entity) else {
      continue;
    };
    #[cfg(test)]
    {
      physics.compared += 1;
    }
    let body = owning_body(entity, &parents, &bodies);
    let body_pose = body.map(|body| match bodies.get(body) {
      Ok(place) => body_pose(place, &globals),
      Err(_) => (Vec2::ZERO, 0.0),
    });
    physics.sync_collider(
      entity,
      body,
      placement(global, collider, body_pose),
      collider,
    );
  }
  todo.clear();
  physics.sync_queue = todo;
  physics.walk_stack = stack;
  physics.relink = relink;
}

/// Push the game's writes since the last step. The write-back is an
/// ordinary change too (propagation must see bodies move), so it is told
/// apart in two ways:
///
/// - By its tick. `read_back` records the tick it ran at
///   (`Physics::write_tick`), a tick no other write has. A root body whose
///   `Transform` last changed at that tick is where Rapier holds it, and
///   velocities last changed at it are Rapier's; these are skipped without
///   a look at Rapier, unless the body is in `Physics::recheck`. A game
///   write between the write-back and this push (in a system, by a command
///   or between schedule runs) has a later tick, even when it writes the
///   same value. A game write inside the physics chain after its
///   propagation is stamped by the write-back: the write-back overwrites
///   its translation, rotation and velocities, as it always did, and
///   `read_back` puts the body in `recheck` because its scale may be new.
///   A body that lost its parent is in `recheck` too.
/// - By value, for everything else: a pose or velocity equal to Rapier's
///   (within the sync tolerance) is not pushed, so untouched bodies stay
///   asleep. A nested body's world pose also depends on its ancestors, so it
///   is always compared.
///
/// Fixed colliders without a body are moved by `sync_colliders`.
pub(crate) fn push_changes(
  mut physics: ResMut<Physics>,
  time: Res<Time>,
  moved: Query<MovedRow, (With<RigidBody>, Changed<GlobalTransform>)>,
  globals: Globals,
  pushed: Query<(Entity, Ref<Velocity>, Ref<AngularVelocity>), Pushed>,
  kinematic: Query<(Entity, &RigidBody, &Velocity, &AngularVelocity)>,
) {
  physics.begin_push();
  let own = physics.write_tick;
  let mut recheck = std::mem::take(&mut physics.recheck);
  let any_recheck = !recheck.is_empty();
  for (entity, global, transform, parent) in &moved {
    if parent.is_none()
      && Some(transform.last_changed()) == own
      && !(any_recheck && recheck.contains(&entity))
    {
      continue;
    }
    #[cfg(test)]
    {
      physics.compared += 1;
    }
    let (translation, rotation) = body_pose((global, &transform, parent), &globals);
    if !(translation.is_finite() && rotation.is_finite()) {
      physics.warn_once(entity, "a non-finite Transform is not pushed to physics");
      continue;
    }
    physics.move_body(entity, translation, rotation);
  }
  // Unchanged since the last push, or changed by the write-back only
  let kept = |changed: bool, last: Tick| !changed || Some(last) == own;
  for (entity, velocity, angular) in &pushed {
    if kept(velocity.is_changed(), velocity.last_changed())
      && kept(angular.is_changed(), angular.last_changed())
      && !(any_recheck && recheck.contains(&entity))
    {
      continue;
    }
    #[cfg(test)]
    {
      physics.compared += 1;
    }
    physics.push_velocity(entity, velocity.as_vec2(), angular.0);
  }
  recheck.clear();
  physics.recheck = recheck;
  let dt = time.fixed.timestep();
  for (entity, body, velocity, angular) in &kinematic {
    let still = velocity.as_vec2() == Vec2::ZERO && angular.0 == 0.0;
    if body.kind == BodyKind::Kinematic && !still {
      physics.drive_kinematic(entity, velocity.as_vec2(), angular.0, dt);
    }
  }
  physics.apply_queued();
}

/// Register new, edited and re-enabled joints, and retry inert ones. After
/// `push_changes`: a joint made as the game moves one of its bodies anchors
/// to where the body was moved.
pub(crate) fn sync_joints(
  mut physics: ResMut<Physics>,
  changed: Query<(Entity, &Joint), Changed<Joint>>,
  joints: Query<&Joint>,
) {
  for (entity, joint) in &changed {
    physics.sync_joint(entity, joint);
  }
  // Re-enabled ones: `Changed` misses edits made while they were disabled
  let mut resync = std::mem::take(&mut physics.resync_joints);
  for entity in resync.drain() {
    if let Ok(joint) = joints.get(entity) {
      physics.sync_joint(entity, joint);
    }
  }
  physics.resync_joints = resync;
  physics.retry_inert_joints();
}

/// Step the simulation by the fixed timestep.
pub(crate) fn step_simulation(mut physics: ResMut<Physics>, time: Res<Time>) {
  crate::profile_scope!("cpu:physics");
  physics.step(time.fixed.timestep());
}

/// The rotation the write-back gives a body at `current` that the step left
/// at `simulated`: turned the short way, so it stays continuous (no jump at
/// the wrap into (-PI, PI]).
fn continued(current: f32, simulated: f32) -> f32 {
  current + angle_delta(current, simulated)
}

/// An entity's world transform once the write-back lands, as the next
/// propagation will compute it: up its hierarchy, bodies the write-back
/// places (`Physics::written_pose`) are where it puts them, other entities
/// where their `Transform` puts them. `None` for an entity without a
/// `Transform` (its children are placed from the identity).
///
/// Read before any write-back lands: the rotation is `continued` from the
/// `Transform`'s, exactly as `read_back` writes it. A large continuous
/// rotation and Rapier's wrapped angle round differently, and a child placed
/// against the wrapped one would be moved by its own write-back every step.
fn world_after_step(
  entity: Entity,
  physics: &Physics,
  locals: &Query<LocalLink, Allow<Disabled>>,
  depth: usize,
) -> Option<Affine2> {
  let (transform, parent) = locals.get(entity).ok()?;
  let written = physics.written_pose(entity, parent.is_some());
  let parent = match parent {
    Some(parent) if depth < MAX_DEPTH => {
      world_after_step(parent.parent(), physics, locals, depth + 1)
    }
    _ => None,
  };
  let local = written
    .and_then(|pose| local_pose(parent, pose, transform.scale))
    .map(|local| Transform {
      rotation: continued(transform.rotation, local.rotation),
      ..local
    })
    .unwrap_or(*transform);
  Some(parent.unwrap_or(Affine2::IDENTITY) * local.to_affine())
}

/// Write the bodies the step moved back into their local `Transform`,
/// through their parent's world transform after the step (an ancestor body
/// may have moved too, however far up). Rotation stays `continued`. Under a
/// parent that cannot be inverted (a zero scale) the `Transform` is left as
/// it is.
///
/// A dynamic body asleep under a parent is written too (only where its
/// local pose changed): Rapier holds it still, so when an ancestor moves its
/// local `Transform` must change for it to stay put.
///
/// Every write is worked out before the first lands, so `world_after_step`
/// reads each ancestor's `Transform` as `read_back` found it whatever order
/// the bodies come in.
///
/// Its tick is recorded as `Physics::write_tick`, and the next step's
/// `push_changes` and `sync_colliders` skip what last changed at that tick
/// (see `push_changes`). The invariant: nothing else changes a body's
/// `Transform`, `Velocity` or `AngularVelocity` at this tick. Every system
/// runs at a tick of its own, but commands applied at a sync point take the
/// world's current tick, which the next system is handed too. So this is an
/// exclusive system that advances the tick before it fetches anything: no
/// command, wherever the scheduler puts its sync point, lands at its tick.
/// A game change made earlier, inside the chain after its propagation, is
/// stamped by the write below; such bodies go into `Physics::recheck`.
pub(crate) fn read_back(world: &mut World, state: &mut EcsSystemState<ReadBack<'static, 'static>>) {
  world.increment_change_tick();
  let Ok((mut physics, mut locals, nested, mut velocities, ticks)) = state.get_mut(world) else {
    return;
  };
  let this_run = ticks.this_run();
  physics.write_tick = Some(this_run);
  let chain_tick = physics.chain_tick;
  let mut writes = std::mem::take(&mut physics.write_backs);
  writes.clear();
  let resting = nested
    .iter()
    .filter_map(|entity| physics.resting_body(entity));
  for state in physics.moved_bodies().chain(resting) {
    let Ok((transform, parent)) = locals.get(state.entity) else {
      continue;
    };
    let parent = parent
      .map(ChildOf::parent)
      .and_then(|parent| world_after_step(parent, &physics, &locals, 0));
    let local =
      local_pose(parent, (state.translation, state.rotation), transform.scale).map(|local| {
        (
          local.translation,
          continued(transform.rotation, local.rotation),
        )
      });
    writes.push(WriteBack {
      entity: state.entity,
      local,
      velocities: state.dynamic.then_some((state.linvel, state.angvel)),
    });
  }
  for write in writes.drain(..) {
    if let Some((translation, rotation)) = write.local
      && let Ok((mut transform, _)) = locals.get_mut(write.entity)
    {
      // Changed after the step's propagation (by a game system inside the
      // chain): this step never saw it, and the write below stamps it with
      // the write-back's tick, scale included, so the next step compares it
      if chain_tick.is_some_and(|tick| transform.last_changed().is_newer_than(tick, this_run)) {
        physics.recheck.insert(write.entity);
      }
      if transform.translation != translation {
        transform.translation = translation;
      }
      if transform.rotation != rotation {
        transform.rotation = rotation;
      }
    }
    if let Some((linvel, angvel)) = write.velocities
      && let Ok((mut velocity, mut angular)) = velocities.get_mut(write.entity)
    {
      velocity.set_if_neq(Velocity::from_vec2(linvel));
      angular.set_if_neq(AngularVelocity(angvel));
    }
  }
  physics.write_backs = writes;
}

/// Turn the step's contacts, forces and broken joints into messages; a
/// broken joint's `Joint` component goes too.
pub(crate) fn emit_messages(
  physics: Res<Physics>,
  mut started: MessageWriter<CollisionStarted>,
  mut stopped: MessageWriter<CollisionStopped>,
  mut forces: MessageWriter<ContactForce>,
  mut broken_joints: MessageWriter<JointBroken>,
  mut commands: Commands,
) {
  for c in &physics.collisions {
    let (a, b, body_a, body_b) = (c.a.collider, c.b.collider, c.a.body, c.b.body);
    if c.started {
      started.write(CollisionStarted {
        a,
        b,
        body_a,
        body_b,
        trigger: c.sensor,
        contact: c.contact.map(|(point, normal)| Contact { point, normal }),
      });
    } else {
      stopped.write(CollisionStopped {
        a,
        b,
        body_a,
        body_b,
        trigger: c.sensor,
      });
    }
  }
  for f in &physics.forces {
    forces.write(ContactForce {
      a: f.a.collider,
      b: f.b.collider,
      body_a: f.a.body,
      body_b: f.b.body,
      force: f.force,
      direction: f.direction,
      started: f.started,
    });
  }
  for &(joint, a, b) in &physics.broken {
    broken_joints.write(JointBroken { joint, a, b });
    if let Ok(mut entity) = commands.get_entity(joint) {
      entity.try_remove::<Joint>();
    }
  }
}

/// Add the physics systems to `FixedSet::Physics`, chained, running only
/// while physics is not paused.
pub(crate) fn register_physics_systems(schedules: &mut EngineSchedules) {
  schedules.add_systems(
    Phase::FixedUpdate,
    (
      forget_removed,
      track_disabled,
      propagate_transforms,
      register_bodies,
      sync_colliders,
      push_changes,
      sync_joints,
      step_simulation,
      read_back,
      emit_messages,
    )
      .chain()
      .in_set(FixedSet::Physics)
      .run_if(physics_enabled),
  );
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ecs::{Messages, Resource, World};
  use crate::core::physics::ColliderShape;
  use crate::core::physics::CollisionStarted;
  use crate::core::physics::test_util::*;
  use crate::core::scene::PhysicsPaused;
  use crate::core::schedule::{EngineSchedules, Phase};

  fn drop_for_one_second(hz: u32) -> f32 {
    let (mut world, mut s) = world(hz);
    let e = ball(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, hz as usize);
    at(&world, e).translation.y
  }

  #[test]
  fn fall_distance_per_second_is_independent_of_the_fixed_rate() {
    let at60 = drop_for_one_second(60);
    let at120 = drop_for_one_second(120);
    assert!((at60 - at120).abs() / at60 < 0.03, "{at60} vs {at120}");
    assert!(
      (at60 - 490.0).abs() < 60.0,
      "roughly g/2 after one second: {at60}"
    );
  }

  #[test]
  fn gravity_is_pixels_whatever_the_unit() {
    let fell = |ppm: f32| {
      let (mut world, mut s) = world(60);
      *world.resource_mut::<PhysicsConfig>() = PhysicsConfig::default().with_pixels_per_meter(ppm);
      let e = ball(&mut world, 0.0, 0.0);
      run(&mut world, &mut s, 60);
      at(&world, e).translation.y
    };
    let (at50, at100) = (fell(50.0), fell(100.0));
    assert!((at50 - at100).abs() < 2.0, "{at50} vs {at100}");
  }

  #[test]
  fn density_follows_the_configured_unit_from_the_first_step() {
    let (mut world, mut s) = world(60);
    *world.resource_mut::<PhysicsConfig>() = PhysicsConfig::default().with_pixels_per_meter(50.0);
    let e = world
      .spawn((
        Transform::IDENTITY,
        RigidBody::dynamic(),
        Collider::rect(100.0, 100.0),
      ))
      .id();
    run(&mut world, &mut s, 1);
    let physics = world.resource::<Physics>();
    let mass = physics
      .bodies
      .get(physics.body_entries[&e].handle)
      .unwrap()
      .mass();
    // 100 px at 50 px/m is 2 m: 4 m² at density 1
    assert!((mass - 4.0).abs() < 1e-3, "{mass}");
  }

  #[test]
  fn a_body_re_enabled_between_steps_registers_at_the_configured_unit() {
    let (mut world, mut s) = world(60);
    s.add_systems(Phase::Last, track_disabled);
    *world.resource_mut::<PhysicsConfig>() = PhysicsConfig::default().with_pixels_per_meter(50.0);
    let e = world
      .spawn((
        Transform::IDENTITY,
        RigidBody::dynamic(),
        Collider::rect(100.0, 100.0),
        Disabled,
      ))
      .id();
    world.entity_mut(e).remove::<Disabled>();
    // The frame's `Last` bookkeeping registers the body before any fixed
    // step runs; its collider joins it in the next step
    s.run(Phase::Last, &mut world);
    assert_eq!(world.resource::<Physics>().body_count(), 1);
    run(&mut world, &mut s, 1);
    let physics = world.resource::<Physics>();
    let mass = physics
      .bodies
      .get(physics.body_entries[&e].handle)
      .unwrap()
      .mass();
    assert!((mass - 4.0).abs() < 1e-3, "{mass}");
  }

  #[test]
  fn a_body_made_kinematic_with_a_velocity_moves_by_it() {
    let (mut world, mut s) = world(60);
    let e = ball(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, 10);
    let made_still = ball(&mut world, 100.0, 0.0);
    run(&mut world, &mut s, 10);
    world.entity_mut(made_still).insert(RigidBody::kinematic());
    world
      .entity_mut(e)
      .insert((RigidBody::kinematic(), Velocity::new(60.0, 0.0)));
    let before = at(&world, e).translation;
    run(&mut world, &mut s, 60);
    let moved = at(&world, e).translation - before;
    assert!((moved - Vec2::new(60.0, 0.0)).length() < 0.5, "{moved}");
    assert_eq!(
      world.get::<Velocity>(made_still),
      Some(&Velocity::zero()),
      "the velocity the simulation wrote back is cleared"
    );
  }

  #[test]
  fn a_bare_collider_is_a_static_wall() {
    let (mut world, mut s) = world(60);
    let wall = floor(&mut world, 100.0);
    let b = ball(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, 120);
    assert!(
      (at(&world, b).translation.y - 90.0).abs() < 2.0,
      "{:?}",
      at(&world, b)
    );
    assert_eq!(at(&world, wall).translation, Vec2::new(0.0, 110.0));
    let physics = world.resource::<Physics>();
    assert_eq!((physics.body_count(), physics.collider_count()), (1, 2));
  }

  #[test]
  fn a_fixed_body_never_moves() {
    let (mut world, mut s) = world(60);
    let platform = world
      .spawn((
        Transform::from_xy(0.0, 110.0),
        RigidBody::fixed(),
        Collider::rect(400.0, 20.0),
      ))
      .id();
    ball(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, 120);
    assert_eq!(at(&world, platform).translation, Vec2::new(0.0, 110.0));
  }

  fn dropped_box(body: RigidBody) -> (f32, f32) {
    let (mut world, mut s) = world(60);
    floor(&mut world, 100.0);
    let e = world
      .spawn((
        Transform::from_xy(0.0, 40.0).with_rotation(0.4),
        body,
        Collider::rect(40.0, 40.0),
      ))
      .id();
    let mut fastest_spin: f32 = 0.0;
    for _ in 0..120 {
      run(&mut world, &mut s, 1);
      fastest_spin = fastest_spin.max(world.get::<AngularVelocity>(e).unwrap().0.abs());
    }
    (at(&world, e).rotation, fastest_spin)
  }

  #[test]
  fn a_box_landing_on_its_corner_rotates() {
    let (rotation, spin) = dropped_box(RigidBody::dynamic());
    assert!((rotation - 0.4).abs() > 0.1, "it tipped over: {rotation}");
    assert!(spin > 0.5, "AngularVelocity was written back: {spin}");
  }

  #[test]
  fn lock_rotation_holds() {
    let (rotation, spin) = dropped_box(RigidBody::dynamic().lock_rotation());
    assert!((rotation - 0.4).abs() < 1e-4, "{rotation}");
    assert_eq!(spin, 0.0);
  }

  #[test]
  fn a_spinning_body_s_rotation_stays_continuous() {
    let (mut world, mut s) = world(60);
    no_gravity(&mut world);
    let e = ball(&mut world, 0.0, 0.0);
    world.get_mut::<AngularVelocity>(e).unwrap().0 = 10.0;
    run(&mut world, &mut s, 60);
    let rotation = at(&world, e).rotation;
    assert!(
      (rotation - 10.0).abs() < 0.2,
      "not wrapped into (-PI, PI]: {rotation}"
    );
  }

  #[test]
  fn a_kinematic_body_moved_by_velocity_writes_its_transform_back() {
    let (mut world, mut s) = world(60);
    let e = world
      .spawn((
        Transform::IDENTITY,
        RigidBody::kinematic(),
        Velocity::new(60.0, 0.0),
        Collider::rect(10.0, 10.0),
      ))
      .id();
    run(&mut world, &mut s, 60);
    assert!(
      (at(&world, e).translation.x - 60.0).abs() < 0.5,
      "{:?}",
      at(&world, e)
    );
    assert_eq!(world.get::<Velocity>(e), Some(&Velocity::new(60.0, 0.0)));
  }

  #[test]
  fn a_kinematic_platform_moved_by_transform_carries_a_resting_box() {
    let (mut world, mut s) = world(60);
    let platform = world
      .spawn((
        Transform::from_xy(0.0, 110.0),
        RigidBody::kinematic(),
        Collider::rect(200.0, 20.0),
      ))
      .id();
    let crate_ = world
      .spawn((
        Transform::from_xy(0.0, 80.0),
        RigidBody::dynamic().lock_rotation(),
        Collider::rect(20.0, 20.0).with_friction(1.0),
      ))
      .id();
    run(&mut world, &mut s, 60);
    for _ in 0..60 {
      world.get_mut::<Transform>(platform).unwrap().translation.x += 1.0;
      run(&mut world, &mut s, 1);
    }
    let x = at(&world, crate_).translation.x;
    assert!((x - 60.0).abs() < 8.0, "carried with the platform: {x}");
  }

  #[test]
  fn higher_dominance_pushes_without_being_pushed() {
    let (mut world, mut s) = world(60);
    no_gravity(&mut world);
    let pusher = world
      .spawn((
        Transform::from_xy(0.0, 0.0),
        RigidBody::dynamic().lock_rotation().with_dominance(1),
        Velocity::new(100.0, 0.0),
        Collider::rect(20.0, 20.0),
      ))
      .id();
    let pushed = world
      .spawn((
        Transform::from_xy(50.0, 0.0),
        RigidBody::dynamic().lock_rotation(),
        Velocity::new(-100.0, 0.0),
        Collider::rect(20.0, 20.0),
      ))
      .id();
    run(&mut world, &mut s, 60);
    assert!(world.get::<Velocity>(pusher).unwrap().dx > 90.0);
    assert!(world.get::<Velocity>(pushed).unwrap().dx > 0.0);
  }

  #[test]
  fn a_resting_body_sleeps_and_the_sync_does_not_wake_it() {
    let (mut world, mut s) = world(60);
    floor(&mut world, 100.0);
    let b = ball(&mut world, 0.0, 50.0);
    run(&mut world, &mut s, 600);
    assert!(
      world.resource::<Physics>().is_sleeping(b),
      "ten seconds at rest"
    );
    let before = at(&world, b);
    run(&mut world, &mut s, 60);
    assert!(
      world.resource::<Physics>().is_sleeping(b),
      "sync must not wake it"
    );
    assert_eq!(at(&world, b), before);
  }

  #[test]
  fn never_sleep_keeps_a_resting_body_awake() {
    let (mut world, mut s) = world(60);
    floor(&mut world, 100.0);
    let b = world
      .spawn((
        Transform::from_xy(0.0, 50.0),
        RigidBody::dynamic().never_sleep(),
        Collider::circle(10.0),
      ))
      .id();
    run(&mut world, &mut s, 600);
    assert!(!world.resource::<Physics>().is_sleeping(b));
  }

  fn fall_after(change: impl FnOnce(&mut World, Entity)) -> f32 {
    let (mut world, mut s) = world(60);
    let e = ball(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, 10);
    change(&mut world, e);
    let before = at(&world, e).translation.y;
    run(&mut world, &mut s, 10);
    at(&world, e).translation.y - before
  }

  #[test]
  fn a_body_made_kinematic_stops_falling() {
    let fell = fall_after(|world, e| {
      world.entity_mut(e).insert(RigidBody::kinematic());
    });
    assert!(fell.abs() < 1e-3, "a kinematic body fell {fell}");
  }

  #[test]
  fn a_body_removed_and_added_again_in_one_tick_keeps_simulating() {
    let fell = fall_after(|world, e| {
      world.entity_mut(e).remove::<RigidBody>();
      world.entity_mut(e).insert(RigidBody::dynamic());
    });
    assert!(fell > 1.0, "the body stopped: fell {fell}");
  }

  #[test]
  fn a_non_finite_transform_is_ignored() {
    let (mut world, mut s) = world(60);
    let e = ball(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, 5);
    world.get_mut::<Transform>(e).unwrap().translation.x = f32::NAN;
    run(&mut world, &mut s, 5);
    let (position, rotation) = world.resource::<Physics>().body_pose(e).unwrap();
    assert!(position.is_finite() && rotation.is_finite());
    assert!(
      at(&world, e).translation.is_finite(),
      "the write-back restored it"
    );
  }

  #[test]
  fn a_body_spawned_at_a_non_finite_pose_registers_once_it_is_fixed() {
    let (mut world, mut s) = world(60);
    let e = world
      .spawn((
        Transform::from_xy(f32::NAN, 0.0),
        RigidBody::dynamic(),
        Collider::circle(5.0),
      ))
      .id();
    run(&mut world, &mut s, 2);
    let physics = world.resource::<Physics>();
    assert_eq!((physics.body_count(), physics.collider_count()), (0, 0));
    world.get_mut::<Transform>(e).unwrap().translation = Vec2::ZERO;
    run(&mut world, &mut s, 1);
    let physics = world.resource::<Physics>();
    assert_eq!((physics.body_count(), physics.collider_count()), (1, 1));
    assert_eq!(
      physics.collider_entries[&e].body,
      Some(e),
      "its collider waited for it"
    );
    run(&mut world, &mut s, 29);
    assert!(at(&world, e).translation.y > 10.0, "it falls");
    assert_eq!(world.resource::<Physics>().warnings, 1);
  }

  #[test]
  fn spawn_and_despawn_in_one_step_leaves_nothing() {
    let (mut world, mut s) = world(60);
    let b = ball(&mut world, 0.0, 0.0);
    let f = floor(&mut world, 100.0);
    world.despawn(b);
    world.despawn(f);
    run(&mut world, &mut s, 1);
    let b = ball(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, 1);
    world.despawn(b);
    run(&mut world, &mut s, 1);
    let physics = world.resource::<Physics>();
    assert_eq!((physics.body_count(), physics.collider_count()), (0, 0));
  }

  #[test]
  fn removing_the_body_leaves_a_static_collider() {
    let (mut world, mut s) = world(60);
    let platform = world
      .spawn((
        Transform::from_xy(0.0, 110.0),
        RigidBody::kinematic(),
        Collider::rect(400.0, 20.0),
      ))
      .id();
    run(&mut world, &mut s, 1);
    world.entity_mut(platform).remove::<RigidBody>();
    let b = ball(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, 120);
    let physics = world.resource::<Physics>();
    assert_eq!((physics.body_count(), physics.collider_count()), (1, 2));
    assert!((at(&world, b).translation.y - 90.0).abs() < 2.0);
  }

  #[test]
  fn a_disabled_floor_lets_a_ball_through_and_stays_registered() {
    let (mut world, mut s) = world(60);
    let f = floor(&mut world, 100.0);
    run(&mut world, &mut s, 1);
    world.entity_mut(f).insert(Disabled);
    let b = ball(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, 60);
    assert!(at(&world, b).translation.y > 150.0, "fell through");
    assert_eq!(world.resource::<Physics>().collider_count(), 2);
    world.entity_mut(f).remove::<Disabled>();
    let caught = ball(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, 120);
    assert!((at(&world, caught).translation.y - 90.0).abs() < 2.0);
  }

  #[test]
  fn edits_made_while_disabled_apply_when_re_enabled() {
    let (mut world, mut s) = world(60);
    let e = ball(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, 10);
    world.entity_mut(e).insert(Disabled);
    run(&mut world, &mut s, 1);
    // Moved, stopped and made weightless while out of the simulation, with
    // steps running in between
    world.get_mut::<Transform>(e).unwrap().translation = Vec2::new(300.0, 0.0);
    *world.get_mut::<Velocity>(e).unwrap() = Velocity::zero();
    world
      .entity_mut(e)
      .insert(RigidBody::dynamic().with_gravity_scale(0.0));
    run(&mut world, &mut s, 5);
    world.entity_mut(e).remove::<Disabled>();
    run(&mut world, &mut s, 30);
    let (position, _) = world.resource::<Physics>().body_pose(e).unwrap();
    assert!(
      (position - Vec2::new(300.0, 0.0)).length() < 1e-3,
      "{position}"
    );
    assert_eq!(at(&world, e).translation, position);
  }

  #[test]
  fn re_enabled_and_made_kinematic_with_a_velocity_in_one_tick_moves_by_it() {
    let (mut world, mut s) = world(60);
    let e = ball(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, 10);
    world.entity_mut(e).insert(Disabled);
    run(&mut world, &mut s, 1);
    world.entity_mut(e).remove::<Disabled>();
    world
      .entity_mut(e)
      .insert((RigidBody::kinematic(), Velocity::new(60.0, 0.0)));
    let before = at(&world, e).translation;
    run(&mut world, &mut s, 60);
    let moved = at(&world, e).translation - before;
    assert!((moved - Vec2::new(60.0, 0.0)).length() < 0.5, "{moved}");
    assert_eq!(world.get::<Velocity>(e), Some(&Velocity::new(60.0, 0.0)));
  }

  #[test]
  fn an_edit_made_while_disabled_applies_when_only_the_frame_end_sees_the_re_enable() {
    let (mut world, mut s) = world(60);
    s.add_systems(Phase::Last, track_disabled);
    let e = ball(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, 10);
    world.entity_mut(e).insert(Disabled);
    run(&mut world, &mut s, 1);
    s.run(Phase::Last, &mut world);
    // Edited while disabled, then a fixed step, then re-enabled later in the
    // frame: only the frame-end bookkeeping sees the removal before the
    // removal events expire
    world.entity_mut(e).insert(RigidBody::kinematic());
    run(&mut world, &mut s, 1);
    world.entity_mut(e).remove::<Disabled>();
    s.run(Phase::Last, &mut world);
    world.clear_trackers();
    world.clear_trackers();
    let before = at(&world, e).translation;
    run(&mut world, &mut s, 10);
    assert_eq!(at(&world, e).translation, before, "it stayed dynamic");
  }

  /// A ball drifting slower than the sleep threshold, asleep after ten
  /// seconds: Rapier stops it, though it was still moving.
  fn sleeping_ball() -> (World, EngineSchedules, Entity) {
    let (mut world, mut s) = world(60);
    no_gravity(&mut world);
    let b = ball(&mut world, 0.0, 0.0);
    world.get_mut::<Velocity>(b).unwrap().dx = 10.0;
    run(&mut world, &mut s, 600);
    assert!(world.resource::<Physics>().is_sleeping(b));
    (world, s, b)
  }

  #[test]
  fn a_body_falling_asleep_writes_zero_velocities() {
    let (world, _, b) = sleeping_ball();
    assert_eq!(world.get::<Velocity>(b), Some(&Velocity::zero()));
    assert_eq!(world.get::<AngularVelocity>(b), Some(&AngularVelocity(0.0)));
  }

  #[test]
  fn a_sleeping_body_made_kinematic_with_a_velocity_moves_by_it() {
    let (mut world, mut s, b) = sleeping_ball();
    world
      .entity_mut(b)
      .insert((RigidBody::kinematic(), Velocity::new(60.0, 0.0)));
    let before = at(&world, b).translation;
    run(&mut world, &mut s, 60);
    let moved = at(&world, b).translation - before;
    assert!((moved - Vec2::new(60.0, 0.0)).length() < 0.5, "{moved}");
  }

  #[test]
  fn a_body_disabled_while_asleep_stays_asleep_and_put_when_re_enabled() {
    let (mut world, mut s, b) = sleeping_ball();
    let before = at(&world, b);
    world.entity_mut(b).insert(Disabled);
    run(&mut world, &mut s, 5);
    world.entity_mut(b).remove::<Disabled>();
    run(&mut world, &mut s, 60);
    assert!(world.resource::<Physics>().is_sleeping(b));
    assert_eq!(at(&world, b), before);
  }

  #[test]
  fn a_disabled_body_keeps_its_velocity() {
    let (mut world, mut s) = world(60);
    no_gravity(&mut world);
    let e = world
      .spawn((
        Transform::IDENTITY,
        RigidBody::dynamic(),
        Velocity::new(60.0, 0.0),
        Collider::circle(4.0),
      ))
      .id();
    run(&mut world, &mut s, 30);
    world.entity_mut(e).insert(Disabled);
    let parked = at(&world, e).translation;
    run(&mut world, &mut s, 30);
    assert_eq!(
      at(&world, e).translation,
      parked,
      "it did not move while disabled"
    );
    world.entity_mut(e).remove::<Disabled>();
    run(&mut world, &mut s, 30);
    let x = at(&world, e).translation.x;
    assert!((x - 60.0).abs() < 0.5, "a second of motion at 60 px/s: {x}");
    assert_eq!(world.get::<Velocity>(e), Some(&Velocity::new(60.0, 0.0)));
  }

  #[test]
  fn a_non_finite_transform_never_reaches_a_re_enabled_collider() {
    let (mut world, mut s) = world(60);
    let f = floor(&mut world, 100.0);
    run(&mut world, &mut s, 1);
    world.entity_mut(f).insert(Disabled);
    run(&mut world, &mut s, 1);
    world.get_mut::<Transform>(f).unwrap().translation = Vec2::new(f32::NAN, 300.0);
    world.entity_mut(f).remove::<Disabled>();
    run(&mut world, &mut s, 1);
    let physics = world.resource::<Physics>();
    let collider = &physics.colliders[physics.collider_entries[&f].handle];
    assert!(
      collider.translation().is_finite(),
      "{:?}",
      collider.translation()
    );
  }

  #[test]
  fn a_body_turned_thousands_of_radians_still_sleeps() {
    let (mut world, mut s) = world(60);
    no_gravity(&mut world);
    // As if a wheel had rolled for minutes: the continuous rotation is large
    // and f32 round-off at that magnitude exceeds the sync tolerance
    let e = world
      .spawn((
        Transform::IDENTITY.with_rotation(3000.0),
        RigidBody::dynamic(),
        Collider::rect(10.0, 10.0),
      ))
      .id();
    run(&mut world, &mut s, 600);
    assert!(
      world.resource::<Physics>().is_sleeping(e),
      "the write-back's round-off must not count as a game write"
    );
  }

  #[test]
  fn a_body_made_kinematic_while_disabled_stays_put_when_re_enabled() {
    let (mut world, mut s) = world(60);
    let e = ball(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, 10);
    world.entity_mut(e).insert(Disabled);
    run(&mut world, &mut s, 1);
    world.entity_mut(e).insert(RigidBody::kinematic());
    run(&mut world, &mut s, 1);
    world.entity_mut(e).remove::<Disabled>();
    let before = at(&world, e).translation;
    run(&mut world, &mut s, 10);
    assert_eq!(at(&world, e).translation, before);
    assert_eq!(world.get::<Velocity>(e), Some(&Velocity::zero()));
  }

  #[test]
  fn a_body_spawned_disabled_registers_when_enabled() {
    let (mut world, mut s) = world(60);
    let e = world
      .spawn((
        Transform::IDENTITY,
        RigidBody::dynamic(),
        Collider::circle(4.0),
        Disabled,
      ))
      .id();
    run(&mut world, &mut s, 2);
    assert_eq!(world.resource::<Physics>().body_count(), 0);
    world.entity_mut(e).remove::<Disabled>();
    run(&mut world, &mut s, 2);
    assert_eq!(world.resource::<Physics>().body_count(), 1);
  }

  #[test]
  fn a_collider_added_after_the_body_still_registers() {
    let (mut world, mut s) = world(60);
    let e = world
      .spawn((Transform::IDENTITY, RigidBody::dynamic()))
      .id();
    run(&mut world, &mut s, 1);
    assert_eq!(world.resource::<Physics>().collider_count(), 0);
    world.entity_mut(e).insert(Collider::circle(4.0));
    run(&mut world, &mut s, 1);
    assert_eq!(world.resource::<Physics>().collider_count(), 1);
  }

  #[test]
  fn a_game_write_to_transform_moves_a_kinematic_body_over_the_step() {
    let (mut world, mut s) = world(60);
    let e = world
      .spawn((
        Transform::IDENTITY,
        RigidBody::kinematic(),
        Collider::rect(4.0, 4.0),
      ))
      .id();
    run(&mut world, &mut s, 1);
    world.get_mut::<Transform>(e).unwrap().translation.x = 50.0;
    run(&mut world, &mut s, 1);
    let (position, _) = world.resource::<Physics>().body_pose(e).unwrap();
    assert!((position.x - 50.0).abs() < 1e-3);
  }

  #[test]
  fn a_falling_body_s_global_transform_follows_it() {
    use crate::core::ecs::GlobalTransform;
    use crate::core::ecs::propagate::propagate_transforms;
    let (mut world, mut s) = world(60);
    s.add_systems(
      Phase::FixedUpdate,
      propagate_transforms.in_set(FixedSet::Transforms),
    );
    let e = ball(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, 30);
    let local = at(&world, e).translation;
    let global = world.get::<GlobalTransform>(e).unwrap().translation();
    assert!(local.y > 0.0);
    assert_eq!(global, local, "propagation sees physics' writes");
  }

  #[test]
  fn paused_physics_runs_no_step() {
    let (mut world, mut s) = world(60);
    let e = ball(&mut world, 0.0, 0.0);
    world.resource_mut::<PhysicsPaused>().0 = true;
    run(&mut world, &mut s, 30);
    assert_eq!(at(&world, e).translation.y, 0.0);
  }

  use crate::core::physics::{CollisionStopped, ContactForce};

  fn read<M: crate::core::ecs::Message + Copy>(world: &World) -> Vec<M> {
    let messages = world.resource::<Messages<M>>();
    messages.get_cursor().read(messages).copied().collect()
  }

  #[test]
  fn started_carries_the_first_contact_from_a_to_b() {
    let (mut world, mut s) = world(60);
    let f = floor(&mut world, 100.0);
    let b = ball(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, 60);
    let started = read::<CollisionStarted>(&world);
    let hit = started
      .iter()
      .find(|m| m.involves(b) && m.involves(f))
      .expect("ball hit floor");
    let contact = hit.contact.expect("a solid contact has a point");
    let down = if hit.a == b { Vec2::Y } else { -Vec2::Y };
    assert!((contact.normal - down).length() < 1e-3, "{contact:?}");
    assert!((contact.point.y - 100.0).abs() < 1.5, "{contact:?}");
    assert!(!hit.trigger);
  }

  #[test]
  fn a_trigger_reports_no_contact_point() {
    let (mut world, mut s) = world(60);
    let zone = world
      .spawn((
        Transform::from_xy(0.0, 100.0),
        Collider::rect(100.0, 40.0).as_trigger(),
      ))
      .id();
    let b = ball(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, 60);
    let started = read::<CollisionStarted>(&world);
    let hit = started
      .iter()
      .find(|m| m.involves(zone))
      .expect("entered the zone");
    assert!(hit.trigger && hit.contact.is_none());
    assert_eq!(hit.other(zone), Some(b));
  }

  #[test]
  fn messages_name_the_owning_body() {
    use crate::core::ecs::ChildOf;
    let (mut world, mut s) = world(60);
    let f = floor(&mut world, 100.0);
    let body = world
      .spawn((Transform::IDENTITY, RigidBody::dynamic()))
      .id();
    let part = world
      .spawn((Transform::IDENTITY, Collider::circle(10.0), ChildOf(body)))
      .id();
    run(&mut world, &mut s, 60);
    let started = read::<CollisionStarted>(&world);
    let hit = started.iter().find(|m| m.involves(f)).expect("landed");
    assert!(hit.involves(body) && hit.involves(part));
    assert_eq!(hit.other(f), Some(body));
    assert_eq!(hit.other_collider(f), Some(part));
    assert_eq!(hit.other(body), Some(f));
  }

  #[test]
  fn contact_force_fires_above_its_threshold_only() {
    let forces = |threshold: f32| {
      let (mut world, mut s) = world(60);
      floor(&mut world, 100.0);
      world.spawn((
        Transform::from_xy(0.0, 60.0),
        RigidBody::dynamic().lock_rotation(),
        Collider::rect(20.0, 20.0)
          .with_mass(10.0)
          .with_force_events(threshold),
      ));
      run(&mut world, &mut s, 30);
      read::<ContactForce>(&world)
    };
    let felt = forces(50.0);
    assert!(
      !felt.is_empty() && felt.iter().all(|m| m.force > 50.0),
      "{felt:?}"
    );
    assert!(forces(1e9).is_empty());
  }

  #[test]
  fn a_collider_despawned_while_touching_sends_collision_stopped() {
    let (mut world, mut s) = world(60);
    floor(&mut world, 100.0);
    let b = ball(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, 120);
    world.despawn(b);
    run(&mut world, &mut s, 1);
    let stopped = read::<CollisionStopped>(&world);
    assert!(stopped.iter().any(|m| m.involves(b)), "{stopped:?}");
  }

  #[test]
  fn touching_bodies_write_collision_messages() {
    let (mut world, mut s) = world(60);
    let f = floor(&mut world, 100.0);
    let b = ball(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, 60);
    let messages = world.resource::<Messages<CollisionStarted>>();
    let started: Vec<_> = messages.get_cursor().read(messages).copied().collect();
    assert!(
      started
        .iter()
        .any(|m| m.involves(b) && m.other(b) == Some(f)),
      "{started:?}"
    );
  }

  #[test]
  fn a_bouncy_ball_bounces_off_a_default_floor() {
    let (mut world, mut s) = world(60);
    floor(&mut world, 200.0);
    let b = world
      .spawn((
        Transform::IDENTITY,
        RigidBody::dynamic(),
        Collider::circle(10.0).with_restitution(1.0),
      ))
      .id();
    // Down, bounce, and most of the way back up within two seconds
    let mut lowest_after_bounce = f32::MAX;
    let mut bounced = false;
    for _ in 0..120 {
      run(&mut world, &mut s, 1);
      let v = world.get::<Velocity>(b).unwrap().dy;
      bounced |= v < 0.0;
      if bounced {
        lowest_after_bounce = lowest_after_bounce.min(at(&world, b).translation.y);
      }
    }
    assert!(
      bounced && lowest_after_bounce < 30.0,
      "{lowest_after_bounce}"
    );
  }

  fn slid(friction: f32) -> f32 {
    let (mut world, mut s) = world(60);
    floor(&mut world, 100.0);
    let e = world
      .spawn((
        Transform::from_xy(0.0, 90.0),
        RigidBody::dynamic().lock_rotation(),
        Velocity::new(100.0, 0.0),
        Collider::rect(20.0, 20.0).with_friction(friction),
      ))
      .id();
    run(&mut world, &mut s, 60);
    world.get::<Velocity>(e).unwrap().dx
  }

  #[test]
  fn zero_friction_on_either_side_slides() {
    assert!(slid(0.0) > 95.0, "{}", slid(0.0));
    assert!(slid(0.5) < 50.0, "{}", slid(0.5));
  }

  #[test]
  fn layers_filter_contacts_both_ways() {
    let fell = |collider: Collider| {
      let (mut world, mut s) = world(60);
      floor(&mut world, 100.0);
      let e = world
        .spawn((Transform::IDENTITY, RigidBody::dynamic(), collider))
        .id();
      run(&mut world, &mut s, 60);
      at(&world, e).translation.y
    };
    assert!(fell(Collider::circle(10.0)) < 95.0, "lands by default");
    assert!(
      fell(Collider::circle(10.0).with_layer(0b10).with_mask(0b10)) > 150.0,
      "its mask skips the floor"
    );
  }

  #[test]
  fn the_floor_s_mask_matters_too() {
    let (mut world, mut s) = world(60);
    world.spawn((
      Transform::from_xy(0.0, 110.0),
      Collider::rect(400.0, 20.0).with_mask(0b1),
    ));
    let e = world
      .spawn((
        Transform::IDENTITY,
        RigidBody::dynamic(),
        Collider::circle(10.0).with_layer(0b100),
      ))
      .id();
    run(&mut world, &mut s, 60);
    assert!(
      at(&world, e).translation.y > 150.0,
      "the floor does not collide with layer 3"
    );
  }

  #[test]
  fn a_kinematic_body_entering_a_fixed_trigger_is_detected() {
    let (mut world, mut s) = world(60);
    let zone = world
      .spawn((
        Transform::from_xy(100.0, 0.0),
        Collider::rect(40.0, 40.0).as_trigger(),
      ))
      .id();
    let mover = world
      .spawn((
        Transform::IDENTITY,
        RigidBody::kinematic(),
        Velocity::new(100.0, 0.0),
        Collider::rect(10.0, 10.0),
      ))
      .id();
    run(&mut world, &mut s, 60);
    let messages = world.resource::<Messages<CollisionStarted>>();
    let started: Vec<_> = messages.get_cursor().read(messages).copied().collect();
    assert!(
      started
        .iter()
        .any(|m| m.involves(mover) && m.involves(zone) && m.trigger),
      "{started:?}"
    );
  }

  #[test]
  fn density_and_mass_set_the_body_s_mass() {
    let (mut world, mut s) = world(60);
    let dense = world
      .spawn((
        Transform::IDENTITY,
        RigidBody::dynamic(),
        Collider::rect(100.0, 100.0).with_density(2.0),
      ))
      .id();
    let heavy = world
      .spawn((
        Transform::from_xy(300.0, 0.0),
        RigidBody::dynamic(),
        Collider::rect(10.0, 10.0).with_mass(5.0),
      ))
      .id();
    run(&mut world, &mut s, 1);
    let physics = world.resource::<Physics>();
    assert!((physics.mass(dense).unwrap() - 2.0).abs() < 1e-3);
    assert!((physics.mass(heavy).unwrap() - 5.0).abs() < 1e-3);
  }

  #[test]
  fn an_edited_collider_applies_in_place() {
    let (mut world, mut s) = world(60);
    let f = floor(&mut world, 100.0);
    let b = ball(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, 120);
    // The floor grows 40 px upward: its top is now at 60
    *world.get_mut::<Collider>(f).unwrap() = Collider::rect(400.0, 100.0);
    world.get_mut::<Transform>(f).unwrap().translation.y = 110.0;
    run(&mut world, &mut s, 120);
    assert!(
      (at(&world, b).translation.y - 50.0).abs() < 3.0,
      "{:?}",
      at(&world, b)
    );
  }

  #[test]
  fn editing_a_body_s_collider_updates_its_mass() {
    let (mut world, mut s) = world(60);
    no_gravity(&mut world);
    let e = world
      .spawn((
        Transform::IDENTITY,
        RigidBody::dynamic(),
        Collider::rect(100.0, 100.0),
      ))
      .id();
    let mass_after = |world: &mut World, s: &mut EngineSchedules, edit: fn(&mut Collider)| {
      edit(&mut world.get_mut::<Collider>(e).unwrap());
      run(world, s, 1);
      world.resource::<Physics>().mass(e).unwrap()
    };
    run(&mut world, &mut s, 1);
    let denser = mass_after(&mut world, &mut s, |c| c.density = 3.0);
    let wider = mass_after(&mut world, &mut s, |c| *c = Collider::rect(200.0, 100.0));
    let fixed_mass = mass_after(&mut world, &mut s, |c| c.mass = Some(7.0));
    for (mass, expected) in [(denser, 3.0), (wider, 2.0), (fixed_mass, 7.0)] {
      assert!((mass - expected).abs() < 1e-3, "{mass} vs {expected}");
    }
  }

  #[test]
  fn a_collider_edited_while_disabled_applies_when_re_enabled() {
    let (mut world, mut s) = world(60);
    let f = floor(&mut world, 100.0);
    run(&mut world, &mut s, 1);
    world.entity_mut(f).insert(Disabled);
    run(&mut world, &mut s, 1);
    // Grown and moved while out of the simulation, with steps in between:
    // its top is now at 60
    *world.get_mut::<Collider>(f).unwrap() = Collider::rect(400.0, 60.0).with_offset(0.0, -20.0);
    run(&mut world, &mut s, 5);
    world.entity_mut(f).remove::<Disabled>();
    let b = ball(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, 120);
    assert!(
      (at(&world, b).translation.y - 50.0).abs() < 2.0,
      "{:?}",
      at(&world, b)
    );
  }

  #[test]
  fn a_polygon_of_one_repeated_point_is_skipped_without_panicking() {
    let (mut world, mut s) = world(60);
    world.spawn((
      Transform::IDENTITY,
      Collider::polygon(vec![Vec2::splat(5.0); 3]),
    ));
    run(&mut world, &mut s, 1);
    assert_eq!(world.resource::<Physics>().collider_count(), 0);
  }

  #[test]
  fn a_collider_edited_to_an_invalid_shape_leaves_until_it_is_fixed() {
    let (mut world, mut s) = world(60);
    let f = floor(&mut world, 100.0);
    run(&mut world, &mut s, 1);
    world.get_mut::<Collider>(f).unwrap().shape = ColliderShape::Rect {
      size: Vec2::new(0.0, 20.0),
    };
    let dropped = ball(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, 60);
    assert_eq!(
      world.resource::<Physics>().collider_count(),
      1,
      "only the ball"
    );
    assert!(at(&world, dropped).translation.y > 150.0, "fell through");
    world.get_mut::<Collider>(f).unwrap().shape = ColliderShape::Rect {
      size: Vec2::new(400.0, 20.0),
    };
    let caught = ball(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, 120);
    assert_eq!(world.resource::<Physics>().collider_count(), 3);
    assert!((at(&world, caught).translation.y - 90.0).abs() < 2.0);
  }

  #[test]
  fn an_invalid_shape_is_skipped_until_it_is_fixed() {
    let (mut world, mut s) = world(60);
    let e = world
      .spawn((Transform::IDENTITY, Collider::rect(0.0, 10.0)))
      .id();
    run(&mut world, &mut s, 1);
    assert_eq!(world.resource::<Physics>().collider_count(), 0);
    world.get_mut::<Collider>(e).unwrap().shape = ColliderShape::Rect {
      size: Vec2::splat(10.0),
    };
    run(&mut world, &mut s, 1);
    assert_eq!(world.resource::<Physics>().collider_count(), 1);
  }

  #[test]
  fn capsules_polygons_and_polylines_hold_things_up() {
    let (mut world, mut s) = world(60);
    world.spawn((
      Transform::IDENTITY,
      Collider::polyline([Vec2::new(-200.0, 100.0), Vec2::new(200.0, 100.0)]),
    ));
    let capsule = world
      .spawn((
        Transform::from_xy(-50.0, 0.0),
        RigidBody::dynamic().lock_rotation(),
        Collider::capsule(40.0, 10.0),
      ))
      .id();
    let triangle = world
      .spawn((
        Transform::from_xy(50.0, 0.0),
        RigidBody::dynamic().lock_rotation(),
        Collider::polygon([
          Vec2::new(-10.0, 10.0),
          Vec2::new(10.0, 10.0),
          Vec2::new(0.0, -10.0),
        ]),
      ))
      .id();
    run(&mut world, &mut s, 120);
    assert!((at(&world, capsule).translation.y - 80.0).abs() < 2.0);
    assert!((at(&world, triangle).translation.y - 90.0).abs() < 2.0);
  }

  use crate::core::ecs::{ChildOf, GlobalTransform};
  use std::f32::consts::FRAC_PI_2;

  #[test]
  fn placement_composes_offset_rotation_scale_and_body() {
    let global = GlobalTransform::from_transform(
      &Transform::from_xy(100.0, 0.0)
        .with_rotation(FRAC_PI_2)
        .with_scale(Vec2::new(2.0, 1.0)),
    );
    let collider = Collider::rect(10.0, 10.0).with_offset(10.0, 0.0);
    let world = placement(&global, &collider, None);
    // The offset is scaled (x2) and turned a quarter: 20 px down
    assert!(
      (world.translation - Vec2::new(100.0, 20.0)).length() < 1e-4,
      "{:?}",
      world
    );
    assert!((world.rotation - FRAC_PI_2).abs() < 1e-5);
    assert!(
      (world.map - glam::Mat2::from_diagonal(Vec2::new(2.0, 1.0)))
        .abs_diff_eq(glam::Mat2::ZERO, 1e-5)
    );
    let relative = placement(&global, &collider, Some((Vec2::new(100.0, 0.0), FRAC_PI_2)));
    assert!(
      (relative.translation - Vec2::new(20.0, 0.0)).length() < 1e-4,
      "{:?}",
      relative
    );
    assert!(relative.rotation.abs() < 1e-5);
  }

  #[test]
  fn a_far_body_s_own_collider_keeps_its_exact_offset() {
    // f32 spacing at 20000 is about 0.002 px: adding then subtracting the
    // body's translation would round the offset by more than the tolerance
    let at = Vec2::new(20000.5, -20000.5);
    let global = GlobalTransform::from_transform(&Transform::from_translation(at));
    let collider = Collider::circle(4.0).with_offset(0.3, 0.7);
    let relative = placement(&global, &collider, Some((at, 0.0)));
    assert_eq!(relative.translation, Vec2::new(0.3, 0.7));
  }

  #[test]
  fn child_colliders_add_mass_and_move_with_their_body() {
    let (mut world, mut s) = world(60);
    floor(&mut world, 100.0);
    let body = world
      .spawn((Transform::IDENTITY, RigidBody::dynamic().lock_rotation()))
      .id();
    for x in [-30.0, 30.0] {
      world.spawn((
        Transform::from_xy(x, 0.0),
        Collider::rect(20.0, 20.0),
        ChildOf(body),
      ));
    }
    run(&mut world, &mut s, 120);
    let mass = world.resource::<Physics>().mass(body).unwrap();
    assert!((mass - 0.08).abs() < 1e-4, "two 20x20 boxes: {mass}");
    assert!(
      (at(&world, body).translation.y - 90.0).abs() < 2.0,
      "{:?}",
      at(&world, body)
    );
  }

  #[test]
  fn a_collider_spawned_with_its_body_attaches_to_it() {
    let (mut world, mut s) = world(60);
    let body = world
      .spawn((Transform::IDENTITY, RigidBody::dynamic()))
      .id();
    world.spawn((Transform::IDENTITY, Collider::circle(10.0), ChildOf(body)));
    run(&mut world, &mut s, 30);
    assert!(world.resource::<Physics>().mass(body).unwrap() > 0.0);
    assert!(
      at(&world, body).translation.y > 10.0,
      "it falls with its body"
    );
  }

  #[test]
  fn a_kinematic_child_body_follows_its_moving_parent() {
    let (mut world, mut s) = world(60);
    let parent = world.spawn(Transform::IDENTITY).id();
    let child = world
      .spawn((
        Transform::from_xy(0.0, 10.0),
        RigidBody::kinematic(),
        Collider::rect(10.0, 10.0),
        ChildOf(parent),
      ))
      .id();
    for _ in 0..60 {
      world.get_mut::<Transform>(parent).unwrap().translation.x += 1.0;
      run(&mut world, &mut s, 1);
    }
    let (position, _) = world.resource::<Physics>().body_pose(child).unwrap();
    assert!(
      (position - Vec2::new(60.0, 10.0)).length() < 1.01,
      "{position}"
    );
  }

  #[test]
  fn a_falling_body_under_a_turned_parent_writes_its_local_transform() {
    let (mut world, mut s) = world(60);
    let parent = world
      .spawn(Transform::from_xy(100.0, 0.0).with_rotation(FRAC_PI_2))
      .id();
    let child = world
      .spawn((
        Transform::IDENTITY,
        RigidBody::dynamic().lock_rotation(),
        Collider::circle(5.0),
        ChildOf(parent),
      ))
      .id();
    run(&mut world, &mut s, 30);
    let (world_position, _) = world.resource::<Physics>().body_pose(child).unwrap();
    assert!(
      (world_position.x - 100.0).abs() < 1e-2,
      "falls straight down in the world"
    );
    let local = at(&world, child);
    // World +Y is the parent's +X once turned a quarter
    assert!(
      local.translation.x > 10.0 && local.translation.y.abs() < 1e-2,
      "{local:?}"
    );
  }

  #[test]
  fn a_body_under_a_moving_body_writes_back_against_its_new_pose() {
    let (mut world, mut s) = world(60);
    no_gravity(&mut world);
    let parent = world
      .spawn((
        Transform::IDENTITY,
        RigidBody::dynamic(),
        Velocity::new(60.0, 0.0),
        Collider::circle(5.0),
      ))
      .id();
    let child = world
      .spawn((
        Transform::from_xy(100.0, 0.0),
        RigidBody::dynamic(),
        Collider::circle(5.0),
        ChildOf(parent),
      ))
      .id();
    run(&mut world, &mut s, 60);
    let physics = world.resource::<Physics>();
    let (parent_at, _) = physics.body_pose(parent).unwrap();
    let (child_at, _) = physics.body_pose(child).unwrap();
    assert!((parent_at.x - 60.0).abs() < 0.5, "{parent_at}");
    assert!(
      (child_at - Vec2::new(100.0, 0.0)).length() < 1e-3,
      "the child stayed put in the world: {child_at}"
    );
    assert!(
      physics.is_sleeping(child),
      "its parent's motion does not drag or wake it"
    );
    let local = at(&world, child).translation;
    assert!(
      (local - (child_at - parent_at)).length() < 1e-3,
      "{local} vs {}",
      child_at - parent_at
    );
  }

  #[test]
  fn a_body_two_levels_below_a_moving_body_stays_put() {
    let (mut world, mut s) = world(60);
    no_gravity(&mut world);
    let a = world
      .spawn((
        Transform::IDENTITY,
        RigidBody::dynamic(),
        Velocity::new(60.0, 0.0),
        Collider::circle(5.0),
      ))
      .id();
    let plain = world.spawn((Transform::IDENTITY, ChildOf(a))).id();
    let b = world
      .spawn((
        Transform::from_xy(100.0, 0.0),
        RigidBody::dynamic(),
        Collider::circle(5.0),
        ChildOf(plain),
      ))
      .id();
    run(&mut world, &mut s, 60);
    let physics = world.resource::<Physics>();
    let (at_b, _) = physics.body_pose(b).unwrap();
    assert!(
      (at_b - Vec2::new(100.0, 0.0)).length() < 1e-3,
      "not dragged by the body two levels up: {at_b}"
    );
    assert!(physics.is_sleeping(b));
  }

  /// A body 100 px from a parent body turned from `start` radians at
  /// `spin` rad/s for five seconds, then left to rest (a dynamic parent is
  /// stopped, a kinematic one keeps turning): whether the child sleeps, and
  /// how far it drifts in the world over the last ten seconds.
  fn under_a_turned_parent(parent: RigidBody, start: f32, spin: f32) -> (bool, f32) {
    let (mut world, mut s) = world(60);
    no_gravity(&mut world);
    let kinematic = parent.kind == BodyKind::Kinematic;
    let parent = world
      .spawn((
        Transform::IDENTITY.with_rotation(start),
        parent,
        AngularVelocity(spin),
        Collider::circle(5.0),
      ))
      .id();
    let child = world
      .spawn((
        Transform::from_xy(100.0, 0.0),
        RigidBody::dynamic(),
        Collider::circle(5.0),
        ChildOf(parent),
      ))
      .id();
    run(&mut world, &mut s, 300);
    if !kinematic {
      *world.get_mut::<AngularVelocity>(parent).unwrap() = AngularVelocity(0.0);
    }
    run(&mut world, &mut s, 120);
    let before = world.resource::<Physics>().body_pose(child).unwrap().0;
    run(&mut world, &mut s, 600);
    let physics = world.resource::<Physics>();
    let after = physics.body_pose(child).unwrap().0;
    (physics.is_sleeping(child), (after - before).length())
  }

  #[test]
  fn a_body_under_a_parent_stopped_after_thousands_of_radians_stays_put_and_sleeps() {
    // f32 round-off of a large continuous rotation differs from Rapier's
    // wrapped angle: the write-back must place the child against the same
    // parent pose propagation computes. From 2010.234 the parent comes to
    // rest at an angle where writing its rotation twice moves it one ulp
    for start in [3000.0, 1000.0, 10000.0, -5000.0, 2010.234] {
      let (asleep, drift) = under_a_turned_parent(RigidBody::dynamic(), start, 7.0);
      assert!(drift < 1e-3, "from {start} rad: drifted {drift} px");
      assert!(asleep, "from {start} rad: never slept");
    }
  }

  #[test]
  fn a_body_under_a_kinematic_parent_turning_past_thousands_of_radians_stays_put() {
    let (asleep, drift) = under_a_turned_parent(RigidBody::kinematic(), 3000.0, 0.5);
    assert!(drift < 1e-3, "drifted {drift} px");
    assert!(asleep, "never slept");
  }

  #[test]
  fn a_mirrored_body_keeps_its_rotation_and_sleeps() {
    for scale in [
      Vec2::new(-1.0, 1.0),
      Vec2::new(1.0, -1.0),
      Vec2::new(-1.0, -1.0),
    ] {
      let (mut world, mut s) = world(60);
      floor(&mut world, 100.0);
      let e = world
        .spawn((
          Transform::from_xy(0.0, 90.0).with_scale(scale),
          RigidBody::dynamic().lock_rotation(),
          Collider::rect(20.0, 20.0),
        ))
        .id();
      run(&mut world, &mut s, 600);
      let physics = world.resource::<Physics>();
      let (_, rotation) = physics.body_pose(e).unwrap();
      assert!(rotation.abs() < 1e-4, "{scale}: simulated at {rotation}");
      let local = at(&world, e).rotation;
      assert!(local.abs() < 1e-4, "{scale}: written back as {local}");
      assert!(physics.is_sleeping(e), "{scale}: never flipped back");
    }
  }

  /// The simulated angle and the local rotation of a body spinning at
  /// 3 rad/s for a second under a parent scaled by `scale`.
  fn spun_under(scale: Vec2) -> (f32, f32) {
    let (mut world, mut s) = world(60);
    no_gravity(&mut world);
    let parent = world.spawn(Transform::IDENTITY.with_scale(scale)).id();
    let child = world
      .spawn((
        Transform::IDENTITY,
        RigidBody::dynamic(),
        AngularVelocity(3.0),
        Collider::circle(5.0),
        ChildOf(parent),
      ))
      .id();
    run(&mut world, &mut s, 60);
    let (_, rotation) = world.resource::<Physics>().body_pose(child).unwrap();
    (rotation, at(&world, child).rotation)
  }

  #[test]
  fn a_turning_body_under_a_mirrored_parent_keeps_turning() {
    // The mirror maps the local x axis to the world's -x: it starts at PI
    let (rotation, local) = spun_under(Vec2::new(-1.0, 1.0));
    assert!(
      angle_delta(rotation, std::f32::consts::PI + 3.0).abs() < 0.05,
      "{rotation}"
    );
    // Turning one way in the world is turning the other way in the mirror
    assert!((local + 3.0).abs() < 0.05, "{local}");
  }

  #[test]
  fn a_spinning_body_under_a_stretched_parent_is_not_pulled_back() {
    let (rotation, _) = spun_under(Vec2::new(2.0, 1.0));
    assert!(angle_delta(rotation, 3.0).abs() < 0.05, "{rotation}");
  }

  #[test]
  fn a_turned_body_at_rest_under_a_stretched_parent_sleeps() {
    let (mut world, mut s) = world(60);
    no_gravity(&mut world);
    let parent = world
      .spawn(Transform::IDENTITY.with_scale(Vec2::new(2.0, 1.0)))
      .id();
    let child = world
      .spawn((
        Transform::IDENTITY.with_rotation(0.7),
        RigidBody::dynamic(),
        Collider::circle(5.0),
        ChildOf(parent),
      ))
      .id();
    run(&mut world, &mut s, 600);
    assert!(world.resource::<Physics>().is_sleeping(child));
    assert!((at(&world, child).rotation - 0.7).abs() < 1e-4);
  }

  #[test]
  fn a_zero_scale_parent_never_writes_a_non_finite_transform() {
    let (mut world, mut s) = world(60);
    let parent = world
      .spawn(Transform::from_xy(10.0, 0.0).with_scale(Vec2::ZERO))
      .id();
    let child = world
      .spawn((
        Transform::from_xy(5.0, 5.0),
        RigidBody::dynamic(),
        Collider::circle(5.0),
        ChildOf(parent),
      ))
      .id();
    run(&mut world, &mut s, 10);
    let local = at(&world, child);
    assert!(
      local.translation.is_finite() && local.rotation.is_finite(),
      "{local:?}"
    );
  }

  #[test]
  fn a_collider_waiting_for_its_new_body_leaves_the_old_one() {
    let (mut world, mut s) = world(60);
    no_gravity(&mut world);
    let old = world
      .spawn((Transform::IDENTITY, RigidBody::dynamic()))
      .id();
    let new = world
      .spawn((
        Transform::from_xy(200.0, 0.0),
        RigidBody::dynamic(),
        Disabled,
      ))
      .id();
    let part = world
      .spawn((
        Transform::IDENTITY,
        Collider::rect(100.0, 100.0),
        ChildOf(old),
      ))
      .id();
    run(&mut world, &mut s, 1);
    world.entity_mut(part).insert(ChildOf(new));
    run(&mut world, &mut s, 1);
    let physics = world.resource::<Physics>();
    assert!(physics.mass(old).unwrap() < 1e-3, "it left the old body");
    assert_eq!(
      physics.collider_count(),
      0,
      "and waits out of the simulation"
    );
    world.entity_mut(new).remove::<Disabled>();
    run(&mut world, &mut s, 2);
    let mass = world.resource::<Physics>().mass(new).unwrap();
    assert!((mass - 1.0).abs() < 1e-3, "then joins the new one: {mass}");
  }

  #[test]
  fn a_scaled_entity_s_collider_scales() {
    let (mut world, mut s) = world(60);
    world.spawn((
      Transform::from_xy(0.0, 110.0).with_scale(Vec2::new(1.0, 2.0)),
      Collider::rect(400.0, 20.0),
    ));
    let b = ball(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, 120);
    // 40 high now: its top is at 90, so the ball rests at 80
    assert!(
      (at(&world, b).translation.y - 80.0).abs() < 2.0,
      "{:?}",
      at(&world, b)
    );
  }

  #[test]
  fn a_turned_entity_turns_its_collider() {
    let (mut world, mut s) = world(60);
    no_gravity(&mut world);
    world.spawn((
      Transform::from_xy(100.0, 0.0).with_rotation(FRAC_PI_2),
      Collider::rect(100.0, 10.0),
    ));
    let b = world
      .spawn((
        Transform::IDENTITY,
        RigidBody::dynamic().lock_rotation(),
        Velocity::new(200.0, 0.0),
        Collider::circle(10.0),
      ))
      .id();
    run(&mut world, &mut s, 60);
    assert!(
      at(&world, b).translation.x < 86.0,
      "the upright bar stopped it: {:?}",
      at(&world, b)
    );
  }

  #[test]
  fn a_zero_scale_leaves_the_collider_out_until_it_grows() {
    let (mut world, mut s) = world(60);
    let e = world
      .spawn((
        Transform::IDENTITY.with_scale(Vec2::ZERO),
        Collider::circle(10.0),
      ))
      .id();
    run(&mut world, &mut s, 2);
    assert_eq!(world.resource::<Physics>().collider_count(), 0);
    world.get_mut::<Transform>(e).unwrap().scale = Vec2::ONE;
    run(&mut world, &mut s, 1);
    assert_eq!(world.resource::<Physics>().collider_count(), 1);
    world.get_mut::<Transform>(e).unwrap().scale = Vec2::ZERO;
    run(&mut world, &mut s, 1);
    assert_eq!(world.resource::<Physics>().collider_count(), 0);
  }

  #[test]
  fn a_collider_shrunk_to_zero_again_and_again_warns_once() {
    // A pickup that tweens in from nothing every time it respawns
    let (mut world, mut s) = world(60);
    let e = world
      .spawn((
        Transform::IDENTITY.with_scale(Vec2::ZERO),
        Collider::circle(10.0),
      ))
      .id();
    for _ in 0..3 {
      run(&mut world, &mut s, 1);
      world.get_mut::<Transform>(e).unwrap().scale = Vec2::ONE;
      run(&mut world, &mut s, 1);
      assert_eq!(world.resource::<Physics>().collider_count(), 1);
      world.get_mut::<Transform>(e).unwrap().scale = Vec2::ZERO;
    }
    run(&mut world, &mut s, 1);
    assert_eq!(world.resource::<Physics>().warnings, 1);
  }

  #[test]
  fn entities_that_never_registered_leave_no_warning_behind() {
    let (mut world, mut s) = world(60);
    let body = world
      .spawn((Transform::from_xy(f32::NAN, 0.0), RigidBody::dynamic()))
      .id();
    let collider = world
      .spawn((Transform::IDENTITY, Collider::rect(0.0, 4.0)))
      .id();
    let joint = world.spawn(Joint::fixed(body, collider)).id();
    let removed = world
      .spawn((Transform::IDENTITY, Collider::rect(0.0, 4.0)))
      .id();
    run(&mut world, &mut s, 1);
    assert_eq!(world.resource::<Physics>().warned_count(), 4);
    world.despawn(body);
    world.despawn(collider);
    world.despawn(joint);
    world.entity_mut(removed).remove::<Collider>();
    run(&mut world, &mut s, 1);
    let physics = world.resource::<Physics>();
    assert_eq!(physics.warned_count(), 0);
    assert!(physics.refused.is_empty());
  }

  #[test]
  fn reparenting_a_collider_moves_its_mass() {
    let (mut world, mut s) = world(60);
    no_gravity(&mut world);
    let a = world
      .spawn((Transform::IDENTITY, RigidBody::dynamic()))
      .id();
    let b = world
      .spawn((Transform::from_xy(200.0, 0.0), RigidBody::dynamic()))
      .id();
    let part = world
      .spawn((
        Transform::IDENTITY,
        Collider::rect(100.0, 100.0),
        ChildOf(a),
      ))
      .id();
    run(&mut world, &mut s, 1);
    assert!((world.resource::<Physics>().mass(a).unwrap() - 1.0).abs() < 1e-3);
    world.entity_mut(part).insert(ChildOf(b));
    run(&mut world, &mut s, 1);
    let physics = world.resource::<Physics>();
    assert!(physics.mass(a).unwrap() < 1e-3);
    assert!((physics.mass(b).unwrap() - 1.0).abs() < 1e-3);
  }

  #[test]
  fn a_parent_losing_its_body_leaves_its_child_colliders_static() {
    let (mut world, mut s) = world(60);
    let platform = world
      .spawn((Transform::from_xy(0.0, 110.0), RigidBody::kinematic()))
      .id();
    world.spawn((
      Transform::IDENTITY,
      Collider::rect(400.0, 20.0),
      ChildOf(platform),
    ));
    run(&mut world, &mut s, 1);
    world.entity_mut(platform).remove::<RigidBody>();
    let b = ball(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, 120);
    assert!((at(&world, b).translation.y - 90.0).abs() < 2.0);
  }

  #[test]
  fn disabling_a_body_disables_its_child_colliders() {
    let (mut world, mut s) = world(60);
    let platform = world
      .spawn((Transform::from_xy(0.0, 110.0), RigidBody::fixed()))
      .id();
    world.spawn((
      Transform::IDENTITY,
      Collider::rect(400.0, 20.0),
      ChildOf(platform),
    ));
    run(&mut world, &mut s, 1);
    world.entity_mut(platform).insert(Disabled);
    let b = ball(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, 60);
    assert!(at(&world, b).translation.y > 150.0, "fell through");
  }

  /// A frame that runs no fixed step (a high refresh rate, slow motion, a
  /// pause): only the `Last` bookkeeping runs, then the trackers clear.
  fn frame_without_a_step(world: &mut World, s: &mut EngineSchedules) {
    s.run(Phase::Last, world);
    world.clear_trackers();
  }

  #[test]
  fn a_collider_whose_body_goes_between_steps_joins_the_body_above() {
    let (mut world, mut s) = world(60);
    s.add_systems(Phase::Last, (forget_removed, track_disabled).chain());
    no_gravity(&mut world);
    let outer = world
      .spawn((Transform::IDENTITY, RigidBody::dynamic()))
      .id();
    let inner = world
      .spawn((Transform::IDENTITY, RigidBody::dynamic(), ChildOf(outer)))
      .id();
    world.spawn((
      Transform::IDENTITY,
      Collider::rect(100.0, 100.0),
      ChildOf(inner),
    ));
    run(&mut world, &mut s, 1);
    assert!((world.resource::<Physics>().mass(inner).unwrap() - 1.0).abs() < 1e-3);
    world.entity_mut(inner).remove::<RigidBody>();
    // The removal events expire before the next step
    for _ in 0..3 {
      frame_without_a_step(&mut world, &mut s);
    }
    run(&mut world, &mut s, 1);
    let mass = world.resource::<Physics>().mass(outer).unwrap();
    assert!(
      (mass - 1.0).abs() < 1e-3,
      "the part joined the outer body: {mass}"
    );
  }

  #[test]
  fn a_collider_detached_between_steps_leaves_its_body() {
    let (mut world, mut s) = world(60);
    s.add_systems(Phase::Last, (forget_removed, track_disabled).chain());
    no_gravity(&mut world);
    let body = world
      .spawn((Transform::IDENTITY, RigidBody::dynamic()))
      .id();
    let part = world
      .spawn((
        Transform::IDENTITY,
        Collider::rect(100.0, 100.0),
        ChildOf(body),
      ))
      .id();
    run(&mut world, &mut s, 1);
    world.entity_mut(part).remove::<ChildOf>();
    for _ in 0..3 {
      frame_without_a_step(&mut world, &mut s);
    }
    run(&mut world, &mut s, 1);
    let physics = world.resource::<Physics>();
    assert!(physics.mass(body).unwrap() < 1e-3, "the part left the body");
    assert_eq!(
      physics.collider_entries[&part].body, None,
      "a fixed collider now"
    );
  }

  /// A fixed platform 400 wide whose top is at 100, its collider on a child
  /// (`(platform, part)`), disabled after a step.
  fn disabled_platform(world: &mut World, s: &mut EngineSchedules) -> (Entity, Entity) {
    let platform = world
      .spawn((Transform::from_xy(0.0, 110.0), RigidBody::fixed()))
      .id();
    let part = world
      .spawn((
        Transform::IDENTITY,
        Collider::rect(400.0, 20.0),
        ChildOf(platform),
      ))
      .id();
    run(world, s, 1);
    world.entity_mut(platform).insert(Disabled);
    run(world, s, 1);
    (platform, part)
  }

  #[test]
  fn a_collider_attached_to_a_disabled_body_is_disabled_with_it() {
    let (mut world, mut s) = world(60);
    let (platform, _) = disabled_platform(&mut world, &mut s);
    world.spawn((
      Transform::from_xy(0.0, -200.0),
      Collider::rect(400.0, 20.0),
      ChildOf(platform),
    ));
    // Falls through both, the new one 200 px above the first
    let dropped = ball(&mut world, 0.0, -300.0);
    run(&mut world, &mut s, 90);
    assert!(at(&world, dropped).translation.y > 150.0, "fell through");
    world.entity_mut(platform).remove::<Disabled>();
    let caught = ball(&mut world, 0.0, -300.0);
    run(&mut world, &mut s, 120);
    assert!(
      (at(&world, caught).translation.y + 110.0).abs() < 2.0,
      "{:?}",
      at(&world, caught)
    );
  }

  #[test]
  fn a_collider_taken_off_a_disabled_body_is_simulated() {
    let (mut world, mut s) = world(60);
    let (_, part) = disabled_platform(&mut world, &mut s);
    // Now a fixed collider at the origin: its top is at -10
    world.entity_mut(part).remove::<ChildOf>();
    let b = ball(&mut world, 0.0, -100.0);
    run(&mut world, &mut s, 120);
    assert!(
      (at(&world, b).translation.y + 20.0).abs() < 2.0,
      "{:?}",
      at(&world, b)
    );
  }

  #[test]
  fn a_disabled_body_s_colliders_stay_when_it_loses_its_body() {
    let (mut world, mut s) = world(60);
    let (platform, _) = disabled_platform(&mut world, &mut s);
    world.entity_mut(platform).remove::<RigidBody>();
    let b = ball(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, 120);
    assert!(
      (at(&world, b).translation.y - 90.0).abs() < 2.0,
      "{:?}",
      at(&world, b)
    );
  }

  fn fnv(hash: &mut u32, value: f32) {
    for byte in value.to_bits().to_le_bytes() {
      *hash ^= u32::from(byte);
      *hash = hash.wrapping_mul(0x0100_0193);
    }
  }

  /// What `mixed_scene` ended with.
  struct MixedRun {
    /// Every body's `Transform`, velocities, Rapier pose and sleep, hashed
    checksum: u32,
    asleep: usize,
    /// Bodies and colliders the sync compared with Rapier over the run
    compared: usize,
  }

  /// A scene through every sync path, run for 360 steps: bodies bouncing
  /// in a box (some spinning, some rotation-locked; they settle, are woken
  /// around step 200 and settle again), a body with a child collider, a
  /// body nested under a moving body, a kinematic body driven by velocity,
  /// a kinematic platform and a kinematic child the game moves, a pendulum
  /// on a joint, and game writes between steps (teleports, new velocities,
  /// a same-value write, a detach). Without `skip_own_writes` the
  /// write-back tick is forgotten before every step, so the sync compares
  /// everything by value.
  fn mixed_scene(skip_own_writes: bool) -> MixedRun {
    let (mut world, mut s) = world(60);
    s.add_systems(
      Phase::FixedUpdate,
      propagate_transforms.in_set(FixedSet::Transforms),
    );
    for (x, y, w, h) in [
      (150.0, -10.0, 340.0, 20.0),
      (150.0, 310.0, 340.0, 20.0),
      (-10.0, 150.0, 20.0, 340.0),
      (310.0, 150.0, 20.0, 340.0),
    ] {
      world.spawn((
        Transform::from_xy(x, y),
        RigidBody::fixed(),
        Collider::rect(w, h).with_restitution(0.8),
      ));
    }
    let mut bouncing = Vec::new();
    for i in 0..60 {
      let (col, row) = ((i % 10) as f32, (i / 10) as f32);
      let angle = i as f32 * 2.399;
      let body = if i % 3 == 0 {
        RigidBody::dynamic().lock_rotation()
      } else {
        RigidBody::dynamic()
      };
      let collider = if i % 2 == 0 {
        Collider::circle(6.0)
      } else {
        Collider::rect(10.0, 10.0)
      };
      let id = world
        .spawn((
          Transform::from_xy(30.0 + col * 24.0, 30.0 + row * 24.0),
          Velocity::new(angle.cos() * 150.0, angle.sin() * 150.0),
          body,
          collider.with_restitution(0.6).with_friction(0.4),
        ))
        .id();
      bouncing.push(id);
    }
    let carrier = world
      .spawn((
        Transform::from_xy(60.0, 250.0),
        RigidBody::dynamic(),
        Velocity::new(40.0, -80.0),
      ))
      .id();
    world.spawn((
      Transform::from_xy(8.0, 0.0),
      Collider::rect(12.0, 6.0),
      ChildOf(carrier),
    ));
    let parent = world
      .spawn((
        Transform::from_xy(200.0, 60.0),
        RigidBody::dynamic(),
        Velocity::new(-60.0, 30.0),
        Collider::circle(8.0),
      ))
      .id();
    let nested = world
      .spawn((
        Transform::from_xy(20.0, 0.0),
        RigidBody::dynamic(),
        Collider::circle(4.0),
        ChildOf(parent),
      ))
      .id();
    world.spawn((
      Transform::from_xy(20.0, 25.0),
      RigidBody::kinematic(),
      Velocity::new(50.0, 0.0),
      AngularVelocity(1.0),
      Collider::rect(30.0, 6.0),
    ));
    let platform = world
      .spawn((
        Transform::from_xy(150.0, 50.0),
        RigidBody::kinematic(),
        Collider::rect(60.0, 8.0),
      ))
      .id();
    let holder = world.spawn(Transform::from_xy(250.0, 60.0)).id();
    world.spawn((
      Transform::from_xy(0.0, 10.0),
      RigidBody::kinematic(),
      Collider::rect(8.0, 8.0),
      ChildOf(holder),
    ));
    let pivot = world
      .spawn((Transform::from_xy(150.0, 100.0), RigidBody::fixed()))
      .id();
    let bob = world
      .spawn((
        Transform::from_xy(180.0, 100.0),
        RigidBody::dynamic(),
        Collider::circle(5.0),
      ))
      .id();
    world.spawn(Joint::revolute(pivot, bob).at(Vec2::new(150.0, 100.0)));
    for step in 0..360 {
      let t = step as f32;
      world.get_mut::<Transform>(platform).unwrap().translation.x = 150.0 + 60.0 * (t * 0.05).sin();
      world.get_mut::<Transform>(holder).unwrap().translation.y = 60.0 + 10.0 * (t * 0.07).sin();
      match step {
        50 => {
          world.get_mut::<Transform>(bouncing[7]).unwrap().translation = Vec2::new(150.0, 150.0)
        }
        80 => *world.get_mut::<Velocity>(bouncing[11]).unwrap() = Velocity::new(0.0, -200.0),
        100 => {
          world.entity_mut(nested).remove::<ChildOf>();
        }
        120 => {
          let same = at(&world, bouncing[20]);
          *world.get_mut::<Transform>(bouncing[20]).unwrap() = same;
        }
        200 => {
          world
            .get_mut::<Transform>(bouncing[30])
            .unwrap()
            .translation = Vec2::new(150.0, 120.0)
        }
        210 => *world.get_mut::<Velocity>(bouncing[40]).unwrap() = Velocity::new(120.0, -150.0),
        _ => {}
      }
      if !skip_own_writes {
        world.resource_mut::<Physics>().write_tick = None;
      }
      run(&mut world, &mut s, 1);
    }
    let mut rows: Vec<_> = world
      .query::<(Entity, &Transform, &Velocity, &AngularVelocity, &RigidBody)>()
      .iter(&world)
      .map(|(entity, transform, velocity, angular, _)| (entity, *transform, *velocity, angular.0))
      .collect();
    rows.sort_by_key(|row| row.0);
    let physics = world.resource::<Physics>();
    let mut checksum = 0x811c_9dc5_u32;
    let mut asleep = 0;
    for (entity, transform, velocity, angular) in rows {
      let (pose, rotation) = physics.body_pose(entity).unwrap_or_default();
      let sleeping = physics.is_sleeping(entity);
      asleep += usize::from(sleeping);
      for value in [
        transform.translation.x,
        transform.translation.y,
        transform.rotation,
        velocity.dx,
        velocity.dy,
        angular,
        pose.x,
        pose.y,
        rotation,
        f32::from(u8::from(sleeping)),
      ] {
        fnv(&mut checksum, value);
      }
    }
    MixedRun {
      checksum,
      asleep,
      compared: physics.compared,
    }
  }

  #[test]
  fn the_write_back_skip_leaves_every_trajectory_as_it_was() {
    let (skipping, comparing) = (mixed_scene(true), mixed_scene(false));
    assert_eq!(
      skipping.checksum, comparing.checksum,
      "skipping the write-back's own changes must not change the simulation"
    );
    assert!(
      skipping.asleep > 50,
      "the scene settles: {}",
      skipping.asleep
    );
    assert!(
      skipping.compared * 3 < comparing.compared,
      "the skip skips: {} against {}",
      skipping.compared,
      comparing.compared
    );
  }

  /// Balls flying apart with no gravity, each with its own collider, and
  /// the engine's propagation after physics.
  fn flying(count: usize) -> (World, EngineSchedules, Vec<Entity>) {
    let (mut world, mut s) = world(60);
    no_gravity(&mut world);
    s.add_systems(
      Phase::FixedUpdate,
      propagate_transforms.in_set(FixedSet::Transforms),
    );
    let balls = (0..count)
      .map(|i| {
        let e = ball(&mut world, i as f32 * 40.0, 0.0);
        *world.get_mut::<Velocity>(e).unwrap() = Velocity::new(5.0 * i as f32, 20.0);
        e
      })
      .collect();
    (world, s, balls)
  }

  /// Run one step and return what the sync compared with Rapier in it.
  fn compared_in_a_step(world: &mut World, s: &mut EngineSchedules) -> usize {
    world.resource_mut::<Physics>().compared = 0;
    run(world, s, 1);
    world.resource::<Physics>().compared
  }

  #[test]
  fn what_the_write_back_wrote_is_not_compared_again() {
    let (mut world, mut s, balls) = flying(10);
    run(&mut world, &mut s, 3);
    assert_eq!(
      compared_in_a_step(&mut world, &mut s),
      0,
      "ten bodies and colliders moved by the write-back alone"
    );
    // A game write between steps: its body and its collider are compared
    world.get_mut::<Transform>(balls[3]).unwrap().translation.y += 50.0;
    assert_eq!(compared_in_a_step(&mut world, &mut s), 2);
    *world.get_mut::<Velocity>(balls[4]).unwrap() = Velocity::new(0.0, -30.0);
    assert_eq!(compared_in_a_step(&mut world, &mut s), 1);
    assert_eq!(
      world.get::<Velocity>(balls[4]),
      Some(&Velocity::new(0.0, -30.0))
    );
  }

  /// What the game writes in `write_after_physics`, once.
  #[derive(Resource, Default)]
  struct LateWrites {
    teleport: Option<(Entity, Vec2)>,
    velocity: Option<(Entity, Velocity)>,
    same: Option<Entity>,
  }

  /// A game system after physics in the same step: it writes on top of the
  /// write-back, at a later tick.
  fn write_after_physics(
    mut writes: ResMut<LateWrites>,
    mut bodies: Query<(&mut Transform, &mut Velocity)>,
  ) {
    if let Some((e, at)) = writes.teleport.take()
      && let Ok((mut transform, _)) = bodies.get_mut(e)
    {
      transform.translation = at;
    }
    if let Some((e, velocity)) = writes.velocity.take()
      && let Ok((_, mut v)) = bodies.get_mut(e)
    {
      *v = velocity;
    }
    if let Some(e) = writes.same.take()
      && let Ok((mut transform, _)) = bodies.get_mut(e)
    {
      transform.set_changed();
    }
  }

  #[test]
  fn a_game_write_after_the_write_back_in_the_same_step_is_pushed() {
    let (mut world, mut s, balls) = flying(3);
    world.insert_resource(LateWrites::default());
    s.add_systems(
      Phase::FixedUpdate,
      write_after_physics
        .after(FixedSet::Physics)
        .before(FixedSet::Transforms),
    );
    run(&mut world, &mut s, 3);
    let (teleported, sped, touched) = (balls[0], balls[1], balls[2]);
    let target = Vec2::new(-300.0, 200.0);
    *world.resource_mut::<LateWrites>() = LateWrites {
      teleport: Some((teleported, target)),
      velocity: Some((sped, Velocity::new(-90.0, 0.0))),
      same: Some(touched),
    };
    // The write-back lands, then the game's writes on the same bodies
    run(&mut world, &mut s, 1);
    assert_eq!(at(&world, teleported).translation, target);
    world.resource_mut::<Physics>().compared = 0;
    run(&mut world, &mut s, 1);
    let physics = world.resource::<Physics>();
    let (position, _) = physics.body_pose(teleported).unwrap();
    let expected = target + Vec2::new(0.0, 20.0) / 60.0;
    assert!((position - expected).length() < 1e-3, "{position}");
    assert_eq!(
      world.get::<Velocity>(sped),
      Some(&Velocity::new(-90.0, 0.0))
    );
    // Teleported: body and collider; the new velocity; touched: body and
    // collider, compared and left alone
    assert_eq!(physics.compared, 5);
    // A same-value write moves nothing: it keeps its course over 5 steps
    let (course, _) = physics.body_pose(touched).unwrap();
    let expected = Vec2::new(80.0, 0.0) + Vec2::new(10.0, 20.0) * 5.0 / 60.0;
    assert!((course - expected).length() < 1e-3, "{course}");
  }

  /// A body flying under a plain parent 100 px to the right.
  fn flying_under_a_parent() -> (World, EngineSchedules, Entity) {
    let (mut world, mut s) = world(60);
    no_gravity(&mut world);
    let parent = world.spawn(Transform::from_xy(100.0, 0.0)).id();
    let body = world
      .spawn((
        Transform::IDENTITY,
        RigidBody::dynamic(),
        Velocity::new(30.0, 0.0),
        Collider::circle(5.0),
        ChildOf(parent),
      ))
      .id();
    run(&mut world, &mut s, 3);
    (world, s, body)
  }

  #[test]
  fn a_body_detached_after_the_write_back_moves_to_its_new_world_pose() {
    let (mut world, mut s, body) = flying_under_a_parent();
    // Its local `Transform`, last written by the write-back, is now its
    // world pose: 100 px to the left
    world.entity_mut(body).remove::<ChildOf>();
    let local = at(&world, body).translation;
    run(&mut world, &mut s, 1);
    let (position, _) = world.resource::<Physics>().body_pose(body).unwrap();
    assert!(
      (position - (local + Vec2::new(0.5, 0.0))).length() < 1e-3,
      "{position}"
    );
  }

  #[test]
  fn a_body_detached_between_steps_moves_to_its_new_world_pose() {
    let (mut world, mut s, body) = flying_under_a_parent();
    s.add_systems(Phase::Last, (forget_removed, track_disabled).chain());
    world.entity_mut(body).remove::<ChildOf>();
    let local = at(&world, body).translation;
    // Only the frame-end bookkeeping sees the removal before it expires
    frame_without_a_step(&mut world, &mut s);
    frame_without_a_step(&mut world, &mut s);
    run(&mut world, &mut s, 1);
    let (position, _) = world.resource::<Physics>().body_pose(body).unwrap();
    assert!(
      (position - (local + Vec2::new(0.5, 0.0))).length() < 1e-3,
      "{position}"
    );
  }

  #[test]
  fn a_velocity_driven_kinematic_body_skipped_by_the_sync_keeps_moving() {
    let (mut world, mut s) = world(60);
    s.add_systems(
      Phase::FixedUpdate,
      propagate_transforms.in_set(FixedSet::Transforms),
    );
    let e = world
      .spawn((
        Transform::IDENTITY,
        RigidBody::kinematic(),
        Velocity::new(60.0, 0.0),
        AngularVelocity(1.0),
        Collider::rect(10.0, 10.0),
      ))
      .id();
    run(&mut world, &mut s, 30);
    assert_eq!(
      compared_in_a_step(&mut world, &mut s),
      0,
      "its write-back is not compared again"
    );
    let moved = at(&world, e);
    assert!((moved.translation.x - 31.0).abs() < 1e-3, "{moved:?}");
    assert!((moved.rotation - 31.0 / 60.0).abs() < 1e-4, "{moved:?}");
    // A `Transform` write wins over its velocity for that step
    world.get_mut::<Transform>(e).unwrap().translation.x = 200.0;
    run(&mut world, &mut s, 1);
    let (position, _) = world.resource::<Physics>().body_pose(e).unwrap();
    assert!((position.x - 200.0).abs() < 1e-3, "{position}");
    run(&mut world, &mut s, 1);
    assert!((at(&world, e).translation.x - 201.0).abs() < 1e-3);
  }

  #[test]
  fn a_nested_body_is_always_compared_and_keeps_its_own_course() {
    let (mut world, mut s) = world(60);
    no_gravity(&mut world);
    s.add_systems(
      Phase::FixedUpdate,
      propagate_transforms.in_set(FixedSet::Transforms),
    );
    let parent = world
      .spawn((
        Transform::IDENTITY,
        RigidBody::dynamic(),
        Velocity::new(60.0, 0.0),
        Collider::circle(5.0),
      ))
      .id();
    let child = world
      .spawn((
        Transform::from_xy(100.0, 0.0),
        RigidBody::dynamic(),
        Velocity::new(0.0, 30.0),
        Collider::circle(5.0),
        ChildOf(parent),
      ))
      .id();
    run(&mut world, &mut s, 60);
    // The parent and its collider were moved by the write-back alone; the
    // child's world pose depends on its parent, so it and its collider are
    // compared, and left where Rapier holds them
    assert_eq!(compared_in_a_step(&mut world, &mut s), 2);
    let physics = world.resource::<Physics>();
    let (child_at, _) = physics.body_pose(child).unwrap();
    assert!(
      (child_at - Vec2::new(100.0, 30.5)).length() < 1e-3,
      "not dragged by its parent: {child_at}"
    );
  }

  #[test]
  fn a_moving_body_s_edited_collider_applies() {
    // The write-back moves it every step; the edit lands on top
    let (mut world, mut s, balls) = flying(1);
    run(&mut world, &mut s, 3);
    let before = world.resource::<Physics>().mass(balls[0]).unwrap();
    world.get_mut::<Collider>(balls[0]).unwrap().density = 2.0;
    run(&mut world, &mut s, 1);
    let after = world.resource::<Physics>().mass(balls[0]).unwrap();
    assert!((after - 2.0 * before).abs() < 1e-6, "{before} -> {after}");
  }

  #[test]
  fn a_moving_nested_body_s_collider_follows_its_parent_s_scale() {
    let (mut world, mut s, body) = flying_under_a_parent();
    let before = world.resource::<Physics>().mass(body).unwrap();
    let parent = world.get::<ChildOf>(body).unwrap().parent();
    world.get_mut::<Transform>(parent).unwrap().scale = Vec2::splat(2.0);
    run(&mut world, &mut s, 1);
    let after = world.resource::<Physics>().mass(body).unwrap();
    assert!((after - 4.0 * before).abs() < 1e-6, "{before} -> {after}");
  }

  /// What the game defers in `defer_writes`, once.
  #[derive(Resource, Default)]
  struct DeferredWrites {
    transform: Option<(Entity, Transform)>,
    velocity: Option<(Entity, Velocity)>,
  }

  /// A game system between the step and the write-back that defers its
  /// writes: they land at the sync point right before `read_back`.
  fn defer_writes(mut writes: ResMut<DeferredWrites>, mut commands: Commands) {
    if let Some((e, transform)) = writes.transform.take() {
      commands.entity(e).insert(transform);
    }
    if let Some((e, velocity)) = writes.velocity.take() {
      commands.entity(e).insert(velocity);
    }
  }

  #[test]
  fn a_command_applied_right_before_the_write_back_is_pushed() {
    let (mut world, mut s, sleeper) = sleeping_ball();
    let platform = world
      .spawn((
        Transform::from_xy(200.0, 0.0),
        RigidBody::kinematic(),
        Collider::rect(10.0, 10.0),
      ))
      .id();
    world.insert_resource(DeferredWrites::default());
    s.add_systems(
      Phase::FixedUpdate,
      defer_writes.after(step_simulation).before(read_back),
    );
    run(&mut world, &mut s, 1);
    // Neither body is written back: one is asleep, the other is not driven
    *world.resource_mut::<DeferredWrites>() = DeferredWrites {
      transform: Some((platform, Transform::from_xy(250.0, 0.0))),
      velocity: Some((sleeper, Velocity::new(60.0, 0.0))),
    };
    run(&mut world, &mut s, 1);
    assert_eq!(at(&world, platform).translation, Vec2::new(250.0, 0.0));
    let before = at(&world, sleeper).translation;
    run(&mut world, &mut s, 1);
    let physics = world.resource::<Physics>();
    let (position, _) = physics.body_pose(platform).unwrap();
    assert!((position.x - 250.0).abs() < 1e-3, "{position}");
    assert!(!physics.is_sleeping(sleeper), "the new velocity wakes it");
    let moved = at(&world, sleeper).translation - before;
    assert!((moved - Vec2::new(1.0, 0.0)).length() < 1e-3, "{moved}");
  }

  /// What `write_in_chain` writes, once.
  #[derive(Resource, Default)]
  struct InChainWrites {
    scale: Option<(Entity, f32)>,
    velocity: Option<(Entity, Velocity)>,
  }

  /// A game system the scheduler put inside the physics chain, after its
  /// propagation and collider sync: what it writes is not in this step's
  /// world poses, and the write-back then stamps the same components.
  fn write_in_chain(
    mut writes: ResMut<InChainWrites>,
    mut bodies: Query<(&mut Transform, &mut Velocity)>,
  ) {
    if let Some((e, scale)) = writes.scale.take()
      && let Ok((mut transform, _)) = bodies.get_mut(e)
    {
      transform.scale = Vec2::splat(scale);
    }
    if let Some((e, velocity)) = writes.velocity.take()
      && let Ok((_, mut v)) = bodies.get_mut(e)
    {
      *v = velocity;
    }
  }

  /// Two flying balls; a system inside the chain doubles the first's scale
  /// and sets the second's velocity. The first's mass before and after, and
  /// both bodies' Rapier poses and velocities three steps on.
  fn written_in_chain(skip_own_writes: bool) -> (f32, f32, Vec<(Vec2, Velocity)>) {
    let (mut world, mut s, balls) = flying(2);
    world.insert_resource(InChainWrites::default());
    s.add_systems(
      Phase::FixedUpdate,
      write_in_chain.after(sync_colliders).before(read_back),
    );
    let steps = |world: &mut World, s: &mut EngineSchedules, count: usize| {
      for _ in 0..count {
        if !skip_own_writes {
          world.resource_mut::<Physics>().write_tick = None;
        }
        run(world, s, 1);
      }
    };
    steps(&mut world, &mut s, 3);
    let before = world.resource::<Physics>().mass(balls[0]).unwrap();
    *world.resource_mut::<InChainWrites>() = InChainWrites {
      scale: Some((balls[0], 2.0)),
      velocity: Some((balls[1], Velocity::new(-50.0, 0.0))),
    };
    steps(&mut world, &mut s, 3);
    let physics = world.resource::<Physics>();
    let after = physics.mass(balls[0]).unwrap();
    let states = balls
      .iter()
      .map(|&e| {
        (
          physics.body_pose(e).unwrap().0,
          *world.get::<Velocity>(e).unwrap(),
        )
      })
      .collect();
    (before, after, states)
  }

  #[test]
  fn a_scale_written_inside_the_physics_chain_reaches_the_collider() {
    let (before, after, _) = written_in_chain(true);
    assert!((after - 4.0 * before).abs() < 1e-6, "{before} -> {after}");
  }

  #[test]
  fn writes_inside_the_physics_chain_end_as_without_the_skip() {
    assert_eq!(written_in_chain(true), written_in_chain(false));
  }
}
