//! [`AudioEngine`]: the world resource that owns kira's manager (on the audio
//! device, or on kira's mock backend when there is none), the bus tree and
//! the live voices

use std::collections::HashMap;
use std::fmt::Display;
use std::ops::Range;
#[cfg(test)]
use std::sync::Arc;
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender};
use kira::backend::cpal::CpalBackend;
use kira::backend::mock::{MockBackend, MockBackendSettings};
use kira::sound::static_sound::{StaticSoundData, StaticSoundHandle};
#[cfg(not(target_arch = "wasm32"))]
use kira::sound::streaming::{StreamingSoundData, StreamingSoundHandle};
use kira::sound::{EndPosition, PlaybackPosition, PlaybackState, Region};
use kira::track::{MainTrackBuilder, SendTrackBuilder, SendTrackHandle, TrackBuilder, TrackHandle};
use kira::{
  AudioManager, AudioManagerSettings, Capacities, Decibels, Easing, Panning, PlaybackRate,
  ResourceLimitReached, Semitones, StartTime, Tween,
};
use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng};

use super::log::{AudioLog, AudioLogEntry, AudioLogKind};
use super::mix::{Blend, DuckState, Level, OPEN_HZ};
use super::mixer::{
  Bus, BusReport, BusTree, GainLayers, MAX_BUS_DB, Output, PlannedBus, Route, SEND_TRACK_CAPACITY,
  Tracks, plan,
};
use super::music::{MusicPlayer, Track};
use super::settings::{SILENCE_DB, VolumeSettings};
use super::spatial::{
  AudioEmitter, Hearing, Listener, ListenerQuery, MAX_FILTERED, SpatialVoice, VoiceFilter,
  emitter_position, heard_db,
};
#[cfg(not(target_arch = "wasm32"))]
use super::stream::{ClipDecoder, StreamError};
use super::view::{PlayOptions, Playable};
use super::voice::{Voice, VoiceChange, VoiceCommand, VoicePick, VoiceState};
use super::{
  AudioClip, ClipData, EncodedClip, FocusLoss, LimitRule, Mixer, Music, MusicData, SnapshotDef,
  Sound, SoundClip, Spatial, VoiceLimit, VolumeKind, volume_to_db,
};
use crate::core::asset::{
  AssetEvent, AssetId, AssetServer, Assets, Handle, LoadState, UntypedAssetId,
};
use crate::core::ecs::kernel::{EntityHashSet, MessageCursor, Messages, Mut};
use crate::core::ecs::{Disabled, Entity, Resource, Vec2, World};
use crate::core::hash::{FastMap, FastSet};

/// The shortest smoothing of any change to a voice (volume, pitch, pause,
/// stop): shorter jumps click
pub(crate) const MIN_FADE: Duration = Duration::from_millis(10);

/// The most voices playing on one bus's own track. kira holds 128 sounds a
/// track, so the voices stolen past the cap have room to fade out
pub(crate) const VOICE_CAP: usize = 96;

/// The seed a headless engine (`HeadlessEngine`, `RYO_HEADLESS`) starts
/// its picks and draws from, so its runs repeat
pub(crate) const HEADLESS_SEED: u64 = 0x5EED_A0D1;

/// The mock backend's sample rate
const MOCK_SAMPLE_RATE: u32 = 48_000;
/// kira renders in chunks of this many frames (its internal buffer size,
/// set on both managers), applying changes once per chunk
const CHUNK_FRAMES: usize = 128;
/// The most real time the mock backend renders in one frame: a frame after
/// a stall (the machine slept, say) renders a second, not the whole stall
const MAX_MOCK_STEP: Duration = Duration::from_secs(1);
/// Voices the table holds before it grows
const VOICE_CAPACITY: usize = 128;
/// The most a one-shot playing as its scene exits may have left to finish:
/// a click or a stinger is heard out, while a longer one (a voice-over, a
/// long streamed clip) stops with its fade rather than run on into the next
/// scene. One on `ui` finishes whatever it has left
const FINISH_AT_EXIT: Duration = Duration::from_secs(2);
/// A voice's own gain, in dB: kira is silent at -60 and below
const GAIN_RANGE: (f32, f32) = (-60.0, 24.0);
/// A voice's own pitch, in semitones either way
const PITCH_RANGE: f32 = 48.0;
/// The most the game's change to a voice's gain may be, either way: the
/// whole of its range
const GAIN_SPAN: f32 = GAIN_RANGE.1 - GAIN_RANGE.0;
/// The most the game's change to a voice's pitch may be, either way
const PITCH_SPAN: f32 = 2.0 * PITCH_RANGE;
/// A bus gain change smaller than this (dB) is not sent to kira
const GAIN_EPSILON: f32 = 1e-4;
/// A cutoff change smaller than this (Hz) is not sent to kira
const CUTOFF_EPSILON: f32 = 0.01;
/// Where a clip played directly goes
const DEFAULT_BUS: &str = "sfx";
/// Where music plays, or under it
const MUSIC_BUS: &str = "music";
/// A music voice's priority: the highest, so no other voice takes its room
const MUSIC_PRIORITY: u8 = u8::MAX;
/// A time scale at or below this stands time still for the buses that
/// follow it: their voices are held, as kira cannot play a rate of 0
const HOLD_SCALE: f32 = 0.01;
/// A time scale change smaller than this is not sent to the voices
const SCALE_EPSILON: f64 = 1e-6;
/// The fastest time scale audio follows: the game clock runs up to 100
/// times as fast, which would swamp kira's mixing thread
const MAX_SCALE: f32 = 8.0;

/// Whether `bus` holds its voices under `game`: one that pauses with the
/// game while it is paused, or one that follows time scale while time
/// stands still (a scale at or below [`HOLD_SCALE`]). A send bus plays no
/// voices
fn holds(bus: &Bus, game: GameTime) -> bool {
  let still = game.scale <= HOLD_SCALE;
  !matches!(bus.output, Output::Send(_))
    && ((bus.pauses_with_game && game.paused) || (bus.follows_time_scale && still))
}

/// Whether a voice owned by `scope` (a scene's; `None` for the engine's),
/// or played from an emitter, is held by its owner while the scene stack
/// holds the scenes in `held`: a scene's voice while its scene is there,
/// and an emitter's while any is (the world's entities stop with the
/// scenes below the top, as physics does). Only on a bus that pauses with
/// the game
fn owner_holds(held: &[u64], scope: Option<u64>, emitter: bool) -> bool {
  match scope {
    Some(scope) => held.contains(&scope),
    None => emitter && !held.is_empty(),
  }
}

/// The bus a UI sound plays on: `bus` if it is `ui` or one under it, and
/// `ui` if not
fn ui_bus(bus: &str) -> &str {
  if bus == "ui" || bus.starts_with("ui/") {
    bus
  } else {
    "ui"
  }
}

/// The game's clock as audio follows it, read from `Time` each frame
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct GameTime {
  /// The game is paused (`time.game.pause()`)
  pub(crate) paused: bool,
  /// The time scale: 0 while paused, the game's speed otherwise, at most
  /// [`MAX_SCALE`]
  pub(crate) scale: f32,
}

impl GameTime {
  /// `time`'s game clock. Hitstop is not in it: it is a visual freeze, and
  /// the hit's sound plays through it
  pub(crate) fn of(time: &crate::core::time::Time) -> GameTime {
    let paused = time.game.is_paused();
    let scale = if paused {
      0.0
    } else {
      time.game.speed().min(MAX_SCALE)
    };
    GameTime { paused, scale }
  }
}

/// A game clock that runs at normal speed (a world without `Time`)
impl Default for GameTime {
  fn default() -> Self {
    GameTime {
      paused: false,
      scale: 1.0,
    }
  }
}

/// The error the failing-device seam reports
#[cfg(test)]
pub(crate) const FAILING_DEVICE: &str = "no output device (the failing-device seam)";

/// What the mock backend renders: the last frame of each chunk of kira's
/// main track, before master's gain (tests)
#[cfg(test)]
#[derive(Clone, Default)]
pub(crate) struct Meter(Arc<std::sync::Mutex<Vec<kira::Frame>>>);

#[cfg(test)]
impl Meter {
  /// Each chunk's last frame so far, oldest first
  pub(crate) fn frames(&self) -> Vec<kira::Frame> {
    self
      .0
      .lock()
      .map(|frames| frames.clone())
      .unwrap_or_default()
  }

  /// The latest chunk's last frame (silence before any)
  pub(crate) fn last(&self) -> kira::Frame {
    self.frames().last().copied().unwrap_or(kira::Frame::ZERO)
  }
}

#[cfg(test)]
impl kira::effect::Effect for Meter {
  fn process(&mut self, input: &mut [kira::Frame], _dt: f64, _info: &kira::info::Info) {
    if let (Some(frame), Ok(mut frames)) = (input.last(), self.0.lock()) {
      frames.push(*frame);
    }
  }
}

/// The kira manager, on the audio device or on the mock backend. There is
/// one per engine, so the variants' sizes do not matter
#[allow(clippy::large_enum_variant)]
enum Manager {
  Device(AudioManager<CpalBackend>),
  Mock(AudioManager<MockBackend>),
}

impl Manager {
  /// The mock backend at 48 kHz: renders nothing aloud, advanced by
  /// `audio_system`. `None` only if kira's mock fails to start, which it
  /// never does
  fn mock() -> Option<Manager> {
    Self::mock_with(MainTrackBuilder::default())
  }

  /// [`mock`](Self::mock), its main track built by `main`
  fn mock_with(main: MainTrackBuilder) -> Option<Manager> {
    let settings = AudioManagerSettings {
      backend_settings: MockBackendSettings {
        sample_rate: MOCK_SAMPLE_RATE,
      },
      capacities: capacities(),
      internal_buffer_size: CHUNK_FRAMES,
      main_track_builder: main,
    };
    AudioManager::<MockBackend>::new(settings)
      .ok()
      .map(Manager::Mock)
  }

  /// The default output device, or the mock backend with one warning.
  /// `buffer_frames` fixes the device's buffer size (cut to what it
  /// supports); a device that will not open with it opens with its default,
  /// with a warning
  fn device(buffer_frames: Option<u32>) -> Option<Manager> {
    let settings = || AudioManagerSettings::<CpalBackend> {
      capacities: capacities(),
      internal_buffer_size: CHUNK_FRAMES,
      ..Default::default()
    };
    // kira restarts the stream with the same buffer when the default
    // device changes; a new device that refuses it stops the sound
    let fixed = buffer_frames
      .filter(|&frames| frames > 0)
      .map(fixed_buffer_config);
    match fixed {
      Some(Ok(Some((config, frames)))) => {
        let mut fixed = settings();
        fixed.backend_settings.config = Some(config);
        match AudioManager::<CpalBackend>::new(fixed) {
          Ok(manager) => return Some(Manager::Device(manager)),
          Err(error) => {
            crate::engine_warn!(module: "Audio", "{}", buffer_refused(frames, &error))
          }
        }
      }
      Some(Err(message)) => crate::engine_warn!(module: "Audio", "{message}"),
      // No device: the open below warns about that
      Some(Ok(None)) | None => {}
    }
    match AudioManager::<CpalBackend>::new(settings()) {
      Ok(manager) => Some(Manager::Device(manager)),
      Err(error) => Manager::fallback(&error),
    }
  }

  /// The mock backend for the session, after the device failed to open
  fn fallback(error: &dyn Display) -> Option<Manager> {
    crate::engine_warn!(
      module: "Audio",
      "the audio device could not be opened ({error}); sound plays silently on the mock backend for this session"
    );
    Manager::mock()
  }

  /// Set kira's main track's volume: `master`'s gain, over every track and
  /// send track
  fn set_main_volume(&mut self, db: f32, tween: Tween) {
    match self {
      Manager::Device(manager) => manager.main_track().set_volume(Decibels(db), tween),
      Manager::Mock(manager) => manager.main_track().set_volume(Decibels(db), tween),
    }
  }

  /// kira's tracks under its main track, and its send tracks (tests)
  #[cfg(test)]
  fn track_counts(&self) -> (usize, usize) {
    match self {
      Manager::Device(manager) => (manager.num_sub_tracks(), manager.num_send_tracks()),
      Manager::Mock(manager) => (manager.num_sub_tracks(), manager.num_send_tracks()),
    }
  }
}

/// The kira manager the web opens at its first user gesture: browsers start
/// a page's audio only inside one, and kira keeps the browser's audio
/// context to itself, so it is made there rather than resumed
enum Unlock {
  /// The audio device, its buffer fixed at this many frames when given
  Device(Option<u32>),
  /// The mock backend (tests)
  #[cfg(test)]
  Mock,
}

impl Unlock {
  fn open(self) -> Option<Manager> {
    match self {
      Unlock::Device(buffer_frames) => Manager::device(buffer_frames),
      #[cfg(test)]
      Unlock::Mock => Manager::mock(),
    }
  }
}

impl Tracks for Manager {
  fn add_sub_track(&mut self, builder: TrackBuilder) -> Result<TrackHandle, ResourceLimitReached> {
    match self {
      Manager::Device(manager) => manager.add_sub_track(builder),
      Manager::Mock(manager) => manager.add_sub_track(builder),
    }
  }

  fn add_send_track(
    &mut self,
    builder: SendTrackBuilder,
  ) -> Result<SendTrackHandle, ResourceLimitReached> {
    match self {
      Manager::Device(manager) => manager.add_send_track(builder),
      Manager::Mock(manager) => manager.add_send_track(builder),
    }
  }
}

/// kira's room for tracks: its defaults, with room for the send tracks of
/// a retired tree beside the live one's
fn capacities() -> Capacities {
  Capacities {
    send_track_capacity: SEND_TRACK_CAPACITY,
    ..Default::default()
  }
}

/// A fixed buffer and its frames (cut to what the device supports)
#[cfg(not(target_arch = "wasm32"))]
type FixedBuffer = (kira::backend::cpal::cpal::StreamConfig, u32);

/// The default output device's stream config with its buffer fixed at
/// `frames`; `Ok(None)` with no device (the open that follows warns), and
/// the warning to give when its config cannot be read
#[cfg(not(target_arch = "wasm32"))]
fn fixed_buffer_config(frames: u32) -> Result<Option<FixedBuffer>, String> {
  use kira::backend::cpal::cpal::traits::{DeviceTrait, HostTrait};
  let Some(device) = kira::backend::cpal::cpal::default_host().default_output_device() else {
    return Ok(None);
  };
  let supported = device
    .default_output_config()
    .map_err(|error| error.to_string());
  buffer_config(frames, supported).map(Some)
}

/// The web's stream takes no buffer size
#[cfg(target_arch = "wasm32")]
fn fixed_buffer_config(
  _frames: u32,
) -> Result<Option<(kira::backend::cpal::cpal::StreamConfig, u32)>, String> {
  Ok(None)
}

/// `supported` with its buffer fixed at `frames`, cut to its range, and the
/// frames it holds; the warning to give when `supported` could not be read
#[cfg(not(target_arch = "wasm32"))]
fn buffer_config(
  frames: u32,
  supported: Result<kira::backend::cpal::cpal::SupportedStreamConfig, String>,
) -> Result<FixedBuffer, String> {
  use kira::backend::cpal::cpal::BufferSize;
  let supported = supported.map_err(|error| {
    format!(
      "audio_buffer_frames = {frames} is not used: the audio device's configuration could not be read ({error}); it opens with its default buffer"
    )
  })?;
  let mut config = supported.config();
  config.buffer_size = fixed_buffer(frames, supported.buffer_size());
  let held = match config.buffer_size {
    BufferSize::Fixed(held) => held,
    BufferSize::Default => frames,
  };
  Ok((config, held))
}

/// The warning when the device will not open with a buffer of `frames`
fn buffer_refused(frames: u32, error: &dyn Display) -> String {
  format!(
    "the audio device would not open with a buffer of {frames} frames ({error}); it opens with its default buffer"
  )
}

/// A fixed buffer of `frames`, cut to `supported`'s range when it has one
#[cfg(not(target_arch = "wasm32"))]
fn fixed_buffer(
  frames: u32,
  supported: &kira::backend::cpal::cpal::SupportedBufferSize,
) -> kira::backend::cpal::cpal::BufferSize {
  use kira::backend::cpal::cpal::{BufferSize, SupportedBufferSize};
  match *supported {
    SupportedBufferSize::Range { min, max } if min <= max => {
      BufferSize::Fixed(frames.clamp(min, max))
    }
    _ => BufferSize::Fixed(frames),
  }
}

/// A clip's samples, taken from its store to play. It lives on the stack for
/// one play, and boxing the decoded variant would allocate on every play
#[allow(clippy::large_enum_variant)]
enum ClipSource {
  /// Decoded frames (an `Arc` inside: cheap to clone)
  Decoded(StaticSoundData),
  /// The file, to stream from at its sample rate
  Encoded { clip: EncodedClip, rate: u32 },
}

impl ClipSource {
  fn of(clip: &AudioClip) -> ClipSource {
    match clip.data() {
      ClipData::Decoded(data) => ClipSource::Decoded(StaticSoundData::clone(data)),
      ClipData::Encoded(encoded) => ClipSource::Encoded {
        clip: encoded.clone(),
        rate: clip.sample_rate(),
      },
    }
  }
}

/// What a voice plays
enum Source {
  /// A clip: played directly, picked by a sound definition, or a music's
  /// one clip
  Clip(Handle<AudioClip>),
  /// A music's intro and loop, joined into one sound as it loaded
  Joined(Handle<Music>),
}

impl Source {
  fn as_ref(&self) -> SourceRef<'_> {
    match self {
      Source::Clip(clip) => SourceRef::Clip(clip),
      Source::Joined(music) => SourceRef::Joined(music),
    }
  }
}

/// A [`Source`], borrowed for one play
#[derive(Clone, Copy)]
enum SourceRef<'a> {
  Clip(&'a Handle<AudioClip>),
  Joined(&'a Handle<Music>),
}

impl<'a> SourceRef<'a> {
  /// The source, held by a voice
  fn held(self) -> Source {
    match self {
      SourceRef::Clip(clip) => Source::Clip(clip.clone()),
      SourceRef::Joined(music) => Source::Joined(music.clone()),
    }
  }

  /// The clip, if it is one
  fn clip(self) -> Option<&'a Handle<AudioClip>> {
    match self {
      SourceRef::Clip(clip) => Some(clip),
      SourceRef::Joined(_) => None,
    }
  }

  fn untyped(self) -> UntypedAssetId {
    match self {
      SourceRef::Clip(clip) => clip.id().untyped(),
      SourceRef::Joined(music) => music.id().untyped(),
    }
  }

  /// Its content version in `world`'s stores; `None` if it is not loaded
  fn version(self, world: &World) -> Option<u64> {
    match self {
      SourceRef::Clip(clip) => world.get_resource::<Assets<AudioClip>>()?.version(clip),
      SourceRef::Joined(music) => {
        let musics = world.get_resource::<Assets<Music>>()?;
        let joined = matches!(musics.get(music)?.data(), MusicData::Concatenated(_));
        joined.then(|| musics.version(music)).flatten()
      }
    }
  }

  /// Its samples, taken from `world`'s stores to play, and their content
  /// version; `None` if it is not loaded
  fn samples(self, world: &World) -> Option<(ClipSource, u64)> {
    match self {
      SourceRef::Clip(clip) => {
        let clips = world.get_resource::<Assets<AudioClip>>()?;
        Some((ClipSource::of(clips.get(clip)?), clips.version(clip)?))
      }
      SourceRef::Joined(music) => {
        let musics = world.get_resource::<Assets<Music>>()?;
        let MusicData::Concatenated(data) = musics.get(music)?.data() else {
          return None;
        };
        let data = StaticSoundData::clone(data);
        Some((ClipSource::Decoded(data), musics.version(music)?))
      }
    }
  }

  /// Its name in the log and in warnings: the clip's asset path, or the
  /// music's (its id for one made in code)
  fn name(self, server: Option<&AssetServer>) -> String {
    match self {
      SourceRef::Clip(clip) => source_name(clip, server),
      SourceRef::Joined(music) => music_name(music.id(), server),
    }
  }
}

/// A live voice's kira handle
enum Playback {
  Static(StaticSoundHandle),
  #[cfg(not(target_arch = "wasm32"))]
  Stream(StreamingSoundHandle<StreamError>),
  /// A loop played before the web's unlock: nothing sounds until it starts
  /// there
  Waiting(Waiting),
}

/// A loop waiting for the web's unlock, as the game left it
#[derive(Debug, Clone, Copy)]
struct Waiting {
  /// What it fades in over once it sounds
  fade_in: Duration,
  paused: bool,
  stopped: bool,
}

impl Waiting {
  /// Waiting to fade in over `fade_in`, paused if its bus holds it (as a
  /// voice played while held starts paused)
  fn new(fade_in: Duration, held: bool) -> Self {
    Self {
      fade_in,
      paused: held,
      stopped: false,
    }
  }

  fn state(self) -> PlaybackState {
    if self.stopped {
      PlaybackState::Stopped
    } else if self.paused {
      PlaybackState::Paused
    } else {
      PlaybackState::Playing
    }
  }
}

impl Playback {
  fn state(&self) -> PlaybackState {
    match self {
      Playback::Static(sound) => sound.state(),
      #[cfg(not(target_arch = "wasm32"))]
      Playback::Stream(sound) => sound.state(),
      Playback::Waiting(waiting) => waiting.state(),
    }
  }

  /// What it fades in over once it sounds, if it waits for the unlock
  fn waiting_fade(&self) -> Option<Duration> {
    match self {
      Playback::Waiting(waiting) => Some(waiting.fade_in),
      _ => None,
    }
  }

  /// Fade out over `fade` and stop
  fn stop(&mut self, fade: Duration) {
    let tween = fade_to_silence(fade);
    match self {
      Playback::Static(sound) => sound.stop(tween),
      #[cfg(not(target_arch = "wasm32"))]
      Playback::Stream(sound) => sound.stop(tween),
      Playback::Waiting(waiting) => waiting.stopped = true,
    }
  }

  /// Fade out over `fade` and pause where it is
  fn pause(&mut self, fade: Duration) {
    let tween = fade_to_silence(fade);
    match self {
      Playback::Static(sound) => sound.pause(tween),
      #[cfg(not(target_arch = "wasm32"))]
      Playback::Stream(sound) => sound.pause(tween),
      Playback::Waiting(waiting) => waiting.paused = true,
    }
  }

  /// Resume where it paused, fading in over `fade`
  fn resume(&mut self, fade: Duration) {
    let tween = fade_from_silence(fade);
    match self {
      Playback::Static(sound) => sound.resume(tween),
      #[cfg(not(target_arch = "wasm32"))]
      Playback::Stream(sound) => sound.resume(tween),
      Playback::Waiting(waiting) => waiting.paused = false,
    }
  }

  fn set_volume(&mut self, db: f32, tween: Tween) {
    match self {
      Playback::Static(sound) => sound.set_volume(Decibels(db), tween),
      #[cfg(not(target_arch = "wasm32"))]
      Playback::Stream(sound) => sound.set_volume(Decibels(db), tween),
      Playback::Waiting(_) => {}
    }
  }

  fn set_panning(&mut self, pan: f32, tween: Tween) {
    match self {
      Playback::Static(sound) => sound.set_panning(Panning(pan), tween),
      #[cfg(not(target_arch = "wasm32"))]
      Playback::Stream(sound) => sound.set_panning(Panning(pan), tween),
      Playback::Waiting(_) => {}
    }
  }

  /// Where it is, in seconds from the start of what plays
  fn position(&self) -> f64 {
    match self {
      Playback::Static(sound) => sound.position(),
      #[cfg(not(target_arch = "wasm32"))]
      Playback::Stream(sound) => sound.position(),
      Playback::Waiting(_) => 0.0,
    }
  }

  fn set_rate(&mut self, rate: f64, tween: Tween) {
    match self {
      Playback::Static(sound) => sound.set_playback_rate(PlaybackRate(rate), tween),
      #[cfg(not(target_arch = "wasm32"))]
      Playback::Stream(sound) => sound.set_playback_rate(PlaybackRate(rate), tween),
      Playback::Waiting(_) => {}
    }
  }

  /// The first decode error a stream reported since the last call, the rest
  /// dropped (a stream that fails keeps failing until it stops)
  fn take_error(&mut self) -> Option<String> {
    match self {
      Playback::Static(_) | Playback::Waiting(_) => None,
      #[cfg(not(target_arch = "wasm32"))]
      Playback::Stream(sound) => {
        let error = sound.pop_error()?;
        while sound.pop_error().is_some() {}
        Some(error.to_string())
      }
    }
  }
}

/// A voice the engine is playing
struct LiveVoice {
  /// Shares its id and state with the game's handles
  voice: Voice,
  playback: Playback,
  /// Held, so the registry never frees a clip (or a joined music) under
  /// playback
  source: Source,
  /// The content version of its source that it plays: as it started, or
  /// as a reload last switched it
  version: u64,
  /// The definition it plays; `None` for a clip played directly, or music
  sound: Option<AssetId<Sound>>,
  /// Its definition's volume and pitch as its own gain and pitch last took
  /// them
  tuning: Tuning,
  /// The music it plays, if it is the music player's
  music: Option<AssetId<Music>>,
  bus: usize,
  /// Its sound's level in dB as it plays it: its definition's volume, the
  /// spread it drew and its play's options, moved by its definition's
  /// reloads
  gain_db: f32,
  /// Its sound's pitch in semitones, the same way
  pitch: f32,
  /// The game's change to its gain in dB (`Voice::set_volume`), added to
  /// its sound's level
  game_db: f32,
  /// The game's change to its pitch in semitones (`Voice::set_pitch`)
  game_pitch: f32,
  /// The time scale its rate carries: the game's for a voice on a bus that
  /// follows time scale, 1 otherwise
  scale: f64,
  /// It is held, apart from any pause of its own: by its bus (paused while
  /// the game is, or time stands still), or by its owner (`owner_held`)
  held: bool,
  /// Its owner holds it: its scene is not updated (below a top scene that
  /// holds the scenes below), or, played from an emitter, any scene is
  /// held. Only on a bus that pauses with the game
  owner_held: bool,
  /// kira's sound is paused, as kira was last told: it paused itself, or
  /// its bus holds it. At most one pause or resume goes to kira a frame
  kira_paused: bool,
  /// It started held and has not sounded yet: the fade-in it plays when it
  /// first does (its own, at least [`MIN_FADE`])
  unheard: Option<Duration>,
  /// The fade of the pause or resume the game asked of it this frame, for
  /// kira to hear in the frame's update
  own_fade: Option<Duration>,
  /// The scene that played it, which stops it as it exits; `None` for the
  /// engine's own (`persist`, or played outside any scene)
  scope: Option<u64>,
  /// How important it is (0 to 255)
  priority: u8,
  /// Where it plays in the world, if it was given a position
  position: Option<Vec2>,
  /// The entity it was played from, which it follows and belongs to: its
  /// loops stop when the entity goes
  emitter: Option<Entity>,
  /// How it is heard from its place, if its sound has spatial settings
  spatial: Option<SpatialVoice>,
  /// Its fade when the engine stops it under a limit
  fade_out: Duration,
  /// The engine frame it started in
  frame: u64,
  /// It loops until stopped, over `loop_region` of `region`, as it started
  looping: bool,
  /// How long what it plays lasts at its normal rate, in seconds: its
  /// region of its clip, or all of it
  length: f64,
  region: Option<Range<f64>>,
  loop_region: Option<Range<f64>>,
  /// The bus tree it plays on: the live one's, or a retired one's
  generation: u32,
  /// As a loop, it could not move to a rebuilt tree: warned once
  stuck_warned: bool,
}

