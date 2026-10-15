//! The character controller: a kinematic body swept each step (Rapier's
//! `KinematicCharacterController`, through the 2D port in `sweep.rs`), with
//! floors, walls and ceilings told apart and `Velocity` left as the real
//! velocity, carried by what it stands on and pushing dynamic bodies.

use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI};

use rapier2d::control::{
  CharacterAutostep, CharacterCollision, CharacterLength, KinematicCharacterController,
};
use rapier2d::parry::query::{QueryDispatcher, ShapeCastHit, ShapeCastStatus};
use rapier2d::prelude::{
  Collider as RapierCollider, ColliderHandle, Pose, QueryFilter as RapierFilter, QueryPipeline,
  Shape, Vector,
};

use super::body::RigidBody;
use super::convert::{from_rapier, to_rapier};
use super::world::Physics;
use super::{one_way, sweep};
use crate::core::ecs::{
  Component, DetectChangesMut, Entity, Knockback, Query, Res, ResMut, Transform, Vec2, Velocity,
};
use crate::core::time::Time;

/// How a character's surfaces are told apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CharacterMode {
  /// Side view: up is -Y. Floors, walls and ceilings; snapping and steps.
  #[default]
  Platformer,
  /// Seen from above: no up. Every surface is a wall; no snapping or steps.
  TopDown,
}

fn character_body() -> RigidBody {
  RigidBody::kinematic().lock_rotation()
}

/// Makes an entity a character: a kinematic body (added when it has none)
/// the engine sweeps by its `Velocity` every step, sliding along what it
/// hits. The `Collider` on the same entity is the swept shape; it collides
/// with what that collider's layer and mask allow, and triggers never block
/// it.
///
/// `Velocity` is what the game wants (its gravity, run and jump, or a
/// preset's); after the step it is the real velocity: landing zeroes the
/// fall, a head bump stops the rise, a wall stops the run.
/// `CharacterState` says what the move touched. Writing `Transform`
/// teleports the character: it is put there without a sweep, forgets what
/// it stood on, and moves on by its `Velocity` from there in the same step
/// (so a respawn onto a floor reports `landed` on that step).
///
/// A character rides what it stands on: a platform moved by `Transform` or
/// by velocity, a turning one, or a dynamic body. Leaving it (a jump, a step
/// off the edge, the platform despawned) adds the platform's velocity to
/// `Velocity`, so the momentum is kept; landing on a rising platform takes
/// its rise off again.
#[derive(Component, Debug, Clone, PartialEq)]
#[non_exhaustive]
#[require(Transform, Velocity, CharacterState, RigidBody = character_body())]
pub struct CharacterController {
  pub mode: CharacterMode,
  /// The steepest floor, in radians (default 45°): steeper surfaces are
  /// walls, and the character slides down them
  pub max_slope: f32,
  /// Walk up ledges this high, in px (default off; platformer only)
  pub step_height: Option<f32>,
  /// Keep to the ground down slopes and over bumps within this distance, in
  /// px; `None` is a fifth of the collider's height, 0 turns it off
  /// (platformer only)
  pub snap_to_ground: Option<f32>,
  /// The gap kept from surfaces, in px; `None` is 1% of the collider's height
  pub skin: Option<f32>,
  /// Push dynamic bodies it walks into, with a mass from its collider's
  /// density (default on)
  pub push: bool,
  /// Drop through the one-way platform underfoot: one-way platforms are
  /// ignored until the character is fully below it, lands on something
  /// solid or is teleported, and the engine clears the flag then (at once,
  /// ignoring nothing, when it is not standing on a one-way platform)
  pub drop_through: bool,
}

impl CharacterController {
  /// A side-view character: floors, walls, ceilings, slopes up to 45°.
  pub fn platformer() -> Self {
    Self {
      mode: CharacterMode::Platformer,
      max_slope: FRAC_PI_4,
      step_height: None,
      snap_to_ground: None,
      skin: None,
      push: true,
      drop_through: false,
    }
  }

  /// A top-down character: every surface is a wall it slides along.
  pub fn top_down() -> Self {
    Self {
      mode: CharacterMode::TopDown,
      ..Self::platformer()
    }
  }

  /// The steepest floor, in degrees.
  pub fn with_max_slope(mut self, degrees: f32) -> Self {
    self.max_slope = degrees.to_radians();
    self
  }

  /// Walk up ledges up to `px` high.
  pub fn with_step_height(mut self, px: f32) -> Self {
    self.step_height = Some(px);
    self
  }

  /// Keep to the ground within `px` (0 turns snapping off).
  pub fn with_snap_to_ground(mut self, px: f32) -> Self {
    self.snap_to_ground = Some(px);
    self
  }

  /// Keep `px` from surfaces.
  pub fn with_skin(mut self, px: f32) -> Self {
    self.skin = Some(px);
    self
  }

  /// Push dynamic bodies (default) or be blocked by them.
  pub fn with_push(mut self, push: bool) -> Self {
    self.push = push;
    self
  }
}

impl Default for CharacterController {
  fn default() -> Self {
    Self::platformer()
  }
}

/// Something a character's move touched: the collider and its body, the
/// world point, and the surface normal there (out of the surface, toward
/// the character).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CharacterHit {
  pub entity: Entity,
  pub body: Entity,
  pub point: Vec2,
  pub normal: Vec2,
}

/// What a character's last move found, written by the engine every step.
/// Normals point out of the surface: flat ground is `(0, -1)`.
#[derive(Component, Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct CharacterState {
  pub on_floor: bool,
  pub on_wall: bool,
  pub on_ceiling: bool,
  /// Zero off the floor
  pub floor_normal: Vec2,
  /// Zero off any wall
  pub wall_normal: Vec2,
  /// The body (or bare collider) underfoot
  pub floor: Option<Entity>,
  /// Whether the floor is a one-way platform
  pub floor_one_way: bool,
  /// How fast the floor moves where the character stands, in px/s (zero off
  /// the floor); added to `Velocity` on the step it leaves the floor
  pub platform_velocity: Vec2,
  /// True on the step the character lands
  pub landed: bool,
  /// True on the step it leaves the floor
  pub left_floor: bool,
  /// On a slope steeper than `max_slope`, sliding down
  pub sliding_down_slope: bool,
  /// The `Velocity` the step asked for, before anything clipped it (for bounces)
  pub velocity_before: Vec2,
  /// Everything the move touched
  pub hits: Vec<CharacterHit>,
  /// Where the floor was touched, moved on by the floor's motion over the
  /// step: where the next step's carry starts
  pub(crate) floor_point: Vec2,
  /// The collider underfoot (drop-through starts from it)
  pub(crate) floor_collider: Option<Entity>,
  /// The last move ended inside or within the skin of something solid,
  /// is still pushing out, or the character is new: the next move pushes
  /// out first. A teleport sets it too, so the teleport step's own move
  /// pushes out first
  pub(crate) stuck: bool,
  /// The one-way collider a drop-through started from
  pub(crate) drop_from: Option<Entity>,
}

impl Default for CharacterState {
  fn default() -> Self {
    Self {
      on_floor: false,
      on_wall: false,
      on_ceiling: false,
      floor_normal: Vec2::ZERO,
      wall_normal: Vec2::ZERO,
      floor: None,
      floor_one_way: false,
      platform_velocity: Vec2::ZERO,
      landed: false,
      left_floor: false,
      sliding_down_slope: false,
      velocity_before: Vec2::ZERO,
      hits: Vec::new(),
      floor_point: Vec2::ZERO,
      floor_collider: None,
      stuck: true,
      drop_from: None,
    }
  }
}

impl CharacterState {
  /// In the air, touching nothing, not dropping, and pushing out first: the
  /// state a teleport's move starts from.
  fn teleported(&mut self) {
    let mut hits = std::mem::take(&mut self.hits);
    hits.clear();
    *self = Self {
      hits,
      velocity_before: self.velocity_before,
      ..Self::default()
    };
  }
}

/// What `Physics::move_character` reports back to the system.
pub(crate) struct CharacterMove {
  /// The normals of every surface the move touched (for clipping knockback)
  pub(crate) normals: Vec<Vec2>,
  /// A drop-through is over
  pub(crate) drop_done: bool,
}

/// How far below its feet a character looks for the floor, beyond its skin.
const FLOOR_PROBE: f32 = 2.0;
/// Colliders whose centres are closer than this, in px, are on one point.
const COINCIDENT: f32 = 0.01;
/// How far, in px along each axis, a push-out first moves a character off
/// another one on its point.
const NUDGE: f32 = 0.05;
/// Slack on the floor and ceiling angle tests.
const ANGLE_SLACK: f32 = 1e-4;

/// The least `normal · up` of a floor (and of a ceiling, against down).
fn floor_cos(max_slope: f32) -> f32 {
  max_slope.cos() - ANGLE_SLACK
}

/// `velocity` without what runs into a surface with this normal. With an
/// `up`, a slope too steep to climb stops the run into it instead of turning
/// it into a climb: the clip may not make the character rise faster than it
/// already did, so then only the run across is taken out.
fn clip(velocity: Vec2, normal: Vec2, up: Option<Vec2>) -> Vec2 {
  let into = velocity.dot(normal);
  if into >= 0.0 {
    return velocity;
  }
  let clipped = velocity - normal * into;
  match up {
    Some(up) if clipped.dot(up) > velocity.dot(up).max(0.0) => {
      let across = (normal - up * normal.dot(up)).normalize_or_zero();
      velocity - across * velocity.dot(across).min(0.0)
    }
    _ => clipped,
  }
}

/// The real velocity after a move: each wall and ceiling normal takes out
/// what runs into it (a slope too steep to climb stops the run, see `clip`),
/// and on a floor (`normals` holds up) the fall stops, the vertical part
/// only, so standing on a gentle slope does not creep down it. `momentum` is
/// the velocity of a floor just left, which the character keeps.
fn real_velocity(wanted: Vec2, normals: &[Vec2], up: Option<Vec2>, momentum: Vec2) -> Vec2 {
  normals
    .iter()
    .fold(wanted, |real, normal| clip(real, *normal, up))
    + momentum
}

/// How far `shape` at `pose` reaches across up (along X): its least and
/// greatest x. `None` for a shape with no support map (a polyline, a
/// compound: their parts have one).
fn across(shape: &dyn Shape, pose: &Pose) -> Option<(f32, f32)> {
  let support = shape.as_support_map()?;
  Some((
    support.support_point(pose, -Vector::X).x,
    support.support_point(pose, Vector::X).x,
  ))
}

/// What a character's sweep of `shape` from `start` may hit: colliders
/// enabled this tick (one disabled this tick stays in the broad phase until
/// the step, and Rapier's filter never checks), one-way ones only when the
/// character starts entirely on their solid side, and none of those while
/// it drops through.
fn solid(
  shape: &dyn Shape,
  start: Pose,
  dropping: bool,
) -> impl Fn(ColliderHandle, &RapierCollider) -> bool + '_ {
  move |_, collider| {
    collider.is_enabled()
      && match one_way::decode(collider.user_data) {
        None => true,
        Some(_) if dropping => false,
        Some(local) => one_way::character_above(collider, local, shape, &start),
      }
  }
}

impl Physics {
  /// How far the body under a character moves the point `at` this step:
  /// a kinematic body from its pose to its next pose (moved by `Transform`
  /// or velocity, turning included), a dynamic body by its velocity there,
  /// anything else (fixed, gone, disabled) not at all.
  pub(crate) fn floor_motion(&self, floor: Entity, at: Vec2, dt: f32) -> Vec2 {
    let Some(body) = self
      .body_entries
      .get(&floor)
      .and_then(|entry| self.bodies.get(entry.handle))
      .filter(|body| body.is_enabled())
    else {
      return Vec2::ZERO;
    };
    if body.is_kinematic() {
      let point = to_rapier(at);
      let now = body.position();
      let next = body.next_position();
      from_rapier(next.transform_point(now.inverse_transform_point(point))) - at
    } else if body.is_dynamic() {
      from_rapier(body.velocity_at_point(to_rapier(at))) * dt
    } else {
      Vec2::ZERO
    }
  }

