//! The animation state: tracks of entries, each track with a current entry
//! (mixing from the entries it replaced) and a queue, applied from the
//! lowest track up. The module notes give every rule.

use std::collections::VecDeque;
use std::sync::Arc;

use super::animation::{Animation, EventValues, Timeline};
use super::apply::{
  CrossedKey, Layer, MAX_LOOPS, MixBlend, MixDirection, apply_layer, events_crossed,
};
use super::data::{AnimationCatalogue, SkeletonData};
use super::pose::Pose;
use crate::core::ecs::Component;

/// The name of the animation an entry from [`SkeletonAnimation::set_empty`]
/// plays: it keys nothing
const EMPTY: &str = "<empty>";

/// A track entry's handle, from [`SkeletonAnimation::set_animation`] and
/// [`SkeletonAnimation::add_animation`]. It finds the entry until the entry
/// is gone (its [`TrackEventKind::Dispose`])
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TrackEntryId(u32);

/// What happened to a track entry
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TrackEventKind {
  /// It became its track's current entry
  Start,
  /// Another entry replaced it as current; it may still be mixing out
  Interrupt,
  /// It will never be applied again: cleared, or replaced and its mix-out
  /// over
  End,
  /// It passed its duration while current: each loop when looping, once
  /// otherwise
  Complete,
  /// It is gone, after its `End` (or, queued and never started, without
  /// one); its handle no longer finds it
  Dispose,
}

/// One animation on a track. Change its public fields through
/// [`SkeletonAnimation::entry_mut`]
#[derive(Debug, Clone)]
pub struct TrackEntry {
  /// Multiplies the time the entry advances by (1). Use `reverse` to play
  /// backward: a negative or non-finite speed holds the entry still
  pub speed: f32,
  /// How much the entry applies over what is under it, 0 to 1 (1). On track
  /// 0 that is the setup pose; on a higher track, the tracks below
  pub alpha: f32,
  /// Seconds the entry takes to mix in from the entry it replaces: the
  /// data's mix for the pair, or its default mix, until changed
  pub mix_duration: f32,
  /// Replace (the default) moves the pose toward the animation; add adds
  /// the animation's change from the setup pose
  pub mix_blend: MixBlend,
  /// While the entry is mixing out, its events fire until the mix of the
  /// entry replacing it reaches this fraction (0: none fire once replaced)
  pub event_threshold: f32,
  /// Plays the animation backward, from its end
  pub reverse: bool,
  /// While this entry mixes in, the entry it replaced holds its time
  /// instead of playing on
  pub hold_previous: bool,
  /// Whether the animation repeats; otherwise it holds its last frame
  pub looping: bool,
  /// For a queued entry: seconds of the previous entry's time after which
  /// it starts (on an empty track, seconds from the call)
  pub delay: f32,
  id: TrackEntryId,
  name: Arc<str>,
  /// The animation's index in the data; `None` for the empty animation (or
  /// before the data lands)
  animation: Option<usize>,
  duration: f32,
  empty: bool,
  /// Seconds since it started, times its speed: the animation's unwrapped
  /// time
  track_time: f32,
  /// Whether it has advanced once: its first step's events span from
  /// before 0
  advanced: bool,
  /// Whether it has been applied once
  applied: bool,
  /// Seconds since it started mixing in (not scaled by its speed)
  mix_time: f32,
  /// The track time at which another entry replaced it
  out_at: Option<f32>,
  /// Seconds a queued head on an empty track has waited
  waited: f32,
  /// The mix duration is still to come from the data (made before the data
  /// landed, or not yet placed on a track)
  mix_from_data: bool,
  /// Per timeline: the timeline of the entry replacing it that keys all the
  /// same properties (value timelines only)
  cover: Vec<Option<usize>>,
  covered: bool,
  /// Per timeline: the factor on its alpha while it mixes out
  fades: Vec<f32>,
  /// Per timeline: the turn each rotate timeline blended by last step,
  /// while in a mix
  turns: Vec<f32>,
  // This step: the track time at the step's start (or at its start within
  // the step), that start as a fraction of the step, the track time one
  // whole step advances it by, and where its event span starts
  step_origin: f32,
  step_start: f32,
  step_rate: f32,
  span_start: f32,
}

impl TrackEntry {
  /// The animation's name (`<empty>` for an entry from
  /// [`SkeletonAnimation::set_empty`])
  pub fn animation(&self) -> &str {
    &self.name
  }

  /// Seconds since the entry started, times its speed: how far into its
  /// animation it is, counting every loop
  pub fn track_time(&self) -> f32 {
    self.track_time
  }

  /// Whether the entry has played its animation through at least once. An
  /// entry that has not started (queued, or kept until the data lands) has
  /// not
  pub fn is_complete(&self) -> bool {
    self.advanced && self.track_time >= self.duration
  }

  fn new(id: TrackEntryId, name: Arc<str>, looping: bool) -> Self {
    Self {
      speed: 1.0,
      alpha: 1.0,
      mix_duration: 0.0,
      mix_blend: MixBlend::Replace,
      event_threshold: 0.0,
      reverse: false,
      hold_previous: false,
      looping,
      delay: 0.0,
      id,
      name,
      animation: None,
      duration: 0.0,
      empty: false,
      track_time: 0.0,
      advanced: false,
      applied: false,
      mix_time: 0.0,
      out_at: None,
      waited: 0.0,
      mix_from_data: true,
      cover: Vec::new(),
      covered: false,
      fades: Vec::new(),
      turns: Vec::new(),
      step_origin: 0.0,
      step_start: 0.0,
      step_rate: 0.0,
      span_start: -1.0,
    }
  }

  /// The animation's time this step: wrapped when looping, held at the end
  /// otherwise, and mirrored when reversed
  fn animation_time(&self) -> f32 {
    let duration = self.duration;
    let time = if duration <= 0.0 {
      0.0
    } else if self.looping {
      self.track_time.rem_euclid(duration)
    } else {
      self.track_time.min(duration)
    };
    if self.reverse { duration - time } else { time }
  }

  /// The track time at which its animation next ends: its duration, or for
  /// a looping entry the end of the loop it is in
  fn next_end(&self) -> f32 {
    let duration = self.duration;
    if self.looping && duration > 0.0 {
      finite_time(((self.track_time / duration).floor() + 1.0) * duration)
    } else {
      duration
    }
  }

  fn speed(&self) -> f32 {
    non_negative(self.speed)
  }

  fn mix_duration(&self) -> f32 {
    non_negative(self.mix_duration)
  }

  fn alpha(&self) -> f32 {
    unit(self.alpha)
  }

  /// How far it has mixed in, 0 to 1 (1 with no mix)
  fn mix_fraction(&self) -> f32 {
    let duration = self.mix_duration();
    if duration <= 0.0 {
      1.0
    } else {
      (self.mix_time / duration).min(1.0)
    }
  }

  fn mix_done(&self) -> bool {
    self.mix_time >= self.mix_duration()
  }

  /// Forgets what it knew of the data's timelines and of its mix's turns
  fn forget_timelines(&mut self) {
    self.covered = false;
    self.cover.clear();
    self.fades.clear();
    self.turns.clear();
  }
}

/// An event key crossed in a step
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FiredEvent {
  pub(crate) track: usize,
  pub(crate) animation: Arc<str>,
  /// Index into [`SkeletonData::events`]
  pub(crate) event: usize,
  pub(crate) values: EventValues,
  /// The key's time in its animation
  pub(crate) time: f32,
  /// When in the step it was crossed, 0 to 1, for ordering
  moment: f32,
}

/// Something that happened to a track entry in a step
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FiredTrackEvent {
  pub(crate) track: usize,
  pub(crate) entry: TrackEntryId,
  pub(crate) animation: Arc<str>,
  pub(crate) kind: TrackEventKind,
}

/// One track: the current entry last, after the entries it is mixing from
/// (oldest first), and the entries queued after it
#[derive(Debug, Clone, Default)]
struct Track {
  chain: Vec<TrackEntry>,
  queue: VecDeque<TrackEntry>,
}

/// A call made before the data landed, kept to make when it lands
#[derive(Debug, Clone)]
enum Kept {
  Set(usize, TrackEntry),
  Add(usize, TrackEntry),
  Empty(usize, f32),
}

impl Kept {
  fn track(&self) -> usize {
    match self {
      Self::Set(track, _) | Self::Add(track, _) | Self::Empty(track, _) => *track,
    }
  }

  fn entry(&self) -> Option<&TrackEntry> {
    match self {
      Self::Set(_, entry) | Self::Add(_, entry) => Some(entry),
      Self::Empty(..) => None,
    }
  }

  fn entry_mut(&mut self) -> Option<&mut TrackEntry> {
    match self {
      Self::Set(_, entry) | Self::Add(_, entry) => Some(entry),
      Self::Empty(..) => None,
    }
  }
}

/// A skeleton's animation state: tracks numbered from 0, each playing a
/// current entry, mixing from the entries it replaced, with a queue after
/// it. Higher tracks apply over lower ones. Without one, a skeleton holds
/// its setup pose
#[derive(Component, Debug, Clone)]
pub struct SkeletonAnimation {
  /// Multiplies the time every track advances by (1). A negative or
  /// non-finite scale holds the skeleton still
  pub time_scale: f32,
  tracks: Vec<Track>,
  /// The data's animations, once a step has seen the data
  catalogue: Option<Arc<AnimationCatalogue>>,
  /// Calls made before the data landed, in order
  kept: Vec<Kept>,
  /// Track events from calls, sent with the next step's
  told: Vec<FiredTrackEvent>,
  /// Names already warned about
  warned: Vec<Arc<str>>,
  warned_track: bool,
  next_id: u32,
  empty_name: Arc<str>,
  crossed: Vec<CrossedKey>,
}

impl Default for SkeletonAnimation {
  fn default() -> Self {
    Self::new()
  }
}

impl SkeletonAnimation {
  /// The most tracks a skeleton has: tracks 0 to 63
  pub const MAX_TRACKS: usize = 64;

