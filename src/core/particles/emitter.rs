//! The particle emitter: a component that places an effect in the world,
//! and the message that reports its end.

use super::effect::ParticleEffect;
use super::store::{NO_SLOT, ParticleStore};
use crate::core::asset::Handle;
use crate::core::ecs::kernel::{DeferredWorld, HookContext};
use crate::core::ecs::{Component, Entity, Message, Transform};

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
///
/// It runs on game time: a paused clock and hitstop freeze it, and slow
/// motion slows it. It steps while its scene updates, so not under a scene
/// that stops it (a pause menu); a scene transition doesn't stop it. One
/// made [`unscaled`](Self::unscaled) runs on real time through all of them.
///
/// The game controls it with [`play`](Self::play), [`stop`](Self::stop),
/// [`clear`](Self::clear), [`restart`](Self::restart) and
/// [`burst`](Self::burst), each done in its next step. It is finished when
/// it is not playing (stopped, or its one cycle over) and has no live
/// particles: an [`EmitterFinished`] message reports each finish, and one
/// made [`despawn_when_finished`](Self::despawn_when_finished) despawns its
/// entity, with its children, in the step after.
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
  /// Its time as a multiple of game time (of real time when unscaled)
  speed: f32,
  /// It runs on real time, through pauses, hitstop and held scenes
  pub(crate) unscaled: bool,
  /// It plays one cycle, even of a looping effect
  pub(crate) once: bool,
  /// Its entity is despawned in the step after it finishes
  pub(crate) despawn_when_finished: bool,
  /// Whether it emits: `stop` and the end of its one cycle clear it
  pub(crate) playing: bool,
  /// What happens at its next finish
  pub(crate) finish: Finish,
  /// What the game asked of it since its last step, done by its next
  pub(crate) pending: Pending,
  /// Its live particles, as the last step left them
  pub(crate) alive: u32,
  /// Its pool in the store, given by the `on_insert` hook
  pub(crate) slot: u32,
  /// Moved with `skip_interpolation` since its last step: its next step
  /// spawns where it stands, with no spread along the jump, and clears it
  pub(crate) teleported: bool,
}

/// Whether an emitter's next finish is reported
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Finish {
  /// Its next finish sends [`EmitterFinished`]
  Armed,
  /// Made stopped: it has nothing to report until it plays
  Idle,
  /// Its finish was reported: one that despawns when finished goes in its
  /// next step, unless `play` or `restart` armed it again
  Reported,
}

/// Controls asked for since an emitter's last step
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Pending {
  /// Remove the live particles
  pub(crate) clear: bool,
  /// Remove the live particles and start the cycle over, bursts and all
  pub(crate) restart: bool,
  /// Particles to add, whatever the effect emits
  pub(crate) burst: u32,
}

/// A [`ParticleEmitter`] finished: it is not playing (stopped, or its one
/// cycle over) and its last particle has died. Sent once per finish, by the
/// step it finishes in; `play` and `restart` arm it again. A game-time
/// emitter made `despawn_when_finished` is still there in the step after
/// (a scene's `update` can read it), and is despawned by that step's
/// particles. An unscaled one can do both within one frame, so its entity
/// may be gone when this is read.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmitterFinished {
  /// The emitter's entity
  pub entity: Entity,
}

impl ParticleEmitter {
  /// An emitter playing `effect`.
  pub fn new(effect: Handle<ParticleEffect>) -> Self {
    Self {
      effect,
      seed: None,
      z: None,
      speed: 1.0,
      unscaled: false,
      once: false,
      despawn_when_finished: false,
      playing: true,
      finish: Finish::Armed,
      pending: Pending::default(),
      alive: 0,
      slot: NO_SLOT,
      teleported: false,
    }
  }

  /// Fixes its random seed, over the effect's, so it repeats.
  pub fn with_seed(mut self, seed: u64) -> Self {
    self.seed = Some(seed);
    self
  }

  /// Sets its draw order among sprites, over the effect's `z`.
  pub fn with_z(mut self, z: i32) -> Self {
    self.z = Some(z);
    self
  }

  /// Runs its time at `speed` times game time, or real time when unscaled
  /// (0 to 100; a value that is not a number, or negative, stops it).
  pub fn with_speed(mut self, speed: f32) -> Self {
    self.speed = allowed_speed(speed);
    self
  }

  /// Runs it on real time: a paused clock, hitstop, slow motion and a scene
  /// stopped under another leave it running (for a menu's effects).
  /// It steps once a frame, in fixed steps of the game's step length (at
  /// most 8 a frame; a longer frame's time beyond them is dropped).
  pub fn unscaled(mut self) -> Self {
    self.unscaled = true;
    self
  }

