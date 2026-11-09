//! The particle store: every emitter's pool, the live count against the
//! budget, the compiled effects, and the scenes' holds.

use super::effect::ParticleEffect;
use super::pool::EmitterPool;
use super::rng::ParticleRng;
use super::step::Compiled;
use crate::core::asset::AssetId;
use crate::core::ecs::{Entity, Resource};
use crate::core::hash::{FastMap, FastSet};

/// The most particles alive at once, over every emitter, unless the game
/// sets its own with `EngineBuilder::particle_budget`. A safety cap, not a
/// quality setting: a spawn past it is dropped (with one warning per
/// effect), and live particles are never cut short.
pub const DEFAULT_PARTICLE_BUDGET: u32 = 20_000;

/// The engine's particle seed in a headless run, so its emitters repeat run
/// to run without the game seeding them.
pub(crate) const PARTICLE_HEADLESS_SEED: u64 = 0x5EED_9A27_1C1E;

/// An emitter's slot before its hook gives it one (no store in the world)
pub(crate) const NO_SLOT: u32 = u32::MAX;

/// Every emitter's particles, by the slot its `on_insert` hook took (the
/// emitter keeps it, so the step finds a pool without a lookup), and what
/// they share: the live count and its budget, the stream emitters' engine
/// seeds come from, each effect compiled once per version, and the scenes
/// held (as audio's emitters are, `audio::hold_scenes`).
#[derive(Resource, Debug)]
pub(crate) struct ParticleStore {
  /// Pools by slot; a free one keeps its arrays' room
  pub(crate) pools: Vec<EmitterPool>,
  /// Free slots, the last freed taken first
  free: Vec<u32>,
  /// Live particles over every pool
  pub(crate) live: u32,
  pub(crate) budget: u32,
  /// Where emitters' engine seeds come from, in the order they are added
  rng: ParticleRng,
  /// Each effect as the step runs it, for the version it was made from
  pub(crate) compiled: FastMap<AssetId<ParticleEffect>, Compiled>,
  /// Effects warned about dropping spawns at the budget
  pub(crate) warned_budget: FastSet<AssetId<ParticleEffect>>,
  /// The scene whose code runs now: an emitter added meanwhile is its
  scope: Option<u64>,
  /// The scenes the stack holds (not updated, under a top scene that
  /// holds them)
  held: Vec<u64>,
  /// Steps run, counting from 1: a pool stepped in step `n - 1` was
  /// stepped in the previous step
  pub(crate) step: u64,
  /// Frames the unscaled emitters ran in, counting from 1: an unscaled
  /// emitter's pool records the last it ran in
  pub(crate) frame: u64,
}

impl ParticleStore {
  /// An empty store holding at most `budget` live particles, its engine
  /// seeds drawn from `seed`
  pub(crate) fn new(budget: u32, seed: u64) -> Self {
    Self {
      pools: Vec::new(),
      free: Vec::new(),
      live: 0,
      budget,
      rng: ParticleRng::new(seed),
      compiled: FastMap::default(),
      warned_budget: FastSet::default(),
      scope: None,
      held: Vec::new(),
      step: 0,
      frame: 0,
    }
  }

  /// Live particles over every emitter
  pub(crate) fn live(&self) -> u32 {
    self.live
  }

  /// The most live particles allowed at once
  pub(crate) fn budget(&self) -> u32 {
    self.budget
  }

  /// The pool in `slot`, if an emitter holds it
  pub(crate) fn pool(&self, slot: u32) -> Option<&EmitterPool> {
    self.pools.get(slot as usize).filter(|pool| pool.used)
  }

  /// `effect` as the step last compiled it
  #[cfg_attr(not(test), expect(dead_code, reason = "read by the particle drawer"))]
  pub(crate) fn compiled(&self, effect: AssetId<ParticleEffect>) -> Option<&Compiled> {
    self.compiled.get(&effect)
  }

  /// Removes every live particle (the emitters carry on emitting)
  pub(crate) fn clear_all(&mut self) {
    for pool in &mut self.pools {
      pool.main.clear();
      pool.bounds = None;
    }
    self.live = 0;
  }

