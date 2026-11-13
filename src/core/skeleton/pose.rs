//! A skeleton's pose: each bone's local transform, and the world transforms
//! computed from them with Spine's inherit modes, plus each slot's colour and
//! attachment and the draw order.

use std::f32::consts::FRAC_PI_2;
use std::sync::Arc;

use glam::{Affine2, Mat2, Vec2, Vec4};

use super::data::{BoneData, Inherit, SkeletonData, SlotId};
use crate::core::ui::Color;

/// A bone's local transform, relative to its parent: what the setup pose and
/// animations set. Angles are radians, y down and positive rotation
/// clockwise
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BonePose {
  pub x: f32,
  pub y: f32,
  /// Radians
  pub rotation: f32,
  pub scale_x: f32,
  pub scale_y: f32,
  /// Radians
  pub shear_x: f32,
  /// Radians
  pub shear_y: f32,
}

impl BonePose {
  /// The bone's setup pose
  pub fn setup(bone: &BoneData) -> Self {
    Self {
      x: bone.x,
      y: bone.y,
      rotation: bone.rotation,
      scale_x: bone.scale_x,
      scale_y: bone.scale_y,
      shear_x: bone.shear_x,
      shear_y: bone.shear_y,
    }
  }

  /// The local linear part: the x axis turned by `rotation + shear_x` and
  /// scaled by `scale_x`, the y axis turned by `rotation + shear_y` and
  /// scaled by `scale_y`
  fn linear(&self) -> Mat2 {
    self.linear_turned(self.rotation)
  }

  /// The local linear part as if the bone were turned by `rotation`
  fn linear_turned(&self, rotation: f32) -> Mat2 {
    let (sin_x, cos_x) = (rotation + self.shear_x).sin_cos();
    let (sin_y, cos_y) = (rotation + self.shear_y).sin_cos();
    Mat2::from_cols(
      Vec2::new(cos_x, sin_x) * self.scale_x,
      Vec2::new(-sin_y, cos_y) * self.scale_y,
    )
  }
}

/// The setup draw order: every slot, in the data's order
pub(super) fn setup_order(data: &SkeletonData) -> impl Iterator<Item = SlotId> {
  (0..data.slots().len()).filter_map(|index| u8::try_from(index).ok().map(SlotId))
}

/// One skeleton's current pose, indexed like the data's bones and slots.
/// The default is the pose of no skeleton: nothing in it
#[derive(Debug, Clone, Default)]
pub(crate) struct Pose {
  /// Local transforms
  pub(crate) bones: Vec<BonePose>,
  /// The bones' transforms in skeleton space (before the root affine), from
  /// [`Pose::place_bone`]
  pub(crate) skeleton_space: Vec<Affine2>,
  /// World transforms from [`Pose::compute_world`], the root affine applied
  pub(crate) world: Vec<Affine2>,
  /// Each slot's colour, sRGB with alpha, 0 to 1 per component (a curve
  /// can overshoot). Kept in float so blends and slow fades never round;
  /// drawing clamps and converts it to 8 bits
  pub(crate) slot_colors: Vec<Vec4>,
  pub(crate) slot_attachments: Vec<Option<Arc<str>>>,
  /// Slots, back to front
  pub(crate) draw_order: Vec<SlotId>,
}

impl Pose {
  /// The setup pose, with world transforms not yet computed (identity)
  pub(crate) fn setup(data: &SkeletonData) -> Self {
    let slots = data.slots();
    Self {
      bones: data.bones().iter().map(BonePose::setup).collect(),
      skeleton_space: vec![Affine2::IDENTITY; data.bones().len()],
      world: vec![Affine2::IDENTITY; data.bones().len()],
      slot_colors: slots.iter().map(|slot| srgb(slot.color)).collect(),
      slot_attachments: slots.iter().map(|slot| slot.attachment.clone()).collect(),
      draw_order: setup_order(data).collect(),
    }
  }

