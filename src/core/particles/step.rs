//! The engine's particle steps: every game-time emitter spawns, moves and
//! removes its particles once per fixed step (`step_particles`), and every
//! unscaled emitter on real time once a frame (`step_unscaled_particles`).

use std::f32::consts::TAU;
use std::time::Duration;

use glam::{Mat2, Vec2};

use super::effect::{Burst, EmitShape, ParticleEffect, Space};
use super::emitter::{EmitterFinished, Finish, ParticleEmitter, Pending};
use super::forces::{Acting, Own, Surroundings};
use super::pool::{EmitterPool, NewParticle, STUCK};
use super::rng::ParticleRng;
use super::space::{self, LocalFrame};
use super::store::ParticleStore;
use crate::core::asset::{AssetId, AssetServer, Assets};
use crate::core::ecs::kernel::EcsSystemState;
use crate::core::ecs::{
  DrawLift, Entity, GlobalTransform, Local, Messages, Mut, PreviousGlobalTransform, Query, Res,
  ResMut, World, entity_order,
};
use crate::core::hash::FastSet;
use crate::core::time::Time;

/// How far past a step's end a burst may fall, or the rate's next particle,
/// and still be due in that step: step times summed in floats never land
/// exactly on a burst's time
const DUE: f64 = 1e-4;

/// The most cycles of a looping effect one step runs through (a very short
/// cycle at a high emitter speed); the rest of the step's time is skipped
const MAX_CYCLES: u32 = 1_000;

/// The most burst firings one emitter works through in a step (bursts a
/// hair apart); the rest are counted as fired, spawning nothing
const MAX_FIRINGS: u32 = 10_000;

/// The most steps an unscaled emitter takes in a frame; a longer frame's
/// time beyond them is dropped
pub(crate) const MAX_UNSCALED_STEPS: u32 = 8;

/// The most of a looping cycle a prewarm runs, in seconds
const PREWARM_SECONDS: f32 = 10.0;

/// The most steps a prewarm takes (10 s of 60 Hz steps): one that would
/// take more takes this many longer ones
const MAX_PREWARM_STEPS: u32 = 600;

/// An effect as the step runs it: the effect with every problem clamped
/// (`sanitized`), for the asset version it was made from, with its angles
/// in radians
#[derive(Debug)]
pub(crate) struct Compiled {
  pub(crate) effect: ParticleEffect,
  pub(crate) version: u64,
  /// `direction_deg` in radians
  pub(crate) direction: f32,
  /// `spread_deg` in radians
  pub(crate) spread: f32,
  /// Whether `speed_over_life` changes over life (a constant one leaves
  /// velocities alone)
  pub(crate) speed_varies: bool,
  /// The largest a particle draws: its largest start size times the size
  /// curve's largest value (the drawer grows a pool's box by it)
  pub(crate) largest_size: f32,
}

impl Compiled {
  /// `effect`'s `version`, clamped, after one warning naming `name()` and
  /// what was clamped if it had problems
  pub(crate) fn new(effect: &ParticleEffect, version: u64, name: impl FnOnce() -> String) -> Self {
    let problems = effect.problems();
    if !problems.is_empty() {
      crate::engine_warn!(
        module: "Particles",
        "Clamped the values not allowed in {}: {}",
        name(),
        problems.join("; ")
      );
    }
    let effect = effect.sanitized();
    let speed_varies = effect
      .speed_over_life
      .keys()
      .windows(2)
      .any(|pair| pair[0].value != pair[1].value);
    let largest_size = effect.size.largest().max(0.0) * effect.size_over_life.largest();
    Self {
      direction: effect.direction_deg.to_radians(),
      spread: effect.spread_deg.to_radians(),
      speed_varies,
      largest_size,
      version,
      effect,
    }
  }
}

/// The emitters the steps run: enabled ones (the default filter leaves
/// `Disabled` out), where they stand
type Emitters<'w, 's> = Query<
  'w,
  's,
  (
    Entity,
    &'static mut ParticleEmitter,
    &'static GlobalTransform,
    Option<&'static DrawLift>,
    Option<&'static PreviousGlobalTransform>,
  ),
>;

type StepParams<'w, 's> = (
  Option<ResMut<'w, ParticleStore>>,
  Option<Res<'w, Assets<ParticleEffect>>>,
  Option<Res<'w, AssetServer>>,
  Option<Res<'w, Time>>,
  Option<ResMut<'w, Messages<EmitterFinished>>>,
  Emitters<'w, 's>,
);

