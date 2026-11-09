//! Collisions: rain splashing, sparks bouncing and embers sticking, on a
//! tile map built in code

use demo_kit::prelude::*;

use crate::shared::{Block, count_label, emit, label_centered, solid_map};

#[derive(Default)]
pub struct CollisionsPage {
  /// The effects, held while the page is up
  effects: Vec<Handle<ParticleEffect>>,
}

impl DemoPage for CollisionsPage {
  fn name(&self) -> &str {
    "Collisions"
  }

  fn description(&self) -> &str {
    "Rain splashes through on_collision, sparks bounce, embers stick"
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    ctx.camera().set_position(640.0, 360.0);
    solid_map(
      ctx,
      &[
        // The ground, and a ledge under each effect
        Block::new(0, 16, 32, 2),
        Block::new(2, 9, 13, 1),
        Block::new(17, 12, 9, 1),
        Block::new(27, 8, 5, 1),
      ],
    );
    self.effects.clear();
    for (path, x, y) in [
      ("fx/rain.particles.ron", 340.0, -20.0),
      ("fx/sparks.particles.ron", 860.0, 330.0),
      ("fx/embers.particles.ron", 1180.0, 190.0),
    ] {
      // The demo's own files: one that fails to load is a bug in the demo
      let effect = ctx
        .assets()
        .load_now::<ParticleEffect>(path)
        .unwrap_or_else(|error| panic!("{path}: {}", error.report()));
      emit(ctx, x, y, &effect);
      self.effects.push(effect);
    }
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) {
    count_label(ui, ctx);
    let blue = Color::new(160, 205, 255, 255);
    let orange = Color::new(255, 200, 110, 255);
    label_centered(ui, 340.0, 410.0, "Rain: dies, plays a splash", blue);
    label_centered(ui, 860.0, 535.0, "Sparks: bounce", orange);
    label_centered(ui, 1180.0, 375.0, "Embers: stick", orange);
  }
}
