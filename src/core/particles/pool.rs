//! Particle storage: one emitter's particles, field by field, in spawn order.

use std::time::Duration;

use glam::Vec2;

use super::effect::ParticleEffect;
use super::rng::ParticleRng;
use super::space::LocalFrame;
use crate::core::asset::AssetId;
use crate::core::ecs::Entity;

/// A particle's flag: it stuck where it hit a collider
/// (`CollisionResponse::Stick`), and from then on only ages
pub(crate) const STUCK: u8 = 1;

/// A particle's flag: it touched a collider in its last step, so a bounce
/// or a pass-through tells only of an entry
pub(crate) const TOUCHING: u8 = 1 << 1;

/// One new particle's starting values, for [`Pool::push`]
#[derive(Debug, Clone, Copy)]
pub(crate) struct NewParticle {
  pub(crate) position: Vec2,
  pub(crate) velocity: Vec2,
  pub(crate) lifetime: f32,
  pub(crate) size: f32,
  /// Radians
  pub(crate) rotation: f32,
  /// Radians per second
  pub(crate) spin: f32,
  pub(crate) color: [f32; 4],
  pub(crate) seed: u32,
}

/// Particles stored field by field (one array per field, index `i` the
/// same particle in each), in spawn order: the oldest first. Dead ones
/// leave by [`retain`](Self::retain), which keeps that order, so the draw
/// order of the survivors never changes. `size` and `color` are the start
/// values; the effect's curves scale them over life as they are read.
#[derive(Debug, Default)]
pub(crate) struct Pool {
  pub(crate) x: Vec<f32>,
  pub(crate) y: Vec<f32>,
  /// Where it was the step before (for drawing between steps)
  pub(crate) px: Vec<f32>,
  pub(crate) py: Vec<f32>,
  pub(crate) vx: Vec<f32>,
  pub(crate) vy: Vec<f32>,
  pub(crate) age: Vec<f32>,
  pub(crate) lifetime: Vec<f32>,
  pub(crate) size: Vec<f32>,
  /// Radians
  pub(crate) rotation: Vec<f32>,
  /// Radians per second
  pub(crate) spin: Vec<f32>,
  pub(crate) color: Vec<[f32; 4]>,
  /// The particle's own random bits, for values it keeps all its life
  pub(crate) seed: Vec<u32>,
  /// Its collision state: [`STUCK`] and [`TOUCHING`]
  pub(crate) flags: Vec<u8>,
}

impl Pool {
  /// How many particles it holds
  pub(crate) fn len(&self) -> usize {
    self.x.len()
  }

  /// Removes every particle, keeping the arrays' room
  pub(crate) fn clear(&mut self) {
    self.x.clear();
    self.y.clear();
    self.px.clear();
    self.py.clear();
    self.vx.clear();
    self.vy.clear();
    self.age.clear();
    self.lifetime.clear();
    self.size.clear();
    self.rotation.clear();
    self.spin.clear();
    self.color.clear();
    self.seed.clear();
    self.flags.clear();
  }

  /// Adds a particle at the end (the newest), aged 0, where it was the
  /// step before being where it starts
  pub(crate) fn push(&mut self, new: &NewParticle) {
    self.x.push(new.position.x);
    self.y.push(new.position.y);
    self.px.push(new.position.x);
    self.py.push(new.position.y);
    self.vx.push(new.velocity.x);
    self.vy.push(new.velocity.y);
    self.age.push(0.0);
    self.lifetime.push(new.lifetime);
    self.size.push(new.size);
    self.rotation.push(new.rotation);
    self.spin.push(new.spin);
    self.color.push(new.color);
    self.seed.push(new.seed);
    self.flags.push(0);
  }

  /// Keeps the particles `keep` is true for (asked by index, in order) and
  /// moves them down over the others, in the order they were in: never a
  /// swap with the last, which would change which particle draws over which
  pub(crate) fn retain(&mut self, mut keep: impl FnMut(usize) -> bool) {
    let len = self.len();
    let mut kept = 0;
    for i in 0..len {
      if !keep(i) {
        continue;
      }
      if kept != i {
        self.x[kept] = self.x[i];
        self.y[kept] = self.y[i];
        self.px[kept] = self.px[i];
        self.py[kept] = self.py[i];
        self.vx[kept] = self.vx[i];
        self.vy[kept] = self.vy[i];
        self.age[kept] = self.age[i];
        self.lifetime[kept] = self.lifetime[i];
        self.size[kept] = self.size[i];
        self.rotation[kept] = self.rotation[i];
        self.spin[kept] = self.spin[i];
        self.color[kept] = self.color[i];
        self.seed[kept] = self.seed[i];
        self.flags[kept] = self.flags[i];
      }
      kept += 1;
    }
    self.truncate(kept);
  }