  /// A state with no tracks playing
  pub fn new() -> Self {
    Self {
      time_scale: 1.0,
      tracks: Vec::new(),
      catalogue: None,
      kept: Vec::new(),
      told: Vec::new(),
      warned: Vec::new(),
      warned_track: false,
      next_id: 0,
      empty_name: Arc::from(EMPTY),
      crossed: Vec::new(),
    }
  }

  /// Plays `name` on `track` now, replacing what plays there: the new entry
  /// mixes in over its `mix_duration`, the old one mixing out, and the
  /// track's queue is dropped. A name the data lacks warns once per
  /// skeleton per name and returns `None`. Before the data lands the call is
  /// kept, and made when it lands
  pub fn set_animation(&mut self, track: usize, name: &str, looping: bool) -> Option<TrackEntryId> {
    if !self.track_ok(track) {
      return None;
    }
    let Some(catalogue) = self.catalogue.clone() else {
      let entry = self.make_entry(Arc::from(name), looping);
      let id = entry.id;
      self.drop_kept(track, true);
      self.kept.push(Kept::Set(track, entry));
      return Some(id);
    };
    let Some(index) = catalogue.index(name) else {
      self.warn_unknown(name);
      return None;
    };
    let mut entry = self.make_entry(Arc::from(name), looping);
    entry.animation = Some(index);
    entry.duration = catalogue.duration(index);
    Some(self.install(track, entry, &catalogue))
  }

  /// Queues `name` on `track` after what is playing or queued there. It
  /// starts `delay` seconds into the previous entry, or, with `delay <= 0`,
  /// at the previous entry's end plus `delay` (the entry's `delay` field
  /// holds the result); a looping entry's end is the end of the loop it is
  /// in. On an empty track it starts after `delay` seconds, at once for
  /// `delay <= 0`. Unknown names and calls before the data lands are
  /// handled as [`set_animation`](Self::set_animation) handles them
  pub fn add_animation(
    &mut self,
    track: usize,
    name: &str,
    looping: bool,
    delay: f32,
  ) -> Option<TrackEntryId> {
    if !self.track_ok(track) {
      return None;
    }
    let Some(catalogue) = self.catalogue.clone() else {
      let mut entry = self.make_entry(Arc::from(name), looping);
      entry.delay = delay;
      let id = entry.id;
      self.kept.push(Kept::Add(track, entry));
      return Some(id);
    };
    let Some(index) = catalogue.index(name) else {
      self.warn_unknown(name);
      return None;
    };
    let mut entry = self.make_entry(Arc::from(name), looping);
    entry.animation = Some(index);
    entry.duration = catalogue.duration(index);
    Some(self.enqueue(track, entry, delay, &catalogue))
  }

  /// Mixes `track` out over `mix` seconds, to the setup pose on track 0 or
  /// to the tracks below on a higher one, then clears it. It plays the
  /// empty animation, so an entry queued after it mixes in from nothing (a
  /// layer fading in)
  pub fn set_empty(&mut self, track: usize, mix: f32) {
    if !self.track_ok(track) {
      return;
    }
    let Some(catalogue) = self.catalogue.clone() else {
      self.drop_kept(track, false);
      self.kept.push(Kept::Empty(track, mix));
      return;
    };
    let mut entry = self.make_entry(self.empty_name.clone(), false);
    entry.empty = true;
    entry.mix_duration = non_negative(mix);
    entry.mix_from_data = false;
    self.install(track, entry, &catalogue);
  }

  /// Stops `track` at once: what plays there ends, and the queue is dropped
  pub fn clear(&mut self, track: usize) {
    if self.catalogue.is_none() {
      self.drop_kept(track, false);
      return;
    }
    let Some(lane) = self.tracks.get_mut(track) else {
      return;
    };
    for entry in lane.chain.drain(..).rev() {
      tell(&mut self.told, track, &entry, TrackEventKind::End);
      tell(&mut self.told, track, &entry, TrackEventKind::Dispose);
    }
    for entry in lane.queue.drain(..) {
      tell(&mut self.told, track, &entry, TrackEventKind::Dispose);
    }
  }

  /// The entry with this handle, playing, mixing out, queued or kept until
  /// the data lands
  pub fn entry(&self, id: TrackEntryId) -> Option<&TrackEntry> {
    self
      .tracks
      .iter()
      .flat_map(|track| track.chain.iter().chain(track.queue.iter()))
      .chain(self.kept.iter().filter_map(Kept::entry))
      .find(|entry| entry.id == id)
  }

  /// The entry with this handle, to change its speed, alpha, mix and so on
  pub fn entry_mut(&mut self, id: TrackEntryId) -> Option<&mut TrackEntry> {
    self
      .tracks
      .iter_mut()
      .flat_map(|track| track.chain.iter_mut().chain(track.queue.iter_mut()))
      .chain(self.kept.iter_mut().filter_map(Kept::entry_mut))
      .find(|entry| entry.id == id)
  }

  /// The entry playing on `track` (the one mixing in, during a mix). Before
  /// the data lands, the entry a kept call will play
  pub fn current(&self, track: usize) -> Option<TrackEntryId> {
    if self.catalogue.is_none() {
      let mut kept = self.kept.iter().filter(|kept| kept.track() == track);
      return match kept.next()? {
        Kept::Set(_, entry) | Kept::Add(_, entry) => Some(entry.id),
        Kept::Empty(..) => kept.next().and_then(Kept::entry).map(|entry| entry.id),
      };
    }
    let entry = self.tracks.get(track)?.chain.last()?;
    (!entry.empty).then_some(entry.id)
  }

  /// One step: advances every track by `dt` seconds times
  /// [`time_scale`](Self::time_scale), starts queued entries, ends finished
  /// mixes, pushes the event keys crossed (in time order) and the track
  /// events, then resets `pose` to the setup pose and applies the tracks
  /// from the lowest up. The first step to see `data` makes the calls kept
  /// until then; a step with new data (a reload) keeps each entry by its
  /// animation's name, and clears a track whose animation is gone, with one
  /// warning
  #[cfg_attr(
    not(test),
    expect(dead_code, reason = "the skeleton step calls it each fixed step")
  )]
  pub(crate) fn advance_and_apply(
    &mut self,
    data: &SkeletonData,
    pose: &mut Pose,
    dt: f32,
    events: &mut Vec<FiredEvent>,
    track_events: &mut Vec<FiredTrackEvent>,
  ) {
    self.see(data);
    track_events.append(&mut self.told);
    let step = non_negative(non_negative(dt) * non_negative(self.time_scale));
    let first_event = events.len();
    let mut crossed = std::mem::take(&mut self.crossed);
    for (index, track) in self.tracks.iter_mut().enumerate() {
      advance(track, index, step, track_events);
      fire(track, index, data, events, &mut crossed);
      end_mixes(track, index, track_events);
    }
    self.crossed = crossed;
    // Each entry's keys are in time order; across entries and tracks,
    // order by when in the step each was crossed (stable, so ties keep
    // track order)
    let fired = &mut events[first_event..];
    for index in 1..fired.len() {
      let mut at = index;
      while at > 0 && fired[at - 1].moment > fired[at].moment {
        fired.swap(at - 1, at);
        at -= 1;
      }
    }
    pose.reset(data);
    for track in &mut self.tracks {
      apply_track(track, data, pose);
    }
  }

  fn track_ok(&mut self, track: usize) -> bool {
    if track < Self::MAX_TRACKS {
      return true;
    }
    if !self.warned_track {
      self.warned_track = true;
      log::warn!(
        "skeleton track {track} is past the last ({}); ignored",
        Self::MAX_TRACKS - 1
      );
    }
    false
  }

  fn warn_unknown(&mut self, name: &str) {
    if !self.warned.iter().any(|warned| &**warned == name) {
      self.warned.push(Arc::from(name));
      log::warn!("skeleton animation {name:?} is not in the skeleton's data; ignored");
    }
  }

  /// Drops the calls kept for `track`, which a new call replaces. With
  /// `keep_empty`, a `set_empty` stays if nothing but queued entries came
  /// after it: the empty animation stays on a track for the next entry to
  /// mix in from, and what was queued after it is dropped
  fn drop_kept(&mut self, track: usize, keep_empty: bool) {
    let stays = self
      .kept
      .iter()
      .rposition(|kept| kept.track() == track && !matches!(kept, Kept::Add(..)))
      .filter(|&at| keep_empty && matches!(self.kept.get(at), Some(Kept::Empty(..))));
    let mut at = 0;
    self.kept.retain(|kept| {
      let keep = kept.track() != track || stays == Some(at);
      at += 1;
      keep
    });
  }

  fn make_entry(&mut self, name: Arc<str>, looping: bool) -> TrackEntry {
    let id = TrackEntryId(self.next_id);
    self.next_id = self.next_id.wrapping_add(1);
    TrackEntry::new(id, name, looping)
  }

  /// Makes `entry` the track's current entry now, mixing from the current
  /// one, and drops the queue
  fn install(
    &mut self,
    track: usize,
    mut entry: TrackEntry,
    catalogue: &AnimationCatalogue,
  ) -> TrackEntryId {
    if self.tracks.len() <= track {
      self.tracks.resize_with(track + 1, Track::default);
    }
    let lane = &mut self.tracks[track];
    for queued in lane.queue.drain(..) {
      tell(&mut self.told, track, &queued, TrackEventKind::Dispose);
    }
    let mut dropped = None;
    if let Some(current) = lane.chain.last_mut() {
      tell(&mut self.told, track, current, TrackEventKind::Interrupt);
      current.out_at = Some(current.track_time);
      // An entry replaced before it was ever shown is dropped, not mixed
      // from; the empty animation stays, to mix in from nothing
      if !current.applied && !current.empty {
        dropped = lane.chain.pop();
      }
    }
    let id = entry.id;
    let from = lane.chain.last();
    if entry.mix_from_data {
      if entry.mix_duration == 0.0 {
        entry.mix_duration = catalogue.mix(from.and_then(|from| from.animation), entry.animation);
      }
      entry.mix_from_data = false;
    }
    tell(&mut self.told, track, &entry, TrackEventKind::Start);
    if let Some(dropped) = dropped {
      tell(&mut self.told, track, &dropped, TrackEventKind::End);
      tell(&mut self.told, track, &dropped, TrackEventKind::Dispose);
    }
    join_mix(&mut lane.chain, entry);
    id
  }

  /// Queues `entry` after the track's last entry
  fn enqueue(
    &mut self,
    track: usize,
    mut entry: TrackEntry,
    delay: f32,
    catalogue: &AnimationCatalogue,
  ) -> TrackEntryId {
    let delay = if delay.is_finite() { delay } else { 0.0 };
    let previous = self
      .tracks
      .get(track)
      .and_then(|lane| lane.queue.back().or(lane.chain.last()));
    let Some(previous) = previous else {
      if delay <= 0.0 {
        return self.install(track, entry, catalogue);
      }
      entry.delay = delay;
      if entry.mix_from_data && entry.mix_duration == 0.0 {
        entry.mix_duration = catalogue.mix(None, entry.animation);
      }
      entry.mix_from_data = false;
      let id = entry.id;
      if self.tracks.len() <= track {
        self.tracks.resize_with(track + 1, Track::default);
      }
      self.tracks[track].queue.push_back(entry);
      return id;
    };
    entry.delay = if delay <= 0.0 {
      (previous.next_end() + delay).max(0.0)
    } else {
      delay
    };
    if entry.mix_from_data && entry.mix_duration == 0.0 {
      entry.mix_duration = catalogue.mix(previous.animation, entry.animation);
    }
    entry.mix_from_data = false;
    let id = entry.id;
    self.tracks[track].queue.push_back(entry);
    id
  }

  /// Takes up `data`'s animations: makes the kept calls the first time, and
  /// keeps entries by name when the data changes
  fn see(&mut self, data: &SkeletonData) {
    let catalogue = data.catalogue();
    if self
      .catalogue
      .as_ref()
      .is_some_and(|seen| Arc::ptr_eq(seen, catalogue))
    {
      return;
    }
    let first = self.catalogue.is_none();
    self.catalogue = Some(catalogue.clone());
    if !first {
      self.rename(catalogue);
    }
    for kept in std::mem::take(&mut self.kept) {
      match kept {
        Kept::Set(track, entry) => {
          if let Some(entry) = self.resolve(entry, catalogue) {
            self.install(track, entry, catalogue);
          }
        }
        Kept::Add(track, entry) => {
          if let Some(entry) = self.resolve(entry, catalogue) {
            let delay = entry.delay;
            self.enqueue(track, entry, delay, catalogue);
          }
        }
        Kept::Empty(track, mix) => self.set_empty(track, mix),
      }
    }
  }

  fn resolve(
    &mut self,
    mut entry: TrackEntry,
    catalogue: &AnimationCatalogue,
  ) -> Option<TrackEntry> {
    let Some(index) = catalogue.index(&entry.name) else {
      self.warn_unknown(&entry.name);
      return None;
    };
    entry.animation = Some(index);
    entry.duration = catalogue.duration(index);
    Some(entry)
  }

  /// After new data: each entry finds its animation by name. A track whose
  /// current animation is gone is cleared; another gone entry is dropped.
  /// Each gone name warns once
  fn rename(&mut self, catalogue: &AnimationCatalogue) {
    let gone = |entry: &TrackEntry| !entry.empty && catalogue.index(&entry.name).is_none();
    for index in 0..self.tracks.len() {
      let lane = &self.tracks[index];
      let current_gone = lane.chain.last().is_some_and(gone);
      let names: Vec<Arc<str>> = lane
        .chain
        .iter()
        .chain(lane.queue.iter())
        .filter(|entry| gone(entry))
        .map(|entry| entry.name.clone())
        .collect();
      for name in &names {
        self.warn_unknown(name);
      }
      if current_gone {
        self.clear(index);
        continue;
      }
      let lane = &mut self.tracks[index];
      let told = &mut self.told;
      lane.chain.retain(|entry| {
        let keep = !gone(entry);
        if !keep {
          tell(told, index, entry, TrackEventKind::End);
          tell(told, index, entry, TrackEventKind::Dispose);
        }
        keep
      });
      lane.queue.retain(|entry| {
        let keep = !gone(entry);
        if !keep {
          tell(told, index, entry, TrackEventKind::Dispose);
        }
        keep
      });
      for entry in lane.chain.iter_mut().chain(lane.queue.iter_mut()) {
        if let Some(found) = catalogue.index(&entry.name).filter(|_| !entry.empty) {
          entry.animation = Some(found);
          entry.duration = catalogue.duration(found);
        }
        entry.forget_timelines();
      }
    }
  }
}