/// What a step keeps between runs, so a steady step allocates nothing:
/// its parameters' state (the emitter query among them), the emitters in
/// entity order, per-particle scratch, the emitters going, and the
/// attractors and colliders it gathers (with their queries)
#[derive(Default)]
pub(crate) struct StepState {
  params: Option<EcsSystemState<StepParams<'static, 'static>>>,
  order: Vec<(u32, Entity)>,
  scratch: Scratch,
  /// Emitters that finished in an earlier step and asked to despawn then
  despawn: Vec<Entity>,
  surroundings: Surroundings,
}

/// Per-particle scratch for the emitter being stepped
#[derive(Default)]
struct Scratch {
  /// Whether each particle lives on
  keep: Vec<bool>,
  /// The part of the step each particle spawned in it lived
  partial: Vec<f32>,
}

/// The fixed step's particles (`FixedSet::Particles`, after propagation and
/// floors): the enabled attractors are gathered, and the colliders if any
/// emitter collides; each enabled game-time emitter whose effect is
/// loaded, in entity order, does what the game asked of it, spawns what
/// its effect owes this step, and moves, ages and collides its particles,
/// and the dead leave. An emitter of a scene the stack holds (or of no
/// scene, while any is held) is frozen, as is one at speed 0; unscaled
/// emitters step in [`step_unscaled_particles`]. The step's collisions are
/// told as one batch, and the attractors' time moves on. Emitters that
/// finish report it, and those that finished in an earlier step and asked
/// to despawn go
pub(crate) fn step_particles(world: &mut World, mut state: Local<StepState>) {
  crate::profile_scope!("cpu:particles");
  let StepState {
    params,
    order,
    scratch,
    despawn,
    surroundings,
  } = &mut *state;
  let params = params.get_or_insert_with(|| EcsSystemState::new(world));
  surroundings.gather(world, false);
  let mut stepped = None;
  if let Ok((store, effects, server, time, mut finished, mut emitters)) = params.get_mut(world)
    && let (Some(mut store), Some(effects), Some(time)) = (store, effects, time)
  {
    let timestep = time.fixed.timestep();
    stepped = Some(timestep);
    store.step += 1;
    let step = store.step;
    forget_unloaded(&mut store, &effects);

    order.clear();
    order.extend(
      emitters
        .iter()
        .filter(|(_, emitter, ..)| !emitter.unscaled)
        .map(|(entity, ..)| entity_order(&entity)),
    );
    order.sort_unstable();
    let mut pass = Pass {
      store: &mut store,
      effects: &effects,
      server: server.as_deref(),
      finished: &mut finished,
      scratch,
      despawn,
      surroundings,
    };
    for &(_, entity) in order.iter() {
      let Ok((entity, mut emitter, global, lift, previous)) = emitters.get_mut(entity) else {
        continue;
      };
      let Some(slot) = pass.ready(entity, &mut emitter) else {
        continue;
      };
      if pass
        .store
        .pools
        .get(slot)
        .is_none_or(|pool| pass.store.holds(pool.scope))
      {
        continue;
      }
      let dt = timestep * emitter.speed();
      if dt <= 0.0 {
        continue;
      }
      let continuous = pass
        .store
        .pools
        .get(slot)
        .is_some_and(|pool| pool.last_step == Some(step - 1));
      let place = Place {
        global,
        lift,
        previous,
      };
      pass.step_emitter(entity, slot, &mut emitter, place, dt, continuous);
      if let Some(pool) = pass.store.pools.get_mut(slot) {
        pool.last_step = Some(step);
      }
    }
  }
  surroundings.tell_collisions(world);
  if let Some(timestep) = stepped {
    surroundings.advance_attractors(world, timestep);
  }
  despawn_finished(world, despawn);
}

