//! Explosions: three that repeat, and one more wherever the player clicks

use demo_kit::prelude::*;

use crate::shared::{count_label, emit, every, particle_image, pointer, screen_view};

#[derive(Default)]
pub struct ExplosionsPage {
  /// What a left click plays
  explosion: Option<Handle<ParticleEffect>>,
  /// What a right click plays
  debris: Option<Handle<ParticleEffect>>,
}

impl DemoPage for ExplosionsPage {
  fn name(&self) -> &str {
    "Explosions"
  }

  fn description(&self) -> &str {
    "Auto-repeating demos + Click to spawn more"
  }

  fn controls(&self) -> Vec<Control> {
    vec![Control::mouse("Left-click explosion, right-click debris")]
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    let screen = screen_view(ctx);
    let image = particle_image(ctx);
    let small = ctx
      .assets()
      .add(every(ParticleEffect::explosion(image.clone(), 30), 30, 2.5));
    let falling = ctx.assets().add(every(
      ParticleEffect::explosion_debris(image.clone(), 20),
      20,
      3.0,
    ));
    let large = ctx
      .assets()
      .add(every(ParticleEffect::explosion(image.clone(), 50), 50, 4.0));
    self.explosion = Some(
      ctx
        .assets()
        .add(ParticleEffect::explosion(image.clone(), 40)),
    );
    self.debris = Some(
      ctx
        .assets()
        .add(ParticleEffect::explosion_debris(image, 25)),
    );

    // Explosions that repeat, so the page is never empty
    emit(ctx, 300.0, screen.y / 2.0, &small);
    emit(ctx, screen.x - 300.0, screen.y / 2.0, &falling);
    emit(ctx, screen.x / 2.0, screen.y / 2.0 - 80.0, &large);
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) {
    let at = pointer(ctx);
    // Each plays once where clicked, and its emitter despawns when done
    if ctx.input.mouse_pressed(MouseButton::Left)
      && let Some(explosion) = &self.explosion
    {
      ctx.particles().play_at(explosion, at);
    }
    if ctx.input.mouse_pressed(MouseButton::Right)
      && let Some(debris) = &self.debris
    {
      ctx.particles().play_at(debris, at);
    }
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) {
    count_label(ui, ctx);
  }
}