  /// Puts the pose back to the setup pose without allocating: the bones'
  /// local transforms, the slots' colours and attachments and the draw
  /// order (world transforms are left as they are). The pose must be one
  /// made from `data` ([`Pose::setup`]): the skeleton step makes a new one
  /// whenever its data changes
  pub(crate) fn reset(&mut self, data: &SkeletonData) {
    debug_assert_eq!(
      self.bones.len(),
      data.bones().len(),
      "a pose of another skeleton"
    );
    debug_assert_eq!(
      self.slot_attachments.len(),
      data.slots().len(),
      "a pose of another skeleton"
    );
    for (pose, bone) in self.bones.iter_mut().zip(data.bones()) {
      *pose = BonePose::setup(bone);
    }
    for (color, slot) in self.slot_colors.iter_mut().zip(data.slots()) {
      *color = srgb(slot.color);
    }
    for (attachment, slot) in self.slot_attachments.iter_mut().zip(data.slots()) {
      attachment.clone_from(&slot.attachment);
    }
    self.draw_order.clear();
    self.draw_order.extend(setup_order(data));
  }

  /// Computes every bone's world transform from the local poses. `root`
  /// places the whole skeleton (the entity's transform). The inherit modes
  /// act in skeleton space, so `root` applies to every bone, whatever its
  /// mode (the module notes have the formulas)
  pub(crate) fn compute_world(&mut self, data: &SkeletonData, root: Affine2) {
    // Bones are parent-first, so one pass sees each parent's skeleton-space
    // transform before its children; a second applies the root.
    for index in 0..data.bones().len() {
      self.place_bone(data, index);
    }
    self.apply_root(root);
  }

  /// Computes bone `index`'s transform in skeleton space from its local
  /// pose and its parent's, which must have been placed already (bones are
  /// parent-first)
  pub(crate) fn place_bone(&mut self, data: &SkeletonData, index: usize) {
    debug_assert_eq!(
      self.bones.len(),
      data.bones().len(),
      "a pose of another skeleton"
    );
    debug_assert_eq!(
      self.skeleton_space.len(),
      data.bones().len(),
      "a pose of another skeleton"
    );
    let bone = &data.bones()[index];
    let local = &self.bones[index];
    let position = Vec2::new(local.x, local.y);
    self.skeleton_space[index] = match bone.parent {
      None => Affine2::from_mat2_translation(local.linear(), position),
      Some(parent) => {
        let parent = self.skeleton_space[parent.index()];
        let translation = parent.transform_point2(position);
        let linear = match bone.inherit {
          Inherit::Normal => parent.matrix2 * local.linear(),
          Inherit::OnlyTranslation => local.linear(),
          Inherit::NoRotationOrReflection => stretch_along_bone(parent.matrix2) * local.linear(),
          Inherit::NoScale => rigid(parent.matrix2, local, true),
          Inherit::NoScaleOrReflection => rigid(parent.matrix2, local, false),
        };
        Affine2::from_mat2_translation(linear, translation)
      }
    };
  }

  /// Computes every bone's world transform from its skeleton-space one:
  /// `root` applied to each
  pub(crate) fn apply_root(&mut self, root: Affine2) {
    debug_assert_eq!(
      self.world.len(),
      self.skeleton_space.len(),
      "a pose of another skeleton"
    );
    for (world, placed) in self.world.iter_mut().zip(&self.skeleton_space) {
      *world = root * *placed;
    }
  }

  /// The transform bone `index`'s position is placed through, in skeleton
  /// space: its parent's (translation is inherited in every mode), or the
  /// identity for a root. The parent must have been placed already
  pub(crate) fn parent_space(&self, data: &SkeletonData, index: usize) -> Affine2 {
    match data.bones()[index].parent {
      Some(parent) => self.skeleton_space[parent.index()],
      None => Affine2::IDENTITY,
    }
  }

  /// The linear map that takes bone `index`'s own direction (its local
  /// rotation, as a unit vector) to the way its x axis points in skeleton
  /// space, by its inherit mode: its parent's linear part under normal
  /// inheritance and the no-scale modes (which point the bone where normal
  /// inheritance does), the parent's stretch alone when it inherits no
  /// rotation, and the identity when it inherits only translation, or has
  /// no parent. The parent must have been placed already
  pub(crate) fn direction_space(&self, data: &SkeletonData, index: usize) -> Mat2 {
    let bone = &data.bones()[index];
    let Some(parent) = bone.parent else {
      return Mat2::IDENTITY;
    };
    let parent = self.skeleton_space[parent.index()].matrix2;
    match bone.inherit {
      Inherit::Normal | Inherit::NoScale | Inherit::NoScaleOrReflection => parent,
      Inherit::OnlyTranslation => Mat2::IDENTITY,
      Inherit::NoRotationOrReflection => stretch_along_bone(parent),
    }
  }
}

