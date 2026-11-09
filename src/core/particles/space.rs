//! Local space: an emitter's own frame, where a `Local` effect's particles
//! live, and moving particles between it and the world.

use glam::{Mat2, Vec2};

use super::pool::{EmitterPool, Pool};
use crate::core::ecs::interpolate::render_global;
use crate::core::ecs::{DrawLift, GlobalTransform, PreviousGlobalTransform};

/// An emitter's own frame: its place (its translation plus its floor lift)
/// and its axes, turned and mirrored as it is but never stretched, so its
/// scale moves the shape only and leaves particles' speeds alone, as in
/// the world
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LocalFrame {
  pub(crate) origin: Vec2,
  /// At unit length and square to each other
  pub(crate) axes: Mat2,
}

impl LocalFrame {
  /// The frame of an emitter at `origin` whose linear part is `linear`
  pub(crate) fn new(origin: Vec2, linear: Mat2) -> Self {
    Self {
      origin,
      axes: rigid_axes(linear),
    }
  }

  /// The frame an emitter's particles draw through: its transform `alpha`
  /// of the way between its last two steps, as entities draw, lifted by
  /// its floor
  pub(crate) fn drawn(
    global: &GlobalTransform,
    previous: Option<&PreviousGlobalTransform>,
    lift: Option<&DrawLift>,
    alpha: f32,
  ) -> Self {
    let drawn = render_global(global, previous, alpha);
    let lift = lift.map_or(Vec2::ZERO, |drawn| drawn.lift);
    Self::new(drawn.translation() + lift, drawn.affine().matrix2)
  }

  /// A point of the frame, in the world
  pub(crate) fn world_point(&self, local: Vec2) -> Vec2 {
    self.origin + self.axes * local
  }

  /// A point of the world, in the frame
  pub(crate) fn local_point(&self, world: Vec2) -> Vec2 {
    self.axes.transpose() * (world - self.origin)
  }

  /// A rotation in the frame (radians), in the world
  pub(crate) fn world_rotation(&self, local: f32) -> f32 {
    if self.mirrored() {
      self.turn() - local
    } else {
      self.turn() + local
    }
  }

  /// A rotation in the world (radians), in the frame
  fn local_rotation(&self, world: f32) -> f32 {
    if self.mirrored() {
      self.turn() - world
    } else {
      world - self.turn()
    }
  }

  /// How far its x axis is turned from the world's, in radians
  fn turn(&self) -> f32 {
    self.axes.x_axis.y.atan2(self.axes.x_axis.x)
  }

  fn mirrored(&self) -> bool {
    self.axes.determinant() < 0.0
  }

  /// The world box round the box from `min` to `max` in the frame
  pub(crate) fn world_box(&self, (min, max): (Vec2, Vec2)) -> (Vec2, Vec2) {
    [min, Vec2::new(max.x, min.y), max, Vec2::new(min.x, max.y)]
      .into_iter()
      .map(|corner| self.world_point(corner))
      .fold((Vec2::INFINITY, Vec2::NEG_INFINITY), |(low, high), at| {
        (low.min(at), high.max(at))
      })
  }
}

/// `linear`'s axes at unit length and square to each other: its turn, and
/// its mirroring when it has one, with no stretch or skew. With no x axis
/// it turns by its y axis; with neither, it is the identity
fn rigid_axes(linear: Mat2) -> Mat2 {
  let x = linear
    .x_axis
    .try_normalize()
    .or_else(|| linear.y_axis.try_normalize().map(|y| -y.perp()))
    .unwrap_or(Vec2::X);
  let y = if linear.determinant() < 0.0 {
    -x.perp()
  } else {
    x.perp()
  };
  Mat2::from_cols(x, y)
}

/// Keeps `pool`'s particles in its effect's space as a step begins (its
/// effect changed space since its last step): stored in the world, they
/// move into `now`, the emitter's frame this step, for a `Local` effect;
/// stored in its last step's frame, they move out into the world for a
/// `World` one. Each stays where it was, going the way it went
pub(crate) fn settle(pool: &mut EmitterPool, now: Option<&LocalFrame>) {
  match (pool.local, now) {
    (None, Some(frame)) => {
      let turn = frame.axes.transpose();
      carry(
        &mut pool.main,
        |at| frame.local_point(at),
        |v| turn * v,
        |r| frame.local_rotation(r),
      );
    }
    (Some(last), None) => {
      carry(
        &mut pool.main,
        |at| last.world_point(at),
        |v| last.axes * v,
        |r| last.world_rotation(r),
      );
    }
    _ => {}
  }
}

