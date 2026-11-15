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
  /// Which way it bends: [`IkBend::Positive`] or [`IkBend::Negative`] for
  /// two bones, any of the three for a chain, and only the default for one
  /// bone. Left out it is [`IkBend::Positive`] for one bone and two and
  /// [`IkBend::Pose`] for a chain: the built data holds it so
  pub bend: Option<IkBend>,
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

impl IkSetup {
  /// Its bend for a constraint of `bones` bones: as set, or the default
  /// for that many
  pub(crate) fn bend_for(&self, bones: usize) -> IkBend {
    self.bend.unwrap_or(IkBend::default_for(bones))
  }
}

/// Which way an IK constraint bends, judged in the space of its first
/// bone's parent; positive is clockwise on screen
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IkBend {
  /// Two bones: the joint bends the positive way. A chain: it keeps to the
  /// positive side, an even arc from straight whatever its pose
  Positive,
  /// Two bones: the joint bends the negative way. A chain: it keeps to the
  /// negative side
  Negative,
  /// A chain: it closes the way its animated pose bends, and opens that
  /// pose toward straight for a farther target
  Pose,
}

impl IkBend {
  /// What a constraint of `bones` bones bends by when its setup leaves it
  /// out: the pose's way for a chain, the positive way otherwise
  pub(crate) fn default_for(bones: usize) -> Self {
    if bones > 2 {
      Self::Pose
    } else {
      Self::Positive
    }
  }

  /// Whether a constraint of `bones` bones can have it: one bone only the
  /// positive way, two either way but not the pose's, a chain any
  pub(crate) fn fits(self, bones: usize) -> bool {
    match bones {
      0 | 1 => self == Self::Positive,
      2 => self != Self::Pose,
      _ => true,
    }
  }

  /// The side a chain keeps, 1 the positive and -1 the negative, or none
  /// for the pose's way
  pub(crate) fn side(self) -> Option<f32> {
    match self {
      Self::Positive => Some(1.0),
      Self::Negative => Some(-1.0),
      Self::Pose => None,
    }
  }
}

impl Default for IkSetup {
  fn default() -> Self {
    Self {
      mix: 1.0,
      softness: 0.0,
      bend: None,
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

/// A transform constraint's settings. The default is every mix and offset
/// 0, in the world mode: at every mix 0 the constraint does nothing, so
/// set the mixes wanted, or [`TransformMix::ALL`] to copy every property
/// fully
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TransformSetup {
  /// How far each property moves toward the target's (all 0 by default)
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