fn tell(told: &mut Vec<FiredTrackEvent>, track: usize, entry: &TrackEntry, kind: TrackEventKind) {
  told.push(FiredTrackEvent {
    track,
    entry: entry.id,
    animation: entry.name.clone(),
    kind,
  });
}

/// Puts `entry` at the end of the chain, mixing from what was current. A
/// mix starts both entries' turn memories afresh (one already mixing keeps
/// its own)
fn join_mix(chain: &mut Vec<TrackEntry>, mut entry: TrackEntry) {
  entry.fades.clear();
  entry.turns.clear();
  if chain.len() == 1
    && let Some(from) = chain.last_mut()
  {
    from.turns.clear();
  }
  chain.push(entry);
}

/// Advances a track's entries by `step` seconds of skeleton time, starts
/// queued entries that are due, and tells the starts, interrupts and
/// completions
fn advance(track: &mut Track, index: usize, step: f32, told: &mut Vec<FiredTrackEvent>) {
  let count = track.chain.len();
  for position in 0..count {
    let held = track
      .chain
      .get(position + 1)
      .is_some_and(|next| next.hold_previous);
    let entry = &mut track.chain[position];
    let delta = if held {
      0.0
    } else {
      finite_time(step * entry.speed())
    };
    entry.span_start = if entry.advanced {
      entry.track_time
    } else {
      -1.0
    };
    entry.step_origin = entry.track_time;
    entry.step_start = 0.0;
    entry.step_rate = delta;
    entry.track_time = finite_time(entry.track_time + delta);
    entry.mix_time = finite_time(entry.mix_time + step);
    entry.advanced = true;
  }
  // Whether the current entry was current when the step began: an entry
  // mixing out completes no more
  let mut current_from = track.chain.last().map(|entry| entry.span_start);
  if track.chain.is_empty()
    && let Some(head) = track.queue.front_mut()
  {
    head.waited = finite_time(head.waited + step);
  }
  while let Some(head) = track.queue.front() {
    let trigger = non_negative(head.delay);
    // Seconds of the step left after the head is due
    let over = match track.chain.last_mut() {
      Some(current) => {
        if current.track_time < trigger {
          break;
        }
        let speed = current.speed();
        let over = if speed > 0.0 && step > 0.0 {
          ((current.track_time - trigger) / speed).clamp(0.0, step)
        } else {
          0.0
        };
        if let Some(from) = current_from.take() {
          complete(current, index, from, trigger.min(current.track_time), told);
        }
        current.out_at = Some(trigger);
        tell(told, index, current, TrackEventKind::Interrupt);
        over
      }
      None => {
        if head.waited < trigger {
          break;
        }
        (head.waited - trigger).clamp(0.0, step)
      }
    };
    let Some(mut entry) = track.queue.pop_front() else {
      break;
    };
    let speed = entry.speed();
    entry.step_start = if step > 0.0 {
      (step - over) / step
    } else {
      0.0
    };
    entry.step_origin = 0.0;
    entry.step_rate = finite_time(step * speed);
    entry.span_start = -1.0;
    entry.track_time = finite_time(over * speed);
    entry.mix_time = over;
    entry.advanced = true;
    tell(told, index, &entry, TrackEventKind::Start);
    join_mix(&mut track.chain, entry);
    current_from = Some(-1.0);
  }
  if let (Some(from), Some(current)) = (current_from, track.chain.last()) {
    complete(current, index, from, current.track_time, told);
  }
}

/// Tells a `Complete` for each time the entry passed its duration in
/// (`last`, `until`] of its track time: each loop when looping (at most
/// [`MAX_LOOPS`] in one step), once otherwise and for a zero-length
/// animation
fn complete(
  entry: &TrackEntry,
  index: usize,
  last: f32,
  until: f32,
  told: &mut Vec<FiredTrackEvent>,
) {
  if until <= last {
    return;
  }
  let duration = entry.duration;
  if !entry.looping || duration <= 0.0 {
    if last < duration && duration <= until {
      tell(told, index, entry, TrackEventKind::Complete);
    }
    return;
  }
  // Loop ends at k x duration, k from 1; one more each side than division
  // gives, as for events, so f32 rounding misses none
  let high = ((until / duration).floor() as i64).saturating_add(1);
  let low = ((last.max(0.0) / duration).floor() as i64)
    .saturating_sub(1)
    .max(1)
    .max(high.saturating_sub(MAX_LOOPS));
  for loops in low..=high {
    let end = loops as f32 * duration;
    if end > last && end <= until {
      tell(told, index, entry, TrackEventKind::Complete);
    }
  }
}

/// Pushes the event keys each entry of the track crossed this step. An
/// entry mixing out fires only until the mix replacing it reaches the
/// entry's event threshold
fn fire(
  track: &Track,
  index: usize,
  data: &SkeletonData,
  events: &mut Vec<FiredEvent>,
  crossed: &mut Vec<CrossedKey>,
) {
  for (position, entry) in track.chain.iter().enumerate() {
    let Some(animation) = entry
      .animation
      .and_then(|found| data.animations().get(found))
    else {
      continue;
    };
    let until = match (entry.out_at, track.chain.get(position + 1)) {
      (Some(out_at), Some(next)) => {
        let threshold = unit(entry.event_threshold);
        let limit = finite_time(out_at + threshold * next.mix_duration() * entry.speed());
        entry.track_time.min(limit)
      }
      _ => entry.track_time,
    };
    crossed.clear();
    events_crossed(
      animation,
      entry.span_start,
      until,
      entry.looping,
      entry.reverse,
      crossed,
    );
    for (at, event, values, time) in crossed.drain(..) {
      let moment = if entry.step_rate > 0.0 {
        entry.step_start + (at - entry.step_origin) / entry.step_rate
      } else {
        entry.step_start
      };
      events.push(FiredEvent {
        track: index,
        animation: entry.name.clone(),
        event,
        values,
        time,
        moment: moment.clamp(0.0, 1.0),
      });
    }
  }
}

