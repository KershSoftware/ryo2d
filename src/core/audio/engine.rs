//! [`AudioEngine`]: the world resource that owns kira's manager (on the audio
//! device, or on kira's mock backend when there is none), the bus tree and
//! the live voices

use std::collections::HashSet;
use std::fmt::Display;
#[cfg(not(target_arch = "wasm32"))]
use std::io::Cursor;
use std::sync::Arc;
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender};
use kira::backend::cpal::CpalBackend;
use kira::backend::mock::{MockBackend, MockBackendSettings};
use kira::sound::static_sound::{StaticSoundData, StaticSoundHandle};
#[cfg(not(target_arch = "wasm32"))]
use kira::sound::streaming::{StreamingSoundData, StreamingSoundHandle};
use kira::sound::{PlaybackState, SoundData};
use kira::track::{TrackBuilder, TrackHandle};
use kira::{
  AudioManager, AudioManagerSettings, Decibels, PlaySoundError, PlaybackRate, ResourceLimitReached,
  Semitones, Tween,
};

use super::log::{AudioLog, AudioLogEntry, AudioLogKind};
use super::view::PlayOptions;
use super::voice::{Voice, VoiceChange, VoiceCommand, VoiceState};
use super::{AudioClip, ClipData};
use crate::core::asset::{AssetId, AssetServer, Handle};
use crate::core::ecs::Resource;

/// The shortest smoothing of any change to a voice (volume, pitch, pause,
/// stop): shorter jumps click
pub(crate) const MIN_FADE: Duration = Duration::from_millis(10);

/// The mock backend's sample rate
const MOCK_SAMPLE_RATE: u32 = 48_000;
/// kira renders in chunks of this many frames (its default internal buffer)
const CHUNK_FRAMES: u128 = 128;
/// The most real time the mock backend renders in one frame: a frame after
/// the machine slept is not hours of audio
const MAX_MOCK_STEP: Duration = Duration::from_secs(60);
/// Voices the table holds before it grows
const VOICE_CAPACITY: usize = 128;
/// A voice's own gain, in dB: kira is silent at -60 and below
const GAIN_RANGE: (f32, f32) = (-60.0, 24.0);
/// A voice's own pitch, in semitones either way
const PITCH_RANGE: f32 = 48.0;
/// The standard buses under `master`, in order
const STANDARD_BUSES: [&str; 5] = ["music", "sfx", "ui", "voice", "ambience"];
/// Where a clip played directly goes
const DEFAULT_BUS: &str = "sfx";

