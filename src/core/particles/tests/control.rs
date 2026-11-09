//! Emitters under the game's control: play, stop, clear, restart and burst,
//! one cycle, finishing, and the views.

use std::time::Duration;

use glam::Vec2;

use crate::core::asset::{Assets, Handle};
use crate::core::ecs::kernel::{IntoScheduleConfigs, Res, ResMut, Resource, Schedule};
use crate::core::ecs::{Entity, Transform, World, WorldExt};
use crate::core::graphics::Image;
use crate::core::particles::effect::{Burst, ParticleEffect};
use crate::core::particles::emitter::ParticleEmitter;
use crate::core::particles::scope;
use crate::core::particles::store::{DEFAULT_PARTICLE_BUDGET, ParticleStore};
use crate::core::particles::testing::{
  self, count, finished, fraction, frame, particles, step, steps,
};
use crate::core::particles::view::{Particles, ParticlesView};
use crate::core::time::Time;

/// Bursts only, standing still, each particle living `lifetime`
fn still(world: &mut World, lifetime: f32) -> ParticleEffect {
  ParticleEffect::new(testing::image(world))
    .with_rate(0.0)
    .with_speed(0.0)
    .with_lifetime(lifetime)
}

/// A stream of one particle a step, standing still, living 10 s
fn stream(world: &mut World) -> ParticleEffect {
  ParticleEffect::new(testing::image(world))
    .with_rate(60.0)
    .with_speed(0.0)
    .with_lifetime(10.0)
}

fn spawn(world: &mut World, emitter: ParticleEmitter) -> Entity {
  world.spawn((Transform::from_xy(0.0, 0.0), emitter)).id()
}

fn emitter_of(world: &mut World, effect: ParticleEffect) -> ParticleEmitter {
  ParticleEmitter::new(testing::effect(world, effect))
}

fn emitter(world: &World, entity: Entity) -> &ParticleEmitter {
  world
    .get::<ParticleEmitter>(entity)
    .expect("the entity has an emitter")
}

fn control(world: &mut World, entity: Entity, f: impl FnOnce(&mut ParticleEmitter)) {
  let mut emitter = world
    .get_mut::<ParticleEmitter>(entity)
    .expect("the entity has an emitter");
  f(&mut emitter);
}

fn live(world: &World) -> u32 {
  world.resource::<ParticleStore>().live()
}

fn exists(world: &World, entity: Entity) -> bool {
  world.get_entity(entity).is_ok()
}

#[test]
fn stop_lets_live_particles_finish() {
  let mut world = testing::world();
  let effect = ParticleEffect::new(testing::image(&mut world))
    .with_rate(60.0)
    .with_lifetime(0.5)
    .with_speed(30.0)
    .with_direction(0.0, 0.0);
  let fx = emitter_of(&mut world, effect);
  let e = spawn(&mut world, fx);
  steps(&mut world, 20);
  let before = particles(&world, e);
  assert_eq!(before.len(), 20);

  control(&mut world, e, ParticleEmitter::stop);
  assert!(!emitter(&world, e).is_playing());
  step(&mut world);
  let after = particles(&world, e);
  assert_eq!(
    after.len(),
    20,
    "no new particle, and none old enough to die"
  );
  for (was, now) in before.iter().zip(&after) {
    assert!(now.x > was.x, "live particles carry on moving");
    assert!(now.age > was.age);
  }
  assert!(!emitter(&world, e).is_finished(), "particles still live");

  steps(&mut world, 40);
  assert_eq!(count(&world, e), 0, "they lived out their lives");
  assert_eq!(live(&world), 0);
  assert!(emitter(&world, e).is_finished());
}

#[test]
fn clear_removes_live_particles() {
  let mut world = testing::world();
  let effect = stream(&mut world);
  let fx = emitter_of(&mut world, effect);
  let e = spawn(&mut world, fx);
  steps(&mut world, 10);
  assert_eq!(count(&world, e), 10);

  control(&mut world, e, ParticleEmitter::clear);
  step(&mut world);
  assert_eq!(count(&world, e), 1, "the ten are gone, and it emits on");
  assert_eq!(live(&world), 1);
  assert_eq!(emitter(&world, e).alive(), 1);
  assert!(emitter(&world, e).is_playing());
}

#[test]
fn restart_rearms_bursts() {
  let mut world = testing::world();
  let effect = still(&mut world, 10.0)
    .with_duration(1.0)
    .with_looping(false)
    .with_burst(Burst::at(0.0, 5.0))
    .with_burst(Burst::at(0.5, 3.0));
  let fx = emitter_of(&mut world, effect);
  let e = spawn(&mut world, fx);
  steps(&mut world, 60);
  assert_eq!(count(&world, e), 8);
  steps(&mut world, 60);
  assert_eq!(count(&world, e), 8, "its one cycle is over");
  assert!(!emitter(&world, e).is_playing());

  control(&mut world, e, ParticleEmitter::restart);
  assert!(emitter(&world, e).is_playing());
  step(&mut world);
  assert_eq!(
    count(&world, e),
    5,
    "cleared, and the burst at 0 fires again"
  );
  steps(&mut world, 28);
  assert_eq!(count(&world, e), 5);
  step(&mut world);
  assert_eq!(count(&world, e), 8, "and the one at 0.5 s");
  assert_eq!(live(&world), 8);
}

