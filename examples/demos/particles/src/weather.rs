//! Weather: rain blown by the wind on the left, snow on the right

use demo_kit::prelude::*;

use crate::shared::{count_label, emit, particle_image, screen_view};

#[derive(Default)]
pub struct WeatherPage;

impl DemoPage for WeatherPage {
  fn name(&self) -> &str {
    "Weather"
  }

  fn description(&self) -> &str {
    "Rain with wind (left) and snow (right)"
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    let screen = screen_view(ctx);
    let image = particle_image(ctx);
    // Living long enough to fall the full screen height
    let rain = ctx.assets().add(
      ParticleEffect::rain(image.clone())
        .with_rate(60.0)
        .with_lifetime((1.3, 1.5)),
    );
    // Snow drifts at about 40 px/s, so it needs some 20 s to cross the screen
    let snow = ctx.assets().add(
      ParticleEffect::snow(image)
        .with_rate(20.0)
        .with_lifetime((19.0, 21.0)),
    );

    // Each preset's line is 400 px wide; two of each cover half the screen,
    // just above its top
    for x in [200.0, 420.0] {
      emit(ctx, x, -50.0, &rain);
    }
    for x in [820.0, 1050.0] {
      emit(ctx, x, -50.0, &snow);
    }
    // Wind pushing the rain sideways
    ctx.world.spawn((
      Transform::from_xy(0.0, 0.0),
      ParticleAttractor::area(screen.x / 2.0, screen.y, 30.0, 0.0),
    ));
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) {
    count_label(ui, ctx);
  }
}
