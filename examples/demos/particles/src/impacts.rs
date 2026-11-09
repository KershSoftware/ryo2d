//! Impacts: sparks and dust that repeat, and more wherever the player clicks

use demo_kit::prelude::*;

use crate::shared::{count_label, emit, every, particle_image, pointer, screen_view};

#[derive(Default)]
pub struct ImpactsPage {
  /// What a left click plays
  sparks: Option<Handle<ParticleEffect>>,
  /// What a right click plays
  dust: Option<Handle<ParticleEffect>>,
}

impl DemoPage for ImpactsPage {
  fn name(&self) -> &str {
    "Impacts"
  }

  fn description(&self) -> &str {
    "Auto-repeating demos + Click to spawn more"
  }

  fn controls(&self) -> Vec<Control> {
    vec![Control::mouse("Left-click sparks, right-click dust")]
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    let screen = screen_view(ctx);
    let image = particle_image(ctx);
    let quick_sparks = ctx
      .assets()
      .add(every(ParticleEffect::sparks(image.clone()), 15, 1.5));
    let slow_sparks = ctx
      .assets()
      .add(every(ParticleEffect::sparks(image.clone()), 15, 2.0));
    let puffs = ctx
      .assets()
      .add(every(ParticleEffect::dust(image.clone()), 12, 2.5));
    self.sparks = Some(ctx.assets().add(ParticleEffect::sparks(image.clone())));
    self.dust = Some(ctx.assets().add(ParticleEffect::dust(image)));

    // Impacts that repeat, so the page is never empty
    emit(ctx, 300.0, screen.y / 2.0, &quick_sparks);
    emit(ctx, screen.x - 300.0, screen.y / 2.0, &slow_sparks);
    emit(ctx, screen.x / 2.0, screen.y / 2.0 + 50.0, &puffs);
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) {
    let at = pointer(ctx);
    // Each plays once where clicked, and its emitter despawns when done
    if ctx.input.mouse_pressed(MouseButton::Left)
      && let Some(sparks) = &self.sparks
    {
      ctx.particles().play_at(sparks, at);
    }
    if ctx.input.mouse_pressed(MouseButton::Right)
      && let Some(dust) = &self.dust
    {
      ctx.particles().play_at(dust, at);
    }
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) {
    count_label(ui, ctx);
  }
}
