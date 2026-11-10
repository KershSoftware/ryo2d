//! Which scene an emitter belongs to, and the scenes stopped under another:
//! a game-time emitter steps while its scene updates, so a scene stopped
//! under another has its particles frozen with it. A scene transition
//! doesn't stop them.

use super::store::ParticleStore;
use crate::core::ecs::World;

/// Run `f` as scene `scope`'s code: the emitters it adds belong to that
/// scene, and are held with it. A world without a particle store just
/// runs `f`
pub(crate) fn in_scene<R>(world: &mut World, scope: u64, f: impl FnOnce(&mut World) -> R) -> R {
  let previous = world
    .get_resource_mut::<ParticleStore>()
    .map(|mut store| store.set_scope(Some(scope)));
  let result = f(world);
  if let Some(previous) = previous
    && let Some(mut store) = world.get_resource_mut::<ParticleStore>()
  {
    store.set_scope(previous);
  }
  result
}

/// The scenes `scopes` are stopped by a scene above: the emitters they
/// added are frozen, and so are emitters of no scene while any is held,
/// until they update again.
/// Called as the stack changes and after each step; the store is touched
/// only when the scenes held change
pub(crate) fn hold_scenes(world: &mut World, scopes: impl Iterator<Item = u64> + Clone) {
  let changed = world
    .get_resource::<ParticleStore>()
    .is_some_and(|store| !store.holds_exactly(scopes.clone()));
  if changed && let Some(mut store) = world.get_resource_mut::<ParticleStore>() {
    store.hold_scopes(scopes);
  }
}

/// Scene `scope` exited: the emitters it added that outlive it belong to no
/// scene now
pub(crate) fn release_scene(world: &mut World, scope: u64) {
  if let Some(mut store) = world.get_resource_mut::<ParticleStore>() {
    store.release_scope(scope);
  }
}
