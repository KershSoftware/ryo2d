//! Trails: a comet following the pointer, and exhaust jets

use demo_kit::prelude::*;

use crate::shared::{count_label, emit, particle_image, pointer, screen_view};

#[derive(Default)]
pub struct TrailsPage {
  /// The comet's emitter, which follows the pointer
  comet: Option<Entity>,
}

impl DemoPage for TrailsPage {
  fn name(&self) -> &str {
    "Trail Effects"
  }

  fn description(&self) -> &str {
    "Comet trail (mouse) and exhaust jets"
  }

  fn controls(&self) -> Vec<Control> {
    vec![Control::mouse("Move to steer the comet")]
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    let screen = screen_view(ctx);
    let image = particle_image(ctx);
    let comet = ctx.assets().add(ParticleEffect::comet_trail(image.clone()));
    // Exhaust jets angled up and in from the corners, and one straight up
    let spread = 0.3f32.to_degrees();
    let up_right = ctx.assets().add(
      ParticleEffect::exhaust(image.clone())
        .with_direction(-45.0, spread)
        .with_speed((50.0, 100.0)),
    );
    let up_left = ctx.assets().add(
      ParticleEffect::exhaust(image.clone())
        .with_direction(-135.0, spread)
        .with_speed((50.0, 100.0)),
    );
    let up = ctx.assets().add(
      ParticleEffect::exhaust(image)
        .with_direction(-90.0, 0.2f32.to_degrees())
        .with_speed((60.0, 120.0)),
    );

    self.comet = Some(emit(ctx, screen.x / 2.0, screen.y / 2.0, &comet));
    emit(ctx, 300.0, screen.y - 100.0, &up_right);
    emit(ctx, screen.x - 300.0, screen.y - 100.0, &up_left);
    emit(ctx, screen.x / 2.0, screen.y - 60.0, &up);
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) {
    let target = pointer(ctx);
    // The comet eases towards the pointer; its trail spreads along the way
    if let Some(comet) = self.comet
      && let Some(mut transform) = ctx.world.get_mut::<Transform>(comet)
    {
      let current = transform.translation;
      transform.translation += (target - current) * 0.1;
    }
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) {
    count_label(ui, ctx);
  }
}
