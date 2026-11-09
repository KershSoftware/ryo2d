//! What the world does to particles as they step: attractors pull them, and
//! colliders stop, bounce, hold or tell of them. Each pass of the step
//! gathers both once, into buffers kept between runs; each emitter then
//! picks the attractors on its layer and the colliders it can hit.

use glam::Vec2;

use super::attractor::{AttractorType, ParticleAttractor};
use super::collision::{
  CollisionConfig, CollisionResponse, MIN_BOUNCE_EVENT_SPEED, ParticleCollision,
};
use super::contact::{
  CONTACT_SKIN, ColliderQuery, Contact, ParticleCollider, candidate_colliders, gather_colliders,
};
use super::effect::{ParticleEffect, Space};
use super::emitter::ParticleEmitter;
use super::noise::noise_force;
use super::pool::{Pool, STUCK, TOUCHING};
use super::store::ParticleStore;
use super::sub::Trigger;
use crate::core::asset::{AssetId, Assets};
use crate::core::ecs::{Entity, GlobalTransform, QueryState, World};
use crate::core::messages::register_message;

/// An enabled attractor as a step sees it: where it stands and how it
/// pulls. It holds no heap data, so a step copies it
#[derive(Debug, Clone, Copy)]
struct Field {
  at: Vec2,
  kind: AttractorType,
  /// `ParticleAttractor::layer_mask`: 0 pulls every layer
  mask: u32,
  /// Its turbulence's time
  time: f32,
}

impl Field {
  /// Whether it pulls particles of an effect on `layer`
  fn pulls(&self, layer: u32) -> bool {
    self.mask == 0 || self.mask & layer != 0
  }
}

/// The attractors: read to gather them, written to advance their time
type AttractorQuery = QueryState<(&'static GlobalTransform, &'static mut ParticleAttractor)>;

/// What one pass of the step gathered from the world, the queries it
/// gathers with, and the collisions it found, all kept between runs so a
/// steady step allocates nothing
#[derive(Default)]
pub(crate) struct Surroundings {
  /// Every enabled emitter, read before a pass to see what it needs
  emitters: Option<QueryState<&'static ParticleEmitter>>,
  attractor_query: Option<AttractorQuery>,
  collider_query: Option<ColliderQuery>,
  /// Whether the pass has an emitter to step
  stepping: bool,
  /// The enabled attractors
  fields: Vec<Field>,
  /// The solid colliders, when some emitter collides
  colliders: Vec<ParticleCollider>,
  /// Their edges, in world space
  edges: Vec<(Vec2, Vec2)>,
  /// The attractors pulling the stepping emitter's layer
  near: Vec<Field>,
  /// The colliders the stepping emitter's particles can hit, in gather order
  candidates: Vec<ParticleCollider>,
  /// The collisions to tell of, written as one batch at the pass's end
  collisions: Vec<ParticleCollision>,
}

impl Surroundings {
  /// Gathers what a pass (of the `unscaled` emitters, or of the game-time
  /// ones) needs, before it runs: the enabled attractors, if it has an
  /// emitter to step, and the solid colliders, only if one of those
  /// emitters' effects collides
  pub(crate) fn gather(&mut self, world: &mut World, unscaled: bool) {
    let Self {
      emitters,
      attractor_query,
      collider_query,
      stepping,
      fields,
      colliders,
      edges,
      ..
    } = self;
    let emitters = emitters.get_or_insert_with(|| QueryState::new(world));
    let attractors = attractor_query.get_or_insert_with(|| QueryState::new(world));
    let collider_query = collider_query.get_or_insert_with(|| QueryState::new(world));
    let world: &World = world;
    fields.clear();
    colliders.clear();
    edges.clear();

    let (store, effects) = (
      world.get_resource::<ParticleStore>(),
      world.get_resource::<Assets<ParticleEffect>>(),
    );
    let mut collide = false;
    *stepping = false;
    for emitter in emitters.iter(world) {
      if emitter.unscaled != unscaled || emitter.speed() <= 0.0 {
        continue;
      }
      // A held scene's emitter does not step (unscaled ones are never held)
      let held = !unscaled
        && store.is_some_and(|store| {
          store
            .pool(emitter.slot)
            .is_some_and(|pool| store.holds(pool.scope))
        });
      if held {
        continue;
      }
      *stepping = true;
      if let (Some(store), Some(effects)) = (store, effects)
        && meets_colliders(store, effects, emitter)
      {
        collide = true;
        break;
      }
    }
    if !*stepping {
      return;
    }
    fields.extend(
      attractors
        .iter(world)
        .filter(|(_, attractor)| attractor.enabled)
        .map(|(global, attractor)| Field {
          at: global.translation(),
          kind: attractor.attractor_type,
          mask: attractor.layer_mask,
          time: attractor.time,
        }),
    );
    if collide {
      gather_colliders(world, collider_query, colliders, edges);
    }
  }