  /// Starts it stopped: it emits nothing until [`play`](Self::play), and
  /// reports no finish before it has played.
  pub fn stopped(mut self) -> Self {
    self.playing = false;
    self.finish = Finish::Idle;
    self
  }

  /// Plays one cycle of its effect, even of a looping one, and stops.
  pub fn once(mut self) -> Self {
    self.once = true;
    self
  }

  /// Despawns its entity, with its children, in the step after it
  /// finishes (see [`is_finished`](Self::is_finished)).
  pub fn despawn_when_finished(mut self) -> Self {
    self.despawn_when_finished = true;
    self
  }

  /// Plays it: a stopped emitter resumes where it stopped, and one whose
  /// one cycle ended plays its cycle again from the start. Its next finish
  /// is reported.
  pub fn play(&mut self) {
    self.playing = true;
    self.finish = Finish::Armed;
  }

  /// Stops it emitting. Its live particles carry on, moving, colliding
  /// and dying as before.
  pub fn stop(&mut self) {
    self.playing = false;
  }

  /// Removes its live particles in its next step. It carries on emitting
  /// if it plays.
  pub fn clear(&mut self) {
    self.pending.clear = true;
  }

  /// Removes its live particles and starts its cycle over at 0 (the start
  /// delay skipped), every burst due again, and plays it. Its next finish
  /// is reported.
  pub fn restart(&mut self) {
    self.pending.restart = true;
    self.play();
  }

  /// Adds `count` particles in its next step, whatever its effect emits
  /// and whether or not it plays, within the effect's `max_particles` and
  /// the engine's budget (the rest are dropped). An emitter made
  /// [`stopped`](Self::stopped) never reports [`EmitterFinished`] and never
  /// despawns for it: only [`play`](Self::play) and
  /// [`restart`](Self::restart) arm that.
  pub fn burst(&mut self, count: u32) {
    self.pending.burst = self.pending.burst.saturating_add(count);
  }

  /// Runs its time at `speed` times game time, or real time when unscaled,
  /// from its next step (0 to 100; a value that is not a number, or
  /// negative, stops it).
  pub fn set_speed(&mut self, speed: f32) {
    self.speed = allowed_speed(speed);
  }

  /// Whether it emits: not after [`stop`](Self::stop), nor once its one
  /// cycle is over.
  pub fn is_playing(&self) -> bool {
    self.playing
  }

  /// Whether it is finished: not playing, with no live particles (as its
  /// last step left them) and no burst still to add.
  pub fn is_finished(&self) -> bool {
    !self.playing && self.alive == 0 && self.pending.burst == 0
  }

  /// Whether it runs on real time ([`unscaled`](Self::unscaled)).
  pub fn is_unscaled(&self) -> bool {
    self.unscaled
  }

  /// Its draw order, if it sets its own.
  pub fn z(&self) -> Option<i32> {
    self.z
  }

  /// How many of its particles are alive, as the last step left them.
  pub fn alive(&self) -> u32 {
    self.alive
  }

  /// Its time as a multiple of game time (real time when unscaled).
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
    emitter.teleported = false;
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
    assert!(emitter.is_playing() && !emitter.is_finished() && !emitter.is_unscaled());
    assert!(!emitter.once && !emitter.despawn_when_finished);
    let emitter = emitter
      .with_seed(9)
      .with_z(3)
      .with_speed(0.5)
      .unscaled()
      .once()
      .despawn_when_finished();
    assert_eq!(emitter.seed, Some(9));
    assert_eq!(emitter.z(), Some(3));
    assert_eq!(emitter.speed(), 0.5);
    assert!(emitter.is_unscaled() && emitter.once && emitter.despawn_when_finished);
    let stopped = ParticleEmitter::new(effect()).stopped();
    assert!(!stopped.is_playing());
    assert_eq!(stopped.finish, Finish::Idle, "nothing to report yet");
  }

  #[test]
  fn controls_wait_for_the_next_step() {
    let mut emitter = ParticleEmitter::new(effect());
    emitter.clear();
    emitter.burst(3);
    emitter.burst(u32::MAX);
    assert_eq!(
      emitter.pending,
      Pending {
        clear: true,
        restart: false,
        burst: u32::MAX
      }
    );
    emitter.stop();
    assert!(!emitter.is_playing());
    assert!(!emitter.is_finished(), "a burst is still to come");
    emitter.finish = Finish::Reported;
    emitter.restart();
    assert!(emitter.is_playing() && emitter.pending.restart);
    assert_eq!(emitter.finish, Finish::Armed);
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
      let mut emitter = ParticleEmitter::new(effect());
      emitter.set_speed(asked);
      assert_eq!(emitter.speed(), kept);
    }
  }
}
