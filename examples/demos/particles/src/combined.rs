//! Combined: fire and smoke, magic in a vortex, sparkles falling into a
//! gravity well, bubbles, confetti and candles at once

use demo_kit::prelude::*;

use crate::shared::{count_label, emit, every, fading, particle_image, screen_view};

#[derive(Default)]
pub struct CombinedPage;

impl DemoPage for CombinedPage {
  fn name(&self) -> &str {
    "Combined Demo"
  }

  fn description(&self) -> &str {
    "Fire, magic, confetti, gravity well, and more"
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    let screen = screen_view(ctx);
    let image = particle_image(ctx);
    let fire = ctx
      .assets()
      .add(ParticleEffect::fire(image.clone()).with_rate(40.0));
    let smoke = ctx.assets().add(ParticleEffect::smoke(image.clone()));
    let magic = ctx
      .assets()
      .add(ParticleEffect::magic(image.clone()).with_rate(25.0));
    let sparkle = ctx.assets().add(fading(
      ParticleEffect::new(image.clone())
        .with_rate(25.0)
        .with_shape(EmitShape::Line { length: 300.0 })
        .with_lifetime((3.0, 5.0))
        .with_size((3.0, 7.0))
        .with_gravity(0.0, 30.0)
        .with_speed((5.0, 15.0)),
      Color::YELLOW,
      Color::new(255, 100, 0, 0),
    ));
    let bubbles = ctx.assets().add(ParticleEffect::bubbles(image.clone()));
    let confetti = ctx
      .assets()
      .add(every(ParticleEffect::confetti(image.clone()), 20, 3.5));
    let candle = ctx
      .assets()
      .add(ParticleEffect::flame_small(image).with_rate(10.0));
    let (cx, cy) = (screen.x / 2.0, screen.y / 2.0);

    // Fire and smoke on the left
    emit(ctx, 200.0, screen.y - 80.0, &fire);
    emit(ctx, 200.0, screen.y - 160.0, &smoke);
    // Magic swirled by a vortex on the right
    emit(ctx, screen.x - 250.0, cy, &magic);
    ctx.world.spawn((
      Transform::from_xy(screen.x - 250.0, cy),
      ParticleAttractor::vortex(100.0, 180.0),
    ));
    // Sparkles falling towards a gravity well in the centre
    ctx.world.spawn((
      Transform::from_xy(cx, cy - 30.0),
      ParticleAttractor::gravity_well(80.0, 160.0),
    ));
    emit(ctx, cx, 100.0, &sparkle);
    // Bubbles rising at the bottom right
    emit(ctx, screen.x - 150.0, screen.y - 50.0, &bubbles);
    // Confetti from the bottom centre
    emit(ctx, cx, screen.y - 40.0, &confetti);
    // Candles along the middle
    for i in 0..3 {
      emit(ctx, 400.0 + i as f32 * 200.0, screen.y - 200.0, &candle);
    }
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) {
    count_label(ui, ctx);
  }
}
