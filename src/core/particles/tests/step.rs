//! The engine's particle step, emitter by emitter.

use std::f32::consts::FRAC_PI_2;

use glam::Vec2;

use crate::core::asset::{Assets, Handle};
use crate::core::ecs::kernel::Disabled;
use crate::core::ecs::{Entity, Interpolated, Transform, World, WorldExt};
use crate::core::particles::ParticleEmitter;
use crate::core::particles::effect::{Burst, EmitShape, ParticleEffect, Space};
use crate::core::particles::scope;
use crate::core::particles::store::{DEFAULT_PARTICLE_BUDGET, ParticleStore};
use crate::core::particles::testing::{
  self, Seen, count, drawn, particle_warnings, particles, snapshot, step, steps,
};
use crate::core::particles::value::{Curve, Gradient};
use crate::core::ui::Color;

const DT: f32 = 1.0 / 60.0;

/// The default effect
fn plain(world: &mut World) -> ParticleEffect {
  ParticleEffect::new(testing::image(world))
}

/// Bursts only, standing still, each particle living `lifetime`
fn still(world: &mut World, lifetime: f32) -> ParticleEffect {
  plain(world)
    .with_rate(0.0)
    .with_speed(0.0)
    .with_lifetime(lifetime)
}

/// An emitter of `effect` at (`x`, `y`)
fn spawn(world: &mut World, effect: ParticleEffect, x: f32, y: f32) -> Entity {
  let effect = testing::effect(world, effect);
  emitter_of(world, effect, Transform::from_xy(x, y))
}

fn emitter_of(world: &mut World, effect: Handle<ParticleEffect>, at: Transform) -> Entity {
  world.spawn((at, ParticleEmitter::new(effect))).id()
}

fn moved_to(world: &mut World, emitter: Entity, x: f32) {
  if let Some(mut transform) = world.get_mut::<Transform>(emitter) {
    transform.translation.x = x;
  }
}

fn live(world: &World) -> u32 {
  world.resource::<ParticleStore>().live()
}

fn close(a: f32, b: f32, within: f32) -> bool {
  (a - b).abs() <= within
}

// Emission

#[test]
fn an_emitter_emits_with_no_scene_code() {
  let mut world = testing::world();
  let effect = plain(&mut world).with_rate(10.0);
  let emitter = spawn(&mut world, effect, 100.0, 100.0);
  steps(&mut world, 59);
  assert_eq!(count(&world, emitter), 9);
  step(&mut world);
  assert_eq!(count(&world, emitter), 10);
  assert_eq!(live(&world), 10);
  assert_eq!(
    world.get::<ParticleEmitter>(emitter).map(|e| e.alive()),
    Some(10)
  );
  for seen in particles(&world, emitter) {
    assert!(seen.x.is_finite() && seen.y.is_finite());
  }
}

#[test]
fn bursts_fire_on_time_with_their_cycles() {
  let mut world = testing::world();
  let effect = still(&mut world, 10.0)
    .with_duration(2.0)
    .with_looping(false)
    .with_burst(Burst::at(0.5, 5.0).with_cycles(3, 0.25));
  let emitter = spawn(&mut world, effect, 0.0, 0.0);
  steps(&mut world, 29);
  assert_eq!(count(&world, emitter), 0, "before step 30");
  step(&mut world);
  assert_eq!(count(&world, emitter), 5, "at step 30");
  steps(&mut world, 14);
  assert_eq!(count(&world, emitter), 5);
  step(&mut world);
  assert_eq!(count(&world, emitter), 10, "at step 45");
  steps(&mut world, 14);
  assert_eq!(count(&world, emitter), 10);
  step(&mut world);
  assert_eq!(count(&world, emitter), 15, "at step 60");
  steps(&mut world, 120);
  assert_eq!(count(&world, emitter), 15, "three cycles, no more");
}

#[test]
fn five_bursts_half_a_second_apart_need_a_cycle_that_holds_them() {
  // The repeating burst the particles skill shows: a burst fires only
  // within its cycle, so five 0.5 s apart take a cycle of 2.5 s
  let mut world = testing::world();
  let effect = still(&mut world, 10.0)
    .with_burst(Burst::at(0.0, 20.0).with_cycles(5, 0.5))
    .with_duration(2.5)
    .with_looping(false);
  let emitter = spawn(&mut world, effect, 0.0, 0.0);
  steps(&mut world, 30);
  assert_eq!(count(&world, emitter), 40, "at 0 and 0.5 s");
  steps(&mut world, 90);
  assert_eq!(count(&world, emitter), 100, "the fifth at 2 s");
  steps(&mut world, 240);
  assert_eq!(count(&world, emitter), 100, "five bursts, no more");
}

#[test]
fn a_hitch_fires_every_burst_it_owes() {
  // Bursts 5 ms apart, three or four to each 16.7 ms step
  let mut world = testing::world();
  let effect = still(&mut world, 10.0)
    .with_duration(10.0)
    .with_burst(Burst::at(0.0, 1.0).with_cycles(0, 0.005));
  let emitter = spawn(&mut world, effect, 0.0, 0.0);
  step(&mut world);
  assert_eq!(count(&world, emitter), 4, "0, 5, 10 and 15 ms");
  steps(&mut world, 59);
  assert_eq!(count(&world, emitter), 201, "every burst from 0 to 1 s");
}

