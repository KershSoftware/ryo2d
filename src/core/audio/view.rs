//! [`AudioView`]: the audio engine as a scene reaches it (`ctx.audio()`)

use super::engine::AudioEngine;
use super::{AudioClip, Sound, Voice};
use crate::core::asset::Handle;
use crate::core::ecs::kernel::Mut;
use crate::core::ecs::{Vec2, World};

/// What [`AudioView::play`] takes: a sound definition's handle or a clip's
/// (`&handle`)
pub enum Playable<'a> {
  /// A sound definition, played as it says
  Sound(&'a Handle<Sound>),
  /// A clip, played with the empty definition: on the `sfx` bus at its
  /// recorded level and pitch, with no limit
  Clip(&'a Handle<AudioClip>),
}

impl<'a> From<&'a Handle<Sound>> for Playable<'a> {
  fn from(sound: &'a Handle<Sound>) -> Self {
    Playable::Sound(sound)
  }
}

impl<'a> From<&'a Handle<AudioClip>> for Playable<'a> {
  fn from(clip: &'a Handle<AudioClip>) -> Self {
    Playable::Clip(clip)
  }
}

/// How one play differs from the sound's own settings, for
/// [`AudioView::play_with`]. The default changes nothing
#[derive(Debug, Clone, Copy, Default)]
pub struct PlayOptions {
  /// Where in the world it plays, for 2D spatial sound
  pub position: Option<Vec2>,
  /// It belongs to the engine and outlives the scene that played it,
  /// instead of stopping when that scene exits
  pub persist: bool,
  /// Added to the sound's gain, in dB (-6 is about half as loud)
  pub volume_db: f32,
  /// Added to the sound's pitch, in semitones (12 is an octave up, and
  /// twice as fast)
  pub pitch: f32,
}

/// The audio engine seen from a scene: plays sounds and reports on the
/// buses. A scene gets one from `ctx.audio()`; it borrows the world, so take
/// it, use it and let it go. It plays a [`Sound`] definition as the
/// definition says, or a clip directly:
///
/// ```
/// use ryo2d::prelude::*;
///
/// struct Coin(Handle<Sound>);
///
/// impl Scene for Coin {
///   fn on_enter(&mut self, ctx: &mut SceneContext) {
///     self.0 = ctx.assets().load("sfx/coin.sound.ron");
///   }
///
///   fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
///     if ctx.input.key_pressed(KeyCode::Space) {
///       // A sound still loading plays nothing
///       ctx.audio().play(&self.0);
///     }
///     SceneAction::Continue
///   }
/// }
/// ```
pub struct AudioView<'w> {
  world: &'w mut World,
}

impl<'w> AudioView<'w> {
  /// The audio engine of `world`. A world the engine did not make has
  /// none, and every play on it returns a stopped voice; `testing::audio_world`
  /// makes one that has
  pub fn new(world: &'w mut World) -> Self {
    Self { world }
  }

  /// Play `sound` (`&handle`, of a [`Sound`] or an [`AudioClip`]) and return
  /// its [`Voice`]. A sound that has not loaded yet, or failed to (warned
  /// about once), plays nothing, and a play its cooldown or a voice limit
  /// refuses starts nothing: the voice returned is already stopped
  pub fn play<'a>(&mut self, sound: impl Into<Playable<'a>>) -> Voice {
    self.play_with(sound, PlayOptions::default())
  }

  /// [`play`](Self::play) with [`PlayOptions`]: louder or quieter, higher or
  /// lower than the sound's own settings
  pub fn play_with<'a>(&mut self, sound: impl Into<Playable<'a>>, options: PlayOptions) -> Voice {
    let playable = sound.into();
    // The engine is taken out of the world while it reads the stores
    self
      .world
      .try_resource_scope(|world: &mut World, mut engine: Mut<'_, AudioEngine>| {
        engine.play(world, playable, &options)
      })
      .unwrap_or_default()
  }

  /// How many voices play on `bus` and every bus under it (`master` counts
  /// them all); 0 for a bus that does not exist
  pub fn voices_on(&self, bus: &str) -> usize {
    self
      .world
      .get_resource::<AudioEngine>()
      .map_or(0, |engine| engine.voices_on(bus))
  }
}
