//! [`AudioEngine`]: the world resource that owns kira's manager (on the audio
//! device, or on kira's mock backend when there is none), the bus tree and
//! the live voices

use std::collections::HashMap;
use std::fmt::Display;
#[cfg(not(target_arch = "wasm32"))]
use std::io::Cursor;
use std::ops::Range;
use std::sync::Arc;
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender};
use kira::backend::cpal::CpalBackend;
use kira::backend::mock::{MockBackend, MockBackendSettings};
use kira::sound::static_sound::{StaticSoundData, StaticSoundHandle};
#[cfg(not(target_arch = "wasm32"))]
use kira::sound::streaming::{StreamingSoundData, StreamingSoundHandle};
use kira::sound::{EndPosition, PlaybackPosition, PlaybackState, Region, SoundData};
use kira::track::{TrackBuilder, TrackHandle};
use kira::{
  AudioManager, AudioManagerSettings, Decibels, PlaySoundError, PlaybackRate, ResourceLimitReached,
  Semitones, Tween,
};
use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng};

use super::log::{AudioLog, AudioLogEntry, AudioLogKind};
use super::view::{PlayOptions, Playable};
use super::voice::{Voice, VoiceChange, VoiceCommand, VoiceState};
use super::{AudioClip, ClipData, LimitRule, Sound, SoundClip, VoiceLimit};
use crate::core::asset::{AssetId, AssetServer, Assets, Handle};
use crate::core::ecs::{Resource, Vec2, World};
use crate::core::hash::{FastMap, FastSet};

/// The shortest smoothing of any change to a voice (volume, pitch, pause,
/// stop): shorter jumps click
pub(crate) const MIN_FADE: Duration = Duration::from_millis(10);