/// Ends the entries whose mix-out is over: everything older than the
/// newest entry that has finished mixing in. Then an empty animation that
/// has finished its mix, with nothing queued after it, clears the track
fn end_mixes(track: &mut Track, index: usize, told: &mut Vec<FiredTrackEvent>) {
  let done = (1..track.chain.len())
    .rev()
    .find(|&position| track.chain[position].mix_done());
  if let Some(done) = done {
    for entry in track.chain.drain(..done) {
      tell(told, index, &entry, TrackEventKind::End);
      tell(told, index, &entry, TrackEventKind::Dispose);
    }
  }
  let finished = track.chain.len() == 1
    && track.queue.is_empty()
    && track
      .chain
      .first()
      .is_some_and(|entry| entry.empty && entry.mix_done());
  if finished {
    for entry in track.chain.drain(..) {
      tell(told, index, &entry, TrackEventKind::End);
      tell(told, index, &entry, TrackEventKind::Dispose);
    }
  }
}

/// Whether a timeline keys values that blend by alpha (bone and colour
/// timelines), as opposed to stepped keys and events
fn blends(timeline: &Timeline) -> bool {
  !matches!(
    timeline,
    Timeline::Attachment { .. } | Timeline::DrawOrder { .. } | Timeline::Event { .. }
  )
}

fn animation_of<'a>(entry: &TrackEntry, data: &'a SkeletonData) -> Option<&'a Animation> {
  entry
    .animation
    .and_then(|found| data.animations().get(found))
}

/// Applies one track: the entries it is mixing from, oldest first, then
/// the current one. Each entry mixing out takes, per timeline, the alpha
/// that makes its weight in the result fall from its own to 0 in step with
/// the mix (the module notes give the rule)
fn apply_track(track: &mut Track, data: &SkeletonData, pose: &mut Pose) {
  let count = track.chain.len();
  if count == 0 {
    return;
  }
  let mixing = count > 1;
  for position in 0..count {
    let entry = &mut track.chain[position];
    let timelines = animation_of(entry, data).map_or(0, |animation| animation.timelines.len());
    if mixing && entry.turns.len() != timelines {
      entry.turns.clear();
      entry.turns.resize(timelines, f32::NAN);
    }
  }
  // Fades, newest to oldest. `plain` is the product of (1 - mix) over the
  // entries newer than the one replacing this entry
  let mut plain = 1.0;
  for position in (0..count.saturating_sub(1)).rev() {
    let (older, newer) = track.chain.split_at_mut(position + 1);
    let entry = &mut older[position];
    let next = &newer[0];
    let next_is_current = position + 2 == count;
    if !entry.covered {
      entry.cover.clear();
      if let (Some(mine), theirs) = (animation_of(entry, data), animation_of(next, data)) {
        for timeline in &mine.timelines {
          let found = theirs.filter(|_| blends(timeline)).and_then(|theirs| {
            theirs.timelines.iter().position(|other| {
              other.target() == timeline.target()
                && timeline
                  .properties()
                  .iter()
                  .all(|property| other.properties().contains(property))
            })
          });
          entry.cover.push(found);
        }
      }
      entry.covered = true;
    }
    let next_mix = next.mix_fraction();
    let next_alpha = next.alpha() * next_mix;
    entry.fades.clear();
    for timeline in 0..entry.cover.len() {
      let covering = entry
        .cover
        .get(timeline)
        .copied()
        .flatten()
        .filter(|_| next.mix_blend == MixBlend::Replace);
      let (applied, beyond) = match covering {
        Some(theirs) => {
          let fade = if next_is_current {
            1.0
          } else {
            next.fades.get(theirs).copied().unwrap_or(plain)
          };
          (next_alpha * fade, fade)
        }
        None => (0.0, plain),
      };
      let factor = if applied >= 1.0 - 1e-6 {
        1.0
      } else {
        (1.0 - next_mix) / (1.0 - applied)
      };
      entry.fades.push(factor * beyond);
    }
    plain *= 1.0 - next_mix;
  }
  for position in 0..count {
    let current = position + 1 == count;
    let entry = &mut track.chain[position];
    let Some(animation) = animation_of(entry, data) else {
      entry.applied = true;
      continue;
    };
    let mix = if position == 0 {
      1.0
    } else {
      entry.mix_fraction()
    };
    let alpha = entry.alpha() * mix;
    // Stepped keys belong to the entry mixing in, and only once it has
    // weight: a layer at alpha 0 does not force them
    let direction = if current && alpha > 0.0 {
      MixDirection::In
    } else {
      MixDirection::Out
    };
    let layer = Layer {
      time: entry.animation_time(),
      alpha,
      fades: if current { &[] } else { &entry.fades },
      blend: entry.mix_blend,
      direction,
    };
    let turns: &mut [f32] = if mixing { &mut entry.turns } else { &mut [] };
    apply_layer(animation, data, pose, layer, turns);
    entry.applied = true;
  }
}

/// A finite value, not negative: anything else is 0
fn non_negative(value: f32) -> f32 {
  if value.is_finite() && value > 0.0 {
    value
  } else {
    0.0
  }
}

/// A value in 0..=1 (NaN is 0)
fn unit(value: f32) -> f32 {
  if value.is_nan() {
    0.0
  } else {
    value.clamp(0.0, 1.0)
  }
}

/// A time kept finite: an overflow holds at the largest float, NaN is 0
fn finite_time(value: f32) -> f32 {
  if value.is_nan() {
    0.0
  } else {
    value.min(f32::MAX)
  }
}

#[cfg(test)]
mod tests {
  use std::sync::Arc;

  use super::*;
  use crate::core::ecs::Vec2;
  use crate::core::skeleton::{
    Animation, BoneId, BoneSetup, Curve, EventData, SlotId, SlotSetup, Timeline,
  };
  use crate::testing::capture_log;

  const ROOT: BoneId = BoneId(0);

  fn rotate(degrees: f32) -> Timeline {
    Timeline::Rotate {
      bone: ROOT,
      frames: vec![(0.0, degrees.to_radians(), Curve::Linear)],
    }
  }

  fn ticks(times: &[f32]) -> Timeline {
    Timeline::Event {
      frames: times
        .iter()
        .map(|&time| (time, 0, EventValues::default()))
        .collect(),
    }
  }

  fn clip(name: &str, duration: f32, timelines: Vec<Timeline>) -> Animation {
    Animation {
      name: Arc::from(name),
      duration,
      timelines,
    }
  }

  /// One root bone, a slot "body" showing "torso", an event "tick", and
  /// animations named for what they do. The default mix is 0.2 s; "slide"
  /// to "thirty" has its own, 0.5 s
  fn data() -> SkeletonData {
    let tenths: Vec<f32> = (0..10).map(|i| i as f32 * 0.1).collect();
    SkeletonData::builder()
      .bone("root", None, BoneSetup::default())
      .slot(
        "body",
        "root",
        SlotSetup {
          attachment: Some(Arc::from("torso")),
          ..SlotSetup::default()
        },
      )
      .event(EventData::new("tick"))
      .animation(clip("zero", 1.0, vec![rotate(0.0)]))
      .animation(clip("ninety", 1.0, vec![rotate(90.0)]))
      .animation(clip("thirty", 1.0, vec![rotate(30.0)]))
      .animation(clip("half_turn", 1.0, vec![rotate(180.0)]))
      .animation(clip(
        "slide",
        1.0,
        vec![Timeline::TranslateX {
          bone: ROOT,
          frames: vec![(0.0, 10.0, Curve::Linear)],
        }],
      ))
      .animation(clip(
        "sweep",
        1.0,
        vec![Timeline::Rotate {
          bone: ROOT,
          frames: vec![
            (0.0, (-30.0_f32).to_radians(), Curve::Linear),
            (1.0, 30.0_f32.to_radians(), Curve::Linear),
          ],
        }],
      ))
      .animation(clip("ticks", 1.0, vec![ticks(&tenths)]))
      .animation(clip("halves", 1.0, vec![ticks(&[0.0, 0.5])]))
      .animation(clip("short", 0.5, vec![rotate(0.0)]))
      .animation(clip(
        "swap",
        1.0,
        vec![Timeline::Attachment {
          slot: SlotId(0),
          frames: vec![(0.0, Some(Arc::from("fist")))],
        }],
      ))
      .animation(clip("instant", 0.0, vec![rotate(45.0), ticks(&[0.0])]))
      .animation(clip(
        "shift",
        1.0,
        vec![Timeline::Translate {
          bone: ROOT,
          frames: vec![(0.0, Vec2::new(10.0, 10.0), Curve::Linear)],
        }],
      ))
      .mix("slide", "thirty", 0.5)
      .default_mix(0.2)
      .build()
      .expect("a valid skeleton")
  }

  struct Rig {
    data: SkeletonData,
    pose: Pose,
    state: SkeletonAnimation,
  }

  /// What one step fired
  #[derive(Default)]
  struct Fired {
    events: Vec<FiredEvent>,
    track: Vec<FiredTrackEvent>,
  }

  impl Fired {
    fn times(&self) -> Vec<f32> {
      self.events.iter().map(|event| event.time).collect()
    }

    fn kinds(&self) -> Vec<(&str, TrackEventKind)> {
      self
        .track
        .iter()
        .map(|event| (&*event.animation, event.kind))
        .collect()
    }
  }

  impl Rig {
    fn new() -> Self {
      let data = data();
      let pose = Pose::setup(&data);
      Self {
        data,
        pose,
        state: SkeletonAnimation::new(),
      }
    }

    fn step(&mut self, dt: f32) -> Fired {
      let mut fired = Fired::default();
      self.state.advance_and_apply(
        &self.data,
        &mut self.pose,
        dt,
        &mut fired.events,
        &mut fired.track,
      );
      fired
    }

    fn set(&mut self, track: usize, name: &str, looping: bool) -> TrackEntryId {
      self
        .state
        .set_animation(track, name, looping)
        .expect("a known animation")
    }

