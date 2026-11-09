//! Magic: sparkles, healing, a ring and a fountain

use demo_kit::prelude::*;

use crate::shared::{count_label, emit, fading, particle_image, screen_view};

#[derive(Default)]
pub struct MagicPage;

impl DemoPage for MagicPage {
  fn name(&self) -> &str {
    "Magic Effects"
  }

  fn description(&self) -> &str {
    "Sparkles, healing, magic ring, and particle fountain"
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    let screen = screen_view(ctx);
    let image = particle_image(ctx);
    let magic = ctx.assets().add(ParticleEffect::magic(image.clone()));
    let heal = ctx.assets().add(ParticleEffect::magic_heal(image.clone()));
    let ring = ctx.assets().add(fading(
      ParticleEffect::new(image.clone())
        .with_rate(40.0)
        // The rim of a circle, each particle drifting out from its centre
        .with_shape(EmitShape::Circle {
          radius: 80.0,
          arc_deg: 360.0,
          thickness: 0.0,
        })
        .outward()
        .with_lifetime((1.0, 2.0))
        .with_size((3.0, 6.0))
        .with_speed((2.0, 8.0)),
      Color::new(180, 100, 255, 200),
      Color::new(100, 50, 255, 0),
    ));
    let fountain = ctx.assets().add(fading(
      ParticleEffect::new(image)
        .with_rate(25.0)
        // Up, in a cone 17 degrees wide
        .with_direction(-90.0, 0.3f32.to_degrees())
        .with_speed((80.0, 150.0))
        .with_lifetime((1.5, 2.5))
        .with_size((4.0, 8.0))
        .with_gravity(0.0, 60.0),
      Color::new(100, 200, 255, 200),
      Color::new(200, 100, 255, 0),
    ));
    let (cx, cy) = (screen.x / 2.0, screen.y / 2.0);

    emit(ctx, cx - 250.0, cy, &magic);
    emit(ctx, cx + 250.0, cy, &heal);
    emit(ctx, cx, cy - 50.0, &ring);
    emit(ctx, cx, screen.y - 80.0, &fountain);
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) {
    count_label(ui, ctx);
  }
}
