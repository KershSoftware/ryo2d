//! Which scene a skeleton belongs to, and the scenes stopped under another:
//! a skeleton animates while its scene updates, so a scene stopped under
//! another has its skeletons frozen with it, as its particles, physics and
//! voices are. A scene transition doesn't stop them.

use super::component::Skeleton;
use crate::core::ecs::{Allow, Disabled, Resource, World};

/// What the world keeps of its skeletons as a whole: the scene whose code
/// runs, which a skeleton added now belongs to, and the scenes held
#[derive(Resource, Debug, Default)]
pub(crate) struct SkeletonStore {
  /// The scene whose code runs now; `None` is the engine's own code
  scope: Option<u64>,
  /// The scenes a scene above stops
  held: Vec<u64>,
}

impl SkeletonStore {
  /// The scene whose code runs now
  pub(crate) fn scope(&self) -> Option<u64> {
    self.scope
  }

  /// Whether a skeleton of `scope` is held: its scene's skeletons while the
  /// scene is, and a skeleton of no scene while any scene is
  pub(crate) fn holds(&self, scope: Option<u64>) -> bool {
    match scope {
      Some(scope) => self.held.contains(&scope),
      None => !self.held.is_empty(),
    }
  }
}

/// Run `f` as scene `scope`'s code: the skeletons it adds belong to that
/// scene, and are held with it. A world without a skeleton store just runs
/// `f`
pub(crate) fn in_scene<R>(world: &mut World, scope: u64, f: impl FnOnce(&mut World) -> R) -> R {
  let previous = world
    .get_resource_mut::<SkeletonStore>()
    .map(|mut store| store.scope.replace(scope));
  let result = f(world);
  if let Some(previous) = previous
    && let Some(mut store) = world.get_resource_mut::<SkeletonStore>()
  {
    store.scope = previous;
  }
  result
}

/// The scenes `scopes` are stopped by a scene above: the skeletons they
/// added are frozen (they neither advance nor send anything, and their
/// sockets stay where they are), and so are skeletons of no scene while
/// any is held, until they update again. Called as the stack changes and
/// after each step; the store is touched only when the scenes held change
pub(crate) fn hold_scenes(world: &mut World, scopes: impl Iterator<Item = u64> + Clone) {
  let changed = world
    .get_resource::<SkeletonStore>()
    .is_some_and(|store| !store.held.iter().copied().eq(scopes.clone()));
  if changed && let Some(mut store) = world.get_resource_mut::<SkeletonStore>() {
    store.held.clear();
    store.held.extend(scopes);
  }
}

/// Scene `scope` exited: the skeletons it added that outlive it belong to
/// no scene now, held while any scene is
pub(crate) fn release_scene(world: &mut World, scope: u64) {
  if !world.contains_resource::<SkeletonStore>() {
    return;
  }
  let mut skeletons = world.query_filtered::<&mut Skeleton, Allow<Disabled>>();
  for mut skeleton in skeletons.iter_mut(world) {
    if skeleton.scope() == Some(scope) {
      skeleton.set_scope(None);
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::asset::Assets;
  use crate::core::skeleton::SkeletonData;
  use crate::testing::asset_world;

  fn skeleton(world: &mut World) -> Skeleton {
    let data = SkeletonData::builder().build().expect("an empty skeleton");
    Skeleton::new(world.resource_mut::<Assets<SkeletonData>>().add(data))
  }

  #[test]
  fn a_skeleton_records_the_scene_it_was_added_in() {
    let mut world = asset_world();
    world.insert_resource(SkeletonStore::default());
    let made = skeleton(&mut world);
    let outside = world.spawn(made.clone()).id();
    let inside = in_scene(&mut world, 4, |world| world.spawn(made.clone()).id());
    // The scope is the scene's only while its code runs
    let after = world.spawn(made.clone()).id();
    let scope = |world: &World, entity| world.get::<Skeleton>(entity).expect("a skeleton").scope();
    assert_eq!(scope(&world, outside), None);
    assert_eq!(scope(&world, inside), Some(4));
    assert_eq!(scope(&world, after), None);
    // One put on an entity by another scene's code is that scene's
    in_scene(&mut world, 7, |world| {
      world.entity_mut(inside).insert(made.clone());
    });
    assert_eq!(scope(&world, inside), Some(7));
    // A scene that exits leaves its skeletons to no scene, disabled ones
    // too
    world.entity_mut(inside).insert(Disabled);
    release_scene(&mut world, 4);
    assert_eq!(scope(&world, inside), Some(7));
    release_scene(&mut world, 7);
    assert_eq!(scope(&world, inside), None);
  }

  #[test]
  fn held_scenes_hold_their_skeletons_and_those_of_no_scene() {
    let mut world = World::new();
    world.insert_resource(SkeletonStore::default());
    let holds = |world: &World, scope| world.resource::<SkeletonStore>().holds(scope);
    assert!(!holds(&world, None) && !holds(&world, Some(1)));
    hold_scenes(&mut world, [1].into_iter());
    assert!(holds(&world, Some(1)));
    assert!(!holds(&world, Some(2)));
    assert!(holds(&world, None), "no scene's: held while any is");
    hold_scenes(&mut world, std::iter::empty());
    assert!(!holds(&world, None) && !holds(&world, Some(1)));
  }
}
