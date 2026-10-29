//! [`Voice`]: a handle to one playing sound. Its methods queue commands that
//! the frame's `audio_system` applies, so a `Voice` can be kept anywhere

use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use crossbeam_channel::Sender;

use super::AudioClip;
use super::engine::MIN_FADE;
use crate::core::asset::AssetId;

/// What a voice is doing, as the engine last saw it
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum VoiceState {
  Playing = 0,
  Paused = 1,
  /// Fading out after a stop
  Stopping = 2,
  /// Over: stopped, or played to its end
  Stopped = 3,
}

impl VoiceState {
  fn from_u8(value: u8) -> VoiceState {
    match value {
      0 => VoiceState::Playing,
      1 => VoiceState::Paused,
      2 => VoiceState::Stopping,
      _ => VoiceState::Stopped,
    }
  }
}

/// A change asked of a voice
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum VoiceChange {
  /// Fade out over at least `MIN_FADE`, then stop
  Stop(Duration),
  /// Fade out over at least `MIN_FADE`, then pause
  Pause(Duration),
  /// Resume, fading in over at least `MIN_FADE`
  Resume(Duration),
  /// The game's change to the voice's gain, in dB, over at least `MIN_FADE`
  Volume(f32, Duration),
  /// The game's change to the voice's pitch, in semitones, over at least
  /// `MIN_FADE`
  Pitch(f32, Duration),
}

/// A change for voice `voice`, queued for `audio_system`
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct VoiceCommand {
  pub(crate) voice: u32,
  pub(crate) change: VoiceChange,
}

/// What a play picked and drew, as its voice started: which of its
/// sound's clips, and its gain and pitch, which are the definition's plus
/// the spread drawn from its ranges plus the play's [`PlayOptions`]. They
/// leave out how far away it is heard from and the game's time scale, and
/// keep what was drawn when the game later changes the voice (with
/// [`Voice::set_volume`] or [`Voice::set_pitch`]). See [`Voice::pick`]
///
/// [`PlayOptions`]: super::PlayOptions
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VoicePick {
  /// Its place in the sound's clips (0 for a clip played directly)
  pub clip_index: usize,
  /// The clip it plays
  pub clip: AssetId<AudioClip>,
  /// Its gain in dB (0 is the clip as recorded)
  pub volume_db: f32,
  /// Its pitch in semitones (12 is an octave up)
  pub pitch: f32,
}

/// What a voice's handles share with the engine's voice table
struct VoiceShared {
  /// From 1; 0 is a voice that never started
  id: u32,
  state: AtomicU8,
  /// It is virtual: a loop keeping its place silently, with no kira sound
  virtualized: AtomicBool,
  /// `None` for a voice that never started
  commands: Option<Sender<VoiceCommand>>,
  /// Empty for a voice that never started, one waiting for its sound to
  /// load, or one that plays no clip of its own (music joined from an
  /// intro and a loop)
  pick: OnceLock<VoicePick>,
}

