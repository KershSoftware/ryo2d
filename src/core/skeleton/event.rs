//! What a skeleton tells the game: the event keys its animations cross,
//! and what happens to its track entries.

use std::sync::Arc;

use super::data::{EventAudio, SkeletonData};
use super::state::{FiredEvent, FiredTrackEvent, TrackEntryId, TrackEventKind};
use crate::core::ecs::{Entity, Message};

/// An event key that a skeleton's animation crossed in a fixed step. A
/// step's events are written in the order they happened, whatever tracks
/// they are on; a looping animation's keys come once a loop. The values
/// are the key's own where it gives them, else the event's
#[derive(Message, Debug, Clone, PartialEq)]
pub struct SkeletonEvent {
  /// The skeleton's entity
  pub entity: Entity,
  /// The track the animation plays on
  pub track: usize,
  /// The animation's name
  pub animation: Arc<str>,
  /// The event's name
  pub name: Arc<str>,
  pub int: i32,
  pub float: f32,
  pub string: Arc<str>,
  /// The sound the event names, with the key's volume and balance where it
  /// gives them. The engine does not play it: the game does, if it wants it
  pub audio: Option<EventAudio>,
  /// The key's time in its animation, in seconds
  pub time: f32,
}

impl SkeletonEvent {
  /// The message for an event `entity`'s animation state fired, with the
  /// values the key left out taken from `data`'s event
  pub(crate) fn new(entity: Entity, data: &SkeletonData, fired: FiredEvent) -> Option<Self> {
    let event = data.events().get(fired.event)?;
    let values = fired.values;
    let audio = event.audio.as_ref().map(|audio| EventAudio {
      path: audio.path.clone(),
      volume: values.volume.unwrap_or(audio.volume),
      balance: values.balance.unwrap_or(audio.balance),
    });
    Some(Self {
      entity,
      track: fired.track,
      animation: fired.animation,
      name: event.name.clone(),
      int: values.int.unwrap_or(event.int),
      float: values.float.unwrap_or(event.float),
      string: values.string.unwrap_or_else(|| event.string.clone()),
      audio,
      time: fired.time,
    })
  }
}

/// Something that happened to one of a skeleton's track entries in a fixed
/// step (what a call such as `set_animation` causes is told in the
/// skeleton's next step). The empty animation's entries carry the name
/// [`SkeletonAnimation::EMPTY`](super::SkeletonAnimation::EMPTY)
#[derive(Message, Debug, Clone, PartialEq)]
pub struct SkeletonTrackEvent {
  /// The skeleton's entity
  pub entity: Entity,
  pub track: usize,
  /// The entry's handle, as `set_animation` and `add_animation` gave it
  pub entry: TrackEntryId,
  /// The entry's animation's name
  pub animation: Arc<str>,
  pub kind: TrackEventKind,
}

impl SkeletonTrackEvent {
  pub(crate) fn new(entity: Entity, fired: FiredTrackEvent) -> Self {
    Self {
      entity,
      track: fired.track,
      entry: fired.entry,
      animation: fired.animation,
      kind: fired.kind,
    }
  }
}