  /// What acts on the particles of the emitter on `emitter`, playing
  /// `effect`: the attractors pulling its layer and, if it collides, the
  /// colliders it can hit (never its own entity's). Nothing acts on a
  /// `Local` effect's, which live in their emitter's frame
  pub(crate) fn acting<'a>(
    &'a mut self,
    emitter: Entity,
    effect: &'a ParticleEffect,
  ) -> Acting<'a> {
    let Self {
      fields,
      colliders,
      edges,
      near,
      candidates,
      collisions,
      ..
    } = self;
    near.clear();
    candidates.clear();
    let world = effect.space == Space::World;
    if world {
      near.extend(fields.iter().filter(|field| field.pulls(effect.layer)));
    }
    let collide = effect.collision.as_ref().filter(|_| world).map(|config| {
      candidates.extend(candidate_colliders(colliders, emitter, config).copied());
      Collide {
        config,
        colliders: candidates,
        edges,
        emitter,
      }
    });
    Acting {
      fields: near,
      collide,
      collisions,
    }
  }

  /// How many collisions are waiting to be told
  pub(crate) fn told(&self) -> usize {
    self.collisions.len()
  }

  /// Forgets the collisions found since [`told`](Self::told) said `told`
  pub(crate) fn forget_told_since(&mut self, told: usize) {
    self.collisions.truncate(told);
  }

  /// Writes the pass's collisions as one batch
  pub(crate) fn tell_collisions(&mut self, world: &mut World) {
    if self.collisions.is_empty() {
      return;
    }
    register_message::<ParticleCollision>(world);
    let _ = world.write_message_batch(self.collisions.drain(..));
  }

  /// Advances the attractors' time by the game-time pass's `dt`, as their
  /// `update` does, if the pass had an emitter to step
  pub(crate) fn advance_attractors(&mut self, world: &mut World, dt: f32) {
    if !self.stepping {
      return;
    }
    let Some(query) = self.attractor_query.as_mut() else {
      return;
    };
    for (_, mut attractor) in query.iter_mut(world) {
      // Only a turbulence keeps time: the rest are left unchanged
      if matches!(attractor.attractor_type, AttractorType::Turbulence { .. }) {
        attractor.update(dt);
      }
    }
  }
}

/// Whether `emitter`'s particles meet colliders: its effect's, or those of
/// a sub-effect, one its effect names or one its pool's sub-effect
/// particles still play
fn meets_colliders(
  store: &ParticleStore,
  effects: &Assets<ParticleEffect>,
  emitter: &ParticleEmitter,
) -> bool {
  let id = emitter.effect.id();
  if collides(store, effects, id) {
    return true;
  }
  let named = current(store, effects, id)
    .into_iter()
    .flat_map(|effect| [&effect.on_death, &effect.on_collision])
    .flatten()
    .map(|sub| sub.effect.id());
  let pooled = store
    .pool(emitter.slot)
    .into_iter()
    .flat_map(|pool| [pool.death.effect, pool.collision.effect])
    .flatten();
  named.chain(pooled).any(|sub| collides(store, effects, sub))
}

/// `effect` as the step compiled it, or, to be compiled in this step for
/// the first time or for a new version, as its asset says
fn current<'a>(
  store: &'a ParticleStore,
  effects: &'a Assets<ParticleEffect>,
  effect: AssetId<ParticleEffect>,
) -> Option<&'a ParticleEffect> {
  let version = effects.version(effect)?;
  match store.compiled.get(&effect) {
    Some(compiled) if compiled.version == version => Some(&compiled.effect),
    _ => effects.get(effect),
  }
}

