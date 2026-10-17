use breakout::scenes::PauseScene;
use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

/// The game under the pause menu, so popping the menu (Resume) comes back
/// here instead of emptying the stack, which would quit too
struct Under {
  pushed: bool,
}

impl Scene for Under {
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    if self.pushed {
      return SceneAction::Continue;
    }
    self.pushed = true;
    SceneAction::Push(Box::new(PauseScene::new(0, 0)))
  }
}

#[test]
fn the_pause_menu_is_navigable_by_pad() {
  let Some(mut engine) = HeadlessEngine::new_or_skip(
    EngineBuilder::new()
      .resolution(800, 600)
      .start_scene(Under { pushed: false }),
  ) else {
    return;
  };
  engine.step_frames(3).unwrap();
  // Resume is the default; down twice reaches Quit to Desktop
  for _ in 0..2 {
    engine.tap_pad(GamepadButton::DPadDown);
    engine.step_frames(2).unwrap();
  }
  engine.tap_pad(GamepadButton::South);
  let outcome = engine.step_frames(3).unwrap();
  assert_eq!(outcome, ryo2d::testing::StepOutcome::Quit);
}
