//! Emission by distance, inherited velocity, local space and prewarm.

use std::f32::consts::FRAC_PI_2;

use glam::Vec2;

use crate::core::ecs::kernel::Disabled;
use crate::core::ecs::{Entity, Transform, World, WorldExt};
use crate::core::particles::ParticleEmitter;
use crate::core::particles::attractor::ParticleAttractor;
use crate::core::particles::collision::{CollisionConfig, CollisionResponse};
use crate::core::particles::effect::{Burst, EmitShape, ParticleEffect, Space};
use crate::core::particles::store::ParticleStore;
use crate::core::particles::testing::{
  self, Seen, collisions, count, drawn, particles, step, steps,
};
use crate::core::physics::Collider;

/// The default effect
fn plain(world: &mut World) -> ParticleEffect {
  ParticleEffect::new(testing::image(world))
}

/// No rate of its own; each particle stands still once born and lives 100 s
fn quiet(world: &mut World) -> ParticleEffect {
  plain(world)
    .with_rate(0.0)
    .with_speed(0.0)
    .with_lifetime(100.0)
    .with_max_particles(10_000)
}

/// An emitter of `effect` placed by `at`
fn spawn(world: &mut World, effect: ParticleEffect, at: Transform) -> Entity {
  let effect = testing::effect(world, effect);
  world.spawn((at, ParticleEmitter::new(effect))).id()
}

fn move_by(world: &mut World, emitter: Entity, by: Vec2) {
  if let Some(mut transform) = world.get_mut::<Transform>(emitter) {
    transform.translation += by;
  }
}

/// The `n` particles `emitter` spawned last, newest last
fn newest(world: &World, emitter: Entity, n: usize) -> Vec<Seen> {
  let seen = particles(world, emitter);
  seen[seen.len().saturating_sub(n)..].to_vec()
}

fn close(a: f32, b: f32, within: f32) -> bool {
  (a - b).abs() <= within
}

fn near(a: Vec2, b: Vec2) -> bool {
  (a - b).abs().max_element() <= 1e-3
}

/// `emitter`'s pool's box, as the drawer culls it
fn bounds(world: &World, emitter: Entity) -> Option<(Vec2, Vec2)> {
  let slot = world.get::<ParticleEmitter>(emitter)?.slot;
  world.resource::<ParticleStore>().pool(slot)?.bounds
}

fn has_problem(effect: &ParticleEffect, prefix: &str) -> bool {
  effect.problems().iter().any(|p| p.starts_with(prefix))
}

// Rate over distance

#[test]
fn rate_per_distance_spawns_by_distance() {
  let mut world = testing::world();
  let effect = quiet(&mut world).with_rate_per_distance(0.5);
  let emitter = spawn(&mut world, effect, Transform::from_xy(0.0, 0.0));
  step(&mut world);
  assert_eq!(count(&world, emitter), 0, "none standing still");

  // 200 px in 20 px moves
  for _ in 0..10 {
    move_by(&mut world, emitter, Vec2::new(20.0, 0.0));
    step(&mut world);
  }
  assert_eq!(count(&world, emitter), 100, "0.5 a pixel over 200 px");
  // Spread along each step's move, as the rate's stream is
  for (i, seen) in newest(&world, emitter, 10).iter().enumerate() {
    let f = (i + 1) as f32 / 10.0;
    assert!(close(seen.x, 180.0 + 20.0 * f, 1e-3), "{i}: {seen:?}");
  }

  steps(&mut world, 10);
  assert_eq!(count(&world, emitter), 100, "none standing still");

  // A teleport spawns nothing along its jump
  move_by(&mut world, emitter, Vec2::new(1000.0, 0.0));
  world.skip_interpolation(emitter);
  step(&mut world);
  assert_eq!(count(&world, emitter), 100, "nothing along a teleport");

  // A fraction of a particle carries to the next step: 1.5 a step
  for _ in 0..2 {
    move_by(&mut world, emitter, Vec2::new(0.0, 3.0));
    step(&mut world);
  }
  assert_eq!(count(&world, emitter), 103);
}