/// One playing sound, returned by `ctx.audio().play(..)`. Keep it to stop,
/// pause or adjust that playback; clones control the same sound.
///
/// A change to a playing voice ramps from where it is over at least 10 ms,
/// so nothing clicks. The level and pitch a sound starts at belong in its
/// play ([`PlayOptions`](super::PlayOptions)): a plain change made in the
/// frame a voice started, which nothing has heard yet, applies at once, but
/// on an audio device the first few milliseconds may already be playing. An
/// `_over` change made then ramps from the level the play started at. The
/// engine applies changes as the scene code that made them (a callback,
/// one of its systems, an observer of its UI nodes) ends, and the rest
/// once a frame after the scenes' updates, so what
/// [`is_playing`](Self::is_playing) reports has changed by the next update.
/// Dropping a `Voice` never stops its sound, and a play that is refused (a
/// clip not loaded yet, say) returns a voice that is already stopped, on
/// which every method does nothing.
///
/// A voice belongs to the scene that played it, from a callback, one of its
/// systems or an observer of its UI nodes: when that scene exits (popped,
/// switched away or reset), a loop stops with its sound's `fade_out`, as
/// does a one-shot paused (by its own call, or held by the game's pause)
/// and a one-shot with more than 2 s left; a one-shot nearly over, or one
/// on `ui`, plays to its end. One played with
/// [`PlayOptions::persist`](super::PlayOptions::persist), or outside every
/// scene, belongs to the engine and plays on. One played from an
/// [`AudioEmitter`](super::AudioEmitter) belongs to its entity instead: as
/// the entity is despawned its loops and paused one-shots stop, and its
/// one-shots playing play out.
///
/// It follows the game's clock through its bus. On a bus that pauses with
/// the game (by default `sfx`, `voice` and `ambience`) it pauses where it
/// is while the game is paused, and resumes from there. On a bus that
/// follows time scale (`sfx` and `ambience`) it plays at its pitch's rate
/// times the game's speed (up to 8 times), and pauses while time stands
/// still (the game paused, or a speed of 0.01 or less). Hitstop does not
/// hold it: a hit's sound plays through the freeze.
///
/// A scene that is not updated is not heard: while a scene above it does
/// not update it (a pause menu pushed over the level, say), its voices on
/// buses that pause with the game are held the same way, and so are the
/// voices of the emitters it added (the world's entities stop with it, as
/// physics does; an emitter added outside every scene, or whose scene has
/// gone, is held while any scene is); they resume when it updates again.
/// The top scene can leave the scenes below audible
/// ([`Scene::holds_audio_below`](crate::core::scene::Scene::holds_audio_below)).
/// A voice held both by its scene and by the game's pause resumes once
/// both let it go.
///
/// A voice played while held waits, unheard, and fades in over its own
/// fade-in when it is let go. These holds are the engine's and leave the
/// voice's own [`pause`](Self::pause) alone: a held voice still reports
/// playing, and one paused itself stays paused when the game resumes.
///
/// ```
/// use std::time::Duration;
/// use ryo2d::prelude::*;
///
/// fn jump(ctx: &mut SceneContext<'_>, clip: &Handle<AudioClip>) -> Voice {
///   let options = PlayOptions { pitch: 2.0, volume_db: -6.0, ..Default::default() };
///   ctx.audio().play_with(clip, options)
/// }
///
/// fn land(voice: &Voice) {
///   voice.stop(Duration::from_millis(80));
/// }
/// ```
#[derive(Clone)]
pub struct Voice(Arc<VoiceShared>);

impl Voice {
  pub(crate) fn new(id: u32, commands: Sender<VoiceCommand>, pick: Option<VoicePick>) -> Voice {
    let voice = Voice(Arc::new(VoiceShared {
      id,
      state: AtomicU8::new(VoiceState::Playing as u8),
      virtualized: AtomicBool::new(false),
      commands: Some(commands),
      pick: OnceLock::new(),
    }));
    voice.set_pick(pick);
    voice
  }

  /// What its play picked, once it starts (a voice that waited for its
  /// sound to load picks as it starts)
  pub(crate) fn set_pick(&self, pick: Option<VoicePick>) {
    if let Some(pick) = pick {
      let _ = self.0.pick.set(pick);
    }
  }

  /// A voice that never started: shared, so returning one allocates nothing
  pub(crate) fn stopped() -> Voice {
    static STOPPED: OnceLock<Voice> = OnceLock::new();
    STOPPED
      .get_or_init(|| {
        Voice(Arc::new(VoiceShared {
          id: 0,
          state: AtomicU8::new(VoiceState::Stopped as u8),
          virtualized: AtomicBool::new(false),
          commands: None,
          pick: OnceLock::new(),
        }))
      })
      .clone()
  }

  /// The voice's id, as the audio log names it (0: it never started)
  #[doc(hidden)]
  pub fn id(&self) -> u32 {
    self.0.id
  }

  pub(crate) fn state(&self) -> VoiceState {
    VoiceState::from_u8(self.0.state.load(Ordering::Acquire))
  }

  pub(crate) fn set_state(&self, state: VoiceState) {
    self.0.state.store(state as u8, Ordering::Release);
  }

  /// It went virtual (`true`), or came back
  pub(crate) fn set_virtual(&self, virtualized: bool) {
    self.0.virtualized.store(virtualized, Ordering::Release);
  }