/// Whether `effect` collides, as [`current`] finds it
fn collides(
  store: &ParticleStore,
  effects: &Assets<ParticleEffect>,
  effect: AssetId<ParticleEffect>,
) -> bool {
  current(store, effects, effect).is_some_and(ParticleEffect::collides)
}

/// What an emitter's own effect does to its particles in a step: the radial,
/// tangential and noise forces, about where the emitter stands (the origin
/// of a `Local` effect's frame)
pub(crate) struct Own {
  pub(crate) centre: Vec2,
  /// The emitter's seed folded to 32 bits
  pub(crate) noise_seed: u32,
}

/// A stable value in `[0, 1)` from a particle's `seed`, different for each
/// `salt` so two values of one particle are not alike
fn unit_hash(seed: u32, salt: u32) -> f32 {
  let mut h = seed ^ salt.wrapping_mul(0x9E37_79B9);
  h ^= h >> 16;
  h = h.wrapping_mul(0x7FEB_352D);
  h ^= h >> 15;
  h = h.wrapping_mul(0x846C_A68B);
  h ^= h >> 16;
  (h >> 8) as f32 / (1u32 << 24) as f32
}

impl Own {
  /// The acceleration of the particle with `seed` at `at`, `age` seconds
  /// old, in pixels per second squared
  #[inline]
  pub(crate) fn force(&self, effect: &ParticleEffect, seed: u32, at: Vec2, age: f32) -> Vec2 {
    let mut force = Vec2::ZERO;
    let (radial, tangential) = (&effect.radial_acceleration, &effect.tangential_acceleration);
    if radial.min != 0.0 || radial.max != 0.0 || tangential.min != 0.0 || tangential.max != 0.0 {
      // Straight out from the centre; none at the centre itself
      let out = (at - self.centre).normalize_or_zero();
      // +y is down: counter-clockwise on screen from the right is up
      let round = Vec2::new(out.y, -out.x);
      force += out * radial.at(unit_hash(seed, 1)) + round * tangential.at(unit_hash(seed, 2));
    }
    if let Some(noise) = &effect.noise {
      force +=
        noise_force(self.noise_seed, at, age * noise.speed, noise.frequency) * noise.strength;
    }
    force
  }
}

/// What acts on one emitter's particles in its step
pub(crate) struct Acting<'a> {
  /// The attractors pulling its layer
  fields: &'a [Field],
  /// Its collisions, when its effect collides
  collide: Option<Collide<'a>>,
  /// Where the collisions to tell of go
  collisions: &'a mut Vec<ParticleCollision>,
}

/// One emitter's collisions: its effect's config and the colliders its
/// particles can hit
struct Collide<'a> {
  config: &'a CollisionConfig,
  colliders: &'a [ParticleCollider],
  edges: &'a [(Vec2, Vec2)],
  emitter: Entity,
}

impl Acting<'_> {
  /// The attractors' pull on a particle at `at`, in pixels per second
  /// squared
  #[inline]
  pub(crate) fn pull(&self, at: Vec2) -> Vec2 {
    self.fields.iter().fold(Vec2::ZERO, |sum, field| {
      let (x, y) = field
        .kind
        .force(field.time, field.at.x, field.at.y, at.x, at.y);
      sum + Vec2::new(x, y)
    })
  }

  /// Meets particle `i` of `p`, which moved from `from` this step, with
  /// the colliders, in gather order, and responds: `Die` removes it,
  /// `Bounce` reflects it off every collider it overlaps in turn, `Stick`
  /// holds it on the surface its radius out (`STUCK`), and `Event` lets it
  /// pass. `Die`, `Stick` and `Event` stop at the first contact. A contact
  /// tells of itself for `Die` and `Stick`, for a bounce's entry at
  /// `MIN_BOUNCE_EVENT_SPEED` or faster (one a step), and for an `Event`'s
  /// entry; an entry is a step touching a collider after one touching none
  /// (`TOUCHING`). Such a contact is told as a collision (with the
  /// particle's place and velocity before the response) when events are
  /// on, or always for an `Event`, and is returned as the particle's hit
  /// whether or not they are
  #[inline]
  pub(crate) fn collide(&mut self, p: &mut Pool, i: usize, from: Vec2) -> Met {
    match &self.collide {
      Some(collide) => collide.meet(p, i, from, self.collisions),
      None => Met {
        alive: true,
        hit: None,
      },
    }
  }
}