#[test]
fn burst_adds_particles_in_any_mode() {
  let mut world = testing::world();
  let effect = still(&mut world, 10.0).with_duration(100.0);
  let fx = emitter_of(&mut world, effect);
  let e = spawn(&mut world, fx);
  step(&mut world);
  assert_eq!(count(&world, e), 0, "nothing to emit");

  control(&mut world, e, |e| e.burst(7));
  assert!(!emitter(&world, e).is_finished(), "it owes a burst");
  step(&mut world);
  assert_eq!(count(&world, e), 7);
  step(&mut world);
  assert_eq!(count(&world, e), 7, "once");

  // Stopped, it still bursts
  control(&mut world, e, ParticleEmitter::stop);
  control(&mut world, e, |e| e.burst(2));
  step(&mut world);
  assert_eq!(count(&world, e), 9);

  // A stream adds the burst to its own
  let effect = stream(&mut world);
  let fx = emitter_of(&mut world, effect);
  let streaming = spawn(&mut world, fx);
  control(&mut world, streaming, |e| e.burst(4));
  step(&mut world);
  assert_eq!(count(&world, streaming), 5);

  // Within max_particles
  let effect = still(&mut world, 10.0)
    .with_duration(100.0)
    .with_max_particles(10);
  let fx = emitter_of(&mut world, effect);
  let capped = spawn(&mut world, fx);
  control(&mut world, capped, |e| e.burst(25));
  step(&mut world);
  assert_eq!(count(&world, capped), 10);
  control(&mut world, capped, |e| e.burst(5));
  step(&mut world);
  assert_eq!(count(&world, capped), 10, "the rest is dropped, not saved");
}

#[test]
fn play_resumes_a_stopped_emitter() {
  let mut world = testing::world();
  let effect = stream(&mut world);
  let fx = emitter_of(&mut world, effect).stopped();
  let e = spawn(&mut world, fx);
  assert!(!emitter(&world, e).is_playing());
  steps(&mut world, 10);
  assert_eq!(count(&world, e), 0, "made stopped");
  assert!(
    finished(&world).is_empty(),
    "it has not played, so not finished"
  );

  control(&mut world, e, ParticleEmitter::play);
  assert!(emitter(&world, e).is_playing());
  steps(&mut world, 10);
  assert_eq!(count(&world, e), 10);
  control(&mut world, e, ParticleEmitter::stop);
  steps(&mut world, 10);
  assert_eq!(count(&world, e), 10);
  control(&mut world, e, ParticleEmitter::play);
  steps(&mut world, 10);
  assert_eq!(count(&world, e), 20, "it carries on where it stopped");

  // A one-cycle emitter whose cycle ended plays its cycle again
  let effect = still(&mut world, 10.0)
    .with_duration(0.5)
    .with_looping(false)
    .with_burst(Burst::at(0.0, 4.0));
  let fx = emitter_of(&mut world, effect);
  let once = spawn(&mut world, fx);
  step(&mut world);
  assert_eq!(count(&world, once), 4);
  steps(&mut world, 60);
  assert_eq!(count(&world, once), 4);
  assert!(!emitter(&world, once).is_playing());
  control(&mut world, once, ParticleEmitter::play);
  step(&mut world);
  assert_eq!(count(&world, once), 8, "its burst at 0 again");
  assert!(emitter(&world, once).is_playing());
}

#[test]
fn despawn_when_finished_despawns_after_the_last_particle_and_reports_it() {
  let mut world = testing::world();
  let effect = still(&mut world, 0.25)
    .with_duration(0.1)
    .with_looping(false)
    .with_burst(Burst::at(0.0, 3.0));
  let fx = emitter_of(&mut world, effect).despawn_when_finished();
  let e = spawn(&mut world, fx);
  let child = world.spawn_child(e, Transform::default());
  step(&mut world);
  assert_eq!(count(&world, e), 3);
  let mut n = 1;
  while finished(&world).is_empty() {
    assert!(count(&world, e) > 0, "reported in the step the last dies");
    step(&mut world);
    n += 1;
    assert!(n < 120, "it never finished");
  }
  assert!((15..=17).contains(&n), "the last died at 0.25 s, step {n}");
  assert_eq!(finished(&world), [e]);
  assert!(emitter(&world, e).is_finished());
  assert!(exists(&world, e), "it goes in the step after it finishes");

  step(&mut world);
  assert!(!exists(&world, e), "despawned");
  assert!(!exists(&world, child), "with its children");
  steps(&mut world, 10);
  assert_eq!(finished(&world), [e], "reported once");
  assert_eq!(live(&world), 0);
  let store = world.resource::<ParticleStore>();
  assert!(
    store.pools.iter().all(|pool| !pool.used),
    "its pool is free"
  );
}

