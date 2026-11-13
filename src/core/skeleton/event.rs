//! What a skeleton tells the game: the event keys its animations cross,
//! and what happens to its track entries.

use std::sync::Arc;

use super::data::{EventAudio, SkeletonData};
use super::state::{FiredEvent, FiredTrackEvent, TrackEntryId, TrackEventKind};
use crate::core::ecs::{Entity, Message};

/// An event key that a skeleton's animation crossed in a fixed step. A
/// step's events are written in the order they happened, whatever tracks
/// they are on; a looping animation's keys come once a loop. The values
/// are the key's own where it gives them, else the event's.
///
/// A skeleton keeps animating through a scene transition, during which a
/// scene's `update` does not run: an event sent then is not seen by
/// `update` (messages last two fixed steps). A system the scene added
/// (`SceneContext::add_systems`) keeps running, and sees it
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
/// [`SkeletonAnimation::EMPTY`](super::SkeletonAnimation::EMPTY).
///
/// As with a [`SkeletonEvent`], one sent during a scene transition is not
/// seen by a scene's `update`, which does not run then: a game that must
/// not miss a `Complete` reads these in a scene system, queues what
/// follows with `add_animation`, or asks the entry (`is_complete`)
#[derive(Message, Debug, Clone, PartialEq)]
pub struct SkeletonTrackEvent {
  /// The skeleton's entity
  pub entity: Entity,
  pub track: usize,
  /// The entry's handle, as `set_animation` and `add_animation` gave it.
  /// It is unique within `entity`'s own animation state only, so compare
  /// `entity` as well: another skeleton's entry can have the same handle
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
