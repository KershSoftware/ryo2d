use flappy::components::Bird;
use flappy::scenes::MenuScene;
use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

fn birds(engine: &mut HeadlessEngine) -> usize {
  let world = engine.world_mut();
  world.query::<&Bird>().iter(world).count()
}

#[test]
fn confirm_starts_from_the_menu() {
  let Some(mut engine) = HeadlessEngine::new_or_skip(
    EngineBuilder::new()
      .resolution(800, 600)
      .start_scene(MenuScene::new()),
  ) else {
    return;
  };
  engine.step_frames(3).unwrap();
  assert_eq!(birds(&mut engine), 0, "the menu has no bird");
  // START is the menu's default focus, so Confirm presses it
  engine.tap_key(KeyCode::Enter);
  engine.step_frames(60).unwrap(); // past the fade
  assert_eq!(birds(&mut engine), 1, "the game scene spawned its bird");
}