  /// Sweep one character by `velocity` plus `extra` (knockback) over `dt`;
  /// update its state and leave the real velocity in `velocity`. A
  /// character the game teleported this step starts from where it was put,
  /// with its state reset. `None` when it is not moved: not registered yet,
  /// disabled, or refused (logged once).
  pub(crate) fn move_character(
    &mut self,
    entity: Entity,
    controller: &CharacterController,
    state: &mut CharacterState,
    velocity: &mut Vec2,
    extra: Vec2,
    dt: f32,
  ) -> Option<CharacterMove> {
    let body_handle = self.body_entries.get(&entity)?.handle;
    let body = self.bodies.get(body_handle)?;
    if !body.is_kinematic() {
      self.warn_once(
        entity,
        "a CharacterController needs a kinematic body (one is added when there is none); this one is not moved",
      );
      return None;
    }
    if !body.is_enabled() {
      return None;
    }
    let Some(collider_handle) = self.collider_entries.get(&entity).map(|entry| entry.handle) else {
      self.warn_once(
        entity,
        "a CharacterController needs a Collider on its own entity",
      );
      return None;
    };
    // A game write to its `Transform` put the body's next pose somewhere
    // new: the character forgets where it was and moves on from there
    let teleported = self.was_teleported(entity);
    if teleported {
      state.teleported();
    }
    let mut wanted = *velocity;
    if !wanted.is_finite() {
      self.warn_once(
        entity,
        "a non-finite Velocity is not applied to a character",
      );
      wanted = Vec2::ZERO;
    }
    let extra = if extra.is_finite() { extra } else { Vec2::ZERO };
    let platformer = controller.mode == CharacterMode::Platformer;
    let up = Vec2::NEG_Y;
    let max_slope = if controller.max_slope.is_finite() {
      controller.max_slope.clamp(0.0, FRAC_PI_2)
    } else {
      FRAC_PI_4
    };
    // The body's next pose is the teleport's target; the collider still
    // holds the old pose until the step
    let (start, shape, groups) = {
      let collider = self.colliders.get(collider_handle)?;
      let start = match (teleported, collider.position_wrt_parent()) {
        (true, Some(local)) => self.bodies.get(body_handle)?.next_position() * local,
        _ => *collider.position(),
      };
      (
        start,
        collider.shared_shape().clone(),
        collider.collision_groups(),
      )
    };
    let local = shape.compute_local_aabb();
    let height = (local.maxs.y - local.mins.y).max(f32::EPSILON);
    let skin = controller
      .skin
      .filter(|skin| skin.is_finite() && *skin > 0.0)
      .unwrap_or(0.01 * height);
    let kcc = KinematicCharacterController {
      up: to_rapier(up),
      offset: CharacterLength::Absolute(skin),
      slide: true,
      autostep: platformer
        .then_some(controller.step_height)
        .flatten()
        .filter(|h| h.is_finite() && *h > 0.0)
        .map(|h| CharacterAutostep {
          max_height: CharacterLength::Absolute(h),
          min_width: CharacterLength::Relative(0.5),
          include_dynamic_bodies: false,
        }),
      // Top-down: every surface can be climbed and none holds, so the
      // character slides along everything
      max_slope_climb_angle: if platformer { max_slope } else { PI },
      min_slope_slide_angle: if platformer { max_slope } else { 0.0 },
      snap_to_ground: if platformer {
        match controller.snap_to_ground {
          None => Some(CharacterLength::Relative(0.2)),
          Some(d) if d.is_finite() && d > 0.0 => Some(CharacterLength::Absolute(d)),
          Some(_) => None,
        }
      } else {
        None
      },
      // Rapier's nudge is in world units, and the world is in pixels
      normal_nudge_factor: 1.0e-4 * self.params.length_unit,
    };
    let desired = (wanted + extra) * dt;
    let was_on_floor = state.on_floor;
    // Only a character standing on a one-way floor drops (from it, so it is
    // taken before this step's probe); anywhere else the flag clears at once
    if controller.drop_through && state.drop_from.is_none() {
      state.drop_from = state
        .floor_collider
        .filter(|_| was_on_floor && state.floor_one_way);
    }
    let dropping = controller.drop_through && state.drop_from.is_some();
    // The floor it stood on carries it, by the floor's motion this step at
    // the touched point. The queries see the floor where it is, not where
    // the step takes it, so the character's own move is swept (and the floor
    // probed) first, as if the floor stood still, and the carry after it:
    // on a rising floor the fall still lands, above a sinking one the
    // character does not float
    let carry = match state.floor {
      Some(floor) if was_on_floor => self.floor_motion(floor, state.floor_point, dt),
      _ => Vec2::ZERO,
    };
    let carrier = state
      .floor
      .filter(|_| carry.length() > sweep::MIN_MOVE)
      .and_then(|floor| self.body_entries.get(&floor))
      .map(|entry| entry.handle);
    // The floor whose frame the own move is in (it carries the character)
    let carried_by = state.floor.filter(|_| was_on_floor);

    // The move and the probe start where the step does
    let solid_from_start = solid(&*shape, start, dropping);
    let filter = RapierFilter::default()
      .exclude_rigid_body(body_handle)
      .exclude_sensors()
      .groups(groups)
      .predicate(&solid_from_start);
    let dispatcher: &dyn QueryDispatcher = self.narrow_phase.query_dispatcher();
    let queries =
      self
        .broad_phase
        .as_query_pipeline(dispatcher, &self.bodies, &self.colliders, filter);
    let mut pose = start;
    // Push out of what it overlaps first, when the last move ended inside
    // something or there is no move (the sweep's casts push out otherwise).
    // Rapier does it when the shape does not move, by at most a quarter of
    // the shape's height a call: a push-out that moved it goes again next step
    let mut pushing_out = false;
    if state.stuck || desired.length() <= sweep::MIN_MOVE {
      let at = from_rapier(pose.translation);
      pose.translation += to_rapier(self.coincident_nudge(entity, collider_handle, at));
      let push_out = KinematicCharacterController {
        snap_to_ground: None,
        ..kcc
      };
      let out = push_out.move_shape(dt, &queries, &*shape, &pose, Vector::ZERO, |_| {});
      pose.translation += out.translation;
      pushing_out = out.translation != Vector::ZERO;
    }
    // Keep to the ground only from the ground, and never on the way up
    let snap = platformer && was_on_floor && desired.dot(up) <= 0.0;
    let mut collisions: Vec<CharacterCollision> = Vec::new();
    let moved = sweep::move_shape(
      &kcc,
      &queries,
      &*shape,
      &pose,
      to_rapier(desired),
      snap,
      |collision| collisions.push(collision),
    );
    pose.translation += moved.translation;
    // The floor under `pose`: what the bare shape meets below, past what it
    // only touches (a neighbour flush against its side); failing that, what
    // the sweep stands it on, its skin kept (a corner or a ledge's edge the
    // skin reaches but the shape does not overlap)
    let find_floor = |queries: &QueryPipeline, pose: &Pose| {
      if !platformer {
        return None;
      }
      let probe = |gap, reach| {
        let down = to_rapier(Vec2::Y);
        let tolerance = kcc.normal_nudge_factor;
        sweep::probe(queries, pose, down, &*shape, gap, reach, tolerance)
      };
      let holds = |(_, hit): &(ColliderHandle, ShapeCastHit)| {
        from_rapier(hit.normal1).dot(up) >= floor_cos(max_slope)
      };
      probe(0.0, skin + FLOOR_PROBE)
        .filter(holds)
        .or_else(|| probe(skin, FLOOR_PROBE))
        // Not one its own move rose off by more than the skin, though the
        // probe still reaches it: rising past a ledge's top or a
        // neighbour's head would land it mid-jump. Measured along the
        // floor's normal, so a walk up a slope keeps it, and the push-out
        // and the carry are not part of the move; and against the floor, so
        // a lift rising faster than the character catches it up (on the
        // floor that carried it, the move is already the lift's own frame).
        // A move from the floor that does not go up (`snap`) keeps it too:
        // a step up rises onto its floor
        .filter(|(handle, hit)| {
          let floor = self.owners.get(handle).map(|owner| owner.body);
          let frame = match floor {
            Some(body) if carried_by != Some(body) => {
              to_rapier(self.floor_motion(body, from_rapier(hit.witness1), dt))
            }
            _ => Vector::ZERO,
          };
          snap || (moved.translation - frame).dot(hit.normal1) <= skin
        })
    };
    // A floor under no more than the skin of the character's bottom is a
    // corner it is held up on by its skin (a ledge's edge or a neighbour's
    // head it is level with, or just past, or barely over): unless something
    // else holds it there, it is moved off, away from the corner, and falls
    // past it to what is beside it. Not when the move goes onto it: a step
    // up, or a landing on a platform's near edge, has only begun to cross it
    let corner = |(handle, hit): &(ColliderHandle, ShapeCastHit), pose: &Pose| {
      if from_rapier(hit.normal1).dot(up) < floor_cos(max_slope) {
        return None;
      }
      self
        .corner_shift(&*shape, pose, *handle, hit, skin)
        .filter(|shift| to_rapier(desired).dot(*shift) >= 0.0)
    };
    let mut floor_hit = find_floor(&queries, &pose);
    if let Some((held_by, shift)) =
      floor_hit.and_then(|hit| corner(&hit, &pose).map(|shift| (hit.0, shift)))
    {
      // Something else under it, at the seam of two floors?
      let not_corner = |handle: ColliderHandle, collider: &RapierCollider| {
        handle != held_by && solid_from_start(handle, collider)
      };
      let others = QueryPipeline {
        filter: filter.predicate(&not_corner),
        ..queries
      };
      floor_hit = find_floor(&others, &pose).filter(|hit| corner(hit, &pose).is_none());
      if floor_hit.is_none() {
        let aside = KinematicCharacterController {
          autostep: None,
          snap_to_ground: None,
          ..kcc
        };
        let off = sweep::move_shape(&aside, &queries, &*shape, &pose, shift, false, |_| {});
        pose.translation += off.translation;
        floor_hit = find_floor(&queries, &pose).filter(|hit| corner(hit, &pose).is_none());
      }
    }

    // The carry, past everything but the floor (it moves with the character),
    // starting where the move left it
    if let Some(carrier) = carrier {
      let solid_from_moved = solid(&*shape, pose, dropping);
      let not_carrier = |handle: ColliderHandle, collider: &RapierCollider| {
        solid_from_moved(handle, collider) && collider.parent() != Some(carrier)
      };
      let carrying = QueryPipeline {
        filter: filter.predicate(&not_carrier),
        ..queries
      };
      let ride = KinematicCharacterController {
        autostep: None,
        ..kcc
      };
      let carried = sweep::move_shape(
        &ride,
        &carrying,
        &*shape,
        &pose,
        to_rapier(carry),
        false,
        |collision| collisions.push(collision),
      );
      pose.translation += carried.translation;
    }

    let previous_platform = state.platform_velocity;
    let (normals, stuck) =
      self.read_surfaces(state, &collisions, floor_hit, platformer, max_slope, dt);
    // Rapier's queries end here (they borrow the sets)

    state.landed = state.on_floor && !was_on_floor;
    state.left_floor = was_on_floor && !state.on_floor;
    // Leaving a moving floor keeps its momentum
    let momentum = if state.left_floor {
      previous_platform
    } else {
      Vec2::ZERO
    };
    // Landing on a rising floor takes its rise off, as leaving it adds its
    // momentum: the carry moves the character with it from here, and a
    // velocity still holding the rise would lift it off again
    let rise = if state.landed {
      up * state.platform_velocity.dot(up).max(0.0)
    } else {
      Vec2::ZERO
    };
    *velocity = real_velocity(wanted - rise, &normals, platformer.then_some(up), momentum);
    state.sliding_down_slope = platformer && moved.sliding_down_slope;
    state.velocity_before = wanted;
    state.stuck = stuck || pushing_out;

    if controller.push && !collisions.is_empty() {
      self.push_bodies(&kcc, collider_handle, filter, &*shape, &collisions, dt);
    }

    let drop_done = self.track_drop(state, controller.drop_through, &*shape, &pose);

    let delta = from_rapier(pose.translation - start.translation);
    // A teleport's pose (its turn included) is where the move started;
    // otherwise the body moves on from where it is, keeping its rotation
    let body = self.bodies.get(body_handle)?;
    let (body_translation, rotation) = if teleported {
      let next = body.next_position();
      (from_rapier(next.translation), next.rotation.angle())
    } else {
      (from_rapier(body.translation()), body.rotation().angle())
    };
    self.drive_to(entity, body_translation + delta, rotation);
    Some(CharacterMove { normals, drop_done })
  }

  /// How far to move a character before its push-out, off the kinematic
  /// bodies (other characters) centred on its point: two characters spawned
  /// or respawned on one spot. The push-out gives two shapes on one point
  /// the same normal, so both would be pushed the same way every step and
  /// stay together; moved apart by entity order first (the lower one left
  /// and down, the higher right and up, a few hundredths of a pixel), each
  /// is pushed away from the other. Only what it touched at the last step
  /// is looked at (the narrow phase's pairs), so this walks its contacts and
  /// makes no query. `at` is where the collider starts the step.
  fn coincident_nudge(&self, entity: Entity, collider: ColliderHandle, at: Vec2) -> Vec2 {
    let mut side = 0.0;
    for pair in self.narrow_phase.contact_pairs_with(collider) {
      let other = if pair.collider1 == collider {
        pair.collider2
      } else {
        pair.collider1
      };
      // On its point first: that rules out nearly every pair at once
      let Some(rapier) = self
        .colliders
        .get(other)
        .filter(|c| from_rapier(c.translation()).distance_squared(at) <= COINCIDENT * COINCIDENT)
      else {
        continue;
      };
      let kinematic = rapier
        .parent()
        .and_then(|body| self.bodies.get(body))
        .is_some_and(|body| body.is_kinematic());
      if !kinematic || !rapier.is_enabled() || rapier.is_sensor() {
        continue;
      }
      if let Some(owner) = self.owners.get(&other) {
        side += if entity < owner.collider { -1.0 } else { 1.0 };
      }
    }
    Vec2::new(side, -side) * NUDGE
  }

  /// How far to move a character of `shape` at `pose` off the floor `hit`
  /// found on `collider` when it is a corner: the floor (the part hit, for
  /// a polyline or compound) overlaps the character's width, across up, by
  /// no more than `skin`. Away from the floor, by that overlap plus the
  /// skin, so the corner is a skin's width beside it. `None` for a floor,
  /// or a shape whose extent is not known (no support map).
  fn corner_shift(
    &self,
    shape: &dyn Shape,
    pose: &Pose,
    collider: ColliderHandle,
    hit: &ShapeCastHit,
    skin: f32,
  ) -> Option<Vector> {
    let (lo, hi) = across(shape, pose)?;
    let collider = self.colliders.get(collider)?;
    let (floor_lo, floor_hi) = across(collider.shape(), collider.position()).or_else(|| {
      let mut part = None;
      collider
        .shape()
        .as_composite_shape()?
        .map_part_at(hit.subshape1, &mut |at, shape, _| {
          let pose = at.map_or(*collider.position(), |at| *collider.position() * *at);
          part = across(shape, &pose);
        });
      part
    })?;
    let overlap = hi.min(floor_hi) - lo.max(floor_lo);
    if overlap > skin {
      return None;
    }
    let away = if floor_lo + floor_hi > lo + hi {
      -1.0
    } else {
      1.0
    };
    Some(Vector::X * away * (overlap + skin))
  }

  /// Keep a drop-through's bookkeeping after the move; true when the drop
  /// `wanted` is over. It is over when the character, at `pose`, is
  /// entirely below the one-way collider it dropped from (`drop_from`),
  /// when it lands (only on something solid: the drop ignores one-way
  /// colliders), and at once when there is nothing to drop from (it stood
  /// on solid ground or nothing, or the collider is gone).
  fn track_drop(
    &self,
    state: &mut CharacterState,
    wanted: bool,
    shape: &dyn Shape,
    pose: &Pose,
  ) -> bool {
    if !wanted {
      state.drop_from = None;
      return false;
    }
    let below = state
      .drop_from
      .and_then(|from| self.collider_entries.get(&from))
      .and_then(|entry| self.colliders.get(entry.handle))
      .and_then(|collider| {
        one_way::decode(collider.user_data)
          .map(|local| one_way::character_below(collider, local, shape, pose))
      })
      .unwrap_or(true);
    let over = below || state.on_floor;
    if over {
      state.drop_from = None;
    }
    over
  }