impl LiveVoice {
  /// Who a log entry about it is about, as it stands
  fn subject(&self) -> Subject<'_> {
    Subject {
      sound: self.sound,
      music: self.music,
      source: self.source.as_ref(),
      bus: self.bus,
      gain_db: self.heard_db(),
      pan: self.pan(),
      rate: self.rate(),
    }
  }

  /// Its own gain: its sound's level and the game's change to it
  fn own_db(&self) -> f32 {
    clamp_gain(self.gain_db + self.game_db)
  }

  /// Its own pitch: its sound's and the game's change to it
  fn own_pitch(&self) -> f32 {
    clamp_pitch(self.pitch + self.game_pitch)
  }

  /// Its playback rate: its own pitch's, times the time scale it carries
  fn rate(&self) -> f64 {
    rate_of(self.own_pitch()) * self.scale
  }

  /// How long it has left to play, in seconds, at its rate: from where it
  /// is to the end of what it plays (a stream's position counts from its
  /// region's start, as its length does)
  fn remaining(&self) -> f64 {
    let left = (self.length - self.playback.position()).max(0.0);
    let rate = self.rate();
    if rate > 0.0 {
      left / rate
    } else {
      f64::INFINITY
    }
  }

  /// Its gain as heard: its own, plus its place's falloff
  fn heard_db(&self) -> f32 {
    heard_db(
      self.own_db(),
      self.spatial.as_ref().map_or(0.0, |spatial| spatial.gain_db),
    )
  }

  /// Its pan, from its place (0 without one)
  fn pan(&self) -> f32 {
    self.spatial.as_ref().map_or(0.0, |spatial| spatial.pan)
  }

  /// Its distance low-pass, if it has one
  fn filter(&self) -> Option<&VoiceFilter> {
    self.spatial.as_ref()?.filter.as_ref()
  }
}

/// The live voices, in the order they started (a retired voice's removal
/// keeps the others' order)
struct VoiceTable {
  live: Vec<LiveVoice>,
  next_id: u32,
}

impl VoiceTable {
  fn new() -> Self {
    Self {
      live: Vec::with_capacity(VOICE_CAPACITY),
      next_id: 1,
    }
  }

  /// A fresh id, never 0
  fn next_id(&mut self) -> u32 {
    let id = self.next_id;
    self.next_id = self.next_id.wrapping_add(1).max(1);
    id
  }

  fn index_of(&self, id: u32) -> Option<usize> {
    self.live.iter().position(|live| live.voice.id() == id)
  }
}

/// A definition's volume (dB) and pitch (semitones) as a voice's sound
/// level last took them: a reload of the definition moves each by its
/// change, keeping the spread the voice drew, its play's options and the
/// game's changes on top. `None` for a voice with no definition
#[derive(Debug, Clone, Copy, Default)]
struct Tuning {
  volume_db: Option<f32>,
  pitch: Option<f32>,
}

/// How far a voice's `part` of its definition (its volume or pitch, as it
/// last took it) moves to the definition's value `now`, which it takes; 0
/// for a voice with no definition (`None`)
fn follow_change(part: &mut Option<f32>, now: f32) -> f32 {
  part
    .as_mut()
    .map_or(0.0, |was| now - std::mem::replace(was, now))
}

/// What a definition that has played remembers between its plays
#[derive(Debug, Default)]
struct SoundState {
  /// Its latest picks (indices into its clips), oldest first, as many as
  /// its `no_repeat`
  recent: Vec<usize>,
  /// When it last started a voice, on the engine's clock
  last_start: Option<Duration>,
}

/// A voice a play stops to make room, and its fade
#[derive(Debug, Clone, Copy)]
struct Victim {
  index: usize,
  fade: Duration,
}

/// A play of a sound definition still loading. Until it lands it is not
/// known to loop, so every such play waits as a voice that reports playing:
/// once the sound is ready, a loop starts as it was played and a one-shot
/// is refused
struct Pending {
  voice: Voice,
  sound: Handle<Sound>,
  options: PlayOptions,
  /// The scene that played it, which ends it as it exits; `None` for the
  /// engine's own
  scope: Option<u64>,
  /// Played as one of the UI theme's sounds
  ui: bool,
  /// What the game set on it meanwhile (`Voice::set_volume`, `set_pitch`)
  game_db: f32,
  game_pitch: f32,
}

/// One play, resolved from a definition (or a bare clip's empty one)
struct Request<'a> {
  sound: Option<AssetId<Sound>>,
  /// The definition's volume and pitch, which its gain and pitch were drawn
  /// around
  tuning: Tuning,
  /// The music it plays, for the music player
  music: Option<AssetId<Music>>,
  source: SourceRef<'a>,
  /// Which of the definition's clips it is, for no-repeat
  pick: usize,
  no_repeat: u8,
  region: Option<&'a Range<f64>>,
  loop_region: Option<&'a Range<f64>>,
  looping: bool,
  bus: &'a str,
  gain_db: f32,
  pitch: f32,
  limit: Option<VoiceLimit>,
  priority: u8,
  cooldown: Duration,
  /// The most of its voices that start in a frame
  per_frame: u8,
  fade_in: Duration,
  fade_out: Duration,
  position: Option<Vec2>,
  /// How it is heard from `position`
  spatial: Option<Spatial>,
  /// The entity it is played from
  emitter: Option<Entity>,
  /// The engine's, not the playing scene's
  persist: bool,
  /// The voice it starts as: one that waited for its sound to load (it is
  /// not counted against its `per_frame` again); a new one when `None`
  voice: Option<Voice>,
  /// The game's changes to its gain and pitch, made while it waited
  game_db: f32,
  game_pitch: f32,
}

impl<'a> Request<'a> {
  /// `clip` played directly: the empty definition, plus `options`
  fn clip(clip: &'a Handle<AudioClip>, options: &PlayOptions) -> Self {
    Self {
      sound: None,
      tuning: Tuning::default(),
      music: None,
      source: SourceRef::Clip(clip),
      pick: 0,
      no_repeat: 0,
      region: None,
      loop_region: None,
      looping: false,
      bus: DEFAULT_BUS,
      gain_db: options.volume_db,
      pitch: options.pitch,
      limit: None,
      priority: Sound::DEFAULT_PRIORITY,
      cooldown: Duration::ZERO,
      per_frame: Sound::DEFAULT_PER_FRAME,
      fade_in: Duration::ZERO,
      fade_out: Duration::ZERO,
      position: options.position,
      spatial: None,
      emitter: None,
      persist: options.persist,
      voice: None,
      game_db: 0.0,
      game_pitch: 0.0,
    }
  }

  /// The definition `sound` (`id`), its clip `pick` chosen, its gain and
  /// pitch drawn, plus `options`
  fn sound(
    id: AssetId<Sound>,
    sound: &'a Sound,
    pick: usize,
    rng: &mut SmallRng,
    options: &PlayOptions,
  ) -> Option<Self> {
    let clip = sound.clips.get(pick)?;
    Some(Self {
      sound: Some(id),
      tuning: Tuning {
        volume_db: Some(sound.volume_db),
        pitch: Some(sound.pitch),
      },
      music: None,
      source: SourceRef::Clip(&clip.clip),
      pick,
      no_repeat: sound.no_repeat,
      region: clip.region.as_ref(),
      loop_region: clip.loop_region.as_ref(),
      looping: sound.looping,
      bus: &sound.bus,
      gain_db: sound.volume_db + spread(rng, sound.volume_range_db) + options.volume_db,
      pitch: sound.pitch + spread(rng, sound.pitch_range) + options.pitch,
      limit: sound.limit,
      priority: sound.priority,
      cooldown: sound.cooldown,
      per_frame: sound.per_frame,
      fade_in: sound.fade_in,
      fade_out: sound.fade_out,
      position: options.position,
      spatial: sound.spatial,
      emitter: None,
      persist: options.persist,
      voice: None,
      game_db: 0.0,
      game_pitch: 0.0,
    })
  }
}

/// How a voice starts in kira
struct Start<'a> {
  /// Its gain as heard (its own and its place's)
  gain_db: f32,
  pan: f32,
  /// Its playback rate: its pitch's, times the time scale it carries
  rate: f64,
  /// It starts held: silent, and paused before it plays (its fade-in is
  /// the resume's)
  held: bool,
  fade_in: Option<Tween>,
  region: Option<&'a Range<f64>>,
  loop_region: Option<&'a Range<f64>>,
  looping: bool,
  /// Where it starts, in seconds from the start of what plays
  position: f64,
}

impl Start<'_> {
  /// Its fade-in. A held start's waits, so it is silent until its resume
  /// fades it in, even if kira renders a chunk before its pause arrives
  fn fade_in(&self) -> Option<Tween> {
    if self.held {
      Some(Tween {
        start_time: StartTime::Delayed(Duration::MAX),
        duration: MIN_FADE,
        ..Default::default()
      })
    } else {
      self.fade_in
    }
  }

  /// `playback`, just started, paused at once if it starts held: kira
  /// reads the pause before it renders the sound, so it never moves
  fn hold(&self, mut playback: Playback) -> Playback {
    if self.held {
      playback.pause(Duration::ZERO);
    }
    playback
  }
}

/// Who a log entry is about
struct Subject<'a> {
  sound: Option<AssetId<Sound>>,
  music: Option<AssetId<Music>>,
  source: SourceRef<'a>,
  bus: usize,
  /// Its gain as heard (its own and its place's)
  gain_db: f32,
  pan: f32,
  /// Its playback rate, the time scale's included
  rate: f64,
}

/// What the log says refused a play: the rule, and why
const REFUSED_PER_FRAME: &str =
  "per_frame: the sound started as many voices this frame as its per_frame allows";
const REFUSED_COOLDOWN: &str = "cooldown: the sound played too recently";
const REFUSED_LIMIT: &str = "limit: a voice limit is full, with no voice the play may stop";
const REFUSED_FULL: &str = "full: voices fading out hold kira's room on the bus's track";
const REFUSED_UNDECODABLE: &str = "undecodable: the clip's stream could not be decoded";
const REFUSED_LOCKED: &str =
  "locked: the audio waits for the player's first gesture, and a one-shot does not wait";
const REFUSED_UNLOCK: &str = "unlock: the loop could not start at the player's first gesture";
const REFUSED_LOADING: &str =
  "loading: a one-shot played while its sound loads would only be late (a loop waits for it)";

/// Why a play did not start
enum Refusal {
  /// The bus's track has no room
  Full,
  /// A stream could not start
  Undecodable(String),
}

/// The engine's place in the registry's events for the assets it plays:
/// reloads switch what plays, and removals drop what it remembers
#[derive(Default)]
struct Reloads {
  clips: MessageCursor<AssetEvent<AudioClip>>,
  sounds: MessageCursor<AssetEvent<Sound>>,
  musics: MessageCursor<AssetEvent<Music>>,
  /// The clips and definitions a frame's events changed, kept to reuse
  changed_clips: Vec<AssetId<AudioClip>>,
  changed_sounds: Vec<AssetId<Sound>>,
}

/// What a voice starts again on: what it played (a move to rebuilt buses,
/// or its clip reloaded), or its music's definition as it now is
struct Replay {
  source: Source,
  region: Option<Range<f64>>,
  loop_region: Option<Range<f64>>,
  /// Its sound's level in dB (the game's change goes on top)
  gain_db: f32,
  bus: usize,
}

/// The engine's audio: kira's manager, the bus tree and the live voices, a
/// world resource. Scenes reach it through `ctx.audio()`; the voices'
/// commands are applied as the scene code that sent them ends, and in the
/// frame's `audio_system`. With no audio device (a
/// headless run, or a device that failed to open) it runs on kira's mock
/// backend: everything works, and nothing plays aloud.
///
/// On the web it opens the device at the player's first gesture (a key, a
/// mouse button or a touch), as browsers start a page's sound only inside
/// one. Until then a one-shot is dropped (logged as refused), while music
/// and loops wait and start at that gesture, each fading in over its own
/// fade-in. A gamepad does not count: browsers do not let a page start sound
/// on one, so a player with only a pad hears nothing until they click, tap
/// or press a key. A web game should open on a "click or tap to start"
/// screen, shown while [`AudioView::waiting_for_gesture`] says so
///
/// [`AudioView::waiting_for_gesture`]: super::AudioView::waiting_for_gesture
#[derive(Resource)]
pub struct AudioEngine {
  /// `None` before the web's unlock (while `locked`), or if even the mock
  /// backend failed to start (it never does): every play is then refused
  manager: Option<Manager>,
  /// Locked: the web, waiting for its first user gesture, and the manager
  /// it opens then ([`unlock`](Self::unlock)). `None` once open
  locked: Option<Unlock>,
  buses: BusTree,
  voices: VoiceTable,
  log: AudioLog,
  /// Cloned into each voice
  commands: Sender<VoiceCommand>,
  inbox: Receiver<VoiceCommand>,
  /// Mock frames owed, in frame-nanoseconds (frames times 10^9), so no
  /// fraction of a frame is lost between frames
  mock_owed: u128,
  /// The wall time the last update spent rendering on the mock backend
  mock_render: Duration,
  /// The device's stream reported an error: later ones are not warned about
  /// (the web's stream reports none)
  #[cfg(any(test, not(target_arch = "wasm32")))]
  stream_error_warned: bool,
  /// Clips whose stream failed to decode, with the content version that
  /// failed: warned about once, and refused until they change (reload)
  undecodable: HashMap<AssetId<AudioClip>, u64>,
  /// Picks clips and draws gains and pitches
  rng: SmallRng,
  /// The real time its frames have counted, for cooldowns
  clock: Duration,
  /// Its frames so far: a voice is not stolen in the frame it started by an
  /// equally important play
  frame: u64,
  /// What each definition played remembers
  sounds: FastMap<AssetId<Sound>, SoundState>,
  /// Definitions warned about once (one that did not load, or has no clips)
  warned_sounds: FastSet<AssetId<Sound>>,
  /// Bus names warned about once (a bus there is not)
  warned_buses: FastSet<String>,
  /// The listener as last found: none (every voice is heard as if at it)
  /// without an `AudioListener` or a main camera
  listener: Option<Listener>,
  /// Finds the listener; made from the world the engine is in
  listener_query: Option<ListenerQuery>,
  /// Emitters added to entities, to play on spawn once their sounds load
  spawned: Vec<Entity>,
  /// The entities played from `spawned` in a frame: one queued twice (an
  /// emitter spawned and a pool's put back and handed out in one frame, say)
  /// plays once. Kept to reuse
  spawned_seen: EntityHashSet,
  /// Voices whose emitter went (despawned, disabled, taken away or put
  /// over), by id, to release in the frame's update. Kept to reuse
  orphans: Vec<u32>,
  /// The world's entities being enabled are watched, for their emitters
  watching_enabled: bool,
  /// Sounds and clips played at a place without spatial settings, warned
  /// about once
  warned_flat: FastSet<UntypedAssetId>,
  /// The voices one play stops to make room, kept to reuse
  victims: Vec<Victim>,
  /// The voices each definition (or clip played directly) started this
  /// frame, for their `per_frame`. Kept to reuse
  frame_starts: Vec<(UntypedAssetId, u8)>,
  /// Plays of definitions still loading, waiting to start
  pending: Vec<Pending>,
  /// Definitions and clips a one-shot was played of while they loaded,
  /// warned about once (debug builds)
  warned_loading: FastSet<UntypedAssetId>,
  /// The player's six volume settings
  settings: VolumeSettings,
  /// The volumes the game set on buses (`set_bus_volume`), in dB, by name:
  /// kept through mixer reloads, and given to a bus the mixer adds later
  bus_volumes: FastMap<String, f32>,
  /// Bus names whose game volume was warned about once (no such bus)
  warned_volumes: FastSet<String>,
  /// The mixer's snapshots, as of its last build
  snapshots: Vec<SnapshotDef>,
  /// The active snapshot and the blend toward it
  blend: Blend,
  /// A snapshot (or `None`, a clear) asked for before the mixer landed
  pending_snapshot: Option<Option<String>>,
  /// Snapshot names warned about once (there is none by that name)
  warned_snapshots: FastSet<String>,
  /// The mixer's ducking rules, as of its last build
  ducks: Vec<DuckState>,
  /// The smoothing of bus gain and cutoff changes: [`MIN_FADE`], or a slow
  /// frame's length while the mix steps
  glide: Duration,
  /// The mixer the bus tree is built from: `None` is the standard layout
  mixer: Option<Handle<Mixer>>,
  /// The mixer's content version the tree was last built from
  mixer_version: Option<u64>,
  /// The mixer has not landed yet: a bus it may add is not warned about
  mixer_pending: bool,
  /// The bus tree's generation, counted at each rebuild
  generation: u32,
  /// A retired tree's send tracks, by its generation, held while voices on
  /// it play so their sends still reach them
  retired_sends: Vec<(u32, Vec<SendTrackHandle>)>,
  /// The time scale the following voices' rates carry: the latest above
  /// [`HOLD_SCALE`] (a held voice keeps the rate it had)
  rate_scale: f64,
  /// The game's clock as the last update followed it: a tree rebuilt
  /// between updates holds its buses by it at once
  game: GameTime,
  /// The holds or the time scale changed, or the tree was rebuilt: the
  /// voices are brought in line in the frame's update
  voices_dirty: bool,
  /// Whether the game's window has focus
  focused: bool,
  /// What the mix does while it has not: the mixer's rule
  focus_loss: FocusLoss,
  /// The scene whose code runs now: the voices it plays are its own
  scope: Option<u64>,
  /// The scenes the scene stack does not update, under a top scene that
  /// holds them: their voices, and the emitters', are held
  /// ([`hold_scopes`](Self::hold_scopes))
  held_scopes: Vec<u64>,
  /// The one music player: the track playing, and one waiting to load
  music: MusicPlayer,
  /// Where it is in the registry's events for what it plays
  reloads: Reloads,
  #[cfg(test)]
  injected_stream_errors: Vec<String>,
  /// Where the streams started count the frames they decode (tests)
  #[cfg(test)]
  stream_decoding: Option<Arc<std::sync::atomic::AtomicUsize>>,
  /// Plays kira refused for want of room: none should ever be
  #[cfg(test)]
  kira_full: usize,
}

impl AudioEngine {
  /// The engine on the default audio device (`device`), falling back to the
  /// mock backend with one warning if it fails to open; on the mock
  /// backend at 48 kHz otherwise
  #[cfg(any(test, not(target_arch = "wasm32")))]
  pub(crate) fn new(device: bool) -> AudioEngine {
    Self::open(device, None)
  }

  /// [`AudioEngine::new`], the device's buffer fixed at `buffer_frames`
  /// when given (`Ryo.toml`'s `audio_buffer_frames`). The web opens its
  /// device at the first user gesture instead ([`unlock`](Self::unlock))
  pub(crate) fn open(device: bool, buffer_frames: Option<u32>) -> AudioEngine {
    match (device, cfg!(target_arch = "wasm32")) {
      (false, _) => Self::with_manager(Manager::mock()),
      (true, false) => Self::with_manager(Manager::device(buffer_frames)),
      (true, true) => Self::locked_until(Unlock::Device(buffer_frames)),
    }
  }

  /// The engine as it is when the device fails to open (tests)
  #[cfg(test)]
  pub(crate) fn new_failing_device() -> AudioEngine {
    Self::with_manager(Manager::fallback(&FAILING_DEVICE))
  }

  /// The engine on the mock backend, with a meter on what it renders
  /// (tests)
  #[cfg(test)]
  pub(crate) fn new_metered() -> (AudioEngine, Meter) {
    let meter = Meter::default();
    let main = MainTrackBuilder::new().with_built_effect(Box::new(meter.clone()));
    (Self::with_manager(Manager::mock_with(main)), meter)
  }

  /// The engine as the web starts it, waiting for the first user gesture,
  /// whose unlock opens the mock backend (tests)
  #[cfg(test)]
  pub(crate) fn new_locked() -> AudioEngine {
    Self::locked_until(Unlock::Mock)
  }

  /// The engine with nothing open, locked until the web's first user
  /// gesture opens `unlock`'s manager
  fn locked_until(unlock: Unlock) -> AudioEngine {
    let mut engine = Self::with_manager(None);
    engine.locked = Some(unlock);
    engine
  }

  /// Whether it waits for the web's first user gesture to open
  pub(crate) fn is_locked(&self) -> bool {
    self.locked.is_some()
  }

  /// The web's first user gesture: open the manager it waited for, inside
  /// the gesture's event, and build the buses on it from the mixer (the
  /// standard layout while the mixer has yet to land). The music and loops
  /// played before it start, each fading in over its own fade-in. An engine
  /// already open is left alone, so only the first gesture opens it
  pub(crate) fn unlock(&mut self, world: &World) {
    let Some(unlock) = self.locked.take() else {
      return;
    };
    self.manager = unlock.open();
    // kira's main track starts at 0 dB: the rebuild gives it master's gain
    if let Some(master) = self.buses.buses.first_mut() {
      master.applied = 0.0;
    }
    self.find_listener(world);
    let server = world.get_resource::<AssetServer>();
    // The mixer the buses were built from, if it landed
    let landed = self
      .mixer
      .as_ref()
      .map(Handle::id)
      .filter(|_| self.mixer_version.is_some())
      .and_then(|id| world.get_resource::<Assets<Mixer>>()?.get(id))
      .filter(|mixer| mixer.check().is_ok());
    match landed {
      Some(mixer) => self.rebuild(world, mixer, server),
      None => self.rebuild(world, &Mixer::default(), server),
    }
  }

  fn with_manager(mut manager: Option<Manager>) -> AudioEngine {
    let settings = VolumeSettings::default();
    let buses = BusTree::standard(
      manager.as_mut().map(|manager| manager as &mut dyn Tracks),
      |planned| {
        layers_of(
          planned,
          &settings,
          &FastMap::default(),
          &Blend::default(),
          &[],
          0.0,
        )
      },
    );
    let (commands, inbox) = crossbeam_channel::unbounded();
    let mut engine = AudioEngine {
      manager,
      locked: None,
      buses,
      voices: VoiceTable::new(),
      log: AudioLog::default(),
      commands,
      inbox,
      mock_owed: 0,
      mock_render: Duration::ZERO,
      #[cfg(any(test, not(target_arch = "wasm32")))]
      stream_error_warned: false,
      undecodable: HashMap::new(),
      rng: SmallRng::from_entropy(),
      clock: Duration::ZERO,
      frame: 0,
      sounds: FastMap::default(),
      warned_sounds: FastSet::default(),
      warned_buses: FastSet::default(),
      listener: None,
      listener_query: None,
      spawned: Vec::new(),
      spawned_seen: EntityHashSet::default(),
      orphans: Vec::new(),
      watching_enabled: false,
      warned_flat: FastSet::default(),
      victims: Vec::new(),
      frame_starts: Vec::new(),
      pending: Vec::new(),
      warned_loading: FastSet::default(),
      settings,
      bus_volumes: FastMap::default(),
      warned_volumes: FastSet::default(),
      snapshots: Vec::new(),
      blend: Blend::default(),
      pending_snapshot: None,
      warned_snapshots: FastSet::default(),
      ducks: Vec::new(),
      glide: MIN_FADE,
      mixer: None,
      mixer_version: None,
      mixer_pending: false,
      generation: 0,
      retired_sends: Vec::new(),
      rate_scale: 1.0,
      game: GameTime::default(),
      voices_dirty: false,
      focused: true,
      focus_loss: FocusLoss::Keep,
      scope: None,
      held_scopes: Vec::new(),
      music: MusicPlayer::default(),
      reloads: Reloads::default(),
      #[cfg(test)]
      injected_stream_errors: Vec::new(),
      #[cfg(test)]
      stream_decoding: None,
      #[cfg(test)]
      kira_full: 0,
    };
    // master's gain goes on kira's main track
    engine.refresh_gains();
    engine
  }

  /// Record each voice's start, stop and changes in the audio log (off by
  /// default; `HeadlessEngine` turns it on)
  #[doc(hidden)]
  pub fn set_logging(&mut self, on: bool) {
    self.log.set_on(on);
  }

  /// The audio log's latest entries, oldest first
  #[doc(hidden)]
  pub fn log(&self) -> &[AudioLogEntry] {
    self.log.entries()
  }

  /// The wall time the last frame's update spent rendering on the mock
  /// backend (zero on a device, which renders on its own thread): the
  /// audio benches leave it out of the frame's main-thread time
  #[doc(hidden)]
  pub fn mock_render_time(&self) -> Duration {
    self.mock_render
  }

  /// Seed the generator that picks clips and draws gains and pitches, so a
  /// run picks the same way each time (`AudioView::seed`; headless engines
  /// start at [`HEADLESS_SEED`])
  pub(crate) fn seed(&mut self, seed: u64) {
    self.rng = SmallRng::seed_from_u64(seed);
  }