  fn send(&self, change: VoiceChange) {
    if self.is_stopped() {
      return;
    }
    if let Some(commands) = &self.0.commands {
      // The engine is gone (the world was dropped): nothing is playing
      let _ = commands.send(VoiceCommand {
        voice: self.0.id,
        change,
      });
    }
  }

  /// Fade out over `fade` (at least 10 ms), then stop for good. From the
  /// call it holds no room under its sound's voice limit or `per_frame`:
  /// a play made after it, even in the same frame, applies the stop first
  pub fn stop(&self, fade: Duration) {
    self.send(VoiceChange::Stop(fade));
  }

  /// Fade out over 10 ms and pause, keeping the position, until
  /// [`resume`](Self::resume) (the game resuming does not resume it)
  pub fn pause(&self) {
    self.pause_over(MIN_FADE);
  }

  /// [`pause`](Self::pause), fading out over `fade` (at least 10 ms): a
  /// line of dialogue paused under a menu over 200 ms, say
  pub fn pause_over(&self, fade: Duration) {
    self.send(VoiceChange::Pause(fade));
  }

  /// Resume a paused voice where it paused, fading in over 10 ms
  pub fn resume(&self) {
    self.resume_over(MIN_FADE);
  }

  /// [`resume`](Self::resume), fading in over `fade` (at least 10 ms; a
  /// voice not heard yet, paused from its start, fades in over its own
  /// fade-in if that is longer)
  pub fn resume_over(&self, fade: Duration) {
    self.send(VoiceChange::Resume(fade));
  }

  /// Set the game's change to the voice's gain in dB, relative to the
  /// sound's level (its definition's volume, the spread its play drew and
  /// its [`PlayOptions`](super::PlayOptions)), as `PlayOptions` are: 0
  /// plays it at the sound's level, -6 about half as loud. The sum is at
  /// most +24 dB, and -60 and below are silent. A reload of the definition
  /// still moves the sound's level beneath it. Smoothed over 10 ms
  pub fn set_volume(&self, db: f32) {
    self.set_volume_over(db, MIN_FADE);
  }

  /// [`set_volume`](Self::set_volume), moving there over `fade` (at least
  /// 10 ms), in an even line of dB: a loop brought down 12 dB over 2 s,
  /// say, with no stepping each frame
  pub fn set_volume_over(&self, db: f32, fade: Duration) {
    self.send(VoiceChange::Volume(db, fade));
  }

  /// Set the game's change to the voice's pitch in semitones, relative to
  /// the sound's pitch (its definition's, the spread its play drew and its
  /// [`PlayOptions`](super::PlayOptions)), as `PlayOptions` are: 12 is an
  /// octave above the sound. The sum is kept within 48 semitones either
  /// way. Pitch changes speed too, as on a turntable. On a bus that follows
  /// time scale, the game's speed still multiplies it. Smoothed over 10 ms
  pub fn set_pitch(&self, semitones: f32) {
    self.set_pitch_over(semitones, MIN_FADE);
  }

  /// [`set_pitch`](Self::set_pitch), gliding there over `fade` (at least
  /// 10 ms)
  pub fn set_pitch_over(&self, semitones: f32, fade: Duration) {
    self.send(VoiceChange::Pitch(semitones, fade));
  }

  /// Whether it is playing: started, not paused, not stopping or stopped.
  /// A voice held by the game's pause, or while its scene is not updated,
  /// still counts as playing
  pub fn is_playing(&self) -> bool {
    self.state() == VoiceState::Playing
  }

  /// Whether it is over: stopped and faded out, played to its end, or never
  /// started. A stopped voice cannot be resumed
  pub fn is_stopped(&self) -> bool {
    self.state() == VoiceState::Stopped
  }

