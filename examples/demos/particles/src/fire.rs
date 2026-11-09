//! Fire: a bonfire, torches, a row of candles and embers rising

use demo_kit::prelude::*;

use crate::shared::{count_label, emit, fading, particle_image, screen_view};

#[derive(Default)]
pub struct FirePage;

impl DemoPage for FirePage {
  fn name(&self) -> &str {
    "Fire Effects"
  }

  fn description(&self) -> &str {
    "Bonfire, torches, candles, and rising embers"
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    let screen = screen_view(ctx);
    let image = particle_image(ctx);
    let fire = ctx.assets().add(ParticleEffect::fire(image.clone()));
    let torch = ctx.assets().add(ParticleEffect::flame_small(image.clone()));
    let candle = ctx
      .assets()
      .add(ParticleEffect::flame_small(image.clone()).with_rate(8.0));
    let embers = ctx.assets().add(fading(
      ParticleEffect::new(image)
        .with_rate(12.0)
        .with_shape(EmitShape::Line { length: 120.0 })
        .with_lifetime((2.0, 4.0))
        .with_size((2.0, 4.0))
        .with_gravity(0.0, -40.0)
        .with_speed((10.0, 30.0)),
      Color::new(255, 120, 20, 255),
      Color::new(255, 60, 0, 0),
    ));
    let cx = screen.x / 2.0;

    // A large bonfire in the centre
    emit(ctx, cx, screen.y - 100.0, &fire);
    // Torches on either side
    for x in [200.0, screen.x - 200.0] {
      emit(ctx, x, screen.y - 80.0, &torch);
    }
    // A row of candles across the scene
    for i in 0..7 {
      emit(ctx, 200.0 + i as f32 * 130.0, screen.y - 220.0, &candle);
    }
    // Embers rising from the bonfire
    emit(ctx, cx, screen.y - 120.0, &embers);
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) {
    count_label(ui, ctx);
  }
}