  fn truncate(&mut self, len: usize) {
    self.x.truncate(len);
    self.y.truncate(len);
    self.px.truncate(len);
    self.py.truncate(len);
    self.vx.truncate(len);
    self.vy.truncate(len);
    self.age.truncate(len);
    self.lifetime.truncate(len);
    self.size.truncate(len);
    self.rotation.truncate(len);
    self.spin.truncate(len);
    self.color.truncate(len);
    self.seed.truncate(len);
    self.flags.truncate(len);
  }
}

/// The particles a sub-effect plays in an emitter's pool (those its own
/// particles' deaths, or collisions, spawned), drawn as a run of their own
#[derive(Debug, Default)]
pub(crate) struct SubPool {
  pub(crate) particles: Pool,
  /// The sub-effect they play: the parent effect's, or, once it has none,
  /// the last it had, so they live out their lives
  pub(crate) effect: Option<AssetId<ParticleEffect>>,
  /// The box round them, as `EmitterPool::bounds`
  pub(crate) bounds: Option<(Vec2, Vec2)>,
  /// The emitter's frame in its last step, while they live in it (a
  /// `Local` sub-effect's), as `EmitterPool::local`
  pub(crate) local: Option<LocalFrame>,
}

impl SubPool {
  /// Removes every particle, keeping the arrays' room
  pub(crate) fn clear(&mut self) {
    self.particles.clear();
    self.bounds = None;
  }

  fn reset(&mut self) {
    self.clear();
    self.effect = None;
    self.local = None;
  }
}

/// One emitter's particles and where its emission stands. Kept by slot in
/// the [`ParticleStore`](super::store::ParticleStore); a freed slot's pool
/// keeps its arrays' room for the next emitter to take it.
#[derive(Debug)]
pub(crate) struct EmitterPool {
  /// The emitter's entity (`Entity::PLACEHOLDER` while the slot is free)
  pub(crate) entity: Entity,
  /// Whether an emitter holds the slot
  pub(crate) used: bool,
  pub(crate) main: Pool,
  /// The particles its effect's `on_death` played
  pub(crate) death: SubPool,
  /// The particles its effect's `on_collision` played
  pub(crate) collision: SubPool,
  /// The emitter's own random stream, seeded in its first step with an
  /// effect (`seeded`)
  pub(crate) rng: ParticleRng,
  pub(crate) seeded: bool,
  /// Its seed folded to 32 bits, for the noise force
  pub(crate) noise_seed: u32,
  /// Drawn from the store's stream as the emitter was added: its seed when
  /// neither it nor its effect fixes one
  pub(crate) engine_seed: u64,
  /// The box round its particles, where they are and where they were the
  /// step before; `None` with no particles
  pub(crate) bounds: Option<(Vec2, Vec2)>,
  /// The effect its emission state below belongs to
  pub(crate) effect: Option<AssetId<ParticleEffect>>,
  /// Seconds into the current cycle, below 0 during the start delay; `None`
  /// until its effect is loaded
  pub(crate) time: Option<f64>,
  /// Whether its first cycle has begun (its start delay is over): a looping
  /// cycle that starts a hair early, inside the tolerance, has a little
  /// time below 0 that is no delay
  pub(crate) cycling: bool,
  /// The rate's fraction of a particle carried to the next step
  pub(crate) carry: f64,
  /// How many times each of the effect's bursts fired this cycle
  pub(crate) fired: Vec<u32>,
  /// Where it emitted from in the step it last stepped
  pub(crate) last: Option<Vec2>,
  /// The emitter's frame in the step it last stepped, while its particles
  /// live in it (a `Local` effect's); `None` while they live in the world
  pub(crate) local: Option<LocalFrame>,
  /// The store's step it last stepped in
  pub(crate) last_step: Option<u64>,
  /// The scene that added it, whose hold holds it; `None`: none's
  pub(crate) scope: Option<u64>,
  /// Its one cycle ended (`ParticleEmitter::once`, or an effect that does
  /// not loop): playing it again starts the cycle over
  pub(crate) ended: bool,
  /// An unscaled emitter's real time (times its speed) not yet stepped
  pub(crate) clock: Duration,
  /// How far an unscaled emitter is through its next step (its `clock`
  /// over the step's length, below 1), as the drawer blends it
  pub(crate) fraction: f32,
}

