//! Event sounds the engine plays: a [`SkeletonSounds`] on a skeleton's
//! entity has each event that names a sound play it as it fires. The
//! module notes give the rules.

use std::sync::Arc;

use super::component::Skeleton;
use super::data::SkeletonData;
use super::event::SkeletonEvent;
use crate::core::asset::{AssetEvent, AssetId, AssetServer, Assets, Handle};
use crate::core::audio::{AudioClip, AudioEngine, PlayOptions, Playable, Sound};
use crate::core::ecs::kernel::EcsSystemState;
use crate::core::ecs::{
  Component, Entity, GlobalTransform, Local, MessageReader, Mut, Query, Res, World,
};
use crate::core::hash::FastMap;

/// Where a skeleton's event sounds play
#[derive(Debug, Clone, PartialEq)]
enum SoundPlace {
  /// At the skeleton's entity, heard from there
  Skeleton,
  /// Nowhere in the world: not spatial
  Flat,
  /// At the named bone, heard from there
  Bone(Arc<str>),
}

/// Has the engine play the sounds its skeleton's events name, each once as
/// its event fires, at the event's volume (its linear volume, floored at
/// 0.001, in dB) and balance. A path ending `.sound.ron` is a sound
/// definition, played as it says; any other is a clip, played directly.
///
/// The sounds are loaded when the skeleton's data lands, held while it
/// lives, and loaded again when it reloads with other paths; a skeleton
/// without this component loads none. Loading is asynchronous: an event
/// whose sound is still loading plays nothing. A sound plays in the
/// skeleton's scene, so a long one stops with the scene, and a skeleton
/// whose scene is held fires no events. Where it plays:
///
/// - [`new`](Self::new): at the skeleton's place in the world, heard from
///   there as its sound's spatial settings say;
/// - [`flat`](Self::flat): not in the world;
/// - [`at_bone`](Self::at_bone): at a bone's place, heard from there.
///
/// Balance pans a flat play, and a play at a place whose sound has no
/// spatial settings (heard as if at the listener); a sound heard from its
/// place takes its pan from there. [`SkeletonEvent`] messages are still
/// sent.
///
/// ```
/// use ryo2d::prelude::*;
///
/// fn spawn_hero(ctx: &mut SceneContext, data: Handle<SkeletonData>) -> Entity {
///   ctx
///     .world
///     .spawn((
///       Transform::default(),
///       Skeleton::new(data),
///       SkeletonAnimation::new(),
///       // Footsteps where the foot lands
///       SkeletonSounds::at_bone("sole"),
///       Renderable::skeleton(),
///     ))
///     .id()
/// }
/// ```
#[derive(Component, Debug, Clone, PartialEq)]
pub struct SkeletonSounds {
  place: SoundPlace,
}

impl SkeletonSounds {
  /// Its sounds play at the skeleton's place in the world, heard from
  /// there as each sound's spatial settings say
  pub fn new() -> Self {
    Self {
      place: SoundPlace::Skeleton,
    }
  }

  /// Its sounds play not in the world, panned by their events' balance
  pub fn flat() -> Self {
    Self {
      place: SoundPlace::Flat,
    }
  }

  /// Its sounds play at the bone `name`'s place in the world, heard from
  /// there. A name the skeleton lacks plays them at the skeleton's place
  pub fn at_bone(name: impl Into<Arc<str>>) -> Self {
    Self {
      place: SoundPlace::Bone(name.into()),
    }
  }
}

impl Default for SkeletonSounds {
  fn default() -> Self {
    Self::new()
  }
}

/// An event's sound, as loaded
#[derive(Clone)]
enum Loaded {
  Sound(Handle<Sound>),
  Clip(Handle<AudioClip>),
}

impl Loaded {
  /// The sound at `path`: a definition if it ends `.sound.ron`, else a clip
  fn load(server: &AssetServer, path: &str) -> Self {
    if path.ends_with(".sound.ron") {
      Loaded::Sound(server.load(path))
    } else {
      Loaded::Clip(server.load(path))
    }
  }

  /// It is in its store: a play of it can start
  fn is_ready(&self, sounds: Option<&Assets<Sound>>, clips: Option<&Assets<AudioClip>>) -> bool {
    match self {
      Loaded::Sound(sound) => sounds.is_some_and(|sounds| sounds.get(sound).is_some()),
      Loaded::Clip(clip) => clips.is_some_and(|clips| clips.get(clip).is_some()),
    }
  }

  fn playable(&self) -> Playable<'_> {
    match self {
      Loaded::Sound(sound) => Playable::Sound(sound),
      Loaded::Clip(clip) => Playable::Clip(clip),
    }
  }
}

/// The sounds one skeleton data's events name, by path, as its version
/// `version` named them
struct DataSounds {
  version: u64,
  sounds: FastMap<Arc<str>, Loaded>,
}

/// One play the step found, made once the world is free
struct Play {
  sound: Loaded,
  options: PlayOptions,
  /// The skeleton's scene, whose voice it is
  scope: Option<u64>,
}