  /// Make `scope` (a scene's) the one whose code runs, returning the one
  /// before: an emitter added until the next change is that scene's, and
  /// held with it. `None` is the engine's own code
  pub(crate) fn set_scope(&mut self, scope: Option<u64>) -> Option<u64> {
    std::mem::replace(&mut self.scope, scope)
  }

  /// Hold the emitters of the scenes in `scopes` (those the stack does not
  /// update, under a top scene that holds them), and the emitters of no
  /// scene while there are any; let the rest go
  pub(crate) fn hold_scopes(&mut self, scopes: impl Iterator<Item = u64>) {
    self.held.clear();
    self.held.extend(scopes);
  }

  /// Whether the scenes held are `scopes`
  pub(crate) fn holds_exactly(&self, scopes: impl Iterator<Item = u64>) -> bool {
    self.held.iter().copied().eq(scopes)
  }

  /// Scene `scope` exited: the emitters it added that outlive it belong to
  /// no scene now, held while any scene is
  pub(crate) fn release_scope(&mut self, scope: u64) {
    for pool in &mut self.pools {
      if pool.used && pool.scope == Some(scope) {
        pool.scope = None;
      }
    }
  }

  /// Whether an emitter of `scope` is held: its scene's emitters while the
  /// scene is, and an emitter of no scene while any scene is
  pub(crate) fn holds(&self, scope: Option<u64>) -> bool {
    match scope {
      Some(scope) => self.held.contains(&scope),
      None => !self.held.is_empty(),
    }
  }

  /// A slot for `entity`'s emitter, added now: it belongs to the scene
  /// whose code runs, and takes the next engine seed
  pub(crate) fn take_slot(&mut self, entity: Entity) -> u32 {
    let engine_seed = self.rng.next_u64();
    let slot = match self.free.pop() {
      Some(slot) if (slot as usize) < self.pools.len() => slot,
      _ => {
        self.pools.push(EmitterPool::free());
        u32::try_from(self.pools.len() - 1).unwrap_or(NO_SLOT)
      }
    };
    if let Some(pool) = self.pools.get_mut(slot as usize) {
      pool.take(entity, self.scope, engine_seed);
    }
    slot
  }

