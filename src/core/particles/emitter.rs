//! The particle emitter: a component that places an effect in the world.

use super::effect::ParticleEffect;
use super::store::{NO_SLOT, ParticleStore};
use crate::core::asset::Handle;
use crate::core::ecs::kernel::{DeferredWorld, HookContext};
use crate::core::ecs::{Component, Transform};

/// The fastest an emitter runs, as a multiple of game time (the game
/// clock's own speed limit)
pub(crate) const MAX_SPEED: f32 = 100.0;

/// Places a [`ParticleEffect`] on its entity: the engine spawns, moves and
/// removes its particles in its fixed step, at the entity's
/// `GlobalTransform` (lifted on a floor), with no call from the game.
/// Despawning the entity, or taking the emitter off it, removes its
/// particles; a `Disabled` emitter is frozen. Each emitter draws from its
/// own random stream, seeded by [`with_seed`](Self::with_seed), else the
/// effect's `seed`, else the engine (fixed in a headless run).
#[derive(Component, Debug, Clone)]
#[require(Transform)]
#[component(on_insert = emitter_inserted, on_discard = emitter_discarded)]
pub struct ParticleEmitter {
  /// The effect it plays
  pub effect: Handle<ParticleEffect>,
  /// A fixed seed for its stream, over the effect's
  pub(crate) seed: Option<u64>,
  /// Its draw order among sprites, over the effect's
  z: Option<i32>,
  /// Its time as a multiple of game time
  speed: f32,
  /// Its live particles, as the last step left them
  pub(crate) alive: u32,
  /// Its pool in the store, given by the `on_insert` hook
  pub(crate) slot: u32,
}

impl ParticleEmitter {
  /// An emitter playing `effect`.
  #[cfg_attr(
    not(test),
    expect(dead_code, reason = "exported with the engine's emitter")
  )]
  pub fn new(effect: Handle<ParticleEffect>) -> Self {
    Self {
      effect,
      seed: None,
      z: None,
      speed: 1.0,
      alive: 0,
      slot: NO_SLOT,
    }
  }

  /// Fixes its random seed, over the effect's, so it repeats.
  #[cfg_attr(
    not(test),
    expect(dead_code, reason = "exported with the engine's emitter")
  )]
  pub fn with_seed(mut self, seed: u64) -> Self {
    self.seed = Some(seed);
    self
  }

  /// Sets its draw order among sprites, over the effect's `z`.
  #[cfg_attr(
    not(test),
    expect(dead_code, reason = "exported with the engine's emitter")
  )]
  pub fn with_z(mut self, z: i32) -> Self {
    self.z = Some(z);
    self
  }

  /// Runs its time at `speed` times game time (0 to 100; a value that is
  /// not a number, or negative, stops it).
  #[cfg_attr(
    not(test),
    expect(dead_code, reason = "exported with the engine's emitter")
  )]
  pub fn with_speed(mut self, speed: f32) -> Self {
    self.speed = allowed_speed(speed);
    self
  }

  /// Its draw order, if it sets its own.
  #[cfg_attr(
    not(test),
    expect(dead_code, reason = "exported with the engine's emitter")
  )]
  pub fn z(&self) -> Option<i32> {
    self.z
  }

  /// How many of its particles are alive, as the last step left them.
  #[cfg_attr(
    not(test),
    expect(dead_code, reason = "exported with the engine's emitter")
  )]
  pub fn alive(&self) -> u32 {
    self.alive
  }

  /// Its time as a multiple of game time.
  pub fn speed(&self) -> f32 {
    self.speed
  }
}

/// `speed` kept to 0 to [`MAX_SPEED`]; 0 when not a number
fn allowed_speed(speed: f32) -> f32 {
  if speed.is_finite() {
    speed.clamp(0.0, MAX_SPEED)
  } else {
    0.0
  }
}

/// An emitter was added, or put over another (whose pool its
/// `on_discard` freed first): it takes a pool in the store, which records
/// the scene whose code runs now and draws its engine seed, so the order
/// emitters are added in fixes their seeds. Nothing without a store
fn emitter_inserted(mut world: DeferredWorld, context: HookContext) {
  let entity = context.entity;
  let Some(mut store) = world.get_resource_mut::<ParticleStore>() else {
    return;
  };
  let slot = store.take_slot(entity);
  if let Some(mut emitter) = world.get_mut::<ParticleEmitter>(entity) {
    emitter.slot = slot;
    emitter.alive = 0;
  }
}

/// An emitter is going (taken away, put over by another, or despawned with
/// its entity): its pool is freed, and its particles leave the live count
fn emitter_discarded(mut world: DeferredWorld, context: HookContext) {
  let entity = context.entity;
  let Some(slot) = world.get::<ParticleEmitter>(entity).map(|e| e.slot) else {
    return;
  };
  if let Some(mut store) = world.get_resource_mut::<ParticleStore>() {
    store.free_slot(slot, entity);
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::asset::Assets;
  use crate::core::graphics::Image;

  fn effect() -> Handle<ParticleEffect> {
    let image = Assets::<Image>::new().add(Image::solid_rect(8, 8, 255, 255, 255, 255));
    Assets::<ParticleEffect>::new().add(ParticleEffect::new(image))
  }

  #[test]
  fn builders_set_what_they_name() {
    let emitter = ParticleEmitter::new(effect());
    assert_eq!(emitter.z(), None);
    assert_eq!(emitter.speed(), 1.0);
    assert_eq!(emitter.alive(), 0);
    let emitter = emitter.with_seed(9).with_z(3).with_speed(0.5);
    assert_eq!(emitter.seed, Some(9));
    assert_eq!(emitter.z(), Some(3));
    assert_eq!(emitter.speed(), 0.5);
  }

  #[test]
  fn a_speed_that_is_not_a_number_stops_the_emitter() {
    for (asked, kept) in [
      (f32::NAN, 0.0),
      (-1.0, 0.0),
      (f32::INFINITY, 0.0),
      (1e9, MAX_SPEED),
      (2.0, 2.0),
    ] {
      assert_eq!(
        ParticleEmitter::new(effect()).with_speed(asked).speed(),
        kept
      );
    }
  }
}
