//! The game scene headless: the player runs, jumps and lands on the engine's
//! platformer preset.

use bullet_hell::components::Player;
use bullet_hell::scenes::GameScene;
use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

/// The player's position and whether it stands on something.
fn player(engine: &mut HeadlessEngine) -> (Vec2, bool) {
  let world = engine.world_mut();
  let (transform, state) = world
    .query_filtered::<(&Transform, &CharacterState), With<Player>>()
    .single(world)
    .expect("one player");
  (transform.translation, state.on_floor)
}

#[test]
fn the_player_runs_jumps_and_lands() {
  let builder = EngineBuilder::new()
    .pixel_art(1920, 1080)
    .fixed_hz(144)
    .start_scene(GameScene::new());
  let Some(mut engine) = HeadlessEngine::new_or_skip(builder) else {
    return;
  };

  // It spawns in the air and falls to the ground
  engine.step_frames(150).expect("frames");
  let (start, on_floor) = player(&mut engine);
  assert!(on_floor, "the player landed on the ground, at {start}");

  // Run right for 60 frames (600 px/s at 144 Hz is 250 px), then jump
  engine.press_key(KeyCode::KeyD);
  engine.step_frames(60).expect("frames");
  engine.release_key(KeyCode::KeyD);
  engine.tap_key(KeyCode::Space);

  // A 154 px jump takes about 1.1 s (160 frames) to come back down
  let mut highest = f32::INFINITY;
  for _ in 0..200 {
    engine.step().expect("frame");
    highest = highest.min(player(&mut engine).0.y);
  }
  let (end, on_floor) = player(&mut engine);
  assert!(on_floor, "the player landed again, at {end}");
  assert!(
    end.x > start.x + 200.0,
    "the player ran right, from {start} to {end}"
  );
  let height = start.y - highest;
  assert!(
    (140.0..170.0).contains(&height),
    "the player jumped about 154 px, not {height}"
  );
  assert!(
    (end.y - start.y).abs() < 1.0,
    "back on the ground, at {end} from {start}"
  );
}
