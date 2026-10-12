//! Built-in sprite effects on entities, drawn by the GPU sprite shader.
//!
//! Tests skip when no GPU is available.

use std::f32::consts::FRAC_PI_2;

use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

const WHITE: [u8; 4] = [255, 255, 255, 255];

type Setup = Box<dyn FnMut(&mut SceneContext) + Send>;

/// A scene that spawns its entities once
struct Fixture(Option<Setup>);

impl Scene for Fixture {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    if let Some(mut setup) = self.0.take() {
      setup(ctx);
    }
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }
}

fn render(
  width: u32,
  height: u32,
  setup: impl FnMut(&mut SceneContext) + Send + 'static,
) -> Option<image::RgbaImage> {
  let builder = EngineBuilder::new()
    .resolution(width, height)
    .start_scene(Fixture(Some(Box::new(setup))));
  let mut engine = HeadlessEngine::new_or_skip(builder)?;
  engine.step_frames(2).ok()?;
  engine.frame().ok()
}

/// 16x16, transparent but for one opaque white column at `column`
fn column_sprite(name: &str, column: u32) -> SpriteHandle {
  let mut data = vec![0u8; 16 * 16 * 4];
  for y in 0..16 {
    let i = ((y * 16 + column) * 4) as usize;
    data[i..i + 4].copy_from_slice(&WHITE);
  }
  SpriteHandle::from_sprite(Sprite::from_rgba(data, 16, 16), name)
}

/// The white pixels of row `y` between `x0` and `x1`
fn white_in_row(frame: &image::RgbaImage, y: u32, x0: u32, x1: u32) -> Vec<u32> {
  (x0..x1)
    .filter(|&x| frame.get_pixel(x, y).0 == WHITE)
    .collect()
}

#[test]
fn wave_shifts_rows_by_its_amplitude_in_pixels_and_wraps() {
  // Amplitude 4 px, one cycle down the height, phase pi/2: row 0 shifts
  // right by about 4 (wrapping column 14 round to 2), row 8 left by about 4
  let Some(frame) = render(64, 64, |ctx| {
    let wave = SpriteEffect::Wave {
      amplitude: 4.0,
      frequency: 1.0,
      speed: 0.0,
      time: FRAC_PI_2,
    };
    ctx.world.spawn((
      Transform::from_xy(16.0, 16.0),
      Renderable::sprite(column_sprite("wave-column", 14))
        .with_anchor(Anchor::TopLeft)
        .with_shader(SpriteShader::BuiltIn(wave)),
    ));
  }) else {
    return;
  };
  assert_eq!(white_in_row(&frame, 16, 0, 64), [18], "row 0 wraps right");
  assert_eq!(white_in_row(&frame, 24, 0, 64), [26], "row 8 shifts left");
}

#[test]
fn wave_follows_its_effect_time() {
  let at = |time: f32| {
    render(64, 64, move |ctx| {
      let wave = SpriteEffect::Wave {
        amplitude: 4.0,
        frequency: 1.0,
        speed: 0.0,
        time,
      };
      ctx.world.spawn((
        Transform::from_xy(16.0, 16.0),
        Renderable::sprite(column_sprite("wave-time-column", 8))
          .with_anchor(Anchor::TopLeft)
          .with_shader(SpriteShader::BuiltIn(wave)),
      ));
    })
  };
  let (Some(start), Some(later)) = (at(0.0), at(FRAC_PI_2)) else {
    return;
  };
  assert_ne!(
    white_in_row(&start, 16, 0, 64),
    white_in_row(&later, 16, 0, 64)
  );
}

/// 16x16 gradient with a 3 px transparent border, so outlines and glows have
/// room inside the quad and pixelate has detail to remove
fn gradient_sprite() -> SpriteHandle {
  let mut data = vec![0u8; 16 * 16 * 4];
  for y in 3..13u32 {
    for x in 3..13u32 {
      let i = ((y * 16 + x) * 4) as usize;
      data[i..i + 4].copy_from_slice(&[(x * 20) as u8, (y * 20) as u8, 90, 255]);
    }
  }
  SpriteHandle::from_sprite(Sprite::from_rgba(data, 16, 16), "effects-gradient")
}

#[test]
fn every_built_in_effect_changes_its_entity() {
  let effects = [
    ("outline", SpriteEffect::outline(Color::RED, 1)),
    ("glow", SpriteEffect::glow(Color::YELLOW, 3.0)),
    ("flash", SpriteEffect::flash(Color::WHITE, 0.8)),
    ("color shift", SpriteEffect::color_shift(Color::BLUE, 0.6)),
    ("silhouette", SpriteEffect::silhouette(Color::BLACK)),
    ("pixelate", SpriteEffect::pixelate(4)),
    ("wave", SpriteEffect::wave(3.0, 1.0, 0.0)),
    ("dissolve", SpriteEffect::dissolve(0.5)),
    ("invert", SpriteEffect::invert()),
    ("grayscale", SpriteEffect::full_grayscale()),
  ];
  // One 32 px cell per effect, the plain sprite in the last cell
  let cell = 32u32;
  let count = effects.len() as u32 + 1;
  let spawned = effects.clone();
  let Some(frame) = render(cell * count, cell, move |ctx| {
    let sprite = gradient_sprite();
    for (i, (_, effect)) in spawned.iter().enumerate() {
      ctx.world.spawn((
        Transform::from_xy(i as f32 * cell as f32 + 8.0, 8.0),
        Renderable::sprite(sprite.clone())
          .with_anchor(Anchor::TopLeft)
          .with_shader(SpriteShader::BuiltIn(effect.clone())),
      ));
    }
    ctx.world.spawn((
      Transform::from_xy((count - 1) as f32 * cell as f32 + 8.0, 8.0),
      Renderable::sprite(sprite).with_anchor(Anchor::TopLeft),
    ));
  }) else {
    return;
  };
  let cell_pixels = |index: u32| -> Vec<[u8; 4]> {
    (0..cell)
      .flat_map(|y| (0..cell).map(move |x| (x, y)))
      .map(|(x, y)| frame.get_pixel(index * cell + x, y).0)
      .collect()
  };
  let plain = cell_pixels(count - 1);
  for (i, (name, _)) in effects.iter().enumerate() {
    let changed = cell_pixels(i as u32)
      .iter()
      .zip(&plain)
      .filter(|(a, b)| a != b)
      .count();
    assert!(changed >= 8, "{name} changed only {changed} pixels");
  }
}
