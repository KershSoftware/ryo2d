//! Attractors pulling the engine's particles, by layer.

use crate::core::ecs::{Transform, World};
use crate::core::particles::ParticleEmitter;
use crate::core::particles::attractor::ParticleAttractor;
use crate::core::particles::effect::{Burst, ParticleEffect};
use crate::core::particles::testing::{self, particles, steps};

/// One motionless particle at the origin and a strong point attractor 100px to its right.
fn particle_near_attractor(emitter_layer: u32, attractor_mask: u32) -> f32 {
  let mut world: World = testing::world();
  let effect = ParticleEffect::new(testing::image(&mut world))
    .with_rate(0.0)
    .with_looping(false)
    .with_burst(Burst::at(0.0, 1.0))
    .with_speed(0.0)
    .with_lifetime(10.0)
    .with_layer(emitter_layer);
  let effect = testing::effect(&mut world, effect);
  let emitter = world
    .spawn((Transform::from_xy(0.0, 0.0), ParticleEmitter::new(effect)))
    .id();
  world.spawn((
    Transform::from_xy(100.0, 0.0),
    ParticleAttractor::point(500.0, 1000.0).with_layer_mask(attractor_mask),
  ));

  steps(&mut world, 2);

  particles(&world, emitter)[0].vx
}

#[test]
fn layered_attractor_pulls_particles_on_its_layer() {
  assert!(particle_near_attractor(0b10, 0b10) > 0.0);
}

#[test]
fn layered_attractor_ignores_particles_on_other_layers() {
  assert_eq!(particle_near_attractor(0b01, 0b10), 0.0);
}
