//! Rapier's `KinematicCharacterController::move_shape` (rapier2d 0.36.1),
//! ported for 2D: the character's sweep.
//!
//! Rapier measures a hit's slope with the signed `Vec2::angle_to`, so in 2D
//! every surface whose normal leans to one side of `up` gets a negative
//! angle: it is never a wall (any slope rising that way can be walked up, a
//! ledge that way is never stepped) and it always holds the character (one
//! falling while it presses against a wall that way sticks to it). The port
//! measures the angle unsigned and otherwise follows Rapier, except:
//!
//! - Snapping is decided by the caller (`snap`), not by Rapier's grounded
//!   test and the direction of the move: Rapier's slide nudge lifts a flat
//!   move by a hair, which turned its snap off on the way down a slope.
//! - No kinematic-platform friction (Rapier's way of carrying a character
//!   on a moving body): carrying is the controller's job.
//! - `sliding_down_slope` is set only for a slide down a surface that faces
//!   up but is too steep to hold the character (Rapier sets it for any free
//!   slide).
//! - A slide within `LEVEL` of level neither climbs nor slips (Rapier tests
//!   the sign exactly, so float noise in a flat floor's normal stopped runs).
//! - A step up moves the character on by at least three skins (`STEP_ONTO`),
//!   not just by the run left, so it ends two skins over the step's top.
//! - parry's fallback for a hit right at the start of a cast is applied
//!   with its threshold in world units, as Rapier's nudge is, to every pair
//!   of support-mapped shapes, the parts of polylines and compounds
//!   included (`StartFallback`).
//!
//! Push-out from a start inside something solid stays with Rapier
//! (`move_shape` with no translation), which measures no angle.

use rapier2d::control::{CharacterCollision, CharacterLength, KinematicCharacterController};
use rapier2d::parry::query::details::cast_shapes_composite_shape_shape;
use rapier2d::parry::query::{
  ClosestPoints, Contact, NonlinearRigidMotion, QueryDispatcher, ShapeCastHit, ShapeCastOptions,
  ShapeCastStatus, ShapeDistance, ShapeIntersection, Unsupported,
};
use rapier2d::prelude::{ColliderHandle, Pose, QueryFilterFlags, QueryPipeline, Shape, Vector};

/// Rapier's cap on the casts in one move.
const MAX_CASTS: usize = 20;
/// A translation shorter than this is no move (Rapier's threshold).
pub(crate) const MIN_MOVE: f32 = 1.0e-5;
/// A surface faces up when its normal is within about 89.9° of up
/// (Rapier's grounded test).
const FACING_UP: f32 = 1.0e-3;
/// How far, in skins, a step up moves the character on at least: from a
/// skin short of the step's side to two skins over its top.
const STEP_ONTO: f32 = 3.0;
/// A tangent rising or falling by less than this share of its length
/// (about 0.006°) is level: it neither climbs nor slips.
const LEVEL: f32 = 1.0e-4;

/// What a sweep did.
pub(crate) struct Swept {
  /// How far the character moves
  pub(crate) translation: Vector,
  /// It slid down a surface that faces up but is too steep to hold it
  pub(crate) sliding_down_slope: bool,
}

/// A surface hit, classified against `up`.
struct Surface {
  normal: Vector,
  /// Too steep to walk up
  wall: bool,
  /// Gentle enough to hold the character
  holds: bool,
}

impl Surface {
  fn new(kcc: &KinematicCharacterController, normal: Vector) -> Self {
    let angle = slope(kcc.up, normal);
    let ceiling = kcc.up.dot(normal) < 0.0;
    Self {
      normal,
      wall: angle >= kcc.max_slope_climb_angle && !ceiling,
      holds: angle <= kcc.min_slope_slide_angle,
    }
  }
}

/// The angle between up and a surface's normal, unsigned.
fn slope(up: Vector, normal: Vector) -> f32 {
  up.angle_to(normal).abs()
}

fn eval(length: CharacterLength, size: f32) -> f32 {
  match length {
    CharacterLength::Relative(fraction) => size * fraction,
    CharacterLength::Absolute(length) => length,
  }
}