#[test]
fn a_looping_effect_restarts_its_bursts_each_cycle() {
  let mut world = testing::world();
  let effect = still(&mut world, 10.0)
    .with_duration(0.5)
    .with_looping(true)
    .with_burst(Burst::at(0.1, 3.0));
  let emitter = spawn(&mut world, effect, 0.0, 0.0);
  steps(&mut world, 5);
  assert_eq!(count(&world, emitter), 0);
  step(&mut world);
  assert_eq!(count(&world, emitter), 3, "0.1 s into the first cycle");
  steps(&mut world, 29);
  assert_eq!(count(&world, emitter), 3);
  step(&mut world);
  assert_eq!(count(&world, emitter), 6, "0.1 s into the second");
  steps(&mut world, 30);
  assert_eq!(count(&world, emitter), 9, "and the third");
}

#[test]
fn a_looping_stream_keeps_its_rate_through_every_cycle() {
  // Each cycle ends 5 µs after 15 steps: inside the tolerance that makes a
  // burst due, so the next starts a little early, and the next earlier
  // still. The stream must not lose that early start as if it were a delay
  let mut world = testing::world();
  world.insert_resource(ParticleStore::new(100_000, 1));
  let effect = plain(&mut world)
    .with_rate(3000.0)
    .with_lifetime(100.0)
    .with_speed(0.0)
    .with_max_particles(100_000)
    .with_duration(15.0 * DT + 5e-6);
  let emitter = spawn(&mut world, effect, 0.0, 0.0);
  steps(&mut world, 600);
  let spawned = count(&world, emitter);
  assert!(
    spawned.abs_diff(30_000) <= 1,
    "{spawned} over 10 s at 3000 a second"
  );
}

#[test]
fn a_finished_cycle_stops_a_non_looping_effect() {
  let mut world = testing::world();
  let effect = plain(&mut world)
    .with_rate(10.0)
    .with_lifetime(10.0)
    .with_duration(1.0)
    .with_looping(false)
    .with_burst(Burst::at(0.0, 2.0));
  let emitter = spawn(&mut world, effect, 0.0, 0.0);
  steps(&mut world, 60);
  assert_eq!(count(&world, emitter), 12);
  steps(&mut world, 120);
  assert_eq!(count(&world, emitter), 12, "nothing after its one cycle");
}

#[test]
fn start_delay_holds_the_first_spawn() {
  let mut world = testing::world();
  let effect = plain(&mut world)
    .with_rate(60.0)
    .with_lifetime(10.0)
    .with_start_delay(0.5)
    .with_burst(Burst::at(0.0, 4.0));
  let emitter = spawn(&mut world, effect, 0.0, 0.0);
  steps(&mut world, 29);
  assert_eq!(count(&world, emitter), 0, "still waiting");
  step(&mut world);
  assert_eq!(count(&world, emitter), 4, "the cycle starts at 0.5 s");
  step(&mut world);
  assert_eq!(count(&world, emitter), 5);
  steps(&mut world, 9);
  assert_eq!(count(&world, emitter), 14);
}

// Caps

#[test]
fn max_particles_caps_the_live_count() {
  let mut world = testing::world();
  let effect = plain(&mut world)
    .with_rate(1000.0)
    .with_lifetime(10.0)
    .with_max_particles(25);
  let emitter = spawn(&mut world, effect, 0.0, 0.0);
  steps(&mut world, 10);
  assert_eq!(count(&world, emitter), 25);
  steps(&mut world, 50);
  assert_eq!(count(&world, emitter), 25);
  assert_eq!(live(&world), 25);
}

#[test]
fn the_budget_drops_spawns_and_warns_once() {
  let _log = testing::log_lock();
  let warned = || {
    particle_warnings()
      .iter()
      .filter(|message| message.contains("budget of 50"))
      .count()
  };
  let mut world = testing::world();
  world.insert_resource(ParticleStore::new(50, 1));
  let effect = |world: &mut World| {
    plain(world)
      .with_rate(1000.0)
      .with_lifetime(0.5)
      .with_speed(0.0)
  };
  let first = effect(&mut world);
  let a = spawn(&mut world, first, 0.0, 0.0);
  let second = effect(&mut world);
  let b = spawn(&mut world, second, 50.0, 0.0);
  steps(&mut world, 10);
  assert_eq!(live(&world), 50);
  assert_eq!(count(&world, a) + count(&world, b), 50);
  assert_eq!(warned(), 2, "one warning per effect");
  let budget = particle_warnings()
    .into_iter()
    .find(|message| message.contains("budget of 50"))
    .unwrap_or_default();
  assert!(
    budget.contains("EngineBuilder::particle_budget"),
    "{budget}"
  );

  // The first die at 0.5 s, and new ones take their places: without them
  // the youngest would be 0.6 s old at 0.67 s
  steps(&mut world, 30);
  assert_eq!(live(&world), 50);
  let youngest = particles(&world, a)
    .into_iter()
    .chain(particles(&world, b))
    .map(|seen| seen.age)
    .fold(f32::INFINITY, f32::min);
  assert!(youngest < 0.2, "spawning resumed: {youngest}");
  assert_eq!(warned(), 2, "still once per effect");
}