/// The frame's unscaled emitters (`PostUpdateSet::Extract`): each enabled
/// one whose effect is loaded, in entity order, adds the frame's real time
/// times its speed to its own clock, and takes a step of the fixed step's
/// length for each step's worth in it, at most [`MAX_UNSCALED_STEPS`] (the
/// time beyond them is dropped). Its pool keeps how far it is through its
/// next step, for the drawer. A paused clock, hitstop and held scenes do
/// not hold them; one at speed 0 is frozen
pub(crate) fn step_unscaled_particles(world: &mut World, mut state: Local<StepState>) {
  crate::profile_scope!("cpu:particles");
  let StepState {
    params,
    order,
    scratch,
    despawn,
    surroundings,
  } = &mut *state;
  let params = params.get_or_insert_with(|| EcsSystemState::new(world));
  surroundings.gather(world, true);
  if let Ok((store, effects, server, time, mut finished, mut emitters)) = params.get_mut(world)
    && let (Some(mut store), Some(effects), Some(time)) = (store, effects, time)
  {
    store.frame += 1;
    let frame = store.frame;
    order.clear();
    order.extend(
      emitters
        .iter()
        .filter(|(_, emitter, ..)| emitter.unscaled)
        .map(|(entity, ..)| entity_order(&entity)),
    );
    if !order.is_empty() {
      order.sort_unstable();
      forget_unloaded(&mut store, &effects);
      let timestep = time.fixed.timestep();
      let length = time.fixed.timestep_duration();
      let real = time.real.delta_duration();
      let mut pass = Pass {
        store: &mut store,
        effects: &effects,
        server: server.as_deref(),
        finished: &mut finished,
        scratch,
        despawn,
        surroundings,
      };
      for &(_, entity) in order.iter() {
        let Ok((entity, mut emitter, global, lift, previous)) = emitters.get_mut(entity) else {
          continue;
        };
        let Some(slot) = pass.ready(entity, &mut emitter) else {
          continue;
        };
        let speed = emitter.speed();
        if speed <= 0.0 || length.is_zero() {
          continue;
        }
        let Some(pool) = pass.store.pools.get_mut(slot) else {
          continue;
        };
        // Its stream spreads along its move from where it stood in its last
        // step, if it ran in the frame before
        let mut continuous = pool.last_step.is_some_and(|last| last + 1 >= frame);
        pool.last_step = Some(frame);
        pool.clock = pool.clock.saturating_add(scaled(real, speed));
        let place = Place {
          global,
          lift,
          previous,
        };
        let mut steps = 0;
        while steps < MAX_UNSCALED_STEPS
          && pass
            .store
            .pools
            .get(slot)
            .is_some_and(|pool| pool.clock >= length)
        {
          if !pass.step_emitter(entity, slot, &mut emitter, place, timestep, continuous) {
            break;
          }
          continuous = true;
          steps += 1;
          if let Some(pool) = pass.store.pools.get_mut(slot) {
            pool.clock = pool.clock.saturating_sub(length);
          }
        }
        if let Some(pool) = pass.store.pools.get_mut(slot) {
          if pool.clock >= length {
            // The steps past the most a frame takes are dropped
            let left = pool.clock.as_nanos() % length.as_nanos();
            pool.clock = Duration::from_nanos(u64::try_from(left).unwrap_or(0));
          }
          let fraction = pool.clock.as_secs_f64() / length.as_secs_f64();
          pool.fraction = fraction.clamp(0.0, 0.999_999) as f32;
        }
      }
    }
  }
  surroundings.tell_collisions(world);
  despawn_finished(world, despawn);
}

/// `real` time times an emitter's `speed` (0 to 100)
fn scaled(real: Duration, speed: f32) -> Duration {
  Duration::try_from_secs_f64(real.as_secs_f64() * f64::from(speed)).unwrap_or(Duration::ZERO)
}

/// Drops what the store kept for effects no longer loaded
fn forget_unloaded(store: &mut ParticleStore, effects: &Assets<ParticleEffect>) {
  store
    .compiled
    .retain(|id, _| effects.version(*id).is_some());
  store
    .warned_budget
    .retain(|id| effects.version(*id).is_some());
}

/// Despawns the emitters that finished in an earlier step and asked to go,
/// with their children
fn despawn_finished(world: &mut World, despawn: &mut Vec<Entity>) {
  for entity in despawn.drain(..) {
    // One may have gone with a parent despawned before it
    let _ = world.try_despawn(entity);
  }
}

/// Where an emitter stands as its step runs
#[derive(Clone, Copy)]
struct Place<'a> {
  global: &'a GlobalTransform,
  lift: Option<&'a DrawLift>,
  previous: Option<&'a PreviousGlobalTransform>,
}

/// What stepping one emitter shares with the others of its pass
struct Pass<'a, 'w> {
  store: &'a mut ParticleStore,
  effects: &'a Assets<ParticleEffect>,
  server: Option<&'a AssetServer>,
  finished: &'a mut Option<ResMut<'w, Messages<EmitterFinished>>>,
  scratch: &'a mut Scratch,
  despawn: &'a mut Vec<Entity>,
  surroundings: &'a mut Surroundings,
}

impl Pass<'_, '_> {
  /// The slot of `entity`'s emitter, when its effect is loaded: an effect
  /// still loading has no cycle yet, so there is nothing to do or warn
  /// about. An emitter added while the world had no store takes a slot now
  fn ready(&mut self, entity: Entity, emitter: &mut Mut<ParticleEmitter>) -> Option<usize> {
    let id = emitter.effect.id();
    self.effects.version(id)?;
    self.effects.get(id)?;
    if self
      .store
      .pool(emitter.slot)
      .is_none_or(|pool| pool.entity != entity)
    {
      emitter.slot = self.store.take_slot(entity);
    }
    let slot = emitter.slot as usize;
    (slot < self.store.pools.len()).then_some(slot)
  }

