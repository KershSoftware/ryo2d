//! Physics benchmark: 1,000 bodies bouncing in a closed box with no gravity
//! and elastic, frictionless contacts, so nothing ever sleeps. One headless
//! frame runs exactly one fixed step; the empty scene is the frame overhead.

use criterion::{Criterion, criterion_group, criterion_main};
use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

const SIZE: f32 = 600.0;

struct Bouncing;

impl Scene for Bouncing {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    for (x, y, w, h) in [
      (SIZE / 2.0, -10.0, SIZE + 40.0, 20.0),
      (SIZE / 2.0, SIZE + 10.0, SIZE + 40.0, 20.0),
      (-10.0, SIZE / 2.0, 20.0, SIZE + 40.0),
      (SIZE + 10.0, SIZE / 2.0, 20.0, SIZE + 40.0),
    ] {
      ctx.world.spawn((
        Transform::from_xy(x, y),
        RigidBody::kinematic(),
        Collider::box_collider(w, h)
          .with_restitution(1.0)
          .with_friction(0.0),
      ));
    }
    for i in 0..1000usize {
      let (col, row) = ((i % 40) as f32, (i / 40) as f32);
      let angle = i as f32 * 2.399;
      let collider = if i % 2 == 0 {
        Collider::circle_collider(5.0)
      } else {
        Collider::box_collider(8.0, 8.0)
      };
      ctx.world.spawn((
        Transform::from_xy(20.0 + col * 14.0, 20.0 + row * 14.0),
        Velocity {
          dx: angle.cos() * 120.0,
          dy: angle.sin() * 120.0,
        },
        RigidBody::new().with_gravity_scale(0.0),
        collider.with_restitution(1.0).with_friction(0.0),
      ));
    }
  }
}

struct Empty;

impl Scene for Empty {}

fn frame(c: &mut Criterion) {
  for (name, builder) in [
    (
      "physics frame, 1000 bouncing bodies",
      EngineBuilder::new()
        .resolution(300, 300)
        .start_scene(Bouncing),
    ),
    (
      "physics frame, empty scene",
      EngineBuilder::new().resolution(300, 300).start_scene(Empty),
    ),
  ] {
    // Needs a GPU; without one this benchmark is skipped
    let Some(mut engine) = HeadlessEngine::new_or_skip(builder) else {
      return;
    };
    engine.step_frames(30).expect("warm-up frames");
    c.bench_function(name, |b| b.iter(|| engine.step().expect("frame")));
  }
}

criterion_group!(benches, frame);
criterion_main!(benches);
