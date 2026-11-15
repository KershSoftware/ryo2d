//! Animations: timelines of keyed values for bones, slots and the meshes
//! slots show, constraints, the draw order and events.

use std::sync::Arc;

use super::constraint::{ConstraintId, TransformMix};
use super::curve::Curve;
use super::data::{BoneId, SlotId};
use super::ik::IkValues;
use crate::core::ecs::Vec2;
use crate::core::ui::Color;

/// The name of the empty animation an animation state plays to fade a track
/// out: no animation in a skeleton's data may take it
pub(super) const EMPTY_NAME: &str = "<empty>";

/// A key of a value timeline: its time in seconds, its value, and the curve
/// that eases from it to the next key
pub type Frame<T> = (f32, T, Curve);

/// An event key's values. Each one left `None` takes the event's default
/// ([`EventData`](super::EventData), and its audio's)
#[derive(Debug, Clone, PartialEq, Default)]
pub struct EventValues {
  pub int: Option<i32>,
  pub float: Option<f32>,
  pub string: Option<Arc<str>>,
  pub volume: Option<f32>,
  pub balance: Option<f32>,
}

/// One keyed property of an animation. Keys are in time order
#[derive(Debug, Clone, PartialEq)]
pub enum Timeline {
  /// A bone's rotation, in radians, relative to its setup rotation
  Rotate {
    bone: BoneId,
    frames: Vec<Frame<f32>>,
  },
  /// A bone's translation, relative to its setup position
  Translate {
    bone: BoneId,
    frames: Vec<Frame<Vec2>>,
  },
  /// A bone's scale, as a factor of its setup scale
  Scale {
    bone: BoneId,
    frames: Vec<Frame<Vec2>>,
  },
  /// A bone's shear, in radians, relative to its setup shear
  Shear {
    bone: BoneId,
    frames: Vec<Frame<Vec2>>,
  },
  /// A bone's x translation alone, relative to its setup x
  TranslateX {
    bone: BoneId,
    frames: Vec<Frame<f32>>,
  },
  /// A bone's y translation alone, relative to its setup y
  TranslateY {
    bone: BoneId,
    frames: Vec<Frame<f32>>,
  },
  /// A bone's x scale alone, as a factor of its setup x scale
  ScaleX {
    bone: BoneId,
    frames: Vec<Frame<f32>>,
  },
  /// A bone's y scale alone, as a factor of its setup y scale
  ScaleY {
    bone: BoneId,
    frames: Vec<Frame<f32>>,
  },
  /// A bone's x shear alone, in radians, relative to its setup x shear
  ShearX {
    bone: BoneId,
    frames: Vec<Frame<f32>>,
  },
  /// A bone's y shear alone, in radians, relative to its setup y shear
  ShearY {
    bone: BoneId,
    frames: Vec<Frame<f32>>,
  },
  /// A slot's attachment by name (`None` hides it); keys are stepped
  Attachment {
    slot: SlotId,
    frames: Vec<(f32, Option<Arc<str>>)>,
  },
  /// A slot's colour
  Color {
    slot: SlotId,
    frames: Vec<Frame<Color>>,
  },
  /// The shape of the mesh a slot shows under the name `attachment`: each
  /// key holds an offset from the setup shape, an x and a y, for each
  /// position the mesh holds
  /// ([`MeshAttachment::position_count`](super::MeshAttachment::position_count)). An
  /// unweighted mesh has a position a vertex, in the space of its slot's
  /// bone; a weighted mesh has one an influence, in that influence's bone's
  /// space, so a keyed shape turns with the bones it rides. An empty key is
  /// no offset: the setup shape. Every other key of a timeline is one
  /// length. `attachment` must be a name some skin has in the slot, or the
  /// data does not build.
  ///
  /// It moves the mesh only while the slot shows that name and the
  /// attachment found there is a mesh holding as many positions as the
  /// keys. While the slot shows another attachment, or none, it does
  /// nothing. Where the slot shows that name and the attachment is not
  /// such a mesh, it is skipped, and the log is told once
  Deform {
    slot: SlotId,
    attachment: Arc<str>,
    frames: Vec<Frame<Vec<f32>>>,
  },
  /// An IK constraint's values. `mix` and `softness` ease to the next key
  /// by the key's curve; `bend_positive`, `compress` and `stretch` are
  /// stepped, changing at their key. A key sets no switch its constraint
  /// could not have, and its `mix` is 0 to 1 and its `softness` not
  /// negative, or the data does not build
  Ik {
    constraint: ConstraintId,
    frames: Vec<Frame<IkValues>>,
  },
  /// A transform constraint's six mixes, each easing to the next key by the
  /// key's curve. A mix may be below 0 or above 1
  TransformMix {
    constraint: ConstraintId,
    frames: Vec<Frame<TransformMix>>,
  },
  /// The draw order: every slot once, back to front, or `None` for the
  /// setup order; keys are stepped
  DrawOrder {
    frames: Vec<(f32, Option<Vec<SlotId>>)>,
  },
  /// Events fired, by index into
  /// [`SkeletonData::events`](super::SkeletonData::events), with the
  /// values the key overrides. Several keys may share a time
  Event {
    frames: Vec<(f32, usize, EventValues)>,
  },
}

