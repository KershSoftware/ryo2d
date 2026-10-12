//! Where things are: `Transform` is local to the parent, `GlobalTransform`
//! is world space and written only by the engine (when a `Transform` is
//! inserted, and by propagation).

use glam::{Affine2, Vec2};
use serde::{Deserialize, Serialize};

use super::kernel::{ChildOf, Component, DeferredWorld, DetectChangesMut, HookContext};

/// Which point of a sprite (or UI element) sits on its position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Anchor {
  TopLeft,
  TopCenter,
  TopRight,
  CenterLeft,
  #[default]
  Center,
  CenterRight,
  BottomLeft,
  BottomCenter,
  BottomRight,
}

impl Anchor {
  /// The top-left corner of an element of the given size placed by this
  /// anchor relative to a screen of the given size, plus an offset.
  pub fn calculate_position(
    &self,
    offset_x: f32,
    offset_y: f32,
    element_width: f32,
    element_height: f32,
    screen_width: f32,
    screen_height: f32,
  ) -> (f32, f32) {
    let (anchor_x, anchor_y) = match self {
      Anchor::TopLeft => (0.0, 0.0),
      Anchor::TopCenter => (screen_width / 2.0 - element_width / 2.0, 0.0),
      Anchor::TopRight => (screen_width - element_width, 0.0),
      Anchor::CenterLeft => (0.0, screen_height / 2.0 - element_height / 2.0),
      Anchor::Center => (
        screen_width / 2.0 - element_width / 2.0,
        screen_height / 2.0 - element_height / 2.0,
      ),
      Anchor::CenterRight => (
        screen_width - element_width,
        screen_height / 2.0 - element_height / 2.0,
      ),
      Anchor::BottomLeft => (0.0, screen_height - element_height),
      Anchor::BottomCenter => (
        screen_width / 2.0 - element_width / 2.0,
        screen_height - element_height,
      ),
      Anchor::BottomRight => (screen_width - element_width, screen_height - element_height),
    };
    (anchor_x + offset_x, anchor_y + offset_y)
  }

  /// Normalized pivot: (0, 0) top-left, (1, 1) bottom-right.
  pub fn to_pivot(self) -> (f32, f32) {
    match self {
      Anchor::TopLeft => (0.0, 0.0),
      Anchor::TopCenter => (0.5, 0.0),
      Anchor::TopRight => (1.0, 0.0),
      Anchor::CenterLeft => (0.0, 0.5),
      Anchor::Center => (0.5, 0.5),
      Anchor::CenterRight => (1.0, 0.5),
      Anchor::BottomLeft => (0.0, 1.0),
      Anchor::BottomCenter => (0.5, 1.0),
      Anchor::BottomRight => (1.0, 1.0),
    }
  }
}

/// Position, rotation (radians) and scale relative to the parent, or to the
/// world for a root entity.
#[derive(Component, Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[require(GlobalTransform)]
#[component(on_insert = place_on_insert)]
pub struct Transform {
  pub translation: Vec2,
  pub rotation: f32,
  pub scale: Vec2,
}

impl Transform {
  pub const IDENTITY: Self = Self {
    translation: Vec2::ZERO,
    rotation: 0.0,
    scale: Vec2::ONE,
  };

  pub fn from_xy(x: f32, y: f32) -> Self {
    Self::from_translation(Vec2::new(x, y))
  }

  pub fn from_translation(translation: Vec2) -> Self {
    Self {
      translation,
      ..Self::IDENTITY
    }
  }

  pub fn with_rotation(mut self, radians: f32) -> Self {
    self.rotation = radians;
    self
  }

  pub fn with_rotation_degrees(self, degrees: f32) -> Self {
    self.with_rotation(degrees.to_radians())
  }

  pub fn with_scale(mut self, scale: Vec2) -> Self {
    self.scale = scale;
    self
  }

  pub fn with_uniform_scale(self, scale: f32) -> Self {
    self.with_scale(Vec2::splat(scale))
  }

  pub fn rotation_degrees(&self) -> f32 {
    self.rotation.to_degrees()
  }

  pub fn set_rotation_degrees(&mut self, degrees: f32) {
    self.rotation = degrees.to_radians();
  }

  pub fn to_affine(&self) -> Affine2 {
    Affine2::from_scale_angle_translation(self.scale, self.rotation, self.translation)
  }
}

impl Default for Transform {
  fn default() -> Self {
    Self::IDENTITY
  }
}

/// Places an entity as soon as its `Transform` is inserted, so world-space
/// readers later in the same step (particles, navigation, lights) see it where
/// it is rather than at the origin. A child is placed under its parent's
/// current `GlobalTransform`; propagation still settles the hierarchy.
fn place_on_insert(mut world: DeferredWorld, context: HookContext) {
  let Some(local) = world.get::<Transform>(context.entity).copied() else {
    return;
  };
  let parent = world
    .get::<ChildOf>(context.entity)
    .and_then(|child_of| world.get::<GlobalTransform>(child_of.parent()))
    .copied();
  let global = match parent {
    Some(parent) => parent.mul_transform(&local),
    None => GlobalTransform::from_transform(&local),
  };
  if let Some(mut current) = world.get_mut::<GlobalTransform>(context.entity) {
    current.set_if_neq(global);
  }
}

/// World-space transform, computed by the engine from the hierarchy.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct GlobalTransform(Affine2);

impl Default for GlobalTransform {
  fn default() -> Self {
    Self::IDENTITY
  }
}

impl GlobalTransform {
  pub const IDENTITY: Self = Self(Affine2::IDENTITY);

