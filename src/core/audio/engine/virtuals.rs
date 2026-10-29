//! Virtual voices: loops that keep their place silently, with no kira
//! sound, while a voice limit or their bus's cap has no room for them or
//! they are out of range, and the ranking that brings them back

use super::*;

/// The most voices virtual at once: past them, a loop that would go
/// virtual is refused (`virtual`), and a loop a play takes the place of
/// stops, as a one-shot does
pub(super) const MAX_VIRTUAL: usize = 1024;
/// A virtual loop's fade in as it comes back, and the fade out of a loop
/// whose place it takes
const VIRTUAL_FADE: Duration = Duration::from_millis(50);
/// How much louder as heard a virtual loop must be than the voice whose
/// place it takes, at equal priority, in dB
const SWAP_DB: f32 = 3.0;
/// How long a voice that started, went virtual or came back stays as it
/// is before the ranking (or its distance) moves it again, so two loops of
/// about the same level do not swap back and forth
pub(super) const STAY_PUT: Duration = Duration::from_millis(250);

/// Why a voice went virtual (`AudioLogKind::Virtual`) or came back
/// (`Real`), the rule first, as a refusal's reason
pub(super) const VIRTUAL_RANGE: &str =
  "range: at or beyond its max_distance, it takes no voice until it comes back in range";
pub(super) const VIRTUAL_LIMIT: &str = "limit: a voice limit or its bus's cap of 96 had no room for it, with no voice it may take the place of";
pub(super) const VIRTUAL_FULL: &str = "full: voices fading out hold kira's room on its bus's track";
pub(super) const VIRTUAL_STOLEN: &str =
  "stolen: a play took its place under a voice limit or its bus's cap";
const VIRTUAL_DISPLACED: &str =
  "displaced: a virtual loop that outranks it (more important, or 3 dB louder) took its place";
const REAL_ROOM: &str = "room: it is in range, and its limits and its bus have room for it";
const REAL_OUTRANKS: &str =
  "outranks: it took the place of a voice less important, or 3 dB quieter as heard";
/// The refusal past [`MAX_VIRTUAL`]
pub(super) const REFUSED_VIRTUAL: &str =
  "virtual: 1024 loops are virtual already, so one with no room or out of range is refused";

/// The ranking's lists, kept to reuse: a frame with virtual voices that
/// moves none allocates nothing
pub(super) struct Ranking {
  /// The virtual loops that may come back now, by index
  waiting: Vec<usize>,
  /// The scopes they compete in, with the real voices in each
  tallies: Vec<Tally>,
  /// Those that may come back, as rank orders them (most important, then
  /// loudest, then oldest first): their index, priority and gain as heard
  fitting: Vec<(usize, u8, f32)>,
  /// The voices one comeback takes the place of
  displaced: Vec<usize>,
}

impl Ranking {
  /// Lists with room for a crowd of virtual loops, so the first frames that
  /// rank them grow nothing
  pub(super) fn with_room() -> Self {
    Self {
      waiting: Vec::with_capacity(64),
      tallies: Vec::with_capacity(16),
      fitting: Vec::with_capacity(16),
      displaced: Vec::with_capacity(8),
    }
  }

  /// Lists that hold nothing, and allocate nothing: what stands in the
  /// engine while the ranking has its own out
  fn empty() -> Self {
    Self {
      waiting: Vec::new(),
      tallies: Vec::new(),
      fitting: Vec::new(),
      displaced: Vec::new(),
    }
  }
}

/// A scope virtual loops compete in: the real voices it counts, and the
/// weakest that may give up its place
struct Tally {
  scope: Scope,
  count: usize,
  /// The least important real voice in it, then the quietest as heard, then
  /// the oldest, of those that may be moved (not music, and as they are for
  /// [`STAY_PUT`]): its index, priority and gain as heard
  weakest: Option<(usize, u8, f32)>,
}

impl Ranking {
  /// Count the real voices of `live` in each tally's scope, and find each
  /// one's weakest that may be moved at `clock`
  fn count(&mut self, live: &[LiveVoice], buses: &BusTree, clock: Duration) {
    for tally in &mut self.tallies {
      tally.count = 0;
      tally.weakest = None;
    }
    for (index, voice) in live.iter().enumerate() {
      if voice.is_virtual()
        || !matches!(
          voice.voice.state(),
          VoiceState::Playing | VoiceState::Paused
        )
      {
        continue;
      }
      let (priority, heard) = (voice.priority, voice.heard_db());
      let movable = voice.music.is_none() && clock.saturating_sub(voice.changed) >= STAY_PUT;
      for tally in &mut self.tallies {
        if !tally.scope.holds(voice, buses) {
          continue;
        }
        tally.count += 1;
        let weaker = tally.weakest.is_none_or(|(_, least, quietest)| {
          priority < least || (priority == least && heard < quietest)
        });
        if movable && weaker {
          tally.weakest = Some((index, priority, heard));
        }
      }
    }
  }

  /// Whether virtual voice `live` of `voices` may come back now: in each
  /// scope it competes in there is room, or a voice it outranks may give up
  /// its place (planned into `displaced`), and kira has room on its bus's
  /// track. `PreventNew` takes no voice's place
  fn fits(&mut self, live: &LiveVoice, voices: &[LiveVoice], buses: &BusTree) -> bool {
    let (tallies, displaced) = (&self.tallies, &mut self.displaced);
    displaced.clear();
    let (priority, heard) = (live.priority, live.heard_db());
    let fits = each_scope(live, buses, |scope, max, rule| {
      let Some(tally) = tallies.iter().find(|tally| tally.scope == scope) else {
        return false;
      };
      let taken = displaced
        .iter()
        .filter(|&&index| scope.holds(&voices[index], buses))
        .count();
      let count = tally.count.saturating_sub(taken);
      if count < max {
        return true;
      }
      // One voice's place makes room in a full scope, no more
      let Some((index, least, quietest)) = tally.weakest else {
        return false;
      };
      let outranks = priority > least || (priority == least && heard >= quietest + SWAP_DB);
      if rule == Some(LimitRule::PreventNew)
        || count > max
        || displaced.contains(&index)
        || !outranks
      {
        return false;
      }
      displaced.push(index);
      true
    });
    fits && buses.has_room(live.bus)
  }
}