type Params = (
  MessageReader<'static, 'static, SkeletonEvent>,
  MessageReader<'static, 'static, AssetEvent<SkeletonData>>,
  Option<Res<'static, Assets<SkeletonData>>>,
  Option<Res<'static, AssetServer>>,
  Option<Res<'static, Assets<Sound>>>,
  Option<Res<'static, Assets<AudioClip>>>,
  Query<
    'static,
    'static,
    (
      Entity,
      &'static Skeleton,
      &'static SkeletonSounds,
      &'static GlobalTransform,
    ),
  >,
);

/// What the sounds' system keeps between runs: its parameters, the store
/// of each skeleton data's sounds, and the plays a step found (its room
/// kept, so a steady step allocates nothing)
#[derive(Default)]
pub(crate) struct SoundsState {
  params: Option<EcsSystemState<Params>>,
  store: FastMap<AssetId<SkeletonData>, DataSounds>,
  plays: Vec<Play>,
}

/// After the skeletons' step (`FixedSet::Skeletons`): the sounds of every
/// skeleton data a [`SkeletonSounds`] skeleton shows are loaded as the
/// data lands or reloads and freed with it, and each `SkeletonEvent` of
/// such a skeleton that names a sound plays it, in the skeleton's scene.
/// Without a `SkeletonSounds` in the world it only moves its readers on
pub(crate) fn play_skeleton_sounds(world: &mut World, mut state: Local<SoundsState>) {
  let SoundsState {
    params,
    store,
    plays,
  } = &mut *state;
  let params = params.get_or_insert_with(|| EcsSystemState::new(world));
  {
    let Ok((mut events, mut data_events, data, server, sounds, clips, skeletons)) =
      params.get_mut(world)
    else {
      return;
    };
    for event in data_events.read() {
      if let AssetEvent::Removed { id } = event {
        store.remove(id);
      }
    }
    if skeletons.is_empty() {
      events.clear();
      return;
    }
    let (Some(data), Some(server)) = (data, server) else {
      events.clear();
      return;
    };
    // The sounds of data that landed or reloaded since its last look
    for (_, skeleton, ..) in &skeletons {
      let id = skeleton.data().id();
      let Some(version) = data.version(id) else {
        continue;
      };
      if store.get(&id).is_some_and(|held| held.version == version) {
        continue;
      }
      let Some(rig) = data.get(id) else {
        continue;
      };
      // The new handles are taken before the old go, so a path kept
      // through a reload is not freed and read again
      let mut loaded = FastMap::default();
      for event in rig.events() {
        if let Some(audio) = &event.audio {
          loaded
            .entry(audio.path.clone())
            .or_insert_with(|| Loaded::load(&server, &audio.path));
        }
      }
      store.insert(
        id,
        DataSounds {
          version,
          sounds: loaded,
        },
      );
    }
    for event in events.read() {
      let Some(audio) = &event.audio else {
        continue;
      };
      let Ok((_, skeleton, place, global)) = skeletons.get(event.entity) else {
        continue;
      };
      let Some(sound) = store
        .get(&skeleton.data().id())
        .and_then(|held| held.sounds.get(&*audio.path))
      else {
        continue;
      };
      if !sound.is_ready(sounds.as_deref(), clips.as_deref()) {
        log::debug!(
          "{}: a skeleton event's sound is still loading, so it plays nothing",
          audio.path
        );
        continue;
      }
      let position = match &place.place {
        SoundPlace::Flat => None,
        SoundPlace::Skeleton => Some(global.translation()),
        SoundPlace::Bone(bone) => Some(
          skeleton
            .bone_world(bone)
            .map_or(global.translation(), |world| world.translation),
        ),
      };
      plays.push(Play {
        sound: sound.clone(),
        options: PlayOptions {
          position: position.filter(|at| at.is_finite()),
          volume_db: volume_db(audio.volume),
          pan: if audio.balance.is_finite() {
            audio.balance
          } else {
            0.0
          },
          ..Default::default()
        },
        scope: skeleton.scope(),
      });
    }
  }
  if plays.is_empty() {
    return;
  }
  world.try_resource_scope(|world: &mut World, mut engine: Mut<'_, AudioEngine>| {
    engine.prepare(world);
    for play in plays.iter() {
      let previous = engine.set_scope(play.scope);
      engine.play(world, play.sound.playable(), &play.options);
      engine.set_scope(previous);
    }
  });
  plays.clear();
}

/// An event's linear volume in the audio system's dB: floored at 0.001
/// (-60 dB, silence); one that is not a number is full volume
fn volume_db(volume: f32) -> f32 {
  if volume.is_finite() {
    20.0 * volume.max(0.001).log10()
  } else {
    0.0
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn an_event_s_volume_is_its_level_in_db() {
    for (volume, db) in [
      (1.0, 0.0),
      (0.5, -6.0206),
      (0.1, -20.0),
      (0.001, -60.0),
      (0.0, -60.0),
      (-1.0, -60.0),
      (2.0, 6.0206),
      (f32::NAN, 0.0),
      (f32::INFINITY, 0.0),
    ] {
      assert!(
        (volume_db(volume) - db).abs() < 1e-3,
        "{volume}: {}",
        volume_db(volume)
      );
    }
  }
}
