//! Run the engine without a window, for tests.
//!
//! [`HeadlessEngine`] drives a real scene through the full update and render
//! pipeline at a fixed timestep and reads frames back as pixels:
//!
//! ```rust,ignore
//! use ryo2d::prelude::*;
//! use ryo2d::testing::HeadlessEngine;
//!
//! #[test]
//! fn enemy_turns_red_when_hit() {
//!   // None (with a "skipping: no GPU" line) on machines without a GPU
//!   let Some(mut engine) = HeadlessEngine::new_or_skip(
//!     EngineBuilder::new().resolution(320, 180).start_scene(Arena::default()),
//!   ) else { return };
//!
//!   engine.step_frames(10).unwrap();
//!   engine.tap_key(KeyCode::Space);
//!   engine.step_frames(30).unwrap();
//!
//!   let frame = engine.frame().unwrap();
//!   assert_eq!(frame.get_pixel(160, 90).0, [255, 0, 0, 255]);
//! }
//! ```
//!
//! Headless runs play sound on kira's mock backend, silently: `ctx.audio()`
//! works as it does on a device. Their saves stay in memory, never on disk
//! ([`HeadlessEngine::saves`] is the store). They have no gamepads, no hot
//! reload and no dev windows; they never read or write `system.json` user
//! preferences, ignore `RYO_CAPTURE_*` and `RYO_INPUT`, and keep the FPS,
//! frame-timing and stats overlays off. `Ryo.toml` is honoured as in a normal build. Input
//! injected with `press_key`/`tap_key`/`press_mouse`/`mouse_move` is queued
//! like window events and seen from the next step; the engine's own debug
//! keys (F1-F5, F9, F10, F12) are inert headless. Seed any randomness your
//! scene uses if you compare frames exactly. Particles need no seed: a
//! headless engine seeds them from a fixed seed, so the same emitters added
//! in the same order repeat run to run, and
//! [`ParticleEmitter::with_seed`](crate::core::particles::ParticleEmitter::with_seed)
//! fixes one emitter's particles whatever else the scene adds.
//!
//! [`UiHarness`] runs UI frames with no window or GPU at all: a UI function
//! of `&mut UiContext` over scripted pointer, wheel and `ui.*` action input,
//! for unit-testing game UI.

#[cfg(test)]
mod atlas_bleeding;
#[cfg(test)]
mod capture_matches_frame;
#[doc(hidden)]
pub mod fixtures;
#[cfg(test)]
mod font_gpu;
mod gpu;
#[cfg(test)]
mod gpu_world;
mod headless;
#[cfg(test)]
mod image_gpu;
mod retain;
#[cfg(test)]
mod shader_gpu;
#[cfg(test)]
mod tile_bleeding;
pub(crate) mod ui;

pub use headless::{HeadlessEngine, OffscreenOptions, StepOutcome, offscreen_with_format};
pub use retain::PixelPair;
#[cfg(test)]
pub(crate) use ui::reload_and_wait;
pub use ui::{UiCacheStats, UiHarness};

pub use crate::core::error::HeadlessError;
pub use crate::core::render::UiRenderStats;
pub use crate::core::saves::MemorySaves;
pub use gpu::shared_gpu;

use std::time::Duration;

use glam::Vec2;

use crate::core::asset::{AssetServer, MemorySource, insert_registry};
use crate::core::audio::{AudioEngine, audio_system};
use crate::core::ecs::{Entity, World};
use crate::core::particles::ParticleEmitter;
use crate::core::particles::ParticleStore;
use crate::core::time::Time;

/// Where the particles of `emitter` (an entity with an engine particle
/// emitter) are, as its last step left them, oldest first; none without an
/// emitter or a particle store
#[doc(hidden)]
pub fn particle_positions(world: &World, emitter: Entity) -> Vec<Vec2> {
  let Some(slot) = world
    .get::<ParticleEmitter>(emitter)
    .map(|emitter| emitter.slot)
  else {
    return Vec::new();
  };
  world
    .get_resource::<ParticleStore>()
    .and_then(|store| store.pool(slot))
    .filter(|pool| pool.entity == emitter)
    .map_or_else(Vec::new, |pool| {
      let particles = &pool.main;
      particles
        .x
        .iter()
        .zip(&particles.y)
        .map(|(&x, &y)| Vec2::new(x, y))
        .collect()
    })
}

/// A world with the asset registry, as the engine's world starts: an
/// `AssetServer` over an empty `MemorySource`, and a store for each of the
/// engine's asset types. Its handles name assets only in this world, not in
/// an engine's or another world's. A store comes only from a world (`Assets` says
/// why), so a test with no engine adds its assets here:
///
/// ```
/// use ryo2d::prelude::*;
///
/// let mut world = ryo2d::testing::asset_world();
/// let dot = world
///   .resource_mut::<Assets<Image>>()
///   .add(Image::solid_rect(4, 4, 255, 255, 255, 255));
/// world.spawn((Transform::default(), Renderable::image(dot.clone())));
/// let images = world.resource::<Assets<Image>>();
/// assert_eq!(images.get(&dot).map(Image::width), Some(4));
/// ```
pub fn asset_world() -> World {
  asset_world_over(MemorySource::new())
}

/// [`asset_world`] over `files`, a `MemorySource` the caller keeps: for a
/// bench that loads files with no engine (`benches/tilemap.rs`)
#[doc(hidden)]
pub fn asset_world_over(files: MemorySource) -> World {
  let mut world = World::new();
  insert_registry(&mut world, AssetServer::new(files));
  world
}