/// A shape's width across `up` and its height along it.
fn dims(shape: &dyn Shape, up: Vector) -> (f32, f32) {
  let extents = shape.compute_local_aabb().extents();
  let height = extents.dot(up.abs());
  ((extents - up.abs() * height).length(), height)
}

fn cast(skin: f32, max_distance: f32) -> ShapeCastOptions {
  ShapeCastOptions {
    target_distance: skin,
    stop_at_penetration: false,
    max_time_of_impact: max_distance,
    compute_impact_geometry_on_penetration: true,
  }
}

/// Sweep `shape` from `start` by `desired`: it moves until it hits
/// something, steps up a ledge (`kcc.autostep`) or slides along it, and
/// repeats. With `snap`, it then drops onto the ground within
/// `kcc.snap_to_ground` below. Every hit is reported to `events`, as Rapier
/// reports it.
pub(crate) fn move_shape(
  kcc: &KinematicCharacterController,
  queries: &QueryPipeline,
  shape: &dyn Shape,
  start: &Pose,
  desired: Vector,
  snap: bool,
  mut events: impl FnMut(CharacterCollision),
) -> Swept {
  let mut swept = Swept {
    translation: Vector::ZERO,
    sliding_down_slope: false,
  };
  let dims = dims(shape, kcc.up);
  let skin = eval(kcc.offset, dims.1);
  // parry's 1e-4 in world units, like the nudge (the controller scales it
  // by the length unit)
  let start_tolerance = kcc.normal_nudge_factor;
  let mut remaining = desired;
  for _ in 0..MAX_CASTS {
    let distance = remaining.length();
    if !distance.is_finite() || distance <= MIN_MOVE {
      break;
    }
    let direction = remaining / distance;
    let at = Pose::from_translation(swept.translation) * *start;
    let options = cast(skin, distance);
    let Some((handle, hit)) = first_hit(queries, &at, direction, shape, options, start_tolerance)
    else {
      swept.translation += remaining;
      break;
    };
    let allowed = direction * hit.time_of_impact;
    swept.translation += allowed;
    remaining -= allowed;
    let at = Pose::from_translation(swept.translation) * *start;
    events(CharacterCollision {
      handle,
      character_pos: at,
      translation_applied: swept.translation,
      translation_remaining: remaining,
      hit,
    });
    let surface = Surface::new(kcc, hit.normal1);
    let ledge = Ledge {
      kcc,
      queries,
      shape,
      at: &at,
      dims,
      skin,
    };
    if !ledge.step_up(handle, &surface, &mut remaining, &mut swept) {
      remaining = slide(kcc, &surface, desired, remaining, &mut swept);
    }
    if !kcc.slide {
      break;
    }
  }
  if let Some(snap_distance) = kcc.snap_to_ground.filter(|_| snap) {
    let at = Pose::from_translation(swept.translation) * *start;
    let reach = eval(snap_distance, dims.1);
    if let Some((_, hit)) = queries.cast_shape(&at, -kcc.up, shape, cast(skin, reach)) {
      swept.translation -= kcc.up * hit.time_of_impact;
    }
  }
  swept
}

/// The first collider `shape` at `at` hits moving along `direction` (a unit
/// vector), with parry's fallback for a hit right at the start of the cast
/// applied up to `tolerance` px (see `StartFallback`).
fn first_hit(
  queries: &QueryPipeline,
  at: &Pose,
  direction: Vector,
  shape: &dyn Shape,
  options: ShapeCastOptions,
  tolerance: f32,
) -> Option<(ColliderHandle, ShapeCastHit)> {
  let dispatcher = StartFallback {
    inner: queries.dispatcher,
    tolerance,
    polyline: None,
  };
  let queries = QueryPipeline {
    dispatcher: &dispatcher,
    ..*queries
  };
  queries.cast_shape(at, direction, shape, options)
}