/// An 8-bit sRGB colour as floats, 0 to 1 per component, still sRGB
pub(crate) fn srgb(color: Color) -> Vec4 {
  Vec4::new(
    f32::from(color.r),
    f32::from(color.g),
    f32::from(color.b),
    f32::from(color.a),
  ) / 255.0
}

/// Below this, an axis counts as collapsed (a zero scale)
const COLLAPSED: f32 = 1e-6;

/// The parent's stretch without its rotation, shear or reflection: its
/// length along the parent bone's direction (its x axis), and its area per
/// unit length across it, as a symmetric matrix along those axes,
/// `R(θ) * diag(s, u) * R(-θ)`
fn stretch_along_bone(parent: Mat2) -> Mat2 {
  let s = parent.x_axis.length();
  let (along, across, u) = if s >= COLLAPSED {
    let along = parent.x_axis / s;
    (along, along.perp(), parent.determinant().abs() / s)
  } else {
    // No x axis: the bone's direction is a quarter turn back from its y axis
    let u = parent.y_axis.length();
    if u < COLLAPSED {
      return Mat2::ZERO;
    }
    let across = parent.y_axis / u;
    (-across.perp(), across, u)
  };
  let along_part = Mat2::from_cols(along * along.x, along * along.y) * s;
  let across_part = Mat2::from_cols(across * across.x, across * across.y) * u;
  along_part + across_part
}