/// Call `f` with each scope `live` competes in, as a play of it is checked
/// (`make_room`): its definition's limit, each bus limit from its bus up,
/// then the cap on its bus's track, each with its most voices and the rule
/// that picks what a play past it does. False as soon as `f` is
fn each_scope(
  live: &LiveVoice,
  buses: &BusTree,
  mut f: impl FnMut(Scope, usize, Option<LimitRule>) -> bool,
) -> bool {
  let rule = live.limit.map(|limit| limit.rule);
  if let (Some(id), Some(limit)) = (live.sound, live.limit)
    && !f(Scope::Sound(id, live.bus), usize::from(limit.max), rule)
  {
    return false;
  }
  let mut at = Some(live.bus);
  while let Some(index) = at {
    let Some(bus) = buses.buses.get(index) else {
      break;
    };
    if let Some(max) = bus.limit
      && !f(Scope::Subtree(index), usize::from(max), rule)
    {
      return false;
    }
    at = bus.parent;
  }
  f(Scope::Track(live.bus), VOICE_CAP, None)
}

impl LiveVoice {
  /// Whether, virtual, it may come back at `clock`: playing (not paused by
  /// the game, nor held), in range, and virtual for [`STAY_PUT`]
  fn may_come_back(&self, clock: Duration) -> bool {
    self.is_virtual()
      && self.voice.state() == VoiceState::Playing
      && !self.held
      && clock.saturating_sub(self.changed) >= STAY_PUT
      && self.spatial.as_ref().is_none_or(|spatial| spatial.near)
  }

  /// The part of what it plays that loops, in seconds from its start: its
  /// loop region kept inside what plays, or all of it
  fn loop_span(&self) -> (f64, f64) {
    let length = self.length.max(0.0);
    match &self.loop_region {
      Some(looped) if looped.end.min(length) > looped.start.max(0.0) => {
        (looped.start.max(0.0), looped.end.min(length))
      }
      _ => (0.0, length),
    }
  }
}

impl EngineState {
  /// The virtual loops on `bus` and every bus under it (0 for no such bus)
  pub(crate) fn virtual_on(&self, bus: &str) -> usize {
    let Some(bus) = self.buses.find(bus) else {
      return 0;
    };
    self
      .voices
      .live
      .iter()
      .filter(|live| live.is_virtual() && self.buses.within(live.bus, bus))
      .count()
  }

  /// Live voice `index`, a loop, goes virtual for `reason`: its kira sound
  /// fades out over `fade` (and its distance low-pass's track with it), and
  /// it keeps its place, on the live tree, with every change the game makes
  pub(super) fn virtualize(
    &mut self,
    index: usize,
    fade: Duration,
    reason: &'static str,
    server: Option<&AssetServer>,
  ) {
    let (clock, generation) = (self.clock, self.generation);
    let Some(live) = self.voices.live.get_mut(index) else {
      return;
    };
    let place = Playback::Virtual(Virtual {
      position: live.playback.position(),
      stopped: false,
    });
    let mut old = std::mem::replace(&mut live.playback, place);
    old.stop(fade);
    // Its filter's track plays the old sound's fade out, then goes
    if let Some(spatial) = &mut live.spatial {
      spatial.filter = None;
    }
    live.fade_in = None;
    live.unheard = None;
    live.generation = generation;
    live.changed = clock;
    live.voice.set_virtual(true);
    self.virtual_count += 1;
    self.record_why(index, AudioLogKind::Virtual, fade, reason, server);
  }

  /// Virtual voice `index` comes back for `reason`: its kira sound starts
  /// where it has got to, at its level as heard now (the game's changes and
  /// its place's), fading in over 50 ms, with its distance low-pass while
  /// there is room for one. False if it could not start (kira's room taken
  /// in a race): it stays virtual. One whose clip cannot be decoded ends
  fn realize(
    &mut self,
    world: &World,
    index: usize,
    reason: &'static str,
    server: Option<&AssetServer>,
  ) -> bool {
    let live = &self.voices.live[index];
    let Playback::Virtual(place) = live.playback else {
      return false;
    };
    let Some((samples, version)) = live.source.as_ref().samples(world) else {
      return false;
    };
    let undecodable = live
      .source
      .as_ref()
      .clip()
      .is_some_and(|clip| self.undecodable.get(&clip.id()) == Some(&version));
    if undecodable {
      self.end_virtual(index, server);
      return false;
    }
    let bus = live.bus;
    let cutoff = live
      .spatial
      .as_ref()
      .zip(live.position)
      .and_then(|(spatial, position)| {
        Hearing::of(&spatial.settings, position, self.listener.as_ref()).cutoff
      });
    let (region, loop_region) = (live.region.clone(), live.loop_region.clone());
    let start = Start {
      gain_db: live.heard_db(),
      pan: live.pan(),
      rate: live.rate(),
      held: false,
      fade_in: Some(fade_from_silence(VIRTUAL_FADE)),
      region: region.as_ref(),
      loop_region: loop_region.as_ref(),
      looping: true,
      position: place.position,
    };
    let mut filter = cutoff.and_then(|hz| self.voice_filter(bus, hz));
    let track = filter.as_mut().map(|filter| &mut filter.track);
    let started = match samples {
      ClipSource::Decoded(data) => self.start_static(bus, track, data, &start),
      ClipSource::Encoded { clip, rate } => self.start_stream(bus, track, clip, rate, &start),
    };
    let playback = match started {
      Ok(playback) => playback,
      Err(Refusal::Full) => {
        self.count_kira_full();
        return false;
      }
      Err(Refusal::Undecodable(error)) => {
        let source = &self.voices.live[index].source;
        if let Source::Clip(clip) = source {
          let clip = clip.clone();
          self.warn_undecodable(&clip, version, &error, server);
        }
        self.end_virtual(index, server);
        return false;
      }
    };
    let (clock, generation) = (self.clock, self.generation);
    let live = &mut self.voices.live[index];
    live.playback = playback;
    live.kira_paused = false;
    live.own_fade = None;
    if let Some(spatial) = &mut live.spatial {
      spatial.filter = filter;
    }
    live.fade_in = FadeIn::begin(clock, VIRTUAL_FADE);
    live.generation = generation;
    live.version = version;
    live.changed = clock;
    live.voice.set_virtual(false);
    self.virtual_count = self.virtual_count.saturating_sub(1);
    self.record_why(index, AudioLogKind::Real, VIRTUAL_FADE, reason, server);
    true
  }

