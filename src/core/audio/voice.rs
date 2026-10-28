//! [`Voice`]: a handle to one playing sound. Its methods queue commands that
//! the frame's `audio_system` applies, so a `Voice` can be kept anywhere

use std::fmt;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use crossbeam_channel::Sender;

use super::AudioClip;
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
  Pause,
  Resume,
  /// The voice's own gain, in dB
  Volume(f32),
  /// The voice's own pitch, in semitones
  Pitch(f32),
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
  /// `None` for a voice that never started
  commands: Option<Sender<VoiceCommand>>,
  /// `None` for a voice that never started, or one that plays no clip of
  /// its own (music joined from an intro and a loop)
  pick: Option<VoicePick>,
}

/// One playing sound, returned by `ctx.audio().play(..)`. Keep it to stop,
/// pause or adjust that playback; clones control the same sound.
///
/// Every change is smoothed (at least 10 ms) so nothing clicks. The engine
/// applies changes once a frame, after the scenes' updates, so what
/// [`is_playing`](Self::is_playing) reports has changed by the next update.
/// Dropping a `Voice` never stops its sound, and a play that is refused (a
/// clip not loaded yet, say) returns a voice that is already stopped, on
/// which every method does nothing.
///
/// A voice belongs to the scene that played it, from a callback, one of its
/// systems or an observer of its UI nodes: when that scene exits (popped,
/// switched away or reset), a loop stops with its sound's `fade_out`, as
/// does a one-shot paused (by its own call, or held by the game's pause),
/// and a one-shot playing then plays to its end. One played with
/// [`PlayOptions::persist`](super::PlayOptions::persist), or outside every
/// scene, belongs to the engine and plays on. One played from an
/// [`AudioEmitter`](super::AudioEmitter) belongs to its entity instead, and
/// stops, by the same rule, as the entity is despawned.
///
/// It follows the game's clock through its bus. On a bus that pauses with
/// the game (by default `sfx`, `voice` and `ambience`) it pauses where it
/// is while the game is paused, and resumes from there. On a bus that
/// follows time scale (`sfx` and `ambience`) it plays at its pitch's rate
/// times the game's speed (up to 8 times), and pauses while time stands
/// still (the game paused, or a speed of 0.01 or less). Hitstop does not
/// hold it: a hit's sound plays through the freeze. A voice played while
/// its bus is held waits, unheard, and fades in over its own fade-in when
/// the bus lets it go. These holds are the engine's and leave the voice's
/// own [`pause`](Self::pause) alone: a held voice still reports playing,
/// and one paused itself stays paused when the game resumes.
///
/// ```
/// use std::time::Duration;
/// use ryo2d::prelude::*;
///
/// fn jump(ctx: &mut SceneContext<'_>, clip: &Handle<AudioClip>) -> Voice {
///   let voice = ctx.audio().play_with(clip, PlayOptions { pitch: 2.0, ..Default::default() });
///   voice.set_volume(-6.0);
///   voice
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
    Voice(Arc::new(VoiceShared {
      id,
      state: AtomicU8::new(VoiceState::Playing as u8),
      commands: Some(commands),
      pick,
    }))
  }

  /// A voice that never started: shared, so returning one allocates nothing
  pub(crate) fn stopped() -> Voice {
    static STOPPED: OnceLock<Voice> = OnceLock::new();
    STOPPED
      .get_or_init(|| {
        Voice(Arc::new(VoiceShared {
          id: 0,
          state: AtomicU8::new(VoiceState::Stopped as u8),
          commands: None,
          pick: None,
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

  /// Fade out over `fade` (at least 10 ms), then stop for good
  pub fn stop(&self, fade: Duration) {
    self.send(VoiceChange::Stop(fade));
  }

  /// Fade out and pause, keeping the position, until [`resume`](Self::resume)
  /// (the game resuming does not resume it)
  pub fn pause(&self) {
    self.send(VoiceChange::Pause);
  }

  /// Resume a paused voice where it paused, fading in
  pub fn resume(&self) {
    self.send(VoiceChange::Resume);
  }

  /// Set the voice's own gain in dB (0 is the clip as recorded; -60 and
  /// below are silent; at most +24), replacing the gain it was played with
  pub fn set_volume(&self, db: f32) {
    self.send(VoiceChange::Volume(db));
  }

  /// Set the voice's own pitch in semitones (-48 to 48), replacing the
  /// pitch it was played with. Pitch changes speed too, as on a turntable.
  /// On a bus that follows time scale, the game's speed still multiplies it
  pub fn set_pitch(&self, semitones: f32) {
    self.send(VoiceChange::Pitch(semitones));
  }

  /// Whether it is playing: started, not paused, not stopping or stopped.
  /// A voice its bus holds while the game is paused still counts as
  /// playing
  pub fn is_playing(&self) -> bool {
    self.state() == VoiceState::Playing
  }

  /// Whether it is over: stopped and faded out, played to its end, or never
  /// started. A stopped voice cannot be resumed
  pub fn is_stopped(&self) -> bool {
    self.state() == VoiceState::Stopped
  }

  /// Which clip the play picked and the gain and pitch it drew, as it
  /// started; `None` for a play that never started (refused, or a sound not
  /// loaded yet). A subtitle for a line picked at random, say:
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
    self.0.pick
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
      .add(AudioClip::from_frames(48_000, &frames))
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
    let clip = clip(&mut world, 1.0);
    let (changed, stopped) = {
      let mut audio = AudioView::new(&mut world);
      (audio.play(&clip), audio.play(&clip))
    };
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
    assert!(log(&world).is_empty(), "{:#?}", log(&world));
    assert!(Voice::default().is_stopped());
  }
}
