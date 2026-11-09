//! Sub-emitters: where an emitter's own particles died or met colliders in
//! a step (its triggers), and the bursts of sub-effect particles those
//! spawn into its pool, which then step by their own effect. One level
//! deep: a sub-effect's particles leave no triggers.

use glam::Vec2;

use super::effect::{ParticleEffect, Space, SubEmitter};
use super::forces::{Met, Own};
use super::pool::{EmitterPool, NewParticle, SubPool};
use super::rng::ParticleRng;
use super::space::{self, LocalFrame};
use super::step::{Compiled, Frame, Scratch, Stepper, advance, effect_name, sample_particle};
use super::store::ParticleStore;
use crate::core::asset::{AssetId, AssetServer, Assets};
use crate::core::hash::FastMap;

/// Where a particle was as it died or met a collider, and its velocity
/// then (before a collision's response), in its pool's space
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Trigger {
  pub(crate) at: Vec2,
  pub(crate) velocity: Vec2,
}

/// The triggers one step of an emitter's own particles left, for the
/// sub-effects ready to play them, kept between steps so a steady step
/// allocates nothing
#[derive(Debug, Default)]
pub(crate) struct Triggers {
  /// Deaths are kept: its `on_death` effect is ready to play
  on_death: bool,
  /// Collisions are kept: its `on_collision` effect is ready to play
  on_collision: bool,
  deaths: Vec<Trigger>,
  hits: Vec<Trigger>,
}

impl Triggers {
  /// Clears them for a step of `effect`'s particles, keeping deaths only if
  /// its `on_death` effect is compiled (loaded), and collisions likewise
  pub(crate) fn arm(
    &mut self,
    effect: &ParticleEffect,
    compiled: &FastMap<AssetId<ParticleEffect>, Compiled>,
  ) {
    let ready = |sub: &Option<SubEmitter>| {
      sub
        .as_ref()
        .is_some_and(|sub| compiled.contains_key(&sub.effect.id()))
    };
    self.on_death = ready(&effect.on_death);
    self.on_collision = ready(&effect.on_collision);
    self.deaths.clear();
    self.hits.clear();
  }

  /// A particle's life ended where it stood, `at`, going at `velocity`
  #[inline]
  pub(crate) fn died(&mut self, at: Vec2, velocity: Vec2) {
    if self.on_death {
      self.deaths.push(Trigger { at, velocity });
    }
  }

  /// A particle met the colliders: a contact that told of itself plays
  /// `on_collision` there, and a `Die` plays `on_death` there too
  #[inline]
  pub(crate) fn met(&mut self, met: &Met) {
    let Some(hit) = met.hit else {
      return;
    };
    if self.on_collision {
      self.hits.push(hit);
    }
    if !met.alive && self.on_death {
      self.deaths.push(hit);
    }
  }
}

/// Which of an emitter's sub-effects
#[derive(Clone, Copy)]
enum Kind {
  Death,
  Collision,
}

/// Compiles the sub-effects of the effect `id`, which plays in `slot`'s
/// pool, as any effect is (once per version): those it names, and those
/// the pool's sub-effect particles still play. A sub-effect with
/// sub-emitters of its own is warned about once per version: they are
/// ignored. One not loaded yet is left out, with no warning
pub(super) fn compile_subs(
  store: &mut ParticleStore,
  effects: &Assets<ParticleEffect>,
  server: Option<&AssetServer>,
  id: AssetId<ParticleEffect>,
  slot: usize,
) {
  let named = |sub: &Option<SubEmitter>| sub.as_ref().map(|sub| sub.effect.id());
  let (death, collision) = store.compiled.get(&id).map_or((None, None), |fx| {
    (named(&fx.effect.on_death), named(&fx.effect.on_collision))
  });
  let (pooled_death, pooled_collision) = store.pools.get(slot).map_or((None, None), |pool| {
    (pool.death.effect, pool.collision.effect)
  });
  let pooled = [pooled_death, pooled_collision]
    .into_iter()
    .filter(|pooled| *pooled != death && *pooled != collision);
  for sub in [death, collision].into_iter().chain(pooled).flatten() {
    if !store.compile(effects, server, sub) {
      continue;
    }
    let Some(compiled) = store.compiled.get_mut(&sub) else {
      continue;
    };
    let nested = [
      ("on_death", compiled.effect.on_death.is_some()),
      ("on_collision", compiled.effect.on_collision.is_some()),
    ];
    if compiled.warned_nested || !nested.iter().any(|(_, has)| *has) {
      continue;
    }
    compiled.warned_nested = true;
    let ignored = nested
      .iter()
      .filter(|(_, has)| *has)
      .map(|(field, _)| format!("{field}: a sub-effect's own sub-emitters are ignored"))
      .collect::<Vec<_>>()
      .join("; ");
    crate::engine_warn!(
      module: "Particles",
      "Played {} as a sub-effect: {ignored}",
      effect_name(server, sub)
    );
  }
}

