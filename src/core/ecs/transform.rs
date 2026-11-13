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
  /// Where this anchor sits in a block, as a fraction of its size
  pub fn fraction(self) -> Vec2 {
    match self {
      Anchor::TopLeft => Vec2::new(0.0, 0.0),
      Anchor::TopCenter => Vec2::new(0.5, 0.0),
      Anchor::TopRight => Vec2::new(1.0, 0.0),
      Anchor::CenterLeft => Vec2::new(0.0, 0.5),
      Anchor::Center => Vec2::new(0.5, 0.5),
      Anchor::CenterRight => Vec2::new(1.0, 0.5),
      Anchor::BottomLeft => Vec2::new(0.0, 1.0),
      Anchor::BottomCenter => Vec2::new(0.5, 1.0),
      Anchor::BottomRight => Vec2::new(1.0, 1.0),
    }
  }

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

/// A `ParentPlaced` entity's own `ChildOf` seeds no walk, so under an
/// interpolated ancestor it takes the `PreviousGlobalTransform` a walk
/// would give it as it is spawned (skipping, as a fresh one does). The
/// search for `Interpolated` stops at an ancestor without a `Transform`, as
/// propagation's inheritance does. The stored global is never drawn while
/// `skip` is set, so the entity's current one stands in.
fn hold_previous(mut world: DeferredWorld, context: HookContext) {
  let entity = context.entity;
  if world.get::<PreviousGlobalTransform>(entity).is_some() {
    return;
  }
  let mut above = world.get::<ChildOf>(entity).map(ChildOf::parent);
  let mut interpolated = false;
  // Bounded, so a cycle (never valid) cannot hang the hook
  for _ in 0..64 {
    let Some(parent) = above else { break };
    if world.get::<Transform>(parent).is_none() {
      break;
    }
    if world.get::<Interpolated>(parent).is_some() {
      interpolated = true;
      break;
    }
    above = world.get::<ChildOf>(parent).map(ChildOf::parent);
  }
  if !interpolated {
    return;
  }
  let global = world
    .get::<GlobalTransform>(entity)
    .copied()
    .unwrap_or_default();
  world
    .commands()
    .entity(entity)
    .try_insert(PreviousGlobalTransform { global, skip: true });
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

  /// Axis lengths, never negative: a mirrored transform's reflection is
  /// told by [`is_mirrored`](Self::is_mirrored), and kept by
  /// [`to_transform`](Self::to_transform) as a negative height.
  pub fn scale(&self) -> Vec2 {
    Vec2::new(
      self.0.matrix2.x_axis.length(),
      self.0.matrix2.y_axis.length(),
    )
  }

  /// Whether it mirrors what it places: one of its axes points the other
  /// way from a turn of the other (a negative scale on one axis, on it or
  /// above it). Both axes negative is a half turn, not a mirror.
  pub fn is_mirrored(&self) -> bool {
    self.0.matrix2.determinant() < 0.0
  }

  pub fn transform_point(&self, point: Vec2) -> Vec2 {
    self.0.transform_point2(point)
  }

  /// The transform that makes this one again: its place, the turn of its
  /// x axis, and its axis lengths, the height negative when it is mirrored
  /// (a mirror along x reads as a half turn and a mirror along y). A skew,
  /// which a `Transform` cannot hold, is dropped.
  pub fn to_transform(&self) -> Transform {
    let scale = self.scale();
    let height = if self.is_mirrored() {
      -scale.y
    } else {
      scale.y
    };
    Transform {
      translation: self.translation(),
      rotation: self.rotation(),
      scale: Vec2::new(scale.x, height),
    }
  }

  /// This transform followed by a local one: a child's world transform.
  pub fn mul_transform(&self, local: &Transform) -> Self {
    Self(self.0 * local.to_affine())
  }

  /// Blend toward `other`: linear on translation and scale, shortest arc on rotation.
  ///
  /// What is the same in both stays exact (a still entity draws where it is,
  /// a moving one keeps an unchanged axis), so nothing on a pixel boundary
  /// flips between frames that blend and frames that don't.
  ///
  /// A mirror is kept: between two mirrored transforms every blend is
  /// mirrored, never turned over instead. Between one that is mirrored and
  /// one that is not (a scale that changed sign, as a character turns
  /// round), each axis goes straight to where it ends: it closes to nothing
  /// and opens the other way, with no turn.
  pub fn lerp(&self, other: &Self, t: f32) -> Self {
    // `a + (b - a) * t` is exact where a component did not change, where
    // `a * (1 - t) + b * t` can round away from it
    let mix = |a: Vec2, b: Vec2| a + (b - a) * t;
    if self.0.matrix2 == other.0.matrix2 {
      return Self(Affine2 {
        matrix2: other.0.matrix2,
        translation: mix(self.0.translation, other.0.translation),
      });
    }
    if self.is_mirrored() != other.is_mirrored() {
      let (from, to) = (self.0.matrix2, other.0.matrix2);
      return Self(Affine2 {
        matrix2: glam::Mat2::from_cols(mix(from.x_axis, to.x_axis), mix(from.y_axis, to.y_axis)),
        translation: mix(self.0.translation, other.0.translation),
      });
    }
    // Both mirrored or neither: their heights have one sign, which the
    // blend keeps
    let (a, b) = (self.to_transform(), other.to_transform());
    let mut delta = b.rotation - a.rotation;
    if delta > std::f32::consts::PI {
      delta -= std::f32::consts::TAU;
    } else if delta < -std::f32::consts::PI {
      delta += std::f32::consts::TAU;
    }
    let blended = Transform {
      translation: mix(a.translation, b.translation),
      rotation: a.rotation + delta * t,
      scale: mix(a.scale, b.scale),
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

/// The marker is public in a module nothing names, so the engine's systems
/// can take it in their filters without it being part of the API
mod parent_placed {
  use super::{Component, hold_previous};

  /// Marks an entity that only its parent's walk places: a child with no
  /// children of its own whose `Transform` the engine sets and games don't
  /// edit (a tile map's collider parts and shadow chains). Propagation's scan
  /// for changes skips it, so a still map's thousands of such children cost
  /// nothing a frame; a parent that moves still carries it, and `Transform`'s
  /// insert hook places it where it is spawned or re-set. The cost: its own
  /// `Transform`, `ChildOf` or `Interpolated` changing seeds no walk, so an
  /// edit to them waits for its parent to move.
  #[derive(Component, Debug, Clone, Copy, Default)]
  #[component(on_insert = hold_previous)]
  pub struct ParentPlaced;
}
pub(crate) use parent_placed::ParentPlaced;

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

  /// Overstep fractions a 120 Hz display gives a 60 Hz step
  const BLENDS: [f32; 6] = [0.077_4, 0.088_7, 0.098, 0.138_2, 0.5, 0.99];

  #[test]
  fn lerp_between_equal_transforms_is_exact() {
    // A still camera at a half pixel jumped a pixel on every frame that
    // blended it, when the blend rounded
    let still = [
      Transform::from_xy(432.0, 244.666_67),
      Transform::from_xy(-17.3, 0.1)
        .with_rotation(0.7)
        .with_uniform_scale(1.5),
    ];
    for transform in still {
      let g = GlobalTransform::from_transform(&transform);
      for t in BLENDS {
        assert_eq!(g.lerp(&g, t), g, "{transform:?} at {t}");
      }
    }
  }

  #[test]
  fn lerp_keeps_what_did_not_change_exact() {
    // A camera scrolling sideways at a fixed height keeps its height
    let a =
      GlobalTransform::from_transform(&Transform::from_xy(10.0, 244.666_67).with_rotation(0.7));
    let b =
      GlobalTransform::from_transform(&Transform::from_xy(20.0, 244.666_67).with_rotation(0.7));
    for t in BLENDS {
      let blend = a.lerp(&b, t);
      assert_eq!(blend.translation().y, 244.666_67, "at {t}");
      assert_eq!(blend.affine().matrix2, a.affine().matrix2, "at {t}");
    }
  }

  #[test]
  fn a_mirrored_transform_reads_as_its_turn_and_a_negative_height() {
    // Mirrored along either axis, at an angle: it says so, its lengths
    // stay lengths, and the transform it gives back makes it again
    for scale in [Vec2::new(-2.0, 3.0), Vec2::new(2.0, -3.0)] {
      let t = Transform::from_xy(10.0, -5.0)
        .with_rotation(0.3)
        .with_scale(scale);
      let g = GlobalTransform::from_transform(&t);
      assert!(g.is_mirrored(), "{scale}");
      assert!(close(g.scale(), Vec2::new(2.0, 3.0)));
      let back = g.to_transform();
      assert!(back.scale.x > 0.0 && back.scale.y < 0.0, "{:?}", back.scale);
      let again = GlobalTransform::from_transform(&back);
      assert!(
        again.affine().abs_diff_eq(g.affine(), 1e-5),
        "{scale}: {again:?} is not {g:?}"
      );
    }
    // Unmirrored ones do not: as they are, turned half a turn (both axes
    // negative), or scaled to nothing
    for scale in [Vec2::new(2.0, 3.0), Vec2::new(-2.0, -3.0), Vec2::ZERO] {
      let t = Transform::from_xy(1.0, 1.0)
        .with_rotation(0.3)
        .with_scale(scale);
      let g = GlobalTransform::from_transform(&t);
      assert!(!g.is_mirrored(), "{scale}");
      assert!(g.to_transform().scale.y >= 0.0, "{scale}");
    }
  }

  #[test]
  fn a_mirrored_transform_keeps_its_mirror_as_it_blends() {
    // Mirrored left to right, turning and widening: at every fraction it
    // is still mirrored, as far round and as wide as the fraction says
    let at = |rotation: f32, width: f32| {
      let t = Transform::from_xy(4.0, 2.0)
        .with_rotation(rotation)
        .with_scale(Vec2::new(-width, 1.0));
      GlobalTransform::from_transform(&t)
    };
    let (a, b) = (at(0.2, 1.0), at(1.0, 3.0));
    for t in [0.0, 0.25, 0.5, 0.99] {
      let blend = a.lerp(&b, t);
      assert!(blend.is_mirrored(), "at {t}: {blend:?}");
      let expected = at(0.2 + 0.8 * t, 1.0 + 2.0 * t);
      assert!(
        blend.affine().abs_diff_eq(expected.affine(), 1e-5),
        "at {t}: {blend:?} is not {expected:?}"
      );
    }
  }

  #[test]
  fn a_transform_mirrored_in_a_step_closes_and_opens_without_turning() {
    // Turned round by its x scale going from 2 to -2: its x axis goes
    // straight through nothing to the other side, and its y axis stays
    let at = |x: f32| {
      GlobalTransform::from_transform(&Transform::from_xy(5.0, 5.0).with_scale(Vec2::new(x, 1.0)))
    };
    let (a, b) = (at(2.0), at(-2.0));
    for (t, x) in [(0.25, 1.0), (0.5, 0.0), (0.75, -1.0)] {
      let blend = a.lerp(&b, t).affine();
      assert_eq!(blend.matrix2.x_axis, Vec2::new(x, 0.0), "at {t}");
      assert_eq!(blend.matrix2.y_axis, Vec2::new(0.0, 1.0), "at {t}");
      assert_eq!(blend.translation, Vec2::new(5.0, 5.0), "at {t}");
    }
  }

  #[test]
  fn anchor_pivots_are_unchanged_by_the_move() {
    assert_eq!(Anchor::default(), Anchor::Center);
    assert_eq!(Anchor::TopLeft.to_pivot(), (0.0, 0.0));
    assert_eq!(Anchor::BottomRight.to_pivot(), (1.0, 1.0));
  }
}