/// What meeting the colliders did to a particle in its step
#[derive(Debug, Clone, Copy)]
pub(crate) struct Met {
  /// Whether it lives on (a `Die` removes it)
  pub(crate) alive: bool,
  /// Its contact that told of itself, if any: the place on the surface,
  /// its radius out (as `Stick` places it), and its velocity before the
  /// response
  pub(crate) hit: Option<Trigger>,
}

impl Collide<'_> {
  /// [`Acting::collide`] for an emitter that collides, its collisions
  /// told into `collisions`
  fn meet(
    &self,
    p: &mut Pool,
    i: usize,
    from: Vec2,
    collisions: &mut Vec<ParticleCollision>,
  ) -> Met {
    let config = self.config;
    let was_touching = p.flags[i] & TOUCHING != 0;
    p.flags[i] &= !TOUCHING;
    // One bounce told a step, even when a corner touches two colliders
    let mut hit = None;
    // The contact as it tells of itself
    let tell = |p: &Pool, contact: &Contact| {
      let at = Vec2::new(contact.point_x, contact.point_y)
        + Vec2::new(contact.normal_x, contact.normal_y) * config.radius;
      let velocity = Vec2::new(p.vx[i], p.vy[i]);
      Trigger { at, velocity }
    };
    // Bounce resolves every overlapping collider in turn (a particle
    // resting on a floor must still hit a wall); the other responses stop
    // at the first contact
    for collider in self.colliders {
      let Some(contact) = collider.contact(self.edges, from, (p.x[i], p.y[i]), config.radius)
      else {
        continue;
      };
      p.flags[i] |= TOUCHING;
      match config.response {
        CollisionResponse::Die => {
          if config.send_events {
            collisions.push(told(self.emitter, p, i, &contact));
          }
          return Met {
            alive: false,
            hit: Some(tell(p, &contact)),
          };
        }
        CollisionResponse::Bounce { .. } => {
          let (nx, ny) = (contact.normal_x, contact.normal_y);
          let speed_along_normal = p.vx[i] * nx + p.vy[i] * ny;
          if speed_along_normal < 0.0 {
            // Entry only: a particle resting on a surface stays touching
            // and stays quiet
            if !was_touching && hit.is_none() && -speed_along_normal >= MIN_BOUNCE_EVENT_SPEED {
              if config.send_events {
                collisions.push(told(self.emitter, p, i, &contact));
              }
              hit = Some(tell(p, &contact));
            }
            let impulse = (1.0 + config.response.restitution()) * speed_along_normal;
            p.vx[i] -= impulse * nx;
            p.vy[i] -= impulse * ny;
          }
          let push = config.radius + CONTACT_SKIN;
          p.x[i] = contact.point_x + nx * push;
          p.y[i] = contact.point_y + ny * push;
        }
        CollisionResponse::Stick => {
          if config.send_events {
            collisions.push(told(self.emitter, p, i, &contact));
          }
          let hit = tell(p, &contact);
          // On the surface, its radius out, as a bounce is placed: not
          // where a fast move ended past it
          p.x[i] = hit.at.x;
          p.y[i] = hit.at.y;
          p.vx[i] = 0.0;
          p.vy[i] = 0.0;
          p.flags[i] |= STUCK;
          return Met {
            alive: true,
            hit: Some(hit),
          };
        }
        CollisionResponse::Event => {
          let hit = (!was_touching).then(|| {
            collisions.push(told(self.emitter, p, i, &contact));
            tell(p, &contact)
          });
          return Met { alive: true, hit };
        }
      }
    }
    Met { alive: true, hit }
  }
}

/// The collision of `emitter`'s particle `i` at `contact`, with its place
/// and velocity before the response
fn told(emitter: Entity, p: &Pool, i: usize, contact: &Contact) -> ParticleCollision {
  ParticleCollision {
    emitter_entity: emitter,
    collider_entity: contact.collider,
    position_x: p.x[i],
    position_y: p.y[i],
    normal_x: contact.normal_x,
    normal_y: contact.normal_y,
    velocity_x: p.vx[i],
    velocity_y: p.vy[i],
  }
}
