//! Constraints: rules that move bones after animation. An IK constraint
//! turns bones so the last one's tip reaches a target bone; a transform
//! constraint makes bones copy a target bone's rotation, place, scale or
//! shear; a spring constraint gives one bone secondary motion, lagging
//! behind where animation puts it and swinging back.

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
  Spring(SpringConstraint),
}

impl Constraint {
  pub fn name(&self) -> &Arc<str> {
    match self {
      Self::Ik(ik) => &ik.name,
      Self::Transform(transform) => &transform.name,
      Self::Spring(spring) => &spring.name,
    }
  }

  /// The bones it moves, first to last (a spring's one bone)
  pub fn bones(&self) -> &[BoneId] {
    match self {
      Self::Ik(ik) => &ik.bones,
      Self::Transform(transform) => &transform.bones,
      Self::Spring(spring) => std::slice::from_ref(&spring.bone),
    }
  }

  /// The bone it follows; a spring follows none
  pub fn target(&self) -> Option<BoneId> {
    match self {
      Self::Ik(ik) => Some(ik.target),
      Self::Transform(transform) => Some(transform.target),
      Self::Spring(_) => None,
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
  /// One or two bones: the most stretch takes the scale to, as a factor of
  /// the reach (at least 1). Past it the limb is stretched by that much and
  /// points at the target. Left out there is no limit. Not keyed; the game's
  /// override can set it
  pub max_stretch: Option<f32>,
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
      max_stretch: None,
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

/// Gives one bone secondary motion: when its animated place moves, the bone
/// lags behind and swings back. It has no target bone
#[derive(Debug, Clone, PartialEq)]
pub struct SpringConstraint {
  pub name: Arc<str>,
  pub bone: BoneId,
  pub setup: SpringSetup,
}

/// A spring constraint's settings. The amounts (`x`, `y`, `rotate`,
/// `scale_x`) are all 0 by default, so set the ones wanted: a spring that
/// moves nothing does not build
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpringSetup {
  /// How far the bone's place follows its origin's lag along the world's
  /// x, 0 to 1
  pub x: f32,
  /// How far the bone's place follows its origin's lag along the world's
  /// y, 0 to 1
  pub y: f32,
  /// How far the bone turns toward its lagging tip, 0 to 1. Needs a length
  pub rotate: f32,
  /// How far the bone's scale along its length follows its lagging tip, 0
  /// to 1. Needs a length
  pub scale_x: f32,
  /// How much of the animated movement each step leaves behind, 0 to 1
  /// (default 0.5)
  pub inertia: f32,
  /// The pull back, per second squared per unit of lag, above 0 (default
  /// 100)
  pub strength: f32,
  /// The rate a second the speed dies away, 0 or more (default 5)
  pub damping: f32,
  /// An acceleration along the world's x, in units a second squared
  /// (default 0)
  pub wind: f32,
  /// An acceleration along the world's y, down positive, in units a second
  /// squared (default 0)
  pub gravity: f32,
  /// The most animated movement a second taken in, so a jump does not
  /// fling the bone, 0 or more (default 5000)
  pub limit: f32,
  /// 0 (the animated pose) to 1 (the sprung pose) (default 1)
  pub mix: f32,
}

impl Default for SpringSetup {
  fn default() -> Self {
    Self {
      x: 0.0,
      y: 0.0,
      rotate: 0.0,
      scale_x: 0.0,
      inertia: 0.5,
      strength: 100.0,
      damping: 5.0,
      wind: 0.0,
      gravity: 0.0,
      limit: 5000.0,
      mix: 1.0,
    }
  }
}

/// A spring constraint's values that animations key and the game
/// overrides; the rest are its setup's
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpringValues {
  /// 0 to 1
  pub mix: f32,
  pub wind: f32,
  pub gravity: f32,
}

impl SpringValues {
  /// The values a spring starts each step with: its setup's
  pub fn of(setup: &SpringSetup) -> Self {
    Self {
      mix: setup.mix,
      wind: setup.wind,
      gravity: setup.gravity,
    }
  }
}

impl Default for SpringValues {
  fn default() -> Self {
    Self::of(&SpringSetup::default())
  }
}
