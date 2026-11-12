//! A skeleton's pose: each bone's local transform, and the world transforms
//! computed from them with Spine's inherit modes, plus each slot's colour and
//! attachment and the draw order.

use std::sync::Arc;

use glam::{Affine2, Mat2, Vec2};

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
    let (sin_x, cos_x) = (self.rotation + self.shear_x).sin_cos();
    let (sin_y, cos_y) = (self.rotation + self.shear_y).sin_cos();
    Mat2::from_cols(
      Vec2::new(cos_x, sin_x) * self.scale_x,
      Vec2::new(-sin_y, cos_y) * self.scale_y,
    )
  }
}

/// One skeleton's current pose, indexed like the data's bones and slots
#[derive(Debug, Clone)]
#[cfg_attr(
  not(test),
  expect(
    dead_code,
    reason = "the animation state and the ECS step read and write the pose"
  )
)]
pub(crate) struct Pose {
  /// Local transforms
  pub(crate) bones: Vec<BonePose>,
  /// World transforms from [`Pose::compute_world`], the root affine applied
  pub(crate) world: Vec<Affine2>,
  pub(crate) slot_colors: Vec<Color>,
  pub(crate) slot_attachments: Vec<Option<Arc<str>>>,
  /// Slots, back to front
  pub(crate) draw_order: Vec<SlotId>,
}

#[cfg_attr(
  not(test),
  expect(
    dead_code,
    reason = "the animation state and the ECS step read and write the pose"
  )
)]
impl Pose {
  /// The setup pose, with world transforms not yet computed (identity)
  pub(crate) fn setup(data: &SkeletonData) -> Self {
    let slots = data.slots();
    Self {
      bones: data.bones().iter().map(BonePose::setup).collect(),
      world: vec![Affine2::IDENTITY; data.bones().len()],
      slot_colors: slots.iter().map(|slot| slot.color).collect(),
      slot_attachments: slots.iter().map(|slot| slot.attachment.clone()).collect(),
      draw_order: (0..slots.len())
        .filter_map(|index| u8::try_from(index).ok().map(SlotId))
        .collect(),
    }
  }

  /// Computes every bone's world transform from the local poses. `root`
  /// places the whole skeleton (the entity's transform). The inherit modes
  /// act in skeleton space, so `root` applies to every bone, whatever its
  /// mode (the module notes have the formulas)
  pub(crate) fn compute_world(&mut self, data: &SkeletonData, root: Affine2) {
    // Bones are parent-first, so one pass sees each parent's skeleton-space
    // transform before its children; a second applies the root.
    for (index, bone) in data.bones().iter().enumerate() {
      let local = &self.bones[index];
      let linear = local.linear();
      let position = Vec2::new(local.x, local.y);
      self.world[index] = match bone.parent {
        None => Affine2::from_mat2_translation(linear, position),
        Some(parent) => {
          let parent = self.world[parent.index()];
          let translation = parent.transform_point2(position);
          let parent_linear = match bone.inherit {
            Inherit::Normal => parent.matrix2,
            Inherit::OnlyTranslation => Mat2::IDENTITY,
            Inherit::NoRotationOrReflection => without_rotation_or_reflection(parent.matrix2),
            Inherit::NoScale => without_scale(parent.matrix2, false),
            Inherit::NoScaleOrReflection => without_scale(parent.matrix2, true),
          };
          Affine2::from_mat2_translation(parent_linear * linear, translation)
        }
      };
    }
    for world in &mut self.world {
      *world = root * *world;
    }
  }
}

/// Below this, an axis counts as collapsed (a zero scale)
const COLLAPSED: f32 = 1e-6;

/// The parent's linear part with its rotation and reflection taken out:
/// writing it as a rotation (its x axis's angle) times an upper-triangular
/// `[[s, t], [0, u]]`, the triangular part with `u` made positive. It keeps
/// the parent's scale and shear
fn without_rotation_or_reflection(parent: Mat2) -> Mat2 {
  let s = parent.x_axis.length();
  if s < COLLAPSED {
    // No x axis to take a rotation from: keep only the y axis's length
    return Mat2::from_cols(Vec2::ZERO, Vec2::new(0.0, parent.y_axis.length()));
  }
  let t = parent.x_axis.dot(parent.y_axis) / s;
  let u = parent.determinant().abs() / s;
  Mat2::from_cols(Vec2::new(s, 0.0), Vec2::new(t, u))
}

/// The parent's linear part with each axis made unit length, which keeps its
/// rotation, shear (the angle between the axes) and reflection. Without
/// reflection, a mirrored parent's y axis is turned back, so the bone
/// follows the parent's x axis unmirrored
fn without_scale(parent: Mat2, remove_reflection: bool) -> Mat2 {
  let x_axis = parent.x_axis.normalize_or_zero();
  let mut y_axis = parent.y_axis.normalize_or_zero();
  if remove_reflection && parent.determinant() < 0.0 {
    y_axis = -y_axis;
  }
  Mat2::from_cols(x_axis, y_axis)
}

#[cfg(test)]
mod tests {
  use std::f32::consts::FRAC_PI_2;

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
    // Parent with rotation and reflection removed: s = |x axis| = 2,
    // t = x.y / s = 0, u = |det| / s = 4 / 2 = 2, so [[2, 0], [0, 2]].
    // axes = 2 * child's own: x 2 * (cos 30, sin 30), y 2 * (-sin 30, cos 30)
    // origin = P * (4, 0) = 4 * (0, -2) = (0, -8)
    // The child's world angle stays 30 degrees
    assert_affine(
      world[1],
      affine((2.0 * COS_30, 1.0), (-1.0, 2.0 * COS_30), (0.0, -8.0)),
    );
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
    // Parent axes normalised: x (0, 1), y (-1, 0); the child is unturned, so
    // its axes are those, unit length.
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
    let world = world(
      &[("parent", None, parent), ("child", Some("parent"), child)],
      Affine2::IDENTITY,
    );
    // Parent axes normalised: x (-1, 0), y (0, 1); the determinant is
    // negative, so the y axis flips to (0, -1): N = [[-1, 0], [0, -1]], a
    // half turn following the parent's x axis.
    // Child local: x (0, 1), y (-1, 0). x axis = N * (0, 1) = (0, -1),
    // y axis = N * (-1, 0) = (1, 0); determinant 0 * 0 - 1 * (-1) = 1, not
    // mirrored.
    // origin = P * (3, 0) = (-6, 0)
    assert_affine(world[1], affine((0.0, -1.0), (1.0, 0.0), (-6.0, 0.0)));
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
    // Without rotation or reflection the parent keeps its shear:
    // s = |x axis| = 1
    // t = x.y / s = -cos 30 sin 45 + sin 30 cos 45 = sin 45 (0.5 - cos 30)
    //   = -0.258819 (-sin 15)
    // u = det / s = cos 30 cos 45 + sin 30 sin 45 = cos 15 = 0.965926
    // so x axis (1, 0) and y axis (t, u): still 105 degrees apart, as the
    // parent's axes are (45 + 90 - 30)
    assert_affine(
      world[2],
      affine((1.0, 0.0), (-0.258_819, 0.965_926), (0.0, 0.0)),
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
    assert_eq!(pose.slot_colors, vec![Color::WHITE, red]);
    assert_eq!(pose.slot_attachments, vec![None, Some(Arc::from("hat"))]);
    assert_eq!(pose.draw_order, vec![SlotId(0), SlotId(1)]);
    assert_eq!(pose.bones, vec![BonePose::setup(&data.bones()[0])]);
  }
}
