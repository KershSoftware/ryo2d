//! Sub-emitters: the effects a particle plays where it dies or meets a
//! collider, in its emitter's pool.

use glam::Vec2;

use crate::core::asset::{Assets, Handle};
use crate::core::ecs::{Entity, Transform, World};
use crate::core::particles::ParticleEmitter;
use crate::core::particles::collision::CollisionConfig;
use crate::core::particles::effect::{Burst, EmitShape, ParticleEffect, Space, SubEmitter};
use crate::core::particles::store::{DEFAULT_PARTICLE_BUDGET, ParticleStore};
use crate::core::particles::testing::{
  self, Seen, collision_particles, collisions, count, death_particles, finished, particle_warnings,
  particles, step, steps,
};
use crate::core::particles::value::Gradient;
use crate::core::physics::Collider;
use crate::core::ui::Color;

const DT: f32 = 1.0 / 60.0;
const RADIUS: f32 = 2.0;

/// One particle, burst as a one-cycle run starts, going right at `speed`
/// px/s and living `lifetime` s
fn one(world: &mut World, speed: f32, lifetime: f32) -> ParticleEffect {
  ParticleEffect::new(testing::image(world))
    .with_rate(0.0)
    .with_looping(false)
    .with_burst(Burst::at(0.0, 1.0))
    .with_direction(0.0, 0.0)
    .with_speed(speed)
    .with_lifetime(lifetime)
}

/// Particles standing still, each living `lifetime` s, and no emission of
/// their own (none plays a part in a sub-effect anyway)
fn still(world: &mut World, lifetime: f32) -> ParticleEffect {
  ParticleEffect::new(testing::image(world))
    .with_rate(0.0)
    .with_speed(0.0)
    .with_lifetime(lifetime)
}

/// [`still`], added to the world's store
fn sub(world: &mut World, lifetime: f32) -> Handle<ParticleEffect> {
  let effect = still(world, lifetime);
  testing::effect(world, effect)
}

/// An emitter of `effect` at (`x`, `y`)
fn spawn(world: &mut World, effect: ParticleEffect, x: f32, y: f32) -> Entity {
  let effect = testing::effect(world, effect);
  world
    .spawn((Transform::from_xy(x, y), ParticleEmitter::new(effect)))
    .id()
}

/// A 200 x 20 floor whose top is at y 50, under x 100
fn floor(world: &mut World) {
  world.spawn((Transform::from_xy(100.0, 60.0), Collider::rect(200.0, 20.0)));
}

fn live(world: &World) -> u32 {
  world.resource::<ParticleStore>().live()
}

fn alive(world: &World, emitter: Entity) -> u32 {
  world
    .get::<ParticleEmitter>(emitter)
    .map_or(0, ParticleEmitter::alive)
}

fn close(a: f32, b: f32, within: f32) -> bool {
  (a - b).abs() <= within
}

fn near(a: Vec2, b: Vec2) -> bool {
  (a - b).abs().max_element() <= 1e-3
}

fn place(seen: &Seen) -> Vec2 {
  Vec2::new(seen.x, seen.y)
}

/// Steps until `emitter` has none of its own particles left, at most
/// `most` steps: its last particle as the step before its death left it
fn until_dead(world: &mut World, emitter: Entity, most: usize) -> Option<Seen> {
  let mut last = None;
  for _ in 0..most {
    let before = particles(world, emitter).first().copied();
    step(world);
    if particles(world, emitter).is_empty() && before.is_some() {
      last = before;
      break;
    }
  }
  last
}

