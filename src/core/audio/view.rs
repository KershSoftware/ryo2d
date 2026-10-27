//! [`AudioView`]: the audio engine as a scene reaches it (`ctx.audio()`)

use super::engine::{AudioEngine, ClipSource};
use super::{AudioClip, Voice};
use crate::core::asset::{AssetServer, Assets, Handle};
use crate::core::ecs::{Vec2, World};

/// What [`AudioView::play`] takes: a clip's handle (`&handle`)
pub enum Playable<'a> {
  /// A clip, played on the `sfx` bus at its recorded level and pitch
  Clip(&'a Handle<AudioClip>),
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
/// it, use it and let it go:
///
/// ```
/// use ryo2d::prelude::*;
///
/// struct Coin(Handle<AudioClip>);
///
/// impl Scene for Coin {
///   fn on_enter(&mut self, ctx: &mut SceneContext) {
///     self.0 = ctx.assets().load("sfx/coin.ogg");
///   }
///
///   fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
///     if ctx.input.key_pressed(KeyCode::Space) {
///       // A clip still loading plays nothing
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

  /// Play `sound` (`&handle`) and return its [`Voice`]. A sound that has not
  /// loaded yet, or failed to, plays nothing: the voice returned is already
  /// stopped
  pub fn play<'a>(&mut self, sound: impl Into<Playable<'a>>) -> Voice {
    self.play_with(sound, PlayOptions::default())
  }

  /// [`play`](Self::play) with [`PlayOptions`]: louder or quieter, higher or
  /// lower than the sound's own settings
  pub fn play_with<'a>(&mut self, sound: impl Into<Playable<'a>>, options: PlayOptions) -> Voice {
    let Playable::Clip(clip) = sound.into();
    let Some((source, version)) = self
      .world
      .get_resource::<Assets<AudioClip>>()
      .and_then(|clips| Some((ClipSource::of(clips.get(clip)?), clips.version(clip)?)))
    else {
      return Voice::stopped();
    };
    // Names clips in the log and in warnings; a clone shares the server
    let server = self.world.get_resource::<AssetServer>().cloned();
    match self.world.get_resource_mut::<AudioEngine>() {
      Some(mut engine) => {
        engine.play_clip(clip.clone(), version, source, &options, server.as_ref())
      }
      None => Voice::stopped(),
    }
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