// Inherited velocity

#[test]
fn inherited_velocity_follows_the_emitter() {
  let mut world = testing::world();
  // 10 a step, each taking half the emitter's velocity
  let effect = quiet(&mut world)
    .with_rate(600.0)
    .with_inherit_velocity(0.5);
  let emitter = spawn(&mut world, effect, Transform::from_xy(0.0, 0.0));
  let velocities = |world: &World| {
    newest(world, emitter, 10)
      .iter()
      .map(|seen| Vec2::new(seen.vx, seen.vy))
      .collect::<Vec<_>>()
  };
  step(&mut world);
  assert!(
    velocities(&world).iter().all(|v| *v == Vec2::ZERO),
    "none in its first step"
  );

  // 10 px right and 5 up a step: (600, -300) px/s
  move_by(&mut world, emitter, Vec2::new(10.0, -5.0));
  step(&mut world);
  for v in velocities(&world) {
    assert!(near(v, Vec2::new(300.0, -150.0)), "{v}");
  }

  step(&mut world);
  assert!(
    velocities(&world).iter().all(|v| *v == Vec2::ZERO),
    "none standing still"
  );

  // A teleport is no velocity
  move_by(&mut world, emitter, Vec2::new(1000.0, 0.0));
  world.skip_interpolation(emitter);
  step(&mut world);
  assert!(
    velocities(&world).iter().all(|v| *v == Vec2::ZERO),
    "none across a teleport"
  );

  // Nor is a move while disabled
  world.entity_mut(emitter).insert(Disabled);
  step(&mut world);
  move_by(&mut world, emitter, Vec2::new(500.0, 0.0));
  world.entity_mut(emitter).remove::<Disabled>();
  step(&mut world);
  assert!(
    velocities(&world).iter().all(|v| *v == Vec2::ZERO),
    "none from where it was disabled"
  );

  // Added to a particle's own velocity
  let effect = quiet(&mut world)
    .with_rate(600.0)
    .with_speed(100.0)
    .with_direction(90.0, 0.0)
    .with_inherit_velocity(1.0);
  let emitter = spawn(&mut world, effect, Transform::from_xy(0.0, 500.0));
  step(&mut world);
  move_by(&mut world, emitter, Vec2::new(-10.0, 0.0));
  step(&mut world);
  for seen in newest(&world, emitter, 10) {
    assert!(
      near(Vec2::new(seen.vx, seen.vy), Vec2::new(-600.0, 100.0)),
      "{seen:?}"
    );
  }
}

// Local space

