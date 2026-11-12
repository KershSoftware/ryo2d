//! Animations: timelines of keyed values for bones and slots, the draw order
//! and events.

use std::sync::Arc;

use super::curve::Curve;
use super::data::{BoneId, SlotId};
use crate::core::ecs::Vec2;
use crate::core::ui::Color;

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

impl Timeline {
  /// The kind's name, for messages
  pub fn kind(&self) -> &'static str {
    match self {
      Self::Rotate { .. } => "rotate",
      Self::Translate { .. } => "translate",
      Self::Scale { .. } => "scale",
      Self::Shear { .. } => "shear",
      Self::Attachment { .. } => "attachment",
      Self::Color { .. } => "color",
      Self::DrawOrder { .. } => "draw order",
      Self::Event { .. } => "event",
    }
  }

  /// The keys' times, in order
  pub(super) fn times(&self) -> Vec<f32> {
    fn of<T>(frames: &[(f32, T, Curve)]) -> Vec<f32> {
      frames.iter().map(|frame| frame.0).collect()
    }
    match self {
      Self::Rotate { frames, .. } => of(frames),
      Self::Translate { frames, .. } | Self::Scale { frames, .. } | Self::Shear { frames, .. } => {
        of(frames)
      }
      Self::Color { frames, .. } => of(frames),
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