  /// Limit the voices on `bus` and the buses under it (tests, until the
  /// mixer sets bus limits)
  #[cfg(test)]
  pub(crate) fn set_bus_limit(&mut self, bus: &str, max: Option<u16>) {
    if let Some(bus) = self.buses.find(bus) {
      self.buses.buses[bus].limit = max;
    }
  }

  /// How many entries the engine keeps about assets: definitions' recent
  /// picks and cooldowns, clips' failed decodes, and the warnings it gave
  /// once (tests)
  #[cfg(test)]
  pub(crate) fn remembered(&self) -> usize {
    self.sounds.len()
      + self.warned_sounds.len()
      + self.warned_flat.len()
      + self.undecodable.len()
      + self.music.warned.len()
      + self.music.warned_buses.len()
      + self.warned_loading.len()
  }

  /// Count the frames every stream started from now on decodes (tests)
  #[cfg(test)]
  pub(crate) fn count_stream_decoding(&mut self) -> Arc<std::sync::atomic::AtomicUsize> {
    let decoded = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    self.stream_decoding = Some(Arc::clone(&decoded));
    decoded
  }

  /// How many plays kira refused for want of room (tests: it never should)
  #[cfg(test)]
  pub(crate) fn kira_full(&self) -> usize {
    self.kira_full
  }

  /// Whether it runs on the mock backend (tests)
  #[cfg(test)]
  pub(crate) fn is_mock(&self) -> bool {
    matches!(self.manager, Some(Manager::Mock(_)))
  }

  /// Pretend the device's stream reported `error` (tests)
  #[cfg(test)]
  pub(crate) fn inject_stream_error(&mut self, error: &str) {
    self.injected_stream_errors.push(error.to_string());
  }

  /// Build the bus tree from `mixer` once it lands, and again each time it
  /// changes (a reload); `None` builds nothing more (the engine sets it once,
  /// at start)
  pub(crate) fn set_mixer(&mut self, mixer: Option<Handle<Mixer>>) {
    self.mixer_pending = mixer.is_some();
    self.mixer = mixer;
    self.mixer_version = None;
  }

  /// The game's window gained (`true`) or lost focus: `master` follows the
  /// mixer's `on_focus_loss` (`Keep` by default, or `Duck`, or `Mute`),
  /// smoothed over [`MIN_FADE`]
  pub(crate) fn set_focused(&mut self, focused: bool) {
    if self.focused == focused {
      return;
    }
    self.focused = focused;
    self.refresh_focus();
  }

  /// Put the focus loss's layer on `master`, as the window's focus and the
  /// mixer's rule now say
  fn refresh_focus(&mut self) {
    let db = focus_db(self.focused, self.focus_loss);
    if let Some(master) = self.buses.buses.first_mut() {
      master.layers.focus = db;
    }
    self.refresh_gains();
  }

  /// Make `scope` (a scene's) the one whose code runs, returning the one
  /// before: the voices played until the next change are its own, and
  /// stop when it exits. `None` is the engine's own code
  pub(crate) fn set_scope(&mut self, scope: Option<u64>) -> Option<u64> {
    std::mem::replace(&mut self.scope, scope)
  }

  /// Hold the voices of the scenes in `scopes` (those the scene stack does
  /// not update, under a top scene that holds them), and the emitters'
  /// while there are any, on the buses that pause with the game; let go of
  /// the rest. Each held voice pauses where it is over [`MIN_FADE`] in the
  /// frame's update, and resumes from there once its owner lets go and its
  /// bus does too. A voice its owner holds as it starts starts held
  pub(crate) fn hold_scopes(&mut self, scopes: impl Iterator<Item = u64> + Clone) {
    if self.held_scopes.iter().copied().eq(scopes.clone()) {
      return;
    }
    self.held_scopes.clear();
    self.held_scopes.extend(scopes);
    self.voices_dirty = true;
  }

  /// Scene `scope` exited: stop the loops it played, the one-shots paused
  /// (by their own call) or held (by the game's pause, or time standing
  /// still), and the one-shots playing with more than [`FINISH_AT_EXIT`]
  /// left (but on `ui`), each with its fade (its sound's `fade_out`, at
  /// least [`MIN_FADE`]). Its one-shots nearly over, and those on `ui`,
  /// finish (a confirm click just before a switch is heard out): a paused
  /// one would otherwise play its rest over the next scene once the clock
  /// resumes, and a long one run on into it. The voices played with
  /// `persist`, and the engine's own, play on
  pub(crate) fn release_scope(&mut self, world: &World, scope: u64) {
    let server = world.get_resource::<AssetServer>();
    for index in 0..self.voices.live.len() {
      if self.voices.live[index].scope == Some(scope) {
        self.release_voice(index, true, server);
      }
    }
    // Its plays still waiting for their sounds never start
    self.pending.retain(|pending| {
      let leaves = pending.scope == Some(scope);
      if leaves {
        pending.voice.set_state(VoiceState::Stopped);
      }
      !leaves
    });
  }

  /// Live voice `index`'s owner is gone (its scene exited, or its emitter's
  /// entity): stop it with its fade (its sound's `fade_out`, at least
  /// [`MIN_FADE`]) if it is a loop, or a one-shot paused or held; a one-shot
  /// playing plays on to its end, but at a `scene_exit` only one with at
  /// most [`FINISH_AT_EXIT`] left, or one on `ui`. An emitter's one-shot
  /// plays out where it was (a death cry as its entity despawns)
  fn release_voice(&mut self, index: usize, scene_exit: bool, server: Option<&AssetServer>) {
    let ui = self.buses.find("ui");
    let Some(live) = self.voices.live.get(index) else {
      return;
    };
    let long = scene_exit
      && !live.looping
      && ui.is_none_or(|ui| !self.buses.within(live.bus, ui))
      && live.remaining() > FINISH_AT_EXIT.as_secs_f64();
    let Some(live) = self.voices.live.get_mut(index) else {
      return;
    };
    let state = live.voice.state();
    let lingers = live.looping || live.held || state == VoiceState::Paused || long;
    if !lingers || !matches!(state, VoiceState::Playing | VoiceState::Paused) {
      return;
    }
    let fade = live.fade_out.max(MIN_FADE);
    live.playback.stop(fade);
    live.voice.set_state(VoiceState::Stopping);
    self.record(index, AudioLogKind::Stop, fade, server);
  }

  /// Make ready to find the listener in `world` (once; again for another
  /// world)
  pub(crate) fn prepare(&mut self, world: &mut World) {
    if !self
      .listener_query
      .as_ref()
      .is_some_and(|query| query.is_of(world))
    {
      self.listener_query = Some(ListenerQuery::new(world));
    }
  }

  /// Find the listener in `world`, if [`prepare`](Self::prepare)d for it
  fn find_listener(&mut self, world: &World) {
    if let Some(query) = &mut self.listener_query
      && query.is_of(world)
    {
      self.listener = query.find(world);
    }
  }

  /// An emitter that plays on spawn was added to `entity`: it plays in the
  /// frame's update, once its sound has loaded
  pub(crate) fn emitter_spawned(&mut self, entity: Entity) {
    self.spawned.push(entity);
  }

  /// True the first time it is asked: the world's first emitter is being
  /// added, and the engine should start watching for entities being
  /// disabled and enabled
  pub(crate) fn watch_enabling(&mut self) -> bool {
    !std::mem::replace(&mut self.watching_enabled, true)
  }

  /// `entity`'s emitter went (despawned, disabled, taken away or put over
  /// by another): the voices it played no longer follow it, and are
  /// released in the frame's update. Voices played from it afterwards (its
  /// next emitter's) are its own again
  pub(crate) fn emitter_gone(&mut self, entity: Entity) {
    for live in &mut self.voices.live {
      if live.emitter == Some(entity) {
        live.emitter = None;
        self.orphans.push(live.voice.id());
      }
    }
  }

  /// Release the voices whose emitter went, as a scene's exit would: a loop
  /// stops with its fade, and a one-shot playing plays out where the
  /// emitter was
  fn release_orphans(&mut self, server: Option<&AssetServer>) {
    if self.orphans.is_empty() {
      return;
    }
    let mut orphans = std::mem::take(&mut self.orphans);
    for &id in &orphans {
      if let Some(index) = self.voices.index_of(id) {
        self.release_voice(index, false, server);
      }
    }
    orphans.clear();
    self.orphans = orphans;
  }

  /// Each bus as it stands, `master` first and parents before children
  #[doc(hidden)]
  pub fn buses(&self) -> Vec<BusReport> {
    self.buses.report()
  }

  /// Set the player's volume setting `kind` (sanitized: NaN is its
  /// default, the rest clamped to 0..=1), its bus following at once
  pub(crate) fn set_volume_setting(&mut self, kind: VolumeKind, value: f32) {
    let value = self.settings.set(kind, value);
    if let Some(bus) = self.buses.buses.get_mut(kind.index()) {
      bus.layers.setting = volume_to_db(value);
    }
    self.refresh_gains();
  }

  pub(crate) fn volume_setting(&self, kind: VolumeKind) -> f32 {
    self.settings.get(kind)
  }

  /// Set `bus`'s volume in dB in place of the mixer's (NaN is 0 dB, the
  /// rest clamped from silence to +24), for the session: a bus the mixer
  /// adds later takes it too. A bus there is not is warned about once,
  /// unless the mixer has yet to land
  pub(crate) fn set_bus_volume(&mut self, bus: &str, db: f32) {
    let db = if db.is_nan() {
      0.0
    } else {
      db.clamp(SILENCE_DB, MAX_BUS_DB)
    };
    match self.bus_volumes.get_mut(bus) {
      Some(volume) => *volume = db,
      None => {
        self.bus_volumes.insert(bus.to_string(), db);
      }
    }
    match self.buses.find(bus) {
      Some(index) => {
        self.buses.buses[index].layers.base = db;
        self.refresh_gains();
      }
      None if !self.mixer_pending => self.warn_volume(bus),
      None => {}
    }
  }

  /// Warn, once for `bus`, that the game set the volume of a bus there is
  /// not
  fn warn_volume(&mut self, bus: &str) {
    if self.warned_volumes.contains(bus) {
      return;
    }
    self.warned_volumes.insert(bus.to_string());
    crate::engine_warn!(
      module: "Audio",
      "set_bus_volume: there is no bus {bus:?}; its volume applies if the mixer adds it"
    );
  }

  /// Give each bus's kira track the volume its gain layers add up to, and
  /// each send route the gains above its sender, where they changed,
  /// smoothed over [`MIN_FADE`]; each bus's change is logged
  fn refresh_gains(&mut self) {
    for index in 0..self.buses.buses.len() {
      let volume = self.buses.kira_volume(index);
      let bus = &mut self.buses.buses[index];
      if (volume - bus.applied).abs() < GAIN_EPSILON {
        continue;
      }
      bus.applied = volume;
      let smooth = tween(self.glide);
      match &mut bus.output {
        _ if index == 0 => {
          if let Some(manager) = &mut self.manager {
            manager.set_main_volume(volume, smooth);
          }
        }
        Output::Track(Some(track)) => track.set_volume(Decibels(volume), smooth),
        Output::Send(Some(send)) => send.set_volume(Decibels(volume), smooth),
        Output::Track(None) | Output::Send(None) => {}
      }
      self.record_bus(index, AudioLogKind::Gain, self.glide);
    }
    // The send routes, which carry the gains above their senders
    for index in 0..self.buses.buses.len() {
      for route in 0..self.buses.buses[index].sends.len() {
        let Route { to, level, applied } = self.buses.buses[index].sends[route];
        let volume = self.buses.route_volume(index, level);
        if (volume - applied).abs() < GAIN_EPSILON {
          continue;
        }
        let to = self.buses.send_id(to);
        let bus = &mut self.buses.buses[index];
        bus.sends[route].applied = volume;
        if let (Some(to), Output::Track(Some(track))) = (to, &mut bus.output) {
          // Every route was built with the track, so it exists
          let _ = track.set_send(to, Decibels(volume), tween(self.glide));
        }
      }
    }
  }

  /// Blend to the mixer's snapshot `name` over its blend time, from where
  /// the mix is. One is active at a time. A name the mixer lacks is warned
  /// about once and changes nothing; before the mixer lands, the call waits
  /// for it
  pub(crate) fn snapshot(&mut self, name: &str) {
    if self.mixer_pending {
      self.pending_snapshot = Some(Some(name.to_string()));
    } else {
      self.activate(Some(name));
    }
  }

  /// Blend back from the active snapshot over its blend time
  pub(crate) fn clear_snapshot(&mut self) {
    if self.mixer_pending {
      self.pending_snapshot = Some(None);
    } else {
      self.activate(None);
    }
  }

  /// Start blending to snapshot `name`, or to no snapshot
  fn activate(&mut self, name: Option<&str>) {
    match name {
      Some(name) => {
        let Some(def) = self.snapshots.iter().find(|snapshot| snapshot.name == name) else {
          self.warn_snapshot(name);
          return;
        };
        let targets = def.buses.iter().map(|(bus, target)| {
          let level = Level {
            db: target.volume_db,
            hz: target.low_pass.unwrap_or(OPEN_HZ),
          };
          (bus.as_str(), level)
        });
        self.blend.begin(Some(name), def.blend, targets);
      }
      None => {
        if self.blend.active.is_none() {
          return;
        }
        let seconds = self.blend.seconds();
        self.blend.begin(None, seconds, std::iter::empty());
      }
    }
    // A blend of no time lands in this call
    self.step_blend(0.0);
  }

  /// Warn, once for `name`, that the mixer has no such snapshot
  fn warn_snapshot(&mut self, name: &str) {
    if !self.warned_snapshots.insert(name.to_string()) {
      return;
    }
    crate::engine_warn!(
      module: "Audio",
      "snapshot: the mixer has no snapshot {name:?}; the mix stays as it is"
    );
  }

  /// Move the snapshot's blend on by `seconds` and put its levels on the
  /// buses
  fn step_blend(&mut self, seconds: f32) {
    if !self.blend.step(seconds) {
      return;
    }
    for (name, level) in self.blend.levels() {
      if let Some(index) = self.buses.find(name) {
        self.buses.buses[index].layers.snapshot = level.db;
      }
    }
    self.blend.settle();
    self.refresh_gains();
    self.refresh_cutoffs();
  }

  /// Each frame: the snapshot's blend, and the ducks. A duck dips while any
  /// voice plays on its trigger bus or under it
  fn step_mix(&mut self, delta: Duration) {
    let seconds = delta.as_secs_f32();
    // A slow frame glides over the whole frame, so a blend does not step
    self.glide = delta.max(MIN_FADE);
    self.step_mix_by(seconds);
    self.glide = MIN_FADE;
  }

  fn step_mix_by(&mut self, seconds: f32) {
    self.step_blend(seconds);
    let mut moved = false;
    for duck in &mut self.ducks {
      let trigger = duck.trigger;
      // A voice its bus holds is not heard, so it ducks nothing
      let playing = self.voices.live.iter().any(|live| {
        live.voice.state() == VoiceState::Playing
          && !live.held
          && self.buses.within(live.bus, trigger)
      });
      moved |= duck.step(playing, seconds);
    }
    if !moved {
      return;
    }
    for duck in &self.ducks {
      let depth: f32 = self
        .ducks
        .iter()
        .filter(|other| other.target == duck.target)
        .map(|other| other.depth)
        .sum();
      if let Some(bus) = self.buses.buses.get_mut(duck.target) {
        bus.layers.duck = depth;
      }
    }
    self.refresh_gains();
  }

  /// Give each snapshot filter the cutoff the snapshot blend is at, where
  /// it changed, smoothed over [`MIN_FADE`]
  fn refresh_cutoffs(&mut self) {
    let glide = self.glide;
    for bus in &mut self.buses.buses {
      let Some(cutoff) = &mut bus.cutoff else {
        continue;
      };
      let hz = self.blend.level(&bus.name).hz;
      if (hz - cutoff.applied).abs() < CUTOFF_EPSILON {
        continue;
      }
      cutoff.applied = hz;
      if let Some(handle) = &mut cutoff.handle {
        handle.set_cutoff(f64::from(hz), tween(glide));
      }
    }
  }

  /// Set `bus`'s snapshot and duck layers (tests, until snapshots and
  /// ducking set them)
  #[cfg(test)]
  pub(crate) fn set_bus_layers(&mut self, bus: &str, snapshot_db: f32, duck_db: f32) {
    if let Some(index) = self.buses.find(bus) {
      let layers = &mut self.buses.buses[index].layers;
      layers.snapshot = snapshot_db;
      layers.duck = duck_db;
      self.refresh_gains();
    }
  }

  /// kira's tracks under its main track, and its send tracks (tests)
  #[cfg(test)]
  pub(crate) fn kira_tracks(&self) -> (usize, usize) {
    self.manager.as_ref().map_or((0, 0), Manager::track_counts)
  }

  /// Where voice `id` is, in seconds from the start of what it plays
  /// (tests)
  #[cfg(test)]
  pub(crate) fn voice_position(&self, id: u32) -> Option<f64> {
    let index = self.voices.index_of(id)?;
    Some(self.voices.live[index].playback.position())
  }

  /// What kira says voice `id`'s playback is doing (tests)
  #[cfg(test)]
  pub(crate) fn voice_playback(&self, id: u32) -> Option<PlaybackState> {
    let index = self.voices.index_of(id)?;
    Some(self.voices.live[index].playback.state())
  }

  /// The cutoff voice `id`'s distance low-pass was last given; `None` for a
  /// voice without one (tests)
  #[cfg(test)]
  pub(crate) fn voice_cutoff(&self, id: u32) -> Option<f64> {
    let index = self.voices.index_of(id)?;
    self.voices.live[index]
      .filter()
      .map(|filter| filter.applied)
  }

  /// kira's tracks under `bus`'s own (tests)
  #[cfg(test)]
  pub(crate) fn sub_tracks_on(&self, bus: &str) -> usize {
    self
      .buses
      .find(bus)
      .and_then(|bus| self.buses.track(bus))
      .map_or(0, TrackHandle::num_sub_tracks)
  }

  /// The sounds kira holds on `bus`'s own track (tests)
  #[cfg(test)]
  pub(crate) fn sounds_on(&self, bus: &str) -> usize {
    self
      .buses
      .find(bus)
      .and_then(|bus| self.buses.track(bus))
      .map_or(0, TrackHandle::num_sounds)
  }

  /// Build the bus tree from the mixer now if it landed or changed since it
  /// was last built (its version gates it, so this mostly does nothing):
  /// what is asked of the engine next sees its buses
  pub(crate) fn mixer_now(&mut self, world: &World) {
    let server = world.get_resource::<AssetServer>();
    self.apply_mixer(world, server);
  }

  /// Rebuild the bus tree once the mixer lands or changes. A mixer that
  /// fails its check (one made in code; a file's failed to load) keeps the
  /// tree as it is, with a warning
  fn apply_mixer(&mut self, world: &World, server: Option<&AssetServer>) {
    let Some(id) = self.mixer.as_ref().map(Handle::id) else {
      return;
    };
    let mixers = world.get_resource::<Assets<Mixer>>();
    let Some((mixer, version)) =
      mixers.and_then(|mixers| Some((mixers.get(id)?, mixers.version(id)?)))
    else {
      // A mixer that failed to load leaves the standard layout, and the
      // buses it would have added are warned about from now on
      if self.mixer_pending && server.is_some_and(|server| server.state(id).is_failed()) {
        self.mixer_pending = false;
      }
      return;
    };
    if self.mixer_version == Some(version) {
      return;
    }
    self.mixer_version = Some(version);
    self.mixer_pending = false;
    if let Err(error) = mixer.check() {
      let name = server
        .and_then(|server| server.path(id))
        .map_or_else(|| format!("{id:?}"), |path| path.to_string());
      crate::engine_warn!(
        module: "Audio",
        "{name}: the mixer is not used ({error}); the buses stay as they were"
      );
      return;
    }
    self.rebuild(world, mixer, server);
  }

  /// Build a new bus tree from `mixer` and retire the old one: voices move
  /// to the new tree's bus their definition names, or a clip's to the bus
  /// of its old bus's name (or the nearest above either);
  /// one-shots play out on the old tracks, which kira keeps until their
  /// sounds finish, and loops restart on the new ones at their position,
  /// crossfading over [`MIN_FADE`] (kira cannot move a sound between tracks,
  /// and fixes a track's effects and sends when it builds it). A retired
  /// tree keeps the volumes it had: only `master`, on kira's main track,
  /// still reaches what plays out there
  fn rebuild(&mut self, world: &World, mixer: &Mixer, server: Option<&AssetServer>) {
    let planned = plan(mixer);
    // The new rules first: the tree is built with the dips the old ones
    // have reached, so an active duck is not lost
    self.snapshots.clone_from(&mixer.snapshots);
    let old_ducks = std::mem::take(&mut self.ducks);
    self.ducks = mixer
      .ducks
      .iter()
      .map(|def| DuckState {
        def: def.clone(),
        trigger: 0,
        target: 0,
        depth: old_ducks
          .iter()
          .find(|old| old.def.trigger == def.trigger && old.def.target == def.target)
          .map_or(0.0, |old| old.depth),
      })
      .collect();
    self.focus_loss = mixer.on_focus_loss;
    let focus = focus_db(self.focused, self.focus_loss);
    let (settings, volumes) = (&self.settings, &self.bus_volumes);
    let (blend, ducks) = (&self.blend, &self.ducks);
    let mut tree = BusTree::build(
      self
        .manager
        .as_mut()
        .map(|manager| manager as &mut dyn Tracks),
      &planned,
      |planned| layers_of(planned, settings, volumes, blend, ducks, focus),
    );
    // The main track keeps master's gain until the refresh below
    tree.buses[0].applied = self.buses.buses[0].applied;
    // Built between updates, its buses hold as the clock last stood, so a
    // voice played on one before the next update starts held if it should
    let game = self.game;
    for bus in &mut tree.buses {
      bus.held = holds(bus, game);
    }
    let mut old = std::mem::replace(&mut self.buses, tree);
    let retired = self.generation;
    self.generation = self.generation.wrapping_add(1);
    // A definition's voice goes to the bus it names (one the mixer had yet
    // to add when it started, say), a clip's to its bus's namesake
    let sounds = world.get_resource::<Assets<Sound>>();
    for live in &mut self.voices.live {
      let named = live
        .sound
        .and_then(|id| sounds.and_then(|sounds| sounds.get(id)))
        .map_or_else(|| old.name(live.bus), |sound| sound.bus.as_str());
      live.bus = self.buses.playable(named);
    }
    let sends = old.take_send_tracks();
    if !sends.is_empty() {
      self.retired_sends.push((retired, sends));
    }
    // The new buses hold nothing yet: the frame's update holds the voices
    // their buses hold, and gives each the time scale its bus follows
    self.voices_dirty = true;
    // Dropping its handles retires the old tree: kira removes each track
    // once its sounds have finished
    drop(old);
    // Every loop on a retired tree moves: this rebuild's, and any that
    // could not move at an earlier one. The music moves to what its
    // definition now says, if that changed, so it switches once a frame
    for index in 0..self.voices.live.len() {
      let live = &self.voices.live[index];
      let live_loop =
        live.looping && matches!(live.voice.state(), VoiceState::Playing | VoiceState::Paused);
      if live.generation == self.generation || !live_loop {
        continue;
      }
      let mut moved = if live.music.is_some() {
        self.switch_music(world, server)
      } else {
        Ok(())
      };
      if moved.is_ok() && self.voices.live[index].generation != self.generation {
        moved = self.restart(world, index, server);
      }
      if let Err(why) = moved {
        if self.voices.live[index].playback.waiting_fade().is_some() {
          self.drop_waiting(index, &why, server);
        } else {
          self.warn_stuck(index, &why, server);
        }
      }
    }
    for duck in &mut self.ducks {
      duck.trigger = self.buses.find(&duck.def.trigger).unwrap_or(0);
      duck.target = self.buses.find(&duck.def.target).unwrap_or(0);
    }
    // The active snapshot blends to the new mixer's version of it, or
    // clears if the new mixer has none
    if let Some(active) = self.blend.active.clone() {
      // The blend carries on from where it is, with the time it has left
      let remaining = self.blend.running().then(|| self.blend.remaining());
      if self
        .snapshots
        .iter()
        .any(|snapshot| snapshot.name == active)
      {
        self.activate(Some(&active));
      } else {
        crate::engine_warn!(
          module: "Audio",
          "snapshot {active:?} is not in the reloaded mixer; the mix blends back to open"
        );
        self.activate(None);
      }
      if let Some(remaining) = remaining {
        self.blend.shorten_to(remaining);
      }
    }
    // Built at the web's unlock while the mixer has yet to land, the
    // standard buses leave a snapshot waiting for it, and a bus it may add
    // unwarned
    if !self.mixer_pending
      && let Some(pending) = self.pending_snapshot.take()
    {
      self.activate(pending.as_deref());
    }
    self.refresh_gains();
    self.refresh_cutoffs();
    if self.mixer_pending {
      return;
    }
    let missing: Vec<String> = self
      .bus_volumes
      .keys()
      .filter(|name| self.buses.find(name).is_none())
      .cloned()
      .collect();
    for name in missing {
      self.warn_volume(&name);
    }
  }

  /// Start live voice `index` again on its bus's track, where it is, and
  /// crossfade from the old playback over [`MIN_FADE`]; a paused voice
  /// stays paused. It plays its source's data as the store now has it. If
  /// it cannot start, it plays on where it was, and why is returned
  fn restart(
    &mut self,
    world: &World,
    index: usize,
    server: Option<&AssetServer>,
  ) -> Result<(), String> {
    let live = &self.voices.live[index];
    let replay = Replay {
      source: live.source.as_ref().held(),
      region: live.region.clone(),
      loop_region: live.loop_region.clone(),
      gain_db: live.gain_db,
      bus: live.bus,
    };
    self.replay(world, index, replay, server)
  }

