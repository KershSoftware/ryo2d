//! Celebration: confetti bursts, rising bubbles and a firework

use demo_kit::prelude::*;

use crate::shared::{count_label, emit, every, fading, particle_image, screen_view};

#[derive(Default)]
pub struct CelebrationPage;

impl DemoPage for CelebrationPage {
  fn name(&self) -> &str {
    "Celebration"
  }

  fn description(&self) -> &str {
    "Confetti bursts, bubbles, and fireworks"
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    let screen = screen_view(ctx);
    let image = particle_image(ctx);
    let confetti = ctx
      .assets()
      .add(every(ParticleEffect::confetti(image.clone()), 25, 2.0));
    let bubbles = ctx.assets().add(ParticleEffect::bubbles(image.clone()));
    let firework = ctx.assets().add(every(
      fading(
        ParticleEffect::new(image)
          .with_lifetime((1.5, 3.0))
          .with_size((3.0, 6.0))
          .with_speed((40.0, 120.0))
          .with_gravity(0.0, 40.0),
        Color::new(255, 200, 50, 255),
        Color::new(255, 80, 0, 0),
      ),
      40,
      3.0,
    ));
    let cx = screen.x / 2.0;

    // Confetti from two places
    for x in [cx - 200.0, cx + 200.0] {
      emit(ctx, x, screen.y - 50.0, &confetti);
    }
    // Bubbles rising along the bottom
    for x in [200.0, 450.0, 700.0, 950.0, 1100.0] {
      emit(ctx, x, screen.y - 50.0, &bubbles);
    }
    // A firework bursting at the top
    emit(ctx, cx, 120.0, &firework);
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) {
    count_label(ui, ctx);
  }
}
