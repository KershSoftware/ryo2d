//! Files: effects read from `assets/fx`, which reload when saved

use demo_kit::prelude::*;

use crate::shared::{Block, count_label, emit, label_centered, solid_map};

/// The effects the page plays: the file under `assets/fx`, and where its
/// emitter sits. The sparks and embers fall on ledges, the splash is on
/// the ground and the rain comes from above
const EFFECTS: [(&str, f32, f32); 4] = [
  ("fx/sparks.particles.ron", 180.0, 220.0),
  ("fx/splash.particles.ron", 440.0, 545.0),
  ("fx/embers.particles.ron", 700.0, 300.0),
  ("fx/rain.particles.ron", 1020.0, -20.0),
];

#[derive(Default)]
pub struct FilesPage {
  /// The effects, held while the page is up
  effects: Vec<Handle<ParticleEffect>>,
}

impl DemoPage for FilesPage {
  fn name(&self) -> &str {
    "Effect Files"
  }

  fn description(&self) -> &str {
    "Effects from .particles.ron files: edit and save one while the demo runs"
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    ctx.camera().set_position(640.0, 360.0);
    solid_map(
      ctx,
      &[
        // The ground, and a ledge under the sparks and under the embers
        Block::new(0, 14, 32, 4),
        Block::new(2, 8, 5, 1),
        Block::new(15, 9, 5, 1),
      ],
    );
    self.effects.clear();
    for (path, x, y) in EFFECTS {
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
    let area = content_area(ui);
    for (path, x, _) in EFFECTS {
      let name = path.trim_start_matches("fx/");
      label_centered(ui, x, 585.0, name, Color::new(180, 220, 255, 255));
    }
    label_centered(
      ui,
      area.x + area.width / 2.0,
      area.y + area.height - 70.0,
      "Edit a file in assets/fx and save it: the effect reloads while the demo runs",
      Color::WHITE,
    );
  }
}