  /// [`restart`](Self::restart) live voice `index` on `replay`: its source's
  /// data as the store now has it, its regions, its gain and its bus, which
  /// the voice takes once it has started. If it cannot start, it plays on
  /// as it was, and why is returned. A loop waiting for the web's unlock
  /// takes them and waits on before the unlock, and at it starts on them,
  /// fading in over its own fade-in
  fn replay(
    &mut self,
    world: &World,
    index: usize,
    replay: Replay,
    server: Option<&AssetServer>,
  ) -> Result<(), String> {
    let live = &self.voices.live[index];
    let Some((samples, version)) = replay.source.as_ref().samples(world) else {
      return Err("its clip is not in the store".into());
    };
    if self.is_locked() {
      self.take_replay(index, replay, version);
      return Ok(());
    }
    let waiting = live.playback.waiting_fade();
    // A paused or held voice starts paused, unheard
    let paused = live.voice.state() == VoiceState::Paused;
    let held = paused || live.held;
    // A filtered voice's filter moves with it. One that waited for the
    // unlock never had one: it gets the one its place gives now
    let cutoff = match waiting {
      Some(_) => live
        .spatial
        .as_ref()
        .zip(live.position)
        .and_then(|(spatial, position)| {
          Hearing::of(&spatial.settings, position, self.listener.as_ref()).cutoff
        }),
      None => live.filter().map(|filter| filter.applied),
    };
    let place_db = live.spatial.as_ref().map_or(0.0, |spatial| spatial.gain_db);
    let own_db = clamp_gain(replay.gain_db + live.game_db);
    let start = Start {
      gain_db: heard_db(own_db, place_db),
      pan: live.pan(),
      rate: live.rate(),
      held,
      fade_in: waiting.map_or(Some(tween(MIN_FADE)), fade_in_of),
      region: replay.region.as_ref(),
      loop_region: replay.loop_region.as_ref(),
      looping: live.looping,
      position: live.playback.position(),
    };
    let bus = replay.bus;
    // The filter goes on a track of its own under its new bus (unfiltered
    // if kira has no room for one, or a new one past the filtered voices'
    // cap)
    let mut filter = match waiting {
      Some(_) => cutoff.and_then(|hz| self.voice_filter(bus, hz)),
      None => cutoff.and_then(|hz| self.filter_track(bus, hz)),
    };
    let track = filter.as_mut().map(|filter| &mut filter.track);
    let started = match samples {
      ClipSource::Decoded(data) => self.start_static(bus, track, data, &start),
      ClipSource::Encoded { clip, rate } => self.start_stream(bus, track, clip, rate, &start),
    };
    let playback = match started {
      Ok(playback) => playback,
      Err(Refusal::Undecodable(error)) => {
        if let Source::Clip(clip) = &replay.source {
          self.warn_undecodable(clip, version, &error, server);
        }
        return Err(format!("its clip could not be opened again: {error}"));
      }
      Err(Refusal::Full) => return Err("its bus has no room".into()),
    };
    let live = &mut self.voices.live[index];
    live.kira_paused = held;
    let mut old = std::mem::replace(&mut live.playback, playback);
    old.stop(MIN_FADE);
    // The old filter's track plays the old sound's fade out, then goes
    if let Some(spatial) = &mut live.spatial {
      spatial.filter = filter;
    }
    // One that waited and starts held fades in at its resume
    if let Some(fade) = waiting {
      live.unheard = held.then_some(fade);
    }
    self.take_replay(index, replay, version);
    // A paused or held one starts silent: its resume fades it in. One that
    // waited had not started
    let fade_in = if held {
      Duration::ZERO
    } else {
      waiting.unwrap_or(MIN_FADE)
    };
    if waiting.is_none() {
      self.record(index, AudioLogKind::Stop, MIN_FADE, server);
    }
    self.record(index, AudioLogKind::Start, fade_in, server);
    Ok(())
  }

  /// Live voice `index` takes `replay`'s source (at content `version`), its
  /// regions, its gain and its bus, on the live tree
  fn take_replay(&mut self, index: usize, replay: Replay, version: u64) {
    let live = &mut self.voices.live[index];
    live.generation = self.generation;
    live.version = version;
    live.source = replay.source;
    live.region = replay.region;
    live.loop_region = replay.loop_region;
    live.gain_db = replay.gain_db;
    if live.bus != replay.bus {
      live.bus = replay.bus;
      // Its new bus's hold and time scale reach it in the frame's update
      self.voices_dirty = true;
    }
  }

  /// Live voice `index`, a loop that waited for the web's unlock, could not
  /// start at it (`why`): it ends, with a warning, logged as refused as a
  /// play that never started is (voice 0)
  fn drop_waiting(&mut self, index: usize, why: &str, server: Option<&AssetServer>) {
    let Some(live) = self.voices.live.get(index) else {
      return;
    };
    let mut entry = self.entry(AudioLogKind::Refused, 0, &live.subject(), server);
    entry.reason = REFUSED_UNLOCK;
    let name = voice_name(live, server);
    crate::engine_warn!(
      module: "Audio",
      "{name}: the loop played before the audio started could not start with it ({why})"
    );
    self.log.push(entry);
    let live = &mut self.voices.live[index];
    live.playback.stop(MIN_FADE);
    live.voice.set_state(VoiceState::Stopping);
  }

  /// Warn, once for live voice `index`, that its loop could not move to the
  /// rebuilt buses. It plays on where it was: a retired tree keeps the
  /// volumes it had, so the sliders no longer reach it (hot reload only).
  /// Natively only a bus whose track has no room gets here: a stream that
  /// cannot open fails on its decoder thread instead
  fn warn_stuck(&mut self, index: usize, why: &str, server: Option<&AssetServer>) {
    let Some(live) = self.voices.live.get_mut(index) else {
      return;
    };
    if std::mem::replace(&mut live.stuck_warned, true) {
      return;
    }
    let name = voice_name(live, server);
    crate::engine_warn!(
      module: "Audio",
      "{name}: the loop could not move to the rebuilt buses ({why}); it plays on as it was, at the old buses' volumes, until it stops"
    );
  }

  /// Bring what plays in line with the registry's reloads, read from its
  /// events. A reloaded music (its definition, its clip, or its intro or
  /// loop file) switches the track playing; a reloaded clip switches each
  /// loop playing it to the new data where it is, crossfading over
  /// [`MIN_FADE`], and lets its one-shots finish on the old; a reloaded
  /// definition retunes its playing loops by the change in its volume and
  /// pitch (the rest of it is for new plays). A reload that failed
  /// (`ReloadFailed`) kept the last good value, so nothing changes. An
  /// asset removed takes with it what the engine remembered of it: a
  /// definition's recent picks and cooldown, a clip's failed decode, and
  /// the warnings given once
  fn follow_reloads(&mut self, world: &World, server: Option<&AssetServer>) {
    let mut music_changed = false;
    if let Some(events) = world.get_resource::<Messages<AssetEvent<Music>>>() {
      for event in self.reloads.musics.read(events) {
        match event {
          AssetEvent::Modified { id } => {
            music_changed |= self
              .music
              .live()
              .is_some_and(|track| track.music.id() == *id);
          }
          AssetEvent::Removed { id } => {
            self.music.warned.remove(id);
            self.music.warned_buses.remove(id);
          }
          _ => {}
        }
      }
    }
    // Taken out to read into, and put back with their room kept
    let mut clips = std::mem::take(&mut self.reloads.changed_clips);
    if let Some(events) = world.get_resource::<Messages<AssetEvent<AudioClip>>>() {
      for event in self.reloads.clips.read(events) {
        match event {
          AssetEvent::Modified { id } => clips.push(*id),
          AssetEvent::Removed { id } => {
            self.undecodable.remove(id);
            self.warned_flat.remove(&id.untyped());
            self.warned_loading.remove(&id.untyped());
          }
          _ => {}
        }
      }
    }
    let mut sounds = std::mem::take(&mut self.reloads.changed_sounds);
    if let Some(events) = world.get_resource::<Messages<AssetEvent<Sound>>>() {
      for event in self.reloads.sounds.read(events) {
        match event {
          AssetEvent::Modified { id } => sounds.push(*id),
          AssetEvent::Removed { id } => {
            self.sounds.remove(id);
            self.warned_sounds.remove(id);
            self.warned_flat.remove(&id.untyped());
            self.warned_loading.remove(&id.untyped());
          }
          _ => {}
        }
      }
    }
    // The music first: a clip it plays switches with it, once
    if music_changed {
      self.follow_music(world, server);
    }
    if !clips.is_empty() {
      self.switch_clips(world, &clips, music_changed, server);
      clips.clear();
    }
    if !sounds.is_empty() {
      self.retune(world, &sounds, server);
      sounds.clear();
    }
    self.reloads.changed_clips = clips;
    self.reloads.changed_sounds = sounds;
  }

  /// Switch each loop playing one of `clips` (reloaded) to the clip's new
  /// data where it is, crossfading over [`MIN_FADE`] (music, with its
  /// definition's loop over the new clip, unless it was tried this frame:
  /// `music_tried`, so a track that cannot switch warns once). Its
  /// one-shots play out on the old data, which kira holds; one that cannot
  /// switch plays on as it was, with a warning
  fn switch_clips(
    &mut self,
    world: &World,
    clips: &[AssetId<AudioClip>],
    music_tried: bool,
    server: Option<&AssetServer>,
  ) {
    for index in 0..self.voices.live.len() {
      let live = &self.voices.live[index];
      let Source::Clip(clip) = &live.source else {
        continue;
      };
      let live_loop =
        live.looping && matches!(live.voice.state(), VoiceState::Playing | VoiceState::Paused);
      if !live_loop || !clips.contains(&clip.id()) {
        continue;
      }
      // One on the new data already (moved to rebuilt buses, or the music
      // switched, this frame) stays
      if live
        .source
        .as_ref()
        .version(world)
        .is_none_or(|version| version == live.version)
      {
        continue;
      }
      if live.music.is_some() {
        if !music_tried {
          self.follow_music(world, server);
        }
      } else if let Err(why) = self.restart(world, index, server) {
        self.warn_unswitched(index, &why, server);
      }
    }
  }

  /// Switch the track playing to its definition as the store now has it
  /// (reloaded, or its clip, intro or loop file): its voice starts again
  /// where it is on the new data, with its loop, volume and bus, crossfading
  /// over [`MIN_FADE`], and takes its new crossfade. A track already on what
  /// the store has (its definition's version and its source's) is left
  /// alone, so it switches at most once a frame. One that cannot start
  /// plays on as it was, and why is returned
  fn switch_music(&mut self, world: &World, server: Option<&AssetServer>) -> Result<(), String> {
    let Some(track) = self.music.live() else {
      return Ok(());
    };
    let (handle, voice, was) = (track.music.clone(), track.voice.id(), track.version);
    let Some(index) = self.voices.index_of(voice) else {
      return Ok(());
    };
    let musics = world.get_resource::<Assets<Music>>();
    let Some((music, version)) =
      musics.and_then(|musics| Some((musics.get(&handle)?, musics.version(&handle)?)))
    else {
      return Ok(());
    };
    let live = &self.voices.live[index];
    if version == was && live.source.as_ref().version(world) == Some(live.version) {
      return Ok(());
    }
    let (source, loop_region) = music_source(world, &handle, music);
    let bus = self.music_bus(&handle, music, server);
    let bus = self.resolve_bus(bus, || music_name(handle.id(), server));
    let replay = Replay {
      source: source.held(),
      region: None,
      loop_region: Some(loop_region),
      gain_db: clamp_gain(music.volume_db),
      bus,
    };
    self.replay(world, index, replay, server)?;
    self.voices.live[index].fade_out = music.crossfade;
    if let Some(track) = &mut self.music.current {
      track.crossfade = music.crossfade;
      track.version = version;
    }
    Ok(())
  }

  /// [`switch_music`](Self::switch_music), warning if the track could not
  /// switch
  fn follow_music(&mut self, world: &World, server: Option<&AssetServer>) {
    if let Err(why) = self.switch_music(world, server) {
      let voice = self.music.live().map(|track| track.voice.id());
      if let Some(index) = voice.and_then(|voice| self.voices.index_of(voice)) {
        self.warn_unswitched(index, &why, server);
      }
    }
  }

  /// Move each loop playing one of `sounds` (reloaded) by the change in its
  /// definition's volume and pitch, over [`MIN_FADE`]: the spread it drew
  /// and its play's options stay, and the game's changes to the voice stay
  /// on top
  fn retune(&mut self, world: &World, sounds: &[AssetId<Sound>], server: Option<&AssetServer>) {
    let Some(defs) = world.get_resource::<Assets<Sound>>() else {
      return;
    };
    for index in 0..self.voices.live.len() {
      let live = &mut self.voices.live[index];
      let live_loop =
        live.looping && matches!(live.voice.state(), VoiceState::Playing | VoiceState::Paused);
      let Some(id) = live.sound else {
        continue;
      };
      if !live_loop || !sounds.contains(&id) {
        continue;
      }
      let Some(def) = defs.get(id) else {
        continue;
      };
      let louder = follow_change(&mut live.tuning.volume_db, def.volume_db);
      let higher = follow_change(&mut live.tuning.pitch, def.pitch);
      let gain = louder != 0.0;
      if gain {
        live.gain_db = clamp_gain(live.gain_db + louder);
        let heard = live.heard_db();
        live.playback.set_volume(heard, tween(MIN_FADE));
      }
      let rate = higher != 0.0;
      if rate {
        live.pitch = clamp_pitch(live.pitch + higher);
        live.playback.set_rate(live.rate(), tween(MIN_FADE));
      }
      if gain {
        self.record(index, AudioLogKind::Gain, MIN_FADE, server);
      }
      if rate {
        self.record(index, AudioLogKind::Rate, MIN_FADE, server);
      }
    }
  }

  /// Warn that live voice `index` could not switch to what was reloaded: it
  /// plays on as it was. Natively only a bus whose track has no room gets
  /// here: a stream that cannot open fails on its decoder thread instead
  fn warn_unswitched(&mut self, index: usize, why: &str, server: Option<&AssetServer>) {
    let Some(live) = self.voices.live.get(index) else {
      return;
    };
    let name = voice_name(live, server);
    crate::engine_warn!(
      module: "Audio",
      "{name}: the loop could not switch to what was reloaded ({why}); it plays on as it was"
    );
  }

  /// The live voices on `bus` and every bus under it (0 for no such bus)
  pub(crate) fn voices_on(&self, bus: &str) -> usize {
    let Some(bus) = self.buses.find(bus) else {
      return 0;
    };
    self
      .voices
      .live
      .iter()
      .filter(|live| self.buses.within(live.bus, bus))
      .count()
  }

  /// Play `playable`, its clips and definitions read from `world`'s stores.
  /// A loop whose sound is still loading (and any play of a definition
  /// still loading, not yet known to loop) waits as a voice, to start once
  /// it is ready ([`start_waiting`](Self::start_waiting)). A one-shot still
  /// loading, or a play its `per_frame`, its cooldown, a voice limit or
  /// kira's room refuses, a stream that cannot open, or a clip whose stream
  /// already failed to decode (until it reloads) is refused and logged with
  /// why; one of a sound that failed to load plays nothing, warned about
  /// once. A refused play's voice is already stopped
  pub(crate) fn play(
    &mut self,
    world: &World,
    playable: Playable<'_>,
    options: &PlayOptions,
  ) -> Voice {
    self.play_from(world, playable, options, None, false)
  }

  /// Play one of the UI theme's sounds, `volume_db` louder or quieter: a
  /// clip as it is, and a definition as it says, on the `ui` bus, or on the
  /// bus under `ui` a definition names (so the game's pause never holds a
  /// UI sound, and the player's ui setting applies to it)
  pub(crate) fn play_ui(&mut self, world: &World, playable: Playable<'_>, volume_db: f32) -> Voice {
    let options = PlayOptions {
      volume_db,
      ..Default::default()
    };
    self.play_from(world, playable, &options, None, true)
  }

  /// Play the sound of `entity`'s [`AudioEmitter`] where the entity is, the
  /// voice following it and belonging to it. An entity that is not an
  /// emitter plays nothing
  pub(crate) fn play_emitter(&mut self, world: &World, entity: Entity) -> Voice {
    let Some(position) = emitter_position(world, entity) else {
      return Voice::stopped();
    };
    let Some(emitter) = world.get::<AudioEmitter>(entity) else {
      return Voice::stopped();
    };
    let options = PlayOptions {
      position: Some(position),
      ..Default::default()
    };
    self.play_from(
      world,
      Playable::Sound(&emitter.sound),
      &options,
      Some(entity),
      false,
    )
  }

  /// [`play`](Self::play), from `emitter` when given; on the `ui` bus (or
  /// the definition's bus under it) when `ui`
  fn play_from(
    &mut self,
    world: &World,
    playable: Playable<'_>,
    options: &PlayOptions,
    emitter: Option<Entity>,
    ui: bool,
  ) -> Voice {
    let server = world.get_resource::<AssetServer>();
    // A mixer that landed or changed is built first: a one-shot cannot move
    // to a bus built after it starts
    self.apply_mixer(world, server);
    // Heard from a place: from where the listener is now
    if options.position.is_some() {
      self.find_listener(world);
    }
    match playable {
      Playable::Clip(clip) => {
        let mut request = Request {
          emitter,
          ..Request::clip(clip, options)
        };
        if ui {
          request.bus = ui_bus(request.bus);
        }
        self.start(world, &request, server)
      }
      Playable::Sound(handle) => {
        let id = handle.id();
        let sound = world
          .get_resource::<Assets<Sound>>()
          .and_then(|sounds| sounds.get(id));
        // A loop played while its sound loads waits for it, as music and
        // emitters do. A definition still loading is not known to loop yet,
        // so a play of one waits too, until it lands (an emitter waits on
        // its own, in `play_spawned`)
        let waits = emitter.is_none()
          && match sound {
            None => server.is_some_and(|server| matches!(server.state(id), LoadState::Loading)),
            Some(sound) => sound.looping && clips_loading(world, sound, server),
          };
        if waits {
          return self.wait_for(handle, sound, options, ui, server);
        }
        let Some(sound) = sound else {
          if server.is_some_and(|server| server.state(id).is_failed()) {
            self.warn_sound(
              id,
              "it did not load, so it plays nothing (the asset warning for it says why)",
              server,
            );
          }
          return Voice::stopped();
        };
        let recent = self
          .sounds
          .get(&id)
          .map_or(&[][..], |state| state.recent.as_slice());
        let request = pick_clip(&mut self.rng, &sound.clips, recent, sound.no_repeat)
          .and_then(|pick| Request::sound(id, sound, pick, &mut self.rng, options))
          .map(|request| Request {
            emitter,
            bus: if ui { ui_bus(request.bus) } else { request.bus },
            ..request
          });
        match request {
          Some(request) => self.start(world, &request, server),
          None => {
            self.warn_sound(id, "it has no clips, so it plays nothing", server);
            Voice::stopped()
          }
        }
      }
    }
  }

  /// A play of `handle` (its definition `sound`, if it has landed) that
  /// waits for it to load: a voice that reports playing, started once the
  /// sound is ready ([`start_waiting`](Self::start_waiting)). It counts
  /// against the sound's `per_frame` now (the default while the definition
  /// is not known); past it, it is refused
  fn wait_for(
    &mut self,
    handle: &Handle<Sound>,
    sound: Option<&Sound>,
    options: &PlayOptions,
    ui: bool,
    server: Option<&AssetServer>,
  ) -> Voice {
    let key = handle.id().untyped();
    let per_frame = sound.map_or(Sound::DEFAULT_PER_FRAME, |sound| sound.per_frame);
    if self.per_frame_full(key, per_frame) {
      return self.refuse_unknown(handle.id(), REFUSED_PER_FRAME, server);
    }
    self.count_start(key);
    let voice = Voice::new(self.voices.next_id(), self.commands.clone(), None);
    self.pending.push(Pending {
      voice: voice.clone(),
      sound: handle.clone(),
      options: *options,
      scope: if options.persist { None } else { self.scope },
      ui,
      game_db: 0.0,
      game_pitch: 0.0,
    });
    voice
  }

  /// A refused play of definition `sound`, not loaded yet, so known only
  /// by its name: logged with `reason`, and a voice that never started
  fn refuse_unknown(
    &mut self,
    sound: AssetId<Sound>,
    reason: &'static str,
    server: Option<&AssetServer>,
  ) -> Voice {
    if self.log.is_on() {
      self.log.push(AudioLogEntry {
        kind: AudioLogKind::Refused,
        voice: 0,
        sound: sound_name(sound, server),
        source: String::new(),
        bus: String::new(),
        gain_db: 0.0,
        pan: 0.0,
        rate: 1.0,
        fade: Duration::ZERO,
        reason,
      });
    }
    Voice::stopped()
  }

  /// Each frame: start the plays that waited for their sounds, once each
  /// sound is ready. A loop starts as it was played, with its fade-in, in
  /// its scene, with what the game did to its voice meanwhile; a one-shot
  /// is refused (it would only be late); a sound that did not load ends
  /// its plays, with the registry's one warning about it
  fn start_waiting(&mut self, world: &World, server: Option<&AssetServer>) {
    let mut index = 0;
    while index < self.pending.len() {
      let pending = &self.pending[index];
      if pending.voice.is_stopped() {
        self.pending.remove(index);
      } else if sound_loading(world, &pending.sound, server) {
        index += 1;
      } else {
        let pending = self.pending.remove(index);
        self.start_pending(world, pending, server);
      }
    }
  }

  /// Start `pending`, its sound done loading (or failed)
  fn start_pending(&mut self, world: &World, pending: Pending, server: Option<&AssetServer>) {
    let id = pending.sound.id();
    let Some(sound) = world
      .get_resource::<Assets<Sound>>()
      .and_then(|sounds| sounds.get(id))
    else {
      // The registry warned once that it did not load, saying why: that is
      // its one warning, which a later play of it does not repeat
      pending.voice.set_state(VoiceState::Stopped);
      self.warned_sounds.insert(id);
      return;
    };
    if !sound.looping {
      pending.voice.set_state(VoiceState::Stopped);
      let Some(clip) = sound.clips.first() else {
        self.warn_sound(id, "it has no clips, so it plays nothing", server);
        return;
      };
      let subject = Subject {
        sound: Some(id),
        music: None,
        source: SourceRef::Clip(&clip.clip),
        bus: self.buses.playable(&sound.bus),
        gain_db: clamp_gain(sound.volume_db),
        pan: 0.0,
        rate: rate_of(clamp_pitch(sound.pitch)),
      };
      self.warn_loading(id.untyped(), || sound_name(id, server));
      self.refuse(&subject, REFUSED_LOADING, server);
      return;
    }
    let recent = self
      .sounds
      .get(&id)
      .map_or(&[][..], |state| state.recent.as_slice());
    let request = pick_clip(&mut self.rng, &sound.clips, recent, sound.no_repeat)
      .and_then(|pick| Request::sound(id, sound, pick, &mut self.rng, &pending.options))
      .map(|request| Request {
        bus: if pending.ui {
          ui_bus(request.bus)
        } else {
          request.bus
        },
        voice: Some(pending.voice.clone()),
        game_db: pending.game_db,
        game_pitch: pending.game_pitch,
        ..request
      });
    let Some(request) = request else {
      pending.voice.set_state(VoiceState::Stopped);
      self.warn_sound(id, "it has no clips, so it plays nothing", server);
      return;
    };
    // It belongs to the scene that played it, not to the code running now
    let previous = self.set_scope(pending.scope);
    let started = self.start(world, &request, server);
    self.set_scope(previous);
    if started.id() == 0 {
      pending.voice.set_state(VoiceState::Stopped);
    }
  }

  /// Apply `command` to a play waiting for its sound, if it is one: what
  /// it does is kept, to start with
  fn change_pending(&mut self, command: VoiceCommand) {
    let Some(index) = self
      .pending
      .iter()
      .position(|pending| pending.voice.id() == command.voice)
    else {
      return;
    };
    let pending = &mut self.pending[index];
    let state = pending.voice.state();
    match command.change {
      VoiceChange::Stop(_) => {
        pending.voice.set_state(VoiceState::Stopped);
        self.pending.remove(index);
      }
      VoiceChange::Pause(_) if state == VoiceState::Playing => {
        pending.voice.set_state(VoiceState::Paused);
      }
      VoiceChange::Resume(_) if state == VoiceState::Paused => {
        pending.voice.set_state(VoiceState::Playing);
      }
      VoiceChange::Pause(_) | VoiceChange::Resume(_) => {}
      VoiceChange::Volume(db, _) => pending.game_db = clamp_offset(db, GAIN_SPAN),
      VoiceChange::Pitch(semitones, _) => {
        pending.game_pitch = clamp_offset(semitones, PITCH_SPAN);
      }
    }
  }

  /// Warn, once for `id` (a definition, or a clip played directly), that a
  /// one-shot was played while it was still loading. Debug builds only:
  /// the refusal is the game's to fix, and a shipped game says nothing
  #[cfg(debug_assertions)]
  fn warn_loading(&mut self, id: UntypedAssetId, name: impl FnOnce() -> String) {
    if self.warned_loading.insert(id) {
      crate::engine_warn!(
        module: "Audio",
        "{}: a one-shot was played while it was still loading, so it plays nothing (a loop waits for its sound; load a one-shot ahead with load_now, or play it once is_ready says so)",
        name()
      );
    }
  }

  #[cfg(not(debug_assertions))]
  fn warn_loading(&mut self, _id: UntypedAssetId, _name: impl FnOnce() -> String) {}

  /// Play `music` on the music player. If a track is playing, it fades
  /// out and `music` fades in, both over `music`'s crossfade; with none, it
  /// fades in over its own fade-in. The track already playing plays on (a
  /// paused one resumes). A track still loading takes over once it has
  /// loaded; one that did not load is warned about once and changes
  /// nothing (ruling P14), nor does one that fails the checks its start
  /// makes. Music belongs to the engine, not to the scene playing it
  pub(crate) fn play_music(&mut self, world: &World, music: &Handle<Music>) {
    let server = world.get_resource::<AssetServer>();
    let id = music.id();
    if let Some(track) = self.music.live()
      && track.music.id() == id
    {
      let voice = track.voice.id();
      // Asked for again, it wins over a track asked for since that has yet
      // to load
      self.music.pending = None;
      self.apply(
        VoiceCommand {
          voice,
          change: VoiceChange::Resume(MIN_FADE),
        },
        server,
      );
      return;
    }
    if self
      .music
      .pending
      .as_ref()
      .is_some_and(|pending| pending.id() == id)
    {
      return;
    }
    match readiness(world, music, server) {
      Readiness::Ready => {
        self.music.pending = None;
        self.start_music(world, music, server);
      }
      Readiness::Loading => self.music.pending = Some(music.clone()),
      // Nothing changes: the track playing plays on, and one still loading
      // still takes over
      Readiness::Failed => self.warn_music(id, server),
    }
  }

