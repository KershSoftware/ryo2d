//! E2E tests for post-process effects: every pass must see its own settings.

use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

fn engine(scene: impl Scene + 'static) -> Option<HeadlessEngine> {
  HeadlessEngine::new_or_skip(EngineBuilder::new().resolution(64, 64).start_scene(scene))
}

fn spawn_sprite(ctx: &mut SceneContext, sprite: Sprite, name: &str) {
  let handle = SpriteHandle::from_sprite(sprite, name);
  ctx
    .world
    .spawn((Transform::new(0.0, 0.0), Renderable::sprite(handle)));
}

/// Mid-grey screen with a strong vignette followed by a near-zero colour tint.
struct StackedEffects;

impl Scene for StackedEffects {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    spawn_sprite(ctx, Sprite::solid_rect(64, 64, 128, 128, 128, 255), "grey");
    ctx.effects.set_vignette_custom(1.0, 0.2, 0.3);
    // Comes after the vignette in the chain; its params.x (0.01) would all but
    // disable the vignette if the vignette read it.
    ctx.effects.set_color_tint(Color::rgb(255, 255, 255), 0.01);
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }
}

#[test]
fn stacked_effects_each_keep_their_settings() {
  let Some(mut engine) = engine(StackedEffects) else {
    return;
  };

  engine.step_frames(2).unwrap();
  let frame = engine.frame().unwrap();
  let corner = frame.get_pixel(1, 1).0[0];
  let centre = frame.get_pixel(32, 32).0[0];
  assert!(
    corner + 30 < centre,
    "vignette must darken the corner (corner {corner}, centre {centre})"
  );
}

/// Mid-grey screen with the default vignette (`set_vignette`).
struct DefaultVignette;

impl Scene for DefaultVignette {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    spawn_sprite(ctx, Sprite::solid_rect(64, 64, 128, 128, 128, 255), "grey");
    ctx.effects.set_vignette(1.0);
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }
}

#[test]
fn default_vignette_darkens_the_corners_and_spares_the_centre() {
  let Some(mut engine) = engine(DefaultVignette) else {
    return;
  };

  engine.step_frames(2).unwrap();
  let frame = engine.frame().unwrap();
  let corner = frame.get_pixel(0, 0).0[0];
  let centre = frame.get_pixel(32, 32).0[0];
  assert_eq!(centre, 128, "the centre is untouched");
  assert!(
    corner < 64,
    "a full-intensity default vignette must darken the corner (corner {corner})"
  );
}

/// Black screen with a 2-px-wide white vertical line at x = 32.
struct BrightLine {
  bloom: bool,
}

impl Scene for BrightLine {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let mut pixels = Vec::with_capacity(64 * 64 * 4);
    for _ in 0..64 {
      for x in 0..64 {
        let v = if (32..34).contains(&x) { 255 } else { 0 };
        pixels.extend_from_slice(&[v, v, v, 255]);
      }
    }
    spawn_sprite(ctx, Sprite::from_rgba(pixels, 64, 64), "line");
    if self.bloom {
      ctx.effects.set_bloom(1.0);
    }
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }
}

fn pixel_right_of_line(bloom: bool) -> Option<u8> {
  let mut engine = engine(BrightLine { bloom })?;
  engine.step_frames(2).unwrap();
  let frame = engine.frame().unwrap();
  Some(frame.get_pixel(38, 32).0[0])
}

#[test]
fn bloom_blurs_horizontally() {
  let (Some(plain), Some(bloomed)) = (pixel_right_of_line(false), pixel_right_of_line(true)) else {
    return;
  };

  assert!(
    bloomed > plain + 5,
    "bloom must spread light sideways (plain {plain}, bloomed {bloomed})"
  );
}

/// Black 128x64 screen with a 2x2 white dot at (20, 31).
struct BrightDot;

impl Scene for BrightDot {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let mut pixels = Vec::with_capacity(128 * 64 * 4);
    for y in 0..64 {
      for x in 0..128 {
        let v = if (20..22).contains(&x) && (31..33).contains(&y) {
          255
        } else {
          0
        };
        pixels.extend_from_slice(&[v, v, v, 255]);
      }
    }
    spawn_sprite(ctx, Sprite::from_rgba(pixels, 128, 64), "dot");
    ctx.effects.set_bloom_custom(0.1, 5.0, 20.0, 1);
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }
}

#[test]
fn bloom_fades_smoothly_within_its_radius() {
  let Some(mut engine) = HeadlessEngine::new_or_skip(
    EngineBuilder::new()
      .resolution(128, 64)
      .start_scene(BrightDot),
  ) else {
    return;
  };

  engine.step_frames(2).unwrap();
  let frame = engine.frame().unwrap();
  let row: Vec<u8> = (22..80).map(|x| frame.get_pixel(x, 31).0[0]).collect();

  // Light falls off with distance from the dot, without copies of it further out
  for (i, pair) in row.windows(2).enumerate() {
    assert!(
      pair[1] <= pair[0],
      "glow brightens again {} px from the dot: {row:?}",
      i + 1
    );
  }
  assert!(row[2] > 0, "the glow reaches past the dot: {row:?}");
  // A 20 px radius spreads 20 px, not four times that
  assert_eq!(row[45], 0, "nothing 45 px away: {row:?}");
}