/// A rigid world linear part for a no-scale bone: its x axis points where
/// normal inheritance would point it (the parent's linear part applied to
/// the bone's own direction), its y axis is perpendicular, a quarter turn
/// clockwise from it or, keeping a mirrored parent's reflection, the other
/// way; then the bone's own scale and shear apply in that frame
fn rigid(parent: Mat2, local: &BonePose, keep_reflection: bool) -> Mat2 {
  let (sin, cos) = (local.rotation + local.shear_x).sin_cos();
  let own = Vec2::new(cos, sin);
  let mut direction = parent * own;
  if direction.length() < COLLAPSED {
    // The parent squashes this direction flat: turn the bone by the parent's
    // rotation instead, from whichever of its axes is left (none: unturned)
    let parent_angle = if parent.x_axis.length() >= COLLAPSED {
      parent.x_axis.to_angle()
    } else if parent.y_axis.length() >= COLLAPSED {
      parent.y_axis.to_angle() - FRAC_PI_2
    } else {
      0.0
    };
    direction = Vec2::from_angle(parent_angle).rotate(own);
  }
  let x_axis = direction.normalize_or_zero();
  let mut y_axis = x_axis.perp();
  if keep_reflection && parent.determinant() < 0.0 {
    y_axis = -y_axis;
  }
  // The bone's own scale and shear, measured from its own x axis
  Mat2::from_cols(x_axis, y_axis) * local.linear_turned(-local.shear_x)
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::skeleton::{BoneSetup, SlotSetup};

  const DEG_30: f32 = std::f32::consts::FRAC_PI_6;
  const DEG_45: f32 = std::f32::consts::FRAC_PI_4;
  const COS_30: f32 = 0.866_025_4;
  const SIN_45: f32 = std::f32::consts::FRAC_1_SQRT_2;

  fn world(bones: &[(&str, Option<&str>, BoneSetup)], root: Affine2) -> Vec<Affine2> {
    let mut builder = SkeletonData::builder();
    for (name, parent, setup) in bones {
      builder = builder.bone(*name, *parent, *setup);
    }
    let data = builder.build().expect("a valid skeleton");
    let mut pose = Pose::setup(&data);
    pose.compute_world(&data, root);
    pose.world
  }

  fn affine(x_axis: (f32, f32), y_axis: (f32, f32), translation: (f32, f32)) -> Affine2 {
    Affine2::from_cols(
      Vec2::new(x_axis.0, x_axis.1),
      Vec2::new(y_axis.0, y_axis.1),
      Vec2::new(translation.0, translation.1),
    )
  }

  fn assert_affine(actual: Affine2, expected: Affine2) {
    assert!(
      actual.abs_diff_eq(expected, 1e-4),
      "expected {expected:?}, got {actual:?}"
    );
  }

  /// A parent at (10, 20), turned 90 degrees, scaled (2, 1): its x axis is
  /// 2 * (cos 90, sin 90) = (0, 2) and its y axis 1 * (-sin 90, cos 90) =
  /// (-1, 0)
  fn turned_parent() -> BoneSetup {
    BoneSetup {
      x: 10.0,
      y: 20.0,
      rotation: FRAC_PI_2,
      scale_x: 2.0,
      ..BoneSetup::default()
    }
  }

  #[test]
  fn normal_inherit_composes_parent_and_child() {
    let child = BoneSetup {
      x: 5.0,
      rotation: FRAC_PI_2,
      ..BoneSetup::default()
    };
    let world = world(
      &[
        ("parent", None, turned_parent()),
        ("child", Some("parent"), child),
      ],
      Affine2::IDENTITY,
    );
    assert_affine(world[0], affine((0.0, 2.0), (-1.0, 0.0), (10.0, 20.0)));
    // Child local: x axis (0, 1), y axis (-1, 0), at (5, 0). World = P * L:
    // x axis = P * (0, 1) = 0 * (0, 2) + 1 * (-1, 0) = (-1, 0)
    // y axis = P * (-1, 0) = -(0, 2) = (0, -2)
    // origin = P * (5, 0) + (10, 20) = (0, 10) + (10, 20) = (10, 30)
    assert_affine(world[1], affine((-1.0, 0.0), (0.0, -2.0), (10.0, 30.0)));
  }

  #[test]
  fn only_translation_ignores_parent_rotation_and_scale() {
    let child = BoneSetup {
      x: 5.0,
      rotation: DEG_30,
      inherit: Inherit::OnlyTranslation,
      ..BoneSetup::default()
    };
    let world = world(
      &[
        ("parent", None, turned_parent()),
        ("child", Some("parent"), child),
      ],
      Affine2::IDENTITY,
    );
    // origin = P * (5, 0) + (10, 20) = (10, 30), as under normal inherit
    // axes = the child's own: x (cos 30, sin 30), y (-sin 30, cos 30)
    assert_affine(
      world[1],
      affine((COS_30, 0.5), (-0.5, COS_30), (10.0, 30.0)),
    );
  }

  #[test]
  fn no_rotation_or_reflection_keeps_world_angle_under_a_flipped_parent() {
    // Parent turned 90 degrees, scaled (-2, 2): x axis -2 * (0, 1) = (0, -2),
    // y axis 2 * (-1, 0) = (-2, 0), determinant 0 * 0 - (-2) * (-2) = -4
    let parent = BoneSetup {
      rotation: FRAC_PI_2,
      scale_x: -2.0,
      scale_y: 2.0,
      ..BoneSetup::default()
    };
    let child = BoneSetup {
      x: 4.0,
      rotation: DEG_30,
      inherit: Inherit::NoRotationOrReflection,
      ..BoneSetup::default()
    };
    let world = world(
      &[("parent", None, parent), ("child", Some("parent"), child)],
      Affine2::IDENTITY,
    );
    // The parent bone points up (its x axis is (0, -2)) and is stretched 2
    // along itself and 2 across: a uniform 2, mirrored. Without its rotation
    // and mirroring, the child keeps its own 30 degrees, scaled 2:
    // axes x 2 * (cos 30, sin 30), y 2 * (-sin 30, cos 30)
    // origin = P * (4, 0) = 4 * (0, -2) = (0, -8)
    assert_affine(
      world[1],
      affine((2.0 * COS_30, 1.0), (-1.0, 2.0 * COS_30), (0.0, -8.0)),
    );
  }

  #[test]
  fn no_rotation_or_reflection_stretches_along_the_parent_bone() {
    let child = BoneSetup {
      inherit: Inherit::NoRotationOrReflection,
      ..BoneSetup::default()
    };
    let world = world(
      &[
        ("parent", None, turned_parent()),
        ("child", Some("parent"), child),
      ],
      Affine2::IDENTITY,
    );
    // The parent bone points down (turned 90 degrees) and is stretched 2
    // along itself, 1 across. The unturned child keeps its world angle: its
    // x axis (1, 0) lies across the parent bone, so stays (1, 0); its y axis
    // (0, 1) lies along it, so is stretched to (0, 2).
    // origin = the parent's, (10, 20)
    assert_affine(world[1], affine((1.0, 0.0), (0.0, 2.0), (10.0, 20.0)));
  }

  #[test]
  fn no_scale_keeps_unit_length_under_a_scaled_parent() {
    // Parent at (10, 0), turned 90 degrees, scaled (2, 3): x axis (0, 2),
    // y axis (-3, 0)
    let parent = BoneSetup {
      x: 10.0,
      rotation: FRAC_PI_2,
      scale_x: 2.0,
      scale_y: 3.0,
      ..BoneSetup::default()
    };
    let child = BoneSetup {
      x: 5.0,
      inherit: Inherit::NoScale,
      ..BoneSetup::default()
    };
    let world = world(
      &[("parent", None, parent), ("child", Some("parent"), child)],
      Affine2::IDENTITY,
    );
    // Normal inheritance would point the unturned child along the parent
    // bone, which points down: x axis (0, 1), unit length, and y axis
    // perpendicular, a quarter turn clockwise from it: (-1, 0).
    // origin = P * (5, 0) + (10, 0) = (0, 10) + (10, 0) = (10, 10)
    assert_affine(world[1], affine((0.0, 1.0), (-1.0, 0.0), (10.0, 10.0)));
  }

  #[test]
  fn no_scale_or_reflection_ignores_a_negative_parent_scale() {
    // Parent scaled (-2, 1): x axis (-2, 0), y axis (0, 1), determinant -2
    let parent = BoneSetup {
      scale_x: -2.0,
      ..BoneSetup::default()
    };
    let child = BoneSetup {
      x: 3.0,
      rotation: FRAC_PI_2,
      inherit: Inherit::NoScaleOrReflection,
      ..BoneSetup::default()
    };
    let mirrored = BoneSetup {
      inherit: Inherit::NoScale,
      ..child
    };
    let world = world(
      &[
        ("parent", None, parent),
        ("child", Some("parent"), child),
        ("mirrored", Some("parent"), mirrored),
      ],
      Affine2::IDENTITY,
    );
    // Normal inheritance points the child (turned 90 degrees) straight down:
    // the parent's mirroring is across the vertical, which leaves down alone.
    // So the x axis is (0, 1), unit. Without reflection the y axis is a
    // quarter turn clockwise from it, (-1, 0), as unmirrored axes are.
    // origin = P * (3, 0) = (-6, 0)
    assert_affine(world[1], affine((0.0, 1.0), (-1.0, 0.0), (-6.0, 0.0)));
    // No scale keeps the mirroring: the y axis is the other perpendicular,
    // (1, 0), where normal inheritance puts it (P * (-1, 0) = (2, 0)) at unit
    // length.
    assert_affine(world[2], affine((0.0, 1.0), (1.0, 0.0), (-6.0, 0.0)));
  }

  #[test]
  fn no_scale_stays_rigid_under_a_chain_with_non_uniform_scale() {
    // A body scaled (2, 1) and an arm turned 45 degrees under it: the arm's
    // world axes are skewed, 2 * cos 45 = (1.414, 0.707) and (-1.414, 0.707).
    let body = BoneSetup {
      scale_x: 2.0,
      ..BoneSetup::default()
    };
    let arm = BoneSetup {
      rotation: DEG_45,
      ..BoneSetup::default()
    };
    let hand = BoneSetup {
      rotation: DEG_30,
      inherit: Inherit::NoScale,
      ..BoneSetup::default()
    };
    let normal_hand = BoneSetup {
      inherit: Inherit::Normal,
      ..hand
    };
    let world = world(
      &[
        ("body", None, body),
        ("arm", Some("body"), arm),
        ("hand", Some("arm"), hand),
        ("normal hand", Some("arm"), normal_hand),
      ],
      Affine2::IDENTITY,
    );
    let hand = world[2].matrix2;
    let normal = world[3].matrix2;
    // Unit length and perpendicular, whatever the chain's skew
    assert!((hand.x_axis.length() - 1.0).abs() < 1e-4, "{hand:?}");
    assert!((hand.y_axis.length() - 1.0).abs() < 1e-4, "{hand:?}");
    assert!(hand.x_axis.dot(hand.y_axis).abs() < 1e-4, "{hand:?}");
    // Pointing where normal inheritance points it, unmirrored (the arm's
    // determinant is positive)
    let direction = normal.x_axis.normalize();
    assert!(
      hand.x_axis.abs_diff_eq(direction, 1e-4),
      "{hand:?} vs {normal:?}"
    );
    assert!(hand.y_axis.abs_diff_eq(direction.perp(), 1e-4), "{hand:?}");
  }

  #[test]
  fn no_scale_keeps_full_size_under_a_parent_scaled_to_zero_on_one_axis() {
    // Parent turned 90 degrees, scaled (0, 2): no x axis, y axis (-2, 0)
    let parent = BoneSetup {
      rotation: FRAC_PI_2,
      scale_x: 0.0,
      scale_y: 2.0,
      ..BoneSetup::default()
    };
    let child = BoneSetup {
      inherit: Inherit::NoScale,
      ..BoneSetup::default()
    };
    let collapsed = BoneSetup {
      scale_y: 0.0,
      ..parent
    };
    let world = world(
      &[
        ("parent", None, parent),
        ("child", Some("parent"), child),
        ("collapsed", None, collapsed),
        ("orphan", Some("collapsed"), child),
      ],
      Affine2::IDENTITY,
    );
    // The parent is still turned 90 degrees (its y axis points left, a
    // quarter turn clockwise from down), so the unturned child points down
    // at full size: x axis (0, 1), y axis (-1, 0).
    assert_affine(world[1], affine((0.0, 1.0), (-1.0, 0.0), (0.0, 0.0)));
    // With both parent axes zero there is no direction left: the child keeps
    // its own axes, unturned and unit.
    assert_affine(world[3], affine((1.0, 0.0), (0.0, 1.0), (0.0, 0.0)));
  }

  #[test]
  fn shear_skews_the_child_axes() {
    // Root sheared 30 degrees on x and 45 on y: x axis (cos 30, sin 30),
    // y axis (-sin 45, cos 45)
    let root = BoneSetup {
      shear_x: DEG_30,
      shear_y: DEG_45,
      ..BoneSetup::default()
    };
    let child = BoneSetup {
      x: 2.0,
      rotation: FRAC_PI_2,
      ..BoneSetup::default()
    };
    let unturned = BoneSetup {
      inherit: Inherit::NoRotationOrReflection,
      ..BoneSetup::default()
    };
    let world = world(
      &[
        ("root", None, root),
        ("child", Some("root"), child),
        ("unturned", Some("root"), unturned),
      ],
      Affine2::IDENTITY,
    );
    assert_affine(
      world[0],
      affine((COS_30, 0.5), (-SIN_45, SIN_45), (0.0, 0.0)),
    );
    // Child local: x (0, 1), y (-1, 0), at (2, 0).
    // x axis = P * (0, 1) = P's y axis = (-sin 45, cos 45)
    // y axis = P * (-1, 0) = -P's x axis = (-cos 30, -sin 30)
    // origin = P * (2, 0) = (2 cos 30, 2 sin 30)
    assert_affine(
      world[1],
      affine((-SIN_45, SIN_45), (-COS_30, -0.5), (2.0 * COS_30, 1.0)),
    );
    // Without rotation or reflection the child stretches along the parent
    // bone, which points at 30 degrees: by its length, 1, along it, and
    // across it by the parent's area per unit length,
    // |x axis x y axis| = cos 30 cos 45 + sin 30 sin 45 = cos 15 = 0.965926.
    // Along d = (cos 30, sin 30), across n = (-sin 30, cos 30):
    // (1, 0) = cos 30 d - sin 30 n -> cos 30 d - 0.965926 sin 30 n
    //   = (0.75 + 0.25 * 0.965926, cos 30 (0.5 - 0.5 * 0.965926))
    //   = (0.991481, 0.014755)
    // (0, 1) = sin 30 d + cos 30 n -> sin 30 d + 0.965926 cos 30 n
    //   = (cos 30 (0.5 - 0.5 * 0.965926), 0.25 + 0.75 * 0.965926)
    //   = (0.014755, 0.974444)
    assert_affine(
      world[2],
      affine((0.991_481, 0.014_755), (0.014_755, 0.974_444), (0.0, 0.0)),
    );
  }

  #[test]
  fn the_root_affine_places_the_whole_skeleton() {
    let root_bone = BoneSetup {
      x: 10.0,
      ..BoneSetup::default()
    };
    let child = BoneSetup {
      x: 5.0,
      inherit: Inherit::OnlyTranslation,
      ..BoneSetup::default()
    };
    // Root affine: scale 2, turned 90 degrees, at (100, 50): x axis (0, 2),
    // y axis (-2, 0)
    let root =
      Affine2::from_scale_angle_translation(Vec2::splat(2.0), FRAC_PI_2, Vec2::new(100.0, 50.0));
    let world = world(
      &[("root", None, root_bone), ("child", Some("root"), child)],
      root,
    );
    // In skeleton space the root bone is unturned at (10, 0) and the child
    // unturned at (15, 0). The root affine applies to both, even the
    // only-translation child:
    // root bone origin = A * (10, 0) + (100, 50) = (0, 20) + (100, 50) = (100, 70)
    // child origin = A * (15, 0) + (100, 50) = (0, 30) + (100, 50) = (100, 80)
    assert_affine(world[0], affine((0.0, 2.0), (-2.0, 0.0), (100.0, 70.0)));
    assert_affine(world[1], affine((0.0, 2.0), (-2.0, 0.0), (100.0, 80.0)));
  }

  #[test]
  fn setup_takes_slot_colours_attachments_and_draw_order() {
    let red = Color::new(255, 0, 0, 128);
    let data = SkeletonData::builder()
      .bone("root", None, BoneSetup::default())
      .slot("back", "root", SlotSetup::default())
      .slot(
        "front",
        "root",
        SlotSetup {
          color: red,
          attachment: Some(Arc::from("hat")),
          ..SlotSetup::default()
        },
      )
      .build()
      .expect("a valid skeleton");
    let pose = Pose::setup(&data);
    assert_eq!(
      pose.slot_colors,
      vec![Vec4::ONE, Vec4::new(1.0, 0.0, 0.0, 128.0 / 255.0)]
    );
    assert_eq!(pose.slot_attachments, vec![None, Some(Arc::from("hat"))]);
    assert_eq!(pose.draw_order, vec![SlotId(0), SlotId(1)]);
    assert_eq!(pose.bones, vec![BonePose::setup(&data.bones()[0])]);
  }

  #[test]
  fn reset_puts_a_changed_pose_back_to_setup() {
    let data = SkeletonData::builder()
      .bone("root", None, turned_parent())
      .slot("back", "root", SlotSetup::default())
      .slot(
        "front",
        "root",
        SlotSetup {
          color: Color::new(255, 0, 0, 128),
          attachment: Some(Arc::from("hat")),
          ..SlotSetup::default()
        },
      )
      .build()
      .expect("a valid skeleton");
    let setup = Pose::setup(&data);
    let mut pose = setup.clone();
    pose.bones[0].x = -3.0;
    pose.bones[0].rotation += 1.0;
    pose.slot_colors[1] = Vec4::splat(0.5);
    pose.slot_attachments[0] = Some(Arc::from("cap"));
    pose.slot_attachments[1] = None;
    pose.draw_order.reverse();
    let placed = affine((0.0, 2.0), (-1.0, 0.0), (10.0, 20.0));
    pose.world[0] = placed;
    pose.reset(&data);
    assert_eq!(pose.bones, setup.bones);
    assert_eq!(pose.slot_colors, setup.slot_colors);
    assert_eq!(pose.slot_attachments, setup.slot_attachments);
    assert_eq!(pose.draw_order, setup.draw_order);
    // The world transforms are the last ones computed, until the next
    assert_affine(pose.world[0], placed);
  }
}