// Shapes

/// A burst of 300 from `shape` at (100, 50), as offsets from the emitter
fn offsets(shape: EmitShape, at: Transform) -> Vec<Vec2> {
  let mut world = testing::world();
  let effect = still(&mut world, 10.0)
    .with_shape(shape)
    .with_burst(Burst::at(0.0, 300.0));
  let effect = testing::effect(&mut world, effect);
  let emitter = emitter_of(&mut world, effect, at);
  step(&mut world);
  let seen = particles(&world, emitter);
  assert_eq!(seen.len(), 300);
  seen
    .iter()
    .map(|s| Vec2::new(s.x, s.y) - at.translation)
    .collect()
}

#[test]
fn each_shape_spawns_inside_its_bounds() {
  let at = Transform::from_xy(100.0, 50.0);
  let circle = |arc_deg, thickness| EmitShape::Circle {
    radius: 10.0,
    arc_deg,
    thickness,
  };

  assert!(
    offsets(EmitShape::Point, at)
      .iter()
      .all(|d| *d == Vec2::ZERO)
  );

  let disc = offsets(circle(360.0, 1.0), at);
  assert!(disc.iter().all(|d| d.length() <= 10.0 + 1e-3));
  assert!(disc.iter().any(|d| d.length() < 5.0), "fills the disc");

  let rim = offsets(circle(360.0, 0.0), at);
  assert!(rim.iter().all(|d| close(d.length(), 10.0, 1e-3)));
  assert!(rim.iter().any(|d| d.x < -5.0) && rim.iter().any(|d| d.y < -5.0));

  // A quarter arc, from 0 (right) to 90 degrees (down)
  let arc = offsets(circle(90.0, 1.0), at);
  assert!(arc.iter().all(|d| d.length() <= 10.0 + 1e-3));
  assert!(arc.iter().all(|d| d.x >= -1e-3 && d.y >= -1e-3), "{arc:?}");
  assert!(arc.iter().any(|d| d.y > 2.0 * d.x) && arc.iter().any(|d| d.x > 2.0 * d.y));

  let rect = |thickness| EmitShape::Rect {
    size: Vec2::new(40.0, 20.0),
    thickness,
  };
  let filled = offsets(rect(1.0), at);
  assert!(
    filled
      .iter()
      .all(|d| d.x.abs() <= 20.0 + 1e-3 && d.y.abs() <= 10.0 + 1e-3)
  );
  assert!(filled.iter().any(|d| d.x.abs() < 10.0 && d.y.abs() < 5.0));
  let outline = offsets(rect(0.0), at);
  assert!(outline.iter().all(|d| {
    let inside = d.x.abs() <= 20.0 + 1e-3 && d.y.abs() <= 10.0 + 1e-3;
    let on_edge = close(d.x.abs(), 20.0, 1e-3) || close(d.y.abs(), 10.0, 1e-3);
    inside && on_edge
  }));

  let line = offsets(EmitShape::Line { length: 100.0 }, at);
  assert!(
    line
      .iter()
      .all(|d| d.x.abs() <= 50.0 + 1e-3 && d.y.abs() <= 1e-3)
  );
  assert!(line.iter().any(|d| d.x < -40.0) && line.iter().any(|d| d.x > 40.0));
}

#[test]
fn the_emitter_s_rotation_and_scale_turn_the_shape() {
  let line = EmitShape::Line { length: 100.0 };
  let turned = Transform::from_xy(100.0, 100.0).with_rotation(FRAC_PI_2);
  let along_y = offsets(line, turned);
  assert!(
    along_y
      .iter()
      .all(|d| d.x.abs() < 1e-3 && d.y.abs() <= 50.0 + 1e-3)
  );
  assert!(along_y.iter().any(|d| d.y < -40.0) && along_y.iter().any(|d| d.y > 40.0));

  let doubled = offsets(line, turned.with_uniform_scale(2.0));
  assert!(doubled.iter().all(|d| d.y.abs() <= 100.0 + 1e-3));
  assert!(doubled.iter().any(|d| d.y.abs() > 80.0));

  // Directions turn with the emitter; speed does not scale
  let mut world = testing::world();
  let effect = still(&mut world, 10.0)
    .with_speed(100.0)
    .with_direction(0.0, 0.0)
    .with_burst(Burst::at(0.0, 20.0));
  let effect = testing::effect(&mut world, effect);
  let emitter = emitter_of(&mut world, effect, turned.with_uniform_scale(2.0));
  step(&mut world);
  for seen in particles(&world, emitter) {
    assert!(
      close(seen.vx, 0.0, 1e-3) && close(seen.vy, 100.0, 1e-3),
      "{seen:?}"
    );
  }
}