/// The first collider `shape` at `at` meets within `distance` along
/// `direction` (a unit vector), keeping `gap` from it (0 or the skin): the
/// floor probe. Like the move's casts it passes what it starts touching or
/// overlapping unless it goes into it (`StartFallback`), so a neighbour
/// flush against the shape's side, which a probe down runs along, does not
/// hide the floor below. A cast that stops at penetration (parry's default)
/// found that neighbour first, at time zero, with its side's normal.
pub(crate) fn probe(
  queries: &QueryPipeline,
  at: &Pose,
  direction: Vector,
  shape: &dyn Shape,
  gap: f32,
  distance: f32,
  tolerance: f32,
) -> Option<(ColliderHandle, ShapeCastHit)> {
  first_hit(
    queries,
    at,
    direction,
    shape,
    cast(gap, distance),
    tolerance,
  )
}

/// parry's query dispatcher, with its fallback for a hit right at the start
/// of a cast applied up to `tolerance` px.
///
/// A cast that starts on the skin and runs nearly tangent to the surface
/// (each slide after a hit does) gets an unreliable normal from parry's GJK,
/// so for a hit within 1e-4 of the start parry takes the normal from a
/// contact query instead, and reports no hit for the pair when the move
/// leaves the surface (`cast_shapes_support_map_support_map`). That 1e-4 is
/// in world units, tuned for metres; in pixels the bad hits came at about
/// 4e-4 px, past it, with normals 10° to 45° off: a slide down a 1000 px
/// wall was pushed off it for a step every few steps, and a run along a
/// 1000 px floor lost up to half a step's run (along a polyline outline it
/// stopped). This applies the same fallback to every pair of support-mapped
/// shapes up to `tolerance` (1e-4 length units), the parts of composite
/// shapes (polylines, compounds) included: they are cast part by part
/// through it, so a part the move leaves is no hit while the rest of the
/// shape still counts. Everything else is parry's.
///
/// A polyline's part measured alone has a rounded end at each vertex: a
/// character over the next part's face, within its skin of the vertex, gets
/// a contact from that end leaning toward it, a wall at the start of every
/// cast it runs along (walking off a tile outline's step-down stalled at
/// its edge for up to half a second). So a start contact at a part's end is
/// the polyline's own contact (its closest part), as a convex shape's
/// corner would be.
struct StartFallback<'a> {
  inner: &'a dyn QueryDispatcher,
  tolerance: f32,
  /// The polyline whose parts are being cast, in their frame
  polyline: Option<&'a dyn Shape>,
}

impl StartFallback<'_> {
  /// `point` (in `part`'s frame) is at one end of the segment `part`.
  fn at_end(&self, part: &dyn Shape, point: Vector) -> bool {
    part.as_segment().is_some_and(|segment| {
      point.distance(segment.a) <= self.tolerance || point.distance(segment.b) <= self.tolerance
    })
  }
}

