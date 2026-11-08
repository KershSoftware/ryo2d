//! A ball that leaves past a bottom corner is still lost, headless

use breakout::prelude::*;
use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

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
  let Some(mut engine) = HeadlessEngine::new_or_skip(
    EngineBuilder::new()
      .resolution(SCREEN_WIDTH, SCREEN_HEIGHT)
      .fixed_hz(120)
      .asset_source(breakout::embedded())
      .start_scene(GameScene::new(0)),
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
}