#[test]
fn zero_and_mirrored_scale_are_safe() {
  // At zero scale every particle starts at the centre, with no NaN
  let mut world = testing::world();
  let effect = still(&mut world, 10.0)
    .with_speed(50.0)
    .with_shape(EmitShape::Circle {
      radius: 10.0,
      arc_deg: 360.0,
      thickness: 1.0,
    })
    .with_burst(Burst::at(0.0, 50.0))
    .outward();
  let effect = testing::effect(&mut world, effect);
  let at = Transform::from_xy(100.0, 100.0).with_scale(Vec2::ZERO);
  let emitter = emitter_of(&mut world, effect, at);
  step(&mut world);
  let seen = particles(&world, emitter);
  assert_eq!(seen.len(), 50);
  for s in &seen {
    assert_eq!((s.x, s.y), (100.0, 100.0));
    assert!(s.vx.is_finite() && s.vy.is_finite());
    assert!(close(Vec2::new(s.vx, s.vy).length(), 50.0, 1e-3));
  }
  steps(&mut world, 10);
  assert!(
    particles(&world, emitter)
      .iter()
      .all(|s| s.x.is_finite() && s.y.is_finite())
  );

  // A mirrored x scale mirrors directions, and the shape with them
  let mut world = testing::world();
  let effect = still(&mut world, 10.0)
    .with_speed(100.0)
    .with_direction(0.0, 0.0)
    .with_shape(EmitShape::Circle {
      radius: 10.0,
      arc_deg: 90.0,
      thickness: 1.0,
    })
    .with_burst(Burst::at(0.0, 50.0));
  let effect = testing::effect(&mut world, effect);
  let at = Transform::from_xy(100.0, 100.0).with_scale(Vec2::new(-1.0, 1.0));
  let emitter = emitter_of(&mut world, effect, at);
  step(&mut world);
  for s in particles(&world, emitter) {
    assert!(close(s.vx, -100.0, 1e-3) && close(s.vy, 0.0, 1e-3), "{s:?}");
    assert!(s.x <= 100.0 + 1e-3 && s.y >= 100.0 - 1e-3, "{s:?}");
  }
}

// Motion and looks

/// One particle of `effect`, after its spawn step and `n` more
fn one_after(effect: impl FnOnce(&mut World) -> ParticleEffect, n: usize) -> Vec<Seen> {
  let mut world = testing::world();
  let effect = effect(&mut world)
    .with_duration(100.0)
    .with_burst(Burst::at(0.0, 1.0));
  let emitter = spawn(&mut world, effect, 0.0, 0.0);
  steps(&mut world, n + 1);
  particles(&world, emitter)
}

#[test]
fn gravity_drag_and_speed_over_life_move_particles() {
  let fallen = one_after(|w| still(w, 10.0).with_gravity(0.0, 100.0), 60);
  assert!(close(fallen[0].vy, 100.0, 1e-2), "{:?}", fallen[0]);
  assert_eq!(fallen[0].vx, 0.0);
  assert!(fallen[0].y > 45.0 && fallen[0].y < 55.0, "{:?}", fallen[0]);

  let dragged = one_after(
    |w| {
      still(w, 10.0)
        .with_speed(100.0)
        .with_direction(0.0, 0.0)
        .with_drag(1.0)
    },
    60,
  );
  let expected = 100.0 * (1.0 - DT).powi(60);
  assert!(close(dragged[0].vx, expected, 1e-2), "{:?}", dragged[0]);

  // A curve from 1 to 0: nearly stopped at the end of its life
  let slowing = |w: &mut World| {
    still(w, 1.005)
      .with_speed(100.0)
      .with_direction(0.0, 0.0)
      .with_speed_over_life(Curve::new([(0.0, 1.0), (1.0, 0.0)]))
  };
  let start = one_after(slowing, 0);
  assert!(close(start[0].vx, 100.0, 1e-3));
  let end = one_after(slowing, 60);
  assert_eq!(end.len(), 1, "still alive");
  let speed = Vec2::new(end[0].vx, end[0].vy).length();
  assert!(speed < 1.0, "under 1% of 100: {speed}");
}

#[test]
fn size_and_colour_follow_their_curves() {
  let size_curve = Curve::new([(0.0, 1.0), (1.0, 0.0)]);
  let gradient = Gradient::new([(0.0, Color::WHITE), (1.0, Color::TRANSPARENT)]);
  let effect = |w: &mut World| {
    still(w, 1.0)
      .with_size(10.0)
      .with_size_over_life(size_curve.clone())
      .with_color(Color::new(255, 0, 0, 255))
      .with_color_over_life(gradient.clone())
  };
  let half = one_after(effect, 30);
  assert!(close(half[0].age, 0.5, 1e-4));
  assert!(
    close(half[0].size, 10.0 * size_curve.sample(0.5), 1e-3),
    "{:?}",
    half[0]
  );
  // Red, times the gradient's mix of its two keys halfway
  let tint = gradient.sample(0.5);
  let [r, g, b, a] = half[0].color;
  assert!(close(r, f32::from(tint.r) / 255.0, 0.01), "{:?}", half[0]);
  assert!(close(a, f32::from(tint.a) / 255.0, 0.01), "{:?}", half[0]);
  assert_eq!((g, b), (0.0, 0.0));
}