#[test]
fn play_at_plays_one_cycle_of_a_looping_effect_and_cleans_up() {
  let mut world = testing::world();
  let effect = still(&mut world, 0.2)
    .with_duration(0.5)
    .with_looping(true)
    .with_burst(Burst::at(0.0, 4.0));
  let fx = testing::effect(&mut world, effect);
  let e = ParticlesView::new(&mut world).play_at(&fx, Vec2::new(30.0, 40.0));
  assert_eq!(
    world.get::<Transform>(e).map(|t| t.translation),
    Some(Vec2::new(30.0, 40.0))
  );
  step(&mut world);
  let first = particles(&world, e);
  assert_eq!(first.len(), 4);
  assert!(first.iter().all(|p| p.x == 30.0 && p.y == 40.0));

  steps(&mut world, 19);
  assert_eq!(count(&world, e), 0, "they died at 0.2 s");
  assert!(emitter(&world, e).is_playing(), "its cycle runs to 0.5 s");
  assert!(finished(&world).is_empty());

  // Looping, its burst would fire again at 0.5 s
  steps(&mut world, 15);
  assert!(!exists(&world, e), "one cycle, then gone");
  assert_eq!(finished(&world), [e]);
  assert_eq!(live(&world), 0);
}

#[test]
fn an_effect_with_nothing_to_emit_finishes_when_its_cycle_ends() {
  let mut world = testing::world();
  let effect = still(&mut world, 1.0)
    .with_duration(0.5)
    .with_looping(false);
  let fx = emitter_of(&mut world, effect);
  let e = spawn(&mut world, fx);
  steps(&mut world, 29);
  assert!(emitter(&world, e).is_playing());
  assert!(!emitter(&world, e).is_finished());
  assert!(finished(&world).is_empty());

  step(&mut world);
  assert!(!emitter(&world, e).is_playing(), "its cycle ended at 0.5 s");
  assert!(emitter(&world, e).is_finished());
  assert_eq!(finished(&world), [e]);
  steps(&mut world, 30);
  assert_eq!(finished(&world), [e], "once per finish");
  assert!(exists(&world, e), "it did not ask to be despawned");

  // Playing again re-arms the report; so does restarting
  control(&mut world, e, ParticleEmitter::play);
  steps(&mut world, 30);
  assert_eq!(finished(&world), [e, e]);
  control(&mut world, e, ParticleEmitter::restart);
  steps(&mut world, 30);
  assert_eq!(finished(&world), [e, e, e]);
}

/// What the `Particles` parameter read
#[derive(Resource, Default)]
struct Read {
  count: u32,
  budget: u32,
  played: Option<Entity>,
}

/// The effect a system plays
#[derive(Resource)]
struct Effect(Handle<ParticleEffect>);

fn read(particles: Particles, mut read: ResMut<Read>) {
  read.count = particles.count();
  read.budget = particles.budget();
}

fn play(mut particles: Particles, effect: Res<Effect>, mut read: ResMut<Read>) {
  read.played = Some(particles.play_at(&effect.0, Vec2::new(5.0, 6.0)));
}

fn run_once<M>(
  world: &mut World,
  system: impl IntoScheduleConfigs<crate::core::ecs::ScheduleSystem, M>,
) {
  let mut schedule = Schedule::default();
  schedule.add_systems(system);
  schedule.run(world);
}

#[test]
fn count_and_budget_read_the_store() {
  let mut world = testing::world();
  world.insert_resource(ParticleStore::new(50, 1));
  let effect = still(&mut world, 10.0)
    .with_duration(100.0)
    .with_burst(Burst::at(0.0, 7.0));
  let fx = testing::effect(&mut world, effect);
  spawn(&mut world, ParticleEmitter::new(fx.clone()));
  step(&mut world);
  let view = ParticlesView::new(&mut world);
  assert_eq!((view.count(), view.budget()), (7, 50));

  world.insert_resource(Read::default());
  run_once(&mut world, read);
  let seen = world.resource::<Read>();
  assert_eq!((seen.count, seen.budget), (7, 50), "the parameter too");

  world.insert_resource(Effect(fx));
  run_once(&mut world, play);
  let played = world.resource::<Read>().played.expect("it played");
  assert_eq!(
    world.get::<Transform>(played).map(|t| t.translation),
    Some(Vec2::new(5.0, 6.0))
  );
  let one_shot = emitter(&world, played);
  assert!(one_shot.once && one_shot.despawn_when_finished);
  step(&mut world);
  assert_eq!(count(&world, played), 7);
  assert_eq!(ParticlesView::new(&mut world).count(), 14);

  ParticlesView::new(&mut world).clear();
  assert_eq!(ParticlesView::new(&mut world).count(), 0);
  step(&mut world);
  assert_eq!(live(&world), 0, "every particle went");
}