#[test]
fn a_death_plays_the_sub_effect_where_it_happened() {
  let mut world = testing::world();
  let puff = sub(&mut world, 10.0);
  let effect =
    one(&mut world, 60.0, 0.5).on_death(SubEmitter::new(puff, 5.0).with_inherit_velocity(0.5));
  let emitter = spawn(&mut world, effect, 10.0, 20.0);
  let last = until_dead(&mut world, emitter, 60).expect("it lived, then died");
  assert!(last.x > 35.0, "it moved before it died: {last:?}");

  let puffs = death_particles(&world, emitter);
  assert_eq!(puffs.len(), 5, "{puffs:?}");
  for puff in &puffs {
    assert!(
      near(place(puff), place(&last)),
      "where it was: {puff:?}, {last:?}"
    );
    assert!(
      near(Vec2::new(puff.vx, puff.vy), Vec2::new(30.0, 0.0)),
      "half its velocity: {puff:?}"
    );
    assert_eq!(puff.age, 0.0);
  }
  assert!(collision_particles(&world, emitter).is_empty());
  assert_eq!(
    alive(&world, emitter),
    5,
    "its sub-effect's particles count"
  );
  assert_eq!(live(&world), 5);

  // They move as their own effect says, and none dies to play it again
  step(&mut world);
  let moved = death_particles(&world, emitter);
  assert_eq!(moved.len(), 5);
  assert!(close(moved[0].x, last.x + 0.5, 1e-3), "{:?}", moved[0]);
  assert!(close(moved[0].age, DT, 1e-6));
}

#[test]
fn a_collision_plays_its_sub_effect() {
  let mut world = testing::world();
  let splash = sub(&mut world, 100.0);
  // Events are off: the sub-effect plays all the same
  let effect = one(&mut world, 0.0, 60.0)
    .with_gravity(0.0, 600.0)
    .with_collision(CollisionConfig::bouncy(RADIUS, 0.5))
    .on_collision(SubEmitter::new(splash, 3.0).with_inherit_velocity(0.25));
  let emitter = spawn(&mut world, effect, 100.0, 0.0);
  floor(&mut world);

  let mut falling = None;
  for _ in 0..60 {
    let before = particles(&world, emitter).first().map(|p| p.vy);
    step(&mut world);
    if !collision_particles(&world, emitter).is_empty() {
      falling = before;
      break;
    }
  }
  let falling = falling.expect("it hit the floor");
  // Its velocity before the bounce: one more step of gravity
  let hit = falling + 600.0 * DT;
  let splashes = collision_particles(&world, emitter);
  assert_eq!(splashes.len(), 3, "{splashes:?}");
  for splash in &splashes {
    assert!(
      near(place(splash), Vec2::new(100.0, 50.0 - RADIUS)),
      "on the floor, the particle's radius out: {splash:?}"
    );
    assert!(close(splash.vx, 0.0, 1e-3), "{splash:?}");
    assert!(close(splash.vy, 0.25 * hit, 1e-2), "{splash:?}, {hit}");
  }
  assert!(collisions(&world).is_empty(), "no messages");
  assert!(death_particles(&world, emitter).is_empty());

  // Each bounce fast enough to tell of plays it again; resting on the
  // floor, the particle plays nothing more
  steps(&mut world, 240);
  let played = collision_particles(&world, emitter).len();
  assert!(played > 3 && played.is_multiple_of(3), "{played}");
  steps(&mut world, 120);
  assert_eq!(collision_particles(&world, emitter).len(), played);
}

#[test]
fn a_die_on_hit_plays_both_its_death_and_its_collision_effects() {
  let mut world = testing::world();
  let puff = sub(&mut world, 100.0);
  let splash = sub(&mut world, 100.0);
  let effect = one(&mut world, 0.0, 60.0)
    .with_gravity(0.0, 600.0)
    .with_collision(CollisionConfig::die_on_hit(RADIUS))
    .on_death(SubEmitter::new(puff, 2.0))
    .on_collision(SubEmitter::new(splash, 3.0));
  let emitter = spawn(&mut world, effect, 100.0, 0.0);
  floor(&mut world);
  assert!(
    until_dead(&mut world, emitter, 60).is_some(),
    "it hit the floor"
  );

  let (puffs, splashes) = (
    death_particles(&world, emitter),
    collision_particles(&world, emitter),
  );
  assert_eq!((puffs.len(), splashes.len()), (2, 3));
  for seen in puffs.iter().chain(&splashes) {
    assert!(
      near(place(seen), Vec2::new(100.0, 50.0 - RADIUS)),
      "where it met the floor: {seen:?}"
    );
  }
}