  /// Stop the music, fading out over `fade` (at least [`MIN_FADE`]), and
  /// forget a track waiting to load
  pub(crate) fn stop_music(&mut self, world: &World, fade: Duration) {
    let server = world.get_resource::<AssetServer>();
    self.music.pending = None;
    if let Some(track) = self.music.current.take()
      && track.is_live()
    {
      let voice = track.voice.id();
      self.apply(
        VoiceCommand {
          voice,
          change: VoiceChange::Stop(fade),
        },
        server,
      );
    }
  }

  /// Stop the music as a scene's `SceneCleanup::MUSIC` does: fading out over
  /// its track's crossfade
  pub(crate) fn stop_music_with_crossfade(&mut self, world: &World) {
    let fade = self
      .music
      .live()
      .map_or(Duration::ZERO, |track| track.crossfade);
    self.stop_music(world, fade);
  }

  /// Pause the music where it is, fading out over [`MIN_FADE`]
  pub(crate) fn pause_music(&mut self, world: &World) {
    self.change_music(world, VoiceChange::Pause(MIN_FADE));
  }

  /// Resume paused music where it paused, fading in over [`MIN_FADE`]
  pub(crate) fn resume_music(&mut self, world: &World) {
    self.change_music(world, VoiceChange::Resume(MIN_FADE));
  }

  /// Apply `change` to the track playing or paused, if there is one
  fn change_music(&mut self, world: &World, change: VoiceChange) {
    let Some(voice) = self.music.live().map(|track| track.voice.id()) else {
      return;
    };
    let server = world.get_resource::<AssetServer>();
    self.apply(VoiceCommand { voice, change }, server);
  }

  /// The music playing or paused, or the track waiting to load that takes
  /// over once it has
  pub(crate) fn current_music(&self) -> Option<AssetId<Music>> {
    self.music.current_music()
  }

  /// Start `handle`'s track (loaded, with its clip) on its bus, taking over
  /// from the track playing: that one fades out and this one fades in over
  /// this one's crossfade. With none playing it fades in over its fade-in.
  /// A track that cannot start leaves the one playing alone
  fn start_music(&mut self, world: &World, handle: &Handle<Music>, server: Option<&AssetServer>) {
    let musics = world.get_resource::<Assets<Music>>();
    let Some((music, version)) =
      musics.and_then(|musics| Some((musics.get(handle)?, musics.version(handle)?)))
    else {
      return;
    };
    let old = self.music.live().map(|track| track.voice.id());
    let crossfade = music.crossfade;
    let stop_old = |engine: &mut AudioEngine| {
      if let Some(voice) = old {
        let change = VoiceChange::Stop(crossfade);
        engine.apply(VoiceCommand { voice, change }, server);
      }
    };
    let fade_in = if old.is_some() {
      crossfade
    } else {
      music.fade_in
    };
    let (source, loop_region) = music_source(world, handle, music);
    let bus = self.music_bus(handle, music, server);
    let request = Request {
      sound: None,
      tuning: Tuning::default(),
      music: Some(handle.id()),
      source,
      pick: 0,
      no_repeat: 0,
      region: None,
      loop_region: Some(&loop_region),
      looping: true,
      bus,
      gain_db: music.volume_db,
      pitch: 0.0,
      limit: None,
      priority: MUSIC_PRIORITY,
      cooldown: Duration::ZERO,
      // The music player starts one track at a time
      per_frame: u8::MAX,
      fade_in,
      fade_out: music.crossfade,
      position: None,
      spatial: None,
      emitter: None,
      persist: true,
      voice: None,
      game_db: 0.0,
      game_pitch: 0.0,
    };
    // The old track stops first, so a limit on the music bus counts it as
    // fading out and leaves the crossfade alone; but only once the new one
    // passes the checks its start makes. What they cannot see (a stream
    // that fails to open, or a limit refusing it) still leaves the music
    // quiet, with the refusal logged
    let clear = self.music_can_start(world, source, bus);
    if clear {
      stop_old(self);
    }
    let voice = self.start(world, &request, server);
    if voice.is_stopped() {
      if clear {
        self.music.current = None;
      }
      return;
    }
    if !clear {
      stop_old(self);
    }
    self.music.current = Some(Track {
      music: handle.clone(),
      voice,
      crossfade,
      version,
    });
  }

  /// Whether a track playing `source` on the bus `bus` passes the checks its
  /// start makes: its samples are in the store, a clip's stream has not
  /// failed to decode at this version, and kira has room on the bus's track
  fn music_can_start(&self, world: &World, source: SourceRef<'_>, bus: &str) -> bool {
    let Some(version) = source.version(world) else {
      return false;
    };
    let undecodable = source
      .clip()
      .is_some_and(|clip| self.undecodable.get(&clip.id()) == Some(&version));
    // Before the web's unlock it waits, needing no room yet
    !undecodable && (self.is_locked() || self.buses.has_room(self.buses.playable(bus)))
  }

  /// Each frame: start the track waiting to load once it has (or forget it,
  /// warned about, if it failed)
  fn step_music(&mut self, world: &World, server: Option<&AssetServer>) {
    let Some(pending) = &self.music.pending else {
      return;
    };
    match readiness(world, pending, server) {
      Readiness::Loading => {}
      Readiness::Ready => {
        if let Some(pending) = self.music.pending.take() {
          self.start_music(world, &pending, server);
        }
      }
      Readiness::Failed => {
        let id = pending.id();
        self.music.pending = None;
        self.warn_music(id, server);
      }
    }
  }

  /// Warn, once for `music`, that it did not load
  fn warn_music(&mut self, music: AssetId<Music>, server: Option<&AssetServer>) {
    if self.music.warned.insert(music) {
      let name = music_name(music, server);
      crate::engine_warn!(
        module: "Audio",
        "{name}: the music did not load, so it does not play (the asset warning for it says why)"
      );
    }
  }

  /// The bus `music` (`handle`'s) plays on: its own, if it is `music` or a
  /// bus under it, else `music`, warned about once
  fn music_bus<'a>(
    &mut self,
    handle: &Handle<Music>,
    music: &'a Music,
    server: Option<&AssetServer>,
  ) -> &'a str {
    if music.bus == MUSIC_BUS || music.bus.starts_with("music/") {
      music.bus.as_str()
    } else {
      self.warn_music_bus(handle.id(), &music.bus, server);
      MUSIC_BUS
    }
  }

  /// Warn, once for `music`, that its bus is outside `music`
  fn warn_music_bus(&mut self, music: AssetId<Music>, bus: &str, server: Option<&AssetServer>) {
    if self.music.warned_buses.insert(music) {
      let name = music_name(music, server);
      crate::engine_warn!(
        module: "Audio",
        "{name}: music plays on the music bus or one under it, not {bus:?}, so it plays on \"music\""
      );
    }
  }

  /// Start `request`'s voice (as its waiting voice, if it has one), unless
  /// its clip is not loaded (a stopped voice: refused and logged if the
  /// clip still loads, not logged if it failed), or it is a one-shot out of
  /// range (culled) or refused (a stopped voice, logged)
  fn start(&mut self, world: &World, request: &Request<'_>, server: Option<&AssetServer>) -> Voice {
    let bus = self.resolve_bus(request.bus, || {
      owner_name(request.sound, request.music, server)
    });
    let pitch = clamp_pitch(request.pitch);
    let scale = self.scale_on(bus);
    // An emitter's voice belongs to its entity, a persistent one to the
    // engine
    let scope = if request.persist || request.emitter.is_some() {
      None
    } else {
      self.scope
    };
    let (bus_held, owner_held) = self.buses.buses.get(bus).map_or((false, false), |bus| {
      (
        bus.held,
        bus.pauses_with_game && owner_holds(&self.held_scopes, scope, request.emitter.is_some()),
      )
    });
    let held = bus_held || owner_held;
    let own_db = clamp_gain(request.gain_db);
    // As played: with the changes the game made to a voice that waited
    let played_db = clamp_gain(own_db + request.game_db);
    let played_pitch = clamp_pitch(pitch + request.game_pitch);
    // A voice paused while it waited starts as a held one does: silent,
    // its fade-in kept for its resume
    let paused = request
      .voice
      .as_ref()
      .is_some_and(|voice| voice.state() == VoiceState::Paused);
    let starts_held = held || paused;
    // How it is heard from its place, if its sound says
    let heard = request
      .position
      .zip(request.spatial)
      .map(|(position, spatial)| {
        (
          spatial,
          Hearing::of(&spatial, position, self.listener.as_ref()),
        )
      });
    let subject = Subject {
      sound: request.sound,
      music: request.music,
      source: request.source,
      bus,
      gain_db: heard_db(played_db, heard.map_or(0.0, |(_, hearing)| hearing.gain_db)),
      pan: heard.map_or(0.0, |(_, hearing)| hearing.pan),
      rate: rate_of(played_pitch) * scale,
    };
    let Some((samples, version)) = request.source.samples(world) else {
      // A one-shot whose clip still loads would only be late (a loop waits
      // for its clips before it gets here)
      let loading = request.source.clip().is_some_and(|clip| {
        server.is_some_and(|server| matches!(server.state(clip), LoadState::Loading))
      });
      if !loading {
        return Voice::stopped();
      }
      self.warn_loading(start_key(request), || {
        request.sound.map_or_else(
          || request.source.name(server),
          |sound| sound_name(sound, server),
        )
      });
      return self.refuse(&subject, REFUSED_LOADING, server);
    };
    if request.position.is_some() && request.spatial.is_none() {
      self.warn_flat(request, server);
    }
    // A one-shot out of range would not be heard; a loop plays on, silent,
    // to be heard as it comes in range
    if heard.is_some_and(|(_, hearing)| hearing.out_of_range) && !request.looping {
      let entry = self.entry(AudioLogKind::Culled, 0, &subject, server);
      self.log.push(entry);
      return Voice::stopped();
    }
    // Copies started in one frame play in step and add up: a sound starts
    // as often as its `per_frame` allows a frame, wherever it plays. The
    // music player's one track is not counted
    let counted = (request.music.is_none() && request.voice.is_none()).then(|| start_key(request));
    if counted.is_some_and(|key| self.per_frame_full(key, request.per_frame)) {
      return self.refuse(&subject, REFUSED_PER_FRAME, server);
    }
    // A clip whose stream failed to decode would only fail the same way on
    // another decoder thread. Kira's room is asked before any limit stops
    // a voice: a stopped voice holds its room until it has faded out
    let undecodable = request
      .source
      .clip()
      .is_some_and(|clip| self.undecodable.get(&clip.id()) == Some(&version));
    // Before the web's unlock nothing can sound: a one-shot is dropped, and
    // a loop waits, to start at the unlock
    let waits = self.is_locked();
    if waits && !request.looping {
      return self.refuse(&subject, REFUSED_LOCKED, server);
    }
    let refusal = if undecodable {
      Some(REFUSED_UNDECODABLE)
    } else if self.cooling(request) {
      Some(REFUSED_COOLDOWN)
    } else if !waits && !self.room_on(bus) {
      Some(REFUSED_FULL)
    } else if !self.make_room(bus, request) {
      Some(REFUSED_LIMIT)
    } else {
      None
    };
    if let Some(reason) = refusal {
      return self.refuse(&subject, reason, server);
    }
    let length = {
      let (rate, frames) = match &samples {
        ClipSource::Decoded(data) => (data.sample_rate, data.frames.len()),
        ClipSource::Encoded { clip, rate } => (*rate, clip.frames),
      };
      let played = request
        .region
        .and_then(|region| within(region, rate, frames))
        .map_or(frames, |(from, to)| to - from);
      played as f64 / f64::from(rate.max(1))
    };
    let (playback, filter) = if waits {
      (
        Playback::Waiting(Waiting::new(request.fade_in, starts_held)),
        None,
      )
    } else {
      // Its distance low-pass, on a track of its own while there is room
      let mut filter = heard
        .and_then(|(_, hearing)| hearing.cutoff)
        .and_then(|hz| self.voice_filter(bus, hz));
      let start = Start {
        gain_db: subject.gain_db,
        pan: subject.pan,
        rate: subject.rate,
        held: starts_held,
        fade_in: fade_in_of(request.fade_in),
        region: request.region,
        loop_region: request.loop_region,
        looping: request.looping,
        position: 0.0,
      };
      let track = filter.as_mut().map(|filter| &mut filter.track);
      let started = match samples {
        ClipSource::Decoded(data) => self.start_static(bus, track, data, &start),
        ClipSource::Encoded { clip, rate } => self.start_stream(bus, track, clip, rate, &start),
      };
      match started {
        Ok(playback) => (playback, filter),
        Err(refusal) => {
          let reason = match &refusal {
            Refusal::Undecodable(error) => {
              // Only a clip streams
              if let Some(clip) = request.source.clip() {
                self.warn_undecodable(clip, version, error, server);
              }
              REFUSED_UNDECODABLE
            }
            Refusal::Full => {
              self.count_kira_full();
              self.warn_full(bus);
              REFUSED_FULL
            }
          };
          self.victims.clear();
          return self.refuse(&subject, reason, server);
        }
      }
    };
    self.steal_victims(server);
    let pick = request.source.clip().map(|clip| VoicePick {
      clip_index: request.pick,
      clip: clip.id(),
      volume_db: own_db,
      pitch,
    });
    let voice = match &request.voice {
      Some(voice) => {
        voice.set_pick(pick);
        voice.clone()
      }
      None => Voice::new(self.voices.next_id(), self.commands.clone(), pick),
    };
    self.voices.live.push(LiveVoice {
      voice: voice.clone(),
      playback,
      source: request.source.held(),
      version,
      sound: request.sound,
      tuning: request.tuning,
      music: request.music,
      bus,
      gain_db: own_db,
      pitch,
      game_db: request.game_db,
      game_pitch: request.game_pitch,
      scale,
      held,
      owner_held,
      kira_paused: starts_held,
      unheard: starts_held.then_some(request.fade_in),
      own_fade: None,
      scope,
      priority: request.priority,
      position: request.position,
      emitter: request.emitter,
      spatial: heard.map(|(settings, hearing)| SpatialVoice {
        settings,
        gain_db: hearing.gain_db,
        pan: hearing.pan,
        filter,
      }),
      fade_out: request.fade_out,
      frame: self.frame,
      looping: request.looping,
      length,
      region: request.region.cloned(),
      loop_region: request.loop_region.cloned(),
      generation: self.generation,
      stuck_warned: false,
    });
    // A held start is silent and paused: its fade-in waits for its resume.
    // One waiting for the unlock is logged as it starts there
    let fade_in = if starts_held {
      Duration::ZERO
    } else {
      request.fade_in
    };
    if !waits {
      self.record(
        self.voices.live.len() - 1,
        AudioLogKind::Start,
        fade_in,
        server,
      );
    }
    if let Some(key) = counted {
      self.count_start(key);
    }
    if let Some(id) = request.sound {
      let state = self.sounds.entry(id).or_default();
      state.last_start = Some(self.clock);
      state.recent.push(request.pick);
      let excess = state
        .recent
        .len()
        .saturating_sub(usize::from(request.no_repeat));
      state.recent.drain(..excess);
    }
    voice
  }

  /// A distance low-pass at `hz` for a new voice on `bus`, unless
  /// [`MAX_FILTERED`] voices have one already (it then plays unfiltered)
  fn voice_filter(&mut self, bus: usize, hz: f64) -> Option<VoiceFilter> {
    let filtered = self
      .voices
      .live
      .iter()
      .filter(|live| live.filter().is_some())
      .count();
    if filtered >= MAX_FILTERED {
      return None;
    }
    self.filter_track(bus, hz)
  }

  /// A low-pass at `hz` on a kira track of its own under `bus`'s, if kira
  /// has room
  fn filter_track(&mut self, bus: usize, hz: f64) -> Option<VoiceFilter> {
    match self.buses.buses.get_mut(bus).map(|bus| &mut bus.output) {
      Some(Output::Track(Some(track))) => VoiceFilter::new(track, hz),
      _ => None,
    }
  }

  /// Warn, once for its sound (or its clip, played directly), that
  /// `request` was played at a place but has no spatial settings, so is
  /// heard as if at the listener
  fn warn_flat(&mut self, request: &Request<'_>, server: Option<&AssetServer>) {
    let id = request
      .sound
      .map_or_else(|| request.source.untyped(), AssetId::untyped);
    if !self.warned_flat.insert(id) {
      return;
    }
    let name = request.sound.map_or_else(
      || request.source.name(server),
      |sound| sound_name(sound, server),
    );
    crate::engine_warn!(
      module: "Audio",
      "{name}: it is played at a place, but has no spatial settings, so it is heard as if at the listener (a sound definition's `spatial` sets how it falls off with distance)"
    );
  }

  /// Whether `request`'s definition played too recently for its cooldown
  fn cooling(&self, request: &Request<'_>) -> bool {
    let Some(id) = request.sound else {
      return false;
    };
    !request.cooldown.is_zero()
      && self
        .sounds
        .get(&id)
        .and_then(|state| state.last_start)
        .is_some_and(|last| self.clock.saturating_sub(last) < request.cooldown)
  }

  /// Plan the voices a play on `bus` stops, into `victims`, to keep within
  /// its definition's limit (its voices on the bus's subtree), each bus
  /// limit from `bus` up (every voice on that bus's subtree, by the
  /// definition's rule, or the cap's without one) and the cap on `bus`'s own
  /// track. False, with nothing planned, if a limit refuses the play
  fn make_room(&mut self, bus: usize, request: &Request<'_>) -> bool {
    self.victims.clear();
    let buses = &self.buses;
    let mut room = Room {
      live: &self.voices.live,
      buses,
      victims: &mut self.victims,
      frame: self.frame,
      listener: self.listener.as_ref(),
      priority: request.priority,
      music: request.music.is_some(),
    };
    let mut fits = match (request.sound, request.limit) {
      (Some(id), Some(limit)) => room.fit(
        Scope::Sound(id, bus),
        usize::from(limit.max),
        Some(limit.rule),
      ),
      _ => true,
    };
    let mut at = Some(bus);
    while let (true, Some(index)) = (fits, at) {
      let Some(this) = buses.buses.get(index) else {
        break;
      };
      if let Some(max) = this.limit {
        let rule = request.limit.map(|limit| limit.rule);
        fits = room.fit(Scope::Subtree(index), usize::from(max), rule);
      }
      at = this.parent;
    }
    fits = fits && room.fit(Scope::Track(bus), VOICE_CAP, None);
    if !fits {
      self.victims.clear();
    }
    fits
  }

  /// Stop the planned victims, each with its fade, logged as stolen
  fn steal_victims(&mut self, server: Option<&AssetServer>) {
    for at in 0..self.victims.len() {
      let Victim { index, fade } = self.victims[at];
      if let Some(live) = self.voices.live.get_mut(index) {
        live.playback.stop(fade);
        live.voice.set_state(VoiceState::Stopping);
      }
      self.record(index, AudioLogKind::Stolen, fade, server);
    }
    self.victims.clear();
  }

  /// Whether kira has room for one more sound on `bus`'s track, warning
  /// once for the bus when it has none
  fn room_on(&mut self, bus: usize) -> bool {
    if self.buses.has_room(bus) {
      return true;
    }
    self.warn_full(bus);
    false
  }

  /// Warn, once for `bus`, that kira's room on its track is full. Only the
  /// voices still fading out fill it past the cap: a stopped sound holds its
  /// room until its fade ends
  fn warn_full(&mut self, bus: usize) {
    let Some(this) = self.buses.buses.get_mut(bus) else {
      return;
    };
    let has_track = matches!(this.output, Output::Track(Some(_)));
    if !has_track || std::mem::replace(&mut this.full_warned, true) {
      return;
    }
    crate::engine_warn!(
      module: "Audio",
      "the {:?} bus is full: voices still fading out hold all of kira's room on its track, so plays on it are refused until they end (shorter fade_outs on the sounds stopped there keep room free)",
      this.name
    );
  }

  /// A refused play: logged with `reason`, and a voice that never started
  fn refuse(
    &mut self,
    subject: &Subject<'_>,
    reason: &'static str,
    server: Option<&AssetServer>,
  ) -> Voice {
    let mut entry = self.entry(AudioLogKind::Refused, 0, subject, server);
    entry.reason = reason;
    self.log.push(entry);
    Voice::stopped()
  }

  /// Whether `key` (a definition, or a clip played directly) started
  /// `per_frame` voices this frame already (at least one may start)
  fn per_frame_full(&self, key: UntypedAssetId, per_frame: u8) -> bool {
    self
      .frame_starts
      .iter()
      .find(|(started, _)| *started == key)
      .is_some_and(|&(_, count)| count >= per_frame.max(1))
  }

  /// Count a start of `key` this frame
  fn count_start(&mut self, key: UntypedAssetId) {
    match self
      .frame_starts
      .iter_mut()
      .find(|(started, _)| *started == key)
    {
      Some((_, count)) => *count = count.saturating_add(1),
      None => self.frame_starts.push((key, 1)),
    }
  }

  #[cfg(test)]
  fn count_kira_full(&mut self) {
    self.kira_full += 1;
  }

  #[cfg(not(test))]
  fn count_kira_full(&mut self) {}

  /// The bus named `name`, or the nearest bus above it that plays voices
  /// (`sfx` for `sfx/steps`; a send bus's parent), or `sfx`, warning once
  /// per name (naming `who` plays on it), unless the mixer that may add it
  /// has yet to land
  fn resolve_bus(&mut self, name: &str, who: impl FnOnce() -> String) -> usize {
    let bus = self.buses.playable(name);
    if self.buses.name(bus) == name || self.mixer_pending || self.warned_buses.contains(name) {
      return bus;
    }
    self.warned_buses.insert(name.to_string());
    let who = who();
    let on = self.buses.name(bus);
    if self.buses.find(name).is_some() {
      crate::engine_warn!(
        module: "Audio",
        "{who}: the bus {name:?} is a send bus, which plays no voices of its own, so it plays on {on:?}"
      );
    } else {
      crate::engine_warn!(
        module: "Audio",
        "{who}: there is no bus {name:?}, so it plays on {on:?} (a game's own buses are added in the mixer)"
      );
    }
    bus
  }

  /// Warn about definition `id` once, with `why`
  fn warn_sound(&mut self, id: AssetId<Sound>, why: &str, server: Option<&AssetServer>) {
    if self.warned_sounds.insert(id) {
      let name = sound_name(id, server);
      crate::engine_warn!(module: "Audio", "{name}: {why}");
    }
  }

  /// Start decoded frames on `bus` (or `track`, a voice's own under it). A
  /// static sound cannot fail to start but for room
  fn start_static(
    &mut self,
    bus: usize,
    track: Option<&mut TrackHandle>,
    mut data: StaticSoundData,
    start: &Start<'_>,
  ) -> Result<Playback, Refusal> {
    let span = Span::of(start, data.sample_rate, data.frames.len());
    data.slice = span.slice;
    data.settings.loop_region = span.loop_region.map(region);
    data.settings.volume = Decibels(start.gain_db).into();
    data.settings.panning = Panning(start.pan).into();
    data.settings.playback_rate = PlaybackRate(start.rate).into();
    data.settings.fade_in_tween = start.fade_in();
    let position = span.wrap(start.position, data.sample_rate, data.frames.len());
    data.settings.start_position = PlaybackPosition::Seconds(position);
    let played = match track {
      Some(track) => track.play(data),
      None => self.buses.play(bus, data),
    };
    let playback = played.map(Playback::Static).map_err(|_| Refusal::Full)?;
    Ok(start.hold(playback))
  }

  /// Start a stream of a clip's file, at its sample rate `rate`, on `bus`
  /// (or `track`, a voice's own under it). Its decoder opens the file on
  /// kira's decoder thread, so a file that cannot open fails there, as the
  /// voice's decode error. The web has no streaming, and decodes every
  /// clip, so never gets here
  #[cfg(not(target_arch = "wasm32"))]
  fn start_stream(
    &mut self,
    bus: usize,
    track: Option<&mut TrackHandle>,
    clip: EncodedClip,
    rate: u32,
    start: &Start<'_>,
  ) -> Result<Playback, Refusal> {
    let frames = clip.frames;
    let span = Span::of(start, rate, frames);
    let position = span.wrap(start.position, rate, frames);
    // The region is the decoder's whole stream, so kira's frames, the loop
    // and the position all count from its start, as a decoded clip's slice
    // does. kira's own slice would seek the decoder from the file's start
    let (from, to) = span.slice.unwrap_or((0, frames));
    let decoder = ClipDecoder::new(clip.bytes, rate, from..to);
    #[cfg(test)]
    let decoder = match &self.stream_decoding {
      Some(decoded) => decoder.counting(Arc::clone(decoded)),
      None => decoder,
    };
    let data = StreamingSoundData::from_decoder(decoder)
      .volume(Decibels(start.gain_db))
      .panning(Panning(start.pan))
      .playback_rate(PlaybackRate(start.rate))
      .fade_in_tween(start.fade_in())
      .start_position(position)
      .loop_region(span.loop_region.map(region));
    let played = match track {
      Some(track) => track.play(data),
      None => self.buses.play(bus, data),
    };
    match played {
      Ok(sound) => Ok(start.hold(Playback::Stream(sound))),
      Err(kira::PlaySoundError::SoundLimitReached) => Err(Refusal::Full),
      Err(kira::PlaySoundError::IntoSoundError(error)) => {
        Err(Refusal::Undecodable(error.to_string()))
      }
    }
  }

  #[cfg(target_arch = "wasm32")]
  fn start_stream(
    &mut self,
    _bus: usize,
    _track: Option<&mut TrackHandle>,
    _clip: EncodedClip,
    _rate: u32,
    _start: &Start<'_>,
  ) -> Result<Playback, Refusal> {
    Err(Refusal::Undecodable(
      "the web cannot stream audio".to_string(),
    ))
  }

  /// The frame's audio work: count the frame, apply the voices' queued
  /// commands, build the bus tree from the mixer if it landed or changed,
  /// switch what plays to the clips, definitions and music reloaded (and
  /// forget what was remembered of those freed), follow the listener and
  /// the emitters (and play the emitters spawned), follow the `game` clock
  /// (pause and time scale), report the device's stream errors, advance the
  /// mock backend by `real_delta`, and retire the voices that ended (and a
  /// retired tree's send tracks once its voices have)
  pub(crate) fn update(&mut self, world: &World, real_delta: Duration, game: GameTime) {
    let server = world.get_resource::<AssetServer>();
    self.clock = self.clock.saturating_add(real_delta);
    self.apply_queued(server);
    self.apply_mixer(world, server);
    self.follow_reloads(world, server);
    self.step_music(world, server);
    self.start_waiting(world, server);
    self.release_orphans(server);
    self.follow_listener(world, real_delta, server);
    self.play_spawned(world, server);
    self.follow_game(game, real_delta, server);
    self.report_stream_errors();
    self.mock_render = Duration::ZERO;
    if let Some(Manager::Mock(manager)) = &mut self.manager {
      let chunks = chunks_due(&mut self.mock_owed, real_delta);
      if chunks > 0 {
        let started = web_time::Instant::now();
        let backend = manager.backend_mut();
        for _ in 0..chunks {
          backend.on_start_processing();
          backend.process();
        }
        self.mock_render = started.elapsed();
      }
    }
    self.retire_ended(server);
    // A track whose voice ended lets its definition go
    if self
      .music
      .current
      .as_ref()
      .is_some_and(|track| track.voice.is_stopped())
    {
      self.music.current = None;
    }
    self.step_mix(real_delta);
    if !self.retired_sends.is_empty() {
      let live = &self.voices.live;
      self
        .retired_sends
        .retain(|(generation, _)| live.iter().any(|voice| voice.generation == *generation));
    }
    // The next frame's starts count afresh, and its voices are new to
    // kira until it renders them
    self.frame_starts.clear();
    self.frame = self.frame.wrapping_add(1);
  }

  /// Whether voices' changes wait to be applied
  pub(crate) fn has_queued(&self) -> bool {
    !self.inbox.is_empty()
  }

  /// Apply the voices' queued changes, in the order they were sent
  pub(crate) fn apply_queued(&mut self, server: Option<&AssetServer>) {
    while let Ok(command) = self.inbox.try_recv() {
      self.apply(command, server);
    }
  }

  /// Follow the game's clock. A bus that pauses with the game holds its
  /// voices while the game is paused, and one that follows time scale
  /// while time stands still (a scale at or below [`HOLD_SCALE`]: the game
  /// paused, or a speed of about 0; hitstop does not hold), and a voice on
  /// a bus that pauses with the game is held while its owner is
  /// ([`hold_scopes`](Self::hold_scopes)): held, a voice is paused in kira
  /// where it is, over [`MIN_FADE`], and resumes from there once nothing
  /// holds it (a voice held from its start, over its own fade-in). A voice
  /// on a bus that follows time scale plays at its pitch's rate times the
  /// scale, gliding over the frame; held, it keeps its rate. Each bus held
  /// or let go is logged (`Pause`, `Resume`), each voice its owner holds or
  /// lets go, each voice's first resume after a held start, and each rate
  /// change. The voices' own pauses and resumes, applied this frame, reach
  /// kira here too, so each voice gets at most one pause or resume a frame
  fn follow_game(&mut self, game: GameTime, real_delta: Duration, server: Option<&AssetServer>) {
    self.game = game;
    let still = game.scale <= HOLD_SCALE;
    let scale = f64::from(game.scale);
    if !still && (scale - self.rate_scale).abs() > SCALE_EPSILON {
      self.rate_scale = scale;
      self.voices_dirty = true;
    }
    for index in 0..self.buses.buses.len() {
      let bus = &mut self.buses.buses[index];
      let held = holds(bus, game);
      if held == bus.held {
        continue;
      }
      bus.held = held;
      self.voices_dirty = true;
      let kind = if held {
        AudioLogKind::Pause
      } else {
        AudioLogKind::Resume
      };
      self.record_bus(index, kind, MIN_FADE);
    }
    if !std::mem::take(&mut self.voices_dirty) {
      return;
    }
    let glide = real_delta.max(MIN_FADE);
    let frame = self.frame;
    for index in 0..self.voices.live.len() {
      let live = &mut self.voices.live[index];
      let state = live.voice.state();
      // A stopping voice fades out as it is: a resume would undo its stop
      if !matches!(state, VoiceState::Playing | VoiceState::Paused) {
        continue;
      }
      let Some(bus) = self.buses.buses.get(live.bus) else {
        continue;
      };
      // Held by its bus, or by its owner (a scene not updated), or both:
      // it plays again once neither holds it
      let owner =
        bus.pauses_with_game && owner_holds(&self.held_scopes, live.scope, live.emitter.is_some());
      let owner_changed = owner != live.owner_held;
      live.owner_held = owner;
      let held = bus.held || owner;
      let let_go = live.held && !held;
      live.held = held;
      // Paused itself, or held: kira reads a pause before a resume sent in
      // the same frame, so only where it ends up is sent. Its own pause or
      // resume this frame gives the fade; a hold's is the shortest
      let paused = state == VoiceState::Paused || live.held;
      let own_fade = live.own_fade.take();
      let own = own_fade.unwrap_or(MIN_FADE);
      let mut first_heard = None;
      // A hold by its owner is logged on the voice (a bus's on the bus);
      // its own pause or resume logged itself
      let mut owner_hold = None;
      if paused != live.kira_paused {
        live.kira_paused = paused;
        let by_owner = owner_changed && own_fade.is_none();
        if paused {
          live.playback.pause(own);
          if by_owner {
            owner_hold = Some((AudioLogKind::Pause, own));
          }
        } else if live.playback.waiting_fade().is_some() {
          // Not heard yet: it starts at the web's unlock, fading in there
          live.playback.resume(MIN_FADE);
        } else {
          let unheard = live.unheard.take();
          let fade = unheard.map_or(own, |fade| fade.max(own));
          live.playback.resume(fade);
          if let_go && unheard.is_some() {
            first_heard = Some(fade);
          } else if by_owner {
            owner_hold = Some((AudioLogKind::Resume, fade));
          }
        }
      }
      let scale = if bus.follows_time_scale {
        self.rate_scale
      } else {
        1.0
      };
      let rescaled = (scale - live.scale).abs() > SCALE_EPSILON;
      // One started this frame has not been heard: its rate moves at once
      let glide = if live.frame == frame {
        Duration::ZERO
      } else {
        glide
      };
      if rescaled {
        live.scale = scale;
        live.playback.set_rate(live.rate(), tween(glide));
      }
      if let Some(fade) = first_heard {
        self.record(index, AudioLogKind::Resume, fade, server);
      }
      if let Some((kind, fade)) = owner_hold {
        self.record(index, kind, fade, server);
      }
      if rescaled {
        self.record(index, AudioLogKind::Rate, glide, server);
      }
    }
  }

  /// Follow the listener (the first `AudioListener`, else the main camera)
  /// and the emitters. Each voice played from an emitter moves to where its
  /// entity is now; one whose entity is gone (or no longer an emitter) is
  /// released, as its scene's exit would release it: a loop stops with its
  /// fade, and a one-shot playing plays out where the entity was. Each
  /// voice heard from a place then takes the gain, pan and low-pass cutoff
  /// its place now gives, where they moved enough to hear, gliding over the
  /// frame (each gain and pan change logged). Nothing is looked up while no
  /// voice has a place
  fn follow_listener(&mut self, world: &World, real_delta: Duration, server: Option<&AssetServer>) {
    let placed = self.voices.live.iter().any(|live| live.position.is_some());
    if !placed && self.spawned.is_empty() {
      return;
    }
    self.find_listener(world);
    let frame = self.frame;
    for index in 0..self.voices.live.len() {
      let live = &mut self.voices.live[index];
      if !matches!(live.voice.state(), VoiceState::Playing | VoiceState::Paused) {
        continue;
      }
      // One started this frame has not been heard: it moves at once
      let glide = if live.frame == frame {
        Duration::ZERO
      } else {
        real_delta.max(MIN_FADE)
      };
      if let Some(entity) = live.emitter {
        match emitter_position(world, entity) {
          Some(position) => live.position = Some(position),
          None => {
            live.emitter = None;
            self.release_voice(index, false, server);
          }
        }
      }
      let live = &mut self.voices.live[index];
      let own_db = live.own_db();
      let (Some(spatial), Some(position)) = (&mut live.spatial, live.position) else {
        continue;
      };
      if live.voice.state() == VoiceState::Stopping {
        continue;
      }
      let hearing = Hearing::of(&spatial.settings, position, self.listener.as_ref());
      let gain = spatial.follow_gain(&hearing);
      if gain {
        let heard = heard_db(own_db, spatial.gain_db);
        live.playback.set_volume(heard, tween(glide));
      }
      let pan = spatial.follow_pan(&hearing);
      if pan {
        live.playback.set_panning(spatial.pan, tween(glide));
      }
      spatial.follow_cutoff(&hearing, tween(glide));
      if gain {
        self.record(index, AudioLogKind::Gain, glide, server);
      }
      if pan {
        self.record(index, AudioLogKind::Pan, glide, server);
      }
    }
  }

  /// Play the emitters added to entities (or enabled) this frame, or before
  /// while their sounds loaded, that play on spawn, where their entities
  /// are, each once however often it was queued. One whose sound is still
  /// loading waits for it; one gone, disabled (it plays again when
  /// enabled), or no longer an emitter is forgotten
  fn play_spawned(&mut self, world: &World, server: Option<&AssetServer>) {
    if self.spawned.is_empty() {
      return;
    }
    // Taken out to play from, and put back with their room kept
    let mut spawned = std::mem::take(&mut self.spawned);
    let mut seen = std::mem::take(&mut self.spawned_seen);
    seen.clear();
    let sounds = world.get_resource::<Assets<Sound>>();
    spawned.retain(|&entity| {
      if !seen.insert(entity) {
        return false;
      }
      let Some(emitter) = world.get::<AudioEmitter>(entity) else {
        return false;
      };
      if world.get::<Disabled>(entity).is_some() {
        return false;
      }
      let id = emitter.sound.id();
      let loading = sound_loading(world, &emitter.sound, server);
      // One past its sound's starts this frame waits its turn
      let turn = sounds
        .and_then(|sounds| sounds.get(id))
        .is_some_and(|sound| self.per_frame_full(id.untyped(), sound.per_frame));
      if loading || turn {
        return true;
      }
      self.play_emitter(world, entity);
      false
    });
    spawned.append(&mut self.spawned);
    self.spawned = spawned;
    self.spawned_seen = seen;
  }

  /// The time scale a voice on `bus` carries: the game's, if the bus
  /// follows time scale
  fn scale_on(&self, bus: usize) -> f64 {
    if self
      .buses
      .buses
      .get(bus)
      .is_some_and(|bus| bus.follows_time_scale)
    {
      self.rate_scale
    } else {
      1.0
    }
  }

  /// Apply one queued change, over its fade (at least [`MIN_FADE`]). A
  /// voice already stopping or gone takes no more changes. A volume or
  /// pitch change to a voice started this frame, which kira has yet to
  /// render, applies at once, so its first audio is at the level set
  fn apply(&mut self, command: VoiceCommand, server: Option<&AssetServer>) {
    let Some(index) = self.voices.index_of(command.voice) else {
      self.change_pending(command);
      return;
    };
    let frame = self.frame;
    let live = &mut self.voices.live[index];
    let state = live.voice.state();
    if matches!(state, VoiceState::Stopping | VoiceState::Stopped) {
      return;
    }
    let fresh = live.frame == frame;
    let change_over = |fade: Duration| {
      if fresh {
        Duration::ZERO
      } else {
        fade.max(MIN_FADE)
      }
    };
    let (kind, fade) = match command.change {
      VoiceChange::Stop(fade) => {
        let fade = fade.max(MIN_FADE);
        live.playback.stop(fade);
        live.voice.set_state(VoiceState::Stopping);
        (AudioLogKind::Stop, fade)
      }
      // kira hears of a pause or resume in the frame's `follow_game`, with
      // the bus's hold: a voice its bus holds stays paused until the bus
      // lets it go
      VoiceChange::Pause(fade) => {
        if state == VoiceState::Paused {
          return;
        }
        let fade = fade.max(MIN_FADE);
        live.voice.set_state(VoiceState::Paused);
        live.own_fade = Some(fade);
        self.voices_dirty = true;
        (AudioLogKind::Pause, fade)
      }
      VoiceChange::Resume(fade) => {
        if state != VoiceState::Paused {
          return;
        }
        let fade = fade.max(MIN_FADE);
        live.voice.set_state(VoiceState::Playing);
        live.own_fade = Some(fade);
        self.voices_dirty = true;
        // Its first sounding takes its fade-in, if that is longer
        let fade = match live.unheard {
          Some(fade_in) if !live.held => fade_in.max(fade),
          _ => fade,
        };
        (AudioLogKind::Resume, fade)
      }
      VoiceChange::Volume(db, fade) => {
        let fade = change_over(fade);
        live.game_db = clamp_offset(db, GAIN_SPAN);
        let heard = live.heard_db();
        live.playback.set_volume(heard, tween(fade));
        (AudioLogKind::Gain, fade)
      }
      VoiceChange::Pitch(semitones, fade) => {
        let fade = change_over(fade);
        live.game_pitch = clamp_offset(semitones, PITCH_SPAN);
        live.playback.set_rate(live.rate(), tween(fade));
        (AudioLogKind::Rate, fade)
      }
    };
    self.record(index, kind, fade, server);
  }

  /// Warn about the first error the device's output stream reports; the
  /// engine carries on (kira restarts the stream), and later errors are not
  /// warned about
  fn report_stream_errors(&mut self) {
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(Manager::Device(manager)) = &mut self.manager {
      while let Some(error) = manager.backend_mut().pop_error() {
        warn_stream_error(&mut self.stream_error_warned, &error);
      }
    }
    #[cfg(test)]
    for error in std::mem::take(&mut self.injected_stream_errors) {
      warn_stream_error(&mut self.stream_error_warned, &error);
    }
  }

  /// Drop the voices kira reports stopped (played out, or faded out after a
  /// stop), marking their handles stopped, and keep the rest in start
  /// order. A stream that failed to decode as it played is logged stopped
  /// and its clip warned about once
  fn retire_ended(&mut self, server: Option<&AssetServer>) {
    for index in 0..self.voices.live.len() {
      let live = &mut self.voices.live[index];
      // The state first: a stream's error is queued before kira stops it
      let ended = live.playback.state() == PlaybackState::Stopped;
      if let Some(error) = live.playback.take_error() {
        // Only a clip streams
        let failed = match &live.source {
          Source::Clip(clip) => Some((clip.clone(), live.version)),
          Source::Joined(_) => None,
        };
        // kira cuts a stream that fails to decode at its next chunk, with
        // no fade, and ignores a stop after that: log what it did
        if live.voice.state() != VoiceState::Stopping {
          live.voice.set_state(VoiceState::Stopping);
          self.record(index, AudioLogKind::Stop, Duration::ZERO, server);
        }
        if let Some((clip, version)) = failed {
          self.warn_undecodable(&clip, version, &error, server);
        }
      }
      if ended {
        self.voices.live[index].voice.set_state(VoiceState::Stopped);
      }
    }
    self.voices.live.retain(|live| !live.voice.is_stopped());
  }

  /// Mark `clip` at content `version` undecodable, so it is refused until it
  /// changes, warning once per version by its path
  fn warn_undecodable(
    &mut self,
    clip: &Handle<AudioClip>,
    version: u64,
    error: &str,
    server: Option<&AssetServer>,
  ) {
    if self.undecodable.insert(clip.id(), version) != Some(version) {
      let name = source_name(clip, server);
      crate::engine_warn!(
        module: "Audio",
        "{name}: the clip's stream could not be decoded ({error}); it is not played again until it reloads"
      );
    }
  }

  /// Log `kind` for live voice `index`, as it stands, if the log is on
  fn record(
    &mut self,
    index: usize,
    kind: AudioLogKind,
    fade: Duration,
    server: Option<&AssetServer>,
  ) {
    if !self.log.is_on() {
      return;
    }
    let Some(live) = self.voices.live.get(index) else {
      return;
    };
    let mut entry = self.entry(kind, live.voice.id(), &live.subject(), server);
    entry.fade = fade;
    self.log.push(entry);
  }

  /// A log entry about `subject` (an empty one when the log is off: nothing
  /// is pushed then)
  fn entry(
    &self,
    kind: AudioLogKind,
    voice: u32,
    subject: &Subject<'_>,
    server: Option<&AssetServer>,
  ) -> AudioLogEntry {
    let on = self.log.is_on();
    AudioLogEntry {
      kind,
      voice,
      sound: if on {
        owner_name(subject.sound, subject.music, server)
      } else {
        String::new()
      },
      source: if on {
        subject.source.name(server)
      } else {
        String::new()
      },
      bus: if on {
        self.buses.name(subject.bus).to_string()
      } else {
        String::new()
      },
      gain_db: subject.gain_db,
      pan: subject.pan,
      rate: subject.rate,
      fade: Duration::ZERO,
      reason: "",
    }
  }

  /// Log `kind` for `bus` (a voice 0 entry: its volume, and `fade`), if the
  /// log is on
  fn record_bus(&mut self, bus: usize, kind: AudioLogKind, fade: Duration) {
    if !self.log.is_on() {
      return;
    }
    self.log.push(AudioLogEntry {
      kind,
      voice: 0,
      sound: String::new(),
      source: String::new(),
      bus: self.buses.name(bus).to_string(),
      gain_db: self.buses.buses.get(bus).map_or(0.0, |bus| bus.applied),
      pan: 0.0,
      rate: 1.0,
      fade,
      reason: "",
    });
  }
}

