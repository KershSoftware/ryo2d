//! The look: exposure, tonemap curves, grading, LUTs and auto-exposure,
//! read back as display bytes.
//!
//! Tests skip when no GPU is available.

use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

type Setup = Box<dyn FnMut(&mut SceneContext) + Send>;

/// A square of `color` filling the 32x32 window, under `look` (graded by
/// the strip `lut` too, if there is one), then `setup`.
struct Square {
  color: [u8; 4],
  look: Look,
  lut: Option<Image>,
  setup: Option<Setup>,
}

impl Scene for Square {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let [r, g, b, a] = self.color;
    let sprite = ctx.assets().add(Image::solid_rect(32, 32, r, g, b, a));
    ctx
      .world
      .spawn((Transform::from_xy(16.0, 16.0), Renderable::image(sprite)));
    *ctx.look() = self.look.clone();
    if let Some(strip) = self.lut.take() {
      let strip = ctx.assets().add(strip);
      let Ok(lut) = ColorLut::from_sprite(&strip, ctx.world.resource::<Assets<Image>>()) else {
        panic!("the strip is a LUT");
      };
      ctx.look().grading.lut = Some(lut);
    }
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
    lut: None,
    setup: None,
  }
}

/// `square`, graded by the strip `lut` too
fn with_lut(square: Square, lut: Image) -> Square {
  Square {
    lut: Some(lut),
    ..square
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
    lut: None,
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

/// A LUT strip that inverts every sRGB channel.
fn inverting_lut() -> Image {
  let strip = ColorLut::identity_strip(16);
  let inverted: Vec<u8> = strip
    .data()
    .chunks(4)
    .flat_map(|p| [255 - p[0], 255 - p[1], 255 - p[2], 255])
    .collect();
  Image::from_rgba(inverted, strip.width(), strip.height())
}

#[test]
fn an_inverting_lut_inverts() {
  let look = graded(ColorGrading::default());
  let Some(pixel) = shown(with_lut(square([200, 100, 50, 255], look), inverting_lut())) else {
    return;
  };
  for (got, want) in pixel.iter().zip([55u8, 155, 205]) {
    assert!(got.abs_diff(want) <= 2, "{pixel:?}");
  }
}

#[test]
fn an_identity_lut_changes_nothing() {
  let look = graded(ColorGrading::default());
  let identity = ColorLut::identity_strip(16);
  let Some(pixel) = shown(with_lut(square([200, 100, 50, 255], look), identity)) else {
    return;
  };
  for (got, want) in pixel.iter().zip([200u8, 100, 50]) {
    assert!(got.abs_diff(want) <= 1, "{pixel:?}");
  }
}

#[test]
fn half_strength_blends_halfway() {
  let look = graded(ColorGrading {
    lut_strength: 0.5,
    ..ColorGrading::default()
  });
  let Some(pixel) = shown(with_lut(square([200, 100, 50, 255], look), inverting_lut())) else {
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
    lut: None,
    color: [0, 0, 0, 255],
    look,
    setup: Some(Box::new(move |ctx: &mut SceneContext| {
      let [r, g, b, a] = color;
      let sprite = ctx.assets().add(Image::solid_rect(32, 32, r, g, b, a));
      ctx.world.spawn((
        Transform::from_xy(16.0, 16.0),
        Renderable::image(sprite)
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
    let sprite = ctx
      .assets()
      .add(Image::solid_rect(4, 4, 255, 255, 255, 255));
    ctx.world.spawn((
      Transform::from_xy(8.0, 16.0),
      Renderable::image(sprite).with_emission(self.0),
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

fn auto() -> Look {
  Look {
    auto_exposure: Some(AutoExposure::default()),
    ..Look::default()
  }
}

#[test]
fn auto_exposure_starts_at_its_target() {
  let Some(pixel) = shown(square([39, 39, 39, 255], auto())) else {
    return;
  };
  // 0.0203 linear, brought to mid-grey (0.18, sRGB 118) on the first frame,
  // within the histogram's quarter-stop bins
  assert!(pixel[0].abs_diff(118) <= 4, "{pixel:?}");
}

#[test]
fn auto_exposure_stops_at_max_ev() {
  let Some(pixel) = shown(square([3, 3, 3, 255], auto())) else {
    return;
  };
  // 0.00091 linear wants 7.6 EV; at the 4 EV limit it shows 0.0146 (sRGB 32)
  assert!(pixel[0].abs_diff(32) <= 3, "{pixel:?}");
}

#[test]
fn auto_exposure_eases_toward_a_darker_scene() {
  // A dark square under a mid-grey one, which is hidden at frame 10
  let scene = Square {
    lut: None,
    color: [39, 39, 39, 255],
    look: auto(),
    setup: Some(Box::new(|ctx: &mut SceneContext| {
      let grey = ctx
        .assets()
        .add(Image::solid_rect(32, 32, 118, 118, 118, 255));
      ctx.world.spawn((
        Transform::from_xy(16.0, 16.0),
        Renderable::image(grey).with_z_order(1),
      ));
    })),
  };
  let Some(mut engine) = start(scene) else {
    return;
  };
  engine.step_frames(10).unwrap();
  let world = engine.world_mut();
  let mut query = world.query::<&mut Renderable>();
  for mut renderable in query.iter_mut(world) {
    if renderable.z_order == 1 {
      renderable.visible = false;
    }
  }
  engine.step_frames(1).unwrap();
  let soon = engine.frame().unwrap().get_pixel(16, 16).0[0];
  engine.step_frames(90).unwrap();
  let later = engine.frame().unwrap().get_pixel(16, 16).0[0];
  // One frame moves 3 EV/s x 1/60 s: still about as dark as the square is
  assert!(soon < 50, "after a frame: {soon}");
  assert!(
    later.abs_diff(118) <= 4,
    "after a second and a half: {later}"
  );
}

#[test]
fn auto_exposure_ignores_overflowed_light() {
  // The dark square plus an 8x8 pile-up past f16's range (6% of the frame)
  let scene = Square {
    lut: None,
    color: [39, 39, 39, 255],
    look: auto(),
    setup: Some(Box::new(|ctx: &mut SceneContext| {
      let blaze = ctx
        .assets()
        .add(Image::solid_rect(8, 8, 255, 255, 255, 255));
      for z in 1..=2 {
        ctx.world.spawn((
          Transform::from_xy(4.0, 4.0),
          Renderable::image(blaze.clone())
            .with_z_order(z)
            .with_blend(BlendMode::Additive)
            .with_emission(65504.0),
        ));
      }
    })),
  };
  // Away from the blaze
  let Some(pixel) = shown_at(scene, 24, 24) else {
    return;
  };
  assert!(pixel[0].abs_diff(118) <= 4, "{pixel:?}");
}

/// The pixel at (`x`, `y`) after two frames.
fn shown_at(scene: impl Scene + 'static, x: u32, y: u32) -> Option<[u8; 4]> {
  let mut engine = start(scene)?;
  engine.step_frames(2).unwrap();
  Some(engine.frame().unwrap().get_pixel(x, y).0)
}