/// [`asset_world`] with a clock and the audio engine on kira's mock backend
/// (silent, advanced by [`audio_step`]), its audio log on: a world to test
/// what plays, with no engine or audio device. Its picks and draws start
/// from the headless engine's fixed seed, so a test's variations repeat run
/// to run; `AudioView::seed` picks another sequence.
///
/// ```
/// use std::time::Duration;
/// use ryo2d::prelude::*;
/// use ryo2d::testing::{audio_step, audio_world};
///
/// let mut world = audio_world();
/// let beep = world
///   .resource_mut::<Assets<AudioClip>>()
///   .add(AudioClip::from_frames(48_000, &[[0.2, 0.2]; 2_400]).unwrap());
/// let voice = AudioView::new(&mut world).play(&beep);
/// assert!(voice.is_playing());
/// audio_step(&mut world, Duration::from_millis(100));
/// assert!(voice.is_stopped(), "a 50 ms beep is over after 100 ms");
/// ```
pub fn audio_world() -> World {
  let mut world = asset_world();
  add_audio(&mut world);
  world
}

/// [`audio_world`] whose registry reads `files`: a test (or bench) can save
/// over a file and reload it, as hot reload would
///
/// ```
/// use std::time::Duration;
/// use ryo2d::prelude::*;
/// use ryo2d::testing::{audio_step, audio_world_over};
///
/// let files = MemorySource::new();
/// let path = "audio/mixer.mixer.ron";
/// files.insert(path, r#"(buses: { "sfx": (volume: -6.0) })"#);
/// let mut world = audio_world_over(files.clone());
/// let server = world.resource::<AssetServer>().clone();
/// let mixer = server.load_now::<Mixer>(&mut world, path).unwrap();
/// let first = world.resource::<Assets<Mixer>>().version(&mixer);
/// // Save a change over the file, and reload it as hot reload would
/// files.insert(path, r#"(buses: { "sfx": (volume: -12.0) })"#);
/// server.reload(path);
/// while world.resource::<Assets<Mixer>>().version(&mixer) == first {
///   audio_step(&mut world, Duration::ZERO);
/// }
/// ```
pub fn audio_world_over(files: MemorySource) -> World {
  let mut world = World::new();
  insert_registry(&mut world, AssetServer::new(files));
  add_audio(&mut world);
  world
}

/// A mono 16-bit WAV file of `frames` frames at `rate`, every sample
/// `value`
#[cfg(test)]
pub(crate) fn wav(rate: u32, frames: usize, value: i16) -> Vec<u8> {
  wav_samples(rate, &vec![value; frames])
}

/// A mono 16-bit WAV file at `rate` of `samples`
#[cfg(test)]
pub(crate) fn wav_samples(rate: u32, samples: &[i16]) -> Vec<u8> {
  let frames = samples.len();
  let data = u32::try_from(frames * 2).unwrap_or(u32::MAX);
  let mut bytes = Vec::with_capacity(44 + frames * 2);
  bytes.extend_from_slice(b"RIFF");
  bytes.extend_from_slice(&(36 + data).to_le_bytes());
  bytes.extend_from_slice(b"WAVEfmt ");
  bytes.extend_from_slice(&16u32.to_le_bytes());
  // PCM, one channel
  bytes.extend_from_slice(&1u16.to_le_bytes());
  bytes.extend_from_slice(&1u16.to_le_bytes());
  bytes.extend_from_slice(&rate.to_le_bytes());
  bytes.extend_from_slice(&(rate * 2).to_le_bytes());
  // Two bytes a frame, 16 bits a sample
  bytes.extend_from_slice(&2u16.to_le_bytes());
  bytes.extend_from_slice(&16u16.to_le_bytes());
  bytes.extend_from_slice(b"data");
  bytes.extend_from_slice(&data.to_le_bytes());
  for sample in samples {
    bytes.extend_from_slice(&sample.to_le_bytes());
  }
  bytes
}

/// [`audio_world`] with a meter on what the mock backend renders
#[cfg(test)]
pub(crate) fn metered_audio_world() -> (World, crate::core::audio::Meter) {
  metered_audio_world_over(MemorySource::new())
}

/// [`audio_world_over`] with a meter on what the mock backend renders
#[cfg(test)]
pub(crate) fn metered_audio_world_over(files: MemorySource) -> (World, crate::core::audio::Meter) {
  let mut world = audio_world_over(files);
  let (mut audio, meter) = AudioEngine::new_metered();
  audio.set_logging(true);
  audio.seed(crate::core::audio::HEADLESS_SEED);
  world.insert_resource(audio);
  (world, meter)
}

fn add_audio(world: &mut World) {
  world.insert_resource(Time::new(60));
  let mut audio = AudioEngine::new(false);
  audio.set_logging(true);
  audio.seed(crate::core::audio::HEADLESS_SEED);
  world.insert_resource(audio);
}

/// One frame of an [`audio_world`] lasting `dt` of real time: the clock
/// advances, the asset registry pumps, and the audio engine applies the
/// voices' commands and renders `dt` on the mock backend (in whole chunks
/// of 128 frames at 48 kHz, the rest carried to the next step); then the
/// change trackers are cleared, as the engine's `Last` clears them, so the
/// removal message `audio_system` writes as it takes its engine out of the
/// world does not pile up
pub fn audio_step(world: &mut World, dt: Duration) {
  if let Some(mut time) = world.get_resource_mut::<Time>() {
    time.advance(dt);
  }
  crate::core::asset::pump(world);
  audio_system(world);
  world.clear_trackers();
}
