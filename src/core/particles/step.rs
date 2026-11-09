//! The engine's particle step: every emitter spawns, moves and removes its
//! particles once per fixed step.

use std::f32::consts::TAU;

use glam::{Mat2, Vec2};

use super::effect::{Burst, EmitShape, ParticleEffect};
use super::emitter::ParticleEmitter;
use super::pool::{EmitterPool, NewParticle};
use super::rng::ParticleRng;
use super::store::ParticleStore;
use crate::core::asset::{AssetId, AssetServer, Assets};
use crate::core::ecs::kernel::EcsSystemState;
use crate::core::ecs::{
  DrawLift, Entity, GlobalTransform, Local, PreviousGlobalTransform, Query, Res, ResMut, World,
  entity_order,
};
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
}

impl Compiled {
  /// `effect`'s `version`, clamped, after one warning naming `name()` and
  /// what was clamped if it had problems
  fn new(effect: &ParticleEffect, version: u64, name: impl FnOnce() -> String) -> Self {
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
    Self {
      direction: effect.direction_deg.to_radians(),
      spread: effect.spread_deg.to_radians(),
      speed_varies,
      version,
      effect,
    }
  }
}

/// The emitters the step runs: enabled ones (the default filter leaves
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
  Emitters<'w, 's>,
);

/// What the step keeps between runs, so a steady step allocates nothing:
/// its parameters' state (the emitter query among them), the emitters in
/// entity order, and per-particle scratch
#[derive(Default)]
pub(crate) struct StepState {
  params: Option<EcsSystemState<StepParams<'static, 'static>>>,
  order: Vec<(u32, Entity)>,
  /// Whether each particle of the emitter being stepped lives on
  keep: Vec<bool>,
  /// The part of the step each particle spawned in it lived
  partial: Vec<f32>,
}

