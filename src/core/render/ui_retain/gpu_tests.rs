//! Retention with what only the engine can do: a sprite's pixels replaced
//! in place, as hot reload does

use crate::core::asset::SpriteHandle;
use crate::core::engine::EngineBuilder;
use crate::core::graphics::Sprite;
use crate::core::testing::{HeadlessEngine, PixelPair};
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

/// A label in a bitmap font, in a UI that otherwise never changes
struct BitmapUi {
  sheet: SpriteHandle,
  font: Option<FontId>,
}

impl Scene for BitmapUi {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    self.font = ctx
      .fonts()
      .add_bitmap("Grid", self.sheet.clone(), (4, 4), "#.")
      .ok();
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    if let Some(font) = self.font {
      let style = TextStyle::new().font(TextFont::Id(font)).size(16.0);
      ui.draw_text_styled("#.#.#", 8.0, 8.0, &style);
    }
    SceneAction::Continue
  }
}

#[test]
fn a_bitmap_font_sheet_updated_in_place_redraws() {
  // Two 4x4 cells: "#" solid, "." empty
  let sheet_of = |rgba: [u8; 4]| {
    let mut pixels = vec![0u8; 8 * 4 * 4];
    for y in 0..4 {
      for x in 0..4 {
        pixels[(y * 8 + x) * 4..][..4].copy_from_slice(&rgba);
      }
    }
    Sprite::from_rgba(pixels, 8, 4)
  };
  let sheet = SpriteHandle::from_sprite(sheet_of([255, 0, 0, 255]), "retain-grid");
  let make = {
    let sheet = sheet.clone();
    move || {
      EngineBuilder::new()
        .resolution(160, 120)
        .start_scene(BitmapUi {
          sheet: sheet.clone(),
          font: None,
        })
    }
  };
  let Some(mut pair) = PixelPair::new(make) else {
    return;
  };
  pair.frames(10, "red cells");
  assert!(pair.on.ui_render_stats().kept);
  sheet.update(sheet_of([0, 0, 255, 255]));
  pair.frames(1, "blue cells, the same size");
  assert!(!pair.on.ui_render_stats().kept, "the update redraws the UI");
  pair.frames(5, "blue cells");
}

/// The settings screen as a node tree
struct NodeUi;

impl Scene for NodeUi {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.spawn_ui(crate::core::testing::fixtures::settings_tree());
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }
}

#[test]
fn the_rebuilds_overlay_keeps_an_idle_ui_image() {
  let builder = EngineBuilder::new()
    .resolution(640, 480)
    .start_scene(NodeUi);
  let Some(mut engine) = HeadlessEngine::new_or_skip(builder) else {
    return;
  };
  engine.debug_config_mut().show_ui_rebuilds = true;
  let _ = engine.step_frames(120);
  let mut kept = 0;
  for _ in 0..120 {
    let _ = engine.step();
    kept += u32::from(engine.ui_render_stats().kept);
  }
  assert!(
    kept >= 110,
    "the overlay kept the UI image in {kept} of 120 frames"
  );
}