#[test]
fn local_particles_follow_a_moving_emitter_and_world_ones_do_not() {
  let mut world = testing::world();
  // Four particles over a box, standing still
  let effect = |world: &mut World, space| {
    quiet(world)
      .with_duration(100.0)
      .with_burst(Burst::at(0.0, 4.0))
      .with_shape(EmitShape::Rect {
        size: Vec2::new(40.0, 20.0),
        thickness: 1.0,
      })
      .with_space(space)
  };
  let local = effect(&mut world, Space::Local);
  let local = spawn(&mut world, local, Transform::from_xy(0.0, 0.0));
  let fixed = effect(&mut world, Space::World);
  let fixed = spawn(&mut world, fixed, Transform::from_xy(0.0, 200.0));
  step(&mut world);
  let local_born = drawn(&world, local, 1.0);
  let fixed_born = drawn(&world, fixed, 1.0);
  assert_eq!(local_born.len(), 4);
  assert_eq!(fixed_born.len(), 4);

  let by = Vec2::new(100.0, 50.0);
  move_by(&mut world, local, by);
  move_by(&mut world, fixed, by);
  step(&mut world);
  for (born, now) in local_born.iter().zip(drawn(&world, local, 1.0)) {
    assert!(near(now, *born + by), "local follows: {born} {now}");
  }
  assert_eq!(drawn(&world, fixed, 1.0), fixed_born, "world ones stay");

  // Turned a quarter, its particles turn round it
  if let Some(mut transform) = world.get_mut::<Transform>(local) {
    transform.rotation = FRAC_PI_2;
  }
  step(&mut world);
  for (born, now) in local_born.iter().zip(drawn(&world, local, 1.0)) {
    assert!(near(now, by + born.perp()), "turned: {born} {now}");
  }

  // Far away, the box the drawer culls by is where they draw
  move_by(&mut world, local, Vec2::new(1000.0, 1000.0));
  step(&mut world);
  let (min, max) = bounds(&world, local).unwrap_or_default();
  for now in drawn(&world, local, 1.0) {
    let inside = now.cmpge(min - 1e-3).all() && now.cmple(max + 1e-3).all();
    assert!(inside, "{now} in {min} to {max}");
  }

  // Forces act in its frame: its gravity points down its turned y axis
  let falling = quiet(&mut world)
    .with_duration(100.0)
    .with_burst(Burst::at(0.0, 1.0))
    .with_gravity(0.0, 100.0)
    .with_space(Space::Local);
  let turned = Transform::from_xy(500.0, 500.0).with_rotation(FRAC_PI_2);
  let falling = spawn(&mut world, falling, turned);
  steps(&mut world, 61);
  let [at] = drawn(&world, falling, 1.0)[..] else {
    panic!("one particle");
  };
  assert!(at.x < 460.0 && close(at.y, 500.0, 1e-2), "{at}");

  // It takes none of its emitter's velocity: it moves with it already
  let carried = quiet(&mut world)
    .with_rate(600.0)
    .with_inherit_velocity(1.0)
    .with_space(Space::Local);
  let carried = spawn(&mut world, carried, Transform::from_xy(0.0, 900.0));
  step(&mut world);
  move_by(&mut world, carried, Vec2::new(10.0, 0.0));
  step(&mut world);
  for seen in newest(&world, carried, 10) {
    assert_eq!((seen.vx, seen.vy), (0.0, 0.0), "{seen:?}");
  }
}

#[test]
fn a_change_of_space_keeps_particles_where_they_are() {
  let mut world = testing::world();
  let effect = quiet(&mut world)
    .with_duration(100.0)
    .with_burst(Burst::at(0.0, 4.0))
    .with_shape(EmitShape::Circle {
      radius: 30.0,
      arc_deg: 360.0,
      thickness: 1.0,
    });
  let effect = testing::effect(&mut world, effect);
  let at = Transform::from_xy(100.0, 50.0).with_rotation(0.5);
  let emitter = world.spawn((at, ParticleEmitter::new(effect.clone()))).id();
  step(&mut world);
  let born = drawn(&world, emitter, 1.0);
  for space in [Space::Local, Space::World] {
    if let Some(fx) = world
      .resource_mut::<crate::core::asset::Assets<ParticleEffect>>()
      .get_mut(effect.id())
    {
      fx.space = space;
    }
    step(&mut world);
    for (was, now) in born.iter().zip(drawn(&world, emitter, 1.0)) {
      assert!(near(now, *was), "{space:?}: {was} {now}");
    }
  }
}

