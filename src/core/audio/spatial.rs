//! 2D spatial sound: the [`AudioListener`] that hears, the [`AudioEmitter`]s
//! that sound from entities, and the math that turns a voice's place into
//! its gain, pan and low-pass cutoff. kira's 3D spatial tracks are not used:
//! 2D needs only gain, pan and a filter, each a tweened change on the voice
//! (the filter on a kira track of the voice's own)

use kira::Tween;
use kira::effect::filter::{FilterBuilder, FilterHandle, FilterMode};
use kira::track::{TrackBuilder, TrackHandle};

use super::AudioEngine;
use super::mix::OPEN_HZ;
use super::settings::SILENCE_DB;
use super::{Falloff, Sound, Spatial};
use crate::core::asset::Handle;
use crate::core::ecs::kernel::{Add, DeferredWorld, HookContext, QueryState, Remove, WorldId};
use crate::core::ecs::{
  Allow, Component, Disabled, Entity, GlobalTransform, On, Query, ResMut, Transform, Vec2, With,
  World,
};
use crate::core::graphics::MainCamera;

/// The most voices with a distance low-pass at once. Each has a kira track
/// of its own (kira's effects are per track); a voice past them plays
/// unfiltered, its gain and pan still following its place
pub(crate) const MAX_FILTERED: usize = 32;

/// How sharply [`Falloff::Inverse`] falls near its source: its amplitude is
/// `(1 - t) / (1 + 15t)` a fraction `t` of the way from the minimum distance
/// to the maximum. That is the inverse-distance law of a source in the open
/// (-6 dB each time the distance doubles) heard from a fifteenth of the span
/// (from the minimum distance itself when the maximum is 16 times it),
/// tapered to silence at the maximum
const INVERSE_SHARPNESS: f32 = 15.0;

/// The least change of a voice's falloff gain sent to kira, in dB: smaller
/// ones are not heard
const GAIN_STEP: f32 = 0.1;
/// The least change of a voice's pan sent to kira
const PAN_STEP: f32 = 0.01;
/// The least change of a voice's low-pass cutoff sent to kira, as a ratio
const CUTOFF_STEP: f64 = 0.01;

/// The entity that hears spatial sound, as a microphone would: its
/// `GlobalTransform` places it, and turns which way its right is. With
/// none in the world, the main camera hears; with several, one of them
/// does. Spawn one on the player for a game whose camera leads or lags it
///
/// ```
/// use ryo2d::prelude::*;
///
/// fn spawn_player(ctx: &mut SceneContext<'_>) -> Entity {
///   ctx.world.spawn((AudioListener, Transform::from_xy(0.0, 0.0))).id()
/// }
/// ```
#[derive(Component, Debug, Clone, Copy, Default)]
#[require(Transform)]
pub struct AudioListener;

/// A sound that comes from an entity, such as a waterfall's roar or an
/// engine's hum: its voices are heard from the entity's `GlobalTransform`,
/// following it as it moves, by its sound's spatial settings
/// ([`Sound::spatial`]).
///
/// It plays as it is added to an entity when `play_on_spawn` is set (a sound
/// still loading plays once it has; emitters of one sound spawned together
/// start as many a frame as its [`Sound::per_frame`] allows, the rest in the
/// frames after), again each time a disabled entity is
/// enabled (a [`Pool`](crate::core::ecs::Pool) handing it out, recycled
/// included), or when the game asks (`ctx.audio().play_emitter(entity)`).
/// Its voices belong to the entity, not to the scene: despawning it,
/// disabling it (a pool putting it back), taking its `AudioEmitter` away or
/// putting another over it stops its loops with their `fade_out` (and any
/// one-shot paused or held by the game's pause), and lets its one-shots
/// playing finish where it was. An emitter put over another then plays its
/// own sound if it plays on spawn; one put back and handed out in the same
/// frame plays once.
///
/// It belongs to the scene whose code or systems added it: while the scene
/// stack does not update that scene (a pause menu pushed over the level),
/// its voices are held with the scene's own, and a pause menu's own emitter
/// plays on above the level it holds. One added outside every scene, or
/// whose scene has left the stack while its entity lives on, is held while
/// any scene is
///
/// ```
/// use ryo2d::prelude::*;
///
/// fn spawn_waterfall(ctx: &mut SceneContext<'_>, roar: &Handle<Sound>) {
///   let emitter = AudioEmitter { sound: roar.clone(), play_on_spawn: true };
///   ctx.world.spawn((emitter, Transform::from_xy(640.0, 200.0)));
/// }
/// ```
#[derive(Component, Debug, Clone)]
#[require(Transform)]
#[component(on_insert = emitter_inserted, on_discard = emitter_discarded)]
pub struct AudioEmitter {
  /// What it plays
  pub sound: Handle<Sound>,
  /// It plays as it is added to an entity (spawned with one, say), and as
  /// its entity is enabled
  pub play_on_spawn: bool,
}

/// An emitter was added, or put over another: if it plays on spawn, and its
/// entity is enabled (not one a pool keeps disabled), the audio engine plays
/// it in the frame's audio update. The world's first emitter has the engine
/// watch for entities being disabled and enabled, whose emitters then stop
/// and play as if despawned and spawned
fn emitter_inserted(mut world: DeferredWorld, context: HookContext) {
  let entity = context.entity;
  let plays = world
    .get::<AudioEmitter>(entity)
    .is_some_and(|emitter| emitter.play_on_spawn)
    && world.get::<Disabled>(entity).is_none();
  let Some(mut engine) = world.get_resource_mut::<AudioEngine>() else {
    return;
  };
  // It belongs to the scene whose code or systems add it
  engine.emitter_added(entity);
  if plays {
    engine.emitter_spawned(entity);
  }
  if engine.watch_enabling() {
    let mut commands = world.commands();
    commands.add_observer(emitter_disabled);
    commands.add_observer(emitter_enabled);
  }
}

/// An emitter is going (taken away, put over by another, or despawned with
/// its entity): the voices it played are let go at once, to be released in
/// the frame's audio update, so the next emitter's voices are not taken
/// with them
fn emitter_discarded(mut world: DeferredWorld, context: HookContext) {
  if let Some(mut engine) = world.get_resource_mut::<AudioEngine>() {
    engine.emitter_discarded(context.entity);
  }
}

/// An entity is being disabled (a pool putting it back): an emitter on it
/// lets its voices go, as if despawned. Every entity disabled in a world
/// with emitters comes here; one whose emitter played nothing costs a
/// lookup
fn emitter_disabled(
  disabled: On<Add<Disabled>>,
  emitters: Query<(), (With<AudioEmitter>, Allow<Disabled>)>,
  engine: Option<ResMut<AudioEngine>>,
) {
  let entity = disabled.entity;
  if emitters.contains(entity)
    && let Some(mut engine) = engine
  {
    engine.emitter_gone(entity);
  }
}

/// An entity is being enabled (a pool handing it out): an emitter on it that
/// plays on spawn plays, as if spawned now. bevy also triggers this as a
/// disabled entity is despawned (every component is removed): its entry is
/// dropped in the frame's audio update, as the entity is gone
fn emitter_enabled(
  enabled: On<Remove<Disabled>>,
  emitters: Query<&AudioEmitter, Allow<Disabled>>,
  engine: Option<ResMut<AudioEngine>>,
) {
  let entity = enabled.entity;
  let plays = emitters
    .get(entity)
    .is_ok_and(|emitter| emitter.play_on_spawn);
  if plays && let Some(mut engine) = engine {
    engine.emitter_spawned(entity);
  }
}

/// The gain in dB of a sound with `spatial` settings heard from `distance`:
/// 0 within its minimum distance, silent (-60) at its maximum and beyond,
/// and between, its curve's: [`Falloff::Linear`]'s amplitude falls in a
/// line (-6 dB halfway), [`Falloff::Inverse`]'s as a source in the open
/// does, fast near it and slowly far off (about -25 dB halfway), and
/// [`Falloff::Log`]'s evenly in dB (-30 dB halfway). A distance that is not
/// a number is as far as can be
pub fn falloff_gain_db(spatial: &Spatial, distance: f32) -> f32 {
  let t = reach(spatial, distance);
  if t <= 0.0 {
    return 0.0;
  }
  if t >= 1.0 {
    return SILENCE_DB;
  }
  let db = match spatial.curve {
    Falloff::Linear => amplitude_db(1.0 - t),
    Falloff::Inverse => amplitude_db((1.0 - t) / (1.0 + INVERSE_SHARPNESS * t)),
    Falloff::Log => SILENCE_DB * t,
  };
  db.max(SILENCE_DB)
}

