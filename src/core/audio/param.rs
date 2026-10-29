//! [`Audio`]: the audio engine as an ECS system reaches it. Its plays are
//! queued in a system buffer and applied where the system's commands apply

use super::engine::AudioEngine;
use super::voice::VoiceState;
use super::{AudioClip, PlayOptions, Playable, Sound, Voice, VolumeKind};
use crate::core::asset::Handle;
use crate::core::ecs::kernel::{
  Deferred, DeferredWorld, Mut, Res, SystemBuffer, SystemMeta, SystemParam,
};
use crate::core::ecs::{Entity, Vec2, World};

/// The audio engine as a system reaches it: plays sounds, and sets the mix
/// and the player's volume settings. Take it as a parameter (`mut audio:
/// Audio`) in a system added to a phase or a scene's schedule; it does what
/// `ctx.audio()` ([`AudioView`](super::AudioView)) does for a scene's
/// callbacks.
///
/// **Its plays are deferred.** `Audio` reads the engine and does not borrow
/// it mutably, so systems that play sound run beside others with no
/// conflict between them. Each play is queued in the system's buffer, which
/// applies where the system's commands apply (at the end of its schedule,
/// or at a sync point before a system ordered after it), and each returns
/// its [`Voice`] at once:
///
/// - until the play applies, the voice reports playing, and
///   [`Voice::pick`] is `None` (the clip is picked, and the gain and pitch
///   drawn, as it applies: a subtitle for the line picked is read in a
///   later system, or the next frame);
/// - the changes made to it meanwhile apply after its start, so a plain
///   change made in the system that played it (`set_volume`, say) is heard
///   from its first audio, as one made in the frame a voice starts is;
/// - a play refused as it applies (its `per_frame`, a cooldown, a limit, a
///   one-shot still loading) leaves the voice stopped from then on.
///
/// A play applied in a scene's section (a system of the scene's own
/// schedule, added with `ctx.add_systems`) belongs to that scene, as a play
/// from its callbacks does; one from a system added to a phase
/// (`EngineBuilder::add_systems`) is the engine's, and plays on across
/// scenes. An observer that takes `Audio` puts its queue in the world's
/// commands, which apply at the world's next flush: one triggered from
/// scene code (`ctx.world.trigger(..)`) may apply its plays outside the
/// scene's section, so they need not be the scene's, and a loop among them
/// can outlive the scene (play it from the scene's own code, or stop it in
/// `on_exit`). Music is a scene's call (`ctx.audio().play_music`), as is
/// loading; an exclusive system (`&mut World`) plays at once with
/// `AudioView::new(world)`. A world without an audio engine plays nothing:
/// every voice returned is already stopped.
///
/// ```
/// use ryo2d::prelude::*;
///
/// /// A walker's footstep sound, and the time to its next step
/// #[derive(Component)]
/// struct Footsteps {
///   sound: Handle<Sound>,
///   next: f32,
/// }
///
/// /// Each walker steps every 0.4 s, heard from where it is
/// fn footsteps(
///   mut audio: Audio,
///   time: Res<Time>,
///   mut walkers: Query<(&mut Footsteps, &Transform)>,
/// ) {
///   for (mut steps, transform) in &mut walkers {
///     steps.next -= time.delta();
///     if steps.next <= 0.0 {
///       steps.next += 0.4;
///       let voice = audio.play_at(&steps.sound, transform.translation);
///       // Applies with its start, so its first audio is at this level
///       voice.set_volume(-3.0);
///     }
///   }
/// }
/// ```
#[derive(SystemParam)]
pub struct Audio<'w, 's> {
  engine: Option<Res<'w, AudioEngine>>,
  queue: Deferred<'s, AudioQueue>,
}

