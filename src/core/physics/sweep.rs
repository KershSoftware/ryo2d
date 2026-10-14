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
//!
//! Push-out from a start inside something solid stays with Rapier
//! (`move_shape` with no translation), which measures no angle.

use rapier2d::control::{CharacterCollision, CharacterLength, KinematicCharacterController};
use rapier2d::parry::query::ShapeCastOptions;
use rapier2d::prelude::{ColliderHandle, Pose, QueryFilterFlags, QueryPipeline, Shape, Vector};

/// Rapier's cap on the casts in one move.
const MAX_CASTS: usize = 20;
/// A translation shorter than this is no move (Rapier's threshold).
pub(crate) const MIN_MOVE: f32 = 1.0e-5;
/// A surface faces up when its normal is within about 89.9° of up
/// (Rapier's grounded test).
const FACING_UP: f32 = 1.0e-3;

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
  let mut remaining = desired;
  for _ in 0..MAX_CASTS {
    let distance = remaining.length();
    if !distance.is_finite() || distance <= MIN_MOVE {
      break;
    }
    let direction = remaining / distance;
    let at = Pose::from_translation(swept.translation) * *start;
    let Some((handle, hit)) = queries.cast_shape(&at, direction, shape, cast(skin, distance))
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
  let slipping_intent = up.dot(input_along) < 0.0;
  let slipping = up.dot(along) < 0.0;
  let climbing_intent = up.dot(input) > 0.0;
  let climbing = up.dot(along) > 0.0;
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
    let nudge = across * across.dot(*remaining).min(min_width);
    *remaining -= nudge;
    swept.translation += up * height + nudge;
    true
  }
}
