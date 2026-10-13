//! E2E tests for post-process effects: every pass must see its own settings.

use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

fn engine(scene: impl Scene + 'static) -> Option<HeadlessEngine> {
  HeadlessEngine::new_or_skip(EngineBuilder::new().resolution(64, 64).start_scene(scene))
}

fn spawn_sprite(ctx: &mut SceneContext, sprite: Sprite, name: &str) {
  let handle = SpriteHandle::from_sprite(sprite, name);
  ctx.world.spawn((
    Transform::from_xy(0.0, 0.0),
    Renderable::sprite(handle).with_anchor(Anchor::TopLeft),
  ));
}

/// Mid-grey screen with a strong vignette followed by a near-zero colour tint.
struct StackedEffects;

impl Scene for StackedEffects {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    spawn_sprite(ctx, Sprite::solid_rect(64, 64, 128, 128, 128, 255), "grey");
    ctx.effects().set_vignette_custom(1.0, 0.2, 0.3);
    // Comes after the vignette in the chain; its params.x (0.01) would all but
    // disable the vignette if the vignette read it.
    ctx
      .effects()
      .set_color_tint(Color::rgb(255, 255, 255), 0.01);
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
    ctx.effects().set_vignette(1.0);
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
      ctx.effects().set_bloom_custom(0.5, 1.0, 5.0, 2);
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

/// A black frame with a 2-px white vertical line at x = 32 drawn `layers`
/// times additively, under default bloom.
struct GlowingLine {
  layers: u32,
  emission: f32,
}

impl Scene for GlowingLine {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let black = SpriteHandle::from_sprite(Sprite::solid_rect(64, 64, 0, 0, 0, 255), "glow-black");
    ctx
      .world
      .spawn((Transform::from_xy(32.0, 32.0), Renderable::sprite(black)));
    let line =
      SpriteHandle::from_sprite(Sprite::solid_rect(2, 64, 255, 255, 255, 255), "glow-line");
    for z in 1..=self.layers as i32 {
      ctx.world.spawn((
        Transform::from_xy(33.0, 32.0),
        Renderable::sprite(line.clone())
          .with_z_order(z)
          .with_blend(BlendMode::Additive)
          .with_emission(self.emission),
      ));
    }
    ctx.effects().set_bloom(1.0);
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }
}

/// The red byte 3 px right of the line.
fn glow_beside(scene: impl Scene + 'static) -> Option<u8> {
  let mut engine = engine(scene)?;
  engine.step_frames(2).unwrap();
  Some(engine.frame().unwrap().get_pixel(37, 32).0[0])
}

#[test]
fn plain_white_does_not_bloom_by_default() {
  let Some(glow) = glow_beside(GlowingLine {
    layers: 1,
    emission: 1.0,
  }) else {
    return;
  };
  assert!(
    glow <= 2,
    "white is not above the default threshold: {glow}"
  );
}

#[test]
fn light_above_white_blooms_by_default() {
  let Some(glow) = glow_beside(GlowingLine {
    layers: 3,
    emission: 1.0,
  }) else {
    return;
  };
  assert!(glow > 20, "three layers of white glow: {glow}");
}

#[test]
fn an_emissive_sprite_blooms_where_plain_white_does_not() {
  let (Some(plain), Some(glowing)) = (
    glow_beside(GlowingLine {
      layers: 1,
      emission: 1.0,
    }),
    glow_beside(GlowingLine {
      layers: 1,
      emission: 3.0,
    }),
  ) else {
    return;
  };
  assert!(
    plain <= 2 && glowing > 20,
    "plain {plain}, emissive {glowing}"
  );
}

/// An emissive line seen by a texture camera, shown by a sprite: the target
/// keeps light above white.
struct GlowThroughTarget;

impl Scene for GlowThroughTarget {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let black =
      SpriteHandle::from_sprite(Sprite::solid_rect(64, 64, 0, 0, 0, 255), "rt-glow-black");
    let line = SpriteHandle::from_sprite(
      Sprite::solid_rect(2, 64, 255, 255, 255, 255),
      "rt-glow-line",
    );
    ctx.world.spawn((
      Transform::from_xy(1000.0, 1000.0),
      Renderable::sprite(black),
    ));
    ctx.world.spawn((
      Transform::from_xy(1001.0, 1000.0),
      Renderable::sprite(line).with_z_order(1).with_emission(3.0),
    ));
    let target = ctx.render_targets().create(64, 64);
    ctx.world.spawn((
      Transform::from_xy(1000.0, 1000.0),
      Camera2d {
        target: CameraTarget::Texture(target),
        ..Default::default()
      },
    ));
    ctx.world.spawn((
      Transform::from_xy(32.0, 32.0),
      Renderable::render_target(target),
    ));
    ctx.effects().set_bloom(1.0);
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }
}

#[test]
fn an_emissive_sprite_seen_through_a_render_target_still_blooms() {
  let Some(glow) = glow_beside(GlowThroughTarget) else {
    return;
  };
  assert!(glow > 20, "{glow}");
}

/// `BrightLine` with bloom (threshold 0.5) and the display stage switched off.
struct BloomWithoutPostProcess;

