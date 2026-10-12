//! Foundation benchmarks: transform propagation over a 10,000-entity
//! hierarchy, and a whole headless frame drawing 10,000 sprites.

use criterion::{Criterion, criterion_group, criterion_main};
use ryo2d::ecs::propagate::propagate_transforms;
use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

#[derive(ScheduleLabel, Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Bench;

/// 100 roots, each with 10 children, each with 9 grandchildren: 10,000
/// entities, propagated once so the timed runs start from a settled world.
fn hierarchy_world() -> (World, Schedule) {
  let mut world = World::new();
  for _ in 0..100 {
    let root = world.spawn(Transform::from_xy(1.0, 1.0)).id();
    for _ in 0..10 {
      let child = world
        .spawn((Transform::from_xy(1.0, 0.0), ChildOf(root)))
        .id();
      for _ in 0..9 {
        world.spawn((Transform::from_xy(0.0, 1.0), ChildOf(child)));
      }
    }
  }
  let mut schedule = Schedule::new(Bench);
  schedule.add_systems(propagate_transforms);
  schedule.run(&mut world);
  world.clear_trackers();
  (world, schedule)
}

fn propagate(c: &mut Criterion) {
  let (mut world, mut schedule) = hierarchy_world();
  c.bench_function("propagate 10k, nothing moved", |b| {
    b.iter(|| {
      schedule.run(&mut world);
      world.clear_trackers();
    })
  });
  c.bench_function("propagate 10k, every root moved", |b| {
    b.iter(|| {
      for mut transform in world
        .query_filtered::<&mut Transform, Without<ChildOf>>()
        .iter_mut(&mut world)
      {
        transform.translation.x += 0.001;
      }
      schedule.run(&mut world);
      world.clear_trackers();
    })
  });
}

/// 10,000 small sprites from three textures on a 100 x 100 grid.
struct ManySprites;

impl Scene for ManySprites {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let textures: Vec<SpriteHandle> = (0..3u8)
      .map(|i| {
        let shade = 60 + i * 80;
        SpriteHandle::from_sprite(
          Sprite::solid_rect(2, 2, shade, 30, 40, 255),
          format!("bench-{i}"),
        )
      })
      .collect();
    for i in 0..10_000usize {
      let (col, row) = ((i % 100) as f32, (i / 100) as f32);
      ctx.world.spawn((
        Transform::from_xy(col * 3.0 + 1.0, row * 3.0 + 1.0),
        Renderable::sprite(textures[i % 3].clone()),
      ));
    }
  }
}

fn frame(c: &mut Criterion) {
  let builder = EngineBuilder::new()
    .resolution(300, 300)
    .start_scene(ManySprites);
  // Needs a GPU; without one this benchmark is skipped
  let Some(mut engine) = HeadlessEngine::new_or_skip(builder) else {
    return;
  };
  engine.step_frames(2).expect("warm-up frames");
  c.bench_function("headless frame, 10k sprites", |b| {
    b.iter(|| engine.step().expect("frame"))
  });
}

criterion_group!(benches, propagate, frame);
criterion_main!(benches);
