//! A requested capture is the frame the engine presented.

use super::HeadlessEngine;
use crate::core::capture::CaptureTarget;
use crate::core::engine::EngineBuilder;
use crate::prelude::*;

/// CPU sprites and UI over the clear colour, the same every frame.
struct Fixture;

impl Scene for Fixture {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let red = SpriteHandle::from_sprite(Sprite::solid_rect(20, 20, 255, 0, 0, 255), "red");
    let blue = SpriteHandle::from_sprite(Sprite::solid_rect(30, 12, 0, 60, 255, 255), "blue");
    ctx
      .world
      .spawn((Transform::from_xy(10.0, 10.0), Renderable::sprite(red)));
    ctx
      .world
      .spawn((Transform::from_xy(40.0, 30.0), Renderable::sprite(blue)));
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    ui.draw_rect(Rect::new(30.0, 5.0, 25.0, 25.0), Color::new(0, 255, 0, 128));
    SceneAction::Continue
  }
}

#[test]
fn capture_matches_the_presented_frame() {
  let builder = EngineBuilder::new().resolution(96, 64).start_scene(Fixture);
  let Some(mut engine) = HeadlessEngine::new_or_skip(builder) else {
    return;
  };
  engine.step_frames(2).unwrap();

  let path = std::env::temp_dir().join(format!(
    "ryo2d-capture-matches-frame-{}.png",
    std::process::id()
  ));
  engine
    .engine
    .capture
    .request(CaptureTarget::Path(path.clone()));
  // A manual request captures the frame after the next one
  engine.step_frames(2).unwrap();
  engine.engine.capture.finish();

  let frame = engine.frame().unwrap();
  let captured = image::open(&path).expect("capture written").to_rgba8();
  let _ = std::fs::remove_file(&path);

  assert_eq!(captured.dimensions(), frame.dimensions());
  assert!(
    captured.as_raw() == frame.as_raw(),
    "captured image differs from the presented frame"
  );
}