impl Audio<'_, '_> {
  /// Play `sound` (`&handle`, of a [`Sound`] or an [`AudioClip`]) as
  /// [`AudioView::play`](super::AudioView::play) does, once the system's
  /// buffer applies; its [`Voice`] is returned now, reporting playing until
  /// then (and stopped from then if the play is refused)
  pub fn play<'a>(&mut self, sound: impl Into<Playable<'a>>) -> Voice {
    self.play_with(sound, PlayOptions::default())
  }

  /// Play `sound` at `position` in the world, heard from there by its
  /// sound's spatial settings, as
  /// [`AudioView::play_at`](super::AudioView::play_at) does (a one-shot at
  /// or beyond its maximum distance is culled as it applies)
  pub fn play_at<'a>(&mut self, sound: impl Into<Playable<'a>>, position: Vec2) -> Voice {
    let options = PlayOptions {
      position: Some(position),
      ..Default::default()
    };
    self.play_with(sound, options)
  }

  /// [`play`](Self::play) with [`PlayOptions`]
  pub fn play_with<'a>(&mut self, sound: impl Into<Playable<'a>>, options: PlayOptions) -> Voice {
    let Some(engine) = &self.engine else {
      return Voice::stopped();
    };
    let sound = match sound.into() {
      Playable::Sound(sound) => QueuedSound::Sound(sound.clone()),
      Playable::Clip(clip) => QueuedSound::Clip(clip.clone()),
    };
    let voice = engine.queued_voice();
    self.queue.push(Queued::Play {
      voice: voice.clone(),
      sound,
      options,
    });
    voice
  }

  /// Play the sound of `entity`'s [`AudioEmitter`](super::AudioEmitter)
  /// where the entity is when the buffer applies, its voice following the
  /// entity and belonging to it, as
  /// [`AudioView::play_emitter`](super::AudioView::play_emitter) does.
  ///
  /// An entity that is not an emitter yet as the buffer applies is played
  /// at the engine's next audio update, by when the system's commands have
  /// applied: bevy applies a system's buffers in the order of its
  /// parameters, so an emitter spawned with the system's own `Commands`
  /// declared after `Audio` does not exist yet. An emitter the system's own
  /// `Commands`, declared before `Audio`, took away or despawned (a barrel
  /// that plays its blast and despawns) plays where it last was, unfollowed:
  /// a one-shot plays out there, and a loop ends with its fade, as an
  /// emitter's loop does as its entity goes. One that is still no emitter
  /// by the next update (never spawned, despawned before the frame,
  /// disabled, or without an `AudioEmitter`) plays nothing: the voice stops,
  /// and the refusal is logged (and warned about once in debug builds)
  pub fn play_emitter(&mut self, entity: Entity) -> Voice {
    let Some(engine) = &self.engine else {
      return Voice::stopped();
    };
    let voice = engine.queued_voice();
    self.queue.push(Queued::Emitter {
      voice: voice.clone(),
      entity,
    });
    voice
  }

  /// Blend the mix to the mixer's snapshot `name`, as
  /// [`AudioView::snapshot`](super::AudioView::snapshot) does, once the
  /// buffer applies
  pub fn snapshot(&mut self, name: &str) {
    if self.engine.is_some() {
      self.queue.push(Queued::Snapshot(name.to_string()));
    }
  }

  /// Blend back from the active snapshot, once the buffer applies
  pub fn clear_snapshot(&mut self) {
    if self.engine.is_some() {
      self.queue.push(Queued::ClearSnapshot);
    }
  }

  /// Set `bus`'s volume in dB in place of the mixer's, as
  /// [`AudioView::set_bus_volume`](super::AudioView::set_bus_volume) does,
  /// once the buffer applies
  pub fn set_bus_volume(&mut self, bus: &str, db: f32) {
    if self.engine.is_some() {
      self.queue.push(Queued::BusVolume(bus.to_string(), db));
    }
  }

  /// Set the player's volume setting `kind`, from 0 (silent) to 1, as
  /// [`AudioView::set_volume_setting`](super::AudioView::set_volume_setting)
  /// does, once the buffer applies. [`volume_setting`](Self::volume_setting)
  /// reads it back at once
  pub fn set_volume_setting(&mut self, kind: VolumeKind, value: f32) {
    if self.engine.is_some() {
      self.queue.push(Queued::Setting(kind, value));
    }
  }

  /// The player's volume setting `kind`, from 0 to 1: the last this system
  /// set (clamped, NaN the default), else the engine's
  pub fn volume_setting(&self, kind: VolumeKind) -> f32 {
    match (&self.engine, self.queue.setting(kind)) {
      (_, Some(value)) => kind.sanitize(value),
      (Some(engine), None) => engine.volume_setting(kind),
      (None, None) => kind.default_value(),
    }
  }
}

