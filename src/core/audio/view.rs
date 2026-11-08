//! [`AudioView`]: the audio engine as a scene reaches it (`ctx.audio()`)

use std::time::Duration;

use super::engine::AudioEngine;
use super::{AudioClip, Mixer, Music, Sound, Voice};
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
  /// matters for loops, for one-shots paused or held, and for one-shots
  /// with more than 2 s left: when the scene exits, those stop with their
  /// fade, while a one-shot nearly over, or one on `ui`, plays to its end
  /// (a click just before a switch is heard out). A loop meant to carry on
  /// into the next scene (rain, say), or a long line of dialogue meant to,
  /// needs it. It also plays on while the scene is not updated (a pause
  /// menu pushed over it; `Scene::holds_audio_below`)
  pub persist: bool,
  /// Added to the sound's gain, in dB (-6 is about half as loud)
  pub volume_db: f32,
  /// Added to the sound's pitch, in semitones (12 is an octave up, and
  /// twice as fast)
  pub pitch: f32,
}

/// The audio engine seen from a scene: plays sounds and reports on the
/// buses. A scene gets one from `ctx.audio()`; it borrows the world, so take
/// it, use it and let it go (a system takes [`Audio`](super::Audio)
/// instead, and an exclusive system `AudioView::new(world)`). It plays a
/// [`Sound`] definition as the definition says, or a clip directly:
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
///       // A one-shot still loading plays nothing
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
  /// its [`Voice`], which belongs to the scene playing it (a loop, a paused
  /// one-shot, or one with more than 2 s left that is not on `ui`, stops as
  /// the scene exits; see [`PlayOptions::persist`]).
  ///
  /// A loop played while its sound loads waits for it: its voice reports
  /// playing, takes what the game does to it, and starts with its fade-in
  /// once the sound is ready (or ends if it fails to load, which the
  /// registry warns about once), and its scene's exit ends it as it does
  /// any loop. A definition still loading is not known to loop until it
  /// lands, so its play waits either way, and a one-shot is refused then.
  /// A play that waited counts against its sound's
  /// [`per_frame`](Sound::per_frame) as it lands, as a play made then
  /// does, so the same loop played twice while it loads starts once. A
  /// one-shot played while its sound loads would only be late, so it is
  /// refused (logged, and warned about once in a debug build), as is a play
  /// of a sound that failed to load (warned about once). A play its
  /// cooldown, its `per_frame` or a voice limit refuses starts nothing. A
  /// refused play's voice is already stopped. A loop its bus's cap, or a
  /// limit whose rule ranks (`StopQuietest`, `StopFarthest`,
  /// `StopLowestPriority`), has no room for is not refused but virtual: it
  /// reports playing, keeps its place silently, and comes back when there is
  /// room ([`Voice::is_virtual`]); `PreventNew` and `StopOldest` refuse a
  /// loop as they do a one-shot
  pub fn play<'a>(&mut self, sound: impl Into<Playable<'a>>) -> Voice {
    self.play_with(sound, PlayOptions::default())
  }

  /// Play `sound` at `position` in the world, heard from there by the
  /// listener (an [`AudioListener`](super::AudioListener), or else the main
  /// camera) as its sound's spatial settings ([`Sound::spatial`]) say:
  /// quieter with distance, panned by its offset to the listener's right,
  /// and muffled with distance if it has a distance low-pass. A one-shot at
  /// or beyond its maximum distance is culled: it never starts, and the
  /// voice returned is already stopped. A loop there is virtual: it takes no
  /// voice, and comes back as the listener comes within 95% of that
  /// distance ([`Voice::is_virtual`]). The place stays where it was played;
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
  /// is despawned, disabled or loses its emitter. A sound still loading is
  /// waited for as [`play`](Self::play) waits: a loop starts where the
  /// entity is once it lands (unless the entity has gone), and a one-shot
  /// is refused then. An entity that is not an emitter, or is disabled,
  /// plays nothing (the voice returned is already stopped)
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

  /// Set `bus`'s volume in dB, in place of the mixer's (at most +24; -60
  /// and below are silent). The player's setting, a snapshot and ducking
  /// still apply on top. It lasts for the session, through mixer reloads,
  /// and applies to a bus the mixer adds later
  pub fn set_bus_volume(&mut self, bus: &str, db: f32) {
    self.with_mixer(|engine| engine.set_bus_volume(bus, db));
  }

  /// Blend the mix to the mixer's snapshot `name` over the snapshot's
  /// blend time, from wherever it is: its volumes are a layer over the
  /// mixer's and the player's, and its low-pass cutoffs close with it. One
  /// snapshot is active at a time. A name the mixer has none of is warned
  /// about once and changes nothing. Asked for before the mixer has loaded,
  /// it applies once the mixer lands (the last call before then wins); if
  /// the mixer fails to load, one warning names the snapshot
  pub fn snapshot(&mut self, name: &str) {
    self.with_mixer(|engine| engine.snapshot(name));
  }

  /// Blend back from the active snapshot, over its blend time
  pub fn clear_snapshot(&mut self) {
    self.with_mixer(|engine| engine.clear_snapshot());
  }

  /// Run `f` on the engine with the bus tree built from the mixer, if it
  /// landed or changed since: a mixer loaded with `load_now` is there at
  /// once
  fn with_mixer<R: Default>(&mut self, f: impl FnOnce(&mut AudioEngine) -> R) -> R {
    self
      .world
      .try_resource_scope(|world: &mut World, mut engine: Mut<'_, AudioEngine>| {
        engine.mixer_now(world);
        f(&mut engine)
      })
      .unwrap_or_default()
  }

  /// Build the buses from `mixer`, made in code, in place of the game's
  /// mixer file (a game with no file, or a test; on the web, one whose own
  /// asset source does not hold it).
  /// It is built at once, as a saved file is: loops carry on, at their
  /// position, on the new buses, and one-shots finish where they are, the
  /// old buses' reverbs and delays ringing out after them (up to 10 s). A
  /// mixer that fails its check (a bus named twice, say) is warned about and
  /// the buses stay as they are
  pub fn set_mixer(&mut self, mixer: Mixer) {
    let Some(mut mixers) = self.world.get_resource_mut::<Assets<Mixer>>() else {
      return;
    };
    let mixer = mixers.add(mixer);
    self
      .world
      .try_resource_scope(|world: &mut World, mut engine: Mut<'_, AudioEngine>| {
        engine.set_mixer(Some(mixer));
        engine.mixer_now(world);
      });
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
  ///   and changes nothing, even to a track still loading.
  /// - A track that cannot start (its stream already failed to decode, or
  ///   kira has no room on its bus) leaves the track playing alone. One whose
  ///   stream fails to open only as it starts, or that a limit on its bus
  ///   refuses, is found out after the track playing was told to fade: the
  ///   music goes quiet, and the refusal is logged and warned about.
  /// - Only music stops music: no sound takes its place under a voice
  ///   limit, whatever its priority
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
      .and_then(|engine| engine.current_music())
  }

  /// Seed what picks a sound's clips and draws its gains and pitches: from
  /// the same seed, the same plays in the same order pick and draw the same.
  /// For replays, tests and captures. Headless runs (`HeadlessEngine`,
  /// `RYO_HEADLESS`) and test worlds (`testing::audio_world`) start seeded,
  /// so they repeat without it; a game's otherwise starts from the system's
  /// randomness
  ///
  /// ```
  /// use ryo2d::prelude::*;
  ///
  /// fn start_replay(ctx: &mut SceneContext<'_>, seed: u64) {
  ///   ctx.audio().seed(seed);
  /// }
  /// ```
  pub fn seed(&mut self, seed: u64) {
    if let Some(mut engine) = self.world.get_resource_mut::<AudioEngine>() {
      engine.seed(seed);
    }
  }

  /// Whether the audio waits for the player's first gesture: true only on
  /// the web, until a click, a tap or a key press (not Escape) opens the
  /// audio device. Until then one-shots are refused, and loops and music
  /// wait to start. A gamepad never opens it (browsers do not count pad
  /// input), so a web game shows "click or tap to start" while this is
  /// true
  pub fn waiting_for_gesture(&self) -> bool {
    self
      .world
      .get_resource::<AudioEngine>()
      .is_some_and(|engine| engine.is_locked())
  }

  /// How many voices play on `bus` and every bus under it (`master` counts
  /// them all), each taking a voice on its track: the virtual ones
  /// ([`virtual_voices_on`](Self::virtual_voices_on)) are not counted; 0
  /// for a bus that does not exist
  pub fn voices_on(&mut self, bus: &str) -> usize {
    self.with_mixer(|engine| engine.voices_on(bus))
  }

  /// How many loops on `bus` and every bus under it are virtual: keeping
  /// their place silently, taking no voice, until there is room for them
  /// and they are in range ([`Voice::is_virtual`]); 0 for a bus that does
  /// not exist
  ///
  /// ```
  /// use ryo2d::prelude::*;
  ///
  /// fn crickets(ctx: &mut SceneContext<'_>) -> String {
  ///   let mut audio = ctx.audio();
  ///   let (heard, kept) = (audio.voices_on("ambience"), audio.virtual_voices_on("ambience"));
  ///   format!("{heard} crickets heard, {kept} waiting their turn")
  /// }
  /// ```
  pub fn virtual_voices_on(&mut self, bus: &str) -> usize {
    self.with_mixer(|engine| engine.virtual_on(bus))
  }
}