/// The pan of a sound with `spatial` settings heard `dx` to the listener's
/// right (negative: to its left): `dx / pan_width`, from -1 (left) to 1
/// (right)
pub fn pan_for(spatial: &Spatial, dx: f32) -> f32 {
  let pan = dx / spatial.pan_width;
  if pan.is_nan() {
    0.0
  } else {
    pan.clamp(-1.0, 1.0)
  }
}

/// How far `distance` is from `spatial`'s minimum toward its maximum: 0
/// within the minimum, 1 at the maximum and beyond, and for a distance (or
/// settings) that is not a number
fn reach(spatial: &Spatial, distance: f32) -> f32 {
  if distance <= spatial.min_distance {
    return 0.0;
  }
  let t = (distance - spatial.min_distance) / (spatial.max_distance - spatial.min_distance);
  if (0.0..1.0).contains(&t) { t } else { 1.0 }
}

/// An amplitude from 0 to 1 in dB
fn amplitude_db(amplitude: f32) -> f32 {
  20.0 * amplitude.log10()
}

/// `spatial`'s distance low-pass: its cutoff at the maximum distance, if it
/// has one that is a frequency, at most open (20 kHz: a file's above that
/// fails to load), so the filter never opens with distance
fn low_pass(spatial: &Spatial) -> Option<f64> {
  spatial
    .low_pass_at_max
    .filter(|hz| hz.is_finite() && *hz > 0.0)
    .map(|hz| hz.min(f64::from(OPEN_HZ)))
}

/// The cutoff of a distance low-pass closing to `closed` Hz: open (20 kHz)
/// within the minimum distance, `closed` at the maximum and beyond, and
/// between, down in even steps of octaves
fn cutoff_hz(spatial: &Spatial, closed: f64, distance: f32) -> f64 {
  let open = f64::from(OPEN_HZ);
  match reach(spatial, distance) {
    t if t <= 0.0 => open,
    t if t >= 1.0 => closed,
    t => open * (closed / open).powf(f64::from(t)),
  }
}

/// A voice's own gain plus its falloff gain, in dB: silent if either is
pub(crate) fn heard_db(own_db: f32, falloff_db: f32) -> f32 {
  if own_db <= SILENCE_DB || falloff_db <= SILENCE_DB {
    SILENCE_DB
  } else {
    (own_db + falloff_db).max(SILENCE_DB)
  }
}

/// Where the listener is, and which way its right is
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Listener {
  pub(crate) at: Vec2,
  right: Vec2,
}

impl Listener {
  pub(crate) fn of(global: &GlobalTransform) -> Listener {
    Listener {
      at: global.translation(),
      right: global.affine().matrix2.x_axis.normalize_or(Vec2::X),
    }
  }

  /// How far away `position` is, and how far to the listener's right
  pub(crate) fn hear(&self, position: Vec2) -> (f32, f32) {
    let offset = position - self.at;
    (offset.length(), offset.dot(self.right))
  }
}

/// Finds the listener in a world: the first [`AudioListener`], else the
/// main camera. Its queries are kept, so a frame's search allocates nothing
pub(crate) struct ListenerQuery {
  world: WorldId,
  listeners: QueryState<&'static GlobalTransform, With<AudioListener>>,
  cameras: QueryState<&'static GlobalTransform, With<MainCamera>>,
}

impl ListenerQuery {
  pub(crate) fn new(world: &mut World) -> ListenerQuery {
    ListenerQuery {
      world: world.id(),
      listeners: QueryState::new(world),
      cameras: QueryState::new(world),
    }
  }

  /// Whether it searches `world`
  pub(crate) fn is_of(&self, world: &World) -> bool {
    self.world == world.id()
  }

  /// The listener in `world`, if there is one (none in a world it was not
  /// made for)
  pub(crate) fn find(&mut self, world: &World) -> Option<Listener> {
    if !self.is_of(world) {
      return None;
    }
    let global = self
      .listeners
      .iter(world)
      .next()
      .or_else(|| self.cameras.iter(world).next())?;
    Some(Listener::of(global))
  }
}

/// Where an emitter's entity is now: `None` if it is gone, disabled (a
/// pool's, put back), or no longer an emitter
pub(crate) fn emitter_position(world: &World, entity: Entity) -> Option<Vec2> {
  let entity = world.get_entity(entity).ok()?;
  if !entity.contains::<AudioEmitter>() || entity.contains::<Disabled>() {
    return None;
  }
  entity
    .get::<GlobalTransform>()
    .map(GlobalTransform::translation)
}

/// How a voice at a place is heard
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Hearing {
  /// Its falloff gain, in dB
  pub(crate) gain_db: f32,
  pub(crate) pan: f32,
  /// Its distance low-pass's cutoff, if its sound has one
  pub(crate) cutoff: Option<f64>,
  /// It is at or beyond its maximum distance
  pub(crate) out_of_range: bool,
}

impl Hearing {
  /// A voice at `position` with `spatial` settings, heard by `listener`; as
  /// if at the listener when there is none
  pub(crate) fn of(spatial: &Spatial, position: Vec2, listener: Option<&Listener>) -> Hearing {
    let (distance, right) = listener.map_or((0.0, 0.0), |listener| listener.hear(position));
    Hearing {
      gain_db: falloff_gain_db(spatial, distance),
      pan: pan_for(spatial, right),
      cutoff: low_pass(spatial).map(|closed| cutoff_hz(spatial, closed, distance)),
      out_of_range: reach(spatial, distance) >= 1.0,
    }
  }
}

/// A voice heard from a place: its sound's spatial settings, as it started,
/// and what kira was last given
pub(crate) struct SpatialVoice {
  pub(crate) settings: Spatial,
  /// Its falloff gain in dB, added to its own
  pub(crate) gain_db: f32,
  pub(crate) pan: f32,
  /// Its distance low-pass, if its sound has one and there was room
  pub(crate) filter: Option<VoiceFilter>,
}

impl SpatialVoice {
  /// Take `hearing`'s falloff gain if it moved enough to hear (or reached
  /// full volume or silence): whether it did
  pub(crate) fn follow_gain(&mut self, hearing: &Hearing) -> bool {
    let (applied, gain) = (self.gain_db, hearing.gain_db);
    let moved = (gain - applied).abs() >= GAIN_STEP
      || (gain != applied && (gain >= 0.0 || gain <= SILENCE_DB));
    if moved {
      self.gain_db = gain;
    }
    moved
  }

  /// Take `hearing`'s pan if it moved enough to hear (or reached the
  /// middle or a side): whether it did
  pub(crate) fn follow_pan(&mut self, hearing: &Hearing) -> bool {
    let (applied, pan) = (self.pan, hearing.pan);
    let moved =
      (pan - applied).abs() >= PAN_STEP || (pan != applied && (pan == 0.0 || pan.abs() >= 1.0));
    if moved {
      self.pan = pan;
    }
    moved
  }

  /// Give its filter `hearing`'s cutoff, gliding with `tween`, if it moved
  /// enough to hear (or opened fully or closed)
  pub(crate) fn follow_cutoff(&mut self, hearing: &Hearing, tween: Tween) {
    let (Some(filter), Some(hz)) = (&mut self.filter, hearing.cutoff) else {
      return;
    };
    let applied = filter.applied;
    let ends = hz >= f64::from(OPEN_HZ) || low_pass(&self.settings) == Some(hz);
    if (hz / applied - 1.0).abs() >= CUTOFF_STEP || (hz != applied && ends) {
      filter.applied = hz;
      filter.handle.set_cutoff(hz, tween);
    }
  }
}

/// A voice's distance low-pass: a kira track of the voice's own under its
/// bus's, holding a low-pass filter. Once it is dropped, kira removes the
/// track when its sound has finished
pub(crate) struct VoiceFilter {
  pub(crate) track: TrackHandle,
  handle: FilterHandle,
  /// The cutoff kira was last given, in Hz
  pub(crate) applied: f64,
}

impl VoiceFilter {
  /// A filter track at `hz` under `parent`; `None` if kira has no room
  pub(crate) fn new(parent: &mut TrackHandle, hz: f64) -> Option<VoiceFilter> {
    // One sound plays on it: a voice that moves to rebuilt buses gets a
    // new one
    let mut builder = TrackBuilder::new()
      .sound_capacity(1)
      .sub_track_capacity(1)
      .persist_until_sounds_finish(true);
    let handle = builder.add_effect(FilterBuilder::new().mode(FilterMode::LowPass).cutoff(hz));
    let track = parent.add_sub_track(builder).ok()?;
    Some(VoiceFilter {
      track,
      handle,
      applied: hz,
    })
  }
}