  /// Steps `entity`'s emitter, in `slot`, by `dt` seconds of its time: what
  /// the game asked of it, then its emission, then every particle; it
  /// reports a finish. `continuous`: it stepped in the step before, so its
  /// stream may spread along its move. Returns false, doing nothing else,
  /// when it finished in an earlier step and asked to despawn: it goes now
  fn step_emitter(
    &mut self,
    entity: Entity,
    slot: usize,
    emitter: &mut Mut<ParticleEmitter>,
    place: Place,
    dt: f32,
    continuous: bool,
  ) -> bool {
    if emitter.despawn_when_finished && emitter.finish == Finish::Reported && emitter.is_finished()
    {
      self.despawn.push(entity);
      return false;
    }
    let id = emitter.effect.id();
    let (Some(version), Some(effect)) = (self.effects.version(id), self.effects.get(id)) else {
      return true;
    };
    if self
      .store
      .compiled
      .get(&id)
      .is_none_or(|compiled| compiled.version != version)
    {
      let server = self.server;
      let compiled = Compiled::new(effect, version, || effect_name(server, id));
      self.store.compiled.insert(id, compiled);
    }
    let ParticleStore {
      pools,
      compiled,
      live,
      budget,
      warned_budget,
      ..
    } = &mut *self.store;
    let (Some(pool), Some(fx)) = (pools.get_mut(slot), compiled.get(&id)) else {
      return true;
    };

    // A new effect starts its emission over
    if pool.effect != Some(id) {
      pool.effect = Some(id);
      pool.time = None;
      pool.cycling = false;
      pool.carry = 0.0;
      pool.fired.clear();
      pool.ended = false;
    }
    if !pool.seeded {
      let seed = emitter.seed.or(fx.effect.seed).unwrap_or(pool.engine_seed);
      pool.rng = ParticleRng::new(seed);
      pool.noise_seed = (seed ^ (seed >> 32)) as u32;
      pool.seeded = true;
    }

    // What the game asked for since the last step
    let pending = emitter.pending;
    if pending != Pending::default() {
      emitter.pending = Pending::default();
    }
    if pending.clear || pending.restart {
      *live = live.saturating_sub(count(pool.main.len()));
      pool.main.clear();
      pool.bounds = None;
    }
    // Restarted, or played after its one cycle ended: the cycle from 0
    if pending.restart || (emitter.playing && pool.ended) {
      pool.time = Some(0.0);
      pool.cycling = true;
      pool.carry = 0.0;
      pool.fired.clear();
      pool.ended = false;
    }

    // Emitted where it draws: lifted by its floor's height, on a floor
    let position = place.global.translation() + place.lift.map_or(Vec2::ZERO, |drawn| drawn.lift);
    // Spread along the move only from where it was last step, and not
    // across a teleport: `skip_interpolation` marks the emitter (whenever it
    // is called) and sets an interpolated entity's `skip` (within the step)
    let teleported = emitter.teleported || place.previous.is_some_and(|p| p.skip);
    if emitter.teleported {
      emitter.teleported = false;
    }
    let spread = continuous && !teleported;
    let from = match pool.last {
      Some(last) if spread => last,
      _ => position,
    };

    let linear = place.global.affine().matrix2;
    // A `Local` effect's particles live in the emitter's frame
    let local = (fx.effect.space == Space::Local).then(|| LocalFrame::new(position, linear));
    space::settle(pool, local.as_ref());
    let mut frame = Frame {
      from,
      to: position,
      linear,
      unit: unit_axes(linear),
      dt,
      spread,
      local,
    };
    let looping = fx.effect.looping && !emitter.once;
    let mut playing = emitter.playing;
    let mut stepper = Stepper {
      id,
      server: self.server,
      live,
      budget: *budget,
      warned_budget,
      scratch: self.scratch,
      acting: self.surroundings.acting(entity, &fx.effect),
    };
    // A prewarmed loop starts as if it had been playing where it stands
    if fx.effect.prewarm && looping && playing && pool.time.is_none() {
      stepper.prewarm(pool, fx, &frame, &mut playing);
      frame.from = position;
      frame.spread = true;
    }
    stepper.particles(pool, fx, &frame, looping, &mut playing, pending.burst);
    pool.last = Some(position);
    pool.local = local;

    let alive = count(pool.main.len());
    if emitter.alive != alive {
      emitter.alive = alive;
    }
    if emitter.playing != playing {
      emitter.playing = playing;
    }
    // Not playing, nothing alive and no burst to come: reported once
    if emitter.is_finished() && emitter.finish == Finish::Armed {
      emitter.finish = Finish::Reported;
      if let Some(messages) = self.finished.as_mut() {
        messages.write(EmitterFinished { entity });
      }
    }
    true
  }
}