    fn add(&mut self, track: usize, name: &str, looping: bool, delay: f32) -> TrackEntryId {
      self
        .state
        .add_animation(track, name, looping, delay)
        .expect("a known animation")
    }

    fn entry(&mut self, id: TrackEntryId) -> &mut TrackEntry {
      self.state.entry_mut(id).expect("a live entry")
    }

    /// Whether every value of the root bone's pose is finite
    fn finite(&self) -> bool {
      let bone = self.pose.bones[0];
      [
        bone.x,
        bone.y,
        bone.rotation,
        bone.scale_x,
        bone.scale_y,
        bone.shear_x,
        bone.shear_y,
      ]
      .into_iter()
      .all(f32::is_finite)
    }

    /// The root bone's rotation, in degrees
    fn degrees(&self) -> f32 {
      self.pose.bones[0].rotation.to_degrees()
    }
  }

  fn close(actual: f32, expected: f32) {
    assert!(
      (actual - expected).abs() < 1e-3,
      "expected {expected}, got {actual}"
    );
  }

  #[test]
  fn a_crossfade_is_half_way_at_half_its_mix() {
    let mut rig = Rig::new();
    rig.set(0, "zero", true);
    rig.step(0.05);
    close(rig.degrees(), 0.0);
    let ninety = rig.set(0, "ninety", true);
    // The data's default mix
    close(rig.entry(ninety).mix_duration, 0.2);
    // 0%, 50% and 100% of the mix
    rig.step(0.0);
    close(rig.degrees(), 0.0);
    rig.step(0.1);
    close(rig.degrees(), 45.0);
    rig.step(0.1);
    close(rig.degrees(), 90.0);
    rig.step(0.1);
    close(rig.degrees(), 90.0);
  }

  #[test]
  fn an_additive_track_adds_over_track_zero() {
    let mut rig = Rig::new();
    rig.set(0, "ninety", true);
    let add = rig.set(1, "thirty", true);
    rig.entry(add).mix_blend = MixBlend::Add;
    rig.step(0.1);
    close(rig.degrees(), 120.0);
    rig.entry(add).alpha = 0.5;
    rig.step(0.1);
    close(rig.degrees(), 105.0);
  }

  #[test]
  fn a_higher_track_at_half_alpha_blends_half() {
    let mut rig = Rig::new();
    rig.set(0, "zero", true);
    let layer = rig.set(1, "ninety", true);
    rig.entry(layer).alpha = 0.5;
    rig.step(0.1);
    close(rig.degrees(), 45.0);
    // An alpha outside 0 to 1 is clamped
    rig.entry(layer).alpha = 3.0;
    rig.step(0.1);
    close(rig.degrees(), 90.0);
    rig.entry(layer).alpha = -1.0;
    rig.step(0.1);
    close(rig.degrees(), 0.0);
    // Track 0 replaces the setup pose: alone at half alpha it blends half
    // way from setup
    let mut rig = Rig::new();
    let base = rig.set(0, "ninety", true);
    rig.entry(base).alpha = 0.5;
    rig.step(0.1);
    close(rig.degrees(), 45.0);
  }

  #[test]
  fn set_empty_fades_a_layer_out_then_clears_it() {
    let mut rig = Rig::new();
    rig.set(0, "thirty", true);
    rig.set(1, "ninety", true);
    rig.step(0.1);
    close(rig.degrees(), 90.0);
    rig.state.set_empty(1, 0.2);
    rig.step(0.1);
    // Half way back to what track 0 leaves; the empty animation playing
    // meanwhile is not reported as the track's current entry
    close(rig.degrees(), 60.0);
    assert_eq!(rig.state.current(1), None);
    let fired = rig.step(0.1);
    close(rig.degrees(), 30.0);
    assert_eq!(rig.state.current(1), None);
    assert!(
      fired
        .kinds()
        .contains(&("<empty>", TrackEventKind::Dispose)),
      "{:?}",
      fired.kinds()
    );
    // On track 0 it fades to the setup pose
    rig.state.set_empty(0, 0.2);
    rig.step(0.1);
    close(rig.degrees(), 15.0);
    rig.step(0.1);
    close(rig.degrees(), 0.0);
    assert_eq!(rig.state.current(0), None);
    // A zero mix clears at once
    rig.set(0, "ninety", true);
    rig.step(0.1);
    rig.state.set_empty(0, 0.0);
    rig.step(0.1);
    close(rig.degrees(), 0.0);
    assert_eq!(rig.state.current(0), None);
  }

  #[test]
  fn a_layer_fades_in_after_set_empty() {
    let mut rig = Rig::new();
    rig.set(0, "zero", true);
    // A first entry on an empty track shows at once, at its alpha
    rig.set(1, "ninety", true);
    rig.step(0.1);
    close(rig.degrees(), 90.0);
    rig.state.clear(1);
    rig.step(0.1);
    close(rig.degrees(), 0.0);
    // To fade in, set the track empty and queue the animation after it
    rig.state.set_empty(1, 0.0);
    let ninety = rig
      .state
      .add_animation(1, "ninety", true, 0.0)
      .expect("a known animation");
    close(rig.entry(ninety).mix_duration, 0.2);
    rig.step(0.1);
    close(rig.degrees(), 45.0);
    rig.step(0.1);
    close(rig.degrees(), 90.0);
  }

  #[test]
  fn a_queued_entry_starts_after_its_delay() {
    let mut rig = Rig::new();
    let zero = rig.set(0, "zero", false);
    let ninety = rig
      .state
      .add_animation(0, "ninety", false, 0.5)
      .expect("a known animation");
    rig.entry(ninety).mix_duration = 0.0;
    rig.step(0.25);
    assert_eq!(rig.state.current(0), Some(zero));
    close(rig.degrees(), 0.0);
    // The delay is from the previous entry's start; a step past it carries
    // the rest into the new entry
    rig.step(0.5);
    assert_eq!(rig.state.current(0), Some(ninety));
    close(rig.entry(ninety).track_time(), 0.25);
    close(rig.degrees(), 90.0);
    // A delay of 0 or less is measured from the previous entry's end (its
    // duration plus the delay); the field holds the result
    let thirty = rig
      .state
      .add_animation(0, "thirty", false, -0.25)
      .expect("a known animation");
    close(rig.entry(thirty).delay, 0.75);
    rig.entry(thirty).mix_duration = 0.0;
    rig.step(0.25);
    assert_eq!(rig.state.current(0), Some(ninety));
    rig.step(0.25);
    assert_eq!(rig.state.current(0), Some(thirty));
    close(rig.degrees(), 30.0);
    // On an empty track the delay counts from the call
    rig.state.clear(1);
    let layer = rig
      .state
      .add_animation(1, "ninety", true, 0.3)
      .expect("a known animation");
    rig.step(0.2);
    assert_eq!(rig.state.current(1), None);
    close(rig.degrees(), 30.0);
    rig.step(0.2);
    assert_eq!(rig.state.current(1), Some(layer));
    close(rig.degrees(), 90.0);
    // With no delay it is current at once, as if set
    let at_once = rig.add(2, "thirty", true, 0.0);
    assert_eq!(rig.state.current(2), Some(at_once));
    // A looping entry's end is the end of the loop it is in when the call
    // is made: 2.25 s into a 1 s loop that is at 3 s, so half a second
    // before it is at 2.5 s
    let mut rig = Rig::new();
    let zero = rig.set(0, "zero", true);
    rig.step(2.25);
    let after = rig.add(0, "ninety", false, -0.5);
    close(rig.entry(after).delay, 2.5);
    rig.step(0.125);
    assert_eq!(rig.state.current(0), Some(zero));
    rig.step(0.25);
    assert_eq!(rig.state.current(0), Some(after));
    close(rig.entry(after).track_time(), 0.125);
  }

  #[test]
  fn events_fire_once_across_loops_large_steps_and_time_scales() {
    let mut rig = Rig::new();
    rig.set(0, "halves", true);
    let mut times = Vec::new();
    let mut fired_steps = Vec::new();
    // (dt, time scale): small steps, a stopped clock, triple speed, a step
    // of 2.5 durations, a pause; 4.1 s of animation in all
    let steps = [
      (0.1, 1.0),
      (0.1, 1.0),
      (0.1, 1.0),
      (0.2, 0.0),
      (0.1, 3.0),
      (2.5, 1.0),
      (0.0, 1.0),
      (0.15, 3.0),
      (0.15, 3.0),
      (0.1, 1.0),
    ];
    for (dt, scale) in steps {
      rig.state.time_scale = scale;
      let fired = rig.step(dt);
      fired_steps.push(fired.events.len());
      times.extend(fired.times());
    }
    // Keys at 0 and 0.5 each loop: 0, 0.5, ..., 4.0 is nine keys
    let expected: Vec<f32> = (0..9).map(|i| if i % 2 == 0 { 0.0 } else { 0.5 }).collect();
    assert_eq!(times, expected, "per step {fired_steps:?}");
    // The stopped clock and the pause fire nothing
    assert_eq!(fired_steps[3], 0);
    assert_eq!(fired_steps[6], 0);
    // The long step fires its five keys in time order
    assert_eq!(fired_steps[5], 5);
    // An entry's first step puts it at its time 0, so a key there fires
    // even while the clock is stopped, and only once
    let mut rig = Rig::new();
    rig.state.time_scale = 0.0;
    rig.set(0, "halves", true);
    assert_eq!(rig.step(0.1).times(), vec![0.0]);
    assert_eq!(rig.step(0.1).times(), Vec::<f32>::new());
    rig.state.time_scale = 1.0;
    assert_eq!(rig.step(0.6).times(), vec![0.5]);
  }

  #[test]
  fn events_from_several_tracks_come_in_time_order() {
    let mut rig = Rig::new();
    rig.set(0, "halves", true);
    let late = rig
      .state
      .add_animation(1, "ticks", true, 0.25)
      .expect("a known animation");
    let fired = rig.step(0.6);
    // Track 0 at 0 and 0.5; track 1 starts 0.25 in, its keys at 0.25 to
    // 0.55 of the step
    let order: Vec<(usize, f32)> = fired
      .events
      .iter()
      .map(|event| (event.track, event.time))
      .collect();
    let expected = [(0, 0.0), (1, 0.0), (1, 0.1), (1, 0.2), (0, 0.5), (1, 0.3)];
    assert_eq!(order.len(), expected.len(), "{order:?}");
    for ((track, time), (want_track, want_time)) in order.iter().zip(expected) {
      assert_eq!(*track, want_track, "{order:?}");
      close(*time, want_time);
    }
    assert!(rig.state.entry(late).is_some());
  }