#[cfg(test)]
mod tests {
  use std::f32::consts::FRAC_PI_2;
  use std::time::Duration;

  use kira::sound::PlaybackState;

  use super::*;
  use crate::core::asset::{
    AssetServer, Assets, AssetsView, AudioClipSettings, ClipMode, MemorySource,
  };
  use crate::core::audio::{
    AudioClip, AudioEngine, AudioLogEntry, AudioLogKind, AudioView, LimitRule, Mixer, SoundClip,
    Voice, VoiceLimit,
  };
  use crate::core::dev_log::{self, LogLevel};
  use crate::core::ecs::{Entity, Pool, PoolConfig, PoolExhaustedPolicy, World};
  use crate::core::graphics::{Camera2d, MainCamera};
  use crate::core::testing::{audio_step, audio_world, audio_world_over};
  use crate::core::time::Time;

  const FRAME: Duration = Duration::from_nanos(16_666_667);
  const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/audio");
  /// kira's silence, in dB
  const SILENCE: f32 = -60.0;

  /// Full volume within 100, silent from 500, panned fully 200 to a side
  fn settings(curve: Falloff) -> Spatial {
    Spatial {
      min_distance: 100.0,
      max_distance: 500.0,
      curve,
      pan_width: 200.0,
      low_pass_at_max: None,
    }
  }

  fn clip(world: &mut World, seconds: f32) -> Handle<AudioClip> {
    let frames = vec![[0.1, 0.1]; (48_000.0 * seconds) as usize];
    world
      .resource_mut::<Assets<AudioClip>>()
      .add(AudioClip::from_frames(48_000, &frames).unwrap())
  }

  /// A sound over a 2 s clip, heard from where it plays by `spatial`
  fn placed(world: &mut World, spatial: Spatial, looping: bool) -> Handle<Sound> {
    let clip = clip(world, 2.0);
    world.resource_mut::<Assets<Sound>>().add(Sound {
      clips: vec![SoundClip::new(clip)],
      looping,
      fade_out: Duration::from_millis(40),
      spatial: Some(spatial),
      ..Default::default()
    })
  }

  /// An emitter of `sound` at (`x`, 0) that plays as it spawns
  fn emitter(world: &mut World, sound: &Handle<Sound>, x: f32) -> Entity {
    let emitter = AudioEmitter {
      sound: sound.clone(),
      play_on_spawn: true,
    };
    world.spawn((emitter, Transform::from_xy(x, 0.0))).id()
  }

  /// Put `entity` at (`x`, `y`), its `GlobalTransform` at once
  fn move_to(world: &mut World, entity: Entity, x: f32, y: f32) {
    world.entity_mut(entity).insert(Transform::from_xy(x, y));
  }

  fn play_at(world: &mut World, sound: &Handle<Sound>, x: f32) -> Voice {
    AudioView::new(world).play_at(sound, Vec2::new(x, 0.0))
  }

  fn log(world: &World) -> Vec<AudioLogEntry> {
    world.resource::<AudioEngine>().log().to_vec()
  }

  /// `voice`'s log entries of `kind`
  fn entries(world: &World, voice: u32, kind: AudioLogKind) -> Vec<AudioLogEntry> {
    log(world)
      .into_iter()
      .filter(|entry| entry.voice == voice && entry.kind == kind)
      .collect()
  }

  /// The voices started, in order
  fn started(world: &World) -> Vec<u32> {
    log(world)
      .iter()
      .filter(|entry| entry.kind == AudioLogKind::Start)
      .map(|entry| entry.voice)
      .collect()
  }

  /// The one voice started
  fn only_voice(world: &World) -> u32 {
    match started(world)[..] {
      [voice] => voice,
      ref voices => panic!("one voice started, not {voices:?}: {:#?}", log(world)),
    }
  }

  /// The gains `voice` was given since it started, in order
  fn gains(world: &World, voice: u32) -> Vec<f32> {
    entries(world, voice, AudioLogKind::Gain)
      .iter()
      .map(|entry| entry.gain_db)
      .collect()
  }

  fn pans(world: &World, voice: u32) -> Vec<f32> {
    entries(world, voice, AudioLogKind::Pan)
      .iter()
      .map(|entry| entry.pan)
      .collect()
  }

  fn close(actual: &[f32], expected: &[f32]) -> bool {
    actual.len() == expected.len()
      && actual
        .iter()
        .zip(expected)
        .all(|(actual, expected)| (actual - expected).abs() < 1e-3)
  }

  fn playback(world: &World, voice: u32) -> Option<PlaybackState> {
    world.resource::<AudioEngine>().voice_playback(voice)
  }

  fn cutoff(world: &World, voice: &Voice) -> Option<f64> {
    world.resource::<AudioEngine>().voice_cutoff(voice.id())
  }

  fn warnings(matching: impl Fn(&str) -> bool) -> usize {
    dev_log::get_entries()
      .iter()
      .filter(|entry| entry.level == LogLevel::Warn && matching(&entry.message))
      .count()
  }

  /// Linear amplitude `a` in dB
  fn db(a: f32) -> f32 {
    20.0 * a.log10()
  }

  #[test]
  fn falloff_curves_match_at_min_mid_and_max() {
    let halfway = [
      // The amplitude halves
      (Falloff::Linear, db(0.5)),
      // The inverse-distance law, tapered: (1 - t) / (1 + 15t)
      (Falloff::Inverse, db(0.5 / 8.5)),
      // Halfway to silence in dB
      (Falloff::Log, -30.0),
    ];
    for (curve, mid) in halfway {
      let spatial = settings(curve);
      assert_eq!(falloff_gain_db(&spatial, 0.0), 0.0, "{curve:?} within min");
      assert_eq!(falloff_gain_db(&spatial, 100.0), 0.0, "{curve:?} at min");
      let at_mid = falloff_gain_db(&spatial, 300.0);
      assert!((at_mid - mid).abs() < 1e-3, "{curve:?} at mid: {at_mid}");
      assert_eq!(
        falloff_gain_db(&spatial, 500.0),
        SILENCE,
        "{curve:?} at max"
      );
      assert_eq!(falloff_gain_db(&spatial, 5000.0), SILENCE, "{curve:?}");
      // Quieter all the way out
      let mut last = 0.0;
      for distance in (100..=500).step_by(10) {
        let gain = falloff_gain_db(&spatial, distance as f32);
        assert!(gain <= last, "{curve:?} at {distance}: {gain} after {last}");
        last = gain;
      }
      // A distance that is not a number is as far as can be
      assert_eq!(falloff_gain_db(&spatial, f32::NAN), SILENCE, "{curve:?}");
    }
    // Inverse falls fast near the source and slowly far from it; Log evenly
    let (inverse, log) = (settings(Falloff::Inverse), settings(Falloff::Log));
    assert!(falloff_gain_db(&inverse, 140.0) < falloff_gain_db(&log, 140.0));
    assert!(falloff_gain_db(&inverse, 460.0) > falloff_gain_db(&log, 460.0));
  }

  #[test]
  fn pan_follows_the_horizontal_offset() {
    let spatial = settings(Falloff::Linear);
    let cases = [
      (0.0, 0.0),
      (100.0, 0.5),
      (-50.0, -0.25),
      (200.0, 1.0),
      (1000.0, 1.0),
      (-1000.0, -1.0),
    ];
    for (dx, pan) in cases {
      assert_eq!(pan_for(&spatial, dx), pan, "{dx} to the right");
    }
    assert_eq!(pan_for(&spatial, f32::NAN), 0.0);
  }

  /// A turned listener hears by its own right: turned a quarter, its x
  /// axis is the world's +y
  #[test]
  fn a_turned_listener_pans_by_its_own_right() {
    let turned = Transform {
      translation: Vec2::new(10.0, 10.0),
      rotation: FRAC_PI_2,
      ..Default::default()
    };
    let listener = Listener::of(&GlobalTransform::from_transform(&turned));
    let (distance, right) = listener.hear(Vec2::new(10.0, 110.0));
    assert!((distance - 100.0).abs() < 1e-3, "{distance}");
    assert!((right - 100.0).abs() < 1e-3, "{right}");
    let (_, right) = listener.hear(Vec2::new(110.0, 10.0));
    assert!(right.abs() < 1e-3, "straight ahead: {right}");
  }