#[test]
fn dead_particles_leave_in_order() {
  let mut world = testing::world();
  let effect = plain(&mut world)
    .with_rate(60.0)
    .with_speed(0.0)
    .with_lifetime((0.2, 1.0));
  let emitter = spawn(&mut world, effect, 0.0, 0.0);
  steps(&mut world, 45);
  let seen = particles(&world, emitter);
  assert!(seen.len() < 45, "some died");
  assert!(
    seen.windows(2).all(|pair| pair[0].age >= pair[1].age),
    "oldest first: {seen:?}"
  );
  assert!(
    seen
      .windows(2)
      .any(|pair| pair[0].age - pair[1].age > 1.5 * DT),
    "a particle between two survivors died"
  );
}

// Movement

/// The `n` particles `emitter` spawned last, newest last
fn newest(world: &World, emitter: Entity, n: usize) -> Vec<Seen> {
  let seen = particles(world, emitter);
  seen[seen.len().saturating_sub(n)..].to_vec()
}

#[test]
fn a_moving_emitter_spreads_its_stream() {
  let mut world = testing::world();
  let effect = plain(&mut world)
    .with_rate(600.0)
    .with_speed(0.0)
    .with_lifetime(10.0);
  let emitter = spawn(&mut world, effect, 0.0, 0.0);
  step(&mut world);
  assert_eq!(count(&world, emitter), 10);
  assert!(particles(&world, emitter).iter().all(|s| s.x == 0.0));
  for (step_number, x) in [(2, 100.0), (3, 200.0)] {
    moved_to(&mut world, emitter, x);
    step(&mut world);
    assert_eq!(count(&world, emitter), 10 * step_number);
    let spawned = newest(&world, emitter, 10);
    for (i, seen) in spawned.iter().enumerate() {
      let f = (i + 1) as f32 / 10.0;
      assert!(close(seen.x, x - 100.0 + 100.0 * f, 1e-3), "{i}: {seen:?}");
      assert!(close(seen.age, (1.0 - f) * DT, 1e-6), "{i}: {seen:?}");
    }
    let mut ages: Vec<f32> = spawned.iter().map(|s| s.age).collect();
    ages.dedup();
    assert_eq!(ages.len(), 10, "distinct ages");
  }
}

/// An `Interpolated` emitter of `effect` at the origin, one step old and
/// snapshotted, ready for a move
fn interpolated_emitter(world: &mut World, effect: Handle<ParticleEffect>) -> Entity {
  let emitter = world
    .spawn((
      Transform::from_xy(0.0, 0.0),
      Interpolated,
      ParticleEmitter::new(effect),
    ))
    .id();
  step(world);
  snapshot(world);
  emitter
}

#[test]
fn a_moving_interpolated_emitter_s_new_particles_never_draw_ahead_of_it() {
  let mut world = testing::world();
  let effect = plain(&mut world)
    .with_rate(600.0)
    .with_speed(0.0)
    .with_lifetime(10.0);
  let effect = testing::effect(&mut world, effect);
  let emitter = interpolated_emitter(&mut world, effect);
  moved_to(&mut world, emitter, 100.0);
  step(&mut world);
  assert_eq!(count(&world, emitter), 20);

  // Drawn at the ship's place as the step begins, and at their own as it ends
  let simulated = newest(&world, emitter, 10);
  let (start, end) = (drawn(&world, emitter, 0.0), drawn(&world, emitter, 1.0));
  for (i, seen) in simulated.iter().enumerate() {
    let f = (i + 1) as f32 / 10.0;
    assert!(close(seen.x, 100.0 * f, 1e-3), "{i}: spread: {seen:?}");
    assert_eq!(start[10 + i].x, 0.0, "{i}: at the emitter it leaves");
    assert!(close(end[10 + i].x, seen.x, 1e-3), "{i}: {:?}", end[10 + i]);
  }

  // Next step they stand where they were put, with no slide back
  snapshot(&mut world);
  moved_to(&mut world, emitter, 200.0);
  step(&mut world);
  let start = drawn(&world, emitter, 0.0);
  for (i, seen) in simulated.iter().enumerate() {
    assert!(
      close(start[10 + i].x, seen.x, 1e-3),
      "{i}: {:?}",
      start[10 + i]
    );
  }
}

#[test]
fn a_moving_interpolated_emitter_s_new_particles_keep_their_place_in_its_shape() {
  let mut world = testing::world();
  let effect = plain(&mut world)
    .with_rate(600.0)
    .with_speed(0.0)
    .with_lifetime(10.0)
    .with_shape(EmitShape::Circle {
      radius: 5.0,
      arc_deg: 360.0,
      thickness: 1.0,
    });
  let effect = testing::effect(&mut world, effect);
  let emitter = interpolated_emitter(&mut world, effect);
  moved_to(&mut world, emitter, 100.0);
  step(&mut world);

  let (start, end) = (drawn(&world, emitter, 0.0), drawn(&world, emitter, 1.0));
  for i in 0..10 {
    let (start, end) = (start[10 + i], end[10 + i]);
    assert!(
      start.length() <= 5.0 + 1e-3,
      "{i}: round the origin: {start}"
    );
    let f = (i + 1) as f32 / 10.0;
    assert!(
      (end - start - Vec2::new(100.0 * f, 0.0)).length() < 1e-3,
      "{i}: its offset kept: {start} to {end}"
    );
  }
}

