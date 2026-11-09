//! Smoke: light columns, a thick plume, ground fog and a coloured steam vent

use demo_kit::prelude::*;

use crate::shared::{count_label, emit, fading, particle_image, screen_view};

#[derive(Default)]
pub struct SmokePage;

impl DemoPage for SmokePage {
  fn name(&self) -> &str {
    "Smoke Effects"
  }

  fn description(&self) -> &str {
    "Columns, thick plumes, ground fog, and colored steam"
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    let screen = screen_view(ctx);
    let image = particle_image(ctx);
    let smoke = ctx.assets().add(ParticleEffect::smoke(image.clone()));
    let thick = ctx.assets().add(ParticleEffect::smoke_thick(image.clone()));
    let fog = ctx.assets().add(fading(
      ParticleEffect::new(image.clone())
        .with_rate(8.0)
        .with_shape(EmitShape::Line { length: 800.0 })
        .with_lifetime((3.0, 5.0))
        .with_size((20.0, 40.0))
        .with_gravity(15.0, -5.0)
        .with_speed((2.0, 8.0)),
      Color::new(180, 180, 180, 40),
      Color::new(120, 120, 120, 0),
    ));
    let steam = ctx.assets().add(fading(
      ParticleEffect::new(image)
        .with_rate(15.0)
        // Up, in a cone 23 degrees wide
        .with_direction(-90.0, 0.4f32.to_degrees())
        .with_speed((30.0, 60.0))
        .with_lifetime((1.5, 3.0))
        .with_size((6.0, 12.0))
        .with_gravity(0.0, -10.0),
      Color::new(100, 200, 180, 120),
      Color::new(80, 160, 140, 0),
    ));
    let cx = screen.x / 2.0;

    // Light smoke columns across the scene
    for x in [200.0, 540.0, 880.0] {
      emit(ctx, x, screen.y - 50.0, &smoke);
    }
    // Thick smoke, as from a fire
    emit(ctx, cx, screen.y - 150.0, &thick);
    // Fog drifting along the ground
    emit(ctx, cx, screen.y - 30.0, &fog);
    // A coloured steam vent
    emit(ctx, screen.x - 200.0, screen.y - 50.0, &steam);
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) {
    count_label(ui, ctx);
  }
}
