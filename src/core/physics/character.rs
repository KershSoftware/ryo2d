//! The character controller: a kinematic body swept each step (Rapier's
//! `KinematicCharacterController`, through the 2D port in `sweep.rs`), with
//! floors, walls and ceilings told apart and `Velocity` left as the real
//! velocity.

use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI};

use rapier2d::control::{
  CharacterAutostep, CharacterCollision, CharacterLength, KinematicCharacterController,
};
use rapier2d::parry::query::{QueryDispatcher, ShapeCastOptions, ShapeCastStatus};
use rapier2d::prelude::{QueryFilter as RapierFilter, Vector};

use super::body::RigidBody;
use super::convert::{from_rapier, to_rapier};
use super::sweep;
use super::world::Physics;
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
  /// How fast the floor moves under the character, in px/s
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
  /// Where the floor was touched, for carrying
  pub(crate) floor_point: Vec2,
  /// The collider underfoot (drop-through starts from it)
  pub(crate) floor_collider: Option<Entity>,
  /// The last move started inside something solid, or the character is new
  /// or teleported: the next move pushes out first
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

/// What `Physics::move_character` reports back to the system.
#[expect(dead_code, reason = "read once drop-through and knockback land")]
pub(crate) struct CharacterMove {
  /// The normals of every surface the move touched (for clipping knockback)
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

impl Physics {
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
      state.stuck = true;
      state.hits.clear();
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

    let filter = RapierFilter::default()
      .exclude_rigid_body(body_handle)
      .exclude_sensors()
      .groups(groups);
    let dispatcher: &dyn QueryDispatcher = self.narrow_phase.query_dispatcher();
    let queries =
      self
        .broad_phase
        .as_query_pipeline(dispatcher, &self.bodies, &self.colliders, filter);
    let mut pose = start;
    // Push out of what it overlaps first, when the last move ended inside
    // something or there is no move (the sweep's casts push out otherwise).
    // Rapier does it when the shape does not move.
    if state.stuck || desired.length() <= sweep::MIN_MOVE {
      let push_out = KinematicCharacterController {
        snap_to_ground: None,
        ..kcc
      };
      let out = push_out.move_shape(dt, &queries, &*shape, &pose, Vector::ZERO, |_| {});
      pose.translation += out.translation;
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

    let cos = max_slope.cos() - ANGLE_SLACK;
    let previous_platform = state.platform_velocity;
    state.hits.clear();
    state.on_wall = false;
    state.on_ceiling = false;
    state.wall_normal = Vec2::ZERO;
    let mut normals = Vec::with_capacity(collisions.len() + 1);
    let mut stuck = false;
    for collision in &collisions {
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
        // A floor: the probe below settles it
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
        state.on_floor = true;
        state.floor_normal = from_rapier(hit.normal1);
        state.floor = owner.map(|owner| owner.body);
        state.floor_collider = owner.map(|owner| owner.collider);
        state.floor_point = from_rapier(hit.witness1);
        normals.push(up);
      }
      None => {
        state.on_floor = false;
        state.floor_normal = Vec2::ZERO;
        state.floor = None;
        state.floor_collider = None;
      }
    }
    // Rapier's queries end here (they borrow the sets)

    // The real velocity: what runs into a wall or ceiling is removed (a slope
    // too steep to climb stops the run, see `clip`), and on a floor the fall
    // stops (the vertical part only, so standing on a gentle slope does not
    // creep down it)
    let lift = platformer.then_some(up);
    let mut real = normals
      .iter()
      .fold(wanted, |real, normal| clip(real, *normal, lift));
    state.platform_velocity = Vec2::ZERO;
    if was_on_floor && !state.on_floor {
      real += previous_platform;
    }
    state.landed = state.on_floor && !was_on_floor;
    state.left_floor = was_on_floor && !state.on_floor;
    state.sliding_down_slope = platformer && moved.sliding_down_slope;
    state.velocity_before = wanted;
    state.stuck = stuck;
    *velocity = real;

    let delta = from_rapier(pose.translation - start.translation);
    let body_translation = from_rapier(self.bodies.get(body_handle)?.translation());
    self.drive_to(entity, body_translation + delta);
    Some(CharacterMove {
      normals,
      drop_done: false,
    })
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
  for (entity, controller, mut state, mut velocity) in &mut characters {
    let mut v = velocity.as_vec2();
    let Some(_moved) =
      physics.move_character(entity, &controller, &mut state, &mut v, Vec2::ZERO, dt)
    else {
      continue;
    };
    velocity.set_if_neq(Velocity::from_vec2(v));
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ecs::{ChildOf, Transform, Velocity, World};
  use crate::core::physics::test_util::*;
  use crate::core::physics::{Collider, Physics, RigidBody};
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
}