#[test]
fn only_a_spread_interpolated_stream_draws_from_the_emitter_s_last_place() {
  let mut world = testing::world();
  let stream = plain(&mut world)
    .with_rate(600.0)
    .with_speed(0.0)
    .with_lifetime(10.0);
  let local = stream.clone().with_space(Space::Local);
  let stream = testing::effect(&mut world, stream);

  // A teleport does not spread: they stand at the new place
  let teleported = interpolated_emitter(&mut world, stream.clone());
  moved_to(&mut world, teleported, 1000.0);
  world.skip_interpolation(teleported);
  step(&mut world);
  for alpha in [0.0, 0.5, 1.0] {
    for at in &drawn(&world, teleported, alpha)[10..] {
      assert_eq!(at.x, 1000.0, "at {alpha}");
    }
  }

  // A plain emitter draws where it stands, so its stream draws where placed
  let plain_emitter = emitter_of(&mut world, stream, Transform::from_xy(0.0, 0.0));
  step(&mut world);
  moved_to(&mut world, plain_emitter, 100.0);
  step(&mut world);
  let simulated = newest(&world, plain_emitter, 10);
  for alpha in [0.0, 0.5, 1.0] {
    let at = drawn(&world, plain_emitter, alpha);
    for (i, seen) in simulated.iter().enumerate() {
      assert_eq!(at[10 + i].x, seen.x, "{i} at {alpha}");
    }
  }

  // A `Local` stream's particles are born at the frame's origin, whichever
  // way the frame moves: they draw with the emitter
  let local = testing::effect(&mut world, local);
  let carried = interpolated_emitter(&mut world, local);
  moved_to(&mut world, carried, 100.0);
  step(&mut world);
  for (alpha, x) in [(0.0, 0.0), (0.5, 50.0), (1.0, 100.0)] {
    let at = drawn(&world, carried, alpha);
    assert!(close(at[at.len() - 1].x, x, 1e-3), "at {alpha}: {at:?}");
  }
}

#[test]
fn a_teleported_or_re_enabled_emitter_does_not_streak() {
  // Moved with `skip_interpolation`, it spawns at its new place only
  let mut world = testing::world();
  let effect = plain(&mut world)
    .with_rate(600.0)
    .with_speed(0.0)
    .with_lifetime(10.0);
  let effect = testing::effect(&mut world, effect);
  let emitter = world
    .spawn((
      Transform::from_xy(0.0, 0.0),
      Interpolated,
      ParticleEmitter::new(effect.clone()),
    ))
    .id();
  step(&mut world);
  snapshot(&mut world);
  moved_to(&mut world, emitter, 10.0);
  step(&mut world);
  assert!(
    newest(&world, emitter, 10).iter().any(|s| s.x < 9.0),
    "a normal step spreads"
  );
  snapshot(&mut world);
  moved_to(&mut world, emitter, 1000.0);
  world.skip_interpolation(emitter);
  step(&mut world);
  for seen in newest(&world, emitter, 10) {
    assert_eq!(seen.x, 1000.0, "no streak along the jump");
    assert_eq!(seen.age, 0.0);
  }
  // Without the skip, the same jump streaks
  snapshot(&mut world);
  moved_to(&mut world, emitter, 2000.0);
  step(&mut world);
  assert!(newest(&world, emitter, 10).iter().any(|s| s.x < 1900.0));

  // Enabled again after being disabled, it starts where it is now
  let emitter = emitter_of(&mut world, effect, Transform::from_xy(0.0, 500.0));
  steps(&mut world, 3);
  world.entity_mut(emitter).insert(Disabled);
  steps(&mut world, 3);
  moved_to(&mut world, emitter, 1000.0);
  world.entity_mut(emitter).remove::<Disabled>();
  step(&mut world);
  assert_eq!(count(&world, emitter), 40);
  for seen in newest(&world, emitter, 10) {
    assert_eq!(seen.x, 1000.0, "no streak from where it was disabled");
  }
}