impl QueryDispatcher for StartFallback<'_> {
  fn intersection_test(
    &self,
    pos12: &Pose,
    g1: &dyn Shape,
    g2: &dyn Shape,
  ) -> Result<ShapeIntersection, Unsupported> {
    self.inner.intersection_test(pos12, g1, g2)
  }

  fn distance(
    &self,
    pos12: &Pose,
    g1: &dyn Shape,
    g2: &dyn Shape,
  ) -> Result<ShapeDistance, Unsupported> {
    self.inner.distance(pos12, g1, g2)
  }

  fn contact(
    &self,
    pos12: &Pose,
    g1: &dyn Shape,
    g2: &dyn Shape,
    prediction: f32,
  ) -> Result<Option<Contact>, Unsupported> {
    self.inner.contact(pos12, g1, g2, prediction)
  }

  fn closest_points(
    &self,
    pos12: &Pose,
    g1: &dyn Shape,
    g2: &dyn Shape,
    max_dist: f32,
  ) -> Result<ClosestPoints, Unsupported> {
    self.inner.closest_points(pos12, g1, g2, max_dist)
  }

  fn cast_shapes(
    &self,
    pos12: &Pose,
    local_vel12: Vector,
    g1: &dyn Shape,
    g2: &dyn Shape,
    options: ShapeCastOptions,
  ) -> Result<Option<ShapeCastHit>, Unsupported> {
    // Each part through this dispatcher, as parry casts them through its
    // own (a polyline's parts in its frame, knowing the polyline)
    if let Some(composite) = g1.as_composite_shape() {
      let parts = StartFallback {
        inner: self.inner,
        tolerance: self.tolerance,
        polyline: g1.as_polyline().map(|_| g1),
      };
      return Ok(cast_shapes_composite_shape_shape(
        &parts,
        pos12,
        local_vel12,
        composite,
        g2,
        options,
      ));
    }
    let Some(mut hit) = self
      .inner
      .cast_shapes(pos12, local_vel12, g1, g2, options)?
    else {
      return Ok(None);
    };
    let fallback = (options.compute_impact_geometry_on_penetration || !options.stop_at_penetration)
      && hit.time_of_impact < self.tolerance
      && g1.as_support_map().is_some()
      && g2.as_support_map().is_some();
    if fallback {
      // From here as parry, in the pair's frame (g1's)
      let Ok(Some(contact)) = self.inner.contact(pos12, g1, g2, f32::MAX) else {
        return Ok(Some(hit));
      };
      if !options.stop_at_penetration && contact.normal1.dot(local_vel12) >= 0.0 {
        return Ok(None);
      }
      hit.normal1 = contact.normal1;
      hit.normal2 = contact.normal2;
      hit.witness1 = contact.point1;
      hit.witness2 = contact.point2;
      hit.status = if contact.dist < options.target_distance {
        ShapeCastStatus::PenetratingOrWithinTargetDist
      } else {
        ShapeCastStatus::Converged
      };
    }
    // At a part's end: the polyline's own contact there decides, its
    // closest part at the time of impact
    if let Some(polyline) = self.polyline.filter(|_| self.at_end(g1, hit.witness1)) {
      let at = Pose::from_parts(
        pos12.translation + local_vel12 * hit.time_of_impact,
        pos12.rotation,
      );
      let reach = options.target_distance + self.tolerance;
      if let Ok(Some(closest)) = self.inner.contact(&at, polyline, g2, reach) {
        if !options.stop_at_penetration && closest.normal1.dot(local_vel12) >= 0.0 {
          return Ok(None);
        }
        hit.normal1 = closest.normal1;
        hit.normal2 = closest.normal2;
        hit.witness1 = closest.point1;
        hit.witness2 = closest.point2;
      }
    }
    Ok(Some(hit))
  }

  fn cast_shapes_nonlinear(
    &self,
    motion1: &NonlinearRigidMotion,
    g1: &dyn Shape,
    motion2: &NonlinearRigidMotion,
    g2: &dyn Shape,
    start_time: f32,
    end_time: f32,
    stop_at_penetration: bool,
  ) -> Result<Option<ShapeCastHit>, Unsupported> {
    self.inner.cast_shapes_nonlinear(
      motion1,
      g1,
      motion2,
      g2,
      start_time,
      end_time,
      stop_at_penetration,
    )
  }
}

/// `translation` split against a surface: the part leaving it (zero when
/// it goes into it) and the part along it.
fn decompose(translation: Vector, normal: Vector) -> (Vector, Vector) {
  let away = translation.dot(normal);
  let along = translation - normal * away;
  if away < 0.0 {
    (Vector::ZERO, along)
  } else {
    (normal * away, along)
  }
}

/// How far a tangent goes along `up`: positive climbing, negative slipping,
/// zero within `LEVEL` of level. parry's normal for flat ground is
/// sometimes off by float noise (`(0, -0.99999994)`, or an x of a few
/// millionths), which tilts a run along it by as much; read as a slip down
/// a surface that holds, that stopped the whole run.
fn incline(up: Vector, tangent: Vector) -> f32 {
  let rise = up.dot(tangent);
  if rise.abs() <= LEVEL * tangent.length() {
    0.0
  } else {
    rise
  }
}