/// The fixed step's particles (`FixedSet::Particles`, after propagation and
/// floors): each enabled emitter whose effect is loaded, in entity order,
/// spawns what its effect owes this step and moves and ages its particles,
/// and the dead leave. An emitter of a scene the stack holds (or of no
/// scene, while any is held) is frozen, as is one at speed 0
pub(crate) fn step_particles(world: &mut World, mut state: Local<StepState>) {
  let StepState {
    params,
    order,
    keep,
    partial,
  } = &mut *state;
  let params = params.get_or_insert_with(|| EcsSystemState::new(world));
  let Ok((store, effects, server, time, mut emitters)) = params.get_mut(world) else {
    return;
  };
  let (Some(mut store), Some(effects), Some(time)) = (store, effects, time) else {
    return;
  };
  let store = &mut *store;
  let timestep = time.fixed.timestep();
  store.step += 1;
  let step = store.step;
  // What was kept for effects no longer loaded goes
  store
    .compiled
    .retain(|id, _| effects.version(*id).is_some());
  store
    .warned_budget
    .retain(|id| effects.version(*id).is_some());

  order.clear();
  order.extend(emitters.iter().map(|(entity, ..)| entity_order(&entity)));
  order.sort_unstable();
  for &(_, entity) in order.iter() {
    let Ok((entity, mut emitter, global, lift, previous)) = emitters.get_mut(entity) else {
      continue;
    };
    // An effect still loading has no cycle yet: nothing to do or warn about
    let id = emitter.effect.id();
    let (Some(version), Some(effect)) = (effects.version(id), effects.get(id)) else {
      continue;
    };
    if store
      .pool(emitter.slot)
      .is_none_or(|pool| pool.entity != entity)
    {
      // Added while the world had no store
      emitter.slot = store.take_slot(entity);
    }
    let slot = emitter.slot as usize;
    if store
      .pools
      .get(slot)
      .is_none_or(|pool| store.holds(pool.scope))
    {
      continue;
    }
    let dt = timestep * emitter.speed();
    if dt <= 0.0 {
      continue;
    }
    if store
      .compiled
      .get(&id)
      .is_none_or(|compiled| compiled.version != version)
    {
      let compiled = Compiled::new(effect, version, || effect_name(server.as_deref(), id));
      store.compiled.insert(id, compiled);
    }
    let ParticleStore {
      pools,
      compiled,
      live,
      budget,
      warned_budget,
      ..
    } = &mut *store;
    let (Some(pool), Some(fx)) = (pools.get_mut(slot), compiled.get(&id)) else {
      continue;
    };

    // A new effect starts its emission over
    if pool.effect != Some(id) {
      pool.effect = Some(id);
      pool.time = None;
      pool.cycling = false;
      pool.carry = 0.0;
      pool.fired.clear();
      pool.playing = true;
    }
    if !pool.seeded {
      let seed = emitter.seed.or(fx.effect.seed).unwrap_or(pool.engine_seed);
      pool.rng = ParticleRng::new(seed);
      pool.seeded = true;
    }

    // Emitted where it draws: lifted by its floor's height, on a floor
    let position = global.translation() + lift.map_or(Vec2::ZERO, |drawn| drawn.lift);
    // Spread along the move only from where it was last step, and not
    // across a teleport: `skip_interpolation` marks the emitter (whenever it
    // is called) and sets an interpolated entity's `skip` (within the step)
    let teleported = emitter.teleported || previous.is_some_and(|p| p.skip);
    if emitter.teleported {
      emitter.teleported = false;
    }
    let spread = pool.last_step == Some(step - 1) && !teleported;
    let from = match pool.last {
      Some(last) if spread => last,
      _ => position,
    };

    let room = (fx.effect.max_particles as usize).saturating_sub(pool.main.len());
    let wanted = emission(pool, &fx.effect, f64::from(dt), room);
    let free = budget.saturating_sub(*live) as usize;
    let granted = wanted.min(free);
    if granted < wanted && warned_budget.insert(id) {
      crate::engine_warn!(
        module: "Particles",
        "The particle budget of {} is full: spawns of {} are dropped until particles die (raise it with EngineBuilder::particle_budget)",
        budget,
        effect_name(server.as_deref(), id)
      );
    }
    let born = pool.main.len();
    partial.clear();
    if position.is_finite() && from.is_finite() {
      let linear = global.affine().matrix2;
      let frame = Frame {
        from,
        to: position,
        linear,
        unit: unit_axes(linear),
        dt,
        spread,
      };
      spawn(pool, fx, &frame, granted, partial);
    }
    *live = live.saturating_add(count(pool.main.len() - born));
    let died = advance(pool, fx, dt, born, partial, keep);
    *live = live.saturating_sub(count(died));
    pool.last = Some(position);
    pool.last_step = Some(step);

    let alive = count(pool.main.len());
    if emitter.alive != alive {
      emitter.alive = alive;
    }
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
/// for them, at most `room`: the rate's whole particles (its fraction
/// carried) and each burst firing due by the step's end, every cycle and
/// repeat included. The start delay comes first; a one-cycle effect stops
/// playing as its cycle ends, and a looping one starts its bursts over
fn emission(pool: &mut EmitterPool, effect: &ParticleEffect, dt: f64, room: usize) -> usize {
  if !pool.playing {
    return 0;
  }
  let duration = f64::from(effect.duration);
  let t0 = *pool.time.get_or_insert(-f64::from(effect.start_delay));
  let mut t1 = t0 + dt;
  pool.fired.resize(effect.bursts.len(), 0);

  // The rate runs from the first cycle's start: to its end for one cycle,
  // on through every cycle when looping (where time below 0 after the
  // first is a cycle begun early, not the delay)
  let active = if effect.looping {
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
    if !effect.looping {
      pool.playing = false;
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
/// mirrored, not stretched)
struct Frame {
  from: Vec2,
  to: Vec2,
  linear: Mat2,
  unit: Mat2,
  dt: f32,
  spread: bool,
}

/// Spawns `n` particles of `fx` into `pool`, in order, recording in
/// `partial` the part of the step each lived: spread along the move, the
/// `i`th of `n` is placed at `(i + 1) / n` of it and lived the rest of the
/// step, so a moving stream leaves an even line
fn spawn(pool: &mut EmitterPool, fx: &Compiled, frame: &Frame, n: usize, partial: &mut Vec<f32>) {
  let effect = &fx.effect;
  let birth_speed = effect.speed_over_life.lookup(0.0);
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
    let local = shape_point(&effect.shape, rng);
    let heading = if effect.outward {
      local
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
    pool.main.push(&NewParticle {
      position: at + frame.linear * local,
      velocity: heading * speed,
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
/// this step by the part of it they lived (`partial`). Each ages, takes its
/// gravity, its drag and its speed curve's change, spins, and moves from
/// where it was; the dead then leave in order, and the box round the rest
/// is kept. Returns how many died
fn advance(
  pool: &mut EmitterPool,
  fx: &Compiled,
  dt: f32,
  born: usize,
  partial: &[f32],
  keep: &mut Vec<bool>,
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
    let mut velocity = Vec2::new(p.vx[i], p.vy[i]);
    velocity += effect.gravity * step;
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
    p.px[i] = p.x[i];
    p.py[i] = p.y[i];
    p.x[i] += velocity.x * step;
    p.y[i] += velocity.y * step;
    p.vx[i] = velocity.x;
    p.vy[i] = velocity.y;
    let alive = age < p.lifetime[i];
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