  #[test]
  fn an_emitter_voice_updates_its_gain_and_pan_as_it_moves() {
    let mut world = audio_world();
    world.spawn(AudioListener);
    let hum = placed(&mut world, settings(Falloff::Linear), true);
    let entity = emitter(&mut world, &hum, 50.0);
    audio_step(&mut world, FRAME);
    let voice = only_voice(&world);
    let start = &entries(&world, voice, AudioLogKind::Start)[0];
    assert_eq!((start.gain_db, start.pan), (0.0, 0.25), "within min");
    for (x, y) in [(300.0, 0.0), (0.0, -200.0), (-150.0, 0.0)] {
      move_to(&mut world, entity, x, y);
      audio_step(&mut world, FRAME);
    }
    // Halfway; a quarter of the way, ahead; an eighth, to the left
    let expected = [db(0.5), db(0.75), db(0.875)];
    assert!(
      close(&gains(&world, voice), &expected),
      "{:?}",
      gains(&world, voice)
    );
    assert!(
      close(&pans(&world, voice), &[1.0, 0.0, -0.75]),
      "{:?}",
      pans(&world, voice)
    );
    // Each glides over the frame
    for entry in log(&world).iter().filter(|entry| entry.voice == voice) {
      if matches!(entry.kind, AudioLogKind::Gain | AudioLogKind::Pan) {
        assert_eq!(entry.fade, FRAME, "{entry:?}");
      }
    }
    // Standing still sends nothing
    let before = log(&world).len();
    for _ in 0..5 {
      audio_step(&mut world, FRAME);
    }
    assert_eq!(log(&world).len(), before, "{:#?}", &log(&world)[before..]);
  }

  #[test]
  fn a_one_shot_beyond_range_is_culled() {
    let mut world = audio_world();
    world.spawn(AudioListener);
    let shot = placed(&mut world, settings(Falloff::Linear), false);
    for x in [600.0, 500.0, -500.0] {
      let voice = play_at(&mut world, &shot, x);
      assert!(voice.is_stopped(), "at {x}: {voice:?}");
      assert_eq!(voice.id(), 0);
      let last = log(&world).pop();
      assert_eq!(
        last.as_ref().map(|entry| (entry.kind, entry.voice)),
        Some((AudioLogKind::Culled, 0)),
        "at {x}: {last:?}"
      );
      assert_eq!(last.map(|entry| entry.bus).as_deref(), Some("sfx"));
    }
    assert_eq!(AudioView::new(&mut world).voices_on("sfx"), 0);
    // Within range it plays, at its distance's gain
    let near = play_at(&mut world, &shot, 400.0);
    assert!(near.is_playing(), "{near:?}");
    let start = &entries(&world, near.id(), AudioLogKind::Start)[0];
    assert!((start.gain_db - db(0.25)).abs() < 1e-3, "{start:?}");
    assert_eq!(start.pan, 1.0);
  }

  #[test]
  fn a_loop_out_of_range_runs_silent_and_returns() {
    let mut world = audio_world();
    world.spawn(AudioListener);
    let hum = placed(&mut world, settings(Falloff::Linear), true);
    // A loop that starts out of range starts, silent
    let distant = play_at(&mut world, &hum, 900.0);
    assert!(distant.is_playing(), "{distant:?}");
    let start = &entries(&world, distant.id(), AudioLogKind::Start)[0];
    assert_eq!(start.gain_db, SILENCE);
    // The sound has started once this frame: its emitter plays in the next
    audio_step(&mut world, FRAME);
    let entity = emitter(&mut world, &hum, 300.0);
    audio_step(&mut world, FRAME);
    let voice = started(&world)[1];
    for x in [900.0, 900.0, 900.0, 100.0] {
      move_to(&mut world, entity, x, 0.0);
      audio_step(&mut world, FRAME);
    }
    assert!(
      close(&gains(&world, voice), &[SILENCE, 0.0]),
      "{:?}",
      gains(&world, voice)
    );
    // It kept playing out of range, and plays on
    assert_eq!(playback(&world, voice), Some(PlaybackState::Playing));
    let position = world
      .resource::<AudioEngine>()
      .voice_position(voice)
      .unwrap_or_default();
    assert!(position > 0.06, "it ran on: {position}");
  }

  #[test]
  fn the_camera_is_the_listener_without_an_audio_listener() {
    let mut world = audio_world();
    let camera = world
      .spawn((
        Camera2d::default(),
        MainCamera,
        Transform::from_xy(1000.0, 0.0),
      ))
      .id();
    let hum = placed(&mut world, settings(Falloff::Linear), true);
    let voice = play_at(&mut world, &hum, 1300.0).id();
    let start = &entries(&world, voice, AudioLogKind::Start)[0];
    assert!((start.gain_db - db(0.5)).abs() < 1e-3, "{start:?}");
    assert_eq!(start.pan, 1.0);
    // The camera moves, and the voice is heard from there
    move_to(&mut world, camera, 1200.0, 0.0);
    audio_step(&mut world, FRAME);
    assert!(close(&gains(&world, voice), &[0.0]));
    assert!(close(&pans(&world, voice), &[0.5]));
    // An AudioListener hears in the camera's place
    world.spawn((AudioListener, Transform::from_xy(1600.0, 0.0)));
    audio_step(&mut world, FRAME);
    assert!(
      close(&gains(&world, voice), &[0.0, db(0.5)]),
      "{:?}",
      gains(&world, voice)
    );
    assert!(close(&pans(&world, voice), &[0.5, -1.0]));
  }

  #[test]
  fn despawning_an_emitter_stops_its_loops() {
    let mut world = audio_world();
    world.spawn(AudioListener);
    let hum = placed(&mut world, settings(Falloff::Linear), true);
    let shot = placed(&mut world, settings(Falloff::Linear), false);
    let drone = placed(&mut world, settings(Falloff::Linear), true);
    let looping = emitter(&mut world, &hum, 50.0);
    let once = emitter(&mut world, &shot, 50.0);
    let kept = emitter(&mut world, &drone, 50.0);
    audio_step(&mut world, FRAME);
    let [loop_voice, shot_voice, kept_voice] = started(&world)[..] else {
      panic!("three voices: {:#?}", log(&world));
    };
    world.despawn(looping);
    world.despawn(once);
    // Taking its emitter away does as despawning it does
    world.entity_mut(kept).remove::<AudioEmitter>();
    audio_step(&mut world, FRAME);
    let stops: Vec<(u32, Duration)> = log(&world)
      .iter()
      .filter(|entry| entry.kind == AudioLogKind::Stop)
      .map(|entry| (entry.voice, entry.fade))
      .collect();
    let fade = Duration::from_millis(40);
    assert_eq!(stops, [(loop_voice, fade), (kept_voice, fade)]);
    // The one-shot plays to its end
    for _ in 0..5 {
      audio_step(&mut world, FRAME);
    }
    assert_eq!(playback(&world, shot_voice), Some(PlaybackState::Playing));
    assert_eq!(playback(&world, loop_voice), None, "gone");
  }

  /// Emitters of one sound spawned in one frame start as its `per_frame`
  /// allows, one a frame by default: the rest wait their turn rather than
  /// being dropped
  #[test]
  fn emitters_of_one_sound_take_turns_to_start() {
    let mut world = audio_world();
    world.spawn(AudioListener);
    let hum = placed(&mut world, settings(Falloff::Linear), true);
    for x in [0.0, 50.0, 100.0] {
      emitter(&mut world, &hum, x);
    }
    let mut counts = Vec::new();
    for _ in 0..5 {
      audio_step(&mut world, FRAME);
      counts.push(started(&world).len());
    }
    assert_eq!(counts, [1, 2, 3, 3, 3], "{:#?}", log(&world));
    let refused = log(&world)
      .iter()
      .filter(|entry| entry.kind == AudioLogKind::Refused)
      .count();
    assert_eq!(refused, 0, "a turn waited for is not a refusal");
  }

  #[test]
  fn the_33rd_filtered_voice_plays_unfiltered() {
    let mut world = audio_world();
    let listener = world.spawn(AudioListener).id();
    let mut spatial = settings(Falloff::Linear);
    spatial.low_pass_at_max = Some(500.0);
    let hum = placed(&mut world, spatial, true);
    if let Some(hum) = world.resource_mut::<Assets<Sound>>().get_mut(&hum) {
      hum.per_frame = u8::MAX;
    }
    let voices: Vec<Voice> = (0..33).map(|_| play_at(&mut world, &hum, 300.0)).collect();
    // Halfway, halfway down in octaves from 20 kHz to 500 Hz
    let halfway = 20_000.0 * (500.0f64 / 20_000.0).sqrt();
    for voice in &voices[..32] {
      let hz = cutoff(&world, voice);
      assert!(hz.is_some_and(|hz| (hz - halfway).abs() < 0.1), "{hz:?}");
    }
    assert!(voices[32].is_playing(), "{:?}", voices[32]);
    assert_eq!(cutoff(&world, &voices[32]), None, "unfiltered");
    // Each filter is a kira track of its own under the bus's
    assert_eq!(world.resource::<AudioEngine>().sub_tracks_on("sfx"), 32);
    // The cutoff follows the distance: the listener comes close
    move_to(&mut world, listener, 300.0, 0.0);
    audio_step(&mut world, FRAME);
    assert_eq!(cutoff(&world, &voices[1]), Some(20_000.0));
    // A filtered voice that ends frees its filter for the next
    voices[0].stop(Duration::ZERO);
    for _ in 0..3 {
      audio_step(&mut world, FRAME);
    }
    let next = play_at(&mut world, &hum, 300.0);
    assert_eq!(cutoff(&world, &next), Some(20_000.0));
  }

