//! The ball against the bricks, headless

use breakout::prelude::*;
use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

/// Copy the files in `from` into `to`
fn copy_files(from: &std::path::Path, to: &std::path::Path) {
  std::fs::create_dir_all(to).unwrap();
  for entry in std::fs::read_dir(from).unwrap() {
    let entry = entry.unwrap();
    std::fs::copy(entry.path(), to.join(entry.file_name())).unwrap();
  }
}

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
  // A lost game saves the high score: run from an empty folder holding the
  // levels the game loads, so the player's save is never touched (the
  // working directory is the process's; this file has one test)
  let levels = std::env::current_dir().unwrap().join("levels");
  let folder = std::env::temp_dir().join(format!("breakout-ball-{}", std::process::id()));
  copy_files(&levels, &folder.join("levels"));
  std::env::set_current_dir(&folder).unwrap();

  let Some(mut engine) = HeadlessEngine::new_or_skip(
    EngineBuilder::new()
      .resolution(SCREEN_WIDTH, SCREEN_HEIGHT)
      .fixed_hz(120)
      .start_scene(GameScene::new(0, None)),
  ) else {
    return;
  };
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

  drop(engine);
  let _ = std::fs::remove_dir_all(&folder);
}