/// What a play queued by [`Audio`] plays
pub(crate) enum QueuedSound {
  Sound(Handle<Sound>),
  Clip(Handle<AudioClip>),
}

/// One thing a system's [`Audio`] asked of the engine, in order
pub(crate) enum Queued {
  Play {
    voice: Voice,
    sound: QueuedSound,
    options: PlayOptions,
  },
  Emitter {
    voice: Voice,
    entity: Entity,
  },
  Snapshot(String),
  ClearSnapshot,
  BusVolume(String, f32),
  Setting(VolumeKind, f32),
}

impl Queued {
  /// The voice it plays, if it is a play
  fn voice(&self) -> Option<&Voice> {
    match self {
      Queued::Play { voice, .. } | Queued::Emitter { voice, .. } => Some(voice),
      _ => None,
    }
  }
}

/// A system's [`Audio`] queue: its system buffer, applied where the
/// system's commands apply. Public only as the parameter's state, which
/// names it; not exported
#[doc(hidden)]
#[derive(Default)]
pub struct AudioQueue(Vec<Queued>);

impl AudioQueue {
  fn push(&mut self, queued: Queued) {
    self.0.push(queued);
  }

  /// The last volume setting `kind` was queued at
  fn setting(&self, kind: VolumeKind) -> Option<f32> {
    self.0.iter().rev().find_map(|queued| match *queued {
      Queued::Setting(queued, value) if queued == kind => Some(value),
      _ => None,
    })
  }
}

/// A queue dropped before it applied (its system gone from its schedule)
/// played nothing: its voices stop
impl Drop for AudioQueue {
  fn drop(&mut self) {
    for queued in self.0.drain(..) {
      if let Some(voice) = queued.voice() {
        voice.set_state(VoiceState::Stopped);
      }
    }
  }
}

impl SystemBuffer for AudioQueue {
  fn apply(&mut self, _system: &SystemMeta, world: &mut World) {
    run(&mut self.0, world);
  }

  /// Where the system runs without the world to itself (an observer), its
  /// queue applies with the world's commands
  fn queue(&mut self, _system: &SystemMeta, mut world: DeferredWorld) {
    if self.0.is_empty() {
      return;
    }
    let mut queued = std::mem::take(&mut self.0);
    world
      .commands()
      .queue(move |world: &mut World| run(&mut queued, world));
  }
}