/// What one emitter's steps share while it is stepped: its effect, the
/// store's live count, budget and budget warnings, the scratch, and what
/// acts on its particles
struct Stepper<'a> {
  id: AssetId<ParticleEffect>,
  server: Option<&'a AssetServer>,
  live: &'a mut u32,
  budget: u32,
  warned_budget: &'a mut FastSet<AssetId<ParticleEffect>>,
  scratch: &'a mut Scratch,
  acting: Acting<'a>,
}

impl Stepper<'_> {
  /// One step of `pool`'s particles along `frame`: its emission (while
  /// `playing`; one cycle unless `looping`) and `burst` more, within its
  /// effect's `max_particles` and the budget, are spawned, and every
  /// particle moves on. A `Local` effect's box is kept in the world, round
  /// its particles in this step's frame and in the last's
  fn particles(
    &mut self,
    pool: &mut EmitterPool,
    fx: &Compiled,
    frame: &Frame,
    looping: bool,
    playing: &mut bool,
    burst: u32,
  ) {
    let room = (fx.effect.max_particles as usize).saturating_sub(pool.main.len());
    let distance = f64::from(frame.to.distance(frame.from));
    let distance = if distance.is_finite() { distance } else { 0.0 };
    let dt = f64::from(frame.dt);
    let emitted = emission(pool, &fx.effect, looping, playing, dt, room, distance);
    let wanted = emitted.saturating_add((burst as usize).min(room.saturating_sub(emitted)));
    let free = self.budget.saturating_sub(*self.live) as usize;
    let granted = wanted.min(free);
    if granted < wanted && self.warned_budget.insert(self.id) {
      crate::engine_warn!(
        module: "Particles",
        "The particle budget of {} is full: spawns of {} are dropped until particles die (raise it with EngineBuilder::particle_budget)",
        self.budget,
        effect_name(self.server, self.id)
      );
    }
    let born = pool.main.len();
    let Scratch { keep, partial } = &mut *self.scratch;
    partial.clear();
    if frame.to.is_finite() && frame.from.is_finite() {
      spawn(pool, fx, frame, granted, partial);
    }
    *self.live = self.live.saturating_add(count(pool.main.len() - born));
    let own = Own {
      centre: if frame.local.is_some() {
        Vec2::ZERO
      } else {
        frame.to
      },
      noise_seed: pool.noise_seed,
    };
    let died = advance(
      pool,
      fx,
      frame.dt,
      (born, partial),
      keep,
      (&mut self.acting, &own),
    );
    *self.live = self.live.saturating_sub(count(died));
    if let Some(now) = &frame.local {
      let then = pool.local.unwrap_or(*now);
      pool.bounds = pool.bounds.map(|inside| {
        let (a, b) = (now.world_box(inside), then.world_box(inside));
        (a.0.min(b.0), a.1.max(b.1))
      });
    }
  }

  /// Runs the first `min(duration, 10)` seconds of `pool`'s looping cycle
  /// where `frame` ends, before the emitter's first step: in steps of
  /// `frame.dt`, or [`MAX_PREWARM_STEPS`] longer ones when it would take
  /// more, its start delay skipped and the collisions it finds told to no
  /// one
  fn prewarm(&mut self, pool: &mut EmitterPool, fx: &Compiled, frame: &Frame, playing: &mut bool) {
    pool.time = Some(0.0);
    pool.cycling = true;
    let seconds = fx.effect.duration.min(PREWARM_SECONDS);
    let (steps, dt) = prewarm_steps(seconds, frame.dt);
    let still = Frame {
      from: frame.to,
      dt,
      spread: true,
      ..*frame
    };
    let told = self.acting.told();
    for _ in 0..steps {
      self.particles(pool, fx, &still, true, playing, 0);
    }
    self.acting.forget_told_since(told);
  }
}

/// How many steps a prewarm of `seconds` takes for an emitter whose steps
/// are `dt` long, and their length: as many of `dt` as it holds (at least
/// one), or [`MAX_PREWARM_STEPS`] longer ones when that is more
fn prewarm_steps(seconds: f32, dt: f32) -> (u32, f32) {
  let wanted = (seconds / dt).round().max(1.0);
  if wanted > MAX_PREWARM_STEPS as f32 {
    (MAX_PREWARM_STEPS, seconds / MAX_PREWARM_STEPS as f32)
  } else {
    (wanted as u32, dt)
  }
}