  /// `entity`'s emitter is going: its pool and its particles with it
  pub(crate) fn free_slot(&mut self, slot: u32, entity: Entity) {
    let Some(pool) = self.pools.get_mut(slot as usize) else {
      return;
    };
    if !pool.used || pool.entity != entity {
      return;
    }
    let count = u32::try_from(pool.main.len()).unwrap_or(u32::MAX);
    self.live = self.live.saturating_sub(count);
    pool.release();
    self.free.push(slot);
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::asset::{Assets, Handle};
  use crate::core::ecs::{Transform, World};
  use crate::core::graphics::Image;
  use crate::core::particles::effect::ParticleEffect;
  use crate::core::particles::emitter::ParticleEmitter;

  fn effect(world: &mut World) -> Handle<ParticleEffect> {
    let mut images = Assets::<Image>::new();
    let image = images.add(Image::solid_rect(8, 8, 255, 255, 255, 255));
    world.insert_resource(images);
    let mut effects = Assets::<ParticleEffect>::new();
    let effect = effects.add(ParticleEffect::new(image));
    world.insert_resource(effects);
    effect
  }

  fn slot(world: &World, entity: Entity) -> u32 {
    world
      .get::<ParticleEmitter>(entity)
      .map_or(NO_SLOT, |e| e.slot)
  }

  #[test]
  fn an_emitter_takes_a_slot_and_gives_it_back() {
    let mut world = World::new();
    world.insert_resource(ParticleStore::new(DEFAULT_PARTICLE_BUDGET, 1));
    let fx = effect(&mut world);
    let a = world.spawn(ParticleEmitter::new(fx.clone())).id();
    let b = world.spawn(ParticleEmitter::new(fx.clone())).id();
    let (sa, sb) = (slot(&world, a), slot(&world, b));
    assert_ne!(sa, sb);
    let store = world.resource::<ParticleStore>();
    assert_eq!(store.pool(sa).map(|pool| pool.entity), Some(a));
    assert_eq!(store.pool(sb).map(|pool| pool.entity), Some(b));
    assert!(
      world.get::<Transform>(a).is_some(),
      "an emitter needs a place"
    );

    world.despawn(a);
    assert!(world.resource::<ParticleStore>().pool(sa).is_none());
    let c = world.spawn(ParticleEmitter::new(fx)).id();
    assert_eq!(slot(&world, c), sa, "a freed slot is taken again");
    assert_eq!(
      world.resource::<ParticleStore>().pool(sa).map(|p| p.entity),
      Some(c)
    );
  }

  #[test]
  fn an_emitter_put_over_another_gets_a_fresh_pool() {
    let mut world = World::new();
    world.insert_resource(ParticleStore::new(DEFAULT_PARTICLE_BUDGET, 1));
    let fx = effect(&mut world);
    let a = world.spawn(ParticleEmitter::new(fx.clone())).id();
    let first = slot(&world, a);
    world.entity_mut(a).insert(ParticleEmitter::new(fx));
    let again = slot(&world, a);
    assert_eq!(again, first, "the slot it gave back");
    let store = world.resource::<ParticleStore>();
    assert_eq!(store.pool(again).map(|p| p.entity), Some(a));
    assert_eq!(store.pools.iter().filter(|p| p.used).count(), 1);
  }

  #[test]
  fn hooks_do_nothing_without_a_store() {
    let mut world = World::new();
    let fx = effect(&mut world);
    let a = world.spawn(ParticleEmitter::new(fx)).id();
    assert_eq!(slot(&world, a), NO_SLOT);
    world.despawn(a);
  }

  #[test]
  fn insertion_order_fixes_the_engine_seeds() {
    let seeds = |store_seed: u64| {
      let mut world = World::new();
      world.insert_resource(ParticleStore::new(DEFAULT_PARTICLE_BUDGET, store_seed));
      let fx = effect(&mut world);
      let a = world.spawn(ParticleEmitter::new(fx.clone())).id();
      let b = world.spawn(ParticleEmitter::new(fx)).id();
      let store = world.resource::<ParticleStore>();
      let seed = |e| {
        store
          .pool(slot(&world, e))
          .map(|pool| pool.engine_seed)
          .unwrap_or_default()
      };
      (seed(a), seed(b))
    };
    let (a, b) = seeds(1);
    assert_ne!(a, b);
    assert_eq!(seeds(1), (a, b));
    assert_ne!(seeds(2), (a, b));
  }

  #[test]
  fn an_emitter_records_the_scope_it_was_added_in() {
    let mut world = World::new();
    world.insert_resource(ParticleStore::new(DEFAULT_PARTICLE_BUDGET, 1));
    let fx = effect(&mut world);
    let outside = world.spawn(ParticleEmitter::new(fx.clone())).id();
    let previous = world.resource_mut::<ParticleStore>().set_scope(Some(4));
    assert_eq!(previous, None);
    let inside = world.spawn(ParticleEmitter::new(fx)).id();
    world.resource_mut::<ParticleStore>().set_scope(previous);
    let store = world.resource::<ParticleStore>();
    let scope = |e| store.pool(slot(&world, e)).and_then(|pool| pool.scope);
    assert_eq!(scope(outside), None);
    assert_eq!(scope(inside), Some(4));

    world.resource_mut::<ParticleStore>().release_scope(4);
    let store = world.resource::<ParticleStore>();
    assert_eq!(store.pool(slot(&world, inside)).and_then(|p| p.scope), None);
  }

  #[test]
  fn holds_follow_audio_s_rule() {
    let mut store = ParticleStore::new(DEFAULT_PARTICLE_BUDGET, 1);
    assert!(!store.holds(None) && !store.holds(Some(1)));
    store.hold_scopes([1].into_iter());
    assert!(store.holds(Some(1)));
    assert!(!store.holds(Some(2)));
    assert!(store.holds(None), "no scope's: held while any is");
    store.hold_scopes(std::iter::empty());
    assert!(!store.holds(None) && !store.holds(Some(1)));
  }

  #[test]
  fn the_budget_and_live_count_start_as_given() {
    let store = ParticleStore::new(50, 1);
    assert_eq!(store.budget(), 50);
    assert_eq!(store.live(), 0);
    assert_eq!(DEFAULT_PARTICLE_BUDGET, 20_000);
    assert_eq!(PARTICLE_HEADLESS_SEED, 0x5EED_9A27_1C1E);
  }
}
