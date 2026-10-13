//! 2D lighting through the engine: lights, shadows, ambient light and
//! atmosphere, read back as display bytes.
//!
//! Tests skip when no GPU is available.

use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

type Setup = Box<dyn FnMut(&mut SceneContext) + Send>;

/// A white floor over the whole 128x64 window, then `setup`.
struct Lit(Setup);

impl Scene for Lit {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let floor =
      SpriteHandle::from_sprite(Sprite::solid_rect(128, 64, 255, 255, 255, 255), "lit-floor");
    ctx.world.spawn((
      Transform::from_xy(0.0, 0.0),
      Renderable::sprite(floor).with_anchor(Anchor::TopLeft),
    ));
    (self.0)(ctx);
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }
}

/// The frame after two steps of the floor lit by `setup`.
fn lit(setup: impl FnMut(&mut SceneContext) + Send + 'static) -> Option<image::RgbaImage> {
  let scene = Lit(Box::new(setup));
  let builder = EngineBuilder::new().resolution(128, 64).start_scene(scene);
  let mut engine = HeadlessEngine::new_or_skip(builder)?;
  engine.step_frames(2).unwrap();
  Some(engine.frame().unwrap())
}

#[test]
fn ambient_mid_grey_lights_white_to_mid_grey() {
  let Some(frame) = lit(|ctx| {
    let mut lighting = ctx.lighting();
    lighting.set_ambient(AmbientLight::new(Color::rgb(128, 128, 128), 1.0));
    lighting.set_enabled(true);
  }) else {
    return;
  };
  let pixel = frame.get_pixel(64, 32).0;
  // Read as linear (the old rule) it showed about 188
  assert!(pixel[0].abs_diff(128) <= 1, "{pixel:?}");
}