#[test]
fn sub_effects_go_one_level_deep() {
  const NESTED: &str = "on_death: a sub-effect's own sub-emitters are ignored";
  let log = crate::core::testing::capture_log();
  let warned = || {
    particle_warnings(&log)
      .iter()
      .filter(|message| message.contains(NESTED))
      .count()
  };
  let before = warned();
  let mut world = testing::world();
  let deeper = sub(&mut world, 100.0);
  let nested = still(&mut world, 0.1).on_death(SubEmitter::new(deeper, 4.0));
  let nested = testing::effect(&mut world, nested);
  let effect = one(&mut world, 0.0, 0.1).on_death(SubEmitter::new(nested.clone(), 3.0));
  let emitter = spawn(&mut world, effect, 0.0, 0.0);
  step(&mut world);
  assert_eq!(warned(), before + 1, "warned as it is first played");

  let mut most = 0;
  for _ in 0..30 {
    step(&mut world);
    most = most.max(death_particles(&world, emitter).len());
  }
  assert_eq!(most, 3, "the sub-effect played");
  assert!(
    death_particles(&world, emitter).is_empty(),
    "its particles died and played nothing"
  );
  assert_eq!(live(&world), 0);
  assert_eq!(alive(&world, emitter), 0);
  assert_eq!(warned(), before + 1, "once per version");

  // A new version of the sub-effect is warned about once more
  let _ = world
    .resource_mut::<Assets<ParticleEffect>>()
    .get_mut(nested.id());
  steps(&mut world, 3);
  assert_eq!(warned(), before + 2);
}

/// An emitter whose one particle dies at once, playing five particles of
/// `puff`
fn dying(world: &mut World, puff: &Handle<ParticleEffect>, x: f32) -> Entity {
  let effect = one(world, 0.0, 0.05).on_death(SubEmitter::new(puff.clone(), 5.0));
  spawn(world, effect, x, 0.0)
}

#[test]
fn sub_particles_vanish_with_the_emitter() {
  let mut world = testing::world();
  let puff = sub(&mut world, 100.0);
  let (a, b, c) = (
    dying(&mut world, &puff, 0.0),
    dying(&mut world, &puff, 10.0),
    dying(&mut world, &puff, 20.0),
  );
  steps(&mut world, 10);
  for emitter in [a, b, c] {
    assert_eq!(death_particles(&world, emitter).len(), 5);
  }
  assert_eq!(live(&world), 15);

  world.despawn(a);
  assert_eq!(live(&world), 10, "despawned with its emitter");
  world.entity_mut(b).remove::<ParticleEmitter>();
  assert_eq!(live(&world), 5, "taken off with its emitter");
  if let Some(mut emitter) = world.get_mut::<ParticleEmitter>(c) {
    emitter.clear();
  }
  step(&mut world);
  assert_eq!(live(&world), 0, "cleared with its emitter's own");
  assert!(death_particles(&world, c).is_empty());
  assert_eq!(alive(&world, c), 0);
}