/// Apply `queued` to `world`'s audio engine, emptying it (its room kept); a
/// world whose engine has gone stops the voices it played
fn run(queued: &mut Vec<Queued>, world: &mut World) {
  if queued.is_empty() {
    return;
  }
  // The engine is taken out of the world while it reads the stores
  let ran = world.try_resource_scope(|world: &mut World, mut engine: Mut<'_, AudioEngine>| {
    engine.prepare(world);
    engine.run_queued(world, queued);
  });
  if ran.is_none() {
    for queued in queued.drain(..) {
      if let Some(voice) = queued.voice() {
        voice.set_state(VoiceState::Stopped);
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use std::time::Duration;

  use super::Audio;
  use crate::core::asset::{Assets, Handle};
  use crate::core::audio::{
    AudioClip, AudioEmitter, AudioEngine, AudioLogEntry, AudioLogKind, AudioView, LimitRule, Mixer,
    SnapshotBus, SnapshotDef, Sound, SoundClip, Voice, VoiceLimit, VolumeKind, in_scene,
    release_scene,
  };
  use crate::core::ecs::kernel::{
    IntoScheduleConfigs, LogLevel, Res, ResMut, Resource, Schedule, ScheduleBuildSettings,
    SingleThreadedExecutor,
  };
  use crate::core::ecs::{Entity, Transform, World};
  use crate::core::testing::{audio_step, audio_world};

  const FRAME: Duration = Duration::from_nanos(16_666_667);

  /// What the test's systems play
  #[derive(Resource, Clone)]
  struct Sounds {
    shot: Handle<AudioClip>,
    other: Handle<AudioClip>,
    bed: Handle<Sound>,
  }

  /// The voices the test's systems played, in order
  #[derive(Resource, Default)]
  struct Played(Vec<Voice>);

  fn clip(world: &mut World, seconds: f32) -> Handle<AudioClip> {
    let frames = vec![[0.1, 0.1]; (48_000.0 * seconds) as usize];
    world
      .resource_mut::<Assets<AudioClip>>()
      .add(AudioClip::from_frames(48_000, &frames).unwrap())
  }

  /// An audio world with the test's sounds and nothing played
  fn world() -> World {
    let mut world = audio_world();
    let (shot, other, looped) = (
      clip(&mut world, 1.0),
      clip(&mut world, 1.0),
      clip(&mut world, 2.0),
    );
    let bed = world.resource_mut::<Assets<Sound>>().add(Sound {
      clips: vec![SoundClip::new(looped)],
      looping: true,
      ..Default::default()
    });
    world.insert_resource(Sounds { shot, other, bed });
    world.init_resource::<Played>();
    world
  }

  /// A schedule run as the engine runs its own, on one thread
  fn schedule() -> Schedule {
    let mut schedule = Schedule::default();
    schedule.set_executor(SingleThreadedExecutor::new());
    schedule
  }

  fn played(world: &World) -> Vec<Voice> {
    world.resource::<Played>().0.clone()
  }

  fn log(world: &World) -> Vec<AudioLogEntry> {
    world.resource::<AudioEngine>().log().to_vec()
  }

  /// `voice`'s log entries, but its start
  fn changes(world: &World, voice: &Voice) -> Vec<(AudioLogKind, f32, Duration)> {
    log(world)
      .iter()
      .filter(|entry| entry.voice == voice.id() && entry.kind != AudioLogKind::Start)
      .map(|entry| (entry.kind, entry.gain_db, entry.fade))
      .collect()
  }

  fn starts(world: &World, voice: &Voice) -> usize {
    log(world)
      .iter()
      .filter(|entry| entry.voice == voice.id() && entry.kind == AudioLogKind::Start)
      .count()
  }

  /// A system plays through `Audio`: its voice reports playing at once,
  /// waiting for the system's buffer to apply, and is live once the
  /// schedule has run. Played outside every scene, it is the engine's
  #[test]
  fn a_system_plays_through_audio_and_its_voice_is_live_after_the_schedule() {
    let mut world = world();
    let mut schedule = schedule();
    schedule.add_systems(
      |mut audio: Audio, sounds: Res<Sounds>, mut played: ResMut<Played>| {
        let voice = audio.play(&sounds.shot);
        assert!(voice.is_playing(), "it reports playing until it applies");
        played.0.push(voice);
      },
    );
    schedule.run(&mut world);
    let [voice] = &played(&world)[..] else {
      panic!("one voice played");
    };
    assert!(voice.is_playing(), "{voice:?}");
    assert_eq!(starts(&world, voice), 1);
    assert_eq!(AudioView::new(&mut world).voices_on("sfx"), 1);
    audio_step(&mut world, FRAME);
    assert!(voice.is_playing(), "{voice:?}");
  }

  /// A play applied in a scene's section is the scene's: a loop a scene's
  /// system plays stops as the scene exits
  #[test]
  fn a_play_applied_in_a_scene_s_section_belongs_to_the_scene() {
    let mut world = world();
    let mut schedule = schedule();
    schedule.add_systems(
      |mut audio: Audio, sounds: Res<Sounds>, mut played: ResMut<Played>| {
        played.0.push(audio.play(&sounds.bed));
      },
    );
    in_scene(&mut world, 7, |world| schedule.run(world));
    audio_step(&mut world, FRAME);
    let [bed] = &played(&world)[..] else {
      panic!("one voice played");
    };
    assert!(bed.is_playing(), "{bed:?}");
    release_scene(&mut world, 7);
    let stops: Vec<u32> = log(&world)
      .iter()
      .filter(|entry| entry.kind == AudioLogKind::Stop)
      .map(|entry| entry.voice)
      .collect();
    assert_eq!(stops, [bed.id()], "the scene's exit stops its loop");
  }

  /// A play refused as it applies (a `PreventNew` limit full) leaves its
  /// voice stopped once the schedule has run
  #[test]
  fn a_refused_play_s_voice_reports_stopped_after_the_apply() {
    let mut world = world();
    let one = clip(&mut world, 1.0);
    let limited = world.resource_mut::<Assets<Sound>>().add(Sound {
      clips: vec![SoundClip::new(one)],
      limit: Some(VoiceLimit {
        max: 1,
        rule: LimitRule::PreventNew,
      }),
      per_frame: 2,
      ..Default::default()
    });
    let first = AudioView::new(&mut world).play(&limited);
    assert!(first.is_playing());
    #[derive(Resource)]
    struct Limited(Handle<Sound>);
    world.insert_resource(Limited(limited));
    let mut schedule = schedule();
    schedule.add_systems(
      |mut audio: Audio, limited: Res<Limited>, mut played: ResMut<Played>| {
        let voice = audio.play(&limited.0);
        assert!(voice.is_playing(), "not refused until it applies");
        played.0.push(voice);
      },
    );
    schedule.run(&mut world);
    let [refused] = &played(&world)[..] else {
      panic!("one voice played");
    };
    assert!(refused.is_stopped(), "{refused:?}");
    assert_ne!(refused.id(), 0, "it had an id of its own");
    let reasons: Vec<&str> = log(&world)
      .iter()
      .filter(|entry| entry.kind == AudioLogKind::Refused)
      .map(|entry| entry.reason)
      .collect();
    assert!(
      matches!(reasons[..], [reason] if reason.starts_with("limit")),
      "{reasons:?}"
    );
    assert!(first.is_playing());
  }

  /// The system that plays a voice and changes it
  fn play_quieter(mut audio: Audio, sounds: Res<Sounds>, mut played: ResMut<Played>) {
    let voice = audio.play(&sounds.shot);
    voice.set_volume(-20.0);
    played.0.push(voice);
  }

  /// A change the system makes to the voice it played applies with its
  /// start, as one made in the frame a voice starts does: at once, with no
  /// ramp, so its first audio is at the level set
  #[test]
  fn a_change_made_in_the_playing_system_applies_with_the_start() {
    let mut world = world();
    let mut schedule = schedule();
    schedule.add_systems(play_quieter);
    schedule.run(&mut world);
    let [voice] = &played(&world)[..] else {
      panic!("one voice played");
    };
    assert_eq!(
      changes(&world, voice),
      [(AudioLogKind::Gain, -20.0, Duration::ZERO)]
    );
  }

  /// A play made at once between the system's play and its buffer's apply
  /// applies the voices' queued changes first: the change waiting for the
  /// system's voice is kept until it starts, and the two voices' ids differ
  #[test]
  fn a_change_drained_before_its_voice_starts_waits_for_the_start() {
    let mut world = world();
    let mut schedule = schedule();
    let direct = |world: &mut World| {
      let other = world.resource::<Sounds>().other.clone();
      let voice = AudioView::new(world).play(&other);
      world.resource_mut::<Played>().0.push(voice);
    };
    // The play at once runs before the first system's buffer applies (as
    // an exclusive system after it unordered would): the buffers apply at
    // the schedule's end
    schedule.set_build_settings(ScheduleBuildSettings {
      auto_insert_apply_deferred: false,
      ..Default::default()
    });
    schedule.add_systems((play_quieter, direct).chain());
    schedule.run(&mut world);
    let [deferred, direct] = &played(&world)[..] else {
      panic!("two voices played");
    };
    assert_ne!(deferred.id(), direct.id());
    assert!(deferred.is_playing() && direct.is_playing());
    assert_eq!(
      changes(&world, deferred),
      [(AudioLogKind::Gain, -20.0, Duration::ZERO)]
    );
    assert_eq!(changes(&world, direct), []);
    let order: Vec<u32> = log(&world)
      .iter()
      .filter(|entry| entry.kind == AudioLogKind::Start)
      .map(|entry| entry.voice)
      .collect();
    assert_eq!(order, [direct.id(), deferred.id()], "deferred to the apply");
  }

  /// `Audio` reads the engine and changes it only through its buffer, so
  /// two systems that play sound run in one schedule without conflicting
  #[test]
  fn two_systems_with_audio_do_not_conflict() {
    let mut world = world();
    let mut schedule = schedule();
    schedule.set_build_settings(ScheduleBuildSettings {
      ambiguity_detection: LogLevel::Error,
      ..Default::default()
    });
    let one = |mut audio: Audio, sounds: Res<Sounds>| {
      audio.play(&sounds.shot);
    };
    let other = |mut audio: Audio, sounds: Res<Sounds>| {
      audio.play(&sounds.other);
    };
    schedule.add_systems((one, other));
    if let Err(error) = schedule.initialize(&mut world) {
      panic!("the systems conflict: {error}");
    }
    schedule.run(&mut world);
    assert_eq!(AudioView::new(&mut world).voices_on("sfx"), 2);
  }

  /// `play_emitter` plays the entity's emitter as it applies, its voice
  /// the entity's
  #[test]
  fn a_system_plays_an_emitter() {
    let mut world = world();
    let bed = world.resource::<Sounds>().bed.clone();
    let entity = world
      .spawn((
        AudioEmitter {
          sound: bed,
          play_on_spawn: false,
        },
        Transform::from_xy(10.0, 0.0),
      ))
      .id();
    #[derive(Resource)]
    struct Target(Entity);
    world.insert_resource(Target(entity));
    let mut schedule = schedule();
    schedule.add_systems(
      |mut audio: Audio, target: Res<Target>, mut played: ResMut<Played>| {
        played.0.push(audio.play_emitter(target.0));
      },
    );
    schedule.run(&mut world);
    let [voice] = &played(&world)[..] else {
      panic!("one voice played");
    };
    assert!(voice.is_playing(), "{voice:?}");
    assert_eq!(starts(&world, voice), 1);
    world.despawn(entity);
    audio_step(&mut world, FRAME);
    audio_step(&mut world, FRAME);
    assert!(!voice.is_playing(), "its entity went: {voice:?}");
  }

  /// An emitter a system spawns with its `Commands` and plays through its
  /// `Audio` is heard whichever parameter comes first: bevy applies a
  /// system's buffers in their order, so with `Audio` first its play
  /// applies before the entity exists, and waits for the next update
  #[test]
  fn an_emitter_spawned_and_played_by_one_system_is_heard() {
    use crate::core::ecs::Commands;
    let mut world = world();
    let mut schedule = schedule();
    schedule.add_systems(
      |mut audio: Audio,
       mut commands: Commands,
       sounds: Res<Sounds>,
       mut played: ResMut<Played>| {
        let entity = commands
          .spawn((
            AudioEmitter {
              sound: sounds.bed.clone(),
              play_on_spawn: false,
            },
            Transform::from_xy(10.0, 0.0),
          ))
          .id();
        let voice = audio.play_emitter(entity);
        voice.set_volume(-6.0);
        played.0.push(voice);
      },
    );
    schedule.run(&mut world);
    let [voice] = &played(&world)[..] else {
      panic!("one voice played");
    };
    assert!(voice.is_playing(), "it waits for the update: {voice:?}");
    audio_step(&mut world, FRAME);
    assert!(voice.is_playing(), "{voice:?}");
    assert_eq!(starts(&world, voice), 1);
    assert_eq!(
      changes(&world, voice),
      [(AudioLogKind::Gain, -6.0, Duration::ZERO)],
      "its change applies with its start"
    );
  }

  /// A system that plays an emitter and despawns its entity hears it
  /// whichever parameter comes first: with `Commands` first the despawn
  /// applies before the play, which plays where the emitter last was,
  /// unfollowed. A one-shot plays out there; a loop ends with its fade, as
  /// an emitter's loop does when its entity goes
  #[test]
  fn an_emitter_played_then_despawned_by_one_system_is_heard() {
    use crate::core::audio::engine::MIN_FADE;
    use crate::core::ecs::{Commands, Component, Query, With};
    #[derive(Component)]
    struct Exploding;
    let mut world = world();
    let shot = world.resource::<Sounds>().shot.clone();
    let bed = world.resource::<Sounds>().bed.clone();
    let boom = world.resource_mut::<Assets<Sound>>().add(Sound {
      clips: vec![SoundClip::new(shot)],
      fade_out: Duration::from_millis(40),
      ..Default::default()
    });
    for (sound, x) in [(boom, 30.0), (bed, 60.0)] {
      let emitter = AudioEmitter {
        sound,
        play_on_spawn: false,
      };
      world.spawn((emitter, Transform::from_xy(x, 0.0), Exploding));
    }
    let mut schedule = schedule();
    schedule.add_systems(
      |mut commands: Commands,
       mut audio: Audio,
       barrels: Query<Entity, With<Exploding>>,
       mut played: ResMut<Played>| {
        for barrel in &barrels {
          played.0.push(audio.play_emitter(barrel));
          commands.entity(barrel).despawn();
        }
      },
    );
    schedule.run(&mut world);
    let voices = played(&world);
    let [boom, bed] = &voices[..] else {
      panic!("two voices played: {voices:?}");
    };
    assert_eq!((starts(&world, boom), starts(&world, bed)), (1, 1));
    audio_step(&mut world, FRAME);
    assert!(boom.is_playing(), "the one-shot plays out: {boom:?}");
    let stopped: Vec<(u32, Duration)> = log(&world)
      .iter()
      .filter(|entry| entry.kind == AudioLogKind::Stop)
      .map(|entry| (entry.voice, entry.fade))
      .collect();
    assert_eq!(stopped, [(bed.id(), MIN_FADE)], "the loop ends");
  }

  /// A system's emitter play whose entity is still no emitter by the
  /// next update (never spawned) is refused there and logged
  #[test]
  fn an_emitter_play_with_no_emitter_by_the_next_update_is_refused() {
    let mut world = world();
    let gone = world.spawn_empty().id();
    world.despawn(gone);
    #[derive(Resource)]
    struct Target(Entity);
    world.insert_resource(Target(gone));
    let mut schedule = schedule();
    schedule.add_systems(
      |mut audio: Audio, target: Res<Target>, mut played: ResMut<Played>| {
        played.0.push(audio.play_emitter(target.0));
      },
    );
    schedule.run(&mut world);
    let [voice] = &played(&world)[..] else {
      panic!("one voice played");
    };
    assert!(voice.is_playing(), "it waits for the update: {voice:?}");
    audio_step(&mut world, FRAME);
    assert!(voice.is_stopped(), "{voice:?}");
    let refused: Vec<&str> = log(&world)
      .iter()
      .filter(|entry| entry.kind == AudioLogKind::Refused)
      .map(|entry| entry.reason)
      .collect();
    let [reason] = refused[..] else {
      panic!("one refusal: {refused:?}");
    };
    assert!(reason.starts_with("emitter:"), "{reason}");
  }

  /// The mix through `Audio`: a volume setting reads back at once, and the
  /// setting, a bus volume and a snapshot reach the engine as the buffer
  /// applies
  #[test]
  fn a_system_sets_the_mix() {
    let mut world = world();
    AudioView::new(&mut world).set_mixer(Mixer {
      snapshots: vec![SnapshotDef {
        name: "hushed".into(),
        blend: 0.0,
        buses: vec![(
          "music".into(),
          SnapshotBus {
            volume_db: -12.0,
            low_pass: None,
          },
        )],
      }],
      ..Default::default()
    });
    let mut schedule = schedule();
    schedule.add_systems(|mut audio: Audio| {
      audio.set_volume_setting(VolumeKind::Music, 0.5);
      assert_eq!(audio.volume_setting(VolumeKind::Music), 0.5);
      audio.set_volume_setting(VolumeKind::Sfx, 7.0);
      assert_eq!(audio.volume_setting(VolumeKind::Sfx), 1.0, "clamped");
      audio.set_bus_volume("sfx", -6.0);
      audio.snapshot("hushed");
    });
    schedule.run(&mut world);
    assert_eq!(
      AudioView::new(&mut world).volume_setting(VolumeKind::Music),
      0.5
    );
    let buses = world.resource::<AudioEngine>().buses();
    let bus = |name: &str| buses.iter().find(|bus| bus.name == name).cloned();
    assert_eq!(bus("sfx").map(|bus| bus.base_db), Some(-6.0));
    assert_eq!(bus("music").map(|bus| bus.snapshot_db), Some(-12.0));
    let mut clear = self::schedule();
    clear.add_systems(|mut audio: Audio| audio.clear_snapshot());
    clear.run(&mut world);
    let buses = world.resource::<AudioEngine>().buses();
    let music = buses.iter().find(|bus| bus.name == "music");
    assert_eq!(music.map(|bus| bus.snapshot_db), Some(0.0));
  }

  /// A play whose buffer never applies (its schedule dropped first) plays
  /// nothing, and its voice stops as the buffer goes
  #[test]
  fn a_play_whose_buffer_never_applies_stops() {
    let mut world = world();
    let mut schedule = schedule();
    schedule.set_apply_final_deferred(false);
    schedule.add_systems(
      |mut audio: Audio, sounds: Res<Sounds>, mut played: ResMut<Played>| {
        played.0.push(audio.play(&sounds.shot));
      },
    );
    schedule.run(&mut world);
    let [voice] = &played(&world)[..] else {
      panic!("one voice played");
    };
    assert!(voice.is_playing(), "waiting for its buffer: {voice:?}");
    drop(schedule);
    assert!(voice.is_stopped(), "{voice:?}");
    assert_eq!(starts(&world, voice), 0);
  }

  /// In a world without an audio engine every play returns a stopped
  /// voice, as `AudioView`'s do
  #[test]
  fn a_world_without_an_engine_plays_nothing() {
    let mut world = crate::core::testing::asset_world();
    let clip = clip(&mut world, 0.1);
    #[derive(Resource)]
    struct Clip(Handle<AudioClip>);
    world.insert_resource(Clip(clip));
    world.init_resource::<Played>();
    let mut schedule = schedule();
    schedule.add_systems(
      |mut audio: Audio, clip: Res<Clip>, mut played: ResMut<Played>| {
        played.0.push(audio.play(&clip.0));
        assert_eq!(audio.volume_setting(VolumeKind::Ui), 1.0);
      },
    );
    schedule.run(&mut world);
    assert!(played(&world).iter().all(Voice::is_stopped));
  }
}
