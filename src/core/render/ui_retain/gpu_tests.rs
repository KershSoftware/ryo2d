//! Retention with what only the engine can do: a sprite's pixels replaced
//! in place, as hot reload does

use crate::core::asset::SpriteHandle;
use crate::core::engine::EngineBuilder;
use crate::core::graphics::Sprite;
use crate::core::testing::PixelPair;
use crate::prelude::*;

/// A sprite and a label
struct SpriteUi(SpriteHandle);

impl Scene for SpriteUi {
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    ui.draw_sprite(&self.0, 10.0, 10.0);
    ui.draw_text("Sprite", 40.0, 10.0, Color::WHITE);
    SceneAction::Continue
  }
}

#[test]
fn a_sprite_updated_in_place_redraws() {
  let sprite = SpriteHandle::from_sprite(Sprite::solid_rect(8, 8, 255, 0, 0, 255), "swatch");
  let make = {
    let sprite = sprite.clone();
    move || {
      EngineBuilder::new()
        .resolution(160, 120)
        .start_scene(SpriteUi(sprite.clone()))
    }
  };
  let Some(mut pair) = PixelPair::new(make) else {
    return;
  };
  pair.frames(10, "red");
  assert!(pair.on.ui_render_stats().kept);
  sprite.update(Sprite::solid_rect(8, 8, 0, 0, 255, 255));
  pair.frames(1, "blue, the same size");
  assert!(!pair.on.ui_render_stats().kept, "the update redraws the UI");
  pair.frames(5, "blue");
  sprite.update(Sprite::solid_rect(16, 16, 0, 255, 0, 255));
  pair.frames(5, "green and larger");
}