  /// Virtual voice `index` can never come back (its clip cannot be
  /// decoded): it ends, logged as stopped
  fn end_virtual(&mut self, index: usize, server: Option<&AssetServer>) {
    let live = &mut self.voices.live[index];
    live.playback.stop(MIN_FADE);
    live.voice.set_state(VoiceState::Stopping);
    self.uncount(index);
    self.record(index, AudioLogKind::Stop, MIN_FADE, server);
  }

  /// Live voice `index` gives up its place to a virtual loop that outranks
  /// it: a loop goes virtual, fading out over 50 ms as the other fades in,
  /// and a one-shot stops with its fade (its `fade_out`, at least
  /// [`MIN_FADE`])
  fn displace(&mut self, index: usize, server: Option<&AssetServer>) {
    let Some(live) = self.voices.live.get_mut(index) else {
      return;
    };
    if live.looping && live.music.is_none() && self.virtual_count < MAX_VIRTUAL {
      self.virtualize(index, VIRTUAL_FADE, VIRTUAL_DISPLACED, server);
      return;
    }
    let fade = live.fade_out.max(MIN_FADE);
    live.playback.stop(fade);
    live.voice.set_state(VoiceState::Stopping);
    self.uncount(index);
    self.record(index, AudioLogKind::Stolen, fade, server);
  }

  /// Each frame with a virtual voice (but before the web's unlock): bring
  /// back each virtual loop that may come back ([`LiveVoice::may_come_back`])
  /// and that, in each scope it competes in (its definition's limit, the
  /// bus limits above it, its bus's cap), finds room or outranks the
  /// weakest real voice that may be moved (more important, or as important
  /// and 3 dB louder as heard), most important and loudest first; the voice
  /// it outranks gives up its place ([`displace`](Self::displace)). Nothing
  /// is logged but the voices that move
  pub(super) fn rank_virtual(&mut self, world: &World, server: Option<&AssetServer>) {
    if self.virtual_count == 0 || self.is_locked() {
      return;
    }
    #[cfg(test)]
    {
      self.ranked += 1;
    }
    let clock = self.clock;
    let mut ranking = std::mem::replace(&mut self.ranking, Ranking::empty());
    ranking.waiting.clear();
    ranking.tallies.clear();
    ranking.fitting.clear();
    for (index, live) in self.voices.live.iter().enumerate() {
      if !live.may_come_back(clock) {
        continue;
      }
      ranking.waiting.push(index);
      let tallies = &mut ranking.tallies;
      each_scope(live, &self.buses, |scope, _, _| {
        if !tallies.iter().any(|tally| tally.scope == scope) {
          tallies.push(Tally {
            scope,
            count: 0,
            weakest: None,
          });
        }
        true
      });
    }
    if !ranking.waiting.is_empty() {
      ranking.count(&self.voices.live, &self.buses, clock);
      for at in 0..ranking.waiting.len() {
        let index = ranking.waiting[at];
        let live = &self.voices.live[index];
        if ranking.fits(live, &self.voices.live, &self.buses) {
          ranking
            .fitting
            .push((index, live.priority, live.heard_db()));
        }
      }
    }
    ranking
      .fitting
      .sort_unstable_by(|a, b| b.1.cmp(&a.1).then(b.2.total_cmp(&a.2)).then(a.0.cmp(&b.0)));
    for at in 0..ranking.fitting.len() {
      let (index, _, _) = ranking.fitting[at];
      // A comeback before it may have taken the room it found
      if !ranking.fits(&self.voices.live[index], &self.voices.live, &self.buses) {
        continue;
      }
      let reason = if ranking.displaced.is_empty() {
        REAL_ROOM
      } else {
        REAL_OUTRANKS
      };
      if !self.realize(world, index, reason, server) {
        continue;
      }
      for taken in 0..ranking.displaced.len() {
        let victim = ranking.displaced[taken];
        self.displace(victim, server);
      }
      ranking.count(&self.voices.live, &self.buses, clock);
    }
    self.ranking = ranking;
  }