  pub fn from_transform(transform: &Transform) -> Self {
    Self(transform.to_affine())
  }

  pub fn affine(&self) -> Affine2 {
    self.0
  }

  pub fn translation(&self) -> Vec2 {
    self.0.translation
  }

  /// Rotation in radians, taken from the x axis.
  pub fn rotation(&self) -> f32 {
    let x = self.0.matrix2.x_axis;
    x.y.atan2(x.x)
  }

  /// Axis lengths; negative scale is not represented (use sprite flips).
  pub fn scale(&self) -> Vec2 {
    Vec2::new(
      self.0.matrix2.x_axis.length(),
      self.0.matrix2.y_axis.length(),
    )
  }

  pub fn transform_point(&self, point: Vec2) -> Vec2 {
    self.0.transform_point2(point)
  }

  pub fn to_transform(&self) -> Transform {
    Transform {
      translation: self.translation(),
      rotation: self.rotation(),
      scale: self.scale(),
    }
  }

  /// This transform followed by a local one: a child's world transform.
  pub fn mul_transform(&self, local: &Transform) -> Self {
    Self(self.0 * local.to_affine())
  }

  /// Blend toward `other`: linear on translation and scale, shortest arc on rotation.
  pub fn lerp(&self, other: &Self, t: f32) -> Self {
    let (a, b) = (self.to_transform(), other.to_transform());
    let mut delta = b.rotation - a.rotation;
    if delta > std::f32::consts::PI {
      delta -= std::f32::consts::TAU;
    } else if delta < -std::f32::consts::PI {
      delta += std::f32::consts::TAU;
    }
    let blended = Transform {
      translation: a.translation.lerp(b.translation, t),
      rotation: a.rotation + delta * t,
      scale: a.scale.lerp(b.scale, t),
    };
    Self::from_transform(&blended)
  }
}

/// The previous fixed step's global transform, for render interpolation.
/// While `skip` is set the renderer draws the current transform instead.
#[derive(Component, Debug, Clone, Copy)]
pub struct PreviousGlobalTransform {
  pub global: GlobalTransform,
  pub skip: bool,
}

/// Opt an entity (and its descendants) into render interpolation.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct Interpolated;

#[cfg(test)]
mod tests {
  use super::*;
  use std::f32::consts::{FRAC_PI_2, PI};

  fn close(a: Vec2, b: Vec2) -> bool {
    (a - b).length() < 1e-4
  }

  #[test]
  fn transform_defaults_to_identity() {
    let t = Transform::default();
    assert_eq!(t, Transform::IDENTITY);
    assert_eq!(t.scale, Vec2::ONE);
  }

  #[test]
  fn degree_helpers_convert() {
    let t = Transform::from_xy(1.0, 2.0).with_rotation_degrees(90.0);
    assert!((t.rotation - FRAC_PI_2).abs() < 1e-6);
    assert!((t.rotation_degrees() - 90.0).abs() < 1e-4);
  }

  #[test]
  fn global_decomposes_translation_rotation_scale() {
    let t = Transform::from_xy(10.0, -5.0)
      .with_rotation(0.3)
      .with_scale(Vec2::new(2.0, 3.0));
    let g = GlobalTransform::from_transform(&t);
    assert!(close(g.translation(), Vec2::new(10.0, -5.0)));
    assert!((g.rotation() - 0.3).abs() < 1e-5);
    assert!(close(g.scale(), Vec2::new(2.0, 3.0)));
    let back = g.to_transform();
    assert!(close(back.translation, t.translation));
    assert!((back.rotation - t.rotation).abs() < 1e-5);
    assert!(close(back.scale, t.scale));
  }

  #[test]
  fn child_of_a_rotated_parent_lands_where_expected() {
    let parent =
      GlobalTransform::from_transform(&Transform::from_xy(100.0, 100.0).with_rotation(FRAC_PI_2));
    let child = parent.mul_transform(&Transform::from_xy(10.0, 0.0));
    assert!(close(child.translation(), Vec2::new(100.0, 110.0)));
    assert!((child.rotation() - FRAC_PI_2).abs() < 1e-5);
  }

  #[test]
  fn transform_point_applies_scale_then_rotation_then_translation() {
    let g = GlobalTransform::from_transform(
      &Transform::from_xy(1.0, 1.0)
        .with_rotation(FRAC_PI_2)
        .with_uniform_scale(2.0),
    );
    assert!(close(
      g.transform_point(Vec2::new(1.0, 0.0)),
      Vec2::new(1.0, 3.0)
    ));
  }

  #[test]
  fn lerp_blends_translation_and_takes_the_short_way_round() {
    let a = GlobalTransform::from_transform(&Transform::from_xy(0.0, 0.0).with_rotation(PI - 0.1));
    let b =
      GlobalTransform::from_transform(&Transform::from_xy(10.0, 0.0).with_rotation(-PI + 0.1));
    let mid = a.lerp(&b, 0.5);
    assert!(close(mid.translation(), Vec2::new(5.0, 0.0)));
    // The short arc passes through PI, not through 0
    assert!(
      (mid.rotation().abs() - PI).abs() < 1e-4,
      "{}",
      mid.rotation()
    );
  }

  #[test]
  fn anchor_pivots_are_unchanged_by_the_move() {
    assert_eq!(Anchor::default(), Anchor::Center);
    assert_eq!(Anchor::TopLeft.to_pivot(), (0.0, 0.0));
    assert_eq!(Anchor::BottomRight.to_pivot(), (1.0, 1.0));
  }
}