  /// Whether it is virtual: a loop that keeps its place silently, taking no
  /// voice, until it can be heard again. A loop goes virtual when its bus's
  /// cap of 96, or a voice limit whose rule ranks (`StopQuietest`,
  /// `StopFarthest`, `StopLowestPriority`), has no room for it (a play past
  /// the limit, or another play taking its place), and when it is at or
  /// beyond its sound's `max_distance` from the listener; `PreventNew` and
  /// `StopOldest` refuse or stop a loop for good. While virtual it
  /// still reports playing, takes every change (volume, pitch, pause,
  /// resume, stop), is held by the game's pause and its scene's hold as a
  /// heard voice is, and moves on through its clip as if heard; each frame
  /// the engine brings it back, fading in over 50 ms where it has got to,
  /// once it is in range (inside 95% of `max_distance`) and there is room,
  /// or it outranks the weakest voice in its way (more important, or 3 dB
  /// louder as heard at equal priority). One-shots and music are never
  /// virtual. A voice that went virtual or came back stays so for at least
  /// 250 ms, so two loops of about the same level do not swap back and
  /// forth
  pub fn is_virtual(&self) -> bool {
    self.0.virtualized.load(Ordering::Acquire)
  }

  /// Which clip the play picked and the gain and pitch it drew, as it
  /// started; `None` for a play that never started (refused), for a loop
  /// waiting for its sound to load until it starts, and for a play a
  /// system's [`Audio`](super::Audio) queued until its buffer applies (the
  /// pick is made then: read it in a later system or frame). A subtitle for
  /// a line picked at random, say:
  ///
  /// ```
  /// use ryo2d::prelude::*;
  ///
  /// fn bark(ctx: &mut SceneContext<'_>, barks: &Handle<Sound>, lines: &[&str]) -> Option<String> {
  ///   let pick = ctx.audio().play(barks).pick()?;
  ///   lines.get(pick.clip_index).map(|line| line.to_string())
  /// }
  /// ```
  pub fn pick(&self) -> Option<VoicePick> {
    self.0.pick.get().copied()
  }
}

/// A voice that never started (`is_stopped` is true), for a field that
/// holds a voice before anything plays
impl Default for Voice {
  fn default() -> Self {
    Voice::stopped()
  }
}

impl fmt::Debug for Voice {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_struct("Voice")
      .field("id", &self.0.id)
      .field("state", &self.state())
      .field("virtual", &self.is_virtual())
      .finish()
  }
}

#[cfg(test)]
mod tests {
  use std::time::Duration;

  use crate::core::asset::{Assets, Handle};
  use crate::core::audio::engine::MIN_FADE;
  use crate::core::audio::{
    AudioClip, AudioEngine, AudioLogEntry, AudioLogKind, AudioView, PlayOptions, Voice,
  };
  use crate::core::ecs::World;
  use crate::core::testing::{audio_step, audio_world};

  const FRAME: Duration = Duration::from_micros(16_667);

  fn clip(world: &mut World, seconds: f32) -> Handle<AudioClip> {
    let frames = vec![[0.1, 0.1]; (48_000.0 * seconds) as usize];
    world
      .resource_mut::<Assets<AudioClip>>()
      .add(AudioClip::from_frames(48_000, &frames).unwrap())
  }

  fn log(world: &World) -> Vec<AudioLogEntry> {
    world.resource::<AudioEngine>().log().to_vec()
  }

  fn entry(world: &World, kind: AudioLogKind) -> AudioLogEntry {
    let log = log(world);
    match log.iter().find(|entry| entry.kind == kind) {
      Some(entry) => entry.clone(),
      None => panic!("no {kind:?} entry in {log:#?}"),
    }
  }

  /// A change made in the scene code that plays a voice reaches kira with
  /// its start: applied as the scene's code ends, with no ramp, so the
  /// first audio rendered is at the level set
  #[test]
  fn a_change_made_as_a_voice_starts_is_heard_from_its_first_audio() {
    let (mut world, meter) = crate::core::testing::metered_audio_world();
    let clip = clip(&mut world, 1.0);
    let voice = crate::core::audio::in_scene(&mut world, 1, |world| {
      let voice = AudioView::new(world).play(&clip);
      voice.set_volume(-20.0);
      voice
    });
    let gain = entry(&world, AudioLogKind::Gain);
    assert_eq!(
      (gain.voice, gain.gain_db, gain.fade),
      (voice.id(), -20.0, Duration::ZERO)
    );
    audio_step(&mut world, FRAME);
    let frames = meter.frames();
    let (first, settled) = (frames[0].left, frames[frames.len() - 1].left);
    assert!(settled > 0.001, "heard: {settled}");
    assert!(
      (first / settled - 1.0).abs() < 0.01,
      "the first audio is at the level set: {first} against {settled}"
    );
  }