  #[test]
  fn a_mixing_out_entry_stops_firing_past_its_event_threshold() {
    // Keys every 0.1 s; replaced 0.05 s in by a 0.4 s mix. A threshold of
    // 0.5 lets the old entry fire until the mix is half done, 0.2 s later
    for steps in [vec![0.1; 5], vec![0.5]] {
      let mut rig = Rig::new();
      let ticks = rig.set(0, "ticks", true);
      assert_eq!(rig.step(0.05).times(), vec![0.0]);
      let zero = rig.set(0, "zero", true);
      rig.entry(zero).mix_duration = 0.4;
      rig.entry(ticks).event_threshold = 0.5;
      let mut times = Vec::new();
      for dt in steps {
        times.extend(rig.step(dt).times());
      }
      assert_eq!(times.len(), 2, "{times:?}");
      close(times[0], 0.1);
      close(times[1], 0.2);
    }
    // At the default threshold, 0, nothing fires once it is replaced
    let mut rig = Rig::new();
    rig.set(0, "ticks", true);
    rig.step(0.05);
    rig.set(0, "zero", true);
    let mut count = 0;
    for _ in 0..5 {
      count += rig.step(0.1).events.len();
    }
    assert_eq!(count, 0);
    // Holding the previous entry freezes its time, so it fires nothing
    let mut rig = Rig::new();
    let ticks = rig.set(0, "ticks", true);
    rig.step(0.05);
    let zero = rig.set(0, "zero", true);
    rig.entry(zero).hold_previous = true;
    rig.entry(ticks).event_threshold = 1.0;
    let before = rig.state.entry(ticks).map(TrackEntry::track_time);
    assert_eq!(rig.step(0.1).events.len(), 0);
    assert_eq!(rig.state.entry(ticks).map(TrackEntry::track_time), before);
  }

  #[test]
  fn track_events_follow_start_interrupt_end_complete_dispose() {
    use TrackEventKind::*;
    let mut rig = Rig::new();
    let first = rig.set(0, "short", false);
    assert!(!rig.entry(first).is_complete());
    assert_eq!(rig.step(0.25).kinds(), vec![("short", Start)]);
    assert!(!rig.entry(first).is_complete());
    // Once for a non-looping entry
    assert_eq!(rig.step(0.3).kinds(), vec![("short", Complete)]);
    assert!(rig.entry(first).is_complete());
    assert_eq!(rig.step(0.3).kinds(), vec![]);
    rig.set(0, "ninety", true);
    assert_eq!(
      rig.step(0.1).kinds(),
      vec![("short", Interrupt), ("ninety", Start)]
    );
    // End and Dispose when the mix-out is over
    assert_eq!(
      rig.step(0.1).kinds(),
      vec![("short", End), ("short", Dispose)]
    );
    // Each loop of a looping entry, even several in one step
    assert_eq!(
      rig.step(2.0).kinds(),
      vec![("ninety", Complete), ("ninety", Complete)]
    );
    // A queued entry that never starts is only disposed
    rig
      .state
      .add_animation(0, "zero", true, 5.0)
      .expect("a known animation");
    rig.state.clear(0);
    assert_eq!(
      rig.step(0.1).kinds(),
      vec![("ninety", End), ("ninety", Dispose), ("zero", Dispose)]
    );
    // A queued entry interrupts the one it replaces
    let short = rig.set(0, "short", false);
    let zero = rig
      .state
      .add_animation(0, "zero", true, 0.0)
      .expect("a known animation");
    rig.entry(zero).mix_duration = 0.0;
    assert_eq!(
      rig.step(0.6).kinds(),
      vec![
        ("short", Start),
        ("short", Complete),
        ("short", Interrupt),
        ("zero", Start),
        ("short", End),
        ("short", Dispose),
      ]
    );
    assert!(rig.state.entry(short).is_none());
    // A step over more loops than the cap completes the cap's worth
    let mut rig = Rig::new();
    rig.set(0, "ninety", true);
    rig.step(0.5);
    let fired = rig.step(5000.0);
    assert_eq!(fired.kinds().len(), 1000);
    assert!(
      fired
        .kinds()
        .iter()
        .all(|kind| *kind == ("ninety", Complete))
    );
  }

  #[test]
  fn zero_length_animations_and_zero_mixes_are_finite() {
    use TrackEventKind::*;
    let mut rig = Rig::new();
    let instant = rig.set(0, "instant", true);
    let fired = rig.step(0.1);
    // A zero-length looping entry completes and fires its key once
    assert_eq!(
      fired.kinds(),
      vec![("instant", Start), ("instant", Complete)]
    );
    assert_eq!(fired.times(), vec![0.0]);
    close(rig.degrees(), 45.0);
    let fired = rig.step(0.1);
    assert_eq!(fired.kinds(), vec![]);
    assert_eq!(fired.times(), Vec::<f32>::new());
    assert!(
      rig
        .state
        .entry(instant)
        .is_some_and(TrackEntry::is_complete)
    );
    // Set again, it completes and fires its key once more
    let again = rig.set(0, "instant", true);
    rig.entry(again).mix_duration = 0.0;
    let fired = rig.step(0.1);
    assert_eq!(
      fired.kinds(),
      vec![
        ("instant", Interrupt),
        ("instant", Start),
        ("instant", Complete),
        ("instant", End),
        ("instant", Dispose)
      ]
    );
    assert_eq!(fired.times(), vec![0.0]);
    assert_eq!(rig.step(0.1).times(), Vec::<f32>::new());
    // A zero mix switches in one step
    let zero = rig.set(0, "zero", true);
    rig.entry(zero).mix_duration = 0.0;
    assert_eq!(
      rig.step(0.1).kinds(),
      vec![
        ("instant", Interrupt),
        ("zero", Start),
        ("instant", End),
        ("instant", Dispose)
      ]
    );
    close(rig.degrees(), 0.0);
    // Non-finite and absurd inputs never reach a pose or a time
    let ninety = rig.set(0, "ninety", true);
    rig.entry(ninety).mix_duration = f32::NAN;
    rig.entry(ninety).alpha = f32::NAN;
    rig.entry(ninety).speed = f32::INFINITY;
    rig.entry(ninety).event_threshold = f32::NAN;
    rig.entry(ninety).delay = f32::NAN;
    rig.state.time_scale = f32::INFINITY;
    // The pose and the entry's time are finite after every such step
    let step = |rig: &mut Rig, dt: f32| {
      rig.step(dt);
      let time = rig.state.entry(ninety).map(TrackEntry::track_time);
      assert!(rig.finite(), "{dt}: {:?}", rig.pose.bones[0]);
      assert!(time.is_some_and(f32::is_finite), "{dt}: {time:?}");
    };
    step(&mut rig, 0.1);
    rig.state.time_scale = 1.0;
    step(&mut rig, f32::NAN);
    // A non-finite speed holds the entry still
    step(&mut rig, 0.1);
    close(rig.entry(ninety).track_time(), 0.0);
    rig.entry(ninety).speed = 1.0;
    rig.entry(ninety).alpha = 1.0;
    // And so do a negative step and a negative time scale
    step(&mut rig, -1.0);
    rig.state.time_scale = -2.0;
    step(&mut rig, 0.1);
    close(rig.entry(ninety).track_time(), 0.0);
    rig.state.time_scale = 1.0;
    // Times that would overflow hold at the largest float: the second such
    // step would add past it, and a speed of 3 would multiply past it
    step(&mut rig, 1e30);
    step(&mut rig, f32::MAX);
    step(&mut rig, f32::MAX);
    close(rig.entry(ninety).track_time(), f32::MAX);
    rig.entry(ninety).speed = 3.0;
    step(&mut rig, f32::MAX);
    step(&mut rig, f32::INFINITY);
    close(rig.degrees(), 90.0);
  }

  #[test]
  fn a_zero_alpha_layer_does_not_force_its_attachments() {
    let mut rig = Rig::new();
    let swap = rig.set(1, "swap", true);
    rig.entry(swap).alpha = 0.0;
    rig.step(0.1);
    assert_eq!(rig.pose.slot_attachments[0].as_deref(), Some("torso"));
    // Any alpha above 0 sets them: a step cannot be part-applied
    rig.entry(swap).alpha = 0.25;
    rig.step(0.1);
    assert_eq!(rig.pose.slot_attachments[0].as_deref(), Some("fist"));
  }

  #[test]
  fn attachments_belong_to_the_entry_mixing_in() {
    // "swap" keys the body's attachment, "zero" does not: from the step the
    // mix begins, the slot shows what the incoming entry (or the tracks
    // below, or the setup pose) gives it
    let mut rig = Rig::new();
    rig.set(0, "swap", true);
    rig.step(0.1);
    assert_eq!(rig.pose.slot_attachments[0].as_deref(), Some("fist"));
    rig.set(0, "zero", true);
    rig.step(0.0);
    // At 0% of the mix the incoming entry has no weight yet: nothing sets
    // the slot, so it shows its setup attachment
    assert_eq!(rig.pose.slot_attachments[0].as_deref(), Some("torso"));
    rig.step(0.1);
    assert_eq!(rig.pose.slot_attachments[0].as_deref(), Some("torso"));
    rig.step(0.2);
    assert_eq!(rig.pose.slot_attachments[0].as_deref(), Some("torso"));
    // Mixing into "swap", it sets the slot once it has any weight
    rig.set(0, "swap", true);
    rig.step(0.0);
    assert_eq!(rig.pose.slot_attachments[0].as_deref(), Some("torso"));
    rig.step(0.05);
    assert_eq!(rig.pose.slot_attachments[0].as_deref(), Some("fist"));
  }