#[test]
fn a_plain_emitter_teleported_between_steps_does_not_streak() {
  // Not interpolated, and moved between fixed steps (from an `Update`
  // system or a `ui` hook, say): `skip_interpolation` still marks the jump
  let mut world = testing::world();
  let effect = plain(&mut world)
    .with_rate(600.0)
    .with_speed(0.0)
    .with_lifetime(10.0);
  let emitter = spawn(&mut world, effect, 0.0, 0.0);
  steps(&mut world, 2);
  moved_to(&mut world, emitter, 1000.0);
  world.skip_interpolation(emitter);
  step(&mut world);
  for seen in newest(&world, emitter, 10) {
    assert_eq!(seen.x, 1000.0, "no streak along the jump");
    assert_eq!(seen.age, 0.0);
  }

  // The mark lasts one step: the next move spreads as usual
  moved_to(&mut world, emitter, 1100.0);
  step(&mut world);
  let spawned = newest(&world, emitter, 10);
  for (i, seen) in spawned.iter().enumerate() {
    let f = (i + 1) as f32 / 10.0;
    assert!(close(seen.x, 1000.0 + 100.0 * f, 1e-3), "{i}: {seen:?}");
  }

  // An interpolated one marked between steps keeps the mark through the
  // next step's snapshot, which clears `PreviousGlobalTransform.skip`
  let effect = plain(&mut world)
    .with_rate(600.0)
    .with_speed(0.0)
    .with_lifetime(10.0);
  let effect = testing::effect(&mut world, effect);
  let smooth = world
    .spawn((
      Transform::from_xy(0.0, 300.0),
      Interpolated,
      ParticleEmitter::new(effect),
    ))
    .id();
  steps(&mut world, 2);
  moved_to(&mut world, smooth, 1000.0);
  world.skip_interpolation(smooth);
  snapshot(&mut world);
  step(&mut world);
  for seen in newest(&world, smooth, 10) {
    assert_eq!(seen.x, 1000.0, "no streak after the snapshot");
  }
}

#[test]
fn an_effect_still_loading_emits_nothing_until_it_lands() {
  let _log = testing::log_lock();
  let warnings = particle_warnings().len();
  let mut world = testing::world();
  // A handle to a slot with no value yet, as a queued load hands out
  let effect: Handle<ParticleEffect> = world
    .resource::<Assets<ParticleEffect>>()
    .ledger()
    .lock()
    .unwrap()
    .reserve();
  let emitter = emitter_of(&mut world, effect.clone(), Transform::from_xy(0.0, 0.0));
  steps(&mut world, 30);
  assert_eq!(count(&world, emitter), 0);
  assert!(
    world
      .resource::<ParticleStore>()
      .compiled(effect.id())
      .is_none()
  );
  assert_eq!(particle_warnings().len(), warnings, "nothing to warn about");

  // It lands: its first cycle starts in that step
  let landed = still(&mut world, 10.0)
    .with_burst(Burst::at(0.0, 5.0))
    .with_burst(Burst::at(0.5, 3.0));
  assert!(
    world
      .resource_mut::<Assets<ParticleEffect>>()
      .insert(effect.id(), landed)
  );
  step(&mut world);
  assert_eq!(count(&world, emitter), 5);
  steps(&mut world, 28);
  assert_eq!(count(&world, emitter), 5);
  step(&mut world);
  assert_eq!(count(&world, emitter), 8, "0.5 s after it landed");
  assert_eq!(particle_warnings().len(), warnings);
}

// Determinism

fn seeded_run(store_seed: u64, unrelated: usize, other: bool) -> Vec<Seen> {
  let mut world = testing::world();
  world.insert_resource(ParticleStore::new(DEFAULT_PARTICLE_BUDGET, store_seed));
  crate::core::ecs::test_util::insert_unrelated(&mut world, unrelated);
  let effect = plain(&mut world)
    .with_rate(120.0)
    .with_lifetime((0.5, 1.5))
    .with_speed((10.0, 90.0))
    .with_size((2.0, 8.0))
    .with_color((Color::new(255, 0, 0, 255), Color::new(0, 0, 255, 255)))
    .with_shape(EmitShape::Circle {
      radius: 20.0,
      arc_deg: 360.0,
      thickness: 0.5,
    });
  let emitter = spawn(&mut world, effect, 50.0, 50.0);
  if other {
    let effect = plain(&mut world).with_rate(200.0).with_speed((0.0, 200.0));
    spawn(&mut world, effect, 300.0, 300.0);
  }
  steps(&mut world, 40);
  particles(&world, emitter)
}

#[test]
fn the_same_seed_repeats_and_others_do_not_disturb_it() {
  let base = seeded_run(1, 0, false);
  assert!(base.len() > 40);
  assert_eq!(seeded_run(1, 0, false), base, "the same seed repeats");
  for unrelated in 0..=crate::core::ecs::test_util::MAX_UNRELATED {
    for other in [false, true] {
      assert_eq!(
        seeded_run(1, unrelated, other),
        base,
        "{unrelated} unrelated resources, another emitter: {other}"
      );
    }
  }
  assert_ne!(seeded_run(2, 0, false), base, "another seed differs");
}

// Lifecycle

#[test]
fn a_disabled_emitter_is_frozen() {
  let mut world = testing::world();
  let effect = plain(&mut world)
    .with_rate(60.0)
    .with_lifetime(10.0)
    .with_speed(50.0);
  let emitter = spawn(&mut world, effect, 0.0, 0.0);
  steps(&mut world, 10);
  let before = particles(&world, emitter);
  world.entity_mut(emitter).insert(Disabled);
  steps(&mut world, 10);
  assert_eq!(
    particles(&world, emitter),
    before,
    "neither moved nor spawned"
  );
  world.entity_mut(emitter).remove::<Disabled>();
  step(&mut world);
  let after = particles(&world, emitter);
  assert_eq!(after.len(), before.len() + 1);
  assert_ne!(after[0], before[0]);
}

