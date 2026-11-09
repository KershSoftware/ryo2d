//! What the pages share: the view, the particle image, placing an emitter,
//! the live count, and the looks of the demo's own effects

use demo_kit::prelude::*;

/// The screen's size, with the camera centred on it so that world positions
/// are screen positions (0, 0 at the top left)
pub fn screen_view(ctx: &mut SceneContext) -> Vec2 {
  let (width, height) = ctx.screen_size();
  ctx.camera().set_position(width / 2.0, height / 2.0);
  Vec2::new(width, height)
}

/// A white disc: the image every particle draws, tinted by its effect
pub fn particle_image(ctx: &mut SceneContext) -> Handle<Image> {
  ctx
    .assets()
    .add(Image::solid_circle(16, 255, 255, 255, 255))
}

/// An emitter of `effect` at `(x, y)`
pub fn emit(ctx: &mut SceneContext, x: f32, y: f32, effect: &Handle<ParticleEffect>) -> Entity {
  ctx
    .world
    .spawn((
      Transform::from_xy(x, y),
      ParticleEmitter::new(effect.clone()),
    ))
    .id()
}

/// `effect` with each particle fading from `from` to `to` and shrinking to
/// nothing over its life
pub fn fading(effect: ParticleEffect, from: Color, to: Color) -> ParticleEffect {
  effect
    .with_color_over_life(Gradient::new([(0.0, from), (1.0, to)]))
    .with_size_over_life(Curve::new([(0.0, 1.0), (1.0, 0.0)]))
}

/// `effect` as a burst of `count` particles every `interval` seconds, the
/// first at once, for as long as it plays
pub fn every(mut effect: ParticleEffect, count: u32, interval: f32) -> ParticleEffect {
  effect.bursts = vec![Burst::at(0.0, count as f32)];
  effect
    .with_rate(0.0)
    .with_duration(interval)
    .with_looping(true)
}

/// The pointer in world space
pub fn pointer(ctx: &mut SceneContext) -> Vec2 {
  let (x, y) = ctx.input.mouse_position();
  ctx.camera().screen_to_world(Vec2::new(x, y))
}

/// The live particles against the engine's budget, at the top right of the
/// content area
pub fn count_label(ui: &mut UiContext, ctx: &mut SceneContext) {
  let particles = ctx.particles();
  let count = format!("Particles: {} / {}", particles.count(), particles.budget());
  let area = content_area(ui);
  let (text_w, _) = ui.text_size(&count);
  ui.draw_text(
    &count,
    area.x + area.width - text_w as f32,
    area.y,
    Color::GREEN,
  );
}