impl EmitterPool {
  /// A free slot's pool
  pub(crate) fn free() -> Self {
    Self {
      entity: Entity::PLACEHOLDER,
      used: false,
      main: Pool::default(),
      death: SubPool::default(),
      collision: SubPool::default(),
      rng: ParticleRng::new(0),
      seeded: false,
      noise_seed: 0,
      engine_seed: 0,
      bounds: None,
      effect: None,
      time: None,
      cycling: false,
      carry: 0.0,
      fired: Vec::new(),
      last: None,
      local: None,
      last_step: None,
      scope: None,
      ended: false,
      clock: Duration::ZERO,
      fraction: 0.0,
    }
  }

  /// Hands the slot to `entity`'s emitter, added in `scope`: no particles
  /// and no emission yet
  pub(crate) fn take(&mut self, entity: Entity, scope: Option<u64>, engine_seed: u64) {
    self.reset();
    self.entity = entity;
    self.used = true;
    self.scope = scope;
    self.engine_seed = engine_seed;
  }

  /// Frees the slot, keeping the arrays' room
  pub(crate) fn release(&mut self) {
    self.reset();
  }

  /// Every particle it holds: its own and its sub-effects'
  pub(crate) fn len(&self) -> usize {
    self.main.len() + self.death.particles.len() + self.collision.particles.len()
  }

  /// Removes every particle, its sub-effects' too, keeping the arrays' room
  pub(crate) fn clear(&mut self) {
    self.main.clear();
    self.bounds = None;
    self.death.clear();
    self.collision.clear();
  }

  fn reset(&mut self) {
    self.entity = Entity::PLACEHOLDER;
    self.used = false;
    self.main.clear();
    self.death.reset();
    self.collision.reset();
    self.rng = ParticleRng::new(0);
    self.seeded = false;
    self.noise_seed = 0;
    self.engine_seed = 0;
    self.bounds = None;
    self.effect = None;
    self.time = None;
    self.cycling = false;
    self.carry = 0.0;
    self.fired.clear();
    self.last = None;
    self.local = None;
    self.last_step = None;
    self.scope = None;
    self.ended = false;
    self.clock = Duration::ZERO;
    self.fraction = 0.0;
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn at(x: f32) -> NewParticle {
    NewParticle {
      position: Vec2::new(x, -x),
      velocity: Vec2::new(x * 2.0, 1.0),
      lifetime: x + 10.0,
      size: x + 1.0,
      rotation: x * 0.5,
      spin: -x,
      color: [x / 10.0, 0.0, 0.0, 1.0],
      seed: x as u32,
    }
  }

  #[test]
  fn retain_compacts_every_field_in_spawn_order() {
    let mut pool = Pool::default();
    for x in 0..6 {
      pool.push(&at(x as f32));
    }
    assert_eq!(pool.len(), 6);
    pool.retain(|i| i % 2 == 1);
    assert_eq!(pool.len(), 3);
    assert_eq!(pool.x, [1.0, 3.0, 5.0]);
    assert_eq!(pool.y, [-1.0, -3.0, -5.0]);
    assert_eq!(pool.px, [1.0, 3.0, 5.0]);
    assert_eq!(pool.py, [-1.0, -3.0, -5.0]);
    assert_eq!(pool.vx, [2.0, 6.0, 10.0]);
    assert_eq!(pool.vy, [1.0, 1.0, 1.0]);
    assert_eq!(pool.age, [0.0; 3]);
    assert_eq!(pool.lifetime, [11.0, 13.0, 15.0]);
    assert_eq!(pool.size, [2.0, 4.0, 6.0]);
    assert_eq!(pool.rotation, [0.5, 1.5, 2.5]);
    assert_eq!(pool.spin, [-1.0, -3.0, -5.0]);
    assert_eq!(pool.color[1], [0.3, 0.0, 0.0, 1.0]);
    assert_eq!(pool.seed, [1, 3, 5]);
    assert_eq!(pool.flags, [0; 3]);
  }

  #[test]
  fn clear_empties_the_pool_and_keeps_its_room() {
    let mut pool = Pool::default();
    for x in 0..100 {
      pool.push(&at(x as f32));
    }
    let room = pool.x.capacity();
    pool.clear();
    assert_eq!(pool.len(), 0);
    assert_eq!(pool.x.capacity(), room);
  }

  #[test]
  fn retain_keeping_everything_or_nothing() {
    let mut pool = Pool::default();
    for x in 0..4 {
      pool.push(&at(x as f32));
    }
    pool.retain(|_| true);
    assert_eq!(pool.x, [0.0, 1.0, 2.0, 3.0]);
    pool.retain(|_| false);
    assert_eq!(pool.len(), 0);
    assert!(pool.seed.is_empty() && pool.color.is_empty());
  }
}
