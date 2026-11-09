//! Radial, tangential and noise forces on the engine's particles.

use crate::core::asset::Handle;
use crate::core::ecs::{Entity, Transform, World};
use crate::core::particles::ParticleEmitter;
use crate::core::particles::effect::{Burst, NoiseForce, ParticleEffect};
use crate::core::particles::testing::{self, Seen, particles, steps};

/// One particle a second long burst at the emitter, drifting right at 10 px/s
fn drifting(world: &mut World) -> ParticleEffect {
  ParticleEffect::new(testing::image(world))
    .with_rate(0.0)
    .with_duration(100.0)
    .with_burst(Burst::at(0.0, 1.0))
    .with_speed(10.0)
    .with_direction(0.0, 0.0)
    .with_lifetime(10.0)
}

fn run(effect: impl FnOnce(&mut World) -> ParticleEffect, n: usize) -> Vec<Seen> {
  let mut world = testing::world();
  let effect = effect(&mut world);
  let effect: Handle<ParticleEffect> = testing::effect(&mut world, effect);
  let emitter: Entity = world
    .spawn((Transform::from_xy(0.0, 0.0), ParticleEmitter::new(effect)))
    .id();
  steps(&mut world, n);
  particles(&world, emitter)
}

#[test]
fn radial_acceleration_pushes_away_from_the_emitter() {
  let out = run(|w| drifting(w).with_radial_acceleration(100.0), 30);
  assert!(out[0].vx > 40.0, "{:?}", out[0]);
  assert!(out[0].vy.abs() < 1e-3, "{:?}", out[0]);

  // A negative value pulls in: it slows from its 10 px/s, and the pull
  // keeps it near the emitter
  let inward = run(|w| drifting(w).with_radial_acceleration(-100.0), 30);
  assert!(inward[0].vx < 10.0 && inward[0].x < 1.0, "{:?}", inward[0]);
}

#[test]
fn tangential_acceleration_turns_counter_clockwise() {
  // +y is down, so counter-clockwise on screen from the right is upward
  let out = run(|w| drifting(w).with_tangential_acceleration(100.0), 30);
  assert!(out[0].vy < -10.0, "{:?}", out[0]);
  assert!(out[0].vx.abs() < 30.0, "{:?}", out[0]);

  let clockwise = run(|w| drifting(w).with_tangential_acceleration(-100.0), 30);
  assert!(clockwise[0].vy > 10.0, "{:?}", clockwise[0]);
}

#[test]
fn noise_moves_particles_smoothly_and_repeats_for_a_seed() {
  let noisy = |w: &mut World| {
    drifting(w)
      .with_speed(0.0)
      .with_seed(7)
      .with_noise(NoiseForce::new(200.0, 0.05))
  };
  let path = |n| {
    run(noisy, n)
      .iter()
      .map(|s| (s.vx, s.vy))
      .collect::<Vec<_>>()
  };
  let a = path(30);
  assert_eq!(a, path(30));
  assert!(a[0].0.abs() + a[0].1.abs() > 0.1, "{a:?}");

  // Smooth: one more step changes the velocity by little
  let b = path(31);
  assert!((a[0].0 - b[0].0).abs() < 20.0 && (a[0].1 - b[0].1).abs() < 20.0);

  // Another seed moves it differently
  let other = run(
    |w| {
      drifting(w)
        .with_speed(0.0)
        .with_seed(8)
        .with_noise(NoiseForce::new(200.0, 0.05))
    },
    30,
  );
  assert_ne!((other[0].vx, other[0].vy), a[0]);
}
