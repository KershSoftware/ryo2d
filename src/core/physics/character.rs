//! The character controller: a kinematic body swept each step (Rapier's
//! `KinematicCharacterController`, through the 2D port in `sweep.rs`), with
//! floors, walls and ceilings told apart and `Velocity` left as the real
//! velocity, carried by what it stands on and pushing dynamic bodies.

use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI};

use rapier2d::control::{
  CharacterAutostep, CharacterCollision, CharacterLength, KinematicCharacterController,
};
use rapier2d::parry::bounding_volume::Aabb;
use rapier2d::parry::query::{QueryDispatcher, ShapeCastHit, ShapeCastOptions, ShapeCastStatus};
use rapier2d::prelude::{
  Collider as RapierCollider, ColliderHandle, Pose, QueryFilter as RapierFilter, QueryPipeline,
  Shape, Vector,
};

use super::body::RigidBody;
use super::convert::{from_rapier, to_rapier};
use super::world::Physics;
use super::{one_way, sweep};
use crate::core::ecs::{
  Component, DetectChangesMut, Entity, Query, Res, ResMut, Transform, Vec2, Velocity,
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
/// teleports the character (no sweep), as for any kinematic body.
///
/// A character rides what it stands on: a platform moved by `Transform` or
/// by velocity, a turning one, or a dynamic body. Leaving it (a jump, a step
/// off the edge, the platform despawned) adds the platform's velocity to
/// `Velocity`, so the momentum is kept.
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
  /// Ignore one-way platforms until fully below the one underfoot; the
  /// engine clears it then (or at once when not on a one-way platform)
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
  /// is still pushing out, or the character is new (until the first step
  /// after it registers) or teleported: the next move pushes out first
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
  /// In the air, touching nothing, and pushing out first on the next move:
  /// the state after a teleport (that step has no sweep).
  fn teleported(&mut self) {
    let mut hits = std::mem::take(&mut self.hits);
    hits.clear();
    *self = Self {
      hits,
      velocity_before: self.velocity_before,
      drop_from: self.drop_from,
      ..Self::default()
    };
  }
}

/// What `Physics::move_character` reports back to the system.
pub(crate) struct CharacterMove {
  /// The normals of every surface the move touched (for clipping knockback)
  #[expect(dead_code, reason = "read once knockback lands")]
  pub(crate) normals: Vec<Vec2>,
  /// A drop-through is over
  pub(crate) drop_done: bool,
}

/// How far below its feet a character looks for the floor, beyond its skin.
const FLOOR_PROBE: f32 = 2.0;
/// Slack on the floor and ceiling angle tests.
const ANGLE_SLACK: f32 = 1e-4;

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

/// What a character's sweep from a pose its shape fills as `start` may hit:
/// colliders enabled this tick (one disabled this tick stays in the broad
/// phase until the step, and Rapier's filter never checks), one-way ones
/// only when the character starts entirely on their solid side, and none of
/// those while it drops through.
fn solid(start: Aabb, dropping: bool) -> impl Fn(ColliderHandle, &RapierCollider) -> bool {
  move |_, collider| {
    collider.is_enabled()
      && match one_way::decode(collider.user_data) {
        None => true,
        Some(_) if dropping => false,
        Some(local) => one_way::character_above(collider, local, &start),
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
  /// update its state and leave the real velocity in `velocity`. `None`
  /// when it is not moved: not registered yet, teleported this step, or
  /// refused (logged once).
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
    if self.was_teleported(entity) {
      state.teleported();
      return None;
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
    let (start, shape, groups) = {
      let collider = self.colliders.get(collider_handle)?;
      (
        *collider.position(),
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
    let dropping = controller.drop_through;
    // A drop starts from the one-way floor stood on before this step's probe
    let stood_on_one_way = state
      .floor_collider
      .filter(|_| was_on_floor && state.floor_one_way);
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

    // The move and the probe start where the step does
    let solid_from_start = solid(shape.compute_aabb(&start), dropping);
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
    let floor_hit = platformer
      .then(|| {
        let options = ShapeCastOptions::with_max_time_of_impact(skin + FLOOR_PROBE);
        queries.cast_shape(&pose, to_rapier(Vec2::Y), &*shape, options)
      })
      .flatten();

    // The carry, past everything but the floor (it moves with the character),
    // starting where the move left it
    if let Some(carrier) = carrier {
      let solid_from_moved = solid(shape.compute_aabb(&pose), dropping);
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
    *velocity = real_velocity(wanted, &normals, platformer.then_some(up), momentum);
    state.sliding_down_slope = platformer && moved.sliding_down_slope;
    state.velocity_before = wanted;
    // Registering this step, the queries saw nothing registered with it (a
    // level spawned in the same tick): push out again once they do
    state.stuck = stuck || pushing_out || self.is_registering(entity);

    if controller.push && !collisions.is_empty() {
      self.push_bodies(&kcc, collider_handle, filter, &*shape, &collisions, dt);
    }

    let drop_done = self.track_drop(state, dropping, stood_on_one_way, &*shape, &pose);

    let delta = from_rapier(pose.translation - start.translation);
    let body_translation = from_rapier(self.bodies.get(body_handle)?.translation());
    self.drive_to(entity, body_translation + delta);
    Some(CharacterMove { normals, drop_done })
  }

  /// Keep a drop-through's bookkeeping; true when it is over. A drop
  /// starts from the one-way collider stood on (`stood_on`) and lasts until
  /// the character, at `pose`, is entirely below it; with no such collider
  /// (solid ground, the air, the collider gone) it is over at once.
  fn track_drop(
    &self,
    state: &mut CharacterState,
    dropping: bool,
    stood_on: Option<Entity>,
    shape: &dyn Shape,
    pose: &Pose,
  ) -> bool {
    if !dropping {
      state.drop_from = None;
      return false;
    }
    if state.drop_from.is_none() {
      state.drop_from = stood_on;
    }
    let end = shape.compute_aabb(pose);
    let still_over = state
      .drop_from
      .and_then(|from| self.collider_entries.get(&from))
      .and_then(|entry| self.colliders.get(entry.handle))
      .and_then(|collider| {
        one_way::decode(collider.user_data)
          .map(|local| !one_way::character_below(collider, local, &end))
      })
      .unwrap_or(false);
    if !still_over {
      state.drop_from = None;
    }
    !still_over
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
    let cos = max_slope.cos() - ANGLE_SLACK;
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
  )>,
) {
  let dt = time.fixed.timestep();
  for (entity, mut controller, mut state, mut velocity) in &mut characters {
    let mut v = velocity.as_vec2();
    let Some(moved) =
      physics.move_character(entity, &controller, &mut state, &mut v, Vec2::ZERO, dt)
    else {
      continue;
    };
    velocity.set_if_neq(Velocity::from_vec2(v));
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

  // Review focus 1: a level spawned in the same tick as the character joins
  // the broad phase at the step, so the first step's queries see none of it
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
        assert!(st.on_floor && !st.landed, "dy {dy}, step {i}: {st:?}");
        let feet = at(&world, e).translation.y + 20.0;
        let top = at(&world, platform).translation.y - 10.0;
        assert!(
          (0.0..=1.0).contains(&(top - feet)),
          "dy {dy}, step {i}: feet {feet}, top {top}"
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
}
