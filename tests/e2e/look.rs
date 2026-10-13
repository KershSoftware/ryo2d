//! The look: exposure, tonemap curves, grading, LUTs and auto-exposure,
//! read back as display bytes.
//!
//! Tests skip when no GPU is available.

use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

type Setup = Box<dyn FnMut(&mut SceneContext) + Send>;

/// A square of `color` filling the 32x32 window, under `look`, then `setup`.
struct Square {
  color: [u8; 4],
  look: Look,
  setup: Option<Setup>,
}

impl Scene for Square {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let [r, g, b, a] = self.color;
    let sprite = SpriteHandle::from_sprite(
      Sprite::solid_rect(32, 32, r, g, b, a),
      format!("look-square-{r}-{g}-{b}-{a}"),
    );
    ctx
      .world
      .spawn((Transform::from_xy(16.0, 16.0), Renderable::sprite(sprite)));
    *ctx.look() = self.look.clone();
    if let Some(setup) = self.setup.as_mut() {
      setup(ctx);
    }
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }
}

fn square(color: [u8; 4], look: Look) -> Square {
  Square {
    color,
    look,
    setup: None,
  }
}

fn start(scene: impl Scene + 'static) -> Option<HeadlessEngine> {
  HeadlessEngine::new_or_skip(EngineBuilder::new().resolution(32, 32).start_scene(scene))
}

/// The centre pixel after two frames.
fn shown(scene: impl Scene + 'static) -> Option<[u8; 4]> {
  let mut engine = start(scene)?;
  engine.step_frames(2).unwrap();
  Some(engine.frame().unwrap().get_pixel(16, 16).0)
}

/// Linear light as the sRGB byte the display image stores.
fn encode(linear: f32) -> u8 {
  let c = linear.clamp(0.0, 1.0);
  let srgb = if c <= 0.003_130_8 {
    c * 12.92
  } else {
    1.055 * c.powf(1.0 / 2.4) - 0.055
  };
  (srgb * 255.0).round() as u8
}

#[test]
fn the_default_look_keeps_colours_below_the_knee() {
  let Some(pixel) = shown(square([200, 100, 50, 255], Look::default())) else {
    return;
  };
  assert_eq!(pixel, [200, 100, 50, 255]);
}

#[test]
fn white_shows_at_about_243_under_the_default_look() {
  let Some(pixel) = shown(square([255, 255, 255, 255], Look::default())) else {
    return;
  };
  for c in &pixel[..3] {
    assert!((242..=244).contains(c), "{pixel:?}");
  }
}

#[test]
fn clamp_shows_white_as_255() {
  let look = Look {
    tonemap: Tonemap::Clamp,
    ..Look::default()
  };
  let Some(pixel) = shown(square([255, 255, 255, 255], look)) else {
    return;
  };
  assert_eq!(pixel, [255, 255, 255, 255]);
}

#[test]
fn a_fade_to_white_still_reaches_255() {
  // Display effects run after the tonemap, so the shoulder cannot hold them back
  let scene = Square {
    color: [255, 255, 255, 255],
    look: Look::default(),
    setup: Some(Box::new(|ctx: &mut SceneContext| {
      ctx.effects().set_fade_color(1.0, Color::WHITE);
    })),
  };
  let Some(pixel) = shown(scene) else {
    return;
  };
  assert_eq!(pixel, [255, 255, 255, 255]);
}

#[test]
fn one_stop_of_exposure_doubles_the_light() {
  let look = Look {
    exposure: 1.0,
    ..Look::default()
  };
  let Some(pixel) = shown(square([128, 128, 128, 255], look)) else {
    return;
  };
  // sRGB 128 is 0.2159 in linear light; doubled, 0.4317
  assert!(pixel[0].abs_diff(encode(0.4317)) <= 1, "{pixel:?}");
}

#[test]
fn every_curve_on_the_gpu_matches_tonemap_apply() {
  let curves = [
    Tonemap::default(),
    Tonemap::Clamp,
    Tonemap::Aces,
    Tonemap::AgX,
  ];
  for tonemap in curves {
    for exposure in [-1.0_f32, 0.0, 1.0, 2.0, 3.0] {
      let look = Look {
        exposure,
        tonemap,
        ..Look::default()
      };
      let Some(pixel) = shown(square([255, 255, 255, 255], look)) else {
        return;
      };
      let want = encode(tonemap.apply(Vec3::splat(exposure.exp2())).x);
      assert!(
        pixel[0].abs_diff(want) <= 1,
        "{tonemap:?} at {exposure} EV: {pixel:?}, want {want}"
      );
    }
  }
}

fn graded(grading: ColorGrading) -> Look {
  Look {
    grading,
    ..Look::default()
  }
}

#[test]
fn zero_saturation_turns_red_grey() {
  let look = graded(ColorGrading {
    saturation: 0.0,
    ..ColorGrading::default()
  });
  let Some(pixel) = shown(square([220, 30, 40, 255], look)) else {
    return;
  };
  assert!(
    pixel[0].abs_diff(pixel[1]) <= 1 && pixel[1].abs_diff(pixel[2]) <= 1,
    "{pixel:?}"
  );
}

#[test]
fn a_warm_temperature_warms_grey() {
  let look = graded(ColorGrading {
    temperature: 0.5,
    ..ColorGrading::default()
  });
  let Some(pixel) = shown(square([128, 128, 128, 255], look)) else {
    return;
  };
  assert!(pixel[0] > pixel[2] + 10, "{pixel:?}");
}