  /// Read what a move touched into `state`: the sweep's hits are walls or
  /// ceilings (a floor is the probe's), and what the probe found is the
  /// floor when its normal is within `max_slope` of up. Returns the normals
  /// that clip the velocity (up for a floor) and whether the move ended
  /// within the skin of something or inside it.
  fn read_surfaces(
    &self,
    state: &mut CharacterState,
    collisions: &[CharacterCollision],
    floor_hit: Option<(ColliderHandle, ShapeCastHit)>,
    platformer: bool,
    max_slope: f32,
    dt: f32,
  ) -> (Vec<Vec2>, bool) {
    let up = Vec2::NEG_Y;
    let cos = floor_cos(max_slope);
    state.hits.clear();
    state.on_wall = false;
    state.on_ceiling = false;
    state.wall_normal = Vec2::ZERO;
    let mut normals = Vec::with_capacity(collisions.len() + 1);
    let mut stuck = false;
    for collision in collisions {
      let normal = from_rapier(collision.hit.normal1);
      stuck |= collision.hit.status == ShapeCastStatus::PenetratingOrWithinTargetDist;
      let Some(owner) = self.owners.get(&collision.handle).copied() else {
        continue;
      };
      state.hits.push(CharacterHit {
        entity: owner.collider,
        body: owner.body,
        point: from_rapier(collision.hit.witness1),
        normal,
      });
      if platformer && normal.dot(up) >= cos {
        // A floor: the probe settles it
      } else if platformer && normal.dot(-up) >= cos {
        state.on_ceiling = true;
        normals.push(normal);
      } else {
        state.on_wall = true;
        state.wall_normal = normal;
        normals.push(normal);
      }
    }
    match floor_hit.filter(|(_, hit)| from_rapier(hit.normal1).dot(up) >= cos) {
      Some((handle, hit)) => {
        let owner = self.owners.get(&handle).copied();
        let point = from_rapier(hit.witness1);
        // How the floor moves there this step: its velocity, and where the
        // touched point will be when the next step carries the character
        let motion = match owner {
          Some(owner) if dt > 0.0 => self.floor_motion(owner.body, point, dt),
          _ => Vec2::ZERO,
        };
        state.on_floor = true;
        state.floor_normal = from_rapier(hit.normal1);
        state.floor = owner.map(|owner| owner.body);
        state.floor_collider = owner.map(|owner| owner.collider);
        state.floor_one_way = self
          .colliders
          .get(handle)
          .is_some_and(|collider| one_way::decode(collider.user_data).is_some());
        state.floor_point = point + motion;
        state.platform_velocity = if dt > 0.0 { motion / dt } else { Vec2::ZERO };
        normals.push(up);
      }
      None => {
        state.on_floor = false;
        state.floor_normal = Vec2::ZERO;
        state.floor = None;
        state.floor_collider = None;
        state.floor_one_way = false;
        state.platform_velocity = Vec2::ZERO;
      }
    }
    (normals, stuck)
  }

  /// Push the dynamic bodies a character's move ran into: Rapier's
  /// approximate impulses, with the mass of the character's collider.
  /// Kinematic and fixed bodies are never pushed.
  fn push_bodies(
    &mut self,
    kcc: &KinematicCharacterController,
    collider: ColliderHandle,
    filter: RapierFilter,
    shape: &dyn Shape,
    collisions: &[CharacterCollision],
    dt: f32,
  ) {
    let mass = self
      .colliders
      .get(collider)
      .map(|collider| collider.mass())
      .filter(|mass| mass.is_finite() && *mass > 0.0)
      .unwrap_or(1.0);
    let dispatcher: &dyn QueryDispatcher = self.narrow_phase.query_dispatcher();
    let mut queries = self.broad_phase.as_query_pipeline_mut(
      dispatcher,
      &mut self.bodies,
      &mut self.colliders,
      filter,
    );
    kcc.solve_character_collision_impulses(dt, &mut queries, shape, mass, collisions.iter());
  }
}