/// The engine's voices outlive it as handles: with kira's manager gone,
/// nothing plays, so they report stopped
impl Drop for AudioEngine {
  fn drop(&mut self) {
    for live in &self.voices.live {
      live.voice.set_state(VoiceState::Stopped);
    }
    for pending in &self.pending {
      pending.voice.set_state(VoiceState::Stopped);
    }
  }
}

/// Run `f` as scene `scope`'s code (a callback, one of its systems, or its
/// UI nodes' observers): the voices it plays belong to that scene, and all
/// but its one-shots nearly over (or on `ui`) stop as it exits. The
/// changes it made to voices are applied as it ends, so those made to a
/// voice it started reach kira with the start. A world without an audio
/// engine just runs `f`
pub(crate) fn in_scene<R>(world: &mut World, scope: u64, f: impl FnOnce(&mut World) -> R) -> R {
  let previous = world
    .get_resource_mut::<AudioEngine>()
    .map(|mut engine| engine.set_scope(Some(scope)));
  let result = f(world);
  if let Some(previous) = previous {
    apply_queued(world);
    if let Some(mut engine) = world.get_resource_mut::<AudioEngine>() {
      engine.set_scope(previous);
    }
  }
  result
}

/// Apply the voices' queued changes now, if there are any
fn apply_queued(world: &mut World) {
  if !world
    .get_resource::<AudioEngine>()
    .is_some_and(AudioEngine::has_queued)
  {
    return;
  }
  let server = world.get_resource::<AssetServer>().cloned();
  if let Some(mut engine) = world.get_resource_mut::<AudioEngine>() {
    engine.apply_queued(server.as_ref());
  }
}

/// Scene `scope` exited: stop the loops it played, its one-shots paused or
/// held, and its one-shots with more than 2 s left (but on `ui`), each with
/// its fade, and let the rest of its playing one-shots finish
pub(crate) fn release_scene(world: &mut World, scope: u64) {
  world.try_resource_scope(|world: &mut World, mut engine: Mut<'_, AudioEngine>| {
    engine.release_scope(world, scope);
  });
}

/// The scene stack holds the scenes `scopes` (those it does not update,
/// under a top scene that holds them): their voices, and the emitters',
/// are held on the buses that pause with the game until they update again
/// ([`AudioEngine::hold_scopes`]). Called as the stack changes and after
/// each step; the engine is touched only when the scenes held change
pub(crate) fn hold_scenes(world: &mut World, scopes: impl Iterator<Item = u64> + Clone) {
  let changed = world
    .get_resource::<AudioEngine>()
    .is_some_and(|engine| !engine.held_scopes.iter().copied().eq(scopes.clone()));
  if changed && let Some(mut engine) = world.get_resource_mut::<AudioEngine>() {
    engine.hold_scopes(scopes);
  }
}

/// A user gesture (a key, a mouse button or a touch) reached the game: on
/// the web, the first that the browser counts as the player's opens the
/// audio device ([`AudioEngine::unlock`]). Elsewhere the device opened at
/// start, and this does nothing
pub(crate) fn unlock(world: &mut World) {
  let locked = world
    .get_resource::<AudioEngine>()
    .is_some_and(AudioEngine::is_locked);
  if !locked || !user_activated() {
    return;
  }
  world.try_resource_scope(|world: &mut World, mut engine: Mut<'_, AudioEngine>| {
    engine.unlock(world);
  });
}

/// Whether the browser counts the event being handled as the player's
/// (transient user activation), so the page may start sound in it. A key,
/// a mouse button or a touch mostly does, but not all (Escape, a touch's
/// start, a pen's press), and an audio context made outside one stays
/// silent: kira keeps it to itself, so it could not be resumed. True where
/// the browser cannot say (`navigator.userActivation` is newer than some)
#[cfg(target_arch = "wasm32")]
fn user_activated() -> bool {
  use wasm_bindgen::{JsCast, JsValue};
  use web_sys::js_sys::Reflect;
  let Some(navigator) = web_sys::window().map(|window| window.navigator()) else {
    return true;
  };
  match Reflect::get(&navigator, &JsValue::from_str("userActivation")) {
    Ok(activation) if activation.is_object() => activation
      .unchecked_into::<web_sys::UserActivation>()
      .is_active(),
    _ => true,
  }
}

/// Only the web waits for a gesture
#[cfg(not(target_arch = "wasm32"))]
fn user_activated() -> bool {
  true
}

/// Stop the music as a scene's `SceneCleanup::MUSIC` does, fading it out over
/// its track's crossfade. A world without an audio engine is left alone
pub(crate) fn stop_scene_music(world: &mut World) {
  world.try_resource_scope(|world: &mut World, mut engine: Mut<'_, AudioEngine>| {
    engine.stop_music_with_crossfade(world);
  });
}

/// Whether a track can start
enum Readiness {
  /// It is loaded, with its clip
  Ready,
  /// It, or its clip, is still loading
  Loading,
  /// It, or its clip, did not load
  Failed,
}

/// Whether `music` can start: it and its clip loaded (a file's music is
/// inserted only once its clip has loaded; one made in code may name a clip
/// still loading)
fn readiness(world: &World, music: &Handle<Music>, server: Option<&AssetServer>) -> Readiness {
  let Some(def) = world
    .get_resource::<Assets<Music>>()
    .and_then(|musics| musics.get(music))
  else {
    let loading = server.is_some_and(|server| matches!(server.state(music), LoadState::Loading));
    return if loading {
      Readiness::Loading
    } else {
      Readiness::Failed
    };
  };
  match def.data() {
    MusicData::Concatenated(_) => Readiness::Ready,
    MusicData::Looping { clip, .. } => {
      let clips = world.get_resource::<Assets<AudioClip>>();
      if clips.is_some_and(|clips| clips.get(clip).is_some()) {
        Readiness::Ready
      } else if server.is_some_and(|server| matches!(server.state(clip), LoadState::Loading)) {
        Readiness::Loading
      } else {
        Readiness::Failed
      }
    }
  }
}

/// Whether a clip of `sound` is still loading (a definition made in code may
/// name clips still loading)
fn clips_loading(world: &World, sound: &Sound, server: Option<&AssetServer>) -> bool {
  let Some(server) = server else {
    return false;
  };
  let clips = world.get_resource::<Assets<AudioClip>>();
  sound.clips.iter().any(|clip| {
    clips.is_none_or(|clips| clips.get(&clip.clip).is_none())
      && matches!(server.state(&clip.clip), LoadState::Loading)
  })
}

/// Whether `sound` is still loading: the definition, or once it has landed
/// a clip of its
fn sound_loading(world: &World, sound: &Handle<Sound>, server: Option<&AssetServer>) -> bool {
  match world
    .get_resource::<Assets<Sound>>()
    .and_then(|sounds| sounds.get(sound))
  {
    Some(def) => clips_loading(world, def, server),
    None => server.is_some_and(|server| matches!(server.state(sound), LoadState::Loading)),
  }
}

/// What `music` (`handle`'s) plays, and its loop in seconds of that: its
/// joined intro and loop, or its clip over its loop region
fn music_source<'a>(
  world: &World,
  handle: &'a Handle<Music>,
  music: &'a Music,
) -> (SourceRef<'a>, Range<f64>) {
  match music.data() {
    MusicData::Concatenated(data) => (SourceRef::Joined(handle), joined_loop(data)),
    MusicData::Looping {
      clip,
      loop_start,
      loop_end,
    } => {
      let length = world
        .get_resource::<Assets<AudioClip>>()
        .and_then(|clips| clips.get(clip))
        .map_or(0.0, |clip| clip.duration().as_secs_f64());
      (
        SourceRef::Clip(clip),
        clip_loop(*loop_start, *loop_end, length),
      )
    }
  }
}

/// A joined track's loop, in seconds of it: from its loop region's start
/// (the intro's end) to its end
fn joined_loop(data: &StaticSoundData) -> Range<f64> {
  let rate = f64::from(data.sample_rate.max(1));
  let start = match data.settings.loop_region.map(|region| region.start) {
    Some(PlaybackPosition::Samples(frames)) => frames as f64 / rate,
    Some(PlaybackPosition::Seconds(seconds)) => seconds,
    None => 0.0,
  };
  start..data.frames.len() as f64 / rate
}

