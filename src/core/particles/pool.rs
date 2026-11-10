//! Particle storage: one emitter's particles, field by field, in spawn order.

use std::ops::Range;
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

/// A pool of at least this many particles compacts by runs (see
/// [`Pool::retain`]). Below it the particle-by-particle loop costs a few
/// microseconds at most, and was measured faster when many die at once
const RUN_COMPACTION: usize = 4_096;

/// A run of survivors shorter than this is copied particle by particle:
/// fourteen `copy_within` calls cost more than copying a few
const SHORT_RUN: usize = 8;

/// One new particle's starting values, for [`Pool::push`]
#[derive(Debug, Clone, Copy)]
pub(crate) struct NewParticle {
  pub(crate) position: Vec2,
  /// Where the draw starts it, in the step it is born
  pub(crate) previous: Vec2,
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

  /// Adds a particle at the end (the newest), aged 0
  pub(crate) fn push(&mut self, new: &NewParticle) {
    self.x.push(new.position.x);
    self.y.push(new.position.y);
    self.px.push(new.previous.x);
    self.py.push(new.previous.y);
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

  /// Keeps the particles `keep` is true for (asked by index, in order, once
  /// each) and moves them down over the others, in the order they were in:
  /// never a swap with the last, which would change which particle draws
  /// over which. A small pool copies particle by particle; a large one,
  /// where nearly every survivor moves on a step that loses its oldest,
  /// moves each run of survivors at once. The arrays keep their room
  pub(crate) fn retain(&mut self, mut keep: impl FnMut(usize) -> bool) {
    let len = self.len();
    let mut kept = 0;
    if len < RUN_COMPACTION {
      for i in 0..len {
        if !keep(i) {
          continue;
        }
        if kept != i {
          self.copy_particle(i, kept);
        }
        kept += 1;
      }
    } else {
      let mut i = 0;
      while i < len {
        if !keep(i) {
          i += 1;
          continue;
        }
        let from = i;
        i += 1;
        while i < len && keep(i) {
          i += 1;
        }
        let count = i - from;
        if from != kept {
          if count < SHORT_RUN {
            for offset in 0..count {
              self.copy_particle(from + offset, kept + offset);
            }
          } else {
            self.copy_run(from..i, kept);
          }
        }
        kept += count;
        // Past the particle that ended the run, already asked
        i += 1;
      }
    }
    self.truncate(kept);
  }

  /// Particle `from`, every field, over particle `to`
  #[inline(always)]
  fn copy_particle(&mut self, from: usize, to: usize) {
    self.x[to] = self.x[from];
    self.y[to] = self.y[from];
    self.px[to] = self.px[from];
    self.py[to] = self.py[from];
    self.vx[to] = self.vx[from];
    self.vy[to] = self.vy[from];
    self.age[to] = self.age[from];
    self.lifetime[to] = self.lifetime[from];
    self.size[to] = self.size[from];
    self.rotation[to] = self.rotation[from];
    self.spin[to] = self.spin[from];
    self.color[to] = self.color[from];
    self.seed[to] = self.seed[from];
    self.flags[to] = self.flags[from];
  }

  /// The particles of `run`, every field, moved down to start at `to`
  #[inline(never)]
  fn copy_run(&mut self, run: Range<usize>, to: usize) {
    self.x.copy_within(run.clone(), to);
    self.y.copy_within(run.clone(), to);
    self.px.copy_within(run.clone(), to);
    self.py.copy_within(run.clone(), to);
    self.vx.copy_within(run.clone(), to);
    self.vy.copy_within(run.clone(), to);
    self.age.copy_within(run.clone(), to);
    self.lifetime.copy_within(run.clone(), to);
    self.size.copy_within(run.clone(), to);
    self.rotation.copy_within(run.clone(), to);
    self.spin.copy_within(run.clone(), to);
    self.color.copy_within(run.clone(), to);
    self.seed.copy_within(run.clone(), to);
    self.flags.copy_within(run, to);
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
      previous: Vec2::new(x, -x),
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

  /// A pool of `n` particles, each field of each one a value of its own,
  /// and that particle's fields as they should read
  fn distinct(n: usize) -> Pool {
    let mut pool = Pool::default();
    for i in 0..n {
      pool.push(&at(i as f32));
      pool.px[i] = i as f32 + 0.25;
      pool.py[i] = i as f32 + 0.5;
      pool.age[i] = i as f32 + 0.75;
      pool.flags[i] = i as u8;
    }
    pool
  }

  /// That `pool` holds the particles `survivors` of a `distinct` pool, in
  /// that order, every field array the same length and in step
  fn assert_holds(pool: &Pool, survivors: &[usize], case: &str) {
    assert_eq!(pool.len(), survivors.len(), "{case}");
    let expected = distinct(survivors.iter().max().map_or(0, |last| last + 1));
    let fields = |p: &Pool, i: usize| {
      let [r, g, b, a] = p.color[i];
      [
        p.x[i],
        p.y[i],
        p.px[i],
        p.py[i],
        p.vx[i],
        p.vy[i],
        p.age[i],
        p.lifetime[i],
        p.size[i],
        p.rotation[i],
        p.spin[i],
        r,
        g,
        b,
        a,
        p.seed[i] as f32,
        f32::from(p.flags[i]),
      ]
    };
    for (at, &was) in survivors.iter().enumerate() {
      assert_eq!(fields(pool, at), fields(&expected, was), "{case}: {at}");
    }
    let lengths = [
      pool.y.len(),
      pool.px.len(),
      pool.py.len(),
      pool.vx.len(),
      pool.vy.len(),
      pool.age.len(),
      pool.lifetime.len(),
      pool.size.len(),
      pool.rotation.len(),
      pool.spin.len(),
      pool.color.len(),
      pool.seed.len(),
      pool.flags.len(),
    ];
    assert_eq!(
      lengths,
      [survivors.len(); 13],
      "{case}: the arrays' lengths"
    );
  }

  /// Through the particle-by-particle path and the run path alike
  #[test]
  fn retain_keeps_every_field_in_step_whatever_dies() {
    type Dies = fn(usize, usize) -> bool;
    let cases: [(&str, Dies); 10] = [
      ("a dead prefix", |i, n| i < n / 5),
      ("a dead middle", |i, n| (n / 2..n / 2 + n / 5).contains(&i)),
      ("a dead suffix", |i, n| i >= n * 4 / 5),
      ("everything dead", |_, _| true),
      ("nothing dead", |_, _| false),
      ("the first dead", |i, _| i == 0),
      ("the last dead", |i, n| i == n - 1),
      ("every other one dead", |i, _| i % 2 == 0),
      ("runs shorter than a move", |i, _| i % SHORT_RUN / 2 == 1),
      ("long and short runs", |i, n| {
        i % 50 == 7 || (i > n / 2 && i % 3 == 0)
      }),
    ];
    for n in [
      200,
      RUN_COMPACTION - 1,
      RUN_COMPACTION,
      RUN_COMPACTION + 500,
    ] {
      for (case, dies) in cases {
        let mut pool = distinct(n);
        let room = pool.x.capacity();
        let mut asked = Vec::with_capacity(n);
        pool.retain(|i| {
          asked.push(i);
          !dies(i, n)
        });
        assert_eq!(
          asked,
          (0..n).collect::<Vec<_>>(),
          "{n}, {case}: asked in order"
        );
        let survivors: Vec<usize> = (0..n).filter(|&i| !dies(i, n)).collect();
        assert_holds(&pool, &survivors, &format!("{n}, {case}"));
        assert_eq!(pool.x.capacity(), room, "{n}, {case}: the room is kept");
      }
    }
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