impl Stepper<'_> {
  /// The sub-effects' part of a step of `pool`, once its own particles have
  /// stepped along `frame`: each trigger they left spawns its
  /// sub-emitter's burst into that sub pool, which then steps
  pub(super) fn subs(&mut self, pool: &mut EmitterPool, fx: &Compiled, frame: &Frame) {
    let EmitterPool {
      death,
      collision,
      rng,
      noise_seed,
      ..
    } = pool;
    let (on_death, on_collision) = (fx.effect.on_death.as_ref(), fx.effect.on_collision.as_ref());
    self.step_sub(death, on_death, Kind::Death, rng, *noise_seed, frame);
    self.step_sub(
      collision,
      on_collision,
      Kind::Collision,
      rng,
      *noise_seed,
      frame,
    );
  }

  /// One sub pool's step: it plays its parent's sub-effect (`emitter`) once
  /// that is compiled, or, until then (still loading) or once its parent
  /// has none, the last it had, so its particles live out their lives, and
  /// loses them once that effect is no longer loaded. Each of
  /// the step's triggers of its `kind` spawns `emitter.count` particles of
  /// the sub-effect (drawn from the emitter's stream, `rng`) where the
  /// particle was, in the sub-effect's space, each with its share of the
  /// particle's velocity, within the sub-effect's `max_particles` and the
  /// budget. Its particles then step by the sub-effect, those born now not
  /// moving, and leave no triggers
  fn step_sub(
    &mut self,
    sub: &mut SubPool,
    emitter: Option<&SubEmitter>,
    kind: Kind,
    rng: &mut ParticleRng,
    noise_seed: u32,
    frame: &Frame,
  ) {
    // A sub-effect still loading is adopted once it is compiled: until then
    // the particles the pool has go on by the effect they play
    if let Some(new) = emitter.map(|emitter| emitter.effect.id())
      && self.compiled.contains_key(&new)
    {
      sub.effect = Some(new);
    }
    let emitter = emitter.filter(|emitter| sub.effect == Some(emitter.effect.id()));
    let Scratch { keep, triggers, .. } = &mut *self.scratch;
    let fired = match kind {
      Kind::Death => &triggers.deaths,
      Kind::Collision => &triggers.hits,
    };
    if sub.particles.len() == 0 && fired.is_empty() {
      sub.local = None;
      return;
    }
    let Some((id, fx)) = sub
      .effect
      .and_then(|id| self.compiled.get(&id).map(|fx| (id, fx)))
    else {
      // Its effect is no longer loaded: nothing draws or steps them
      self.budget.gone(sub.particles.len());
      sub.clear();
      return;
    };
    let local = (fx.effect.space == Space::Local).then(|| LocalFrame::new(frame.to, frame.linear));
    space::settle(&mut sub.particles, sub.local, local.as_ref());
    let born = sub.particles.len();
    // Only an `emitter` arms its triggers
    if let Some(emitter) = emitter {
      let cap = fx.effect.max_particles as usize;
      let to_local = local.map(|local| local.axes.transpose());
      for trigger in fired {
        let wanted = emitter.count.sample(rng).round().max(0.0) as usize;
        let Some(trigger) = carried(trigger, frame.local.as_ref(), local.as_ref()) else {
          continue;
        };
        let room = cap.saturating_sub(sub.particles.len());
        let granted = self.budget.grant(id, wanted.min(room));
        let inherited = trigger.velocity * emitter.inherit_velocity;
        for _ in 0..granted {
          let new = sample_particle(fx, rng, frame.linear, frame.unit);
          let (offset, velocity) = match to_local {
            Some(to_local) => (to_local * new.position, to_local * new.velocity),
            None => (new.position, new.velocity),
          };
          sub.particles.push(&NewParticle {
            position: trigger.at + offset,
            velocity: velocity + inherited,
            ..new
          });
        }
        self.budget.born(granted);
      }
    }
    let own = Own {
      centre: if local.is_some() {
        Vec2::ZERO
      } else {
        frame.to
      },
      noise_seed,
    };
    let mut acting = self.surroundings.acting(self.entity, &fx.effect);
    let (died, bounds) = advance(
      &mut sub.particles,
      fx,
      frame.dt,
      (born, &[]),
      keep,
      (&mut acting, &own),
      None,
    );
    self.budget.gone(died);
    sub.bounds = space::world_bounds(bounds, local.as_ref(), sub.local);
    sub.local = local;
  }
}

/// `trigger`, left in the space of `from` (the emitter's frame this step,
/// or the world when `None`), in the space of `to`; `None` when it is not
/// finite
fn carried(
  trigger: &Trigger,
  from: Option<&LocalFrame>,
  to: Option<&LocalFrame>,
) -> Option<Trigger> {
  let (at, velocity) = match (from, to) {
    (Some(from), None) => (from.world_point(trigger.at), from.axes * trigger.velocity),
    (None, Some(to)) => (
      to.local_point(trigger.at),
      to.axes.transpose() * trigger.velocity,
    ),
    // One space: the world, or both this step's frame
    _ => (trigger.at, trigger.velocity),
  };
  (at.is_finite() && velocity.is_finite()).then_some(Trigger { at, velocity })
}

#[cfg(test)]
mod tests {
  use std::f32::consts::FRAC_PI_2;

  use glam::Mat2;

  use super::*;

  fn close(a: Vec2, b: Vec2) -> bool {
    (a - b).abs().max_element() < 1e-4
  }

  #[test]
  fn a_trigger_is_carried_between_spaces() {
    // A frame at (100, 50), turned a quarter
    let frame = LocalFrame::new(Vec2::new(100.0, 50.0), Mat2::from_angle(FRAC_PI_2));
    let local = Trigger {
      at: Vec2::new(10.0, 0.0),
      velocity: Vec2::new(5.0, 0.0),
    };
    let world = carried(&local, Some(&frame), None).expect("finite");
    assert!(close(world.at, Vec2::new(100.0, 60.0)), "{world:?}");
    assert!(close(world.velocity, Vec2::new(0.0, 5.0)), "{world:?}");
    let back = carried(&world, None, Some(&frame)).expect("finite");
    assert!(close(back.at, local.at) && close(back.velocity, local.velocity));
    assert_eq!(carried(&local, Some(&frame), Some(&frame)), Some(local));
    assert_eq!(carried(&local, None, None), Some(local));

    let lost = Trigger {
      at: Vec2::NAN,
      velocity: Vec2::ZERO,
    };
    assert_eq!(carried(&lost, None, None), None);
  }
}