/// A one-clip track's loop in seconds, in a clip `length` long: from
/// `start` (0 for one that is not a time from 0) to `end`, or the clip's
/// end for none, or one not after the start
fn clip_loop(start: f64, end: Option<f64>, length: f64) -> Range<f64> {
  let start = if start.is_finite() && start >= 0.0 {
    start
  } else {
    0.0
  };
  let end = end
    .filter(|end| end.is_finite() && *end > start)
    .unwrap_or(length);
  start..end
}

/// A planned bus's gain layers: the game's volume for it, or the mixer's,
/// the player's setting it carries, the active snapshot's and ducks'
/// levels now, and on `master` the focus loss's `focus_db`
fn layers_of(
  planned: &PlannedBus<'_>,
  settings: &VolumeSettings,
  volumes: &FastMap<String, f32>,
  blend: &Blend,
  ducks: &[DuckState],
  focus_db: f32,
) -> GainLayers {
  GainLayers {
    base: volumes
      .get(planned.name)
      .copied()
      .unwrap_or_else(|| planned.def.map_or(0.0, |def| def.volume_db)),
    setting: planned
      .setting
      .map_or(0.0, |kind| volume_to_db(settings.get(kind))),
    snapshot: blend.level(planned.name).db,
    duck: ducks
      .iter()
      .filter(|duck| duck.def.target == planned.name)
      .map(|duck| duck.depth)
      .sum(),
    focus: if planned.parent.is_none() {
      focus_db
    } else {
      0.0
    },
  }
}

/// `master`'s focus layer in dB: what `rule` does while the window is not
/// `focused` (a duck that is not a cut in dB does nothing)
fn focus_db(focused: bool, rule: FocusLoss) -> f32 {
  match rule {
    _ if focused => 0.0,
    FocusLoss::Keep => 0.0,
    FocusLoss::Duck(db) if db.is_nan() => 0.0,
    FocusLoss::Duck(db) => db.clamp(SILENCE_DB, 0.0),
    FocusLoss::Mute => SILENCE_DB,
  }
}

/// Which voices a limit counts
#[derive(Debug, Clone, Copy)]
enum Scope {
  /// A definition's voices on a bus's subtree
  Sound(AssetId<Sound>, usize),
  /// Every voice on a bus's subtree
  Subtree(usize),
  /// Every voice on a bus's own track
  Track(usize),
}

/// The voices one play may stop to make room, planned before any is
/// stopped, so a play a later limit refuses stops none
struct Room<'a> {
  live: &'a [LiveVoice],
  buses: &'a BusTree,
  victims: &'a mut Vec<Victim>,
  frame: u64,
  listener: Option<&'a Listener>,
  /// The new play's priority
  priority: u8,
  /// The new play is the music player's: only it may stop music
  music: bool,
}

impl Room<'_> {
  /// Keep `scope` within `max` with one more voice, planning stops by
  /// `rule` (or, with none, the cap's: the least important voice, then the
  /// quietest, then the oldest) until there is room. False if `rule` is
  /// `PreventNew`, or no voice may be stopped
  fn fit(&mut self, scope: Scope, max: usize, rule: Option<LimitRule>) -> bool {
    loop {
      let count = (0..self.live.len())
        .filter(|&index| self.counts(index, scope))
        .count();
      if count < max {
        return true;
      }
      if rule == Some(LimitRule::PreventNew) {
        return false;
      }
      let Some(index) = self.victim(scope, rule) else {
        return false;
      };
      let fade = match rule {
        Some(_) => self.live[index].fade_out.max(MIN_FADE),
        None => MIN_FADE,
      };
      self.victims.push(Victim { index, fade });
    }
  }

  /// Whether voice `index` counts against `scope`'s limit: in it, playing or
  /// paused, and not already planned to stop
  fn counts(&self, index: usize, scope: Scope) -> bool {
    let Some(live) = self.live.get(index) else {
      return false;
    };
    let in_scope = match scope {
      Scope::Sound(id, bus) => live.sound == Some(id) && self.buses.within(live.bus, bus),
      Scope::Subtree(bus) => self.buses.within(live.bus, bus),
      Scope::Track(bus) => live.bus == bus,
    };
    in_scope
      && !matches!(
        live.voice.state(),
        VoiceState::Stopping | VoiceState::Stopped
      )
      && !self.victims.iter().any(|victim| victim.index == index)
  }

  /// The voice `rule` stops in `scope`, among those the new play may stop:
  /// never a more important one, nor an equally important one started this
  /// frame (stopping it would only stack two starts of a burst), and never
  /// the music for a play that is not music. Voices are in start order, so
  /// the oldest of equals wins
  fn victim(&self, scope: Scope, rule: Option<LimitRule>) -> Option<usize> {
    let mut chosen: Option<usize> = None;
    for index in 0..self.live.len() {
      let live = &self.live[index];
      let may_stop = (self.music || live.music.is_none())
        && (live.priority < self.priority
          || (live.priority == self.priority && live.frame != self.frame));
      if !may_stop || !self.counts(index, scope) {
        continue;
      }
      let better = match chosen.map(|at| &self.live[at]) {
        None => true,
        Some(best) => match rule {
          Some(LimitRule::StopOldest | LimitRule::PreventNew) => false,
          // As heard: its place's falloff counts
          Some(LimitRule::StopQuietest) => live.heard_db() < best.heard_db(),
          Some(LimitRule::StopFarthest) => self.distance(live) > self.distance(best),
          Some(LimitRule::StopLowestPriority) => live.priority < best.priority,
          None => {
            live.priority < best.priority
              || (live.priority == best.priority && live.heard_db() < best.heard_db())
          }
        },
      };
      if better {
        chosen = Some(index);
      }
    }
    chosen
  }

  /// A voice's distance from the listener: 0 without a position, or while
  /// there is no listener
  fn distance(&self, live: &LiveVoice) -> f32 {
    match (live.position, self.listener) {
      (Some(position), Some(listener)) => listener.hear(position).0,
      _ => 0.0,
    }
  }
}

/// What a play counts against its `per_frame`: its definition, or the clip
/// played directly
fn start_key(request: &Request<'_>) -> UntypedAssetId {
  request
    .sound
    .map_or_else(|| request.source.untyped(), AssetId::untyped)
}

/// Which of `clips` a play picks: by weight, avoiding the latest
/// `no_repeat` picks in `recent` (never every clip). A weight that is not a
/// number, or is below 0, counts as 0. If the clips left all weigh 0, the
/// avoidance gives way (a clip of weight 0 never plays); if every clip
/// weighs 0, each is as likely. `None` with no clips
fn pick_clip(
  rng: &mut SmallRng,
  clips: &[SoundClip],
  recent: &[usize],
  no_repeat: u8,
) -> Option<usize> {
  let last = clips.len().checked_sub(1)?;
  let avoid = usize::from(no_repeat).min(last);
  let avoided = &recent[recent.len().saturating_sub(avoid)..];
  let left = |index: usize| !avoided.contains(&index);
  weighted(rng, clips, left)
    .or_else(|| weighted(rng, clips, |_| true))
    .or_else(|| {
      let count = (0..clips.len()).filter(|&index| left(index)).count();
      let nth = rng.gen_range(0..count.max(1));
      (0..clips.len()).filter(|&index| left(index)).nth(nth)
    })
}

/// A pick by weight among the clips `allowed`; `None` if they weigh nothing
fn weighted(
  rng: &mut SmallRng,
  clips: &[SoundClip],
  allowed: impl Fn(usize) -> bool,
) -> Option<usize> {
  let weight = |index: usize| -> f64 {
    let weight = clips[index].weight;
    if allowed(index) && weight.is_finite() && weight > 0.0 {
      f64::from(weight)
    } else {
      0.0
    }
  };
  let total: f64 = (0..clips.len()).map(weight).sum();
  if !(total > 0.0 && total.is_finite()) {
    return None;
  }
  let mut at = rng.gen_range(0.0..total);
  let mut chosen = None;
  for index in 0..clips.len() {
    let weight = weight(index);
    if weight <= 0.0 {
      continue;
    }
    chosen = Some(index);
    if at < weight {
      break;
    }
    at -= weight;
  }
  chosen
}

/// A random offset up to `range` either side of 0 (0 for a range that is
/// not a number above 0)
fn spread(rng: &mut SmallRng, range: f32) -> f32 {
  if range.is_finite() && range > 0.0 {
    rng.gen_range(-1.0f32..=1.0) * range
  } else {
    0.0
  }
}

/// What part of a clip plays, and what part of that loops, in frames
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Span {
  /// The frames played, `start..end` of the clip; all of it when `None`
  slice: Option<(usize, usize)>,
  /// The frames looped, from the start of what plays; no loop when `None`
  loop_region: Option<(usize, usize)>,
}

impl Span {
  /// `start`'s region and loop of a clip of `frames` frames at `rate`. kira
  /// reads past the end of a slice that runs off its clip, and loops
  /// forever on an empty loop, so each part is cut to what holds it and is
  /// never empty: a region that is not a span plays the whole clip, a loop
  /// region that is not loops all that plays, and nothing empty loops
  fn of(start: &Start<'_>, rate: u32, frames: usize) -> Span {
    let slice = start.region.and_then(|region| within(region, rate, frames));
    let played = slice.map_or(frames, |(from, to)| to - from);
    let loop_region = (start.looping && played > 0).then(|| {
      start
        .loop_region
        .and_then(|looped| within(looped, rate, played))
        .unwrap_or((0, played))
    });
    Span { slice, loop_region }
  }

  /// `position` (seconds from the start of what plays, at `rate`), kept in
  /// what plays of a clip of `frames` frames: past its end it wraps into
  /// the loop (a loop moved to a clip reloaded shorter), or else is the end
  fn wrap(&self, position: f64, rate: u32, frames: usize) -> f64 {
    let rate = f64::from(rate.max(1));
    let played = self.slice.map_or(frames, |(from, to)| to - from);
    let at = (position.max(0.0) * rate) as usize;
    if at < played {
      return position;
    }
    match self.loop_region {
      Some((start, end)) if end > start => (start + (at - start) % (end - start)) as f64 / rate,
      _ => played as f64 / rate,
    }
  }
}

/// `span`, in seconds, as frames at `rate` cut to the first `frames`;
/// `None` if it is not a span, or nothing of it is left
fn within(span: &Range<f64>, rate: u32, frames: usize) -> Option<(usize, usize)> {
  let is_span = span.start.is_finite() && span.end.is_finite() && span.start >= 0.0;
  if !is_span || span.end <= span.start {
    return None;
  }
  let at = |seconds: f64| ((seconds * f64::from(rate)).round() as usize).min(frames);
  let (from, to) = (at(span.start), at(span.end));
  (to > from).then_some((from, to))
}

/// Frames `start..end` as kira's region
fn region((start, end): (usize, usize)) -> Region {
  Region {
    start: PlaybackPosition::Samples(start),
    end: EndPosition::Custom(PlaybackPosition::Samples(end)),
  }
}

/// A clip's asset path, or its id for a clip made in code
fn source_name(clip: &Handle<AudioClip>, server: Option<&AssetServer>) -> String {
  match server.and_then(|server| server.path(clip)) {
    Some(path) => path.to_string(),
    None => format!("{:?}", clip.id()),
  }
}

/// A definition's asset path, or its id for one made in code
fn sound_name(sound: AssetId<Sound>, server: Option<&AssetServer>) -> String {
  match server.and_then(|server| server.path(sound)) {
    Some(path) => path.to_string(),
    None => format!("{sound:?}"),
  }
}

/// A music's asset path, or its id for one made in code
fn music_name(music: AssetId<Music>, server: Option<&AssetServer>) -> String {
  match server.and_then(|server| server.path(music)) {
    Some(path) => path.to_string(),
    None => format!("{music:?}"),
  }
}

/// What a voice plays, by name: its sound definition's, or its music's;
/// empty for a clip played directly
fn owner_name(
  sound: Option<AssetId<Sound>>,
  music: Option<AssetId<Music>>,
  server: Option<&AssetServer>,
) -> String {
  match (sound, music) {
    (Some(sound), _) => sound_name(sound, server),
    (None, Some(music)) => music_name(music, server),
    (None, None) => String::new(),
  }
}

/// A live voice by name in a warning: its definition's path, or its music's,
/// or else its clip's (or joined music's, made in code)
fn voice_name(live: &LiveVoice, server: Option<&AssetServer>) -> String {
  let path = match (live.sound, live.music) {
    (Some(sound), _) => server.and_then(|server| server.path(sound)),
    (None, Some(music)) => server.and_then(|server| server.path(music)),
    (None, None) => None,
  };
  path.map_or_else(
    || live.source.as_ref().name(server),
    |path| path.to_string(),
  )
}

/// Warn about `error` unless a stream error was warned about already
#[cfg(any(test, not(target_arch = "wasm32")))]
fn warn_stream_error(warned: &mut bool, error: &dyn Display) {
  if std::mem::replace(warned, true) {
    return;
  }
  crate::engine_warn!(
    module: "Audio",
    "the audio output stream reported an error ({error}); sound carries on, and later stream errors are not reported"
  );
}

/// A gain in dB kira can play: NaN is 0 dB, the rest clamped to
/// [`GAIN_RANGE`]
fn clamp_gain(db: f32) -> f32 {
  if db.is_nan() {
    0.0
  } else {
    db.clamp(GAIN_RANGE.0, GAIN_RANGE.1)
  }
}

/// A change to a level (dB or semitones) that a sum is then clamped by:
/// NaN is none, the rest clamped to ±`span`, the whole of what the sum may
/// cover
fn clamp_offset(change: f32, span: f32) -> f32 {
  if change.is_nan() {
    0.0
  } else {
    change.clamp(-span, span)
  }
}

/// A pitch in semitones: NaN is 0, the rest clamped to ±[`PITCH_RANGE`]
fn clamp_pitch(semitones: f32) -> f32 {
  if semitones.is_nan() {
    0.0
  } else {
    semitones.clamp(-PITCH_RANGE, PITCH_RANGE)
  }
}

/// The playback rate of a pitch in semitones
fn rate_of(semitones: f32) -> f64 {
  PlaybackRate::from(Semitones(f64::from(semitones))).0
}

/// A voice's fade-in as kira starts it: none for no time, as a sound
/// starting at its beginning does not click
fn fade_in_of(duration: Duration) -> Option<Tween> {
  (!duration.is_zero()).then(|| fade_from_silence(duration))
}

/// The curve of a fade to or from silence: kira fades a sound between -60
/// and 0 dB in a line of dB, which stays near silence for most of a fade in
/// and drops most of the way at once in a fade out, so a crossfade dips
/// about 27 dB in its middle. A fade out eased in (and a fade in eased out)
/// at this power is about -3.75 dB halfway, so two crossfading hold their
/// level as an equal-power crossfade does
const FADE_POWER: i32 = 4;

/// A fade from silence over `duration`: eased out at [`FADE_POWER`], or a
/// line for one of [`MIN_FADE`] or less, too short to hear its curve
fn fade_from_silence(duration: Duration) -> Tween {
  shaped(duration, Easing::OutPowi(FADE_POWER))
}

/// A fade to silence over `duration`: eased in at [`FADE_POWER`], or a line
/// for one of [`MIN_FADE`] or less
fn fade_to_silence(duration: Duration) -> Tween {
  shaped(duration, Easing::InPowi(FADE_POWER))
}

fn shaped(duration: Duration, easing: Easing) -> Tween {
  if duration <= MIN_FADE {
    return tween(duration);
  }
  Tween {
    duration,
    easing,
    ..Default::default()
  }
}

/// A linear tween over `duration`, starting now: a change between two
/// heard levels, even in dB
fn tween(duration: Duration) -> Tween {
  Tween {
    duration,
    ..Default::default()
  }
}

/// How many whole chunks of the mock backend `real_delta` adds to what is
/// `owed` (frame-nanoseconds), keeping the remainder owed
fn chunks_due(owed: &mut u128, real_delta: Duration) -> u128 {
  let chunk = CHUNK_FRAMES as u128 * 1_000_000_000;
  *owed += real_delta.min(MAX_MOCK_STEP).as_nanos() * u128::from(MOCK_SAMPLE_RATE);
  let chunks = *owed / chunk;
  *owed %= chunk;
  chunks
}

#[cfg(test)]
mod tests {
  use std::time::Duration;

  use super::*;
  use crate::core::asset::{Assets, AssetsView, AudioClipSettings, ClipMode, MemorySource};
  use crate::core::audio::{AudioListener, AudioView, BusDef, Falloff};
  use crate::core::dev_log::{self, LogLevel};
  use crate::core::ecs::World;
  use crate::core::testing::{audio_step, audio_world, audio_world_over};
  use crate::core::time::Time;

  const FRAME: Duration = Duration::from_nanos(16_666_667);
  const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/audio");

  fn fixture(name: &str) -> Vec<u8> {
    match std::fs::read(format!("{FIXTURES}/{name}")) {
      Ok(bytes) => bytes,
      Err(error) => panic!("{name}: {error}"),
    }
  }

  fn files(names: &[&str]) -> MemorySource {
    let files = MemorySource::new();
    for name in names {
      files.insert(name, fixture(name));
    }
    files
  }

  /// `path` loaded to stream
  fn streamed(world: &mut World, path: &str) -> Handle<AudioClip> {
    let settings = AudioClipSettings {
      mode: ClipMode::Stream,
    };
    match AssetsView::new(world).load_now_with::<AudioClip, _>(path, settings) {
      Ok(clip) => clip,
      Err(error) => panic!("{path} loads to stream: {}", error.report()),
    }
  }

  /// Step until `voice` stops: a stream decodes on its own thread
  fn wait_until_stopped(world: &mut World, voice: &Voice) {
    for _ in 0..500 {
      if voice.is_stopped() {
        break;
      }
      audio_step(world, Duration::from_millis(10));
      std::thread::sleep(Duration::from_millis(2));
    }
    assert!(voice.is_stopped(), "{voice:?}");
  }

  fn log(world: &World) -> Vec<AudioLogEntry> {
    world.resource::<AudioEngine>().log().to_vec()
  }

  /// Warnings that a clip at `path` could not be decoded as a stream
  fn undecodable_warnings(path: &str) -> usize {
    let named = format!("{path}:");
    warnings(|message| message.starts_with(&named) && message.contains("could not be decoded"))
  }

  fn tone(world: &mut World, mode: ClipMode) -> Handle<AudioClip> {
    let settings = AudioClipSettings { mode };
    match AssetsView::new(world).load_now_with::<AudioClip, _>("tone.wav", settings) {
      Ok(clip) => clip,
      Err(error) => panic!("{}", error.report()),
    }
  }

  fn second(world: &mut World) -> Handle<AudioClip> {
    world
      .resource_mut::<Assets<AudioClip>>()
      .add(AudioClip::from_frames(48_000, &vec![[0.1, 0.1]; 48_000]).unwrap())
  }

  fn warnings(matching: impl Fn(&str) -> bool) -> usize {
    dev_log::get_entries()
      .iter()
      .filter(|entry| entry.level == LogLevel::Warn && matching(&entry.message))
      .count()
  }

  /// The mock backend renders on the main thread, where a device renders on
  /// its own: the time it took is reported, so a bench can leave it out
  #[test]
  fn the_mock_backend_reports_its_render_time() {
    let mut world = audio_world();
    let clip = second(&mut world);
    AudioView::new(&mut world).play(&clip);
    audio_step(&mut world, FRAME);
    let rendered = world.resource::<AudioEngine>().mock_render_time();
    assert!(rendered > Duration::ZERO, "a frame was rendered");
    audio_step(&mut world, Duration::ZERO);
    assert_eq!(
      world.resource::<AudioEngine>().mock_render_time(),
      Duration::ZERO,
      "nothing was rendered"
    );
  }

  #[test]
  fn a_clip_plays_on_sfx_and_the_log_shows_it() {
    let mut world = audio_world_over(files(&["tone.wav"]));
    let clip = tone(&mut world, ClipMode::Auto);
    let voice = AudioView::new(&mut world).play(&clip);
    assert!(voice.is_playing());
    let log = world.resource::<AudioEngine>().log().to_vec();
    let Some(first) = log.first() else {
      panic!("the log is empty");
    };
    assert_eq!(first.kind, AudioLogKind::Start);
    assert_eq!(first.voice, voice.id());
    assert_eq!(first.source, "tone.wav");
    assert_eq!(first.bus, "sfx");
    assert_eq!(first.gain_db, 0.0);
    assert_eq!(first.rate, 1.0);
    assert_eq!(first.fade, Duration::ZERO);
  }

  #[test]
  fn voices_on_counts_a_bus() {
    let mut world = audio_world();
    // A clip starts once a frame: one each
    let clips: Vec<Handle<AudioClip>> = (0..4).map(|_| second(&mut world)).collect();
    let voices: Vec<Voice> = clips[..3]
      .iter()
      .map(|clip| AudioView::new(&mut world).play(clip))
      .collect();
    let mut audio = AudioView::new(&mut world);
    assert_eq!(audio.voices_on("sfx"), 3);
    assert_eq!(audio.voices_on("master"), 3, "master counts its subtree");
    assert_eq!(audio.voices_on("music"), 0);
    assert_eq!(audio.voices_on("no such bus"), 0);
    audio.play(&clips[3]);
    assert_eq!(audio.voices_on("sfx"), 4);
    voices[0].stop(Duration::ZERO);
    audio_step(&mut world, Duration::from_millis(50));
    assert_eq!(AudioView::new(&mut world).voices_on("sfx"), 3);
  }

  #[test]
  fn a_device_that_fails_falls_back_to_the_mock_with_one_warning() {
    let mut world = audio_world();
    let mut engine = AudioEngine::new_failing_device();
    assert!(engine.is_mock());
    engine.set_logging(true);
    world.insert_resource(engine);
    let clip = second(&mut world);
    let voice = AudioView::new(&mut world).play(&clip);
    for _ in 0..5 {
      audio_step(&mut world, FRAME);
    }
    assert!(voice.is_playing(), "the mock plays: {voice:?}");
    assert_eq!(AudioView::new(&mut world).voices_on("sfx"), 1);
    let about_the_device =
      warnings(|message| message.contains("audio device") && message.contains(FAILING_DEVICE));
    assert_eq!(about_the_device, 1);
  }

  #[test]
  fn a_stream_error_warns_once() {
    let mut world = audio_world();
    {
      let mut engine = world.resource_mut::<AudioEngine>();
      engine.inject_stream_error("unplugged (stream error seam 1)");
      engine.inject_stream_error("unplugged (stream error seam 2)");
    }
    audio_step(&mut world, FRAME);
    world
      .resource_mut::<AudioEngine>()
      .inject_stream_error("unplugged (stream error seam 3)");
    audio_step(&mut world, FRAME);
    assert_eq!(
      warnings(|message| message.contains("(stream error seam")),
      1
    );
    // The engine carries on
    let clip = second(&mut world);
    assert!(AudioView::new(&mut world).play(&clip).is_playing());
  }

  /// A world over `path`, a WAV loaded to stream whose bytes are then
  /// replaced in code by `bytes` (of 22,050 frames at 22,050 Hz, it says),
  /// and the files it reads
  fn gone_bad(path: &str, bytes: &[u8]) -> (World, MemorySource, Handle<AudioClip>) {
    let files = MemorySource::new();
    files.insert(path, fixture("tone.wav"));
    let mut world = audio_world_over(files.clone());
    let clip = streamed(&mut world, path);
    if let Some(clip) = world.resource_mut::<Assets<AudioClip>>().get_mut(&clip) {
      let encoded = EncodedClip {
        bytes: Arc::from(bytes),
        frames: 22_050,
      };
      *clip = AudioClip::new(ClipData::Encoded(encoded), Duration::from_secs(1), 22_050);
    }
    (world, files, clip)
  }

  /// A world over `path`, a WAV loaded to stream whose bytes then go bad,
  /// so its stream cannot open
  fn garbled(path: &str) -> (World, Handle<AudioClip>) {
    let (world, _, clip) = gone_bad(path, b"not audio at all");
    (world, clip)
  }

  /// A surround clip streamed (made in code: the loader refuses one) fails
  /// as it plays. kira cuts the stream at its next chunk, with no fade (it
  /// cannot be smoothed), and the log says so; the clip is warned about
  /// once, by its path
  #[test]
  fn a_streamed_clip_that_cannot_decode_stops_and_warns_once() {
    let (mut world, _, clip) = gone_bad("surround-in-code.wav", &fixture("surround.wav"));
    let voice = AudioView::new(&mut world).play(&clip);
    // It starts: the stream decodes on its own thread, and fails there
    assert!(voice.is_playing(), "{voice:?}");
    wait_until_stopped(&mut world, &voice);
    let log = log(&world);
    let stops: Vec<Duration> = log
      .iter()
      .filter(|entry| entry.kind == AudioLogKind::Stop && entry.voice == voice.id())
      .map(|entry| entry.fade)
      .collect();
    assert_eq!(stops, [Duration::ZERO], "{log:#?}");
    assert_eq!(AudioView::new(&mut world).voices_on("sfx"), 0);
    assert_eq!(undecodable_warnings("surround-in-code.wav"), 1);
  }

  /// Once a clip's stream has failed to decode, it is not played again (no
  /// decoder thread is started for it) until it reloads
  #[test]
  fn a_clip_known_undecodable_is_refused_until_it_reloads() {
    let (mut world, files, clip) = gone_bad("undecodable-reloaded.wav", b"not audio at all");
    let first = AudioView::new(&mut world).play(&clip);
    wait_until_stopped(&mut world, &first);
    let again = AudioView::new(&mut world).play(&clip);
    assert!(again.is_stopped(), "{again:?}");
    assert_eq!(again.id(), 0);
    let last = log(&world).pop();
    assert_eq!(
      last.as_ref().map(|entry| entry.kind),
      Some(AudioLogKind::Refused)
    );
    assert_eq!(
      last.map(|entry| entry.source).as_deref(),
      Some("undecodable-reloaded.wav")
    );
    // A reload (of the file, a stereo WAV) clears the mark
    let version = |world: &World| world.resource::<Assets<AudioClip>>().version(&clip);
    let before = version(&world);
    files.insert("undecodable-reloaded.wav", fixture("tone.wav"));
    world
      .resource::<AssetServer>()
      .reload("undecodable-reloaded.wav");
    for _ in 0..500 {
      if version(&world) != before {
        break;
      }
      audio_step(&mut world, Duration::from_millis(10));
      std::thread::sleep(Duration::from_millis(2));
    }
    assert_ne!(version(&world), before, "the clip reloaded");
    let reloaded = AudioView::new(&mut world).play(&clip);
    assert!(reloaded.is_playing(), "{reloaded:?}");
    wait_until_stopped(&mut world, &reloaded);
    assert_eq!(undecodable_warnings("undecodable-reloaded.wav"), 1);
  }