fn count(n: usize) -> u32 {
  u32::try_from(n).unwrap_or(u32::MAX)
}

/// How a warning names an effect: by its asset path, or as made in code
fn effect_name(server: Option<&AssetServer>, id: AssetId<ParticleEffect>) -> String {
  server.and_then(|server| server.path(id)).map_or_else(
    || String::from("an effect added in code"),
    |path| format!("the particle effect \"{path}\""),
  )
}

/// Moves `pool`'s cycle on by `dt` seconds and counts the particles it owes
/// for them, at most `room`: the whole particles of the rate and of the
/// `distance` the emitter moved (their fraction carried), and each burst
/// firing due by the step's end, every cycle and repeat included. Nothing
/// while not `playing`. The start delay comes first; a one-cycle run (not
/// `looping`) ends with its cycle, clearing `playing`, and a looping one
/// starts its bursts over
fn emission(
  pool: &mut EmitterPool,
  effect: &ParticleEffect,
  looping: bool,
  playing: &mut bool,
  dt: f64,
  room: usize,
  distance: f64,
) -> usize {
  if !*playing {
    return 0;
  }
  let duration = f64::from(effect.duration);
  let t0 = *pool.time.get_or_insert(-f64::from(effect.start_delay));
  let mut t1 = t0 + dt;
  pool.fired.resize(effect.bursts.len(), 0);

  // The rate runs from the first cycle's start: to its end for one cycle,
  // on through every cycle when looping (where time below 0 after the
  // first is a cycle begun early, not the delay)
  let active = if looping {
    let delay = if pool.cycling { 0.0 } else { (-t0).max(0.0) };
    (dt - delay).max(0.0)
  } else {
    let end = if t1 + DUE >= duration { duration } else { t1 };
    (end - t0.max(0.0)).max(0.0)
  };
  if t1 >= 0.0 {
    pool.cycling = true;
  }
  pool.carry += f64::from(effect.rate) * active;
  // The move counts for the step's part inside the cycle, as the rate does
  if dt > 0.0 {
    pool.carry += f64::from(effect.rate_per_distance) * distance * (active / dt);
  }
  let whole = (pool.carry + DUE).floor().max(0.0);
  pool.carry = (pool.carry - whole).max(0.0);
  let mut wanted = (whole as usize).min(room);

  let mut firings = 0;
  let mut cycles = 0;
  loop {
    for (burst, fired) in effect.bursts.iter().zip(pool.fired.iter_mut()) {
      let due = due_firings(burst, t1, duration);
      if burst.count.largest() < 0.5 {
        // Never a particle: nothing to draw
        *fired = (*fired).max(due);
        continue;
      }
      while *fired < due {
        if wanted >= room || firings >= MAX_FIRINGS {
          *fired = due;
          break;
        }
        *fired += 1;
        firings += 1;
        let n = burst.count.sample(&mut pool.rng).round().max(0.0) as usize;
        wanted = wanted.saturating_add(n).min(room);
      }
    }
    if t1 + DUE < duration {
      break;
    }
    // The cycle ends in this step
    if !looping {
      pool.ended = true;
      *playing = false;
      break;
    }
    t1 -= duration;
    pool.fired.iter_mut().for_each(|fired| *fired = 0);
    cycles += 1;
    if cycles >= MAX_CYCLES {
      t1 = t1.rem_euclid(duration);
      break;
    }
  }
  pool.time = Some(t1);
  wanted
}

/// How many of `burst`'s firings fall by `t` seconds into a cycle of
/// `duration`: its first at its time, each repeat `interval` later, as many
/// as its cycles allow (0: every one) and only those before the cycle ends
fn due_firings(burst: &Burst, t: f64, duration: f64) -> u32 {
  let start = f64::from(burst.time).max(0.0);
  if start > t + DUE || start >= duration {
    return 0;
  }
  let interval = f64::from(burst.interval);
  if burst.cycles == 1 || interval <= 0.0 {
    return 1;
  }
  let by_time = ((t + DUE - start) / interval).floor() + 1.0;
  let in_cycle = ((duration - start) / interval).ceil();
  let mut due = by_time.min(in_cycle);
  if burst.cycles > 0 {
    due = due.min(f64::from(burst.cycles));
  }
  due.clamp(0.0, f64::from(u32::MAX)) as u32
}