  /// A voice limit's "quietest" is the quietest heard: its own gain and its
  /// distance's
  #[test]
  fn stop_quietest_hears_the_distance() {
    let mut world = audio_world();
    world.spawn(AudioListener);
    let clip = clip(&mut world, 2.0);
    let sound = world.resource_mut::<Assets<Sound>>().add(Sound {
      clips: vec![SoundClip::new(clip)],
      limit: Some(VoiceLimit {
        max: 2,
        rule: LimitRule::StopQuietest,
      }),
      spatial: Some(settings(Falloff::Linear)),
      ..Default::default()
    });
    let near = play_at(&mut world, &sound, 100.0);
    audio_step(&mut world, FRAME);
    let far = play_at(&mut world, &sound, 400.0);
    audio_step(&mut world, FRAME);
    play_at(&mut world, &sound, 150.0);
    let stolen: Vec<u32> = log(&world)
      .iter()
      .filter(|entry| entry.kind == AudioLogKind::Stolen)
      .map(|entry| entry.voice)
      .collect();
    assert_eq!(stolen, [far.id()]);
    assert!(near.is_playing());
  }

  /// An emitter that does not play as it spawns plays when asked
  #[test]
  fn an_emitter_plays_when_asked() {
    let mut world = audio_world();
    world.spawn(AudioListener);
    let hum = placed(&mut world, settings(Falloff::Linear), true);
    let entity = world
      .spawn((
        AudioEmitter {
          sound: hum,
          play_on_spawn: false,
        },
        Transform::from_xy(-300.0, 0.0),
      ))
      .id();
    audio_step(&mut world, FRAME);
    assert!(started(&world).is_empty());
    let voice = AudioView::new(&mut world).play_emitter(entity);
    assert!(voice.is_playing(), "{voice:?}");
    let start = &entries(&world, voice.id(), AudioLogKind::Start)[0];
    assert!((start.gain_db - db(0.5)).abs() < 1e-3, "{start:?}");
    assert_eq!(start.pan, -1.0);
    // It follows the entity, and stops with it
    move_to(&mut world, entity, 0.0, 0.0);
    audio_step(&mut world, FRAME);
    assert!(close(&gains(&world, voice.id()), &[0.0]));
    world.despawn(entity);
    audio_step(&mut world, FRAME);
    assert_eq!(
      entries(&world, voice.id(), AudioLogKind::Stop).len(),
      1,
      "{:#?}",
      log(&world)
    );
    // An entity with no emitter plays nothing
    let bare = world.spawn(Transform::default()).id();
    assert!(AudioView::new(&mut world).play_emitter(bare).is_stopped());
  }

  /// A pool's emitters play as the pool hands their entities out, not as it
  /// makes them (disabled), and stop as it takes them back
  #[test]
  fn a_pooled_emitter_plays_when_handed_out_and_stops_when_put_back() {
    let mut world = audio_world();
    world.spawn(AudioListener);
    let hum = placed(&mut world, settings(Falloff::Linear), true);
    let factory = move || {
      let emitter = AudioEmitter {
        sound: hum.clone(),
        play_on_spawn: true,
      };
      (emitter, Transform::from_xy(50.0, 0.0))
    };
    let config = PoolConfig {
      initial_capacity: 4,
      ..Default::default()
    };
    let mut pool = Pool::new(factory, config);
    let Some(first) = pool.acquire(&mut world) else {
      panic!("the pool hands one out");
    };
    audio_step(&mut world, FRAME);
    assert_eq!(started(&world).len(), 1, "{:#?}", log(&world));
    let voice = started(&world)[0];
    pool.release(&mut world, first);
    audio_step(&mut world, FRAME);
    assert_eq!(entries(&world, voice, AudioLogKind::Stop).len(), 1);
    // Handed out again, it plays again
    assert!(pool.acquire(&mut world).is_some());
    audio_step(&mut world, FRAME);
    assert_eq!(started(&world).len(), 2, "{:#?}", log(&world));
  }

  /// Letting an emitter's voices go looks only at that entity's voices: an
  /// entity with none (a pool's, or any entity disabled in a world with
  /// emitters) costs nothing however many voices play
  #[test]
  fn an_emitter_s_cleanup_looks_only_at_its_own_voices() {
    let mut world = audio_world();
    world.spawn(AudioListener);
    let hum = placed(&mut world, settings(Falloff::Linear), true);
    let bed = clip(&mut world, 2.0);
    let bed = world.resource_mut::<Assets<Sound>>().add(Sound {
      clips: vec![SoundClip::new(bed)],
      looping: true,
      per_frame: 40,
      ..Default::default()
    });
    for _ in 0..40 {
      assert!(AudioView::new(&mut world).play(&bed).is_playing());
    }
    let playing = emitter(&mut world, &hum, 50.0);
    let quiet = world
      .spawn((
        AudioEmitter {
          sound: hum.clone(),
          play_on_spawn: false,
        },
        Transform::default(),
      ))
      .id();
    let bare = world.spawn(Transform::default()).id();
    audio_step(&mut world, FRAME);
    assert_eq!(started(&world).len(), 41, "{:#?}", log(&world));
    let examined = |world: &World| world.resource::<AudioEngine>().emitter_examined;
    let before = examined(&world);

    // Entities with no emitter voices: disabled, their emitter taken away
    world.entity_mut(quiet).insert(Disabled);
    world.entity_mut(bare).insert(Disabled);
    world.entity_mut(quiet).remove::<AudioEmitter>();
    assert_eq!(
      examined(&world),
      before,
      "nothing to clean, nothing looked at"
    );

    // The playing emitter's one voice, and only it, is looked at and let go
    let tracked = |world: &World| world.resource::<AudioEngine>().emitter_voice_count();
    assert_eq!(tracked(&world), 1);
    let voice = started(&world)[40];
    world.despawn(playing);
    assert_eq!(examined(&world), before + 1);
    assert_eq!(tracked(&world), 0);
    audio_step(&mut world, FRAME);
    assert_eq!(stops(&world), [(voice, Duration::from_millis(40))]);

    // An emitter's one-shot that plays out is forgotten as it ends, its
    // entity still there
    let shot = placed(&mut world, settings(Falloff::Linear), false);
    let shooter = emitter(&mut world, &shot, 50.0);
    audio_step(&mut world, FRAME);
    assert_eq!(tracked(&world), 1);
    for _ in 0..130 {
      audio_step(&mut world, FRAME);
    }
    assert!(world.get_entity(shooter).is_ok());
    assert_eq!(tracked(&world), 0, "a 2 s one-shot ended");
  }

  /// The voices that stopped, with their fades
  fn stops(world: &World) -> Vec<(u32, Duration)> {
    log(world)
      .iter()
      .filter(|entry| entry.kind == AudioLogKind::Stop)
      .map(|entry| (entry.voice, entry.fade))
      .collect()
  }

  /// A pool of emitters of `sound` at (50, 0) that play on spawn
  fn emitter_pool(sound: &Handle<Sound>, config: PoolConfig) -> Pool<(AudioEmitter, Transform)> {
    let sound = sound.clone();
    let factory = move || {
      let emitter = AudioEmitter {
        sound: sound.clone(),
        play_on_spawn: true,
      };
      (emitter, Transform::from_xy(50.0, 0.0))
    };
    Pool::new(factory, config)
  }