/// What a timeline keys, for finding two timelines that key the same thing
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Target<'a> {
  Bone(BoneId),
  Slot(SlotId),
  /// The attachment a slot shows under a name: what a deform timeline
  /// keys, so two of one slot for different names key different things
  Attachment(SlotId, &'a str),
  /// A constraint: its timeline keys all its values as one property
  Constraint(ConstraintId),
  Skeleton,
}

/// How many floats each key of a deform timeline holds that is not empty
/// (every such key is one length in built data); 0 when all are empty
pub(super) fn deform_width(frames: &[Frame<Vec<f32>>]) -> usize {
  let lengths = frames.iter().map(|frame| frame.1.len());
  lengths.into_iter().find(|&length| length > 0).unwrap_or(0)
}

impl Timeline {
  /// The kind's name, for messages
  pub fn kind(&self) -> &'static str {
    match self {
      Self::Rotate { .. } => "rotate",
      Self::Translate { .. } => "translate",
      Self::Scale { .. } => "scale",
      Self::Shear { .. } => "shear",
      Self::TranslateX { .. } => "translate x",
      Self::TranslateY { .. } => "translate y",
      Self::ScaleX { .. } => "scale x",
      Self::ScaleY { .. } => "scale y",
      Self::ShearX { .. } => "shear x",
      Self::ShearY { .. } => "shear y",
      Self::Attachment { .. } => "attachment",
      Self::Color { .. } => "color",
      Self::Deform { .. } => "deform",
      Self::Ik { .. } => "ik",
      Self::TransformMix { .. } => "transform mix",
      Self::DrawOrder { .. } => "draw order",
      Self::Event { .. } => "event",
    }
  }

  /// The bone, slot, slot's attachment, constraint or (for draw order and
  /// events) whole skeleton it keys
  pub(super) fn target(&self) -> Target<'_> {
    match self {
      Self::Rotate { bone, .. }
      | Self::Translate { bone, .. }
      | Self::Scale { bone, .. }
      | Self::Shear { bone, .. }
      | Self::TranslateX { bone, .. }
      | Self::TranslateY { bone, .. }
      | Self::ScaleX { bone, .. }
      | Self::ScaleY { bone, .. }
      | Self::ShearX { bone, .. }
      | Self::ShearY { bone, .. } => Target::Bone(*bone),
      Self::Attachment { slot, .. } | Self::Color { slot, .. } => Target::Slot(*slot),
      Self::Deform {
        slot, attachment, ..
      } => Target::Attachment(*slot, attachment),
      Self::Ik { constraint, .. } | Self::TransformMix { constraint, .. } => {
        Target::Constraint(*constraint)
      }
      Self::DrawOrder { .. } | Self::Event { .. } => Target::Skeleton,
    }
  }

  /// The properties of its target it keys: a combined timeline keys both
  /// axes, so it clashes with a per-axis timeline of either. A constraint's
  /// timeline keys one property, the constraint
  pub(super) fn properties(&self) -> &'static [&'static str] {
    match self {
      Self::Translate { .. } => &["translate x", "translate y"],
      Self::Scale { .. } => &["scale x", "scale y"],
      Self::Shear { .. } => &["shear x", "shear y"],
      Self::TranslateX { .. } => &["translate x"],
      Self::TranslateY { .. } => &["translate y"],
      Self::ScaleX { .. } => &["scale x"],
      Self::ScaleY { .. } => &["scale y"],
      Self::ShearX { .. } => &["shear x"],
      Self::ShearY { .. } => &["shear y"],
      Self::Rotate { .. } => &["rotate"],
      Self::Attachment { .. } => &["attachment"],
      Self::Color { .. } => &["color"],
      Self::Deform { .. } => &["deform"],
      Self::Ik { .. } | Self::TransformMix { .. } => &["values"],
      Self::DrawOrder { .. } => &["draw order"],
      Self::Event { .. } => &["event"],
    }
  }

  /// The first key's time (every timeline of built data has a key)
  pub(super) fn first_time(&self) -> Option<f32> {
    match self {
      Self::Rotate { frames, .. }
      | Self::TranslateX { frames, .. }
      | Self::TranslateY { frames, .. }
      | Self::ScaleX { frames, .. }
      | Self::ScaleY { frames, .. }
      | Self::ShearX { frames, .. }
      | Self::ShearY { frames, .. } => frames.first().map(|frame| frame.0),
      Self::Translate { frames, .. } | Self::Scale { frames, .. } | Self::Shear { frames, .. } => {
        frames.first().map(|frame| frame.0)
      }
      Self::Color { frames, .. } => frames.first().map(|frame| frame.0),
      Self::Deform { frames, .. } => frames.first().map(|frame| frame.0),
      Self::Ik { frames, .. } => frames.first().map(|frame| frame.0),
      Self::TransformMix { frames, .. } => frames.first().map(|frame| frame.0),
      Self::Attachment { frames, .. } => frames.first().map(|frame| frame.0),
      Self::DrawOrder { frames } => frames.first().map(|frame| frame.0),
      Self::Event { frames } => frames.first().map(|frame| frame.0),
    }
  }

  /// The keys' times, in order
  pub(super) fn times(&self) -> Vec<f32> {
    fn of<T>(frames: &[(f32, T, Curve)]) -> Vec<f32> {
      frames.iter().map(|frame| frame.0).collect()
    }
    match self {
      Self::Rotate { frames, .. }
      | Self::TranslateX { frames, .. }
      | Self::TranslateY { frames, .. }
      | Self::ScaleX { frames, .. }
      | Self::ScaleY { frames, .. }
      | Self::ShearX { frames, .. }
      | Self::ShearY { frames, .. } => of(frames),
      Self::Translate { frames, .. } | Self::Scale { frames, .. } | Self::Shear { frames, .. } => {
        of(frames)
      }
      Self::Color { frames, .. } => of(frames),
      Self::Deform { frames, .. } => of(frames),
      Self::Ik { frames, .. } => of(frames),
      Self::TransformMix { frames, .. } => of(frames),
      Self::Attachment { frames, .. } => frames.iter().map(|frame| frame.0).collect(),
      Self::DrawOrder { frames } => frames.iter().map(|frame| frame.0).collect(),
      Self::Event { frames } => frames.iter().map(|frame| frame.0).collect(),
    }
  }
}

/// A named animation: its length in seconds and its timelines
#[derive(Debug, Clone, PartialEq)]
pub struct Animation {
  pub name: Arc<str>,
  pub duration: f32,
  pub timelines: Vec<Timeline>,
}