#[test]
fn a_world_without_particles_reads_zero() {
  let image = Assets::<Image>::new().add(Image::solid_rect(8, 8, 255, 255, 255, 255));
  let fx = Assets::<ParticleEffect>::new().add(ParticleEffect::new(image));
  let mut world = World::new();
  let mut view = ParticlesView::new(&mut world);
  assert_eq!(view.count(), 0);
  assert_eq!(view.budget(), DEFAULT_PARTICLE_BUDGET);
  view.clear();
  let e = view.play_at(&fx, Vec2::new(1.0, 2.0));
  assert_eq!(
    world.get::<Transform>(e).map(|t| t.translation),
    Some(Vec2::new(1.0, 2.0))
  );
  let one_shot = emitter(&world, e);
  assert!(one_shot.is_playing() && one_shot.once && one_shot.despawn_when_finished);

  world.insert_resource(Read {
    budget: 1,
    count: 1,
    played: None,
  });
  run_once(&mut world, read);
  let seen = world.resource::<Read>();
  assert_eq!((seen.count, seen.budget), (0, DEFAULT_PARTICLE_BUDGET));
}

// Unscaled emitters

fn timestep(world: &World) -> Duration {
  world.resource::<Time>().fixed.timestep_duration()
}

#[test]
fn an_unscaled_emitter_runs_on_real_time_in_its_own_pass() {
  let mut world = testing::world();
  let effect = stream(&mut world);
  let fx = testing::effect(&mut world, effect);
  let unscaled = spawn(&mut world, ParticleEmitter::new(fx.clone()).unscaled());
  let game = spawn(&mut world, ParticleEmitter::new(fx));
  steps(&mut world, 5);
  assert_eq!(count(&world, unscaled), 0, "the fixed step leaves it alone");
  assert_eq!(count(&world, game), 5);
  let ts = timestep(&world);
  for _ in 0..5 {
    frame(&mut world, ts);
  }
  assert_eq!(count(&world, unscaled), 5, "a step a frame at 60 Hz");
  assert_eq!(count(&world, game), 5, "frames leave game time alone");

  // A paused clock, hitstop and a held scene do not hold it (an emitter of
  // no scene is held while any scene is)
  world.resource_mut::<Time>().game.pause();
  world.resource_mut::<Time>().game.hitstop(10.0);
  scope::hold_scenes(&mut world, [1].into_iter());
  for _ in 0..5 {
    frame(&mut world, ts);
  }
  assert_eq!(count(&world, unscaled), 10);

  // Its speed scales the real time it steps on
  control(&mut world, unscaled, |e| e.set_speed(2.0));
  frame(&mut world, ts);
  assert_eq!(count(&world, unscaled), 12, "two steps in the frame");
}

#[test]
fn an_unscaled_emitter_takes_at_most_eight_steps_a_frame() {
  let mut world = testing::world();
  let effect = stream(&mut world);
  let fx = emitter_of(&mut world, effect).unscaled();
  let e = spawn(&mut world, fx);
  let ts = timestep(&world);
  frame(&mut world, ts * 20);
  assert_eq!(count(&world, e), 8, "the other twelve steps are dropped");
  assert_eq!(fraction(&world, e), 0.0);

  // How far it is through its next step is kept for the drawer
  frame(&mut world, ts / 2);
  assert_eq!(count(&world, e), 8, "half a step: none yet");
  assert!((fraction(&world, e) - 0.5).abs() < 1e-3);
  frame(&mut world, ts * 3 / 4);
  assert_eq!(count(&world, e), 9);
  assert!((fraction(&world, e) - 0.25).abs() < 1e-3);
}

#[test]
fn an_unscaled_one_shot_finishes_and_goes_on_real_time() {
  let mut world = testing::world();
  let effect = still(&mut world, 0.1)
    .with_duration(0.2)
    .with_burst(Burst::at(0.0, 2.0));
  let fx = emitter_of(&mut world, effect)
    .unscaled()
    .once()
    .despawn_when_finished();
  let e = spawn(&mut world, fx);
  world.resource_mut::<Time>().game.pause();
  let ts = timestep(&world);
  frame(&mut world, ts);
  assert_eq!(count(&world, e), 2);
  for _ in 0..30 {
    frame(&mut world, ts);
  }
  assert_eq!(finished(&world), [e]);
  assert!(!exists(&world, e), "gone in the step after its finish");
}
