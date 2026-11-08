//! The ball against the bricks, headless

use breakout::prelude::*;
use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

/// The first launched ball's centre y and vertical velocity
fn ball(world: &mut World) -> Option<(f32, f32)> {
  let mut balls = world.query::<(&Ball, &Transform, &Velocity)>();
  balls
    .iter(world)
    .find(|(ball, ..)| !ball.frozen)
    .map(|(_, transform, velocity)| (transform.translation.y, velocity.dy))
}

#[test]
fn a_brick_sends_the_ball_back_down() {
  let Some(mut engine) = HeadlessEngine::new_or_skip(
    EngineBuilder::new()
      .resolution(SCREEN_WIDTH, SCREEN_HEIGHT)
      .fixed_hz(120)
      .asset_source(breakout::embedded())
      .start_scene(GameScene::new(0)),
  ) else {
    return;
  };
  // The bricks land with their map, a few frames in
  for _ in 0..600 {
    engine.step().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(5));
    let world = engine.world_mut();
    if world.query::<&Block>().iter(world).count() > 0 {
      break;
    }
  }
  engine.step_frames(5).unwrap();
  // Launch straight up from the centred, still paddle
  engine.tap_key(KeyCode::KeyW);

  let mut bricks = None;
  let mut broken_at = None;
  let mut back_down = false;
  for frame in 0..600 {
    engine.step().unwrap();
    let world = engine.world_mut();
    let count = world.query::<&Block>().iter(world).count();
    let start = *bricks.get_or_insert(count);
    let Some((y, dy)) = ball(world) else {
      continue;
    };
    if count < start {
      broken_at.get_or_insert(frame);
    }
    if broken_at.is_none() {
      continue;
    }
    assert_eq!(
      count,
      start - 1,
      "frame {frame}: one brick breaks, not a column"
    );
    if y > 400.0 {
      back_down = true;
      break;
    }
    assert!(
      dy > 0.0,
      "frame {frame}: the ball heads away from the bricks (dy {dy})"
    );
  }
  assert!(broken_at.is_some(), "the ball reached the bricks");
  assert!(back_down, "the ball came back down to the paddle");
}
