//! [`AudioView`]: the audio engine as a scene reaches it (`ctx.audio()`)

use std::time::Duration;

use super::engine::AudioEngine;
use super::{AudioClip, Mixer, Music, Sound, Voice, VolumeKind};
use crate::core::asset::{AssetId, Assets, Handle};
use crate::core::ecs::kernel::Mut;
use crate::core::ecs::{Entity, Vec2, World};

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
  /// Where in the world it plays, heard from there by its sound's spatial
  /// settings (see [`AudioView::play_at`]); as if at the listener without
  /// them
  pub position: Option<Vec2>,
  /// It belongs to the engine and outlives the scene that played it. It
  /// matters for loops, and for one-shots paused or held by the game's
  /// pause: when the scene exits, those stop with their fade, while a
  /// one-shot playing then plays to its end (a click just before a switch
  /// is heard out). A loop meant to carry on into the next scene (rain,
  /// say) needs it
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
  /// its [`Voice`], which belongs to the scene playing it (a loop, or a
  /// paused one-shot, stops as the scene exits; see
  /// [`PlayOptions::persist`]). A sound that has not
  /// loaded yet, or failed to (warned about once), plays nothing, and a play
  /// its cooldown or a voice limit refuses starts nothing: the voice
  /// returned is already stopped
  pub fn play<'a>(&mut self, sound: impl Into<Playable<'a>>) -> Voice {
    self.play_with(sound, PlayOptions::default())
  }

  /// Play `sound` at `position` in the world, heard from there by the
  /// listener (an [`AudioListener`](super::AudioListener), or else the main
  /// camera) as its sound's spatial settings ([`Sound::spatial`]) say:
  /// quieter with distance, panned by its offset to the listener's right,
  /// and muffled with distance if it has a distance low-pass. A one-shot at
  /// or beyond its maximum distance is culled: it never starts, and the
  /// voice returned is already stopped. A loop there starts silent, to be
  /// heard as the listener comes near. The place stays where it was played;
  /// for a sound that follows an entity, see
  /// [`play_emitter`](Self::play_emitter)
  ///
  /// ```
  /// use ryo2d::prelude::*;
  ///
  /// fn explode(ctx: &mut SceneContext<'_>, boom: &Handle<Sound>, at: Vec2) {
  ///   ctx.audio().play_at(boom, at);
  /// }
  /// ```
  pub fn play_at<'a>(&mut self, sound: impl Into<Playable<'a>>, position: Vec2) -> Voice {
    let options = PlayOptions {
      position: Some(position),
      ..Default::default()
    };
    self.play_with(sound, options)
  }

  /// Play the sound of `entity`'s [`AudioEmitter`](super::AudioEmitter)
  /// where the entity is, as [`play_at`](Self::play_at) does, its voice
  /// following the entity as it moves. The voice belongs to the entity, not
  /// to the scene: it stops (a loop, or a one-shot paused) when the entity
  /// is despawned, disabled or loses its emitter. An entity that is not an
  /// emitter, or is disabled, plays nothing (the voice returned is already
  /// stopped)
  pub fn play_emitter(&mut self, entity: Entity) -> Voice {
    self
      .world
      .try_resource_scope(|world: &mut World, mut engine: Mut<'_, AudioEngine>| {
        engine.prepare(world);
        engine.play_emitter(world, entity)
      })
      .unwrap_or_default()
  }

  /// [`play`](Self::play) with [`PlayOptions`]: louder or quieter, higher or
  /// lower than the sound's own settings
  pub fn play_with<'a>(&mut self, sound: impl Into<Playable<'a>>, options: PlayOptions) -> Voice {
    let playable = sound.into();
    // The engine is taken out of the world while it reads the stores
    self
      .world
      .try_resource_scope(|world: &mut World, mut engine: Mut<'_, AudioEngine>| {
        engine.prepare(world);
        engine.play(world, playable, &options)
      })
      .unwrap_or_default()
  }

  /// Set the player's volume setting `kind`, from 0 (silent) to 1 (the mix
  /// as designed), on the perceptual curve: 0.5 is about -12 dB. Playing
  /// sounds follow it at once (smoothed over 10 ms). A value below 0 or
  /// above 1 is clamped, and one that is not a number is the kind's default
  pub fn set_volume_setting(&mut self, kind: VolumeKind, value: f32) {
    if let Some(mut engine) = self.world.get_resource_mut::<AudioEngine>() {
      engine.set_volume_setting(kind, value);
    }
  }

  /// The player's volume setting `kind`, from 0 to 1
  pub fn volume_setting(&self, kind: VolumeKind) -> f32 {
    self
      .world
      .get_resource::<AudioEngine>()
      .map_or(kind.default_value(), |engine| engine.volume_setting(kind))
  }

  /// Set `bus`'s volume in dB, in place of the mixer's (at most +24; -60
  /// and below are silent). The player's setting, a snapshot and ducking
  /// still apply on top. It lasts for the session, through mixer reloads,
  /// and applies to a bus the mixer adds later
  pub fn set_bus_volume(&mut self, bus: &str, db: f32) {
    if let Some(mut engine) = self.world.get_resource_mut::<AudioEngine>() {
      engine.set_bus_volume(bus, db);
    }
  }

  /// Blend the mix to the mixer's snapshot `name` over the snapshot's
  /// blend time, from wherever it is: its volumes are a layer over the
  /// mixer's and the player's, and its low-pass cutoffs close with it. One
  /// snapshot is active at a time. A name the mixer has none of is warned
  /// about once and changes nothing
  pub fn snapshot(&mut self, name: &str) {
    if let Some(mut engine) = self.world.get_resource_mut::<AudioEngine>() {
      engine.snapshot(name);
    }
  }

  /// Blend back from the active snapshot, over its blend time
  pub fn clear_snapshot(&mut self) {
    if let Some(mut engine) = self.world.get_resource_mut::<AudioEngine>() {
      engine.clear_snapshot();
    }
  }

  /// Build the buses from `mixer`, made in code, in place of the game's
  /// mixer file (a game with no file, such as one on the web, or a test).
  /// It applies at the end of the frame as a saved file would: loops carry
  /// on, at their position, on the new buses, and one-shots finish where
  /// they are. A mixer that fails its check (a bus named twice, say) is
  /// warned about and the buses stay as they are
  pub fn set_mixer(&mut self, mixer: Mixer) {
    let Some(mut mixers) = self.world.get_resource_mut::<Assets<Mixer>>() else {
      return;
    };
    let mixer = mixers.add(mixer);
    if let Some(mut engine) = self.world.get_resource_mut::<AudioEngine>() {
      engine.set_mixer(Some(mixer));
    }
  }

  /// Play `music` on the engine's one music player, on its bus (`music`, or
  /// one under it). It belongs to the engine, not to the scene playing it:
  /// it carries on across scene changes until it is stopped or replaced, or
  /// a scene's `SceneCleanup::MUSIC` stops it.
  ///
  /// - With a track playing, that track fades out and `music` fades in, both
  ///   over `music`'s crossfade ([`Music::crossfade`]); with none, it fades
  ///   in over its own fade-in.
  /// - The track already playing plays on: nothing restarts (a paused one
  ///   resumes), so every scene can ask for its track as it enters.
  /// - A track still loading takes over once it has loaded, and the track
  ///   playing plays until then; one that did not load is warned about once
  ///   and changes nothing
  ///
  /// ```
  /// use ryo2d::prelude::*;
  ///
  /// fn to_battle(ctx: &mut SceneContext<'_>, battle: &Handle<Music>) {
  ///   ctx.audio().play_music(battle);
  /// }
  /// ```
  pub fn play_music(&mut self, music: &Handle<Music>) {
    self
      .world
      .try_resource_scope(|world: &mut World, mut engine: Mut<'_, AudioEngine>| {
        engine.play_music(world, music);
      });
  }

  /// Stop the music, fading out over `fade` (at least 10 ms), and forget a
  /// track still loading. Nothing plays until the next
  /// [`play_music`](Self::play_music)
  pub fn stop_music(&mut self, fade: Duration) {
    self
      .world
      .try_resource_scope(|world: &mut World, mut engine: Mut<'_, AudioEngine>| {
        engine.stop_music(world, fade);
      });
  }

  /// Pause the music where it is (smoothed over 10 ms), until
  /// [`resume_music`](Self::resume_music), or `play_music` of the same track
  pub fn pause_music(&mut self) {
    self
      .world
      .try_resource_scope(|world: &mut World, mut engine: Mut<'_, AudioEngine>| {
        engine.pause_music(world);
      });
  }

  /// Resume paused music where it paused (smoothed over 10 ms)
  pub fn resume_music(&mut self) {
    self
      .world
      .try_resource_scope(|world: &mut World, mut engine: Mut<'_, AudioEngine>| {
        engine.resume_music(world);
      });
  }

  /// The music playing (or paused), or the track asked for that takes over
  /// once it has loaded; `None` with no music, or once it was stopped
  pub fn current_music(&self) -> Option<AssetId<Music>> {
    self
      .world
      .get_resource::<AudioEngine>()
      .and_then(AudioEngine::current_music)
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
