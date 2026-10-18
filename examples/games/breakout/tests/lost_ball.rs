//! A ball that leaves past a bottom corner is still lost, headless

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

/// The launched ball, if one is in play
fn launched(world: &mut World) -> Option<Entity> {
  let mut balls = world.query::<(Entity, &Ball)>();
  balls
    .iter(world)
    .find(|(_, ball)| !ball.frozen)
    .map(|(entity, _)| entity)
}

#[test]
fn a_ball_leaving_past_a_bottom_corner_is_lost() {
  // Losing a life can end the game, which saves the high score: run from an
  // empty folder holding the levels the game loads, so the player's save is
  // never touched (the working directory is the process's; this file has
  // one test)
  let levels = std::env::current_dir().unwrap().join("levels");
  let folder = std::env::temp_dir().join(format!("breakout-lost-{}", std::process::id()));
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
  engine.tap_key(KeyCode::KeyW);
  engine.step_frames(2).unwrap();
  let Some(ball) = launched(engine.world_mut()) else {
    panic!("the ball launched");
  };

  // Just below the bottom of the left wall, heading down and out at the
  // steepest angle the game allows: it is past the wall before it is off
  // the bottom of the screen
  let horizontal = BALL_SPEED * BALL_MAX_HORIZONTAL_RATIO;
  let vertical = (BALL_SPEED * BALL_SPEED - horizontal * horizontal).sqrt();
  {
    let world = engine.world_mut();
    if let Some(mut transform) = world.get_mut::<Transform>(ball) {
      transform.translation = Vec2::new(BALL_SIZE, SCREEN_HEIGHT as f32 + 2.0);
    }
    if let Some(mut velocity) = world.get_mut::<Velocity>(ball) {
      *velocity = Velocity::new(-horizontal, vertical);
    }
  }

  let mut lost = false;
  for _ in 0..120 {
    engine.step().unwrap();
    if engine.world().get_entity(ball).is_err() {
      lost = true;
      break;
    }
  }
  assert!(lost, "the game noticed the ball was gone and took it away");
  assert!(
    launched(engine.world_mut()).is_none(),
    "a new ball waits on the paddle"
  );

  drop(engine);
  let _ = std::fs::remove_dir_all(&folder);
}