  /// A stream opens on its decoder thread, so one that cannot open starts
  /// (nothing is read on the calling thread), then ends: cut with no fade,
  /// as kira cuts a stream that fails, and warned about once
  #[test]
  fn a_stream_that_cannot_open_reports_once_and_ends() {
    let (mut world, clip) = garbled("cannot-open.wav");
    let voice = AudioView::new(&mut world).play(&clip);
    assert!(voice.is_playing(), "{voice:?}");
    wait_until_stopped(&mut world, &voice);
    assert_eq!(undecodable_warnings("cannot-open.wav"), 1);
    let entries: Vec<(AudioLogKind, Duration)> = log(&world)
      .iter()
      .filter(|entry| entry.voice == voice.id())
      .map(|entry| (entry.kind, entry.fade))
      .collect();
    assert_eq!(
      entries,
      [
        (AudioLogKind::Start, Duration::ZERO),
        (AudioLogKind::Stop, Duration::ZERO)
      ]
    );
    assert_eq!(AudioView::new(&mut world).voices_on("sfx"), 0);
  }

  /// A stream that cannot open is warned about by its path whether or not
  /// the log is on
  #[test]
  fn a_clip_that_cannot_open_is_named_by_its_path_with_the_log_off() {
    let (mut world, clip) = garbled("garbled-at-play.wav");
    world.resource_mut::<AudioEngine>().set_logging(false);
    let voice = AudioView::new(&mut world).play(&clip);
    wait_until_stopped(&mut world, &voice);
    assert_eq!(undecodable_warnings("garbled-at-play.wav"), 1);
  }

  /// Where `voice` is, in seconds from the start of what it plays
  fn position(world: &World, voice: &Voice) -> f64 {
    let engine = world.resource::<AudioEngine>();
    match engine.voices.index_of(voice.id()) {
      Some(index) => engine.voices.live[index].playback.position(),
      None => panic!("{voice:?} is not playing"),
    }
  }

  /// Step frames of game time, leaving the decoder threads time, until
  /// `voice` is `seconds` in. A stream advances only once its decoder thread
  /// has given it frames, so this waits for that however long it takes (at
  /// most 2,000 frames)
  fn advance_to(world: &mut World, voice: &Voice, seconds: f64) {
    for _ in 0..2000 {
      if position(world, voice) >= seconds {
        return;
      }
      audio_step(world, FRAME);
      std::thread::sleep(Duration::from_millis(1));
    }
    panic!("{voice:?} did not reach {seconds} s");
  }

  /// A Vorbis stream started partway in plays to the end of the file and
  /// ends. Here a one-shot is started again where it is, 2 s in, as a
  /// reload switches a loop
  #[test]
  fn a_streamed_vorbis_one_shot_started_partway_finishes() {
    let mut world = audio_world_over(files(&["tone-3s.ogg"]));
    let clip = streamed(&mut world, "tone-3s.ogg");
    let voice = AudioView::new(&mut world).play(&clip);
    advance_to(&mut world, &voice, 2.0);
    let restarted = world.resource_scope(|world, mut engine: Mut<AudioEngine>| {
      let index = engine.voices.index_of(voice.id());
      index.map(|index| engine.restart(world, index, world.get_resource::<AssetServer>()))
    });
    assert!(matches!(restarted, Some(Ok(()))), "{restarted:?}");
    let started = position(&world, &voice);
    // Once the new stream has frames, about a second of the file is left
    advance_to(&mut world, &voice, started + 0.01);
    for _ in 0..90 {
      if voice.is_stopped() {
        break;
      }
      audio_step(&mut world, FRAME);
      std::thread::sleep(Duration::from_millis(2));
    }
    assert!(voice.is_stopped(), "it ends within 1.5 s: {voice:?}");
  }

  /// A Vorbis loop started partway in (a reload switches it to the saved
  /// file where it is, 2 s in) wraps to its start at the end of the file
  #[test]
  fn a_streamed_vorbis_loop_started_partway_wraps() {
    let files = files(&["tone-3s.ogg"]);
    let mut world = audio_world_over(files.clone());
    let clip = streamed(&mut world, "tone-3s.ogg");
    let sound = world.resource_mut::<Assets<Sound>>().add(Sound {
      clips: vec![SoundClip::new(clip.clone())],
      looping: true,
      ..Default::default()
    });
    let voice = AudioView::new(&mut world).play(&sound);
    advance_to(&mut world, &voice, 2.0);
    let version = |world: &World| world.resource::<Assets<AudioClip>>().version(&clip);
    let before = version(&world);
    files.insert("tone-3s.ogg", fixture("tone-3s.ogg"));
    world.resource::<AssetServer>().reload("tone-3s.ogg");
    for _ in 0..500 {
      if version(&world) != before {
        break;
      }
      audio_step(&mut world, Duration::ZERO);
      std::thread::sleep(Duration::from_millis(1));
    }
    assert_ne!(version(&world), before, "the clip reloaded");
    let switched = position(&world, &voice);
    assert!(switched >= 2.0, "it switched {switched} s in");
    // Once the new stream has frames, 1.5 s takes it past the end (about
    // 1 s on) and about half a second into the loop
    advance_to(&mut world, &voice, switched + 0.01);
    for _ in 0..90 {
      audio_step(&mut world, FRAME);
      std::thread::sleep(Duration::from_millis(2));
    }
    assert!(voice.is_playing(), "{voice:?}");
    let at = position(&world, &voice);
    assert!(at < 1.0, "it wrapped: {at} s in after 1.5 s");
  }

  /// A streamed clip's region is its decoder's whole stream: a region late
  /// in the file is decoded from there, not from the file's start
  #[test]
  fn a_streamed_region_decodes_only_itself() {
    let mut world = audio_world_over(files(&["tone-3s.ogg"]));
    let clip = streamed(&mut world, "tone-3s.ogg");
    let decoded = world.resource_mut::<AudioEngine>().count_stream_decoding();
    let mut late = SoundClip::new(clip);
    // From frame 110,250 of the file's 132,352
    late.region = Some(2.5..3.0);
    let sound = world.resource_mut::<Assets<Sound>>().add(Sound {
      clips: vec![late],
      ..Default::default()
    });
    let voice = AudioView::new(&mut world).play(&sound);
    advance_to(&mut world, &voice, 0.01);
    let count = decoded.load(std::sync::atomic::Ordering::Relaxed);
    assert!(
      count < 55_125,
      "{count} frames decoded by the time the region played, which starts at frame 110,250"
    );
  }

  /// A streamed region plays only its part: a one-shot ends at the region's
  /// end, and a loop wraps inside it, its position counted from the
  /// region's start
  #[test]
  fn a_streamed_region_plays_its_part_and_loops_inside_it() {
    let mut world = audio_world_over(files(&["tone-3s.ogg"]));
    let clip = streamed(&mut world, "tone-3s.ogg");
    let mut part = SoundClip::new(clip);
    part.region = Some(2.0..2.25);
    let sounds = [false, true].map(|looping| {
      world.resource_mut::<Assets<Sound>>().add(Sound {
        clips: vec![part.clone()],
        looping,
        ..Default::default()
      })
    });
    let once = AudioView::new(&mut world).play(&sounds[0]);
    let looped = AudioView::new(&mut world).play(&sounds[1]);
    advance_to(&mut world, &looped, 0.2);
    let (mut highest, mut wrapped) = (0.0f64, false);
    for _ in 0..2000 {
      audio_step(&mut world, FRAME);
      std::thread::sleep(Duration::from_millis(1));
      let at = position(&world, &looped);
      highest = highest.max(at);
      if at < 0.1 {
        wrapped = true;
        break;
      }
    }
    assert!(wrapped, "the loop wrapped");
    assert!(
      highest < 0.26,
      "it stayed in its 0.25 s region: {highest} s in"
    );
    assert!(looped.is_playing(), "{looped:?}");
    for _ in 0..60 {
      if once.is_stopped() {
        break;
      }
      audio_step(&mut world, FRAME);
      std::thread::sleep(Duration::from_millis(1));
    }
    assert!(
      once.is_stopped(),
      "the one-shot ended with its region: {once:?}"
    );
  }

  #[test]
  fn the_voice_table_keeps_the_order_voices_started_in() {
    let mut world = audio_world();
    let voices: Vec<Voice> = (0..4)
      .map(|_| {
        let clip = second(&mut world);
        AudioView::new(&mut world).play(&clip)
      })
      .collect();
    voices[0].stop(Duration::ZERO);
    voices[2].stop(Duration::ZERO);
    audio_step(&mut world, Duration::from_millis(50));
    let order: Vec<u32> = world
      .resource::<AudioEngine>()
      .voices
      .live
      .iter()
      .map(|live| live.voice.id())
      .collect();
    assert_eq!(order, [voices[1].id(), voices[3].id()]);
  }

  /// A voice kept after its engine is gone does not report itself playing
  #[test]
  fn dropping_the_engine_stops_its_voices() {
    let mut world = audio_world();
    let (one, other) = (second(&mut world), second(&mut world));
    let (playing, paused) = {
      let mut audio = AudioView::new(&mut world);
      (audio.play(&one), audio.play(&other))
    };
    paused.pause();
    audio_step(&mut world, FRAME);
    assert!(playing.is_playing() && !paused.is_stopped());
    drop(world);
    assert!(playing.is_stopped(), "{playing:?}");
    assert!(paused.is_stopped(), "{paused:?}");
  }

  #[test]
  fn a_short_streamed_clip_plays_to_its_end() {
    let mut world = audio_world_over(files(&["tone.wav"]));
    let clip = tone(&mut world, ClipMode::Stream);
    let voice = AudioView::new(&mut world).play(&clip);
    assert!(voice.is_playing());
    for _ in 0..500 {
      if voice.is_stopped() {
        break;
      }
      audio_step(&mut world, Duration::from_millis(10));
      std::thread::sleep(Duration::from_millis(2));
    }
    assert!(voice.is_stopped(), "a 250 ms stream ends: {voice:?}");
  }

  #[test]
  fn the_mock_advances_in_whole_chunks_and_carries_the_rest() {
    let mut owed = 0;
    // 800 frames at 48 kHz: 6 chunks of 128, 32 frames carried
    assert_eq!(chunks_due(&mut owed, FRAME), 6);
    let mut owed = 0;
    let second: u128 = (0..60).map(|_| chunks_due(&mut owed, FRAME)).sum();
    assert_eq!(second, 375, "a second of frames is 48,000 frames");
    // A frame after a long stall renders at most a second
    let mut owed = 0;
    assert_eq!(chunks_due(&mut owed, Duration::from_secs(5)), 375);
  }

  #[test]
  fn a_fixed_buffer_is_cut_to_what_the_device_supports() {
    use kira::backend::cpal::cpal::{BufferSize, SupportedBufferSize};
    let range = SupportedBufferSize::Range { min: 64, max: 4096 };
    assert_eq!(fixed_buffer(256, &range), BufferSize::Fixed(256));
    assert_eq!(fixed_buffer(16, &range), BufferSize::Fixed(64));
    assert_eq!(fixed_buffer(9000, &range), BufferSize::Fixed(4096));
    assert_eq!(
      fixed_buffer(256, &SupportedBufferSize::Unknown),
      BufferSize::Fixed(256),
      "a device that does not say takes it as given"
    );
  }

  /// Review M4: the buffer setting's warnings name the frames asked for
  #[test]
  fn the_buffer_setting_s_warnings_name_its_frames() {
    use kira::backend::cpal::cpal::{SampleFormat, SupportedBufferSize, SupportedStreamConfig};
    let supported =
      SupportedStreamConfig::new(2, 48_000, SupportedBufferSize::Unknown, SampleFormat::F32);
    let config = buffer_config(256, Ok(supported));
    let fixed = kira::backend::cpal::cpal::BufferSize::Fixed(256);
    assert!(config.is_ok_and(|(config, frames)| config.buffer_size == fixed && frames == 256));
    let unread = buffer_config(256, Err("the device is busy".to_string()));
    let Err(message) = unread else {
      panic!("a config that cannot be read is not used");
    };
    assert!(message.contains("audio_buffer_frames = 256"), "{message}");
    assert!(message.contains("the device is busy"), "{message}");
    let refused = buffer_refused(512, &"unsupported");
    assert!(
      refused.contains("512 frames") && !refused.contains("Fixed"),
      "{refused}"
    );
  }

  #[test]
  fn gain_and_pitch_are_clamped_and_nan_is_neutral() {
    assert_eq!(clamp_gain(f32::NAN), 0.0);
    assert_eq!(clamp_gain(f32::NEG_INFINITY), -60.0);
    assert_eq!(clamp_gain(100.0), 24.0);
    assert_eq!(clamp_pitch(f32::NAN), 0.0);
    assert_eq!(clamp_pitch(-100.0), -48.0);
    assert!((rate_of(12.0) - 2.0).abs() < 1e-12);
  }

  /// A position past what plays (a loop moved to a clip reloaded shorter)
  /// wraps into the loop, or else is the end
  #[test]
  fn a_position_past_what_plays_wraps_into_the_loop() {
    let looped = Span {
      slice: None,
      loop_region: Some((100, 400)),
    };
    assert_eq!(looped.wrap(0.25, 1_000, 400), 0.25);
    assert_eq!(looped.wrap(0.5, 1_000, 400), 0.2);
    assert_eq!(looped.wrap(1.25, 1_000, 400), 0.35);
    let sliced = Span {
      slice: Some((50, 350)),
      loop_region: None,
    };
    assert_eq!(sliced.wrap(0.5, 1_000, 400), 0.3);
  }

  /// An audio world as the web starts it: nothing opens until the first
  /// user gesture
  fn locked_world() -> World {
    let mut world = audio_world();
    let mut engine = AudioEngine::new_locked();
    engine.set_logging(true);
    world.insert_resource(engine);
    world
  }

  /// A loop of `clip` on `bus`, fading in over `fade_in`
  fn hum(
    world: &mut World,
    clip: &Handle<AudioClip>,
    bus: &str,
    fade_in: Duration,
  ) -> Handle<Sound> {
    world.resource_mut::<Assets<Sound>>().add(Sound {
      clips: vec![SoundClip::new(clip.clone())],
      bus: bus.to_string(),
      looping: true,
      fade_in,
      ..Default::default()
    })
  }

  fn entries(world: &World, kind: AudioLogKind) -> Vec<AudioLogEntry> {
    log(world)
      .into_iter()
      .filter(|entry| entry.kind == kind)
      .collect()
  }

  #[test]
  fn a_view_says_it_waits_for_the_gesture_until_the_unlock() {
    // Native and headless engines open at start: nothing to wait for
    let mut world = audio_world();
    assert!(!AudioView::new(&mut world).waiting_for_gesture());
    // The web's engine waits for the player's first gesture
    let mut world = locked_world();
    assert!(AudioView::new(&mut world).waiting_for_gesture());
    audio_step(&mut world, FRAME);
    assert!(AudioView::new(&mut world).waiting_for_gesture());
    unlock(&mut world);
    assert!(!AudioView::new(&mut world).waiting_for_gesture());
  }

  #[test]
  fn before_the_unlock_one_shots_drop_and_music_waits() {
    let mut world = locked_world();
    let clip = second(&mut world);
    let mut theme = Music::looping(clip.clone(), 0.0, None);
    theme.fade_in = Duration::from_millis(500);
    let theme = world.resource_mut::<Assets<Music>>().add(theme);
    let rain = hum(&mut world, &clip, "ambience", Duration::from_millis(200));
    let (shot, rain) = {
      let mut audio = AudioView::new(&mut world);
      let shot = audio.play(&clip);
      audio.play_music(&theme);
      (shot, audio.play(&rain))
    };
    for _ in 0..3 {
      audio_step(&mut world, FRAME);
    }
    // Nothing is open: the one-shot was dropped, and the loops wait
    assert_eq!(world.resource::<AudioEngine>().kira_tracks(), (0, 0));
    assert!(shot.is_stopped());
    assert_eq!(entries(&world, AudioLogKind::Refused).len(), 1);
    assert!(rain.is_playing(), "a loop waits, as playing: {rain:?}");
    assert_eq!(AudioView::new(&mut world).current_music(), Some(theme.id()));
    assert!(entries(&world, AudioLogKind::Start).is_empty());
    // The first gesture: the music and the loop start, each fading in over
    // its own fade-in
    unlock(&mut world);
    assert!(world.resource::<AudioEngine>().is_mock());
    let starts: Vec<(String, Duration)> = entries(&world, AudioLogKind::Start)
      .into_iter()
      .map(|entry| (entry.bus, entry.fade))
      .collect();
    assert_eq!(
      starts,
      [
        ("music".to_string(), Duration::from_millis(500)),
        ("ambience".to_string(), Duration::from_millis(200)),
      ]
    );
    audio_step(&mut world, FRAME);
    let music = entries(&world, AudioLogKind::Start)[0].voice;
    let engine = world.resource::<AudioEngine>();
    assert_eq!(engine.voice_playback(music), Some(PlaybackState::Playing));
    assert_eq!(
      engine.voice_playback(rain.id()),
      Some(PlaybackState::Playing)
    );
    // A one-shot after it plays
    assert!(AudioView::new(&mut world).play(&clip).is_playing());
  }

  #[test]
  fn the_unlock_happens_once() {
    let mut world = locked_world();
    let clip = second(&mut world);
    let rain = hum(&mut world, &clip, "ambience", Duration::ZERO);
    let rain = AudioView::new(&mut world).play(&rain);
    audio_step(&mut world, FRAME);
    assert_eq!(world.resource::<AudioEngine>().kira_tracks(), (0, 0));
    unlock(&mut world);
    for _ in 0..30 {
      audio_step(&mut world, FRAME);
    }
    let engine = world.resource::<AudioEngine>();
    let (tracks, logged) = (engine.kira_tracks(), engine.log().len());
    let Some(played) = engine.voice_position(rain.id()) else {
      panic!("the loop plays after the unlock: {rain:?}");
    };
    assert!(played > 0.4, "{played}");
    // A second gesture changes nothing: the loop plays on where it was
    unlock(&mut world);
    audio_step(&mut world, FRAME);
    let engine = world.resource::<AudioEngine>();
    assert_eq!(engine.kira_tracks(), tracks);
    let Some(now) = engine.voice_position(rain.id()) else {
      panic!("the loop plays on: {rain:?}");
    };
    assert!(now > played, "{now} after {played}");
    let later = &engine.log()[logged..];
    assert!(
      later.iter().all(|entry| entry.kind != AudioLogKind::Start),
      "{later:?}"
    );
  }

  /// What the game does to a loop waiting for the unlock holds at it: one
  /// stopped never starts, and one paused starts paused, fading in over its
  /// fade-in when it resumes
  #[test]
  fn a_loop_waiting_for_the_unlock_keeps_what_the_game_did_to_it() {
    let mut world = locked_world();
    let clip = second(&mut world);
    let fade = Duration::from_millis(300);
    let rain = hum(&mut world, &clip, "ambience", fade);
    let wind = hum(&mut world, &clip, "ambience", fade);
    let (stopped, paused) = {
      let mut audio = AudioView::new(&mut world);
      (audio.play(&rain), audio.play(&wind))
    };
    stopped.stop(Duration::ZERO);
    paused.pause();
    audio_step(&mut world, FRAME);
    assert!(stopped.is_stopped());
    unlock(&mut world);
    let starts: Vec<(u32, Duration)> = entries(&world, AudioLogKind::Start)
      .into_iter()
      .map(|entry| (entry.voice, entry.fade))
      .collect();
    assert_eq!(starts, [(paused.id(), Duration::ZERO)], "it starts held");
    audio_step(&mut world, FRAME);
    let engine = world.resource::<AudioEngine>();
    assert_eq!(
      engine.voice_playback(paused.id()),
      Some(PlaybackState::Paused)
    );
    paused.resume();
    audio_step(&mut world, FRAME);
    // Fading in
    let engine = world.resource::<AudioEngine>();
    assert_eq!(
      engine.voice_playback(paused.id()),
      Some(PlaybackState::Resuming)
    );
    let resumes: Vec<Duration> = entries(&world, AudioLogKind::Resume)
      .into_iter()
      .filter(|entry| entry.voice == paused.id())
      .map(|entry| entry.fade)
      .collect();
    assert_eq!(resumes, [fade]);
  }

  /// A loop played at a place before the unlock starts at it with the
  /// distance low-pass its place gives
  #[test]
  fn a_placed_loop_waiting_for_the_unlock_starts_with_its_low_pass() {
    let mut world = locked_world();
    world.spawn(AudioListener);
    let clip = second(&mut world);
    let sound = world.resource_mut::<Assets<Sound>>().add(Sound {
      clips: vec![SoundClip::new(clip)],
      looping: true,
      spatial: Some(Spatial {
        min_distance: 100.0,
        max_distance: 1000.0,
        curve: Falloff::Linear,
        pan_width: 1000.0,
        low_pass_at_max: Some(500.0),
      }),
      ..Default::default()
    });
    audio_step(&mut world, FRAME);
    let voice = AudioView::new(&mut world).play_at(&sound, Vec2::new(550.0, 0.0));
    audio_step(&mut world, FRAME);
    unlock(&mut world);
    let Some(hz) = world.resource::<AudioEngine>().voice_cutoff(voice.id()) else {
      panic!("it starts with its low-pass");
    };
    assert!(hz > 500.0 && hz < 20_000.0, "{hz}");
  }

  /// A loop played before the unlock while its bus is held (the game
  /// paused) reports what the same loop played after it would: kira's side
  /// paused, the voice playing. Let go before the unlock, it logs nothing
  /// until it starts there, fading in over its own fade-in
  #[test]
  fn a_held_loop_waiting_for_the_unlock_reports_its_hold() {
    let mut world = locked_world();
    let clip = second(&mut world);
    let fade = Duration::from_millis(250);
    let rain = hum(&mut world, &clip, "ambience", fade);
    world.resource_mut::<Time>().game.pause();
    audio_step(&mut world, FRAME);
    let voice = AudioView::new(&mut world).play(&rain);
    audio_step(&mut world, FRAME);
    let playback = world.resource::<AudioEngine>().voice_playback(voice.id());
    assert_eq!(playback, Some(PlaybackState::Paused), "held");
    assert!(voice.is_playing());
    world.resource_mut::<Time>().game.resume();
    audio_step(&mut world, FRAME);
    let playback = world.resource::<AudioEngine>().voice_playback(voice.id());
    assert_eq!(playback, Some(PlaybackState::Playing), "let go");
    unlock(&mut world);
    let about: Vec<(AudioLogKind, Duration)> = log(&world)
      .into_iter()
      .filter(|entry| entry.voice == voice.id())
      .map(|entry| (entry.kind, entry.fade))
      .collect();
    assert_eq!(about, [(AudioLogKind::Start, fade)]);
  }

  /// A loop that cannot start at the unlock (its music now names a clip
  /// still loading) ends there, refused as a play that never started is:
  /// logged with voice 0
  #[test]
  fn a_waiting_loop_that_cannot_start_at_the_unlock_is_refused() {
    let mut world = locked_world();
    let clip = second(&mut world);
    let theme = world
      .resource_mut::<Assets<Music>>()
      .add(Music::looping(clip, 0.0, None));
    AudioView::new(&mut world).play_music(&theme);
    audio_step(&mut world, FRAME);
    let loading = world
      .resource::<AssetServer>()
      .load::<AudioClip>("still-loading.wav");
    if let Some(music) = world.resource_mut::<Assets<Music>>().get_mut(&theme) {
      *music = Music::looping(loading, 0.0, None);
    }
    unlock(&mut world);
    let refused: Vec<(u32, String)> = entries(&world, AudioLogKind::Refused)
      .into_iter()
      .map(|entry| (entry.voice, entry.bus))
      .collect();
    assert_eq!(refused, [(0, "music".to_string())]);
    audio_step(&mut world, FRAME);
    assert_eq!(AudioView::new(&mut world).current_music(), None);
    assert!(entries(&world, AudioLogKind::Start).is_empty());
  }

  /// A mixer that lands before the first gesture is built on kira at it,
  /// with the player's volumes: master's on kira's main track
  #[test]
  fn a_mixer_that_lands_before_the_unlock_is_built_at_it() {
    let mut world = locked_world();
    AudioView::new(&mut world).set_volume_setting(VolumeKind::Master, 0.5);
    AudioView::new(&mut world).set_mixer(Mixer {
      buses: vec![BusDef::new("ambience/rain")],
      ..Default::default()
    });
    audio_step(&mut world, FRAME);
    let clip = second(&mut world);
    let rain = hum(&mut world, &clip, "ambience/rain", Duration::ZERO);
    let rain = AudioView::new(&mut world).play(&rain);
    audio_step(&mut world, FRAME);
    assert_eq!(world.resource::<AudioEngine>().kira_tracks(), (0, 0));
    let before = log(&world).len();
    unlock(&mut world);
    audio_step(&mut world, FRAME);
    let engine = world.resource::<AudioEngine>();
    assert_eq!(engine.sounds_on("ambience/rain"), 1);
    assert_eq!(
      engine.voice_playback(rain.id()),
      Some(PlaybackState::Playing)
    );
    let master = engine.log()[before..]
      .iter()
      .find(|entry| entry.kind == AudioLogKind::Gain && entry.voice == 0 && entry.bus == "master")
      .map(|entry| entry.gain_db);
    let Some(master) = master else {
      panic!("master's gain goes on kira's main track at the unlock");
    };
    assert!((master - volume_to_db(0.5)).abs() < 1e-3, "{master}");
  }
}