#[test]
fn sub_particles_count_against_the_budget() {
  let log = crate::core::testing::capture_log();
  // The sub-effect's own cap: two deaths of ten particles each, at most
  // three alive
  let mut world = testing::world();
  let capped = still(&mut world, 100.0).with_max_particles(3);
  let capped = testing::effect(&mut world, capped);
  let effect = one(&mut world, 0.0, 0.05)
    .with_burst(Burst::at(0.0, 1.0))
    .on_death(SubEmitter::new(capped, 10.0));
  let emitter = spawn(&mut world, effect, 0.0, 0.0);
  steps(&mut world, 10);
  assert_eq!(death_particles(&world, emitter).len(), 3);
  assert_eq!(live(&world), 3);

  // The engine's budget, over every pool
  let warned = || {
    particle_warnings(&log)
      .iter()
      .filter(|message| message.contains("budget of 8"))
      .count()
  };
  let before = warned();
  let mut world = testing::world();
  world.insert_resource(ParticleStore::new(8, 1));
  let free = sub(&mut world, 100.0);
  let effect = one(&mut world, 0.0, 0.05)
    .with_burst(Burst::at(0.0, 1.0))
    .on_death(SubEmitter::new(free, 10.0));
  let emitter = spawn(&mut world, effect, 0.0, 0.0);
  steps(&mut world, 10);
  assert_eq!(death_particles(&world, emitter).len(), 8);
  assert_eq!(live(&world), 8);
  assert_eq!(alive(&world, emitter), 8);
  assert_eq!(warned(), before + 1, "warned once, for the sub-effect");
}

#[test]
fn finished_waits_for_sub_particles() {
  let mut world = testing::world();
  let puff = sub(&mut world, 0.5);
  // Its one cycle ends long before its particle dies
  let effect = one(&mut world, 0.0, 0.1)
    .with_duration(0.05)
    .on_death(SubEmitter::new(puff, 2.0));
  let emitter = spawn(&mut world, effect, 0.0, 0.0);
  assert!(until_dead(&mut world, emitter, 30).is_some());
  let state = world.get::<ParticleEmitter>(emitter).cloned();
  let state = state.expect("an emitter");
  assert!(!state.is_playing());
  assert_eq!(death_particles(&world, emitter).len(), 2);
  assert_eq!(state.alive(), 2);
  assert!(!state.is_finished());
  assert!(
    finished(&world).is_empty(),
    "its sub-effect's particles live"
  );

  steps(&mut world, 40);
  assert!(death_particles(&world, emitter).is_empty());
  assert_eq!(alive(&world, emitter), 0);
  assert_eq!(finished(&world), [emitter]);
}

#[test]
fn a_sub_effect_not_yet_loaded_spawns_nothing() {
  let log = crate::core::testing::capture_log();
  let warnings = particle_warnings(&log).len();
  let mut world = testing::world();
  // A handle to a slot with no value yet, as a queued load hands out
  let loading: Handle<ParticleEffect> = world
    .resource::<Assets<ParticleEffect>>()
    .ledger()
    .lock()
    .unwrap()
    .reserve();
  // A particle born every 0.1 s, dying 0.05 s later
  let effect = still(&mut world, 0.05)
    .with_rate(10.0)
    .on_death(SubEmitter::new(loading.clone(), 2.0));
  let emitter = spawn(&mut world, effect, 0.0, 0.0);
  steps(&mut world, 30);
  assert!(death_particles(&world, emitter).is_empty());
  assert_eq!(live(&world) as usize, count(&world, emitter));
  assert_eq!(
    particle_warnings(&log).len(),
    warnings,
    "nothing to warn about"
  );

  // It lands: the deaths after play it
  let landed = still(&mut world, 100.0);
  assert!(
    world
      .resource_mut::<Assets<ParticleEffect>>()
      .insert(loading.id(), landed)
  );
  steps(&mut world, 12);
  assert!(!death_particles(&world, emitter).is_empty());
  assert_eq!(particle_warnings(&log).len(), warnings);
}