/// The most voices playing on one bus's own track. kira holds 128 sounds a
/// track, so the voices stolen past the cap have room to fade out
pub(crate) const VOICE_CAP: usize = 96;

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
/// A voice's own gain, in dB: kira is silent at -60 and below
const GAIN_RANGE: (f32, f32) = (-60.0, 24.0);
/// A voice's own pitch, in semitones either way
const PITCH_RANGE: f32 = 48.0;
/// The standard buses under `master`, in order
pub(crate) const STANDARD_BUSES: [&str; 5] = ["music", "sfx", "ui", "voice", "ambience"];
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
      internal_buffer_size: CHUNK_FRAMES,
      ..Default::default()
    };
    AudioManager::<MockBackend>::new(settings)
      .ok()
      .map(Manager::Mock)
  }

  /// The default output device, or the mock backend with one warning
  fn device() -> Option<Manager> {
    let settings = AudioManagerSettings {
      internal_buffer_size: CHUNK_FRAMES,
      ..Default::default()
    };
    match AudioManager::<CpalBackend>::new(settings) {
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
  /// The most voices playing on it and the buses under it
  limit: Option<u16>,
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
      limit: None,
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
        limit: None,
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

  /// Whether kira has room for one more sound on `bus`'s track. Counting
  /// kira's sounds, not the voices, takes in the ones it has yet to free
  fn has_room(&self, bus: usize) -> bool {
    self
      .buses
      .get(bus)
      .and_then(|bus| bus.track.as_ref())
      .is_some_and(|track| track.num_sounds() < track.sound_capacity())
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
enum ClipSource {
  /// Decoded frames (an `Arc` inside: cheap to clone)
  Decoded(StaticSoundData),
  /// The file's bytes, to stream from
  Encoded(Arc<[u8]>),
}

impl ClipSource {
  fn of(clip: &AudioClip) -> ClipSource {
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
  /// The clip's content version when the voice started
  clip_version: u64,
  /// The definition it plays; `None` for a clip played directly
  sound: Option<AssetId<Sound>>,
  bus: usize,
  /// Its own gain in dB
  gain_db: f32,
  /// Its own pitch in semitones
  pitch: f32,
  /// How important it is (0 to 255)
  priority: u8,
  /// Where it plays in the world, if it was given a position
  position: Option<Vec2>,
  /// Its fade when the engine stops it under a limit
  fade_out: Duration,
  /// The engine frame it started in
  frame: u64,
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

/// One play, resolved from a definition (or a bare clip's empty one)
struct Request<'a> {
  sound: Option<AssetId<Sound>>,
  clip: &'a Handle<AudioClip>,
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
  fade_in: Duration,
  fade_out: Duration,
  position: Option<Vec2>,
}

impl<'a> Request<'a> {
  /// `clip` played directly: the empty definition, plus `options`
  fn clip(clip: &'a Handle<AudioClip>, options: &PlayOptions) -> Self {
    Self {
      sound: None,
      clip,
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
      fade_in: Duration::ZERO,
      fade_out: Duration::ZERO,
      position: options.position,
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
      clip: &clip.clip,
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
      fade_in: sound.fade_in,
      fade_out: sound.fade_out,
      position: options.position,
    })
  }
}

/// How a voice starts in kira
struct Start<'a> {
  gain_db: f32,
  pitch: f32,
  fade_in: Option<Tween>,
  region: Option<&'a Range<f64>>,
  loop_region: Option<&'a Range<f64>>,
  looping: bool,
}

/// Who a log entry is about
struct Subject<'a> {
  sound: Option<AssetId<Sound>>,
  clip: &'a Handle<AudioClip>,
  bus: usize,
  gain_db: f32,
  pitch: f32,
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
  /// Where the listener is: nowhere until spatial sound places it, so every
  /// voice is at distance 0
  listener: Option<Vec2>,
  /// The voices one play stops to make room, kept to reuse
  victims: Vec<Victim>,
  #[cfg(test)]
  injected_stream_errors: Vec<String>,
  /// Plays kira refused for want of room: none should ever be
  #[cfg(test)]
  kira_full: usize,
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
      undecodable: HashMap::new(),
      rng: SmallRng::from_entropy(),
      clock: Duration::ZERO,
      frame: 0,
      sounds: FastMap::default(),
      warned_sounds: FastSet::default(),
      warned_buses: FastSet::default(),
      listener: None,
      victims: Vec::new(),
      #[cfg(test)]
      injected_stream_errors: Vec::new(),
      #[cfg(test)]
      kira_full: 0,
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

  /// Seed the generator that picks clips and draws gains and pitches, so a
  /// run picks the same way each time
  #[doc(hidden)]
  pub fn seed(&mut self, seed: u64) {
    self.rng = SmallRng::seed_from_u64(seed);
  }

  /// Put the listener at `position` (tests, until spatial sound places it)
  #[cfg(test)]
  pub(crate) fn set_listener(&mut self, position: Option<Vec2>) {
    self.listener = position;
  }

  /// Limit the voices on `bus` and the buses under it (tests, until the
  /// mixer sets bus limits)
  #[cfg(test)]
  pub(crate) fn set_bus_limit(&mut self, bus: &str, max: Option<u16>) {
    if let Some(bus) = self.buses.find(bus) {
      self.buses.buses[bus].limit = max;
    }
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
  /// A definition or clip not loaded yet (or that failed: warned about once)
  /// plays nothing; a play its cooldown, a voice limit or kira's room
  /// refuses, a stream that cannot open, or a clip whose stream already
  /// failed to decode (until it reloads) is refused and logged. Either way
  /// the voice returned is already stopped
  pub(crate) fn play(
    &mut self,
    world: &World,
    playable: Playable<'_>,
    options: &PlayOptions,
  ) -> Voice {
    let server = world.get_resource::<AssetServer>();
    match playable {
      Playable::Clip(clip) => self.start(world, &Request::clip(clip, options), server),
      Playable::Sound(handle) => {
        let id = handle.id();
        let Some(sound) = world
          .get_resource::<Assets<Sound>>()
          .and_then(|sounds| sounds.get(id))
        else {
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
          .and_then(|pick| Request::sound(id, sound, pick, &mut self.rng, options));
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

  /// Start `request`'s voice, unless its clip is not loaded (a stopped
  /// voice, not logged) or it is refused (a stopped voice, logged)
  fn start(&mut self, world: &World, request: &Request<'_>, server: Option<&AssetServer>) -> Voice {
    let bus = self.resolve_bus(request.bus, request.sound, server);
    let subject = Subject {
      sound: request.sound,
      clip: request.clip,
      bus,
      gain_db: clamp_gain(request.gain_db),
      pitch: clamp_pitch(request.pitch),
    };
    let Some((clip, version)) = world
      .get_resource::<Assets<AudioClip>>()
      .and_then(|clips| Some((clips.get(request.clip)?, clips.version(request.clip)?)))
    else {
      return Voice::stopped();
    };
    // A clip whose stream failed to decode would only fail the same way on
    // another decoder thread. Kira's room is asked before any limit stops
    // a voice: a stopped voice holds its room until it has faded out
    let refused = self.undecodable.get(&request.clip.id()) == Some(&version)
      || self.cooling(request)
      || !self.buses.has_room(bus)
      || !self.make_room(bus, request);
    if refused {
      return self.refuse(&subject, server);
    }
    let start = Start {
      gain_db: subject.gain_db,
      pitch: subject.pitch,
      fade_in: (!request.fade_in.is_zero()).then(|| tween(request.fade_in)),
      region: request.region,
      loop_region: request.loop_region,
      looping: request.looping,
    };
    let started = match ClipSource::of(clip) {
      ClipSource::Decoded(data) => self.start_static(bus, data, &start),
      ClipSource::Encoded(bytes) => self.start_stream(bus, bytes, &start),
    };
    let playback = match started {
      Ok(playback) => playback,
      Err(refusal) => {
        match &refusal {
          Refusal::Undecodable(error) => {
            self.warn_undecodable(request.clip, version, error, server)
          }
          Refusal::Full => self.count_kira_full(),
        }
        self.victims.clear();
        return self.refuse(&subject, server);
      }
    };
    self.steal_victims(server);
    let voice = Voice::new(self.voices.next_id(), self.commands.clone());
    self.voices.live.push(LiveVoice {
      voice: voice.clone(),
      playback,
      clip: request.clip.clone(),
      clip_version: version,
      sound: request.sound,
      bus,
      gain_db: subject.gain_db,
      pitch: subject.pitch,
      priority: request.priority,
      position: request.position,
      fade_out: request.fade_out,
      frame: self.frame,
    });
    self.record(
      self.voices.live.len() - 1,
      AudioLogKind::Start,
      request.fade_in,
      server,
    );
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
      listener: self.listener,
      priority: request.priority,
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
        live.playback.stop(tween(fade));
        live.voice.set_state(VoiceState::Stopping);
      }
      self.record(index, AudioLogKind::Stolen, fade, server);
    }
    self.victims.clear();
  }

  /// A refused play: logged, and a voice that never started
  fn refuse(&mut self, subject: &Subject<'_>, server: Option<&AssetServer>) -> Voice {
    let entry = self.entry(AudioLogKind::Refused, 0, subject, server);
    self.log.push(entry);
    Voice::stopped()
  }

  #[cfg(test)]
  fn count_kira_full(&mut self) {
    self.kira_full += 1;
  }

  #[cfg(not(test))]
  fn count_kira_full(&mut self) {}

  /// The bus named `name`, or the nearest bus above it (`sfx` for
  /// `sfx/steps`), or `sfx`, warning once per name
  fn resolve_bus(
    &mut self,
    name: &str,
    sound: Option<AssetId<Sound>>,
    server: Option<&AssetServer>,
  ) -> usize {
    if let Some(bus) = self.buses.find(name) {
      return bus;
    }
    let mut at = name;
    let fallback = loop {
      match at.rsplit_once('/') {
        Some((parent, _)) => match self.buses.find(parent) {
          Some(bus) => break bus,
          None => at = parent,
        },
        None => break self.buses.find(DEFAULT_BUS).unwrap_or(0),
      }
    };
    if !self.warned_buses.contains(name) {
      self.warned_buses.insert(name.to_string());
      let who = sound.map_or_else(String::new, |id| sound_name(id, server));
      crate::engine_warn!(
        module: "Audio",
        "{who}: there is no bus {name:?}, so it plays on {:?} (a game's own buses are added in the mixer)",
        self.buses.name(fallback)
      );
    }
    fallback
  }

  /// Warn about definition `id` once, with `why`
  fn warn_sound(&mut self, id: AssetId<Sound>, why: &str, server: Option<&AssetServer>) {
    if self.warned_sounds.insert(id) {
      let name = sound_name(id, server);
      crate::engine_warn!(module: "Audio", "{name}: {why}");
    }
  }

  /// Start decoded frames on `bus`. A static sound cannot fail to start but
  /// for room
  fn start_static(
    &mut self,
    bus: usize,
    mut data: StaticSoundData,
    start: &Start<'_>,
  ) -> Result<Playback, Refusal> {
    let span = Span::of(start, data.sample_rate, data.frames.len());
    data.slice = span.slice;
    data.settings.loop_region = span.loop_region.map(region);
    data.settings.volume = Decibels(start.gain_db).into();
    data.settings.playback_rate = PlaybackRate(rate_of(start.pitch)).into();
    data.settings.fade_in_tween = start.fade_in;
    self
      .buses
      .play(bus, data)
      .map(Playback::Static)
      .map_err(|_| Refusal::Full)
  }

  /// Open a stream over a clip's bytes and start it. The web has no
  /// streaming, and decodes every clip, so never gets here
  #[cfg(not(target_arch = "wasm32"))]
  fn start_stream(
    &mut self,
    bus: usize,
    bytes: Arc<[u8]>,
    start: &Start<'_>,
  ) -> Result<Playback, Refusal> {
    let mut data = StreamingSoundData::from_cursor(Cursor::new(bytes))
      .map_err(|error| Refusal::Undecodable(error.to_string()))?;
    let frames = data.num_frames();
    let seconds = data.duration().as_secs_f64();
    let rate = if seconds > 0.0 {
      (frames as f64 / seconds).round() as u32
    } else {
      0
    };
    let span = Span::of(start, rate, frames);
    data.slice = span.slice;
    let data = data
      .volume(Decibels(start.gain_db))
      .playback_rate(PlaybackRate(rate_of(start.pitch)))
      .fade_in_tween(start.fade_in)
      .loop_region(span.loop_region.map(region));
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
    _start: &Start<'_>,
  ) -> Result<Playback, Refusal> {
    Err(Refusal::Undecodable(
      "the web cannot stream audio".to_string(),
    ))
  }

  /// The frame's audio work: count the frame, apply the voices' queued
  /// commands, report the device's stream errors, advance the mock backend
  /// by `real_delta`, and retire the voices that ended
  pub(crate) fn update(&mut self, real_delta: Duration, server: Option<&AssetServer>) {
    self.frame = self.frame.wrapping_add(1);
    self.clock = self.clock.saturating_add(real_delta);
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
  /// stop), marking their handles stopped, and keep the rest in start
  /// order. A stream that failed to decode as it played is logged stopped
  /// and its clip warned about once
  fn retire_ended(&mut self, server: Option<&AssetServer>) {
    for index in 0..self.voices.live.len() {
      let live = &mut self.voices.live[index];
      // The state first: a stream's error is queued before kira stops it
      let ended = live.playback.state() == PlaybackState::Stopped;
      if let Some(error) = live.playback.take_error() {
        let (clip, version) = (live.clip.clone(), live.clip_version);
        // kira cuts a stream that fails to decode at its next chunk, with
        // no fade, and ignores a stop after that: log what it did
        if live.voice.state() != VoiceState::Stopping {
          live.voice.set_state(VoiceState::Stopping);
          self.record(index, AudioLogKind::Stop, Duration::ZERO, server);
        }
        self.warn_undecodable(&clip, version, &error, server);
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
    let subject = Subject {
      sound: live.sound,
      clip: &live.clip,
      bus: live.bus,
      gain_db: live.gain_db,
      pitch: live.pitch,
    };
    let mut entry = self.entry(kind, live.voice.id(), &subject, server);
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
      sound: match subject.sound {
        Some(id) if on => sound_name(id, server),
        _ => String::new(),
      },
      source: if on {
        source_name(subject.clip, server)
      } else {
        String::new()
      },
      bus: if on {
        self.buses.name(subject.bus).to_string()
      } else {
        String::new()
      },
      gain_db: subject.gain_db,
      pan: 0.0,
      rate: rate_of(subject.pitch),
      fade: Duration::ZERO,
    }
  }
}

/// The engine's voices outlive it as handles: with kira's manager gone,
/// nothing plays, so they report stopped
impl Drop for AudioEngine {
  fn drop(&mut self) {
    for live in &self.voices.live {
      live.voice.set_state(VoiceState::Stopped);
    }
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
  listener: Option<Vec2>,
  /// The new play's priority
  priority: u8,
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
  /// frame (stopping it would only stack two starts of a burst). Voices are
  /// in start order, so the oldest of equals wins
  fn victim(&self, scope: Scope, rule: Option<LimitRule>) -> Option<usize> {
    let mut chosen: Option<usize> = None;
    for index in 0..self.live.len() {
      let live = &self.live[index];
      let may_stop = live.priority < self.priority
        || (live.priority == self.priority && live.frame != self.frame);
      if !may_stop || !self.counts(index, scope) {
        continue;
      }
      let better = match chosen.map(|at| &self.live[at]) {
        None => true,
        Some(best) => match rule {
          Some(LimitRule::StopOldest | LimitRule::PreventNew) => false,
          Some(LimitRule::StopQuietest) => live.gain_db < best.gain_db,
          Some(LimitRule::StopFarthest) => self.distance(live) > self.distance(best),
          Some(LimitRule::StopLowestPriority) => live.priority < best.priority,
          None => {
            live.priority < best.priority
              || (live.priority == best.priority && live.gain_db < best.gain_db)
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
      (Some(position), Some(listener)) => position.distance(listener),
      _ => 0.0,
    }
  }
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
  use crate::core::audio::AudioView;
  use crate::core::dev_log::{self, LogLevel};
  use crate::core::ecs::World;
  use crate::core::testing::{audio_step, audio_world, audio_world_over};

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
  /// fails as it plays. kira cuts the stream at its next chunk, with no fade
  /// (it cannot be smoothed), and the log says so; the clip is warned about
  /// once, by its path
  #[test]
  fn a_streamed_clip_that_cannot_decode_stops_and_warns_once() {
    let mut world = audio_world_over(files(&["surround.wav"]));
    let clip = streamed(&mut world, "surround.wav");
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
    assert_eq!(undecodable_warnings("surround.wav"), 1);
  }

  /// Once a clip's stream has failed to decode, it is not played again (no
  /// decoder thread is started for it) until it reloads
  #[test]
  fn a_clip_known_undecodable_is_refused_until_it_reloads() {
    let files = MemorySource::new();
    files.insert("surround-reloaded.wav", fixture("surround.wav"));
    let mut world = audio_world_over(files.clone());
    let clip = streamed(&mut world, "surround-reloaded.wav");
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
      Some("surround-reloaded.wav")
    );
    // A reload (here, to a stereo file) clears the mark
    let version = |world: &World| world.resource::<Assets<AudioClip>>().version(&clip);
    let before = version(&world);
    files.insert("surround-reloaded.wav", fixture("tone.wav"));
    world
      .resource::<AssetServer>()
      .reload("surround-reloaded.wav");
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
    assert_eq!(undecodable_warnings("surround-reloaded.wav"), 1);
  }

  /// A stream that cannot even open when played is refused, and warned
  /// about by its path, whether or not the log is on
  #[test]
  fn a_clip_that_cannot_open_is_named_by_its_path_with_the_log_off() {
    let files = MemorySource::new();
    files.insert("garbled-at-play.wav", fixture("tone.wav"));
    let mut world = audio_world_over(files);
    world.resource_mut::<AudioEngine>().set_logging(false);
    let clip = streamed(&mut world, "garbled-at-play.wav");
    // Its bytes go bad after loading
    if let Some(clip) = world.resource_mut::<Assets<AudioClip>>().get_mut(&clip) {
      let garbage: Arc<[u8]> = Arc::from(&b"not audio at all"[..]);
      *clip = AudioClip::new(ClipData::Encoded(garbage), Duration::from_secs(1), 22_050);
    }
    let voice = AudioView::new(&mut world).play(&clip);
    assert!(voice.is_stopped(), "{voice:?}");
    assert_eq!(undecodable_warnings("garbled-at-play.wav"), 1);
  }

  #[test]
  fn the_voice_table_keeps_the_order_voices_started_in() {
    let mut world = audio_world();
    let clip = second(&mut world);
    let voices: Vec<Voice> = (0..4)
      .map(|_| AudioView::new(&mut world).play(&clip))
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
    let clip = second(&mut world);
    let (playing, paused) = {
      let mut audio = AudioView::new(&mut world);
      (audio.play(&clip), audio.play(&clip))
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
  fn gain_and_pitch_are_clamped_and_nan_is_neutral() {
    assert_eq!(clamp_gain(f32::NAN), 0.0);
    assert_eq!(clamp_gain(f32::NEG_INFINITY), -60.0);
    assert_eq!(clamp_gain(100.0), 24.0);
    assert_eq!(clamp_pitch(f32::NAN), 0.0);
    assert_eq!(clamp_pitch(-100.0), -48.0);
    assert!((rate_of(12.0) - 2.0).abs() < 1e-12);
  }
}