#[test]
fn despawning_an_emitter_frees_its_pool() {
  let mut world = testing::world();
  let effect = plain(&mut world).with_rate(600.0).with_lifetime(10.0);
  let effect = testing::effect(&mut world, effect);
  let first = emitter_of(&mut world, effect.clone(), Transform::from_xy(0.0, 0.0));
  steps(&mut world, 5);
  assert_eq!(live(&world), 50);
  let slot = world.get::<ParticleEmitter>(first).map(|e| e.slot);
  world.despawn(first);
  assert_eq!(live(&world), 0);
  let next = emitter_of(&mut world, effect, Transform::from_xy(0.0, 0.0));
  assert_eq!(world.get::<ParticleEmitter>(next).map(|e| e.slot), slot);
  step(&mut world);
  assert_eq!(live(&world), 10);
  assert_eq!(count(&world, next), 10);
}

#[test]
fn clearing_the_store_removes_every_particle() {
  let mut world = testing::world();
  let effect = plain(&mut world).with_rate(600.0).with_lifetime(10.0);
  let emitter = spawn(&mut world, effect, 0.0, 0.0);
  steps(&mut world, 3);
  assert_eq!(live(&world), 30);
  world.resource_mut::<ParticleStore>().clear_all();
  assert_eq!(live(&world), 0);
  assert_eq!(count(&world, emitter), 0);
  step(&mut world);
  assert_eq!(count(&world, emitter), 10, "the emitter carries on");
  assert_eq!(live(&world), 10);
}

// Holds

#[test]
fn held_scenes_freeze_their_emitters() {
  let mut world = testing::world();
  let effect = plain(&mut world)
    .with_rate(60.0)
    .with_lifetime(10.0)
    .with_speed(50.0);
  let effect = testing::effect(&mut world, effect);
  let at = Transform::from_xy(0.0, 0.0);
  let in_one = scope::in_scene(&mut world, 1, |world| emitter_of(world, effect.clone(), at));
  let of_none = emitter_of(&mut world, effect.clone(), at);
  let in_two = scope::in_scene(&mut world, 2, |world| emitter_of(world, effect.clone(), at));
  let seen = |world: &World| [in_one, of_none, in_two].map(|e| particles(world, e));
  steps(&mut world, 5);

  let before = seen(&world);
  scope::hold_scenes(&mut world, [1].into_iter());
  steps(&mut world, 5);
  let held = seen(&world);
  assert_eq!(held[0], before[0], "scene 1's emitter is held");
  assert_eq!(
    held[1], before[1],
    "an emitter of no scene is held while any scene is"
  );
  assert_ne!(held[2], before[2], "scene 2's emitter moves on");

  scope::hold_scenes(&mut world, std::iter::empty());
  step(&mut world);
  let freed = seen(&world);
  assert_ne!(freed[0], held[0], "let go, scene 1's moves again");
  assert_ne!(freed[1], held[1]);

  // Scene 1 exits: its emitter outlives it, of no scene now, so a hold of
  // scene 2 holds it
  scope::release_scene(&mut world, 1);
  scope::hold_scenes(&mut world, [2].into_iter());
  let before = seen(&world);
  steps(&mut world, 3);
  let after = seen(&world);
  assert_eq!(after[0], before[0]);
  assert_eq!(after[2], before[2]);
}

// Floors

#[test]
fn a_floor_emitter_emits_at_its_lifted_point_in_its_first_step() {
  use glam::UVec2;

  use crate::core::ecs::{Floor, IntoScheduleConfigs};
  use crate::core::particles::step::step_particles;
  use crate::core::schedule::{FixedSet, Phase};
  use crate::core::tilemap::test_util::MapWorld;
  use crate::core::tilemap::{MapRoot, TileMap};

  // The engine's fixed step, with a map whose floor 1 draws 32 px up
  let mut maps = MapWorld::new();
  let mut map = TileMap::new(UVec2::new(4, 3), UVec2::splat(16));
  map.add_tile_layer("ground");
  let upper = map.add_tile_layer("upper");
  map.layers[upper].info.floor = 1;
  map.layers[upper].info.offset = Vec2::new(0.0, -32.0);
  let map = maps.world.resource_mut::<Assets<TileMap>>().add(map);
  let root = maps
    .world
    .spawn((MapRoot::new(map), Transform::IDENTITY))
    .id();
  maps.settle(root);
  maps
    .world
    .insert_resource(ParticleStore::new(DEFAULT_PARTICLE_BUDGET, 1));
  maps.schedules().add_systems(
    Phase::FixedUpdate,
    step_particles.in_set(FixedSet::Particles),
  );

  // Spawned on floor 1 and fired in one step
  let effect = still(&mut maps.world, 10.0).with_burst(Burst::at(0.0, 3.0));
  let effect = testing::effect(&mut maps.world, effect);
  let emitter = maps
    .world
    .spawn((
      Transform::from_xy(20.0, 30.0),
      Floor::On(1),
      ParticleEmitter::new(effect),
    ))
    .id();
  maps.frame();
  let seen = particles(&maps.world, emitter);
  assert_eq!(seen.len(), 3);
  for s in seen {
    assert_eq!((s.x, s.y), (20.0, -2.0), "where it draws");
  }
}