#[test]
fn a_particle_dying_of_age_by_a_wall_ends_on_its_side() {
  let mut world = testing::world();
  let puff = sub(&mut world, 100.0);
  // Right at 600 px/s, 10 px a step, from x 5: at 95 after its ninth move,
  // it would cross the wall at 100 in its tenth, the step its life ends
  let effect = one(&mut world, 600.0, 0.16)
    .with_collision(CollisionConfig::bouncy(RADIUS, 0.5))
    .on_death(SubEmitter::new(puff, 1.0));
  let emitter = spawn(&mut world, effect, 5.0, 0.0);
  let wall = Collider::segment(Vec2::new(0.0, -50.0), Vec2::new(0.0, 50.0));
  world.spawn((Transform::from_xy(100.0, 0.0), wall));

  let last = until_dead(&mut world, emitter, 30).expect("it lived, then died");
  assert!(close(last.x, 95.0, 1e-3), "{last:?}");
  let puffs = death_particles(&world, emitter);
  assert_eq!(puffs.len(), 1);
  assert!(
    near(place(&puffs[0]), place(&last)),
    "where it stood as its last step began, short of the wall: {:?}",
    puffs[0]
  );
}

/// The particles an emitter's deaths play in 40 steps, its store seeded
/// with `store_seed`
fn seeded_deaths(store_seed: u64) -> Vec<Seen> {
  let mut world = testing::world();
  world.insert_resource(ParticleStore::new(DEFAULT_PARTICLE_BUDGET, store_seed));
  let spray = ParticleEffect::new(testing::image(&mut world))
    .with_lifetime((1.0, 3.0))
    .with_speed((10.0, 80.0))
    .with_shape(EmitShape::Circle {
      radius: 8.0,
      arc_deg: 360.0,
      thickness: 1.0,
    });
  let spray = testing::effect(&mut world, spray);
  let effect = ParticleEffect::new(testing::image(&mut world))
    .with_rate(30.0)
    .with_lifetime((0.1, 0.3))
    .on_death(SubEmitter::new(spray, (2.0, 6.0)));
  let emitter = spawn(&mut world, effect, 0.0, 0.0);
  steps(&mut world, 40);
  death_particles(&world, emitter)
}

#[test]
fn sub_spawns_repeat_with_the_seed() {
  let deaths = seeded_deaths(1);
  assert!(deaths.len() > 10, "{}", deaths.len());
  assert_eq!(seeded_deaths(1), deaths, "drawn from the emitter's stream");
  assert_ne!(seeded_deaths(2), deaths);
}

#[test]
fn a_prewarmed_effect_plays_its_sub_effects() {
  let mut world = testing::world();
  let puff = sub(&mut world, 5.0);
  let effect = still(&mut world, 0.2)
    .with_rate(20.0)
    .with_prewarm(true)
    .on_death(SubEmitter::new(puff, 1.0));
  let emitter = spawn(&mut world, effect, 0.0, 0.0);
  step(&mut world);
  let puffs = death_particles(&world, emitter).len();
  assert!(puffs >= 10, "a second of deaths played already: {puffs}");
}

#[test]
fn a_sub_effect_collides_though_its_parent_does_not() {
  let mut world = testing::world();
  // Its particles fall onto the floor and die there
  let drop = still(&mut world, 60.0)
    .with_gravity(0.0, 600.0)
    .with_collision(CollisionConfig::die_on_hit(RADIUS));
  let drop = testing::effect(&mut world, drop);
  let effect = one(&mut world, 0.0, 0.05).on_death(SubEmitter::new(drop, 3.0));
  let emitter = spawn(&mut world, effect, 100.0, 0.0);
  floor(&mut world);

  let (mut most, mut lowest) = (0, f32::MIN);
  for _ in 0..90 {
    step(&mut world);
    let drops = death_particles(&world, emitter);
    most = most.max(drops.len());
    lowest = drops.iter().fold(lowest, |low, seen| low.max(seen.y));
  }
  assert_eq!(most, 3);
  assert!(
    death_particles(&world, emitter).is_empty(),
    "they died on the floor"
  );
  assert!(lowest <= 50.0, "none passed it: {lowest}");
}