/// What is left of the move after hitting `surface` (Rapier's
/// `handle_slopes`): a wall stops the climb a move would make up it, a
/// surface that holds stops the slip a fall would make down it, and
/// everything else slides; then a nudge off the surface.
fn slide(
  kcc: &KinematicCharacterController,
  surface: &Surface,
  input: Vector,
  remaining: Vector,
  swept: &mut Swept,
) -> Vector {
  let up = kcc.up;
  let (leaving, along) = decompose(remaining, surface.normal);
  let (_, input_along) = decompose(input - up * up.dot(input), surface.normal);
  let slipping_intent = incline(up, input_along) < 0.0;
  let slipping = incline(up, along) < 0.0;
  let climbing_intent = up.dot(input) > 0.0;
  let climbing = incline(up, along) > 0.0;
  let allowed = if (surface.wall && climbing && !climbing_intent)
    || (surface.holds && slipping && !slipping_intent)
  {
    leaving
  } else {
    if slipping && !surface.holds && up.dot(surface.normal) > FACING_UP {
      swept.sliding_down_slope = true;
    }
    leaving + along
  };
  allowed + surface.normal * kcc.normal_nudge_factor
}

/// Stepping up a ledge from where a cast stopped (Rapier's `handle_stairs`).
struct Ledge<'a> {
  kcc: &'a KinematicCharacterController,
  queries: &'a QueryPipeline<'a>,
  shape: &'a dyn Shape,
  at: &'a Pose,
  /// The shape's width and height
  dims: (f32, f32),
  skin: f32,
}

impl Ledge<'_> {
  /// Step up onto the ledge `handle` if it is a wall no higher than the
  /// step height, with room on top and no ramp there too steep to climb.
  fn step_up(
    &self,
    handle: ColliderHandle,
    surface: &Surface,
    remaining: &mut Vector,
    swept: &mut Swept,
  ) -> bool {
    let Some(autostep) = self.kcc.autostep else {
      return false;
    };
    if !surface.wall {
      return false;
    }
    let up = self.kcc.up;
    let min_width = eval(autostep.min_width, self.dims.0) + self.skin;
    let max_height = eval(autostep.max_height, self.dims.1) + self.skin;
    let mut queries = *self.queries;
    if !autostep.include_dynamic_bodies {
      let dynamic = queries
        .colliders
        .get(handle)
        .and_then(|collider| collider.parent())
        .and_then(|body| queries.bodies.get(body))
        .is_some_and(|body| body.is_dynamic());
      if dynamic {
        return false;
      }
      queries.filter.flags |= QueryFilterFlags::EXCLUDE_DYNAMIC;
    }
    let raised = Pose::from_parts(self.at.translation + up * max_height, self.at.rotation);
    let Some(across) = (*remaining - up * remaining.dot(up)).try_normalize() else {
      return false;
    };
    let options = |distance| cast(self.skin, distance);
    if queries
      .cast_shape(self.at, up, self.shape, options(max_height))
      .is_some()
    {
      // No headroom
      return false;
    }
    if queries
      .cast_shape(&raised, across, self.shape, options(min_width))
      .is_some()
    {
      // No room on top
      return false;
    }
    let on_top = Pose::from_parts(raised.translation + across * min_width, raised.rotation);
    let below = queries.cast_shape(&on_top, -up, self.shape, options(max_height));
    if let Some((_, hit)) = below {
      let vertical = up * remaining.dot(up);
      let along = |translation: Vector| {
        let into = (-translation).dot(hit.normal1).max(0.0) * (1.0 + 1.0e-5);
        translation + hit.normal1 * into
      };
      let climbing = up.dot(along(vertical) + along(*remaining - vertical)) >= 0.0;
      if climbing && slope(up, hit.normal1) > self.kcc.max_slope_climb_angle {
        // A ramp too steep to climb on top
        return false;
      }
    }
    let height = max_height - below.map_or(max_height, |(_, hit)| hit.time_of_impact);
    *remaining -= up * height;
    // Rapier moves on by what is left of the run; this puts the character
    // two skins onto the step at least (it stopped a skin short of the
    // step's side), within the room on top just checked. With only the run
    // left it was often a skin or less over the step's edge, a corner the
    // character slips off (see `Physics::corner_shift`) once it stops
    let run = across.dot(*remaining).min(min_width);
    let onto = run.max(STEP_ONTO * self.skin).min(min_width);
    *remaining -= across * run;
    swept.translation += up * height + across * onto;
    true
  }
}