  /// Move each virtual loop on through what it plays by `delta` at its rate
  /// (its pitch, and the time scale its bus follows), wrapping in its loop,
  /// as kira would have played it, but one paused or held
  pub(super) fn advance_virtual(&mut self, delta: Duration) {
    if self.virtual_count == 0 {
      return;
    }
    let seconds = delta.as_secs_f64();
    for live in &mut self.voices.live {
      if !live.is_virtual() || live.held || live.voice.state() != VoiceState::Playing {
        continue;
      }
      let rate = live.rate();
      let (start, end) = live.loop_span();
      if let Playback::Virtual(place) = &mut live.playback {
        let mut at = place.position + seconds * rate;
        if at >= end && end > start {
          at = start + (at - start) % (end - start);
        }
        place.position = at;
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use std::time::Duration;

  use super::*;
  use crate::core::audio::{AudioListener, AudioView, BusDef, Falloff};
  use crate::core::ecs::Transform;
  use crate::core::testing::{audio_step, audio_world, metered_audio_world};
  use crate::core::time::Time;

  const FRAME: Duration = Duration::from_nanos(16_666_667);
  /// A comeback's fade-in, and a displaced loop's fade out
  const FIFTY_MS: Duration = Duration::from_millis(50);

  /// A clip `seconds` long at 48 kHz, every frame 0.1 on both sides
  fn clip(world: &mut World, seconds: f32) -> Handle<AudioClip> {
    let frames = vec![[0.1, 0.1]; (48_000.0 * seconds) as usize];
    world
      .resource_mut::<Assets<AudioClip>>()
      .add(AudioClip::from_frames(48_000, &frames).unwrap())
  }

  fn add(world: &mut World, sound: Sound) -> Handle<Sound> {
    world.resource_mut::<Assets<Sound>>().add(sound)
  }

  /// A loop of `clip`, as many starting a frame as are played
  fn looping(clip: &Handle<AudioClip>) -> Sound {
    Sound {
      clips: vec![SoundClip::new(clip.clone())],
      looping: true,
      per_frame: u8::MAX,
      ..Default::default()
    }
  }

  /// A loop of `clip` limited to `max` voices by `rule`, fading out over
  /// 30 ms when stopped
  fn limited(
    world: &mut World,
    clip: &Handle<AudioClip>,
    max: u16,
    rule: LimitRule,
  ) -> Handle<Sound> {
    let sound = Sound {
      limit: Some(VoiceLimit { max, rule }),
      fade_out: Duration::from_millis(30),
      ..looping(clip)
    };
    add(world, sound)
  }

  /// Heard from where it plays: full within 100, silent from 500, hardly
  /// panned
  fn placed(clip: &Handle<AudioClip>) -> Sound {
    Sound {
      spatial: Some(Spatial {
        min_distance: 100.0,
        max_distance: 500.0,
        curve: Falloff::Linear,
        pan_width: 1.0e6,
        low_pass_at_max: None,
      }),
      ..looping(clip)
    }
  }

  fn play(world: &mut World, sound: &Handle<Sound>) -> Voice {
    AudioView::new(world).play(sound)
  }

  fn steps(world: &mut World, frames: usize) {
    for _ in 0..frames {
      audio_step(world, FRAME);
    }
  }

  fn log(world: &World) -> Vec<AudioLogEntry> {
    world.resource::<AudioEngine>().log().to_vec()
  }

  /// `voice`'s log entries of `kind`
  fn entries(world: &World, voice: &Voice, kind: AudioLogKind) -> Vec<AudioLogEntry> {
    log(world)
      .into_iter()
      .filter(|entry| entry.voice == voice.id() && entry.kind == kind)
      .collect()
  }

  /// Each voice that went virtual or came back, in order
  fn swaps(world: &World) -> Vec<(AudioLogKind, u32)> {
    log(world)
      .into_iter()
      .filter(|entry| matches!(entry.kind, AudioLogKind::Virtual | AudioLogKind::Real))
      .map(|entry| (entry.kind, entry.voice))
      .collect()
  }

  /// Where `voice` is in what it plays, in seconds
  fn position(world: &World, voice: &Voice) -> f64 {
    match world.resource::<AudioEngine>().voice_position(voice.id()) {
      Some(at) => at,
      None => panic!("{voice:?} is gone"),
    }
  }

  fn sounds_on(world: &World, bus: &str) -> usize {
    world.resource::<AudioEngine>().sounds_on(bus)
  }

  /// Put the listener (or any entity) at (`x`, 0)
  fn move_to(world: &mut World, entity: Entity, x: f32) {
    world.entity_mut(entity).insert(Transform::from_xy(x, 0.0));
  }

  /// A loop a limit steals keeps its place: it goes virtual (logged with
  /// why, not as stolen), moves on through its clip unheard, and once the
  /// limit frees comes back where it would be had it played on, fading in
  /// over 50 ms
  #[test]
  fn a_loop_stolen_by_a_limit_comes_back_where_it_would_be() {
    let mut world = audio_world();
    let half = clip(&mut world, 0.5);
    let hum = limited(&mut world, &half, 1, LimitRule::StopOldest);
    let first = play(&mut world, &hum);
    steps(&mut world, 30);
    let second = play(&mut world, &hum);
    assert!(first.is_playing() && first.is_virtual(), "{first:?}");
    assert!(second.is_playing() && !second.is_virtual(), "{second:?}");
    let went = entries(&world, &first, AudioLogKind::Virtual);
    assert_eq!(went.len(), 1, "{went:#?}");
    assert!(went[0].reason.starts_with("stolen"), "{}", went[0].reason);
    assert_eq!(went[0].fade, Duration::from_millis(30));
    assert!(entries(&world, &first, AudioLogKind::Stolen).is_empty());
    // Its kira sound fades out: only the second's is left
    steps(&mut world, 3);
    assert_eq!(sounds_on(&world, "sfx"), 1);
    let mut audio = AudioView::new(&mut world);
    assert_eq!(
      (audio.voices_on("sfx"), audio.virtual_voices_on("sfx")),
      (1, 1)
    );
    steps(&mut world, 39);
    // 72 frames in: the limit frees, and it comes back in the next update
    second.stop(MIN_FADE);
    audio_step(&mut world, FRAME);
    assert!(first.is_playing() && !first.is_virtual(), "{first:?}");
    let back = entries(&world, &first, AudioLogKind::Real);
    assert_eq!(back.len(), 1, "{back:#?}");
    assert!(back[0].reason.starts_with("room"), "{}", back[0].reason);
    assert_eq!(back[0].fade, FIFTY_MS);
    // Where it would be had it played on: 74 frames into a 0.5 s loop
    audio_step(&mut world, FRAME);
    let expected = (74.0 * FRAME.as_secs_f64()) % 0.5;
    let at = position(&world, &first);
    assert!(
      (at - expected).abs() < FRAME.as_secs_f64(),
      "at {at}, expected {expected}"
    );
    assert_eq!(sounds_on(&world, "sfx"), 1);
  }

  /// A placed loop at or beyond its maximum distance takes no voice: it is
  /// virtual, so kira holds no sound for it and nothing renders. Past the
  /// maximum but short of 95% of it, it stays virtual; inside that it comes
  /// back where it has got to, fading in over 50 ms to its place's gain, as
  /// rendered. Out of range again, it goes virtual again
  #[test]
  fn a_loop_beyond_its_range_takes_no_voice_and_fades_in_as_the_listener_nears() {
    let (mut world, meter) = metered_audio_world();
    let listener = world
      .spawn((AudioListener, Transform::from_xy(0.0, 0.0)))
      .id();
    let dc = clip(&mut world, 1.0);
    let hum = add(&mut world, placed(&dc));
    let voice = AudioView::new(&mut world).play_at(&hum, Vec2::new(900.0, 0.0));
    assert!(voice.is_playing() && voice.is_virtual(), "{voice:?}");
    let went = entries(&world, &voice, AudioLogKind::Virtual);
    assert_eq!(went.len(), 1, "{went:#?}");
    assert!(went[0].reason.starts_with("range"), "{}", went[0].reason);
    assert!(entries(&world, &voice, AudioLogKind::Start).is_empty());
    steps(&mut world, 20);
    assert_eq!(sounds_on(&world, "sfx"), 0);
    let silent = meter.rendered(800);
    assert!(
      silent
        .iter()
        .all(|frame| frame.left == 0.0 && frame.right == 0.0),
      "nothing renders"
    );
    // 480 away: in range, but not inside 95% of it
    move_to(&mut world, listener, 420.0);
    steps(&mut world, 20);
    assert!(voice.is_virtual(), "{voice:?}");
    assert_eq!(sounds_on(&world, "sfx"), 0);
    // 400 away: back, fading in to a quarter of the amplitude (Linear)
    move_to(&mut world, listener, 500.0);
    let from = meter.rendered_count();
    audio_step(&mut world, FRAME);
    assert!(voice.is_playing() && !voice.is_virtual(), "{voice:?}");
    let back = entries(&world, &voice, AudioLogKind::Real);
    assert_eq!(back.len(), 1, "{back:#?}");
    assert!(back[0].reason.starts_with("room"), "{}", back[0].reason);
    assert_eq!(back[0].fade, FIFTY_MS);
    assert_eq!(sounds_on(&world, "sfx"), 1);
    steps(&mut world, 4);
    let heard = meter.rendered(meter.rendered_count() - from);
    let level = 0.1 * 0.25;
    let loudest = |frames: &[kira::Frame]| {
      frames
        .iter()
        .map(|frame| frame.left.abs())
        .fold(0.0, f32::max)
    };
    let first_chunk = loudest(&heard[..128]);
    assert!(first_chunk < 0.2 * level, "it fades in: {first_chunk}");
    let halfway = heard[1200].left;
    assert!(
      halfway > 0.3 * level && halfway < 0.9 * level,
      "rising at 25 ms: {halfway} of {level}"
    );
    for frame in &heard[2700..] {
      assert!(
        (frame.left - level).abs() < 0.01 * level,
        "{} of {level}",
        frame.left
      );
    }
    // Out of range again (once it has stayed 250 ms), it goes virtual
    move_to(&mut world, listener, 1500.0);
    steps(&mut world, 20);
    assert!(voice.is_playing() && voice.is_virtual(), "{voice:?}");
    assert_eq!(sounds_on(&world, "sfx"), 0);
    let swaps = swaps(&world);
    let kinds: Vec<AudioLogKind> = swaps.iter().map(|&(kind, _)| kind).collect();
    assert_eq!(
      kinds,
      [
        AudioLogKind::Virtual,
        AudioLogKind::Real,
        AudioLogKind::Virtual
      ]
    );
  }

  /// Hysteresis: a virtual loop takes a heard one's place only 3 dB louder
  /// (at equal priority), and a voice that went virtual or came back stays
  /// so for 250 ms however loud the other gets, so two loops of about the
  /// same level do not swap back and forth
  #[test]
  fn two_loops_a_decibel_apart_do_not_swap_back_and_forth() {
    let mut world = audio_world();
    let long = clip(&mut world, 2.0);
    let hum = limited(&mut world, &long, 1, LimitRule::StopOldest);
    let a = play(&mut world, &hum);
    audio_step(&mut world, FRAME);
    let b = play(&mut world, &hum);
    assert!(a.is_virtual() && !b.is_virtual());
    let settled = swaps(&world).len();
    // The virtual one a decibel louder, then the heard one: neither swaps
    a.set_volume(1.0);
    steps(&mut world, 60);
    b.set_volume(2.0);
    steps(&mut world, 60);
    assert_eq!(swaps(&world).len(), settled, "{:#?}", swaps(&world));
    // 3 dB louder than the heard one, the virtual one takes its place
    a.set_volume(5.0);
    audio_step(&mut world, FRAME);
    assert_eq!(
      swaps(&world)[settled..],
      [
        (AudioLogKind::Real, a.id()),
        (AudioLogKind::Virtual, b.id())
      ]
    );
    let outranked = entries(&world, &a, AudioLogKind::Real);
    assert!(
      outranked[0].reason.starts_with("outranks"),
      "{outranked:#?}"
    );
    let displaced = entries(&world, &b, AudioLogKind::Virtual);
    let displaced = &displaced[displaced.len() - 1];
    assert!(displaced.reason.starts_with("displaced"), "{displaced:#?}");
    assert_eq!(displaced.fade, FIFTY_MS);
    // Each stays put for 250 ms, however loud the other gets
    b.set_volume(20.0);
    steps(&mut world, 14);
    assert!(!a.is_virtual() && b.is_virtual(), "{a:?} {b:?}");
    assert_eq!(swaps(&world).len(), settled + 2);
    steps(&mut world, 1);
    assert!(a.is_virtual() && !b.is_virtual(), "{a:?} {b:?}");
    assert_eq!(swaps(&world).len(), settled + 4);
  }

  /// A virtual loop takes the game's changes: its volume and pitch as it
  /// comes back, its pause keeping it virtual until it resumes, and its
  /// stop ending it without coming back
  #[test]
  fn a_virtual_loop_takes_the_game_s_changes() {
    let mut world = audio_world();
    let long = clip(&mut world, 2.0);
    let hum = limited(&mut world, &long, 1, LimitRule::StopOldest);
    let a = play(&mut world, &hum);
    audio_step(&mut world, FRAME);
    let b = play(&mut world, &hum);
    a.set_volume(-12.0);
    a.set_pitch(12.0);
    steps(&mut world, 20);
    assert!(a.is_virtual());
    b.stop(MIN_FADE);
    audio_step(&mut world, FRAME);
    let back = entries(&world, &a, AudioLogKind::Real);
    assert_eq!(back.len(), 1, "{back:#?}");
    assert_eq!(back[0].gain_db, -12.0);
    assert!((back[0].rate - 2.0).abs() < 1e-9, "{}", back[0].rate);

    // Paused, it stays virtual with room free, until it resumes
    audio_step(&mut world, FRAME);
    let c = play(&mut world, &hum);
    assert!(a.is_virtual());
    a.pause();
    audio_step(&mut world, FRAME);
    assert!(!a.is_playing() && a.is_virtual(), "{a:?}");
    c.stop(MIN_FADE);
    steps(&mut world, 30);
    assert!(a.is_virtual());
    assert_eq!(entries(&world, &a, AudioLogKind::Real).len(), 1);
    a.resume();
    audio_step(&mut world, FRAME);
    assert!(a.is_playing() && !a.is_virtual(), "{a:?}");

    // Stopped, it ends, and never comes back
    audio_step(&mut world, FRAME);
    let d = play(&mut world, &hum);
    assert!(a.is_virtual());
    a.stop(Duration::from_millis(100));
    audio_step(&mut world, FRAME);
    assert!(a.is_stopped(), "{a:?}");
    let stop = entries(&world, &a, AudioLogKind::Stop);
    assert_eq!(stop.len(), 1, "{stop:#?}");
    assert_eq!(stop[0].fade, Duration::from_millis(100));
    d.stop(MIN_FADE);
    steps(&mut world, 30);
    assert_eq!(entries(&world, &a, AudioLogKind::Real).len(), 2);
    assert_eq!(AudioView::new(&mut world).virtual_voices_on("sfx"), 0);
  }

  /// The game's pause and its scene's hold hold a virtual loop as they do a
  /// heard one: it does not move on through its clip, and does not come
  /// back with room free, until let go
  #[test]
  fn a_virtual_loop_is_held_by_the_pause_and_its_scene() {
    for by_scene in [false, true] {
      let mut world = audio_world();
      let long = clip(&mut world, 2.0);
      let hum = limited(&mut world, &long, 1, LimitRule::StopOldest);
      world.resource_mut::<AudioEngine>().set_scope(Some(7));
      let a = play(&mut world, &hum);
      audio_step(&mut world, FRAME);
      let b = play(&mut world, &hum);
      world.resource_mut::<AudioEngine>().set_scope(None);
      steps(&mut world, 10);
      assert!(a.is_virtual());
      let before = position(&world, &a);
      assert!(before > 0.15, "it moves on: {before}");
      if by_scene {
        world
          .resource_mut::<AudioEngine>()
          .hold_scopes([7].into_iter());
      } else {
        world.resource_mut::<Time>().game.pause();
      }
      steps(&mut world, 30);
      assert_eq!(
        position(&world, &a),
        before,
        "held (by its scene: {by_scene})"
      );
      b.stop(MIN_FADE);
      steps(&mut world, 30);
      assert!(a.is_playing() && a.is_virtual(), "{a:?}");
      assert_eq!(position(&world, &a), before);
      if by_scene {
        world
          .resource_mut::<AudioEngine>()
          .hold_scopes(std::iter::empty());
      } else {
        world.resource_mut::<Time>().game.resume();
      }
      audio_step(&mut world, FRAME);
      assert!(
        !a.is_virtual(),
        "let go, it comes back (by its scene: {by_scene})"
      );
      audio_step(&mut world, FRAME);
      let at = position(&world, &a);
      let expected = before + 2.0 * FRAME.as_secs_f64();
      assert!(
        (at - expected).abs() < 0.005,
        "at {at}, expected {expected}"
      );
    }
  }

  /// A virtual loop belongs to its owner as a heard one does: its scene's
  /// exit ends it (a persistent one plays on, and comes back), and so does
  /// its emitter's despawn
  #[test]
  fn a_virtual_loop_ends_with_its_owner() {
    let mut world = audio_world();
    world.spawn(AudioListener);
    let long = clip(&mut world, 2.0);
    let hum = limited(&mut world, &long, 1, LimitRule::StopOldest);
    world.resource_mut::<AudioEngine>().set_scope(Some(3));
    let heard = play(&mut world, &hum);
    // Played in the same frame, the limit has no room for them
    let scenes = play(&mut world, &hum);
    let persistent = AudioView::new(&mut world).play_with(
      &hum,
      PlayOptions {
        persist: true,
        ..Default::default()
      },
    );
    world.resource_mut::<AudioEngine>().set_scope(None);
    assert!(scenes.is_virtual() && persistent.is_virtual());
    let went = entries(&world, &scenes, AudioLogKind::Virtual);
    assert!(went[0].reason.starts_with("limit"), "{went:#?}");
    crate::core::audio::release_scene(&mut world, 3);
    audio_step(&mut world, FRAME);
    assert!(scenes.is_stopped(), "{scenes:?}");
    assert_eq!(entries(&world, &scenes, AudioLogKind::Stop).len(), 1);
    assert!(!heard.is_playing());
    assert!(persistent.is_playing() && persistent.is_virtual());
    steps(&mut world, 20);
    assert!(persistent.is_playing() && !persistent.is_virtual());

    // An emitter's: its despawn ends it
    let dc = clip(&mut world, 1.0);
    let hum = add(&mut world, placed(&dc));
    let emitter = AudioEmitter {
      sound: hum,
      play_on_spawn: false,
    };
    let entity = world.spawn((emitter, Transform::from_xy(900.0, 0.0))).id();
    let far = AudioView::new(&mut world).play_emitter(entity);
    assert!(far.is_virtual(), "{far:?}");
    audio_step(&mut world, FRAME);
    world.despawn(entity);
    audio_step(&mut world, FRAME);
    assert!(far.is_stopped(), "{far:?}");
    assert_eq!(AudioView::new(&mut world).virtual_voices_on("master"), 0);
  }

  /// One-shots are never virtual: a stolen one stops, one a limit refuses
  /// is refused, and one out of range is culled
  #[test]
  fn a_one_shot_is_never_virtual() {
    let mut world = audio_world();
    world.spawn(AudioListener);
    let long = clip(&mut world, 2.0);
    let shot = |rule| Sound {
      looping: false,
      limit: Some(VoiceLimit { max: 1, rule }),
      ..looping(&long)
    };
    let oldest = add(&mut world, shot(LimitRule::StopOldest));
    let first = play(&mut world, &oldest);
    audio_step(&mut world, FRAME);
    let second = play(&mut world, &oldest);
    assert!(second.is_playing());
    assert!(!first.is_virtual(), "{first:?}");
    assert_eq!(entries(&world, &first, AudioLogKind::Stolen).len(), 1);
    steps(&mut world, 3);
    assert!(first.is_stopped(), "{first:?}");
    let prevent = add(&mut world, shot(LimitRule::PreventNew));
    play(&mut world, &prevent);
    let refused = play(&mut world, &prevent);
    assert!(refused.is_stopped() && !refused.is_virtual());
    let far = add(
      &mut world,
      Sound {
        looping: false,
        ..placed(&long)
      },
    );
    let culled = AudioView::new(&mut world).play_at(&far, Vec2::new(900.0, 0.0));
    assert!(culled.is_stopped() && !culled.is_virtual());
    steps(&mut world, 30);
    assert!(swaps(&world).is_empty(), "{:#?}", swaps(&world));
  }

  /// Music is never virtual: one a limit has no room for is refused, and a
  /// loop it takes the place of goes virtual but never takes it back
  #[test]
  fn music_is_never_virtual() {
    let mut world = audio_world();
    let long = clip(&mut world, 2.0);
    world
      .resource_mut::<AudioEngine>()
      .set_bus_limit("music", Some(1));
    // A loop on the music bus as important as music, started this frame,
    // leaves the music no room
    let bed = add(
      &mut world,
      Sound {
        bus: "music".to_string(),
        priority: 255,
        ..looping(&long)
      },
    );
    let bed = play(&mut world, &bed);
    let track = world
      .resource_mut::<Assets<Music>>()
      .add(Music::looping(long.clone(), 0.0, None));
    AudioView::new(&mut world).play_music(&track);
    assert!(AudioView::new(&mut world).current_music().is_none());
    let refused = log(&world)
      .into_iter()
      .filter(|entry| entry.kind == AudioLogKind::Refused)
      .collect::<Vec<_>>();
    assert_eq!(refused.len(), 1, "{refused:#?}");
    assert!(refused[0].reason.starts_with("limit"), "{refused:#?}");
    assert!(swaps(&world).is_empty());
    // A frame on, the music takes the bed's place: the bed goes virtual
    audio_step(&mut world, FRAME);
    AudioView::new(&mut world).play_music(&track);
    assert!(AudioView::new(&mut world).current_music().is_some());
    assert!(bed.is_playing() && bed.is_virtual(), "{bed:?}");
    // The bed, as important and far louder, never takes it back
    bed.set_volume(24.0);
    steps(&mut world, 60);
    assert!(bed.is_virtual());
    assert_eq!(swaps(&world), [(AudioLogKind::Virtual, bed.id())]);
  }

  /// At most 1024 voices are virtual: past them a play that would go
  /// virtual is refused (`virtual`), and a voice a play takes the place of
  /// stops, as one-shots do
  #[test]
  fn past_1024_virtual_loops_a_play_is_refused_and_a_steal_stops() {
    let mut world = audio_world();
    let long = clip(&mut world, 2.0);
    let of = |priority| Sound {
      priority,
      ..looping(&long)
    };
    let (important, minor, urgent) = (of(200), of(100), of(250));
    let important = add(&mut world, important);
    let minor = add(&mut world, minor);
    let urgent = add(&mut world, urgent);
    for _ in 0..96 {
      play(&mut world, &important);
    }
    let mut minors = Vec::new();
    for played in 1..=1025 {
      minors.push(play(&mut world, &minor));
      if played % 255 == 0 {
        audio_step(&mut world, FRAME);
      }
    }
    let kept = minors[..1024]
      .iter()
      .filter(|voice| voice.is_playing() && voice.is_virtual())
      .count();
    assert_eq!(kept, 1024);
    assert!(minors[1024].is_stopped(), "{:?}", minors[1024]);
    let refused = log(&world)
      .into_iter()
      .filter(|entry| entry.kind == AudioLogKind::Refused)
      .collect::<Vec<_>>();
    assert_eq!(refused.len(), 1);
    assert!(refused[0].reason.starts_with("virtual"), "{refused:#?}");
    let mut audio = AudioView::new(&mut world);
    assert_eq!(
      (audio.voices_on("sfx"), audio.virtual_voices_on("sfx")),
      (96, 1024)
    );
    audio_step(&mut world, FRAME);
    let taken = play(&mut world, &urgent);
    assert!(taken.is_playing() && !taken.is_virtual());
    let stolen = log(&world)
      .into_iter()
      .filter(|entry| entry.kind == AudioLogKind::Stolen)
      .count();
    assert_eq!(stolen, 1);
    assert_eq!(AudioView::new(&mut world).virtual_voices_on("sfx"), 1024);
  }

  /// Loops past the cap of 96 go virtual, the play's (it may take no
  /// voice started this frame) and those a later play takes the place of;
  /// so does one with kira's room on the track held by those fading out.
  /// None is refused
  #[test]
  fn loops_past_the_cap_or_kira_s_room_go_virtual() {
    let mut world = audio_world();
    let long = clip(&mut world, 2.0);
    let bed = add(&mut world, looping(&long));
    for _ in 0..96 {
      play(&mut world, &bed);
    }
    let past = play(&mut world, &bed);
    assert!(past.is_virtual());
    assert!(
      entries(&world, &past, AudioLogKind::Virtual)[0]
        .reason
        .starts_with("limit")
    );
    audio_step(&mut world, FRAME);
    // Each takes an older voice's place, whose fade holds kira's room
    let taking: Vec<Voice> = (0..32).map(|_| play(&mut world, &bed)).collect();
    assert!(taking.iter().all(|voice| !voice.is_virtual()));
    let reasons = |world: &World, prefix: &str| {
      log(world)
        .iter()
        .filter(|entry| entry.kind == AudioLogKind::Virtual && entry.reason.starts_with(prefix))
        .count()
    };
    assert_eq!(reasons(&world, "stolen"), 32);
    let full = play(&mut world, &bed);
    assert!(full.is_playing() && full.is_virtual(), "{full:?}");
    assert_eq!(reasons(&world, "full"), 1);
    assert!(
      log(&world)
        .iter()
        .all(|entry| entry.kind != AudioLogKind::Refused)
    );
    let mut audio = AudioView::new(&mut world);
    assert_eq!(
      (audio.voices_on("sfx"), audio.virtual_voices_on("sfx")),
      (96, 34)
    );
  }

  /// `PreventNew` keeps a heard voice from being taken: a loop it keeps
  /// out goes virtual, and comes back only once there is room, however
  /// loud
  #[test]
  fn a_loop_prevent_new_keeps_out_comes_back_only_with_room() {
    let mut world = audio_world();
    let long = clip(&mut world, 2.0);
    let hum = limited(&mut world, &long, 1, LimitRule::PreventNew);
    let a = play(&mut world, &hum);
    audio_step(&mut world, FRAME);
    let b = play(&mut world, &hum);
    assert!(!a.is_virtual() && b.is_virtual());
    b.set_volume(20.0);
    steps(&mut world, 30);
    assert!(b.is_virtual());
    a.stop(MIN_FADE);
    audio_step(&mut world, FRAME);
    assert!(!b.is_virtual(), "{b:?}");
    assert!(
      entries(&world, &b, AudioLogKind::Real)[0]
        .reason
        .starts_with("room")
    );
  }

  /// A virtual loop moves with a rebuild and takes a reload as a heard one
  /// does, starting nothing, and comes back on what they made current
  #[test]
  fn a_virtual_loop_comes_back_on_what_a_rebuild_and_a_reload_made_current() {
    let mut world = audio_world();
    AudioView::new(&mut world).set_mixer(Mixer {
      buses: vec![BusDef::new("sfx/beds")],
      ..Default::default()
    });
    let long = clip(&mut world, 2.0);
    let bed = add(
      &mut world,
      Sound {
        bus: "sfx/beds".to_string(),
        limit: Some(VoiceLimit {
          max: 1,
          rule: LimitRule::StopOldest,
        }),
        ..looping(&long)
      },
    );
    let a = play(&mut world, &bed);
    audio_step(&mut world, FRAME);
    let b = play(&mut world, &bed);
    assert!(a.is_virtual());
    // The mixer saved without its bus: it moves to sfx with the heard one,
    // with no kira sound to stop or start
    let before = log(&world).len();
    AudioView::new(&mut world).set_mixer(Mixer::default());
    let moved: Vec<(AudioLogKind, u32, String)> = log(&world)[before..]
      .iter()
      .filter(|entry| entry.voice != 0)
      .map(|entry| (entry.kind, entry.voice, entry.bus.clone()))
      .collect();
    assert_eq!(
      moved,
      [
        (AudioLogKind::Stop, b.id(), "sfx/beds".to_string()),
        (AudioLogKind::Start, b.id(), "sfx".to_string())
      ]
    );
    // Its definition 6 dB louder
    if let Some(sound) = world.resource_mut::<Assets<Sound>>().get_mut(&bed) {
      sound.volume_db = 6.0;
    }
    audio_step(&mut world, FRAME);
    b.stop(MIN_FADE);
    steps(&mut world, 20);
    let back = entries(&world, &a, AudioLogKind::Real);
    assert_eq!(back.len(), 1, "{back:#?}");
    assert_eq!((back[0].bus.as_str(), back[0].gain_db), ("sfx", 6.0));
    assert_eq!(sounds_on(&world, "sfx"), 1);
  }

  /// The ranking runs only in frames with a virtual voice
  #[test]
  fn a_frame_without_virtual_voices_ranks_nothing() {
    let mut world = audio_world();
    let long = clip(&mut world, 2.0);
    let hum = limited(&mut world, &long, 2, LimitRule::StopOldest);
    let oldest = play(&mut world, &hum);
    play(&mut world, &hum);
    steps(&mut world, 10);
    assert_eq!(world.resource::<AudioEngine>().ranked, 0);
    play(&mut world, &hum);
    assert!(oldest.is_virtual());
    steps(&mut world, 10);
    assert_eq!(world.resource::<AudioEngine>().ranked, 10);
    // Stopped, it is ranked in the frame its stop applies, and then gone
    oldest.stop(MIN_FADE);
    steps(&mut world, 10);
    assert!(oldest.is_stopped());
    assert_eq!(world.resource::<AudioEngine>().ranked, 11);
  }

  /// Before the web's unlock a loop out of range is virtual, as after it,
  /// and comes back once the listener nears after the unlock
  #[test]
  fn a_loop_out_of_range_before_the_unlock_comes_back_after_it() {
    let mut world = audio_world();
    let mut engine = AudioEngine::new_locked();
    engine.set_logging(true);
    world.insert_resource(engine);
    let listener = world
      .spawn((AudioListener, Transform::from_xy(0.0, 0.0)))
      .id();
    let dc = clip(&mut world, 1.0);
    let hum = add(&mut world, placed(&dc));
    let voice = AudioView::new(&mut world).play_at(&hum, Vec2::new(900.0, 0.0));
    assert!(voice.is_playing() && voice.is_virtual(), "{voice:?}");
    steps(&mut world, 5);
    crate::core::audio::unlock(&mut world);
    steps(&mut world, 20);
    assert!(voice.is_virtual());
    move_to(&mut world, listener, 800.0);
    audio_step(&mut world, FRAME);
    assert!(!voice.is_virtual(), "{voice:?}");
    assert_eq!(sounds_on(&world, "sfx"), 1);
  }
}