  /// A fade longer than 10 ms asked for as a voice starts is kept: it ramps
  /// from the level the play started at, so a bed faded up from silence
  /// starts silent
  #[test]
  fn a_fade_asked_as_a_voice_starts_ramps_from_its_start() {
    let (mut world, meter) = crate::core::testing::metered_audio_world();
    let clip = clip(&mut world, 3.0);
    let voice = crate::core::audio::in_scene(&mut world, 1, |world| {
      let options = PlayOptions {
        volume_db: -60.0,
        ..Default::default()
      };
      let voice = AudioView::new(world).play_with(&clip, options);
      voice.set_volume_over(60.0, Duration::from_secs(1));
      voice.set_pitch_over(3.0, Duration::from_millis(500));
      voice
    });
    let fades: Vec<(AudioLogKind, Duration)> = log(&world)
      .iter()
      .filter(|entry| entry.voice == voice.id() && entry.kind != AudioLogKind::Start)
      .map(|entry| (entry.kind, entry.fade))
      .collect();
    assert_eq!(
      fades,
      [
        (AudioLogKind::Gain, Duration::from_secs(1)),
        (AudioLogKind::Rate, Duration::from_millis(500)),
      ]
    );
    audio_step(&mut world, FRAME);
    let first = meter.frames()[0].left;
    assert!(first.abs() < 0.001, "it starts near silent: {first}");
  }

  /// The `_over` forms change a playing voice over the fade they are given
  /// (at least 10 ms), as `stop` does; the plain forms take 10 ms
  #[test]
  fn the_over_forms_take_their_fade() {
    let mut world = audio_world();
    let clip = clip(&mut world, 3.0);
    let voice = AudioView::new(&mut world).play(&clip);
    audio_step(&mut world, FRAME);
    voice.set_volume_over(-6.0, Duration::from_secs(2));
    voice.set_pitch_over(3.0, Duration::from_millis(500));
    audio_step(&mut world, FRAME);
    voice.pause_over(Duration::from_millis(200));
    audio_step(&mut world, FRAME);
    // kira is still fading it out to its pause
    let pausing = world.resource::<AudioEngine>().voice_playback(voice.id());
    assert_eq!(pausing, Some(kira::sound::PlaybackState::Pausing));
    for _ in 0..15 {
      audio_step(&mut world, FRAME);
    }
    voice.resume_over(Duration::from_millis(300));
    audio_step(&mut world, FRAME);
    voice.set_volume_over(0.0, Duration::ZERO);
    audio_step(&mut world, FRAME);
    let fades: Vec<(AudioLogKind, Duration)> = log(&world)
      .iter()
      .filter(|entry| entry.voice == voice.id() && entry.kind != AudioLogKind::Start)
      .map(|entry| (entry.kind, entry.fade))
      .collect();
    assert_eq!(
      fades,
      [
        (AudioLogKind::Gain, Duration::from_secs(2)),
        (AudioLogKind::Rate, Duration::from_millis(500)),
        (AudioLogKind::Pause, Duration::from_millis(200)),
        (AudioLogKind::Resume, Duration::from_millis(300)),
        (AudioLogKind::Gain, MIN_FADE),
      ]
    );
    let resuming = world.resource::<AudioEngine>().voice_playback(voice.id());
    assert_eq!(resuming, Some(kira::sound::PlaybackState::Resuming));
  }

  #[test]
  fn stop_fades_and_really_stops() {
    let mut world = audio_world();
    let clip = clip(&mut world, 1.0);
    let voice = AudioView::new(&mut world).play(&clip);
    assert!(voice.is_playing());
    voice.stop(Duration::from_millis(30));
    // Still fading out after 20 ms: kira got the 30 ms fade
    audio_step(&mut world, Duration::from_millis(20));
    assert!(!voice.is_stopped() && !voice.is_playing(), "{voice:?}");
    audio_step(&mut world, Duration::from_millis(30));
    let stop = entry(&world, AudioLogKind::Stop);
    assert_eq!(stop.fade, Duration::from_millis(30));
    assert_eq!(stop.voice, voice.id());
    assert!(voice.is_stopped(), "{voice:?}");
    assert_eq!(AudioView::new(&mut world).voices_on("sfx"), 0);
  }