/// Moves every particle of `p` into another space: its places by `place`,
/// its velocity by `turn` and its rotation by `rotate`
fn carry(
  p: &mut Pool,
  place: impl Fn(Vec2) -> Vec2,
  turn: impl Fn(Vec2) -> Vec2,
  rotate: impl Fn(f32) -> f32,
) {
  for i in 0..p.len() {
    let at = place(Vec2::new(p.x[i], p.y[i]));
    let was = place(Vec2::new(p.px[i], p.py[i]));
    let velocity = turn(Vec2::new(p.vx[i], p.vy[i]));
    (p.x[i], p.y[i]) = (at.x, at.y);
    (p.px[i], p.py[i]) = (was.x, was.y);
    (p.vx[i], p.vy[i]) = (velocity.x, velocity.y);
    p.rotation[i] = rotate(p.rotation[i]);
  }
}

#[cfg(test)]
mod tests {
  use std::f32::consts::FRAC_PI_2;

  use super::*;
  use crate::core::ecs::Transform;

  fn frame_of(transform: Transform) -> LocalFrame {
    let global = GlobalTransform::from_transform(&transform);
    LocalFrame::new(global.translation(), global.affine().matrix2)
  }

  fn close(a: Vec2, b: Vec2) -> bool {
    (a - b).abs().max_element() < 1e-4
  }

  #[test]
  fn a_frame_turns_and_mirrors_but_never_stretches() {
    let turned = frame_of(
      Transform::from_xy(10.0, 20.0)
        .with_rotation(FRAC_PI_2)
        .with_scale(Vec2::new(3.0, 0.5)),
    );
    assert!(close(
      turned.world_point(Vec2::new(1.0, 0.0)),
      Vec2::new(10.0, 21.0)
    ));
    assert!(close(
      turned.world_point(Vec2::new(0.0, 1.0)),
      Vec2::new(9.0, 20.0)
    ));
    let back = turned.local_point(turned.world_point(Vec2::new(4.0, -7.0)));
    assert!(close(back, Vec2::new(4.0, -7.0)), "{back}");
    assert!((turned.world_rotation(0.25) - (FRAC_PI_2 + 0.25)).abs() < 1e-5);

    let mirrored = frame_of(Transform::from_xy(0.0, 0.0).with_scale(Vec2::new(-2.0, 1.0)));
    assert!(close(
      mirrored.world_point(Vec2::new(1.0, 1.0)),
      Vec2::new(-1.0, 1.0)
    ));
    let r = mirrored.world_rotation(0.25);
    assert!((mirrored.local_rotation(r) - 0.25).abs() < 1e-5, "{r}");

    // Scaled to nothing, it keeps a square frame
    let flat = frame_of(Transform::from_xy(5.0, 5.0).with_scale(Vec2::ZERO));
    assert_eq!(flat.axes, Mat2::IDENTITY);
    let thin = frame_of(Transform::from_xy(0.0, 0.0).with_scale(Vec2::new(0.0, 2.0)));
    assert_eq!(thin.axes, Mat2::IDENTITY);
    let nan = LocalFrame::new(Vec2::ZERO, Mat2::from_cols(Vec2::NAN, Vec2::NAN));
    assert_eq!(nan.axes, Mat2::IDENTITY);
  }

  #[test]
  fn a_world_box_holds_every_corner_turned() {
    let turned = frame_of(Transform::from_xy(100.0, 0.0).with_rotation(FRAC_PI_2));
    let (min, max) = turned.world_box((Vec2::new(0.0, -1.0), Vec2::new(10.0, 1.0)));
    assert!(close(min, Vec2::new(99.0, 0.0)), "{min}");
    assert!(close(max, Vec2::new(101.0, 10.0)), "{max}");
  }
}