/// The error the failing-device seam reports
#[cfg(test)]
pub(crate) const FAILING_DEVICE: &str = "no output device (the failing-device seam)";

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
    let settings = AudioManagerSettings {
      backend_settings: MockBackendSettings {
        sample_rate: MOCK_SAMPLE_RATE,
      },
      ..Default::default()
    };
    AudioManager::<MockBackend>::new(settings)
      .ok()
      .map(Manager::Mock)
  }

  /// The default output device, or the mock backend with one warning
  fn device() -> Option<Manager> {
    match AudioManager::<CpalBackend>::new(AudioManagerSettings::default()) {
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

  fn add_sub_track(&mut self, builder: TrackBuilder) -> Result<TrackHandle, ResourceLimitReached> {
    match self {
      Manager::Device(manager) => manager.add_sub_track(builder),
      Manager::Mock(manager) => manager.add_sub_track(builder),
    }
  }
}

/// One bus: a kira track under its parent's
struct Bus {
  /// Its full name, such as `sfx` or `sfx/footsteps`
  name: String,
  parent: Option<usize>,
  /// `None` if kira had no room for it (never with the standard layout):
  /// plays on it are refused
  track: Option<TrackHandle>,
}

/// The buses, `master` first, in the standard layout (`master`, and under
/// it `music`, `sfx`, `ui`, `voice` and `ambience`)
struct BusTree {
  buses: Vec<Bus>,
}

impl BusTree {
  /// `master` under kira's main track, and the standard five under it
  fn standard(manager: Option<&mut Manager>) -> BusTree {
    let master = manager.and_then(|manager| manager.add_sub_track(TrackBuilder::new()).ok());
    let mut buses = Vec::with_capacity(1 + STANDARD_BUSES.len());
    buses.push(Bus {
      name: "master".to_string(),
      parent: None,
      track: master,
    });
    for name in STANDARD_BUSES {
      let track = buses[0]
        .track
        .as_mut()
        .and_then(|master| master.add_sub_track(TrackBuilder::new()).ok());
      buses.push(Bus {
        name: name.to_string(),
        parent: Some(0),
        track,
      });
    }
    BusTree { buses }
  }

  fn find(&self, name: &str) -> Option<usize> {
    self.buses.iter().position(|bus| bus.name == name)
  }

  fn name(&self, bus: usize) -> &str {
    self.buses.get(bus).map_or("", |bus| bus.name.as_str())
  }

  /// Whether `bus` is `ancestor` or under it
  fn within(&self, bus: usize, ancestor: usize) -> bool {
    let mut at = Some(bus);
    while let Some(index) = at {
      if index == ancestor {
        return true;
      }
      at = self.buses.get(index).and_then(|bus| bus.parent);
    }
    false
  }

  /// Start `data` on `bus`. A bus without a track has no room
  fn play<D: SoundData>(
    &mut self,
    bus: usize,
    data: D,
  ) -> Result<D::Handle, PlaySoundError<D::Error>> {
    match self.buses.get_mut(bus).and_then(|bus| bus.track.as_mut()) {
      Some(track) => track.play(data),
      None => Err(PlaySoundError::SoundLimitReached),
    }
  }
}

/// A clip's samples, taken from its store to play. It lives on the stack for
/// one play, and boxing the decoded variant would allocate on every play
#[allow(clippy::large_enum_variant)]
pub(crate) enum ClipSource {
  /// Decoded frames (an `Arc` inside: cheap to clone)
  Decoded(StaticSoundData),
  /// The file's bytes, to stream from
  Encoded(Arc<[u8]>),
}

impl ClipSource {
  pub(crate) fn of(clip: &AudioClip) -> ClipSource {
    match clip.data() {
      ClipData::Decoded(data) => ClipSource::Decoded(StaticSoundData::clone(data)),
      ClipData::Encoded(bytes) => ClipSource::Encoded(Arc::clone(bytes)),
    }
  }
}

/// A live voice's kira handle
enum Playback {
  Static(StaticSoundHandle),
  #[cfg(not(target_arch = "wasm32"))]
  Stream(StreamingSoundHandle<kira::sound::FromFileError>),
}

impl Playback {
  fn state(&self) -> PlaybackState {
    match self {
      Playback::Static(sound) => sound.state(),
      #[cfg(not(target_arch = "wasm32"))]
      Playback::Stream(sound) => sound.state(),
    }
  }

  fn stop(&mut self, tween: Tween) {
    match self {
      Playback::Static(sound) => sound.stop(tween),
      #[cfg(not(target_arch = "wasm32"))]
      Playback::Stream(sound) => sound.stop(tween),
    }
  }

  fn pause(&mut self, tween: Tween) {
    match self {
      Playback::Static(sound) => sound.pause(tween),
      #[cfg(not(target_arch = "wasm32"))]
      Playback::Stream(sound) => sound.pause(tween),
    }
  }

  fn resume(&mut self, tween: Tween) {
    match self {
      Playback::Static(sound) => sound.resume(tween),
      #[cfg(not(target_arch = "wasm32"))]
      Playback::Stream(sound) => sound.resume(tween),
    }
  }

  fn set_volume(&mut self, db: f32, tween: Tween) {
    match self {
      Playback::Static(sound) => sound.set_volume(Decibels(db), tween),
      #[cfg(not(target_arch = "wasm32"))]
      Playback::Stream(sound) => sound.set_volume(Decibels(db), tween),
    }
  }

  fn set_rate(&mut self, rate: f64, tween: Tween) {
    match self {
      Playback::Static(sound) => sound.set_playback_rate(PlaybackRate(rate), tween),
      #[cfg(not(target_arch = "wasm32"))]
      Playback::Stream(sound) => sound.set_playback_rate(PlaybackRate(rate), tween),
    }
  }

  /// The first decode error a stream reported since the last call, the rest
  /// dropped (a stream that fails keeps failing until it stops)
  fn take_error(&mut self) -> Option<String> {
    match self {
      Playback::Static(_) => None,
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
  /// Held, so the registry never frees a clip under playback
  clip: Handle<AudioClip>,
  bus: usize,
  /// Its own gain in dB
  gain_db: f32,
  /// Its own pitch in semitones
  pitch: f32,
}

/// The live voices, in the order they started
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

/// Why a play did not start
enum Refusal {
  /// The bus's track has no room
  Full,
  /// A stream could not be opened
  Undecodable(String),
}

/// The engine's audio: kira's manager, the bus tree and the live voices, a
/// world resource. Scenes reach it through `ctx.audio()`; the frame's
/// `audio_system` applies the voices' commands. With no audio device (a
/// headless run, or a device that failed to open) it runs on kira's mock
/// backend: everything works, and nothing plays aloud
#[derive(Resource)]
pub struct AudioEngine {
  /// `None` only if even the mock backend failed to start (it never does):
  /// every play is then refused
  manager: Option<Manager>,
  buses: BusTree,
  voices: VoiceTable,
  log: AudioLog,
  /// Cloned into each voice
  commands: Sender<VoiceCommand>,
  inbox: Receiver<VoiceCommand>,
  /// Mock frames owed, in frame-nanoseconds (frames times 10^9), so no
  /// fraction of a frame is lost between frames
  mock_owed: u128,
  /// The device's stream reported an error: later ones are not warned about
  /// (the web's stream reports none)
  #[cfg(any(test, not(target_arch = "wasm32")))]
  stream_error_warned: bool,
  /// Clips whose stream failed to decode, warned about once each
  clips_warned: HashSet<AssetId<AudioClip>>,
  #[cfg(test)]
  injected_stream_errors: Vec<String>,
}

impl AudioEngine {
  /// The engine on the default audio device (`device`), falling back to the
  /// mock backend with one warning if it fails to open; on the mock
  /// backend at 48 kHz otherwise
  pub(crate) fn new(device: bool) -> AudioEngine {
    let manager = if device {
      Manager::device()
    } else {
      Manager::mock()
    };
    Self::with_manager(manager)
  }

  /// The engine as it is when the device fails to open (tests)
  #[cfg(test)]
  pub(crate) fn new_failing_device() -> AudioEngine {
    Self::with_manager(Manager::fallback(&FAILING_DEVICE))
  }

  fn with_manager(mut manager: Option<Manager>) -> AudioEngine {
    let buses = BusTree::standard(manager.as_mut());
    let (commands, inbox) = crossbeam_channel::unbounded();
    AudioEngine {
      manager,
      buses,
      voices: VoiceTable::new(),
      log: AudioLog::default(),
      commands,
      inbox,
      mock_owed: 0,
      #[cfg(any(test, not(target_arch = "wasm32")))]
      stream_error_warned: false,
      clips_warned: HashSet::new(),
      #[cfg(test)]
      injected_stream_errors: Vec::new(),
    }
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

  pub(crate) fn is_logging(&self) -> bool {
    self.log.is_on()
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

  /// Start `clip` (whose samples are `source`) on the default bus. A play
  /// kira has no room for, or a stream that cannot open, is refused: the
  /// voice returned is already stopped. `server` names clips in the log
  pub(crate) fn play_clip(
    &mut self,
    clip: Handle<AudioClip>,
    source: ClipSource,
    options: &PlayOptions,
    server: Option<&AssetServer>,
  ) -> Voice {
    let bus = self.buses.find(DEFAULT_BUS).unwrap_or(0);
    let gain_db = clamp_gain(options.volume_db);
    let pitch = clamp_pitch(options.pitch);
    let started = match source {
      ClipSource::Decoded(mut data) => {
        data.settings.volume = Decibels(gain_db).into();
        data.settings.playback_rate = PlaybackRate(rate_of(pitch)).into();
        // A static sound cannot fail to start but for room
        self
          .buses
          .play(bus, data)
          .map(Playback::Static)
          .map_err(|_| Refusal::Full)
      }
      ClipSource::Encoded(bytes) => self.start_stream(bus, bytes, gain_db, pitch),
    };
    let playback = match started {
      Ok(playback) => playback,
      Err(refusal) => {
        if let Refusal::Undecodable(error) = &refusal {
          self.warn_undecodable(&clip, error, server);
        }
        let entry = self.entry(AudioLogKind::Refused, 0, &clip, bus, gain_db, pitch, server);
        self.log.push(entry);
        return Voice::stopped();
      }
    };
    let voice = Voice::new(self.voices.next_id(), self.commands.clone());
    self.voices.live.push(LiveVoice {
      voice: voice.clone(),
      playback,
      clip,
      bus,
      gain_db,
      pitch,
    });
    self.record(
      self.voices.live.len() - 1,
      AudioLogKind::Start,
      Duration::ZERO,
      server,
    );
    voice
  }

  /// Open a stream over a clip's bytes and start it. The web has no
  /// streaming, and decodes every clip, so never gets here
  #[cfg(not(target_arch = "wasm32"))]
  fn start_stream(
    &mut self,
    bus: usize,
    bytes: Arc<[u8]>,
    gain_db: f32,
    pitch: f32,
  ) -> Result<Playback, Refusal> {
    let data = StreamingSoundData::from_cursor(Cursor::new(bytes))
      .map_err(|error| Refusal::Undecodable(error.to_string()))?
      .volume(Decibels(gain_db))
      .playback_rate(PlaybackRate(rate_of(pitch)));
    match self.buses.play(bus, data) {
      Ok(sound) => Ok(Playback::Stream(sound)),
      Err(PlaySoundError::SoundLimitReached) => Err(Refusal::Full),
      Err(PlaySoundError::IntoSoundError(error)) => Err(Refusal::Undecodable(error.to_string())),
    }
  }

  #[cfg(target_arch = "wasm32")]
  fn start_stream(
    &mut self,
    _bus: usize,
    _bytes: Arc<[u8]>,
    _gain_db: f32,
    _pitch: f32,
  ) -> Result<Playback, Refusal> {
    Err(Refusal::Undecodable(
      "the web cannot stream audio".to_string(),
    ))
  }

  /// The frame's audio work: apply the voices' queued commands, report the
  /// device's stream errors, advance the mock backend by `real_delta`, and
  /// retire the voices that ended
  pub(crate) fn update(&mut self, real_delta: Duration, server: Option<&AssetServer>) {
    while let Ok(command) = self.inbox.try_recv() {
      self.apply(command, server);
    }
    self.report_stream_errors();
    if let Some(Manager::Mock(manager)) = &mut self.manager {
      let backend = manager.backend_mut();
      for _ in 0..chunks_due(&mut self.mock_owed, real_delta) {
        backend.on_start_processing();
        backend.process();
      }
    }
    self.retire_ended(server);
  }

  /// Apply one queued change, smoothed over at least [`MIN_FADE`]. A voice
  /// already stopping or gone takes no more changes
  fn apply(&mut self, command: VoiceCommand, server: Option<&AssetServer>) {
    let Some(index) = self.voices.index_of(command.voice) else {
      return;
    };
    let live = &mut self.voices.live[index];
    let state = live.voice.state();
    if matches!(state, VoiceState::Stopping | VoiceState::Stopped) {
      return;
    }
    let smooth = tween(MIN_FADE);
    let (kind, fade) = match command.change {
      VoiceChange::Stop(fade) => {
        let fade = fade.max(MIN_FADE);
        live.playback.stop(tween(fade));
        live.voice.set_state(VoiceState::Stopping);
        (AudioLogKind::Stop, fade)
      }
      VoiceChange::Pause => {
        if state == VoiceState::Paused {
          return;
        }
        live.playback.pause(smooth);
        live.voice.set_state(VoiceState::Paused);
        (AudioLogKind::Pause, MIN_FADE)
      }
      VoiceChange::Resume => {
        if state != VoiceState::Paused {
          return;
        }
        live.playback.resume(smooth);
        live.voice.set_state(VoiceState::Playing);
        (AudioLogKind::Resume, MIN_FADE)
      }
      VoiceChange::Volume(db) => {
        live.gain_db = clamp_gain(db);
        live.playback.set_volume(live.gain_db, smooth);
        (AudioLogKind::Gain, MIN_FADE)
      }
      VoiceChange::Pitch(semitones) => {
        live.pitch = clamp_pitch(semitones);
        live.playback.set_rate(rate_of(live.pitch), smooth);
        (AudioLogKind::Rate, MIN_FADE)
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
  /// stop), marking their handles stopped. A stream that failed to decode
  /// as it played is stopped, and its clip warned about once
  fn retire_ended(&mut self, server: Option<&AssetServer>) {
    let mut index = 0;
    while index < self.voices.live.len() {
      let live = &mut self.voices.live[index];
      // The state first: a stream's error is queued before kira stops it
      let ended = live.playback.state() == PlaybackState::Stopped;
      if let Some(error) = live.playback.take_error() {
        let clip = live.clip.clone();
        if live.voice.state() != VoiceState::Stopping {
          live.playback.stop(tween(MIN_FADE));
          live.voice.set_state(VoiceState::Stopping);
          self.record(index, AudioLogKind::Stop, MIN_FADE, server);
        }
        self.warn_undecodable(&clip, &error, server);
      }
      if ended {
        let live = self.voices.live.swap_remove(index);
        live.voice.set_state(VoiceState::Stopped);
      } else {
        index += 1;
      }
    }
  }

  /// Warn once per clip that its stream could not be decoded
  fn warn_undecodable(
    &mut self,
    clip: &Handle<AudioClip>,
    error: &str,
    server: Option<&AssetServer>,
  ) {
    if self.clips_warned.insert(clip.id()) {
      let name = source_name(clip, server);
      crate::engine_warn!(
        module: "Audio",
        "{name}: the clip's stream could not be decoded as it played ({error}); its voices stop"
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
    let mut entry = self.entry(
      kind,
      live.voice.id(),
      &live.clip,
      live.bus,
      live.gain_db,
      live.pitch,
      server,
    );
    entry.fade = fade;
    self.log.push(entry);
  }

  /// A log entry for a voice of `clip` on `bus` (an empty one when the log
  /// is off: nothing is pushed then)
  #[allow(clippy::too_many_arguments)]
  fn entry(
    &self,
    kind: AudioLogKind,
    voice: u32,
    clip: &Handle<AudioClip>,
    bus: usize,
    gain_db: f32,
    pitch: f32,
    server: Option<&AssetServer>,
  ) -> AudioLogEntry {
    let on = self.log.is_on();
    AudioLogEntry {
      kind,
      voice,
      source: if on {
        source_name(clip, server)
      } else {
        String::new()
      },
      bus: if on {
        self.buses.name(bus).to_string()
      } else {
        String::new()
      },
      gain_db,
      pan: 0.0,
      rate: rate_of(pitch),
      fade: Duration::ZERO,
    }
  }
}

/// A clip's asset path, or its id for a clip made in code
fn source_name(clip: &Handle<AudioClip>, server: Option<&AssetServer>) -> String {
  match server.and_then(|server| server.path(clip)) {
    Some(path) => path.to_string(),
    None => format!("{:?}", clip.id()),
  }
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

/// A linear tween over `duration`, starting now
fn tween(duration: Duration) -> Tween {
  Tween {
    duration,
    ..Default::default()
  }
}

/// How many whole chunks of the mock backend `real_delta` adds to what is
/// `owed` (frame-nanoseconds), keeping the remainder owed
fn chunks_due(owed: &mut u128, real_delta: Duration) -> u128 {
  let chunk = CHUNK_FRAMES * 1_000_000_000;
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
  use crate::core::audio::AudioView;
  use crate::core::dev_log::{self, LogLevel};
  use crate::core::ecs::World;
  use crate::core::testing::{audio_step, audio_world, audio_world_over};

  const FRAME: Duration = Duration::from_nanos(16_666_667);
  const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/audio");

  fn files(names: &[&str]) -> MemorySource {
    let files = MemorySource::new();
    for name in names {
      match std::fs::read(format!("{FIXTURES}/{name}")) {
        Ok(bytes) => files.insert(name, bytes),
        Err(error) => panic!("{name}: {error}"),
      }
    }
    files
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
      .add(AudioClip::from_frames(48_000, &vec![[0.1, 0.1]; 48_000]))
  }

  fn warnings(matching: impl Fn(&str) -> bool) -> usize {
    dev_log::get_entries()
      .iter()
      .filter(|entry| entry.level == LogLevel::Warn && matching(&entry.message))
      .count()
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
    let clip = second(&mut world);
    let voices: Vec<Voice> = (0..3)
      .map(|_| AudioView::new(&mut world).play(&clip))
      .collect();
    let mut audio = AudioView::new(&mut world);
    assert_eq!(audio.voices_on("sfx"), 3);
    assert_eq!(audio.voices_on("master"), 3, "master counts its subtree");
    assert_eq!(audio.voices_on("music"), 0);
    assert_eq!(audio.voices_on("no such bus"), 0);
    audio.play(&clip);
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

  /// A surround file forced to stream loads (only its header is read) and
  /// fails as it plays: the voice stops, and the clip is warned about once
  #[test]
  fn a_streamed_clip_that_cannot_decode_stops_and_warns_once() {
    let mut world = audio_world_over(files(&["surround.wav"]));
    let settings = AudioClipSettings {
      mode: ClipMode::Stream,
    };
    let clip =
      match AssetsView::new(&mut world).load_now_with::<AudioClip, _>("surround.wav", settings) {
        Ok(clip) => clip,
        Err(error) => panic!("a streamed surround file loads: {}", error.report()),
      };
    for _ in 0..2 {
      let voice = AudioView::new(&mut world).play(&clip);
      // It starts: the stream decodes on its own thread, and fails there
      assert!(voice.is_playing(), "{voice:?}");
      for _ in 0..500 {
        if voice.is_stopped() {
          break;
        }
        audio_step(&mut world, Duration::from_millis(10));
        std::thread::sleep(Duration::from_millis(2));
      }
      assert!(voice.is_stopped(), "{voice:?}");
      let log = world.resource::<AudioEngine>().log().to_vec();
      let stop = log
        .iter()
        .find(|entry| entry.kind == AudioLogKind::Stop && entry.voice == voice.id());
      assert_eq!(stop.map(|stop| stop.fade), Some(MIN_FADE), "{log:#?}");
    }
    assert_eq!(AudioView::new(&mut world).voices_on("sfx"), 0);
    let about_the_clip =
      warnings(|message| message.contains("surround.wav") && message.contains("as it played"));
    assert_eq!(about_the_clip, 1);
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
}