  /// Review I1: a pool takes an entity back and hands it out again in one
  /// frame (it hands out the latest freed first): its loop stops and plays
  /// again once, not twice. Taking an emitter away and adding it back, or
  /// disabling and enabling it, in one frame does the same
  #[test]
  fn an_emitter_put_back_and_handed_out_in_one_frame_plays_once() {
    let mut world = audio_world();
    world.spawn(AudioListener);
    let hum = placed(&mut world, settings(Falloff::Linear), true);
    let mut pool = emitter_pool(&hum, PoolConfig::default());
    let Some(first) = pool.acquire(&mut world) else {
      panic!("the pool hands one out");
    };
    audio_step(&mut world, FRAME);
    let old = only_voice(&world);
    pool.release(&mut world, first);
    assert_eq!(pool.acquire(&mut world), Some(first), "the same entity");
    for _ in 0..5 {
      audio_step(&mut world, FRAME);
    }
    let fade = Duration::from_millis(40);
    assert_eq!(stops(&world), [(old, fade)], "{:#?}", log(&world));
    assert_eq!(started(&world).len(), 2);
    assert_eq!(AudioView::new(&mut world).voices_on("sfx"), 1);
    // Its emitter taken away and added back
    let again = started(&world)[1];
    world.entity_mut(first).remove::<AudioEmitter>();
    world.entity_mut(first).insert(AudioEmitter {
      sound: hum.clone(),
      play_on_spawn: true,
    });
    for _ in 0..5 {
      audio_step(&mut world, FRAME);
    }
    assert_eq!(stops(&world), [(old, fade), (again, fade)]);
    assert_eq!(started(&world).len(), 3);
    assert_eq!(AudioView::new(&mut world).voices_on("sfx"), 1);
    // Spawned, then disabled and enabled before the audio runs: once
    let entity = emitter(&mut world, &hum, 50.0);
    world.entity_mut(entity).insert(Disabled);
    world.entity_mut(entity).remove::<Disabled>();
    audio_step(&mut world, FRAME);
    assert_eq!(started(&world).len(), 4, "{:#?}", log(&world));
    assert_eq!(AudioView::new(&mut world).voices_on("sfx"), 2);
  }

  /// Review M1: a pool that recycles its oldest entity puts it back and
  /// hands it out again: its loop stops and its emitter plays again
  #[test]
  fn a_recycled_pooled_emitter_plays_again() {
    let mut world = audio_world();
    world.spawn(AudioListener);
    let hum = placed(&mut world, settings(Falloff::Linear), true);
    let config = PoolConfig {
      initial_capacity: 1,
      max_capacity: 1,
      expand_batch_size: 1,
      exhausted_policy: PoolExhaustedPolicy::RecycleOldest,
    };
    let mut pool = emitter_pool(&hum, config);
    let first = pool.acquire(&mut world);
    audio_step(&mut world, FRAME);
    let old = only_voice(&world);
    assert_eq!(pool.acquire(&mut world), first, "recycled");
    for _ in 0..5 {
      audio_step(&mut world, FRAME);
    }
    assert_eq!(started(&world).len(), 2, "{:#?}", log(&world));
    assert_eq!(stops(&world), [(old, Duration::from_millis(40))]);
    assert_eq!(AudioView::new(&mut world).voices_on("sfx"), 1);
  }

  /// Review M2: an emitter put over another stops the old one's voice, with
  /// its fade, and plays its own sound if it plays on spawn
  #[test]
  fn replacing_an_emitter_swaps_its_sound() {
    let mut world = audio_world();
    world.spawn(AudioListener);
    let hum = placed(&mut world, settings(Falloff::Linear), true);
    let drone = placed(&mut world, settings(Falloff::Linear), true);
    let entity = emitter(&mut world, &hum, 50.0);
    audio_step(&mut world, FRAME);
    let old = only_voice(&world);
    world.entity_mut(entity).insert(AudioEmitter {
      sound: drone.clone(),
      play_on_spawn: true,
    });
    audio_step(&mut world, FRAME);
    let fade = Duration::from_millis(40);
    assert_eq!(stops(&world), [(old, fade)], "{:#?}", log(&world));
    let new = started(&world)[1..].to_vec();
    assert_eq!(new.len(), 1, "{:#?}", log(&world));
    let start = &entries(&world, new[0], AudioLogKind::Start)[0];
    assert_eq!(start.sound, format!("{:?}", drone.id()));
    // One that does not play on spawn only stops the old one
    world.entity_mut(entity).insert(AudioEmitter {
      sound: hum.clone(),
      play_on_spawn: false,
    });
    audio_step(&mut world, FRAME);
    assert_eq!(stops(&world), [(old, fade), (new[0], fade)]);
    assert_eq!(started(&world).len(), 2);
  }

  /// Review M3: each filtered voice's kira track goes once its voice has
  /// ended
  #[test]
  fn the_filter_tracks_go_once_their_voices_end() {
    let mut world = audio_world();
    world.spawn(AudioListener);
    let mut spatial = settings(Falloff::Linear);
    spatial.low_pass_at_max = Some(500.0);
    let blip = clip(&mut world, 0.03);
    let shot = world.resource_mut::<Assets<Sound>>().add(Sound {
      clips: vec![SoundClip::new(blip)],
      spatial: Some(spatial),
      per_frame: 4,
      ..Default::default()
    });
    let mut filtered = 0;
    for _ in 0..20 {
      for _ in 0..4 {
        let voice = play_at(&mut world, &shot, 300.0);
        filtered += usize::from(cutoff(&world, &voice).is_some());
      }
      audio_step(&mut world, FRAME);
    }
    assert_eq!(filtered, 80, "every one filtered");
    for _ in 0..10 {
      audio_step(&mut world, FRAME);
    }
    assert_eq!(AudioView::new(&mut world).voices_on("sfx"), 0);
    assert_eq!(world.resource::<AudioEngine>().sub_tracks_on("sfx"), 0);
  }