/// Where and how one step's particles start: along the move from `from` to
/// `to` when `spread`, else all at `to`, the shape through the emitter's
/// `linear` part and directions through its `unit` axes (turned and
/// mirrored, not stretched). A `Local` effect's start at the emitter in
/// its frame (`local`)
#[derive(Clone, Copy)]
struct Frame {
  from: Vec2,
  to: Vec2,
  linear: Mat2,
  unit: Mat2,
  dt: f32,
  spread: bool,
  local: Option<LocalFrame>,
}

impl Frame {
  /// `share` of the emitter's velocity in the step (its move over `dt`),
  /// which a `World` effect's particles start with: none for a `Local`
  /// effect's, which the frame carries, none without a move to spread
  /// along (a teleport, a first step), and none past what a number holds
  fn inherited(&self, share: f32) -> Vec2 {
    if self.local.is_some() || !self.spread {
      return Vec2::ZERO;
    }
    let velocity = (self.to - self.from) / self.dt * share;
    if velocity.is_finite() {
      velocity
    } else {
      Vec2::ZERO
    }
  }
}

/// Spawns `n` particles of `fx` into `pool`, in order, recording in
/// `partial` the part of the step each lived: spread along the move, the
/// `i`th of `n` is placed at `(i + 1) / n` of it and lived the rest of the
/// step, so a moving stream leaves an even line. Each starts with the
/// effect's share of the emitter's velocity; a `Local` effect's start at
/// the emitter, in its frame
fn spawn(pool: &mut EmitterPool, fx: &Compiled, frame: &Frame, n: usize, partial: &mut Vec<f32>) {
  let effect = &fx.effect;
  let birth_speed = effect.speed_over_life.lookup(0.0);
  let inherited = frame.inherited(effect.inherit_velocity);
  let to_local = frame.local.map(|local| local.axes.transpose());
  for i in 0..n {
    let (at, lived) = if frame.spread {
      let f = (i + 1) as f32 / n as f32;
      (
        frame.from + (frame.to - frame.from) * f,
        (1.0 - f) * frame.dt,
      )
    } else {
      (frame.to, 0.0)
    };
    let rng = &mut pool.rng;
    let point = shape_point(&effect.shape, rng);
    let heading = if effect.outward {
      point
        .try_normalize()
        .unwrap_or_else(|| Vec2::from_angle(rng.range(0.0, TAU)))
    } else {
      let half = fx.spread * 0.5;
      Vec2::from_angle(fx.direction + rng.range(-half, half))
    };
    let heading = (frame.unit * heading).try_normalize().unwrap_or(heading);
    let speed = effect.speed.sample(rng) * birth_speed;
    let lifetime = effect.lifetime.sample(rng);
    let size = effect.size.sample(rng);
    let rotation = effect.rotation_deg.sample(rng).to_radians();
    let spin = effect.angular_velocity_deg.sample(rng).to_radians();
    let color = effect.color.sample(rng);
    let seed = (rng.next_u64() >> 32) as u32;
    let (offset, velocity) = (frame.linear * point, heading * speed);
    let (position, velocity) = match to_local {
      Some(to_local) => (to_local * offset, to_local * velocity),
      None => (at + offset, velocity + inherited),
    };
    pool.main.push(&NewParticle {
      position,
      velocity,
      lifetime,
      size,
      rotation,
      spin,
      color,
      seed,
    });
    partial.push(lived);
  }
}

/// A point of `shape` in the emitter's space: uniform over its area (or
/// its edge, at a thickness of 0)
fn shape_point(shape: &EmitShape, rng: &mut ParticleRng) -> Vec2 {
  match *shape {
    EmitShape::Point => Vec2::ZERO,
    EmitShape::Circle {
      radius,
      arc_deg,
      thickness,
    } => {
      // The arc runs from 0 (right) towards 90 degrees (down)
      let angle = rng.range(0.0, arc_deg.to_radians());
      let inner = (1.0 - thickness) * (1.0 - thickness);
      Vec2::from_angle(angle) * radius * rng.range(inner, 1.0).sqrt()
    }
    EmitShape::Rect { size, thickness } => rect_point(size * 0.5, thickness, rng),
    EmitShape::Line { length } => Vec2::new(rng.range(-length * 0.5, length * 0.5), 0.0),
  }
}

/// A point of the band between a box of half-size `half` and the box
/// `thickness` of the way in from it: the strips above and below the full
/// width, the sides between them, each by its area; the outline by length
/// when the band has no area
fn rect_point(half: Vec2, thickness: f32, rng: &mut ParticleRng) -> Vec2 {
  let inner = half * (1.0 - thickness);
  let ends = 4.0 * half.x * (half.y - inner.y);
  let sides = 4.0 * (half.x - inner.x) * inner.y;
  let area = ends + sides;
  if area.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
    return outline_point(half, rng);
  }
  let pick = rng.range(0.0, area);
  if pick < ends {
    let x = rng.range(-half.x, half.x);
    let y = rng.range(inner.y, half.y);
    Vec2::new(x, if pick < ends * 0.5 { -y } else { y })
  } else {
    let x = rng.range(inner.x, half.x);
    let y = rng.range(-inner.y, inner.y);
    Vec2::new(if pick - ends < sides * 0.5 { -x } else { x }, y)
  }
}

