//! [`Voice`]: a handle to one playing sound. Its methods queue commands that
//! the frame's `audio_system` applies, so a `Voice` can be kept anywhere

use std::fmt;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use crossbeam_channel::Sender;

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

/// What a voice's handles share with the engine's voice table
struct VoiceShared {
  /// From 1; 0 is a voice that never started
  id: u32,
  state: AtomicU8,
  /// `None` for a voice that never started
  commands: Option<Sender<VoiceCommand>>,
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
  pub(crate) fn new(id: u32, commands: Sender<VoiceCommand>) -> Voice {
    Voice(Arc::new(VoiceShared {
      id,
      state: AtomicU8::new(VoiceState::Playing as u8),
      commands: Some(commands),
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

  /// Fade out and pause, keeping the position
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
  /// pitch it was played with. Pitch changes speed too, as on a turntable
  pub fn set_pitch(&self, semitones: f32) {
    self.send(VoiceChange::Pitch(semitones));
  }

  /// Whether it is playing: started, not paused, not stopping or stopped
  pub fn is_playing(&self) -> bool {
    self.state() == VoiceState::Playing
  }

  /// Whether it is over: stopped and faded out, played to its end, or never
  /// started. A stopped voice cannot be resumed
  pub fn is_stopped(&self) -> bool {
    self.state() == VoiceState::Stopped
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
    audio_step(&mut world, Duration::from_millis(50));
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
      let entry = entry(&world, kind);
      assert!(entry.fade >= MIN_FADE, "{kind:?}: {:?}", entry.fade);
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