  #[test]
  fn a_crossfade_keeps_each_bone_turning_one_way() {
    // "sweep" turns the bone from -30 to 30 degrees; "half_turn" holds 180.
    // Half way through, the short way from the sweep to 180 flips from
    // anticlockwise to clockwise; the mix keeps the way it started with
    let mut rig = Rig::new();
    rig.set(0, "sweep", true);
    rig.step(0.0001);
    let turn = rig.set(0, "half_turn", true);
    rig.entry(turn).mix_duration = 1.0;
    let mut last = rig.degrees();
    for _ in 0..20 {
      rig.step(0.05);
      let now = rig.degrees();
      let change = (now - last + 540.0).rem_euclid(360.0) - 180.0;
      assert!(change.abs() < 20.0, "{last} to {now}");
      last = now;
    }
    // At the end it rests at 180
    close((last + 360.0).rem_euclid(360.0), 180.0);
  }

  #[test]
  fn reverse_plays_backward_and_fires_events_backward() {
    let mut rig = Rig::new();
    let sweep = rig.set(0, "sweep", false);
    rig.entry(sweep).reverse = true;
    rig.step(0.25);
    // A quarter in, reversed: the animation's time is 0.75
    close(rig.degrees(), 15.0);
    let mut rig = Rig::new();
    let halves = rig.set(0, "halves", false);
    rig.entry(halves).reverse = true;
    let mut times = Vec::new();
    for _ in 0..6 {
      times.extend(rig.step(0.2).times());
    }
    // The key at 0.5 is crossed half way, the key at 0 at the end
    assert_eq!(times, vec![0.5, 0.0]);
    // Looping, each loop backward
    let mut rig = Rig::new();
    let ticks = rig.set(0, "halves", true);
    rig.entry(ticks).reverse = true;
    assert_eq!(rig.step(1.75).times(), vec![0.5, 0.0, 0.5]);
  }

  #[test]
  fn an_unknown_name_warns_once_and_does_nothing() {
    let capture = capture_log();
    let mut rig = Rig::new();
    rig.set(0, "ninety", true);
    rig.step(0.1);
    for _ in 0..3 {
      assert_eq!(rig.state.set_animation(0, "dance", true), None);
      assert_eq!(rig.state.add_animation(0, "dance", true, 0.0), None);
    }
    assert_eq!(capture.containing("dance"), 1);
    rig.step(0.1);
    close(rig.degrees(), 90.0);
    // Per skeleton: another skeleton warns again
    let mut other = Rig::new();
    other.step(0.1);
    assert_eq!(other.state.set_animation(0, "dance", true), None);
    assert_eq!(capture.containing("dance"), 2);
  }

  #[test]
  fn a_call_before_the_data_lands_is_kept() {
    let capture = capture_log();
    // A fresh state has seen no data, so it keeps the calls, and their
    // entries can be changed meanwhile
    let mut state = SkeletonAnimation::new();
    let layer = state
      .set_animation(1, "ninety", true)
      .expect("kept until the data lands");
    assert_eq!(state.current(1), Some(layer));
    if let Some(entry) = state.entry_mut(layer) {
      entry.alpha = 0.5;
    }
    let unknown = state.set_animation(2, "dance", true);
    assert!(unknown.is_some());
    // A kept entry has not started, so it is not complete
    assert!(state.entry(layer).is_some_and(|entry| !entry.is_complete()));
    let mut rig = Rig::new();
    rig.state = state;
    rig.set(0, "zero", true);
    let fired = rig.step(0.1);
    close(rig.degrees(), 45.0);
    assert_eq!(rig.state.current(1), Some(layer));
    assert!(fired.kinds().contains(&("ninety", TrackEventKind::Start)));
    // The unknown name warns when the data lands, and does nothing
    assert_eq!(rig.state.current(2), None);
    assert_eq!(capture.containing("dance"), 1);
    // A kept entry's mix comes from the data when it lands, unless it was
    // set meanwhile
    let mut state = SkeletonAnimation::new();
    state.set_animation(0, "zero", true);
    let unset = state
      .add_animation(0, "ninety", true, 0.5)
      .expect("kept until the data lands");
    let set = state
      .add_animation(0, "thirty", true, 0.5)
      .expect("kept until the data lands");
    if let Some(entry) = state.entry_mut(set) {
      entry.mix_duration = 0.4;
    }
    let mut rig = Rig::new();
    rig.state = state;
    rig.step(0.1);
    close(rig.entry(unset).mix_duration, 0.2);
    close(rig.entry(set).mix_duration, 0.4);
  }

  #[test]
  fn new_data_keeps_tracks_by_animation_name() {
    use TrackEventKind::*;
    // The same skeleton with its animations in another order, and without
    // "slide"
    let reloaded = |data: &SkeletonData| {
      let mut builder = SkeletonData::builder()
        .bone("root", None, BoneSetup::default())
        .slot("body", "root", SlotSetup::default());
      for name in ["thirty", "zero", "ninety"] {
        let animation = data.animation(name).expect("in the data").clone();
        builder = builder.animation(animation);
      }
      builder.build().expect("a valid skeleton")
    };
    let mut rig = Rig::new();
    rig.set(0, "ninety", true);
    rig.set(1, "slide", true);
    rig.step(0.3);
    rig.data = reloaded(&rig.data);
    let capture = capture_log();
    let fired = rig.step(0.1);
    close(rig.degrees(), 90.0);
    close(rig.pose.bones[0].x, 0.0);
    assert_eq!(rig.state.current(1), None);
    assert_eq!(fired.kinds(), vec![("slide", End), ("slide", Dispose)]);
    assert_eq!(capture.containing("slide"), 1);
    let time = rig.state.current(0).and_then(|id| rig.state.entry(id));
    close(time.map_or(0.0, TrackEntry::track_time), 0.4);
    // A gone animation that is only mixing out or queued is dropped, and
    // the track plays on
    let mut rig = Rig::new();
    rig.set(0, "slide", true);
    rig.step(0.1);
    let thirty = rig.set(0, "thirty", true);
    let queued = rig.add(0, "slide", true, 5.0);
    rig.step(0.1);
    rig.data = reloaded(&rig.data);
    let fired = rig.step(0.1);
    assert_eq!(rig.state.current(0), Some(thirty));
    assert!(rig.state.entry(queued).is_none());
    assert_eq!(
      fired.kinds(),
      vec![("slide", End), ("slide", Dispose), ("slide", Dispose)]
    );
    close(rig.degrees(), 30.0);
    close(rig.entry(thirty).track_time(), 0.2);
    assert_eq!(capture.containing("slide"), 2);
    // A track whose current animation is gone is cleared whole: the entry
    // it was mixing from does not come back
    let mut rig = Rig::new();
    rig.set(0, "ninety", true);
    rig.step(0.1);
    rig.set(0, "slide", true);
    rig.step(0.1);
    rig.data = reloaded(&rig.data);
    let fired = rig.step(0.1);
    assert_eq!(rig.state.current(0), None);
    assert_eq!(
      fired.kinds(),
      vec![
        ("slide", End),
        ("slide", Dispose),
        ("ninety", End),
        ("ninety", Dispose),
      ]
    );
    close(rig.degrees(), 0.0);
  }

  #[test]
  fn a_crossfade_does_not_dip_toward_what_is_under_it() {
    // Both animations key the bone's rotation, so the mix is a plain blend
    // of the two, with none of the setup pose (0 degrees) in it
    let mut rig = Rig::new();
    rig.set(0, "thirty", true);
    rig.step(0.1);
    rig.set(0, "ninety", true);
    rig.step(0.1);
    close(rig.degrees(), 60.0);
    // On a layer, each entry weighs its alpha times its share of the mix:
    // at half alpha and half the mix a quarter each, over track 0's 0
    let mut rig = Rig::new();
    rig.set(0, "zero", true);
    let from = rig.set(1, "ninety", true);
    rig.entry(from).alpha = 0.5;
    rig.step(0.1);
    close(rig.degrees(), 45.0);
    let to = rig.set(1, "thirty", true);
    rig.entry(to).alpha = 0.5;
    rig.step(0.1);
    close(rig.degrees(), 0.25 * 90.0 + 0.25 * 30.0);
    // A property the incoming animation does not key fades to what is
    // under the track, while what it does key fades in from there
    let mut rig = Rig::new();
    rig.set(0, "slide", true);
    rig.step(0.1);
    close(rig.pose.bones[0].x, 10.0);
    rig.set(0, "ninety", true);
    rig.step(0.1);
    close(rig.pose.bones[0].x, 5.0);
    close(rig.degrees(), 45.0);
    // An incoming entry that adds replaces nothing, so what the outgoing
    // one keys fades the same way while the addition fades in: half of 90
    // plus half of 30
    let mut rig = Rig::new();
    rig.set(0, "ninety", true);
    rig.step(0.1);
    let add = rig.set(0, "thirty", true);
    rig.entry(add).mix_blend = MixBlend::Add;
    rig.step(0.1);
    close(rig.degrees(), 45.0 + 15.0);
    // Three in a chain, all keying the rotation: 90 to 30 a quarter done
    // is 75; then with that mix half done and a mix to 0 a quarter done,
    // the weights are 0.5 x 0.75, 0.5 x 0.75 and 0.25
    let mut rig = Rig::new();
    rig.set(0, "ninety", true);
    rig.step(0.1);
    let thirty = rig.set(0, "thirty", true);
    rig.entry(thirty).mix_duration = 0.4;
    rig.step(0.1);
    close(rig.degrees(), 75.0);
    let zero = rig.set(0, "zero", true);
    rig.entry(zero).mix_duration = 0.4;
    rig.step(0.1);
    close(rig.degrees(), 0.375 * 90.0 + 0.375 * 30.0);
    // The same chain ending in an animation that does not key the
    // rotation: the two older ones keep their shares of the 0.75 left, the
    // rest fading to the setup pose
    let mut rig = Rig::new();
    rig.set(0, "ninety", true);
    rig.step(0.1);
    let thirty = rig.set(0, "thirty", true);
    rig.entry(thirty).mix_duration = 0.4;
    rig.step(0.1);
    let slide = rig.set(0, "slide", true);
    rig.entry(slide).mix_duration = 0.4;
    rig.step(0.1);
    close(rig.degrees(), 0.375 * 90.0 + 0.375 * 30.0);
    close(rig.pose.bones[0].x, 2.5);
    // The two documented dips. A combined translate is not covered by a
    // translate x alone: its x fades from 10 to 5, and the incoming 10
    // blends half way back
    let mut rig = Rig::new();
    rig.set(0, "shift", true);
    rig.step(0.1);
    rig.set(0, "slide", true);
    rig.step(0.1);
    close(rig.pose.bones[0].x, 7.5);
    close(rig.pose.bones[0].y, 5.0);
    // And past a newer entry that does not key a property, an older one
    // fades as if nothing newer keyed it: "slide", "thirty" half mixed in,
    // then "slide" again a quarter in leave x at 10 x 0.5 x 0.75, blended
    // a quarter of the way to 10 (5.3125, where no dip would give 6.25)
    let mut rig = Rig::new();
    rig.set(0, "slide", true);
    rig.step(0.1);
    let thirty = rig.set(0, "thirty", true);
    rig.entry(thirty).mix_duration = 0.4;
    rig.step(0.1);
    let slide = rig.set(0, "slide", true);
    rig.entry(slide).mix_duration = 0.4;
    rig.step(0.1);
    close(rig.pose.bones[0].x, 3.75 + (10.0 - 3.75) * 0.25);
  }