#[test]
fn a_local_effect_does_not_collide() {
  // Its warnings stay in its own capture
  let _log = crate::core::testing::capture_log();
  let mut world = testing::world();
  let colliding = plain(&mut world).with_collision(CollisionConfig::die_on_hit(2.0));
  assert!(!has_problem(&colliding, "collision:"), "a world effect may");
  let local = colliding.clone().with_space(Space::Local);
  assert!(
    local
      .problems()
      .iter()
      .any(|p| p == "collision: a Local effect does not collide"),
    "{:?}",
    local.problems()
  );
  let clean = local.sanitized();
  assert!(clean.collision.is_none(), "the collision is dropped");
  assert!(clean.problems().is_empty(), "{:?}", clean.problems());

  // Falling onto a floor, it passes through and tells nothing
  let falling = quiet(&mut world)
    .with_duration(100.0)
    .with_burst(Burst::at(0.0, 1.0))
    .with_gravity(0.0, 600.0)
    .with_collision(CollisionConfig::die_on_hit(2.0).with_events())
    .with_space(Space::Local);
  let falling = spawn(&mut world, falling, Transform::from_xy(100.0, 0.0));
  world.spawn((Transform::from_xy(100.0, 60.0), Collider::rect(200.0, 20.0)));
  steps(&mut world, 60);
  let [at] = drawn(&world, falling, 1.0)[..] else {
    panic!("it lives");
  };
  assert!(at.y > 70.0, "below the floor: {at}");
  assert!(collisions(&world).is_empty());

  // Nor do attractors pull it
  let still = quiet(&mut world)
    .with_duration(100.0)
    .with_burst(Burst::at(0.0, 1.0))
    .with_space(Space::Local);
  let still = spawn(&mut world, still, Transform::from_xy(0.0, 500.0));
  world.spawn((
    Transform::from_xy(100.0, 500.0),
    ParticleAttractor::point(500.0, 1000.0),
  ));
  steps(&mut world, 2);
  assert_eq!(particles(&world, still)[0].vx, 0.0);
}

// Prewarm

#[test]
fn prewarm_starts_full() {
  let mut world = testing::world();
  let warm = |world: &mut World| {
    plain(world)
      .with_rate(100.0)
      .with_lifetime(1.0)
      .with_speed(0.0)
      .with_looping(true)
      .with_prewarm(true)
  };
  let effect = warm(&mut world);
  let emitter = spawn(&mut world, effect, Transform::from_xy(0.0, 0.0));
  step(&mut world);
  let seen = particles(&world, emitter);
  assert!(seen.len().abs_diff(100) <= 1, "{} live", seen.len());
  let ages: Vec<f32> = seen.iter().map(|seen| seen.age).collect();
  assert!(ages.iter().all(|age| (0.0..1.0).contains(age)), "{ages:?}");
  for tenth in 0..10 {
    let low = tenth as f32 / 10.0;
    let within = ages
      .iter()
      .filter(|age| (low..low + 0.1).contains(*age))
      .count();
    assert!((8..=12).contains(&within), "{within} aged {low} to {low}.1");
  }
  steps(&mut world, 30);
  assert!(count(&world, emitter).abs_diff(100) <= 1, "and stays full");

  // Without messages: each particle's entry into a collider round the
  // emitter is told for this step's spawns only
  let mut world = testing::world();
  let effect = warm(&mut world)
    .with_collision(CollisionConfig::new(1.0).with_response(CollisionResponse::Event));
  spawn(&mut world, effect, Transform::from_xy(0.0, 0.0));
  world.spawn((Transform::from_xy(0.0, 0.0), Collider::rect(400.0, 400.0)));
  step(&mut world);
  let told = collisions(&world).len();
  assert!((1..=2).contains(&told), "{told} told");
}

#[test]
fn prewarm_needs_a_looping_effect() {
  // Its warnings stay in its own capture
  let _log = crate::core::testing::capture_log();
  let mut world = testing::world();
  let warm = plain(&mut world)
    .with_rate(100.0)
    .with_lifetime(1.0)
    .with_prewarm(true);
  assert!(!has_problem(&warm, "prewarm:"), "a looping effect may");
  let once = warm.clone().with_looping(false);
  assert!(has_problem(&once, "prewarm:"), "{:?}", once.problems());
  assert!(!once.sanitized().prewarm, "turned off");
  assert!(once.sanitized().problems().is_empty());

  // It starts empty, as does a looping one played once
  let emitter = spawn(&mut world, once, Transform::from_xy(0.0, 0.0));
  let effect = testing::effect(&mut world, warm);
  let played_once = world
    .spawn((
      Transform::from_xy(0.0, 100.0),
      ParticleEmitter::new(effect).once(),
    ))
    .id();
  step(&mut world);
  assert!(count(&world, emitter) <= 2, "{}", count(&world, emitter));
  assert!(
    count(&world, played_once) <= 2,
    "{}",
    count(&world, played_once)
  );
}