  #[test]
  fn every_change_uses_at_least_the_minimum_fade() {
    let mut world = audio_world();
    let (one, other) = (clip(&mut world, 1.0), clip(&mut world, 1.0));
    let (changed, stopped) = {
      let mut audio = AudioView::new(&mut world);
      (audio.play(&one), audio.play(&other))
    };
    // Playing, past the frame they started in
    audio_step(&mut world, FRAME);
    changed.set_volume(-6.0);
    changed.set_pitch(12.0);
    changed.pause();
    stopped.stop(Duration::ZERO);
    audio_step(&mut world, FRAME);
    assert!(!changed.is_playing(), "paused: {changed:?}");
    changed.resume();
    audio_step(&mut world, FRAME);
    assert!(changed.is_playing(), "resumed: {changed:?}");
    for kind in [
      AudioLogKind::Gain,
      AudioLogKind::Rate,
      AudioLogKind::Pause,
      AudioLogKind::Resume,
      AudioLogKind::Stop,
    ] {
      // `stop(ZERO)` among them: raised to the minimum
      assert_eq!(entry(&world, kind).fade, MIN_FADE, "{kind:?}");
    }
    assert_eq!(entry(&world, AudioLogKind::Gain).gain_db, -6.0);
    assert!((entry(&world, AudioLogKind::Rate).rate - 2.0).abs() < 1e-9);
    assert_eq!(entry(&world, AudioLogKind::Stop).voice, stopped.id());
  }

  #[test]
  fn dropping_a_voice_does_not_stop_it() {
    let mut world = audio_world();
    let clip = clip(&mut world, 1.0);
    drop(AudioView::new(&mut world).play(&clip));
    for _ in 0..10 {
      audio_step(&mut world, FRAME);
    }
    assert_eq!(AudioView::new(&mut world).voices_on("sfx"), 1);
    assert!(
      !log(&world)
        .iter()
        .any(|entry| entry.kind == AudioLogKind::Stop),
      "{:#?}",
      log(&world)
    );
  }

  #[test]
  fn a_voice_that_plays_to_its_end_is_stopped() {
    let mut world = audio_world();
    let clip = clip(&mut world, 0.05);
    let voice = AudioView::new(&mut world).play(&clip);
    for _ in 0..6 {
      audio_step(&mut world, FRAME);
    }
    assert!(voice.is_stopped(), "{voice:?}");
    assert_eq!(AudioView::new(&mut world).voices_on("sfx"), 0);
  }

  #[test]
  fn play_options_set_the_voice_s_gain_and_rate() {
    let mut world = audio_world();
    let clip = clip(&mut world, 1.0);
    let options = PlayOptions {
      volume_db: -12.0,
      pitch: -12.0,
      ..Default::default()
    };
    AudioView::new(&mut world).play_with(&clip, options);
    let start = entry(&world, AudioLogKind::Start);
    assert_eq!(start.gain_db, -12.0);
    assert!((start.rate - 0.5).abs() < 1e-9);
  }

  #[test]
  fn a_voice_that_never_started_ignores_every_call() {
    let mut world = audio_world();
    let unloaded = crate::core::asset::AssetsView::new(&mut world).load::<AudioClip>("nope.wav");
    let voice = AudioView::new(&mut world).play(&unloaded);
    assert!(voice.is_stopped());
    assert_eq!(voice.id(), 0);
    voice.set_volume(-3.0);
    voice.stop(Duration::ZERO);
    audio_step(&mut world, FRAME);
    // Its refusal is all the log says
    let kinds: Vec<AudioLogKind> = log(&world).iter().map(|entry| entry.kind).collect();
    assert_eq!(kinds, [AudioLogKind::Refused], "{:#?}", log(&world));
    assert!(Voice::default().is_stopped());
  }
}