  #[test]
  fn an_entry_s_speed_scales_its_time_but_not_its_mix() {
    let mut rig = Rig::new();
    let sweep = rig.set(0, "sweep", false);
    rig.entry(sweep).speed = 2.0;
    rig.step(0.25);
    // Half way through at double speed: the sweep from -30 to 30 degrees
    // is at 0
    close(rig.entry(sweep).track_time(), 0.5);
    close(rig.degrees(), 0.0);
    // The skeleton's time scale multiplies it
    rig.state.time_scale = 0.5;
    rig.step(0.25);
    close(rig.entry(sweep).track_time(), 0.75);
    close(rig.degrees(), 15.0);
    // A mix runs on the skeleton's time, whatever the entry's speed
    let mut rig = Rig::new();
    rig.set(0, "zero", true);
    rig.step(0.1);
    let ninety = rig.set(0, "ninety", true);
    rig.entry(ninety).speed = 4.0;
    rig.step(0.1);
    close(rig.entry(ninety).track_time(), 0.4);
    close(rig.degrees(), 45.0);
    rig.state.time_scale = 0.5;
    rig.step(0.1);
    close(rig.degrees(), 67.5);
  }

  #[test]
  fn an_entry_mixes_in_over_the_data_s_mix_for_its_pair() {
    let mut rig = Rig::new();
    rig.set(0, "slide", true);
    rig.step(0.1);
    // "slide" to "thirty" has a mix of its own; the way back takes the
    // default
    let thirty = rig.set(0, "thirty", true);
    close(rig.entry(thirty).mix_duration, 0.5);
    rig.step(0.1);
    let slide = rig.set(0, "slide", true);
    close(rig.entry(slide).mix_duration, 0.2);
    // A queued entry takes the mix from the entry queued before it
    let mut rig = Rig::new();
    rig.set(0, "slide", true);
    rig.step(0.1);
    let thirty = rig.add(0, "thirty", true, 0.0);
    let slide = rig.add(0, "slide", true, 0.0);
    close(rig.entry(thirty).mix_duration, 0.5);
    close(rig.entry(slide).mix_duration, 0.2);
  }

  #[test]
  fn an_entry_replaced_before_it_shows_is_dropped() {
    use TrackEventKind::*;
    let mut rig = Rig::new();
    rig.set(0, "zero", true);
    rig.step(0.1);
    // Two calls in one frame: the first never showed, so the second mixes
    // from the entry that did
    let unseen = rig.set(0, "half_turn", true);
    rig.set(0, "ninety", true);
    assert!(rig.state.entry(unseen).is_none());
    let fired = rig.step(0.1);
    close(rig.degrees(), 45.0);
    assert_eq!(
      fired.kinds(),
      vec![
        ("zero", Interrupt),
        ("half_turn", Start),
        ("half_turn", Interrupt),
        ("ninety", Start),
        ("half_turn", End),
        ("half_turn", Dispose),
      ]
    );
    // The empty animation stays, so an entry set in the same frame as
    // set_empty mixes in from nothing: once the data is known and before,
    // and whether or not something was queued after the empty animation
    // (which the set drops)
    for (landed, queue) in [(true, false), (true, true), (false, false), (false, true)] {
      let mut rig = Rig::new();
      if landed {
        rig.step(0.1);
      }
      rig.state.set_empty(1, 0.0);
      let queued = queue.then(|| rig.add(1, "thirty", true, 5.0));
      let ninety = rig.set(1, "ninety", true);
      assert_eq!(rig.state.current(1), Some(ninety));
      assert!(queued.is_none_or(|id| rig.state.entry(id).is_none()));
      rig.step(0.1);
      close(rig.degrees(), 45.0);
      rig.step(0.1);
      close(rig.degrees(), 90.0);
    }
  }

  #[test]
  fn an_entry_mixing_out_completes_no_more() {
    use TrackEventKind::*;
    let mut rig = Rig::new();
    rig.set(0, "short", true);
    assert_eq!(rig.step(0.4).kinds(), vec![("short", Start)]);
    let zero = rig.set(0, "zero", true);
    rig.entry(zero).mix_duration = 1.0;
    // "short" passes the end of its 0.5 s loop twice while it mixes out
    assert_eq!(
      rig.step(0.9).kinds(),
      vec![("short", Interrupt), ("zero", Start)]
    );
  }

  #[test]
  fn set_animation_and_clear_drop_what_is_queued() {
    use TrackEventKind::*;
    let mut rig = Rig::new();
    rig.set(0, "zero", true);
    let queued = rig.add(0, "thirty", true, 5.0);
    rig.step(0.1);
    // Setting an animation disposes the queue, which never started
    rig.set(0, "ninety", true);
    assert!(rig.state.entry(queued).is_none());
    assert_eq!(
      rig.step(0.1).kinds(),
      vec![("thirty", Dispose), ("zero", Interrupt), ("ninety", Start)]
    );
    // Clearing during the mix ends both entries at once, the newest first,
    // and disposes the queue
    rig.add(0, "thirty", true, 5.0);
    rig.state.clear(0);
    assert_eq!(
      rig.step(0.1).kinds(),
      vec![
        ("ninety", End),
        ("ninety", Dispose),
        ("zero", End),
        ("zero", Dispose),
        ("thirty", Dispose),
      ]
    );
    assert_eq!(rig.state.current(0), None);
    close(rig.degrees(), 0.0);
  }

  #[test]
  fn several_queued_entries_can_start_in_one_step() {
    use TrackEventKind::*;
    let mut rig = Rig::new();
    rig.set(0, "short", false);
    // "ninety" at the end of "short" (0.5 s), "thirty" 0.25 s into it
    let ninety = rig.add(0, "ninety", false, 0.0);
    let thirty = rig.add(0, "thirty", false, 0.25);
    for id in [ninety, thirty] {
      rig.entry(id).mix_duration = 0.0;
    }
    let fired = rig.step(1.0);
    assert_eq!(rig.state.current(0), Some(thirty));
    close(rig.entry(thirty).track_time(), 0.25);
    close(rig.degrees(), 30.0);
    assert_eq!(
      fired.kinds(),
      vec![
        ("short", Start),
        ("short", Complete),
        ("short", Interrupt),
        ("ninety", Start),
        ("ninety", Interrupt),
        ("thirty", Start),
        ("short", End),
        ("short", Dispose),
        ("ninety", End),
        ("ninety", Dispose),
      ]
    );
  }

  #[test]
  fn a_later_call_drops_the_calls_kept_for_its_track() {
    use TrackEventKind::*;
    let kept = "kept until the data lands";
    let mut state = SkeletonAnimation::new();
    // set_animation, over a kept animation and what was queued after it
    let first = state.set_animation(0, "ninety", true).expect(kept);
    let queued = state.add_animation(0, "zero", true, 0.0).expect(kept);
    let second = state.set_animation(0, "thirty", true).expect(kept);
    assert!(state.entry(first).is_none());
    assert!(state.entry(queued).is_none());
    assert_eq!(state.current(0), Some(second));
    // set_empty
    let faded = state.set_animation(1, "ninety", true).expect(kept);
    state.set_empty(1, 0.0);
    assert!(state.entry(faded).is_none());
    assert_eq!(state.current(1), None);
    // clear
    let cleared = state.set_animation(2, "ninety", true).expect(kept);
    state.clear(2);
    assert!(state.entry(cleared).is_none());
    assert_eq!(state.current(2), None);
    // Only what is left is made when the data lands. The empty animation
    // is zero-length, so it completes at once
    let mut rig = Rig::new();
    rig.state = state;
    let fired = rig.step(0.1);
    close(rig.degrees(), 30.0);
    assert_eq!(
      fired.kinds(),
      vec![
        ("thirty", Start),
        ("<empty>", Start),
        ("<empty>", Complete),
        ("<empty>", End),
        ("<empty>", Dispose),
      ]
    );
  }

  #[test]
  fn a_track_past_the_last_warns_once_and_does_nothing() {
    let capture = capture_log();
    let past = SkeletonAnimation::MAX_TRACKS;
    // Before the data lands, and after
    let mut rig = Rig::new();
    assert_eq!(rig.state.set_animation(past, "ninety", true), None);
    rig.step(0.1);
    assert_eq!(rig.state.set_animation(past, "ninety", true), None);
    assert_eq!(
      rig.state.add_animation(usize::MAX, "ninety", true, 0.0),
      None
    );
    rig.state.set_empty(usize::MAX, 0.2);
    rig.state.clear(usize::MAX);
    assert_eq!(rig.state.current(past), None);
    rig.step(0.1);
    close(rig.degrees(), 0.0);
    assert_eq!(capture.containing("past the last"), 1);
    // The last track plays
    assert!(rig.state.set_animation(past - 1, "ninety", true).is_some());
    rig.step(0.1);
    close(rig.degrees(), 90.0);
  }
}
