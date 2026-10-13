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
      let look = Look { exposure, tonemap };
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