/// A point on the outline of a box of half-size `half`, by length
fn outline_point(half: Vec2, rng: &mut ParticleRng) -> Vec2 {
  let (w, h) = (2.0 * half.x, 2.0 * half.y);
  let length = 2.0 * (w + h);
  if length.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
    return Vec2::ZERO;
  }
  let d = rng.range(0.0, length);
  if d < w {
    Vec2::new(-half.x + d, -half.y)
  } else if d < w + h {
    Vec2::new(half.x, -half.y + (d - w))
  } else if d < 2.0 * w + h {
    Vec2::new(half.x - (d - w - h), half.y)
  } else {
    Vec2::new(-half.x, half.y - (d - 2.0 * w - h))
  }
}

/// The emitter's axes at unit length: its rotation and any mirroring, with
/// no stretch, so a scale moves the shape but leaves speeds alone. An axis
/// scaled to nothing is made square to the other; both, and it is the
/// identity
fn unit_axes(linear: Mat2) -> Mat2 {
  match (linear.x_axis.try_normalize(), linear.y_axis.try_normalize()) {
    (Some(x), Some(y)) => Mat2::from_cols(x, y),
    (Some(x), None) => Mat2::from_cols(x, x.perp()),
    (None, Some(y)) => Mat2::from_cols(-y.perp(), y),
    (None, None) => Mat2::IDENTITY,
  }
}

/// Steps `pool`'s particles: those before `born` by `dt`, those spawned
/// this step by the part of it they lived (`partial`). Each ages, takes
/// the attractors' pull (`acting`), its gravity, its drag and its speed
/// curve's change, spins, moves from where it was, and meets the colliders
/// if it lives on; a stuck one only ages. The dead then leave in order,
/// and the box round the rest is kept. Returns how many died
fn advance(
  pool: &mut EmitterPool,
  fx: &Compiled,
  dt: f32,
  (born, partial): (usize, &[f32]),
  keep: &mut Vec<bool>,
  (acting, own): (&mut Acting, &Own),
) -> usize {
  let effect = &fx.effect;
  let curve = &effect.speed_over_life;
  let p = &mut pool.main;
  let n = p.len();
  keep.clear();
  let mut min = Vec2::splat(f32::INFINITY);
  let mut max = Vec2::splat(f32::NEG_INFINITY);
  for i in 0..n {
    let step = if i < born {
      dt
    } else {
      partial.get(i - born).copied().unwrap_or(0.0)
    };
    let was = p.age[i];
    let age = was + step;
    p.age[i] = age;
    let mut alive = age < p.lifetime[i];
    if p.flags[i] & STUCK != 0 {
      // Stuck where it hit: it only ages
      p.px[i] = p.x[i];
      p.py[i] = p.y[i];
    } else {
      let from = Vec2::new(p.x[i], p.y[i]);
      let mut velocity = Vec2::new(p.vx[i], p.vy[i]);
      velocity += acting.pull(from) * step;
      velocity += effect.gravity * step;
      velocity += own.force(effect, p.seed[i], from, age) * step;
      velocity *= (1.0 - effect.drag * step).max(0.0);
      if fx.speed_varies {
        let life = p.lifetime[i];
        let before = curve.lookup(was / life);
        if before != 0.0 {
          let ratio = curve.lookup(age / life) / before;
          if ratio.is_finite() {
            velocity *= ratio;
          }
        }
      }
      p.rotation[i] += p.spin[i] * step;
      p.px[i] = from.x;
      p.py[i] = from.y;
      p.x[i] += velocity.x * step;
      p.y[i] += velocity.y * step;
      p.vx[i] = velocity.x;
      p.vy[i] = velocity.y;
      // One that dies of age this step meets nothing
      if alive {
        alive = acting.collide(p, i, from);
      }
    }
    keep.push(alive);
    if alive {
      let (now, then) = (Vec2::new(p.x[i], p.y[i]), Vec2::new(p.px[i], p.py[i]));
      min = min.min(now).min(then);
      max = max.max(now).max(then);
    }
  }
  p.retain(|i| keep.get(i).copied().unwrap_or(false));
  pool.bounds = (min.x <= max.x && min.y <= max.y).then_some((min, max));
  n - p.len()
}
