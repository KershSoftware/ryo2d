//! Constraints: rules that move bones after animation. An IK constraint
//! turns bones so the last one's tip reaches a target bone; a transform
//! constraint makes bones copy a target bone's rotation, place, scale or
//! shear.

use std::sync::Arc;

use super::data::BoneId;

/// A constraint's place in [`SkeletonData::constraints`](super::SkeletonData::constraints)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ConstraintId(pub u8);

impl ConstraintId {
  pub fn index(self) -> usize {
    usize::from(self.0)
  }
}

/// The most constraints a skeleton has: they are indexed by a `u8`
pub const MAX_CONSTRAINTS: usize = 255;

/// The most bones an IK constraint turns
pub const MAX_IK_BONES: usize = 16;

/// A rule that moves bones after animation. A skeleton's constraints apply
/// in the order its data lists them
#[derive(Debug, Clone, PartialEq)]
pub enum Constraint {
  Ik(IkConstraint),
  Transform(TransformConstraint),
}

impl Constraint {
  pub fn name(&self) -> &Arc<str> {
    match self {
      Self::Ik(ik) => &ik.name,
      Self::Transform(transform) => &transform.name,
    }
  }

  /// The bones it moves, first to last
  pub fn bones(&self) -> &[BoneId] {
    match self {
      Self::Ik(ik) => &ik.bones,
      Self::Transform(transform) => &transform.bones,
    }
  }

  /// The bone it follows
  pub fn target(&self) -> BoneId {
    match self {
      Self::Ik(ik) => ik.target,
      Self::Transform(transform) => transform.target,
    }
  }
}

/// Turns one bone, two bones or a chain so the last one's tip reaches or
/// points at the target. Each bone after the first is an immediate child
/// of the one before
#[derive(Debug, Clone, PartialEq)]
pub struct IkConstraint {
  pub name: Arc<str>,
  /// One to [`MAX_IK_BONES`] bones, first to last
  pub bones: Vec<BoneId>,
  pub target: BoneId,
  pub setup: IkSetup,
}

/// An IK constraint's settings
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IkSetup {
  /// 0 (the animated pose) to 1 (the solved pose)
  pub mix: f32,
  /// Two bones: a distance from full reach within which the tip slows as
  /// the bones straighten
  pub softness: f32,
  /// Two bones: which way the joint bends, judged in the space of the
  /// first bone's parent
  pub bend_positive: bool,
  /// One bone: scale it down when the target is nearer than its length
  pub compress: bool,
  /// One or two bones: scale the first along its length when the target
  /// is beyond reach
  pub stretch: bool,
  /// Scale across the length by the same factor whenever stretch or
  /// compress is on, by the setup, an animation's key or the game's
  /// override. A setup may set it with neither: it is not keyed or
  /// overridden, so it is the setup's alone
  pub uniform: bool,
}

impl Default for IkSetup {
  fn default() -> Self {
    Self {
      mix: 1.0,
      softness: 0.0,
      bend_positive: true,
      compress: false,
      stretch: false,
      uniform: false,
    }
  }
}

/// Makes bones copy a target bone's rotation, place, scale and shear, each
/// by its own mix
#[derive(Debug, Clone, PartialEq)]
pub struct TransformConstraint {
  pub name: Arc<str>,
  /// At least one bone
  pub bones: Vec<BoneId>,
  pub target: BoneId,
  pub setup: TransformSetup,
}

/// A transform constraint's settings
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TransformSetup {
  pub mix: TransformMix,
  /// Added to the target's values
  pub offset: TransformOffset,
  /// Work on the bones' own values, not their transforms in the
  /// skeleton's space
  pub local: bool,
  /// Add the target's values to the bone's, not move toward them
  pub relative: bool,
}

/// How far each property moves toward the target's: 0 leaves it alone and
/// 1 applies it fully. A mix may be below 0 or above 1
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TransformMix {
  pub rotate: f32,
  pub x: f32,
  pub y: f32,
  pub scale_x: f32,
  pub scale_y: f32,
  pub shear_y: f32,
}

impl TransformMix {
  /// Every property fully
  pub const ALL: Self = Self {
    rotate: 1.0,
    x: 1.0,
    y: 1.0,
    scale_x: 1.0,
    scale_y: 1.0,
    shear_y: 1.0,
  };
}

/// Added to the target's values before they are copied
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TransformOffset {
  /// Radians
  pub rotation: f32,
  pub x: f32,
  pub y: f32,
  pub scale_x: f32,
  pub scale_y: f32,
  /// Radians
  pub shear_y: f32,
}