impl Scene for BloomWithoutPostProcess {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    BrightLine { bloom: false }.on_enter(ctx);
    ctx.effects().set_bloom_custom(0.5, 1.0, 5.0, 2);
    let _ = ctx
      .render_pipeline()
      .set_enabled(pass_ids::POST_PROCESS, false);
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }
}

#[test]
fn bloom_survives_switching_post_process_off() {
  let (Some(plain), Some(mut engine)) =
    (pixel_right_of_line(false), engine(BloomWithoutPostProcess))
  else {
    return;
  };
  engine.step_frames(2).unwrap();
  let bloomed = engine.frame().unwrap().get_pixel(38, 32).0[0];
  assert!(bloomed > plain + 5, "plain {plain}, bloomed {bloomed}");
}

const QUARTER: &str = "
fn effect(in: EffectIn, params: Params) -> vec4<f32> {
  return vec4<f32>(scene(in.uv).rgb * 0.25, 1.0);
}
";

/// Two additive white squares (2.0 in linear light) under the clamp curve,
/// a quarter of them kept by a custom effect in `stage`.
struct Quartered(EffectStage);

impl Scene for Quartered {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let white = SpriteHandle::from_sprite(
      Sprite::solid_rect(64, 64, 255, 255, 255, 255),
      "quarter-white",
    );
    for z in 0..2 {
      ctx.world.spawn((
        Transform::from_xy(32.0, 32.0),
        Renderable::sprite(white.clone())
          .with_z_order(z)
          .with_blend(BlendMode::Additive),
      ));
    }
    ctx.look().tonemap = Tonemap::Clamp;
    let shader = ShaderHandle::from_source("quarter.wgsl", QUARTER);
    let _ = ctx.effects().add_custom_in("quarter", shader, self.0);
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }
}

fn quartered(stage: EffectStage) -> Option<u8> {
  let mut engine = engine(Quartered(stage))?;
  // Custom effects compile asynchronously: give them a few frames
  engine.step_frames(4).unwrap();
  Some(engine.frame().unwrap().get_pixel(32, 32).0[0])
}

#[test]
fn an_hdr_custom_effect_sees_light_above_white() {
  let (Some(hdr), Some(display)) = (quartered(EffectStage::Hdr), quartered(EffectStage::Display))
  else {
    return;
  };
  // About 2.03 x 0.25 = 0.51 in HDR (sRGB 189); 1.0 x 0.25 after the clamp (sRGB 137)
  assert!(hdr.abs_diff(189) <= 3, "hdr {hdr}");
  assert!(display.abs_diff(137) <= 3, "display {display}");
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
    ctx.effects().set_bloom_custom(0.1, 5.0, 5.0, 1);
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
  // Radius 5 spreads to about 3 sigma (25 px), not to lattice copies beyond
  assert_eq!(row[45], 0, "nothing 47 px away: {row:?}");
}

/// Black 128x64 screen with an 8x8 white square centred on (24, 32), bloom
/// radius 5 and one pass
struct BrightSquare;

impl Scene for BrightSquare {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let mut pixels = Vec::with_capacity(128 * 64 * 4);
    for y in 0..64 {
      for x in 0..128 {
        let v = if (20..28).contains(&x) && (28..36).contains(&y) {
          255
        } else {
          0
        };
        pixels.extend_from_slice(&[v, v, v, 255]);
      }
    }
    spawn_sprite(ctx, Sprite::from_rgba(pixels, 128, 64), "square");
    ctx.effects().set_bloom_custom(0.1, 5.0, 5.0, 1);
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }
}

#[test]
fn bloom_spreads_as_wide_as_the_original_kernel() {
  let Some(mut engine) = HeadlessEngine::new_or_skip(
    EngineBuilder::new()
      .resolution(128, 64)
      .start_scene(BrightSquare),
  ) else {
    return;
  };
  engine.step_frames(2).unwrap();
  let frame = engine.frame().unwrap();
  // Light, not sRGB bytes
  let at = |dx: u32| {
    let c = frame.get_pixel(24 + dx, 32).0[0] as f32 / 255.0;
    if c <= 0.04045 {
      c / 12.92
    } else {
      ((c + 0.055) / 1.055).powf(2.4)
    }
  };
  let (near, far) = (at(6), at(16));

  // Each pass blurs with sigma = 1.7 x radius (8.5 px here), as the original
  // 9-tap kernel did: 16 px from the centre keeps about a fifth of the light
  // at 6 px (with sigma 2.5 px it kept none)
  assert!(near > 0.1, "glow next to the square: {near}");
  assert!(
    far > near * 0.12 && far < near * 0.4,
    "glow 16 px out is {far}, 6 px out {near}"
  );
}

struct GreyFade;

impl Scene for GreyFade {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.effects().set_fade_color(1.0, Color::rgb(128, 128, 128));
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }
}

#[test]
fn a_fade_reaches_the_colour_it_names() {
  let Some(mut engine) = engine(GreyFade) else {
    return;
  };
  engine.step_frames(2).unwrap();
  let pixel = engine.frame().unwrap().get_pixel(32, 32).0;
  // Read as linear (the old rule) it showed about 188
  assert!(pixel[0].abs_diff(128) <= 1, "{pixel:?}");
}