  /// An emitter spawned while its sound loads plays once it has
  #[test]
  fn an_emitter_waits_for_its_sound_to_load() {
    let files = MemorySource::new();
    files.insert(
      "sfx/waits.sound.ron",
      r#"(clips: [(path: "sfx/waits.wav")], looping: true, spatial: (min_distance: 100.0, max_distance: 500.0))"#,
    );
    files.insert(
      "sfx/waits.wav",
      std::fs::read(format!("{FIXTURES}/tone.wav")).unwrap_or_default(),
    );
    let mut world = audio_world_over(files);
    world.spawn(AudioListener);
    let hum = AssetsView::new(&mut world).load::<Sound>("sfx/waits.sound.ron");
    emitter(&mut world, &hum, 50.0);
    for _ in 0..500 {
      if !started(&world).is_empty() {
        break;
      }
      audio_step(&mut world, Duration::from_millis(10));
      std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(started(&world).len(), 1, "{:#?}", log(&world));
  }

  /// `play_emitter` waits for a sound still loading as `play` does: a loop
  /// reports playing and starts once its sound lands, its voice the
  /// entity's (despawning the entity stops it); a one-shot is refused then,
  /// logged as loading; and a play whose entity goes while it waits never
  /// starts
  #[test]
  fn play_emitter_waits_for_a_sound_still_loading() {
    let spatial = "spatial: (min_distance: 100.0, max_distance: 500.0)";
    let files = MemorySource::new();
    files.insert(
      "sfx/emitted-loop.sound.ron",
      format!(r#"(clips: [(path: "sfx/emitted.wav")], looping: true, {spatial})"#),
    );
    files.insert(
      "sfx/emitted-shot.sound.ron",
      format!(r#"(clips: [(path: "sfx/emitted.wav")], {spatial})"#),
    );
    files.insert(
      "sfx/emitted-gone.sound.ron",
      format!(r#"(clips: [(path: "sfx/emitted.wav")], looping: true, {spatial})"#),
    );
    files.insert(
      "sfx/emitted.wav",
      std::fs::read(format!("{FIXTURES}/tone.wav")).unwrap_or_default(),
    );
    let mut world = audio_world_over(files);
    world.spawn(AudioListener);
    let quiet = |world: &mut World, path: &str, x: f32| {
      let sound = AssetsView::new(world).load::<Sound>(path);
      let emitter = AudioEmitter {
        sound: sound.clone(),
        play_on_spawn: false,
      };
      let entity = world.spawn((emitter, Transform::from_xy(x, 0.0))).id();
      (sound, entity)
    };
    let (hum, looping) = quiet(&mut world, "sfx/emitted-loop.sound.ron", 50.0);
    let (shot, once) = quiet(&mut world, "sfx/emitted-shot.sound.ron", 50.0);
    let (_, going) = quiet(&mut world, "sfx/emitted-gone.sound.ron", 50.0);
    let (looped, refused, gone) = {
      let mut audio = AudioView::new(&mut world);
      (
        audio.play_emitter(looping),
        audio.play_emitter(once),
        audio.play_emitter(going),
      )
    };
    assert!(looped.is_playing() && looped.id() != 0, "{looped:?}");
    assert!(gone.is_playing(), "{gone:?}");
    world.despawn(going);
    for _ in 0..500 {
      let server = world.resource::<AssetServer>();
      if server.is_ready(&hum) && server.is_ready(&shot) {
        break;
      }
      audio_step(&mut world, Duration::from_millis(10));
      std::thread::sleep(Duration::from_millis(2));
    }
    audio_step(&mut world, FRAME);
    assert_eq!(started(&world), [looped.id()], "{:#?}", log(&world));
    assert!(looped.is_playing(), "{looped:?}");
    assert!(gone.is_stopped(), "{gone:?}");
    assert!(refused.is_stopped(), "{refused:?}");
    let tracked = |world: &World| world.resource::<AudioEngine>().emitter_voice_count();
    assert_eq!(
      tracked(&world),
      1,
      "the loop; the plays that never started are forgotten"
    );
    let refusals: Vec<(String, &str)> = log(&world)
      .into_iter()
      .filter(|entry| entry.kind == AudioLogKind::Refused)
      .map(|entry| (entry.sound, entry.reason))
      .collect();
    let [(sound, reason)] = &refusals[..] else {
      panic!("one refused: {refusals:?}");
    };
    assert_eq!(sound, "sfx/emitted-shot.sound.ron");
    assert!(reason.starts_with("loading"), "{reason}");
    // The entity's: despawning it stops the loop
    world.despawn(looping);
    audio_step(&mut world, FRAME);
    assert_eq!(entries(&world, looped.id(), AudioLogKind::Stop).len(), 1);
    assert_eq!(tracked(&world), 0);
  }

  /// A sound with no spatial settings played at a place is heard as if at
  /// the listener, with a warning once
  #[test]
  fn a_sound_without_spatial_settings_played_at_a_place_warns_once() {
    let files = MemorySource::new();
    files.insert(
      "sfx/flat-at-a-place.sound.ron",
      r#"(clips: [(path: "sfx/flat-at-a-place.wav")])"#,
    );
    files.insert(
      "sfx/flat-at-a-place.wav",
      std::fs::read(format!("{FIXTURES}/tone.wav")).unwrap_or_default(),
    );
    let mut world = audio_world_over(files);
    world.spawn(AudioListener);
    let flat = match AssetsView::new(&mut world).load_now::<Sound>("sfx/flat-at-a-place.sound.ron")
    {
      Ok(sound) => sound,
      Err(error) => panic!("{}", error.report()),
    };
    for _ in 0..3 {
      let voice = play_at(&mut world, &flat, 900.0);
      assert!(voice.is_playing(), "not culled: {voice:?}");
      let start = &entries(&world, voice.id(), AudioLogKind::Start)[0];
      assert_eq!((start.gain_db, start.pan), (0.0, 0.0));
      audio_step(&mut world, FRAME);
    }
    let named = |message: &str| {
      message.starts_with("sfx/flat-at-a-place.sound.ron:") && message.contains("no spatial")
    };
    assert_eq!(warnings(named), 1);
  }

  /// A spatial voice holds with its bus while the game is paused, and its
  /// place still reaches it
  #[test]
  fn a_spatial_voice_holds_with_its_bus() {
    let mut world = audio_world();
    world.spawn(AudioListener);
    let hum = placed(&mut world, settings(Falloff::Linear), true);
    let entity = emitter(&mut world, &hum, 300.0);
    audio_step(&mut world, FRAME);
    let voice = only_voice(&world);
    world.resource_mut::<Time>().game.pause();
    move_to(&mut world, entity, 100.0, 0.0);
    audio_step(&mut world, FRAME);
    assert_eq!(playback(&world, voice), Some(PlaybackState::Paused));
    assert!(close(&gains(&world, voice), &[0.0]));
    world.resource_mut::<Time>().game.resume();
    audio_step(&mut world, FRAME);
    assert_eq!(playback(&world, voice), Some(PlaybackState::Playing));
  }

  /// A mixer rebuild restarts a loop on the new buses with its filter
  #[test]
  fn a_filtered_loop_keeps_its_filter_across_a_mixer_rebuild() {
    let mut world = audio_world();
    world.spawn(AudioListener);
    let mut spatial = settings(Falloff::Linear);
    spatial.low_pass_at_max = Some(500.0);
    let hum = placed(&mut world, spatial, true);
    let voice = play_at(&mut world, &hum, 500.0);
    audio_step(&mut world, FRAME);
    assert_eq!(cutoff(&world, &voice), Some(500.0));
    AudioView::new(&mut world).set_mixer(Mixer::default());
    audio_step(&mut world, FRAME);
    assert_eq!(cutoff(&world, &voice), Some(500.0));
    assert_eq!(world.resource::<AudioEngine>().sub_tracks_on("sfx"), 1);
    let start = entries(&world, voice.id(), AudioLogKind::Start);
    assert_eq!(start.len(), 2, "restarted: {start:#?}");
    assert_eq!((start[1].gain_db, start[1].pan), (SILENCE, 1.0));
  }

  // What the mock backend renders from a placed voice: the gain, pan and
  // low-pass a listener hears, not only what the engine sends kira

  /// How a rendered test's clip plays
  #[derive(Debug, Clone, Copy, PartialEq)]
  enum Mode {
    Decoded,
    Streamed,
  }

  /// How a rendered test's voice is played
  #[derive(Debug, Clone, Copy, PartialEq)]
  enum Via {
    PlayAt,
    Emitter,
  }

  /// Every clip mode with every way of playing
  const COVERAGE: [(Mode, Via); 4] = [
    (Mode::Decoded, Via::PlayAt),
    (Mode::Decoded, Via::Emitter),
    (Mode::Streamed, Via::PlayAt),
    (Mode::Streamed, Via::Emitter),
  ];

  /// The mock backend's rate, and the rendered clips'
  const RATE: u32 = 48_000;
  /// A rendered clip's constant sample, and its level as kira reads it
  const DC: i16 = 3_277;
  const LEVEL: f32 = DC as f32 / 32_768.0;

  /// Two seconds of [`DC`], a mono file's samples
  fn steady() -> Vec<i16> {
    vec![DC; 2 * RATE as usize]
  }

  /// A voice of a looping sound heard from a place, in a metered world
  struct Rendered {
    world: World,
    meter: crate::core::audio::Meter,
    listener: Entity,
    emitter: Option<Entity>,
    mode: Mode,
  }

  /// `samples` as a mono 48 kHz file, decoded or streamed, looped by a
  /// sound heard by `spatial`, played at `at` (or from an emitter there,
  /// which plays as the first step spawns it), the listener at the origin
  fn rendered(samples: &[i16], mode: Mode, via: Via, spatial: Spatial, at: Vec2) -> Rendered {
    let files = MemorySource::new();
    files.insert("clip.wav", crate::core::testing::wav_samples(RATE, samples));
    let (mut world, meter) = crate::core::testing::metered_audio_world_over(files);
    let settings = AudioClipSettings {
      mode: match mode {
        Mode::Decoded => ClipMode::Decode,
        Mode::Streamed => ClipMode::Stream,
      },
    };
    let clip = match AssetsView::new(&mut world).load_now_with::<AudioClip, _>("clip.wav", settings)
    {
      Ok(clip) => clip,
      Err(error) => panic!("{}", error.report()),
    };
    assert_eq!(
      world
        .resource::<Assets<AudioClip>>()
        .get(&clip)
        .map(AudioClip::is_streamed),
      Some(mode == Mode::Streamed)
    );
    let sound = world.resource_mut::<Assets<Sound>>().add(Sound {
      clips: vec![SoundClip::new(clip)],
      looping: true,
      spatial: Some(spatial),
      ..Default::default()
    });
    let listener = world.spawn((AudioListener, Transform::IDENTITY)).id();
    let emitter = match via {
      Via::PlayAt => {
        AudioView::new(&mut world).play_at(&sound, at);
        None
      }
      Via::Emitter => {
        let emitter = AudioEmitter {
          sound,
          play_on_spawn: true,
        };
        Some(world.spawn((emitter, Transform::from_xy(at.x, at.y))).id())
      }
    };
    Rendered {
      world,
      meter,
      listener,
      emitter,
      mode,
    }
  }

  impl Rendered {
    /// `frames` frames, a stream given time to decode between them
    fn step(&mut self, frames: usize) {
      for _ in 0..frames {
        audio_step(&mut self.world, FRAME);
        if self.mode == Mode::Streamed {
          std::thread::sleep(Duration::from_millis(2));
        }
      }
    }

    /// The listener moved to `at`
    fn listen_from(&mut self, at: Vec2) {
      move_to(&mut self.world, self.listener, at.x, at.y);
    }

    /// The last `count` frames rendered
    fn last(&self, count: usize) -> Vec<kira::Frame> {
      self.meter.rendered(count)
    }
  }

  /// Each side's level over `frames` in dB (a constant's RMS is its level;
  /// silence is -inf)
  fn levels_db(frames: &[kira::Frame]) -> (f32, f32) {
    let rms = |side: fn(&kira::Frame) -> f32| {
      let power = frames.iter().map(|frame| side(frame).powi(2)).sum::<f32>();
      (power / frames.len() as f32).sqrt()
    };
    (db(rms(|frame| frame.left)), db(rms(|frame| frame.right)))
  }

  /// The pan `frames` were rendered at, by kira's constant-power law: left
  /// is `a·√(1 - p)` and right `a·√(1 + p)`, so `p` is `(R² - L²) / (R² +
  /// L²)`
  fn pan_of(frames: &[kira::Frame]) -> f32 {
    let (left, right) = frames.iter().fold((0.0, 0.0), |(left, right), frame| {
      (left + frame.left.powi(2), right + frame.right.powi(2))
    });
    (right - left) / (right + left)
  }

  /// The amplitude of the `hz` sine in `frames`' left side: its correlation
  /// with a sine and a cosine there (exact for a whole number of periods)
  fn amplitude(frames: &[kira::Frame], hz: f64) -> f32 {
    let step = std::f64::consts::TAU * hz / f64::from(RATE);
    let (cos, sin) = frames
      .iter()
      .enumerate()
      .fold((0.0, 0.0), |(cos, sin), (n, frame)| {
        let phase = step * n as f64;
        let x = f64::from(frame.left);
        (cos + x * phase.cos(), sin + x * phase.sin())
      });
    let n = frames.len() as f64;
    (2.0 / n * (cos * cos + sin * sin).sqrt()) as f32
  }

  /// A spatial loop renders at its curve's gain on each curve, within its
  /// minimum distance, between, and near its maximum, decoded or streamed,
  /// played at a place or from an emitter
  #[test]
  fn a_placed_loop_renders_at_its_curve_s_gain() {
    for curve in [Falloff::Linear, Falloff::Log, Falloff::Inverse] {
      let spatial = settings(curve);
      for (mode, via) in COVERAGE {
        let mut voice = rendered(&steady(), mode, via, spatial, Vec2::ZERO);
        voice.step(10);
        // Straight ahead, so it is not panned
        for distance in [50.0, 300.0, 480.0] {
          voice.listen_from(Vec2::new(0.0, distance));
          voice.step(6);
          let expected = db(LEVEL) + falloff_gain_db(&spatial, distance);
          let (left, right) = levels_db(&voice.last(400));
          assert!(
            (left - expected).abs() < 0.5 && (right - expected).abs() < 0.5,
            "{curve:?}, {mode:?} {via:?}, at {distance}: {left:.2} and {right:.2} dB, not {expected:.2}"
          );
        }
      }
    }
  }

  /// Full volume within 200, and fully to one side 150 to it
  fn sides() -> Spatial {
    Spatial {
      min_distance: 200.0,
      max_distance: 800.0,
      curve: Falloff::Linear,
      pan_width: 150.0,
      low_pass_at_max: None,
    }
  }

  /// A voice a full pan width to the listener's right renders near silence
  /// on the left and about +3 dB on the right (kira's constant-power pan,
  /// `Frame::panned`, gives the side √2); mirrored on the left; equal ahead
  #[test]
  fn a_full_pan_width_to_one_side_renders_on_that_side() {
    let (level, sided) = (db(LEVEL), db(LEVEL * std::f32::consts::SQRT_2));
    for (mode, via) in COVERAGE {
      for at in [
        Vec2::new(150.0, 0.0),
        Vec2::new(-150.0, 0.0),
        Vec2::new(0.0, 100.0),
      ] {
        let mut voice = rendered(&steady(), mode, via, sides(), at);
        voice.step(10);
        let (left, right) = levels_db(&voice.last(400));
        let case = format!("{mode:?} {via:?} at {at}: {left:.2} and {right:.2} dB");
        if at.x > 0.0 {
          assert!(left < level - 60.0, "{case}");
          assert!((right - sided).abs() < 0.5, "{case}: +3 dB is {sided:.2}");
        } else if at.x < 0.0 {
          assert!(right < level - 60.0, "{case}");
          assert!((left - sided).abs() < 0.5, "{case}: +3 dB is {sided:.2}");
        } else {
          assert!((left - right).abs() < 0.01, "{case}");
          assert!((left - level).abs() < 0.5, "{case}");
        }
      }
    }
  }

  /// An emitter crossing from the listener's left to its right moves the
  /// balance each frame to where it now is, gliding over the frame: each
  /// frame's first 64 frames are within 0.03 of where the last frame ended
  /// (the glide has moved about 0.017 of the frame's 0.2 step by then; a
  /// change sent without it is 0.05 along, as kira eases a change only
  /// across its first 128-frame chunk), and its last 64 are at its place's
  /// pan
  #[test]
  fn an_emitter_crossing_moves_the_balance_within_each_frame() {
    for mode in [Mode::Decoded, Mode::Streamed] {
      let mut voice = rendered(
        &steady(),
        mode,
        Via::Emitter,
        sides(),
        Vec2::new(-150.0, 0.0),
      );
      voice.step(10);
      let Some(entity) = voice.emitter else {
        panic!("an emitter");
      };
      let mut was = pan_of(&voice.last(64));
      assert!(
        (was + 1.0).abs() < 0.01,
        "{mode:?}: it starts on the left: {was}"
      );
      for step in 1..=10 {
        let x = -150.0 + 30.0 * step as f32;
        move_to(&mut voice.world, entity, x, 0.0);
        let before = voice.meter.rendered_count();
        voice.step(1);
        let frame = voice.last(voice.meter.rendered_count() - before);
        let expected = pan_for(&sides(), x);
        let (first, last) = (pan_of(&frame[..64]), pan_of(&frame[frame.len() - 64..]));
        assert!(
          (first - was).abs() < 0.03,
          "{mode:?} to {x}: the frame starts at {first}, not gliding from {was} to {expected}"
        );
        assert!(
          (last - expected).abs() < 0.03,
          "{mode:?} to {x}: the frame ends at {last}, not {expected}"
        );
        was = last;
      }
      assert!(
        (was - 1.0).abs() < 0.03,
        "{mode:?}: it ends on the right: {was}"
      );
    }
  }

  /// With `low_pass_at_max`, a pair of tones (200 Hz and 8 kHz) loses its
  /// high one with distance as the cutoff closes: near the maximum the high
  /// tone has fallen at least 12 dB more than the low one
  #[test]
  fn the_distance_low_pass_takes_the_high_band_first() {
    let spatial = Spatial {
      low_pass_at_max: Some(500.0),
      ..settings(Falloff::Linear)
    };
    let tau = std::f64::consts::TAU;
    let pair: Vec<i16> = (0..2 * RATE)
      .map(|n| {
        let t = f64::from(n) / f64::from(RATE);
        let sample = 0.05 * (tau * 200.0 * t).sin() + 0.05 * (tau * 8_000.0 * t).sin();
        (sample * 32_767.0).round() as i16
      })
      .collect();
    for (mode, via) in COVERAGE {
      let mut voice = rendered(&pair, mode, via, spatial, Vec2::ZERO);
      voice.step(10);
      let mut bands = Vec::new();
      for distance in [50.0, 300.0, 480.0] {
        voice.listen_from(Vec2::new(0.0, distance));
        voice.step(12);
        // A tenth of a second: whole periods of both tones
        let frames = voice.last(4_800);
        bands.push((amplitude(&frames, 200.0), amplitude(&frames, 8_000.0)));
      }
      let (low, high) = bands[0];
      assert!(
        low > 0.04 && high > 0.03,
        "{mode:?} {via:?}: open: {bands:?}"
      );
      let fall = |(l, h): (f32, f32)| (db(l / low), db(h / high));
      let (low_mid, high_mid) = fall(bands[1]);
      let (low_far, high_far) = fall(bands[2]);
      assert!(
        high_mid < low_mid - 3.0,
        "{mode:?} {via:?}: between, the highs fall more: {low_mid:.1} and {high_mid:.1} dB"
      );
      assert!(
        low_far - high_far >= 12.0,
        "{mode:?} {via:?}: near the maximum, the highs fall 12 dB more: {low_far:.1} and {high_far:.1} dB"
      );
    }
  }
}