#[test]
fn sub_particles_live_in_their_own_effect_s_space() {
  let mut world = testing::world();
  // A world particle dying into a `Local` sub-effect: its particles live
  // in the emitter's frame
  let follow = still(&mut world, 100.0).with_space(Space::Local);
  let follow = testing::effect(&mut world, follow);
  let effect = one(&mut world, 60.0, 0.25).on_death(SubEmitter::new(follow, 1.0));
  let emitter = spawn(&mut world, effect, 100.0, 100.0);
  let last = until_dead(&mut world, emitter, 30).expect("it died");
  let puffs = death_particles(&world, emitter);
  assert_eq!(puffs.len(), 1);
  assert!(
    near(place(&puffs[0]), place(&last) - Vec2::new(100.0, 100.0)),
    "in the emitter's frame: {:?}, {last:?}",
    puffs[0]
  );

  // A `Local` particle dying into a world sub-effect: its particles are
  // placed in the world, where it died
  let stay = sub(&mut world, 100.0);
  let local = one(&mut world, 60.0, 0.25)
    .with_space(Space::Local)
    .on_death(SubEmitter::new(stay, 1.0));
  let other = spawn(&mut world, local, 300.0, 100.0);
  let last = until_dead(&mut world, other, 30).expect("it died");
  let puffs = death_particles(&world, other);
  assert_eq!(puffs.len(), 1);
  assert!(
    near(place(&puffs[0]), place(&last) + Vec2::new(300.0, 100.0)),
    "in the world: {:?}, {last:?}",
    puffs[0]
  );
}

/// A reserved slot of the effects' store with no value in it yet: an effect
/// still loading
fn loading_effect(world: &World) -> Handle<ParticleEffect> {
  world
    .resource::<Assets<ParticleEffect>>()
    .ledger()
    .lock()
    .unwrap_or_else(std::sync::PoisonError::into_inner)
    .reserve()
}

/// `emitter`'s effect's `on_death`, now playing `effect` (a game's change,
/// or a saved file's)
fn play_on_death(world: &mut World, emitter: Entity, effect: Handle<ParticleEffect>, n: f32) {
  let id = match world.get::<ParticleEmitter>(emitter) {
    Some(emitter) => emitter.effect.id(),
    None => panic!("no emitter"),
  };
  if let Some(parent) = world.resource_mut::<Assets<ParticleEffect>>().get_mut(id) {
    parent.on_death = Some(SubEmitter::new(effect, n));
  }
}

#[test]
fn a_sub_effect_still_loading_leaves_the_last_one_s_particles_playing() {
  let mut world = testing::world();
  let old = sub(&mut world, 100.0);
  let effect = one(&mut world, 0.0, 0.05).on_death(SubEmitter::new(old.clone(), 5.0));
  let emitter = spawn(&mut world, effect, 0.0, 0.0);
  steps(&mut world, 10);
  let before = death_particles(&world, emitter);
  assert_eq!(before.len(), 5, "{before:?}");

  // The parent names a sub-effect that has not loaded: the old particles
  // keep stepping by the effect they play
  let new = loading_effect(&world);
  play_on_death(&mut world, emitter, new.clone(), 5.0);
  steps(&mut world, 3);
  let waiting = death_particles(&world, emitter);
  assert_eq!(waiting.len(), 5, "not cleared: {waiting:?}");
  assert_eq!(live(&world), 5, "still counted");
  for (was, now) in before.iter().zip(&waiting) {
    assert!(now.age > was.age, "still stepping: {now:?}");
    assert_eq!(now.color[0], now.color[2], "still its old look: {now:?}");
  }

  // Once it lands the pool adopts it, its particles following it
  let lands = still(&mut world, 200.0).with_color_over_life(Gradient::constant(Color::BLUE));
  world
    .resource_mut::<Assets<ParticleEffect>>()
    .insert(new.id(), lands);
  step(&mut world);
  let after = death_particles(&world, emitter);
  assert_eq!(after.len(), 5, "{after:?}");
  for particle in &after {
    assert_eq!(particle.color, [0.0, 0.0, 1.0, 1.0], "{particle:?}");
  }
}