#[test]
fn lift_raises_black() {
  let look = graded(ColorGrading {
    lift: Vec3::splat(0.1),
    ..ColorGrading::default()
  });
  let Some(pixel) = shown(square([0, 0, 0, 255], look)) else {
    return;
  };
  assert!(pixel[0].abs_diff(encode(0.1)) <= 1, "{pixel:?}");
}

/// A LUT that inverts every sRGB channel.
fn inverting_lut() -> ColorLut {
  let mut strip = ColorLut::identity_strip(16);
  let inverted: Vec<u8> = strip
    .data()
    .chunks(4)
    .flat_map(|p| [255 - p[0], 255 - p[1], 255 - p[2], 255])
    .collect();
  strip = Sprite::from_rgba(inverted, strip.width(), strip.height());
  let Ok(lut) = ColorLut::from_sprite(&SpriteHandle::from_sprite(strip, "lut-invert")) else {
    panic!("a 256x16 strip is a LUT");
  };
  lut
}

#[test]
fn an_inverting_lut_inverts() {
  let look = graded(ColorGrading {
    lut: Some(inverting_lut()),
    ..ColorGrading::default()
  });
  let Some(pixel) = shown(square([200, 100, 50, 255], look)) else {
    return;
  };
  for (got, want) in pixel.iter().zip([55u8, 155, 205]) {
    assert!(got.abs_diff(want) <= 2, "{pixel:?}");
  }
}

#[test]
fn an_identity_lut_changes_nothing() {
  let strip = SpriteHandle::from_sprite(ColorLut::identity_strip(16), "lut-identity");
  let look = graded(ColorGrading {
    lut: ColorLut::from_sprite(&strip).ok(),
    ..ColorGrading::default()
  });
  let Some(pixel) = shown(square([200, 100, 50, 255], look)) else {
    return;
  };
  for (got, want) in pixel.iter().zip([200u8, 100, 50]) {
    assert!(got.abs_diff(want) <= 1, "{pixel:?}");
  }
}

#[test]
fn half_strength_blends_halfway() {
  let look = graded(ColorGrading {
    lut: Some(inverting_lut()),
    lut_strength: 0.5,
    ..ColorGrading::default()
  });
  let Some(pixel) = shown(square([200, 100, 50, 255], look)) else {
    return;
  };
  assert!(pixel[0].abs_diff(128) <= 2, "{pixel:?}");
}

#[test]
fn extreme_exposure_and_contrast_stay_finite() {
  let look = Look {
    exposure: 16.0,
    grading: ColorGrading {
      contrast: 10.0,
      ..ColorGrading::default()
    },
    ..Look::default()
  };
  let Some(pixel) = shown(square([255, 255, 255, 255], look)) else {
    return;
  };
  assert!(pixel[..3].iter().all(|&c| c >= 242), "{pixel:?}");
}

/// A square of `color` with `emission`, under `look`.
fn emitting(color: [u8; 4], emission: f32, look: Look) -> Square {
  Square {
    color: [0, 0, 0, 255],
    look,
    setup: Some(Box::new(move |ctx: &mut SceneContext| {
      let [r, g, b, a] = color;
      let sprite =
        SpriteHandle::from_sprite(Sprite::solid_rect(32, 32, r, g, b, a), "look-emitting");
      ctx.world.spawn((
        Transform::from_xy(16.0, 16.0),
        Renderable::sprite(sprite)
          .with_z_order(1)
          .with_emission(emission),
      ));
    })),
  }
}

#[test]
fn emission_multiplies_a_sprites_light() {
  let look = Look {
    tonemap: Tonemap::Clamp,
    ..Look::default()
  };
  let Some(pixel) = shown(emitting([128, 128, 128, 255], 2.0, look)) else {
    return;
  };
  assert!(pixel[0].abs_diff(encode(0.4317)) <= 1, "{pixel:?}");
}

/// A 4-px white square with `emission` at the left of a 128x32 frame, with bloom.
struct Blazing(f32);

impl Scene for Blazing {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let sprite =
      SpriteHandle::from_sprite(Sprite::solid_rect(4, 4, 255, 255, 255, 255), "look-blazing");
    ctx.world.spawn((
      Transform::from_xy(8.0, 16.0),
      Renderable::sprite(sprite).with_emission(self.0),
    ));
    ctx.effects().set_bloom(1.0);
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }
}

fn blazing(emission: f32) -> Option<image::RgbaImage> {
  let builder = EngineBuilder::new()
    .resolution(128, 32)
    .start_scene(Blazing(emission));
  let mut engine = HeadlessEngine::new_or_skip(builder)?;
  engine.step_frames(2).unwrap();
  Some(engine.frame().unwrap())
}

#[test]
fn infinite_emission_shows_white_and_leaves_the_rest_alone() {
  let (Some(frame), Some(plain)) = (blazing(f32::INFINITY), blazing(1.0)) else {
    return;
  };
  let centre = frame.get_pixel(8, 16).0;
  assert!(centre[..3].iter().all(|&c| c >= 242), "{centre:?}");
  // Far from it the background is what it is without the glow
  let (far, background) = (frame.get_pixel(124, 16).0, plain.get_pixel(124, 16).0);
  for (got, want) in far.iter().zip(background) {
    assert!(got.abs_diff(want) <= 1, "{far:?} vs {background:?}");
  }
  // NaN would show as black
  assert!(
    frame.pixels().all(|p| p.0[..3] != [0, 0, 0]),
    "no black (NaN) pixels"
  );
}