/// The physics step, after `push_changes`: sweep every character.
pub(crate) fn move_characters(
  mut physics: ResMut<Physics>,
  time: Res<Time>,
  mut characters: Query<(
    Entity,
    &mut CharacterController,
    &mut CharacterState,
    &mut Velocity,
    Option<&mut Knockback>,
  )>,
) {
  let dt = time.fixed.timestep();
  for (entity, mut controller, mut state, mut velocity, mut knockback) in &mut characters {
    let mut v = velocity.as_vec2();
    let extra = knockback
      .as_ref()
      .filter(|kb| kb.active)
      .map_or(Vec2::ZERO, |kb| {
        let (x, y) = kb.current_velocity();
        Vec2::new(x, y)
      });
    let Some(moved) = physics.move_character(entity, &controller, &mut state, &mut v, extra, dt)
    else {
      continue;
    };
    velocity.set_if_neq(Velocity::from_vec2(v));
    if let Some(knockback) = knockback.as_mut() {
      for normal in &moved.normals {
        knockback.clip(*normal);
      }
    }
    if moved.drop_done {
      controller.drop_through = false;
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ecs::{ChildOf, Disabled, Transform, Velocity, World};
  use crate::core::physics::test_util::*;
  use crate::core::physics::{AngularVelocity, Collider, Physics, RigidBody};
  use crate::core::schedule::EngineSchedules;

  /// A 20×40 platformer character centred at (x, y).
  fn character(world: &mut World, x: f32, y: f32) -> Entity {
    world
      .spawn((
        Transform::from_xy(x, y),
        CharacterController::platformer(),
        Collider::rect(20.0, 40.0),
      ))
      .id()
  }

  /// One step with `gravity` px/s² added to the character's velocity first,
  /// as a game's movement code does.
  fn step(world: &mut World, s: &mut EngineSchedules, e: Entity, gravity: f32) {
    world.get_mut::<Velocity>(e).unwrap().dy += gravity / 60.0;
    run(world, s, 1);
  }

  fn state(world: &World, e: Entity) -> CharacterState {
    world.get::<CharacterState>(e).unwrap().clone()
  }

  fn velocity(world: &World, e: Entity) -> Vec2 {
    world.get::<Velocity>(e).unwrap().as_vec2()
  }

  #[test]
  fn a_character_falls_and_lands_on_the_floor() {
    let (mut world, mut s) = world(60);
    let f = floor(&mut world, 100.0);
    let e = character(&mut world, 0.0, 0.0);
    let mut landings = 0;
    for _ in 0..120 {
      step(&mut world, &mut s, e, 980.0);
      landings += usize::from(state(&world, e).landed);
    }
    let st = state(&world, e);
    assert!(st.on_floor);
    assert_eq!(landings, 1, "landed once");
    assert_eq!(st.floor, Some(f));
    assert!(
      (st.floor_normal - Vec2::NEG_Y).length() < 1e-3,
      "{:?}",
      st.floor_normal
    );
    assert!(velocity(&world, e).y.abs() < 1e-3, "the fall stops");
    let bottom = at(&world, e).translation.y + 20.0;
    assert!((99.0..=100.0).contains(&bottom), "{bottom}");
  }

  #[test]
  fn a_wall_stops_the_run() {
    let (mut world, mut s) = world(60);
    world.spawn((Transform::from_xy(100.0, 0.0), Collider::rect(20.0, 400.0)));
    let e = character(&mut world, 0.0, 0.0);
    world.get_mut::<Velocity>(e).unwrap().dx = 300.0;
    for _ in 0..60 {
      world.get_mut::<Velocity>(e).unwrap().dx = 300.0;
      run(&mut world, &mut s, 1);
    }
    let st = state(&world, e);
    assert!(st.on_wall);
    // Shape-cast normals from within the skin carry about 1.5e-3 of noise
    assert!(
      (st.wall_normal - Vec2::NEG_X).length() < 2e-3,
      "{:?}",
      st.wall_normal
    );
    assert_eq!(st.velocity_before.x, 300.0);
    assert!(velocity(&world, e).x.abs() < 1e-3);
    assert!(at(&world, e).translation.x + 10.0 <= 90.5);
  }

  #[test]
  fn a_ceiling_stops_the_rise() {
    let (mut world, mut s) = world(60);
    world.spawn((Transform::from_xy(0.0, -100.0), Collider::rect(400.0, 20.0)));
    let e = character(&mut world, 0.0, 0.0);
    world.get_mut::<Velocity>(e).unwrap().dy = -300.0;
    let mut bumped = false;
    for _ in 0..30 {
      run(&mut world, &mut s, 1);
      bumped |= state(&world, e).on_ceiling;
    }
    assert!(bumped);
    assert!(velocity(&world, e).y.abs() < 1e-3);
    assert!(at(&world, e).translation.y - 20.0 >= -90.5);
  }

  /// A ramp rising to the right at `degrees`, its foot at (0, 100).
  fn ramp(world: &mut World, degrees: f32) {
    let rise = 300.0 * degrees.to_radians().tan();
    world.spawn((
      Transform::IDENTITY,
      Collider::polygon([
        Vec2::new(0.0, 100.0),
        Vec2::new(300.0, 100.0),
        Vec2::new(300.0, 100.0 - rise),
      ]),
    ));
    floor(world, 100.0);
  }

  #[test]
  fn a_character_walks_up_a_gentle_slope() {
    let (mut world, mut s) = world(60);
    ramp(&mut world, 30.0);
    let e = character(&mut world, -60.0, 70.0);
    for _ in 0..120 {
      world.get_mut::<Velocity>(e).unwrap().dx = 150.0;
      step(&mut world, &mut s, e, 980.0);
    }
    assert!(
      at(&world, e).translation.y < 40.0,
      "climbed: {:?}",
      at(&world, e)
    );
    assert!(state(&world, e).on_floor);
  }

  #[test]
  fn a_character_standing_on_a_gentle_slope_does_not_creep() {
    let (mut world, mut s) = world(60);
    ramp(&mut world, 30.0);
    let e = character(&mut world, 150.0, 0.0);
    for _ in 0..60 {
      step(&mut world, &mut s, e, 980.0);
    }
    let x = at(&world, e).translation.x;
    for _ in 0..60 {
      step(&mut world, &mut s, e, 980.0);
    }
    assert!((at(&world, e).translation.x - x).abs() < 1.0);
  }

  #[test]
  fn a_character_slides_down_a_steep_slope() {
    let (mut world, mut s) = world(60);
    ramp(&mut world, 60.0);
    let e = character(&mut world, 200.0, -280.0);
    let mut sliding = false;
    for _ in 0..90 {
      step(&mut world, &mut s, e, 980.0);
      sliding |= state(&world, e).sliding_down_slope;
    }
    assert!(sliding);
    assert!(at(&world, e).translation.x < 200.0, "slid back down");
  }

  #[test]
  fn a_ledge_below_the_step_height_is_climbed_and_a_higher_one_is_not() {
    for (height, climbs) in [(8.0, true), (20.0, false)] {
      let (mut world, mut s) = world(60);
      floor(&mut world, 100.0);
      world.spawn((
        Transform::from_xy(140.0, 100.0 - height / 2.0),
        Collider::rect(100.0, height),
      ));
      let e = world
        .spawn((
          Transform::from_xy(0.0, 79.0),
          CharacterController::platformer().with_step_height(12.0),
          Collider::rect(20.0, 40.0),
        ))
        .id();
      for _ in 0..90 {
        world.get_mut::<Velocity>(e).unwrap().dx = 150.0;
        step(&mut world, &mut s, e, 980.0);
      }
      let x = at(&world, e).translation.x;
      assert_eq!(x > 100.0, climbs, "ledge {height}: x = {x}");
    }
  }

  #[test]
  fn snapping_keeps_a_character_on_a_downward_slope() {
    let (mut world, mut s) = world(60);
    let rise = 300.0 * 20f32.to_radians().tan();
    world.spawn((
      Transform::IDENTITY,
      Collider::polygon([
        Vec2::new(0.0, 100.0 - rise),
        Vec2::new(0.0, 100.0),
        Vec2::new(300.0, 100.0),
      ]),
    ));
    floor(&mut world, 100.0);
    let e = character(&mut world, 20.0, 100.0 - rise - 30.0);
    for _ in 0..30 {
      step(&mut world, &mut s, e, 980.0);
    }
    let mut off = 0;
    for _ in 0..60 {
      world.get_mut::<Velocity>(e).unwrap().dx = 200.0;
      step(&mut world, &mut s, e, 980.0);
      off += usize::from(!state(&world, e).on_floor);
    }
    assert_eq!(off, 0, "left the ground on the way down");
  }

  #[test]
  fn top_down_characters_slide_along_walls_and_have_no_floor() {
    let (mut world, mut s) = world(60);
    world.spawn((Transform::from_xy(100.0, 0.0), Collider::rect(20.0, 800.0)));
    let e = world
      .spawn((
        Transform::IDENTITY,
        CharacterController::top_down(),
        Collider::circle(10.0),
      ))
      .id();
    let mut walled = false;
    for _ in 0..60 {
      world.get_mut::<Velocity>(e).unwrap().dx = 200.0;
      world.get_mut::<Velocity>(e).unwrap().dy = 200.0;
      run(&mut world, &mut s, 1);
      let st = state(&world, e);
      assert!(!st.on_floor && !st.on_ceiling);
      walled |= st.on_wall;
    }
    assert!(walled);
    let t = at(&world, e).translation;
    assert!(t.x <= 80.5 && t.y > 150.0, "slid along the wall: {t:?}");
  }

  #[test]
  fn a_dynamic_body_with_a_controller_is_warned_and_not_moved() {
    let (mut world, mut s) = world(60);
    let e = world
      .spawn((
        Transform::IDENTITY,
        RigidBody::dynamic(),
        CharacterController::platformer(),
        Collider::rect(20.0, 40.0),
      ))
      .id();
    run(&mut world, &mut s, 5);
    assert_eq!(world.resource::<Physics>().warnings, 1);
    assert!(state(&world, e).hits.is_empty());
  }

  #[test]
  fn a_controller_needs_a_collider_on_its_own_entity() {
    let (mut world, mut s) = world(60);
    let e = world
      .spawn((Transform::IDENTITY, CharacterController::platformer()))
      .id();
    world.spawn((Transform::IDENTITY, Collider::rect(20.0, 40.0), ChildOf(e)));
    run(&mut world, &mut s, 5);
    assert_eq!(world.resource::<Physics>().warnings, 1);
  }

  #[test]
  fn writing_transform_teleports_a_character_past_a_wall() {
    let (mut world, mut s) = world(60);
    world.spawn((Transform::from_xy(100.0, 0.0), Collider::rect(20.0, 400.0)));
    let e = character(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, 1);
    world.get_mut::<Transform>(e).unwrap().translation.x = 300.0;
    run(&mut world, &mut s, 2);
    let (position, _) = world.resource::<Physics>().body_pose(e).unwrap();
    assert!((position.x - 300.0).abs() < 1e-3, "{position:?}");
  }

  // Review focus 1
  #[test]
  fn a_character_spawned_inside_a_wall_is_pushed_out() {
    let (mut world, mut s) = world(60);
    world.spawn((Transform::from_xy(100.0, 0.0), Collider::rect(20.0, 400.0)));
    let e = character(&mut world, 85.0, 0.0);
    run(&mut world, &mut s, 10);
    let x = at(&world, e).translation.x;
    assert!(x.is_finite() && x + 10.0 <= 90.5, "{x}");
  }

  /// Steps with gravity added to each of `characters` first.
  fn fall_together(
    world: &mut World,
    s: &mut EngineSchedules,
    characters: &[Entity],
    steps: usize,
  ) {
    for _ in 0..steps {
      for &e in characters {
        world.get_mut::<Velocity>(e).unwrap().dy += 980.0 / 60.0;
      }
      run(world, s, 1);
    }
  }

  // Review focus 1: two players spawned on one spot
  #[test]
  fn two_characters_spawned_on_one_point_separate_and_stay_on_the_floor() {
    let (mut world, mut s) = world(60);
    floor(&mut world, 100.0);
    let a = character(&mut world, 0.0, 79.0);
    let b = character(&mut world, 0.0, 79.0);
    fall_together(&mut world, &mut s, &[a, b], 10);
    let (xa, xb) = (at(&world, a).translation.x, at(&world, b).translation.x);
    assert!((xa - xb).abs() >= 19.5, "apart: {xa} and {xb}");
    assert!(
      xa.abs() < 30.0 && xb.abs() < 30.0,
      "stayed put: {xa} and {xb}"
    );
    fall_together(&mut world, &mut s, &[a, b], 50);
    for e in [a, b] {
      let st = state(&world, e);
      let t = at(&world, e).translation;
      assert!(
        st.on_floor && t.x.abs() < 30.0,
        "{e}: {t:?} on floor {}",
        st.on_floor
      );
    }
  }

  #[test]
  fn two_characters_spawned_on_one_point_against_a_wall_separate() {
    let (mut world, mut s) = world(60);
    floor(&mut world, 100.0);
    world.spawn((Transform::from_xy(100.0, 0.0), Collider::rect(20.0, 400.0)));
    let a = character(&mut world, 80.0, 79.0);
    let b = character(&mut world, 80.0, 79.0);
    fall_together(&mut world, &mut s, &[a, b], 10);
    let (xa, xb) = (at(&world, a).translation.x, at(&world, b).translation.x);
    assert!((xa - xb).abs() >= 19.5, "apart: {xa} and {xb}");
    assert!(xa.max(xb) + 10.0 <= 90.5, "out of the wall: {xa} and {xb}");
  }

  #[test]
  fn two_top_down_characters_spawned_on_one_point_separate() {
    let (mut world, mut s) = world(60);
    let spawn = |world: &mut World| {
      world
        .spawn((
          Transform::from_xy(50.0, 50.0),
          CharacterController::top_down(),
          Collider::circle(8.0),
        ))
        .id()
    };
    let a = spawn(&mut world);
    let b = spawn(&mut world);
    run(&mut world, &mut s, 10);
    let (pa, pb) = (at(&world, a).translation, at(&world, b).translation);
    assert!(pa.distance(pb) >= 15.5, "apart: {pa:?} and {pb:?}");
    let centre = Vec2::splat(50.0);
    assert!(
      pa.distance(centre) < 20.0 && pb.distance(centre) < 20.0,
      "stayed put: {pa:?} and {pb:?}"
    );
  }

  /// One step of `all` falling, with `jumpers` jumping at 300 px/s from the
  /// floor: each leaves it and rises by a step of the jump less gravity.
  fn assert_jump(world: &mut World, s: &mut EngineSchedules, jumpers: &[Entity], all: &[Entity]) {
    let ys: Vec<f32> = jumpers
      .iter()
      .map(|&e| at(world, e).translation.y)
      .collect();
    for &e in jumpers {
      world.get_mut::<Velocity>(e).unwrap().dy = -300.0;
    }
    fall_together(world, s, all, 1);
    for (&e, y) in jumpers.iter().zip(ys) {
      let st = state(world, e);
      assert!(st.left_floor && !st.on_floor, "{e} jumped: {st:?}");
      let risen = y - at(world, e).translation.y;
      let expected = (300.0 - 980.0 / 60.0) / 60.0;
      assert!(
        (risen - expected).abs() < 0.05,
        "{e} rose {risen} of {expected}"
      );
    }
  }

  /// Every one of `all` stands on `floor`, step after step.
  fn assert_standing(
    world: &mut World,
    s: &mut EngineSchedules,
    all: &[Entity],
    floor: Entity,
    steps: usize,
  ) {
    for i in 0..steps {
      fall_together(world, s, all, 1);
      for &e in all {
        let st = state(world, e);
        assert!(
          st.on_floor && st.floor == Some(floor) && !st.landed && !st.left_floor,
          "{e}, step {i}: {st:?}"
        );
      }
    }
  }

  // The floor probe casts down past what the character only touches: a
  // neighbour flush against its side is no floor, and must not hide the
  // floor below (the probe's first hit was the neighbour, at time zero)
  #[test]
  fn two_characters_standing_side_by_side_stay_on_the_floor_and_can_jump() {
    let (mut world, mut s) = world(60);
    let f = floor(&mut world, 100.0);
    // 20 wide each: their sides touch exactly
    let both = [
      character(&mut world, 0.0, 79.0),
      character(&mut world, 20.0, 79.0),
    ];
    fall_together(&mut world, &mut s, &both, 10);
    assert_standing(&mut world, &mut s, &both, f, 120);
    for e in both {
      assert!(!state(&world, e).on_wall, "{e}: standing is no wall");
    }
    let xs = both.map(|e| at(&world, e).translation.x);
    assert!(xs[0].abs() < 1e-3 && (xs[1] - 20.0).abs() < 1e-3, "{xs:?}");
    // One, the other staying put; then the other, a step behind
    assert_jump(&mut world, &mut s, &both[..1], &both);
    assert!(state(&world, both[1]).on_floor, "the other stays");
    assert_jump(&mut world, &mut s, &both[1..], &both);
    fall_together(&mut world, &mut s, &both, 60);
    assert_standing(&mut world, &mut s, &both, f, 60);
  }

  // Rapier's push-out moves a character only out of an overlap, and at most
  // a quarter of its height a step, so it can leave characters exactly
  // touching: they stand all the same
  #[test]
  fn three_overlapping_characters_pushed_apart_stay_on_the_floor() {
    let (mut world, mut s) = world(60);
    let f = floor(&mut world, 100.0);
    let all = [-10.0, 0.0, 10.0].map(|x| character(&mut world, x, 79.0));
    fall_together(&mut world, &mut s, &all, 30);
    let mut xs = all.map(|e| at(&world, e).translation.x);
    xs.sort_by(f32::total_cmp);
    assert!(
      xs[1] - xs[0] >= 19.99 && xs[2] - xs[1] >= 19.99,
      "apart: {xs:?}"
    );
    assert!(xs[0] > -50.0 && xs[2] < 50.0, "stayed put: {xs:?}");
    assert_standing(&mut world, &mut s, &all, f, 120);
    assert_jump(&mut world, &mut s, &all, &all);
    fall_together(&mut world, &mut s, &all, 60);
    assert_standing(&mut world, &mut s, &all, f, 60);
  }

  // Walls are still walls: pressed into a neighbour flush against it, a
  // character is on the floor and on the wall, stopped, and climbs nothing
  #[test]
  fn a_character_holding_into_a_touching_neighbour_keeps_its_footing() {
    for side in [1.0, -1.0] {
      let (mut world, mut s) = world(60);
      floor(&mut world, 100.0);
      let e = character(&mut world, 0.0, 79.0);
      let other = character(&mut world, 20.0 * side, 79.0);
      fall_together(&mut world, &mut s, &[e, other], 10);
      let start = at(&world, e).translation;
      for i in 0..120 {
        world.get_mut::<Velocity>(e).unwrap().dx = 150.0 * side;
        fall_together(&mut world, &mut s, &[e, other], 1);
        let st = state(&world, e);
        assert!(
          st.on_floor && !st.landed && !st.left_floor && st.on_wall,
          "side {side}, step {i}: {st:?}"
        );
        assert!(
          (st.wall_normal - Vec2::new(-side, 0.0)).length() < 1e-3,
          "side {side}, step {i}: {:?}",
          st.wall_normal
        );
        assert!(velocity(&world, e).x.abs() < 1e-3, "side {side}: stopped");
        // Against it (the sweep keeps its skin from what it hits, so it
        // may back off by that), never into it or up it
        let t = at(&world, e).translation;
        let back = (start.x - t.x) * side;
        assert!(
          (-1e-3..0.5).contains(&back) && (t.y - start.y).abs() < 0.05,
          "side {side}, step {i}: moved from {start} to {t}"
        );
        // Kinematic: never pushed (it may keep its own skin, as above)
        let o = (at(&world, other).translation.x - 20.0 * side) * side;
        assert!(
          (-1e-3..0.5).contains(&o),
          "side {side}, step {i}: the other moved by {o}"
        );
        assert!(state(&world, other).on_floor, "side {side}, step {i}");
      }
      // A jump while still holding into it
      world.get_mut::<Velocity>(e).unwrap().dx = 150.0 * side;
      assert_jump(&mut world, &mut s, &[e], &[e, other]);
    }
  }

  // The same probe, against a wall it was put flush against
  #[test]
  fn a_character_flush_against_a_wall_stays_on_the_floor() {
    let (mut world, mut s) = world(60);
    let f = floor(&mut world, 100.0);
    // The wall's left face at x = 90, the character's right side too
    world.spawn((Transform::from_xy(100.0, 0.0), Collider::rect(20.0, 160.0)));
    let e = character(&mut world, 80.0, 79.0);
    fall_together(&mut world, &mut s, &[e], 10);
    assert_standing(&mut world, &mut s, &[e], f, 30);
    // Pressing into it: on the wall too, and no further
    for i in 0..30 {
      world.get_mut::<Velocity>(e).unwrap().dx = 150.0;
      fall_together(&mut world, &mut s, &[e], 1);
      let st = state(&world, e);
      assert!(
        st.on_floor && st.floor == Some(f) && st.on_wall,
        "step {i}: {st:?}"
      );
      assert!(at(&world, e).translation.x + 10.0 <= 90.0 + 1e-3);
    }
    assert_jump(&mut world, &mut s, &[e], &[e]);
  }

  /// `steps` steps of `all` falling, watching `e`: off the floor it must be
  /// moving, never held up in the air by something that is not its floor.
  /// How many times it landed.
  fn fall_watching(
    world: &mut World,
    s: &mut EngineSchedules,
    e: Entity,
    all: &[Entity],
    steps: usize,
  ) -> usize {
    let mut landings = 0;
    for i in 0..steps {
      let y = at(world, e).translation.y;
      fall_together(world, s, all, 1);
      let st = state(world, e);
      let moved = (at(world, e).translation.y - y).abs();
      assert!(
        st.on_floor || moved > 1e-3,
        "{e}, step {i}: held in the air at {}, velocity {}: {st:?}",
        at(world, e).translation,
        velocity(world, e)
      );
      landings += usize::from(st.landed);
    }
    landings
  }

  // A jump in place beside a neighbour clears its head and comes down past
  // its corner. Flush, or within the skin of it, the sweep (keeping its
  // skin) stops the fall on that corner, which the character's bottom
  // overlaps by no more than the skin: a corner, not a floor, so it is
  // moved off it, away from the neighbour, and lands on the ground beside
  // it, once. It hung in the air there, then stood on the corner (wedging
  // the neighbour), or slipped off it and landed twice
  #[test]
  fn a_character_jumping_in_place_beside_a_flush_neighbour_lands_beside_it() {
    for gap in [0.0, 0.05, 0.1, 0.2, 0.28] {
      for jumper in [0, 1] {
        let (mut world, mut s) = world(60);
        let f = floor(&mut world, 100.0);
        let both = [
          character(&mut world, 0.0, 79.0),
          character(&mut world, 20.0 + gap, 79.0),
        ];
        let (e, other) = (both[jumper], both[1 - jumper]);
        fall_together(&mut world, &mut s, &both, 10);
        assert_standing(&mut world, &mut s, &both, f, 10);
        for jump in 0..2 {
          // 46 px high: over the other's 40
          assert_jump(&mut world, &mut s, &[e], &both);
          let landings = fall_watching(&mut world, &mut s, e, &both, 120);
          let st = state(&world, e);
          let t = at(&world, e).translation;
          assert!(
            st.on_floor && st.floor == Some(f) && landings == 1,
            "gap {gap}, {jumper}, jump {jump}: landed {landings} times, at {t}: {st:?}"
          );
          let v = velocity(&world, e);
          assert!(v.y.abs() < 1e-3, "gap {gap}, {jumper}, jump {jump}: {v}");
          let o = state(&world, other);
          assert!(o.on_floor && o.floor == Some(f), "the other stands");
        }
      }
    }
  }

  // The same with a ledge: a character falling with its side level with the
  // ledge's edge, or overlapping it by no more than the skin, or past it
  // within the skin, is moved off its corner and falls to the floor below;
  // one overlapping it by a pixel stands on it
  #[test]
  fn a_character_falling_flush_past_a_ledge_corner_does_not_hang_in_the_air() {
    for (past, on_ledge) in [
      (-1.0, true),
      (-0.3, false),
      (-0.1, false),
      (0.0, false),
      (0.1, false),
      (1.0, false),
    ] {
      let (mut world, mut s) = world(60);
      // The ledge's right edge at x = 200, its top at 100
      let ledge = floor(&mut world, 100.0);
      let below = world
        .spawn((Transform::from_xy(0.0, 310.0), Collider::rect(2000.0, 20.0)))
        .id();
      // Its left side `past` px right of the edge, 1 px above the top
      let e = character(&mut world, 210.0 + past, 79.0);
      let landings = fall_watching(&mut world, &mut s, e, &[e], 150);
      let st = state(&world, e);
      assert!(
        st.on_floor && landings == 1,
        "{past} past the edge: landed {landings} times: {st:?}"
      );
      assert!(
        velocity(&world, e).y.abs() < 1e-3,
        "{past}: {}",
        velocity(&world, e)
      );
      let expected = if on_ledge { ledge } else { below };
      assert_eq!(st.floor, Some(expected), "{past} past the edge");
    }
  }

  // A pixel of a ledge under it is a floor, standing or walking to the edge
  // and stopping there
  #[test]
  fn a_character_overlapping_a_ledge_by_a_pixel_stands_on_it() {
    let (mut world, mut s) = world(60);
    let ledge = floor(&mut world, 100.0);
    let e = character(&mut world, 209.0, 79.0);
    fall_together(&mut world, &mut s, &[e], 10);
    assert_standing(&mut world, &mut s, &[e], ledge, 60);
    assert!((at(&world, e).translation.x - 209.0).abs() < 1e-3);
  }

  // A tile-aligned character beside a block of tiles, flush against it,
  // jumps over its top and comes down past its corner to the ground (the
  // outline is one polyline: its top run is the part measured)
  #[test]
  fn a_tile_aligned_character_jumping_beside_a_flush_block_lands_on_the_ground() {
    use crate::core::physics::{TileCell, TileColliders};
    for side in [1.0, -1.0] {
      let (mut world, mut s) = world(60);
      // 16 px tiles from (0, 100): a 2 × 2 block on columns 10 and 11,
      // over a ground row (its top at 132)
      let mut cells = vec![TileCell::Empty; 20 * 3];
      for x in 0..20 {
        cells[2 * 20 + x] = TileCell::Solid;
      }
      for (x, y) in [(10, 0), (11, 0), (10, 1), (11, 1)] {
        cells[y * 20 + x] = TileCell::Solid;
      }
      world.spawn((
        Transform::from_xy(0.0, 100.0),
        TileColliders::from_cells(20, 3, Vec2::splat(16.0), cells),
      ));
      // 16 × 32, flush against the block's left (x = 160) or right (x = 192)
      let x = if side > 0.0 { 152.0 } else { 200.0 };
      let e = world
        .spawn((
          Transform::from_xy(x, 115.0),
          CharacterController::platformer(),
          Collider::rect(16.0, 32.0),
        ))
        .id();
      fall_together(&mut world, &mut s, &[e], 20);
      assert!(state(&world, e).on_floor, "side {side}: on the ground");
      assert_jump(&mut world, &mut s, &[e], &[e]);
      let landings = fall_watching(&mut world, &mut s, e, &[e], 120);
      let st = state(&world, e);
      let feet = at(&world, e).translation.y + 16.0;
      assert!(
        st.on_floor && landings == 1 && (131.0..=132.0).contains(&feet),
        "side {side}: landed {landings} times, feet at {feet}: {st:?}"
      );
    }
  }

  /// Steps of `all` falling until `e` lands (at most `steps`): how many
  /// times it landed, after asserting that it found no floor while rising.
  fn rise_and_land(
    world: &mut World,
    s: &mut EngineSchedules,
    e: Entity,
    all: &[Entity],
    steps: usize,
  ) -> usize {
    let mut landings = 0;
    for i in 0..steps {
      let y = at(world, e).translation.y;
      fall_together(world, s, all, 1);
      let st = state(world, e);
      let rose = y - at(world, e).translation.y;
      assert!(
        rose < 0.05 || !st.on_floor,
        "{e}, step {i}: on a floor while rising {rose}: {st:?}"
      );
      landings += usize::from(st.landed);
      if st.on_floor && velocity(world, e).y >= 0.0 {
        break;
      }
    }
    landings
  }

  // A jump flush past a ledge's side or a neighbour's rises past its top
  // corner, which the probe reaches (2 px beyond the skin) while the
  // character is still rising: the floor its own move rises off is not its
  // floor, or it landed mid-jump (and a preset refilled its coyote time)
  #[test]
  fn rising_flush_past_a_corner_does_not_land_on_it() {
    for neighbour in [false, true] {
      for side in [1.0, -1.0] {
        let (mut world, mut s) = world(60);
        let f = floor(&mut world, 100.0);
        // Its side against the character's, its top 30 px up (or 40 for a
        // neighbour): a 300 px/s jump rises 46 px
        let other = if neighbour {
          character(&mut world, 20.0 * side, 79.0)
        } else {
          world
            .spawn((
              Transform::from_xy(60.0 * side, 85.0),
              Collider::rect(100.0, 30.0),
            ))
            .id()
        };
        let e = character(&mut world, 0.0, 79.0);
        let all = if neighbour { vec![e, other] } else { vec![e] };
        fall_together(&mut world, &mut s, &all, 10);
        assert_standing(&mut world, &mut s, &all, f, 10);
        assert_jump(&mut world, &mut s, &[e], &all);
        let landings = rise_and_land(&mut world, &mut s, e, &all, 120);
        let st = state(&world, e);
        assert!(
          st.on_floor && landings == 1,
          "neighbour {neighbour}, side {side}: landed {landings} times: {st:?}"
        );
      }
    }
  }

  // The same rising up through a one-way platform: once the feet clear it,
  // it is under them while the character still rises
  #[test]
  fn a_jump_up_through_a_one_way_platform_lands_on_it_once() {
    let (mut world, mut s) = world(60);
    let f = floor(&mut world, 200.0);
    let platform = one_way_platform(&mut world, 170.0);
    let e = character(&mut world, 0.0, 179.0);
    fall_together(&mut world, &mut s, &[e], 10);
    assert_standing(&mut world, &mut s, &[e], f, 10);
    assert_jump(&mut world, &mut s, &[e], &[e]);
    let landings = rise_and_land(&mut world, &mut s, e, &[e], 120);
    let st = state(&world, e);
    assert!(
      st.on_floor && st.floor == Some(platform) && landings == 1,
      "landed {landings} times: {st:?}"
    );
  }

  // A hop slow enough to stay within the probe's reach of the floor on its
  // first step leaves it on that step, once
  #[test]
  fn the_take_off_step_leaves_the_floor_once() {
    for speed in [60.0, 90.0, 300.0] {
      let (mut world, mut s) = world(60);
      let f = floor(&mut world, 100.0);
      let e = character(&mut world, 0.0, 79.0);
      fall_together(&mut world, &mut s, &[e], 10);
      assert_standing(&mut world, &mut s, &[e], f, 10);
      world.get_mut::<Velocity>(e).unwrap().dy = -speed;
      fall_together(&mut world, &mut s, &[e], 1);
      let st = state(&world, e);
      assert!(
        st.left_floor && !st.on_floor,
        "{speed} px/s, the take-off: {st:?}"
      );
      let (mut left, mut landings) = (0, 0);
      for _ in 0..60 {
        fall_together(&mut world, &mut s, &[e], 1);
        let st = state(&world, e);
        left += usize::from(st.left_floor);
        landings += usize::from(st.landed);
      }
      assert_eq!((left, landings), (0, 1), "{speed} px/s");
      assert!(state(&world, e).on_floor);
    }
  }

  // Walking up a slope moves along it, not off it: whether the game adds
  // gravity (the move is kept to the ground) or moves along the slope
  // (a move that goes up, so nothing keeps it to the ground)
  #[test]
  fn walking_up_a_slope_keeps_the_floor() {
    for along in [false, true] {
      let (mut world, mut s) = world(60);
      ramp(&mut world, 30.0);
      let e = character(&mut world, 150.0, 0.0);
      fall_together(&mut world, &mut s, &[e], 60);
      assert!(state(&world, e).on_floor);
      let x = at(&world, e).translation.x;
      for i in 0..30 {
        let v = if along {
          Vec2::from_angle(-30f32.to_radians()) * 120.0
        } else {
          Vec2::new(120.0, velocity(&world, e).y + 980.0 / 60.0)
        };
        *world.get_mut::<Velocity>(e).unwrap() = Velocity::from_vec2(v);
        run(&mut world, &mut s, 1);
        let st = state(&world, e);
        assert!(
          st.on_floor && !st.left_floor && !st.landed,
          "along {along}, step {i}: {st:?}"
        );
      }
      assert!(
        at(&world, e).translation.x > x + 40.0,
        "along {along}: climbed"
      );
    }
  }

  // A step up rises onto the floor: it stays on it
  #[test]
  fn stepping_up_a_ledge_keeps_the_floor() {
    let (mut world, mut s) = world(60);
    floor(&mut world, 100.0);
    world.spawn((Transform::from_xy(140.0, 96.0), Collider::rect(100.0, 8.0)));
    let e = world
      .spawn((
        Transform::from_xy(0.0, 79.0),
        CharacterController::platformer().with_step_height(12.0),
        Collider::rect(20.0, 40.0),
      ))
      .id();
    fall_together(&mut world, &mut s, &[e], 10);
    for i in 0..60 {
      world.get_mut::<Velocity>(e).unwrap().dx = 150.0;
      fall_together(&mut world, &mut s, &[e], 1);
      let st = state(&world, e);
      assert!(
        st.on_floor && !st.left_floor && !st.landed,
        "step {i}: {st:?}"
      );
    }
    assert!(at(&world, e).translation.x > 100.0, "stepped up");
  }

  // Top-down characters have no floor to lose: touching side by side they
  // still block each other and slide along each other
  #[test]
  fn two_top_down_characters_touching_side_by_side_block_each_other() {
    let (mut world, mut s) = world(60);
    let spawn = |world: &mut World, x: f32| {
      world
        .spawn((
          Transform::from_xy(x, 0.0),
          CharacterController::top_down(),
          Collider::rect(20.0, 40.0),
        ))
        .id()
    };
    let a = spawn(&mut world, 0.0);
    let b = spawn(&mut world, 20.0);
    // Sliding down b's side, short of its end
    for i in 0..30 {
      *world.get_mut::<Velocity>(a).unwrap() = Velocity::new(150.0, 60.0);
      run(&mut world, &mut s, 1);
      let st = state(&world, a);
      assert!(
        !st.on_floor && !st.on_ceiling && st.on_wall,
        "step {i}: {st:?}"
      );
      assert!(
        (st.wall_normal - Vec2::NEG_X).length() < 1e-3,
        "step {i}: {:?}",
        st.wall_normal
      );
      let (ta, tb) = (at(&world, a).translation, at(&world, b).translation);
      assert!(
        (-0.5..=1e-3).contains(&ta.x) && (tb - Vec2::new(20.0, 0.0)).length() < 1e-3,
        "step {i}: {ta} and {tb}"
      );
      assert!(
        (ta.y - (i + 1) as f32).abs() < 0.05,
        "step {i}: slid to {ta}"
      );
      let v = velocity(&world, a);
      assert!((v - Vec2::new(0.0, 60.0)).length() < 1e-3, "step {i}: {v}");
      assert!(!state(&world, b).on_floor);
    }
  }

  // Review focus 1: a level spawned in the same tick as the character is in
  // the first step's queries (`Physics::make_queryable`)
  #[test]
  fn a_falling_character_spawned_in_a_wall_with_the_level_is_pushed_out() {
    let (mut world, mut s) = world(60);
    world.spawn((Transform::from_xy(100.0, 0.0), Collider::rect(20.0, 400.0)));
    let e = character(&mut world, 85.0, 0.0);
    for _ in 0..5 {
      step(&mut world, &mut s, e, 980.0);
    }
    let x = at(&world, e).translation.x;
    assert!(x + 10.0 <= 90.5, "{x}");
  }

  // Review focus 1: Rapier pushes out at most a quarter of the height a step
  #[test]
  fn a_falling_character_spawned_deep_in_a_wall_is_pushed_out_over_a_few_steps() {
    let (mut world, mut s) = world(60);
    world.spawn((Transform::from_xy(100.0, 0.0), Collider::rect(20.0, 400.0)));
    let e = character(&mut world, 95.0, 0.0);
    for _ in 0..5 {
      step(&mut world, &mut s, e, 980.0);
    }
    let x = at(&world, e).translation.x;
    assert!(x + 10.0 <= 90.5, "{x}");
  }

  // Review focus 1
  #[test]
  fn a_falling_character_teleported_into_a_wall_is_pushed_out() {
    let (mut world, mut s) = world(60);
    world.spawn((Transform::from_xy(100.0, 0.0), Collider::rect(20.0, 400.0)));
    let e = character(&mut world, 0.0, -100.0);
    for _ in 0..3 {
      step(&mut world, &mut s, e, 980.0);
    }
    world.get_mut::<Transform>(e).unwrap().translation.x = 85.0;
    for _ in 0..5 {
      step(&mut world, &mut s, e, 980.0);
    }
    let x = at(&world, e).translation.x;
    assert!(x + 10.0 <= 90.5, "{x}");
  }

  #[test]
  fn a_teleport_into_the_air_forgets_the_floor() {
    let (mut world, mut s) = world(60);
    floor(&mut world, 100.0);
    let e = character(&mut world, 0.0, 79.0);
    for _ in 0..30 {
      step(&mut world, &mut s, e, 980.0);
    }
    assert!(state(&world, e).on_floor);
    world.get_mut::<Transform>(e).unwrap().translation.y = -100.0;
    let mut landings = 0;
    for i in 0..120 {
      step(&mut world, &mut s, e, 980.0);
      let st = state(&world, e);
      if i < 2 {
        assert!(
          !st.on_floor && !st.left_floor && !st.landed && st.floor.is_none(),
          "step {i} after the teleport: {st:?}"
        );
      }
      landings += usize::from(st.landed);
    }
    assert_eq!(landings, 1, "landed once");
    assert!(state(&world, e).on_floor);
  }

  // A correction (a game write to `Transform` mid-flight) must not stall the
  // character: it moves on from where it was put, in the same step
  #[test]
  fn a_teleported_character_keeps_moving_that_step() {
    let (mut world, mut s) = world(60);
    let e = character(&mut world, 0.0, 0.0);
    world.get_mut::<Velocity>(e).unwrap().dx = 300.0;
    run(&mut world, &mut s, 3);
    world.get_mut::<Transform>(e).unwrap().translation = Vec2::new(500.0, -200.0);
    *world.get_mut::<Velocity>(e).unwrap() = Velocity::new(300.0, -60.0);
    run(&mut world, &mut s, 1);
    let position = at(&world, e).translation;
    assert!(
      (position - Vec2::new(505.0, -201.0)).length() < 1e-3,
      "moved by v·dt from the teleport: {position}"
    );
    assert_eq!(velocity(&world, e), Vec2::new(300.0, -60.0));
  }

  // The turn is part of the teleport: the move starts from it, so the body
  // must end in it too (and keep it, or every step would be a teleport)
  #[test]
  fn a_teleport_that_turns_a_character_keeps_the_turn() {
    for controller in [
      CharacterController::platformer(),
      CharacterController::top_down(),
    ] {
      let (mut world, mut s) = world(60);
      let e = world
        .spawn((
          Transform::IDENTITY,
          controller,
          Collider::rect(20.0, 40.0),
          Velocity::new(60.0, 0.0),
        ))
        .id();
      run(&mut world, &mut s, 3);
      let mut transform = world.get_mut::<Transform>(e).unwrap();
      transform.translation = Vec2::new(100.0, 0.0);
      transform.rotation = 1.0;
      run(&mut world, &mut s, 1);
      let after = at(&world, e);
      assert!(
        (after.translation - Vec2::new(101.0, 0.0)).length() < 1e-3,
        "moved by v·dt from the teleport: {after:?}"
      );
      assert!(
        (after.rotation - 1.0).abs() < 1e-5,
        "kept the turn: {after:?}"
      );
      let (_, rotation) = world.resource::<Physics>().body_pose(e).unwrap();
      assert!((rotation - 1.0).abs() < 1e-5, "the body too: {rotation}");
      run(&mut world, &mut s, 3);
      let later = at(&world, e);
      assert!(
        (later.translation - Vec2::new(104.0, 0.0)).length() < 1e-3,
        "kept moving: {later:?}"
      );
      assert!(
        (later.rotation - 1.0).abs() < 1e-5,
        "still turned: {later:?}"
      );
    }
  }

  #[test]
  fn a_teleport_onto_a_floor_lands_once_on_that_step() {
    let (mut world, mut s) = world(60);
    floor(&mut world, 100.0);
    let e = character(&mut world, 0.0, -300.0);
    for _ in 0..3 {
      step(&mut world, &mut s, e, 980.0);
    }
    // Just above the floor, falling
    world.get_mut::<Transform>(e).unwrap().translation = Vec2::new(0.0, 79.0);
    step(&mut world, &mut s, e, 980.0);
    let st = state(&world, e);
    assert!(
      st.on_floor && st.landed,
      "landed on the teleport's step: {st:?}"
    );
    assert!(!st.left_floor);
    let mut landings = 0;
    for _ in 0..30 {
      step(&mut world, &mut s, e, 980.0);
      landings += usize::from(state(&world, e).landed);
    }
    assert_eq!(landings, 0, "and only then");
    assert!(state(&world, e).on_floor);
  }

  // Review focus 3
  #[test]
  fn a_fast_character_does_not_tunnel_through_a_thin_wall() {
    let (mut world, mut s) = world(60);
    world.spawn((Transform::from_xy(100.0, 0.0), Collider::rect(4.0, 400.0)));
    let e = character(&mut world, 0.0, 0.0);
    for _ in 0..10 {
      world.get_mut::<Velocity>(e).unwrap().dx = 3000.0;
      run(&mut world, &mut s, 1);
    }
    assert!(at(&world, e).translation.x + 10.0 <= 98.5);
  }

  // Review focus 5
  #[test]
  fn a_non_finite_velocity_does_not_move_a_character() {
    let (mut world, mut s) = world(60);
    let e = character(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, 1);
    world.get_mut::<Velocity>(e).unwrap().dx = f32::NAN;
    run(&mut world, &mut s, 3);
    assert_eq!(at(&world, e).translation, Vec2::ZERO);
    assert_eq!(velocity(&world, e), Vec2::ZERO);
    assert_eq!(world.resource::<Physics>().warnings, 1);
    let (position, _) = world.resource::<Physics>().body_pose(e).unwrap();
    assert!(position.is_finite());
  }

  // Rapier's own 2D sweep measures slopes signed and gets one side wrong
  // (see `sweep.rs`): these hold on both
  #[test]
  fn a_character_falling_against_a_wall_slides_down_it_on_either_side() {
    let fall: f32 = (1..=40).map(|i| 980.0 / 60.0 * i as f32 / 60.0).sum();
    for side in [1.0, -1.0] {
      let (mut world, mut s) = world(60);
      world.spawn((
        Transform::from_xy(100.0 * side, 0.0),
        Collider::rect(20.0, 2000.0),
      ));
      let e = character(&mut world, 0.0, 0.0);
      for _ in 0..40 {
        world.get_mut::<Velocity>(e).unwrap().dx = 300.0 * side;
        step(&mut world, &mut s, e, 980.0);
      }
      let t = at(&world, e).translation;
      assert!(state(&world, e).on_wall, "side {side}");
      assert!(t.x.abs() + 10.0 <= 90.5, "side {side}: {t:?}");
      assert!(
        (t.y - fall).abs() < 1.0,
        "side {side}: fell {} of {fall}",
        t.y
      );
    }
  }

  /// A `w`×`h` block two ways: a box, and its outline as a closed polyline
  /// (what a tile map makes).
  fn blocks(w: f32, h: f32) -> [(&'static str, Collider); 2] {
    let (x, y) = (w / 2.0, h / 2.0);
    let outline = [(-x, -y), (x, -y), (x, y), (-x, y), (-x, -y)];
    [
      ("box", Collider::rect(w, h)),
      (
        "outline",
        Collider::polyline(outline.map(|(x, y)| Vec2::new(x, y))),
      ),
    ]
  }

  // A slide along a wall casts from the skin, nearly tangent to the wall,
  // where parry's normal can be tens of degrees off (see `sweep.rs`): that
  // pushed the character off the wall for a step every few steps
  #[test]
  fn a_character_pressing_into_a_tall_wall_slides_down_it_on_either_side() {
    for (kind, wall) in blocks(20.0, 1000.0) {
      for side in [1.0, -1.0] {
        let (mut world, mut s) = world(60);
        world.spawn((Transform::from_xy(70.0 * side, 0.0), wall.clone()));
        let e = character(&mut world, 40.0 * side, 0.0);
        let mut against = None;
        for i in 0..120 {
          // A wall slide: pressing in a little, falling at a capped speed
          *world.get_mut::<Velocity>(e).unwrap() = Velocity::new(24.375 * side, 80.0);
          run(&mut world, &mut s, 1);
          let st = state(&world, e);
          let x = at(&world, e).translation.x;
          let Some(wall_x) = against else {
            against = st.on_wall.then_some(x);
            continue;
          };
          assert!(st.on_wall, "{kind}, side {side}, step {i}: off the wall");
          assert!(
            (st.wall_normal - Vec2::new(-side, 0.0)).length() < 1e-3,
            "{kind}, side {side}, step {i}: {:?}",
            st.wall_normal
          );
          assert!(
            (x - wall_x).abs() < 0.05,
            "{kind}, side {side}, step {i}: {x}"
          );
        }
        assert!(against.is_some(), "{kind}, side {side}: reached the wall");
      }
    }
  }

  #[test]
  fn a_steep_slope_cannot_be_walked_up_from_either_side() {
    let rise = 300.0 * 60f32.to_radians().tan();
    for side in [1.0, -1.0] {
      let (mut world, mut s) = world(60);
      world.spawn((
        Transform::IDENTITY,
        Collider::polygon([
          Vec2::new(0.0, 100.0),
          Vec2::new(300.0 * side, 100.0),
          Vec2::new(300.0 * side, 100.0 - rise),
        ]),
      ));
      floor(&mut world, 100.0);
      let e = character(&mut world, -60.0 * side, 79.0);
      for _ in 0..120 {
        world.get_mut::<Velocity>(e).unwrap().dx = 150.0 * side;
        step(&mut world, &mut s, e, 980.0);
      }
      let t = at(&world, e).translation;
      assert!(t.y > 75.0, "side {side}: climbed to {t:?}");
    }
  }

  #[test]
  fn a_ledge_is_climbed_on_either_side() {
    for side in [1.0, -1.0] {
      let (mut world, mut s) = world(60);
      floor(&mut world, 100.0);
      world.spawn((
        Transform::from_xy(140.0 * side, 96.0),
        Collider::rect(100.0, 8.0),
      ));
      let e = world
        .spawn((
          Transform::from_xy(0.0, 79.0),
          CharacterController::platformer().with_step_height(12.0),
          Collider::rect(20.0, 40.0),
        ))
        .id();
      for _ in 0..90 {
        world.get_mut::<Velocity>(e).unwrap().dx = 150.0 * side;
        step(&mut world, &mut s, e, 980.0);
      }
      let x = at(&world, e).translation.x;
      assert!(x * side > 100.0, "side {side}: x = {x}");
    }
  }

  #[test]
  fn a_jump_is_not_snapped_back_to_the_floor() {
    let (mut world, mut s) = world(60);
    floor(&mut world, 100.0);
    let e = character(&mut world, 0.0, 79.0);
    for _ in 0..30 {
      step(&mut world, &mut s, e, 980.0);
    }
    assert!(state(&world, e).on_floor);
    let y = at(&world, e).translation.y;
    world.get_mut::<Velocity>(e).unwrap().dy = -300.0;
    step(&mut world, &mut s, e, 980.0);
    let st = state(&world, e);
    assert!(st.left_floor && !st.on_floor);
    let risen = y - at(&world, e).translation.y;
    let expected = (300.0 - 980.0 / 60.0) / 60.0;
    assert!(
      (risen - expected).abs() < 0.05,
      "rose {risen} of {expected}"
    );
  }

  /// A kinematic platform 200×20 whose top is at y = 100, and a character landed on it.
  fn on_platform(world: &mut World, s: &mut EngineSchedules) -> (Entity, Entity) {
    let platform = world
      .spawn((
        Transform::from_xy(0.0, 110.0),
        RigidBody::kinematic(),
        Collider::rect(200.0, 20.0),
      ))
      .id();
    let e = character(world, 0.0, 70.0);
    for _ in 0..30 {
      step(world, s, e, 980.0);
    }
    assert!(state(world, e).on_floor);
    (platform, e)
  }

  #[test]
  fn a_character_rides_a_platform_moved_by_transform() {
    let (mut world, mut s) = world(60);
    let (platform, e) = on_platform(&mut world, &mut s);
    let x0 = at(&world, e).translation.x;
    for _ in 0..60 {
      world.get_mut::<Transform>(platform).unwrap().translation.x += 2.0;
      step(&mut world, &mut s, e, 980.0);
      assert!(state(&world, e).on_floor);
    }
    let moved = at(&world, e).translation.x - x0;
    assert!((moved - 120.0).abs() < 2.0, "{moved}");
  }

  #[test]
  fn a_character_rides_a_platform_moving_up_and_down() {
    let (mut world, mut s) = world(60);
    let (platform, e) = on_platform(&mut world, &mut s);
    let mut landings = 0;
    for i in 0..120 {
      let dy = if (i / 30) % 2 == 0 { -60.0 } else { 60.0 };
      world.get_mut::<Velocity>(platform).unwrap().dy = dy;
      step(&mut world, &mut s, e, 980.0);
      let st = state(&world, e);
      assert!(st.on_floor, "step {i}");
      landings += usize::from(st.landed);
    }
    assert_eq!(landings, 0, "never left the platform");
  }

  #[test]
  fn a_velocity_driven_platform_carries_a_character() {
    let (mut world, mut s) = world(60);
    let (platform, e) = on_platform(&mut world, &mut s);
    world.get_mut::<Velocity>(platform).unwrap().dx = 60.0;
    let x0 = at(&world, e).translation.x;
    for _ in 0..60 {
      step(&mut world, &mut s, e, 980.0);
    }
    let moved = at(&world, e).translation.x - x0;
    assert!((moved - 60.0).abs() < 2.0, "{moved}");
    assert!((state(&world, e).platform_velocity.x - 60.0).abs() < 1.0);
  }

  #[test]
  fn a_character_rides_a_turning_platform() {
    let (mut world, mut s) = world(60);
    let platform = world
      .spawn((
        Transform::from_xy(0.0, 110.0),
        RigidBody::kinematic(),
        Collider::rect(300.0, 20.0),
      ))
      .id();
    let e = character(&mut world, 100.0, 70.0);
    for _ in 0..30 {
      step(&mut world, &mut s, e, 980.0);
    }
    let before = at(&world, e).translation;
    world.get_mut::<AngularVelocity>(platform).unwrap().0 = 0.5;
    for _ in 0..30 {
      step(&mut world, &mut s, e, 980.0);
    }
    let after = at(&world, e).translation;
    assert!(state(&world, e).on_floor);
    assert!(
      after.y > before.y + 5.0,
      "carried down with the turning platform: {before:?} -> {after:?}"
    );
  }

  #[test]
  fn jumping_off_a_moving_platform_keeps_its_momentum() {
    let (mut world, mut s) = world(60);
    let (platform, e) = on_platform(&mut world, &mut s);
    world.get_mut::<Velocity>(platform).unwrap().dx = 120.0;
    for _ in 0..10 {
      step(&mut world, &mut s, e, 980.0);
    }
    world.get_mut::<Velocity>(e).unwrap().dy = -400.0;
    for _ in 0..5 {
      step(&mut world, &mut s, e, 980.0);
    }
    assert!(!state(&world, e).on_floor);
    assert!(
      (velocity(&world, e).x - 120.0).abs() < 1.0,
      "{:?}",
      velocity(&world, e)
    );
  }

  #[test]
  fn a_character_pushes_a_dynamic_crate_unless_told_not_to() {
    for push in [true, false] {
      let (mut world, mut s) = world(60);
      floor(&mut world, 100.0);
      let crate_ = world
        .spawn((
          Transform::from_xy(60.0, 90.0),
          RigidBody::dynamic().lock_rotation(),
          Collider::rect(20.0, 20.0).with_friction(0.2),
        ))
        .id();
      let e = world
        .spawn((
          Transform::from_xy(0.0, 79.0),
          CharacterController::platformer().with_push(push),
          Collider::rect(20.0, 40.0),
        ))
        .id();
      for _ in 0..90 {
        world.get_mut::<Velocity>(e).unwrap().dx = 150.0;
        step(&mut world, &mut s, e, 980.0);
      }
      let moved = at(&world, crate_).translation.x - 60.0;
      assert_eq!(moved > 20.0, push, "push {push}: crate moved {moved}");
    }
  }

  // Review focus 2
  #[test]
  fn a_character_whose_floor_is_despawned_falls() {
    let (mut world, mut s) = world(60);
    let (platform, e) = on_platform(&mut world, &mut s);
    world.despawn(platform);
    let y0 = at(&world, e).translation.y;
    for _ in 0..10 {
      step(&mut world, &mut s, e, 980.0);
    }
    let st = state(&world, e);
    assert!(!st.on_floor && st.floor.is_none());
    assert!(at(&world, e).translation.y > y0 + 5.0);
  }

  // Review focus 2: a disabled floor goes at once, as a despawned one does
  // (the broad phase keeps it until the step), and its momentum is kept
  #[test]
  fn a_character_whose_moving_floor_is_disabled_falls_at_once() {
    let (mut world, mut s) = world(60);
    let (platform, e) = on_platform(&mut world, &mut s);
    world.get_mut::<Velocity>(platform).unwrap().dx = 120.0;
    for _ in 0..5 {
      step(&mut world, &mut s, e, 980.0);
    }
    world.entity_mut(platform).insert(Disabled);
    step(&mut world, &mut s, e, 980.0);
    let st = state(&world, e);
    assert!(
      st.left_floor && !st.on_floor && st.floor.is_none(),
      "{st:?}"
    );
    assert!(
      (velocity(&world, e).x - 120.0).abs() < 1.0,
      "{:?}",
      velocity(&world, e)
    );
  }

  // The sweep sees the platform where it is, not where the step takes it:
  // the character must neither sink into a rising one nor float above a
  // sinking one, at any speed
  #[test]
  fn a_character_stays_on_top_of_a_rising_or_sinking_platform() {
    for dy in [-60.0, -300.0, 60.0, 300.0] {
      let (mut world, mut s) = world(60);
      let (platform, e) = on_platform(&mut world, &mut s);
      world.get_mut::<Velocity>(platform).unwrap().dy = dy;
      for i in 0..60 {
        step(&mut world, &mut s, e, 980.0);
        let st = state(&world, e);
        // Its own move is a step's gravity into the platform, never off it
        assert!(
          st.on_floor && !st.landed && !st.left_floor,
          "dy {dy}, step {i}: {st:?}"
        );
        let feet = at(&world, e).translation.y + 20.0;
        let top = at(&world, platform).translation.y - 10.0;
        assert!(
          (0.0..=1.0).contains(&(top - feet)),
          "dy {dy}, step {i}: feet {feet}, top {top}"
        );
      }
    }
  }

  // A jump off a lift keeps the lift's rise (momentum), so a lift rising
  // faster than the jump's top speed catches the character up while it
  // still rises in the world: against the lift it is falling onto it, and
  // it lands, once, and rides on (it was taken for leaving the lift and
  // fell through it, or landed and bounced off it)
  #[test]
  fn a_jump_off_a_fast_lift_lands_back_on_it() {
    for lift in [200.0, 300.0, 600.0] {
      for jump in [60.0, 150.0, 300.0] {
        let (mut world, mut s) = world(60);
        let (platform, e) = on_platform(&mut world, &mut s);
        world.get_mut::<Velocity>(platform).unwrap().dy = -lift;
        for _ in 0..10 {
          step(&mut world, &mut s, e, 980.0);
        }
        assert!(state(&world, e).on_floor, "lift {lift}: riding");
        world.get_mut::<Velocity>(e).unwrap().dy = -jump;
        let (mut left, mut landings) = (0, 0);
        for _ in 0..90 {
          step(&mut world, &mut s, e, 980.0);
          let st = state(&world, e);
          left += usize::from(st.left_floor);
          landings += usize::from(st.landed);
        }
        let st = state(&world, e);
        assert!(
          (left, landings) == (1, 1) && st.on_floor && st.floor == Some(platform),
          "lift {lift}, jump {jump}: left {left}, landed {landings}: {st:?}"
        );
        let feet = at(&world, e).translation.y + 20.0;
        let top = at(&world, platform).translation.y - 10.0;
        assert!(
          (0.0..=1.0).contains(&(top - feet)),
          "lift {lift}, jump {jump}: feet {feet}, top {top}"
        );
      }
    }
  }

  // An upright character on a tilting floor touches it with its high
  // corner; the carry follows the touched point round, so that corner keeps
  // its place on the platform
  #[test]
  fn a_character_keeps_its_place_on_a_turning_platform() {
    for spin in [2.0f32, -2.0] {
      let (mut world, mut s) = world(60);
      let platform = world
        .spawn((
          Transform::from_xy(0.0, 110.0),
          RigidBody::kinematic(),
          Collider::rect(400.0, 20.0),
        ))
        .id();
      let e = character(&mut world, 120.0, 70.0);
      for _ in 0..30 {
        step(&mut world, &mut s, e, 980.0);
      }
      world.get_mut::<AngularVelocity>(platform).unwrap().0 = spin;
      for i in 0..12 {
        step(&mut world, &mut s, e, 980.0);
        assert!(state(&world, e).on_floor, "spin {spin}, step {i}");
      }
      let p = at(&world, platform);
      let side = spin.signum();
      let corner = at(&world, e).translation + Vec2::new(-10.0 * side, 20.0);
      let local = Vec2::from_angle(-p.rotation).rotate(corner - p.translation);
      let stood = 120.0 - 10.0 * side;
      assert!(
        (local.x - stood).abs() < 0.5,
        "spin {spin}: the corner moved from {stood} to {} on the platform",
        local.x
      );
    }
  }

  fn one_way_platform(world: &mut World, top: f32) -> Entity {
    world
      .spawn((
        Transform::from_xy(0.0, top + 5.0),
        Collider::rect(200.0, 10.0).one_way(),
      ))
      .id()
  }

  #[test]
  fn a_character_jumps_up_through_a_one_way_platform_and_lands_on_it() {
    let (mut world, mut s) = world(60);
    floor(&mut world, 200.0);
    let platform = one_way_platform(&mut world, 100.0);
    let e = character(&mut world, 0.0, 179.0);
    for _ in 0..10 {
      step(&mut world, &mut s, e, 980.0);
    }
    world.get_mut::<Velocity>(e).unwrap().dy = -620.0;
    for _ in 0..90 {
      step(&mut world, &mut s, e, 980.0);
    }
    let st = state(&world, e);
    assert!(st.on_floor && st.floor_one_way);
    assert_eq!(st.floor, Some(platform));
    let bottom = at(&world, e).translation.y + 20.0;
    assert!((99.0..=100.0).contains(&bottom), "{bottom}");
  }

  #[test]
  fn a_character_passes_a_one_way_platform_from_the_side() {
    let (mut world, mut s) = world(60);
    one_way_platform(&mut world, 0.0);
    let e = character(&mut world, -150.0, 5.0);
    for _ in 0..60 {
      world.get_mut::<Velocity>(e).unwrap().dx = 300.0;
      run(&mut world, &mut s, 1);
    }
    assert!(at(&world, e).translation.x > 150.0);
  }

  #[test]
  fn a_character_walks_off_a_one_way_platform() {
    let (mut world, mut s) = world(60);
    one_way_platform(&mut world, 100.0);
    let e = character(&mut world, 0.0, 70.0);
    for _ in 0..90 {
      world.get_mut::<Velocity>(e).unwrap().dx = 200.0;
      step(&mut world, &mut s, e, 980.0);
    }
    assert!(!state(&world, e).on_floor);
    assert!(at(&world, e).translation.y > 120.0, "fell past the end");
  }

  #[test]
  fn drop_through_falls_through_the_platform_underfoot_and_clears() {
    let (mut world, mut s) = world(60);
    floor(&mut world, 300.0);
    one_way_platform(&mut world, 100.0);
    let e = character(&mut world, 0.0, 70.0);
    for _ in 0..30 {
      step(&mut world, &mut s, e, 980.0);
    }
    assert!(state(&world, e).floor_one_way);
    world
      .get_mut::<CharacterController>(e)
      .unwrap()
      .drop_through = true;
    for _ in 0..90 {
      step(&mut world, &mut s, e, 980.0);
    }
    assert!(at(&world, e).translation.y > 250.0, "on the floor below");
    assert!(!world.get::<CharacterController>(e).unwrap().drop_through);
  }

  // A game may clear drop_through itself; the next drop starts afresh from
  // the platform it is made from
  #[test]
  fn a_drop_the_game_cuts_short_does_not_spoil_the_next() {
    let (mut world, mut s) = world(60);
    floor(&mut world, 400.0);
    one_way_platform(&mut world, 100.0);
    let lower = one_way_platform(&mut world, 200.0);
    let e = character(&mut world, 0.0, 70.0);
    for _ in 0..30 {
      step(&mut world, &mut s, e, 980.0);
    }
    world
      .get_mut::<CharacterController>(e)
      .unwrap()
      .drop_through = true;
    for _ in 0..10 {
      step(&mut world, &mut s, e, 980.0);
    }
    world
      .get_mut::<CharacterController>(e)
      .unwrap()
      .drop_through = false;
    for _ in 0..60 {
      step(&mut world, &mut s, e, 980.0);
    }
    assert_eq!(state(&world, e).floor, Some(lower));
    world
      .get_mut::<CharacterController>(e)
      .unwrap()
      .drop_through = true;
    for _ in 0..90 {
      step(&mut world, &mut s, e, 980.0);
    }
    assert!(at(&world, e).translation.y > 350.0, "on the floor below");
    assert!(!world.get::<CharacterController>(e).unwrap().drop_through);
  }

  #[test]
  fn drop_through_on_solid_ground_does_nothing_and_clears() {
    let (mut world, mut s) = world(60);
    floor(&mut world, 100.0);
    let e = character(&mut world, 0.0, 79.0);
    for _ in 0..10 {
      step(&mut world, &mut s, e, 980.0);
    }
    world
      .get_mut::<CharacterController>(e)
      .unwrap()
      .drop_through = true;
    step(&mut world, &mut s, e, 980.0);
    assert!(state(&world, e).on_floor);
    assert!(!world.get::<CharacterController>(e).unwrap().drop_through);
  }

  #[test]
  fn a_turned_one_way_platform_blocks_a_character_along_its_own_up() {
    let (mut world, mut s) = world(60);
    // Its up turned to +X: solid against what comes from the right
    world.spawn((
      Transform::IDENTITY.with_rotation(std::f32::consts::FRAC_PI_2),
      Collider::rect(200.0, 10.0).one_way(),
    ));
    let spawn = |world: &mut World, x: f32, y: f32| {
      world
        .spawn((
          Transform::from_xy(x, y),
          CharacterController::top_down(),
          Collider::circle(8.0),
        ))
        .id()
    };
    // On separate lines, so they do not run into each other
    let from_right = spawn(&mut world, 60.0, 0.0);
    let from_left = spawn(&mut world, -60.0, 30.0);
    for _ in 0..30 {
      world.get_mut::<Velocity>(from_right).unwrap().dx = -300.0;
      world.get_mut::<Velocity>(from_left).unwrap().dx = 300.0;
      run(&mut world, &mut s, 1);
    }
    assert!(at(&world, from_right).translation.x > 10.0, "blocked");
    assert!(at(&world, from_left).translation.x > 60.0, "passed");
  }

  // Review focus 3
  #[test]
  fn a_fast_fall_lands_on_a_one_way_segment() {
    let (mut world, mut s) = world(60);
    world.spawn((
      Transform::IDENTITY,
      Collider::segment(Vec2::new(-100.0, 0.0), Vec2::new(100.0, 0.0)).one_way(),
    ));
    let e = character(&mut world, 0.0, -400.0);
    for _ in 0..30 {
      world.get_mut::<Velocity>(e).unwrap().dy = 3000.0;
      run(&mut world, &mut s, 1);
    }
    let bottom = at(&world, e).translation.y + 20.0;
    assert!((-1.0..=0.0).contains(&bottom), "{bottom}");
  }

  #[test]
  fn solid_ground_is_not_one_way() {
    let (mut world, mut s) = world(60);
    floor(&mut world, 100.0);
    let e = character(&mut world, 0.0, 79.0);
    for _ in 0..10 {
      step(&mut world, &mut s, e, 980.0);
    }
    let st = state(&world, e);
    assert!(st.on_floor && !st.floor_one_way);
  }

  // The carry is swept by the one-way rule too: a lift rising through a
  // one-way platform takes its rider up through it
  #[test]
  fn a_lift_carries_a_character_up_through_a_one_way_platform() {
    let (mut world, mut s) = world(60);
    let (lift, e) = on_platform(&mut world, &mut s);
    one_way_platform(&mut world, 30.0);
    world.get_mut::<Velocity>(lift).unwrap().dy = -120.0;
    for i in 0..60 {
      step(&mut world, &mut s, e, 980.0);
      let st = state(&world, e);
      assert!(st.on_floor && !st.landed, "step {i}: {st:?}");
      let feet = at(&world, e).translation.y + 20.0;
      let top = at(&world, lift).translation.y - 10.0;
      assert!(
        (0.0..=1.0).contains(&(top - feet)),
        "step {i}: feet {feet}, top {top}"
      );
    }
    assert_eq!(state(&world, e).floor, Some(lift));
    let feet = at(&world, e).translation.y + 20.0;
    assert!(feet < 30.0, "through the platform: {feet}");
  }

  #[test]
  fn a_lift_sinking_through_a_one_way_platform_leaves_its_rider_on_it() {
    let (mut world, mut s) = world(60);
    let (lift, e) = on_platform(&mut world, &mut s);
    let platform = one_way_platform(&mut world, 160.0);
    world.get_mut::<Velocity>(lift).unwrap().dy = 120.0;
    for _ in 0..60 {
      step(&mut world, &mut s, e, 980.0);
    }
    let st = state(&world, e);
    assert!(st.on_floor && st.floor_one_way, "{st:?}");
    assert_eq!(st.floor, Some(platform));
    let bottom = at(&world, e).translation.y + 20.0;
    assert!((159.0..=160.0).contains(&bottom), "{bottom}");
  }

  // The carry's one-way rule starts from where the character's own move
  // left it: a jump that clears a one-way platform the character stood
  // inside of is not carried back into it by the lift sinking beneath
  #[test]
  fn a_jump_clear_of_a_one_way_platform_is_not_carried_back_into_it() {
    let (mut world, mut s) = world(60);
    one_way_platform(&mut world, 0.0);
    // A lift resting part-way up through it, its top 5 px below the platform's
    let lift = world
      .spawn((
        Transform::from_xy(0.0, 15.0),
        RigidBody::kinematic(),
        Collider::rect(200.0, 20.0),
      ))
      .id();
    let e = character(&mut world, 0.0, -16.0);
    for _ in 0..30 {
      step(&mut world, &mut s, e, 980.0);
    }
    let st = state(&world, e);
    assert!(st.on_floor && st.floor == Some(lift), "{st:?}");
    world.get_mut::<Transform>(lift).unwrap().translation.y += 20.0;
    world.get_mut::<Velocity>(e).unwrap().dy = -600.0;
    step(&mut world, &mut s, e, 980.0);
    let feet = at(&world, e).translation.y + 20.0;
    assert!((-1.0..=0.0).contains(&feet), "{feet}");
  }

  #[test]
  fn a_character_stays_on_top_of_a_rising_or_sinking_one_way_platform() {
    for dy in [-60.0, -300.0, 60.0, 300.0] {
      let (mut world, mut s) = world(60);
      let platform = world
        .spawn((
          Transform::from_xy(0.0, 105.0),
          RigidBody::kinematic(),
          Collider::rect(200.0, 10.0).one_way(),
        ))
        .id();
      let e = character(&mut world, 0.0, 70.0);
      for _ in 0..30 {
        step(&mut world, &mut s, e, 980.0);
      }
      world.get_mut::<Velocity>(platform).unwrap().dy = dy;
      for i in 0..60 {
        step(&mut world, &mut s, e, 980.0);
        let st = state(&world, e);
        assert!(
          st.on_floor && st.floor_one_way && !st.landed,
          "dy {dy}, step {i}: {st:?}"
        );
        let feet = at(&world, e).translation.y + 20.0;
        let top = at(&world, platform).translation.y - 5.0;
        assert!(
          (0.0..=1.0).contains(&(top - feet)),
          "dy {dy}, step {i}: feet {feet}, top {top}"
        );
      }
    }
  }

  // A drop that lands on solid ground before it is clear of the platform is
  // over, so the next one-way platform it jumps up through holds it
  #[test]
  fn a_drop_onto_a_floor_just_below_the_platform_ends_on_landing() {
    let (mut world, mut s) = world(60);
    world.spawn((Transform::from_xy(0.0, 140.0), Collider::rect(2000.0, 20.0)));
    one_way_platform(&mut world, 100.0);
    let high = world
      .spawn((
        Transform::from_xy(400.0, 65.0),
        Collider::rect(200.0, 10.0).one_way(),
      ))
      .id();
    let e = character(&mut world, 0.0, 70.0);
    for _ in 0..30 {
      step(&mut world, &mut s, e, 980.0);
    }
    assert!(state(&world, e).floor_one_way);
    world
      .get_mut::<CharacterController>(e)
      .unwrap()
      .drop_through = true;
    for _ in 0..30 {
      step(&mut world, &mut s, e, 980.0);
    }
    let st = state(&world, e);
    assert!(st.on_floor && !st.floor_one_way, "{st:?}");
    assert!(
      !world.get::<CharacterController>(e).unwrap().drop_through,
      "over on landing"
    );
    world.get_mut::<Velocity>(e).unwrap().dx = 200.0;
    for _ in 0..120 {
      step(&mut world, &mut s, e, 980.0);
    }
    world.get_mut::<Velocity>(e).unwrap().dx = 0.0;
    world.get_mut::<Velocity>(e).unwrap().dy = -500.0;
    for _ in 0..90 {
      step(&mut world, &mut s, e, 980.0);
    }
    let st = state(&world, e);
    assert!(st.on_floor && st.floor == Some(high), "{st:?}");
  }

  // A respawn mid-drop (`Transform` and `Velocity` written) ends the drop
  #[test]
  fn a_teleport_ends_a_drop() {
    let (mut world, mut s) = world(60);
    floor(&mut world, 400.0);
    one_way_platform(&mut world, 100.0);
    let ledge = world
      .spawn((
        Transform::from_xy(500.0, -95.0),
        Collider::rect(200.0, 10.0).one_way(),
      ))
      .id();
    let e = character(&mut world, 0.0, 70.0);
    for _ in 0..30 {
      step(&mut world, &mut s, e, 980.0);
    }
    world
      .get_mut::<CharacterController>(e)
      .unwrap()
      .drop_through = true;
    for _ in 0..5 {
      step(&mut world, &mut s, e, 980.0);
    }
    // 1 px above the ledge
    world.get_mut::<Transform>(e).unwrap().translation = Vec2::new(500.0, -121.0);
    *world.get_mut::<Velocity>(e).unwrap() = Velocity::zero();
    step(&mut world, &mut s, e, 980.0);
    assert!(
      !world.get::<CharacterController>(e).unwrap().drop_through,
      "cleared on the teleport's step"
    );
    assert_eq!(state(&world, e).drop_from, None);
    for _ in 0..30 {
      step(&mut world, &mut s, e, 980.0);
    }
    let st = state(&world, e);
    assert!(st.on_floor && st.floor == Some(ledge), "{st:?}");
    assert!(!world.get::<CharacterController>(e).unwrap().drop_through);
  }

  // Only a character standing on a one-way platform drops: pressed in the
  // air, the flag clears at once and the landing it makes that step holds
  #[test]
  fn drop_through_pressed_in_the_air_does_not_skip_the_landing() {
    let (mut world, mut s) = world(60);
    let platform = one_way_platform(&mut world, 100.0);
    let e = character(&mut world, 0.0, 75.0);
    run(&mut world, &mut s, 1);
    world
      .get_mut::<CharacterController>(e)
      .unwrap()
      .drop_through = true;
    world.get_mut::<Velocity>(e).unwrap().dy = 600.0;
    run(&mut world, &mut s, 1);
    assert!(!world.get::<CharacterController>(e).unwrap().drop_through);
    for _ in 0..30 {
      step(&mut world, &mut s, e, 980.0);
    }
    let st = state(&world, e);
    assert!(st.on_floor && st.floor == Some(platform), "{st:?}");
  }

  // A round character's box sticks out along a turned direction; the rule
  // measures the shape itself
  #[test]
  fn round_characters_stand_on_a_tilted_one_way_platform() {
    for degrees in [10.0f32, 20.0] {
      for (name, collider) in [
        ("circle", Collider::circle(12.0)),
        ("capsule", Collider::capsule(40.0, 10.0)),
      ] {
        let (mut world, mut s) = world(60);
        let angle = degrees.to_radians();
        let platform = world
          .spawn((
            Transform::from_xy(0.0, 100.0).with_rotation(angle),
            Collider::rect(300.0, 10.0).one_way(),
          ))
          .id();
        let e = world
          .spawn((
            Transform::from_xy(0.0, 50.0),
            CharacterController::platformer(),
            collider,
          ))
          .id();
        for _ in 0..60 {
          step(&mut world, &mut s, e, 980.0);
        }
        let st = state(&world, e);
        assert!(
          st.on_floor && st.floor_one_way && st.floor == Some(platform),
          "{name} at {degrees}°: {st:?}"
        );
        let up = Vec2::from_angle(angle).rotate(Vec2::NEG_Y);
        let out = (at(&world, e).translation - Vec2::new(0.0, 100.0)).dot(up);
        assert!(
          out > 5.0,
          "{name} at {degrees}°: {out} along the platform's up"
        );
      }
    }
  }

  #[test]
  fn a_round_character_is_held_by_a_one_way_wall_turned_30_degrees() {
    let (mut world, mut s) = world(60);
    let angle = 30f32.to_radians();
    world.spawn((
      Transform::IDENTITY.with_rotation(angle),
      Collider::rect(200.0, 10.0).one_way(),
    ));
    let up = Vec2::from_angle(angle).rotate(Vec2::NEG_Y);
    let e = world
      .spawn((
        Transform::from_xy(up.x * 40.0, up.y * 40.0),
        CharacterController::top_down(),
        Collider::circle(8.0),
      ))
      .id();
    for _ in 0..30 {
      *world.get_mut::<Velocity>(e).unwrap() = Velocity::from_vec2(-up * 300.0);
      run(&mut world, &mut s, 1);
    }
    let out = at(&world, e).translation.dot(up);
    assert!(out > 12.5, "blocked on the solid side: {out}");
  }

  #[test]
  fn a_run_along_flat_ground_never_stalls() {
    // parry's normal for flat ground is sometimes (0, -0.99999994): a run
    // along it must not be taken for a slip down it and stopped. (A cast
    // starting near the skin can also get a normal a degree or two off,
    // which moves the run by a few thousandths of a pixel.)
    for gravity in [927.0, 980.0] {
      let (mut world, mut s) = world(60);
      world.spawn((Transform::from_xy(0.0, 110.0), Collider::rect(200.0, 20.0)));
      let e = character(&mut world, 0.0, 79.0);
      for _ in 0..30 {
        step(&mut world, &mut s, e, gravity);
      }
      for _ in 0..60 {
        if !state(&world, e).on_floor {
          break;
        }
        let x = at(&world, e).translation.x;
        world.get_mut::<Velocity>(e).unwrap().dx = 180.0;
        step(&mut world, &mut s, e, gravity);
        let ran = at(&world, e).translation.x - x;
        assert!(
          (ran - 3.0).abs() < 0.05,
          "ran {ran} from x = {x} (gravity {gravity})"
        );
      }
      assert!(!state(&world, e).on_floor, "ran off the end");
    }
  }

  // Each step of a run casts along the floor from its skin, where parry's
  // normal can be tens of degrees off (see `sweep.rs`): along a 1000 px
  // floor some steps lost up to half their run
  // A polyline is cast part by part through the start-of-cast fallback (see
  // `sweep.rs`), which drops only a part the move leaves: the walls of a
  // room's outline still stop a run along its floor
  #[test]
  fn a_run_inside_an_outline_room_stops_at_its_wall_on_either_side() {
    let room = [
      (-200.0, -100.0),
      (200.0, -100.0),
      (200.0, 100.0),
      (-200.0, 100.0),
    ];
    for side in [1.0, -1.0] {
      let (mut world, mut s) = world(60);
      world.spawn((
        Transform::from_xy(0.0, 0.0),
        Collider::polyline(room.iter().chain(&room[..1]).map(|&(x, y)| Vec2::new(x, y))),
      ));
      let e = character(&mut world, 0.0, 79.0);
      for _ in 0..120 {
        world.get_mut::<Velocity>(e).unwrap().dx = 180.0 * side;
        step(&mut world, &mut s, e, 927.0);
      }
      let st = state(&world, e);
      let x = at(&world, e).translation.x;
      assert!(x.abs() + 10.0 <= 200.0, "side {side}: {x}");
      assert!(st.on_floor && st.on_wall, "side {side}: {st:?}");
    }
  }

  #[test]
  fn a_run_along_a_long_floor_keeps_its_pace() {
    for (kind, floor) in blocks(1000.0, 20.0) {
      for x0 in [-470.0, -468.7, -452.1] {
        let (mut world, mut s) = world(60);
        world.spawn((Transform::from_xy(0.0, 110.0), floor.clone()));
        let e = character(&mut world, x0, 79.0);
        for _ in 0..30 {
          step(&mut world, &mut s, e, 927.0);
        }
        for i in 0..300 {
          let x = at(&world, e).translation.x;
          world.get_mut::<Velocity>(e).unwrap().dx = 180.0;
          step(&mut world, &mut s, e, 927.0);
          let ran = at(&world, e).translation.x - x;
          assert!(
            (ran - 3.0).abs() < 0.05,
            "{kind} from {x0}, step {i}: ran {ran} from x = {x}"
          );
          assert!(
            !state(&world, e).on_wall,
            "{kind} from {x0}, step {i}: a wall"
          );
        }
      }
    }
  }
}
