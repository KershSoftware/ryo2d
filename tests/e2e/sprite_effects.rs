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
    ctx.look().tonemap = Tonemap::Clamp;
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
    .pixel_art(width, height)
    .start_scene(Fixture(Some(Box::new(setup))));
  let mut engine = HeadlessEngine::new_or_skip(builder)?;
  engine.step_frames(2).ok()?;
  engine.frame().ok()
}

/// 16x16, transparent but for one opaque white column at `column`
fn column_sprite(ctx: &mut SceneContext, column: u32) -> Handle<Image> {
  let mut data = vec![0u8; 16 * 16 * 4];
  for y in 0..16 {
    let i = ((y * 16 + column) * 4) as usize;
    data[i..i + 4].copy_from_slice(&WHITE);
  }
  ctx.assets().add(Image::from_rgba(data, 16, 16))
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
    let image = column_sprite(ctx, 14);
    ctx.world.spawn((
      Transform::from_xy(16.0, 16.0),
      Renderable::image(image)
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
      let image = column_sprite(ctx, 8);
      ctx.world.spawn((
        Transform::from_xy(16.0, 16.0),
        Renderable::image(image)
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
fn gradient_sprite(ctx: &mut SceneContext) -> Handle<Image> {
  let mut data = vec![0u8; 16 * 16 * 4];
  for y in 3..13u32 {
    for x in 3..13u32 {
      let i = ((y * 16 + x) * 4) as usize;
      data[i..i + 4].copy_from_slice(&[(x * 20) as u8, (y * 20) as u8, 90, 255]);
    }
  }
  ctx.assets().add(Image::from_rgba(data, 16, 16))
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
    let sprite = gradient_sprite(ctx);
    for (i, (_, effect)) in spawned.iter().enumerate() {
      ctx.world.spawn((
        Transform::from_xy(i as f32 * cell as f32 + 8.0, 8.0),
        Renderable::image(sprite.clone())
          .with_anchor(Anchor::TopLeft)
          .with_shader(SpriteShader::BuiltIn(effect.clone())),
      ));
    }
    ctx.world.spawn((
      Transform::from_xy((count - 1) as f32 * cell as f32 + 8.0, 8.0),
      Renderable::image(sprite).with_anchor(Anchor::TopLeft),
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

const RED: [u8; 4] = [220, 30, 40, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];

/// The columns of row `y` from 0 to `x1` near `colour`
fn columns_near(frame: &image::RgbaImage, y: u32, x1: u32, colour: [u8; 4]) -> Vec<u32> {
  (0..x1)
    .filter(|&x| {
      let pixel = frame.get_pixel(x, y).0;
      (0..3).all(|c| pixel[c].abs_diff(colour[c]) < 24)
    })
    .collect()
}

fn outline() -> SpriteShader {
  SpriteShader::BuiltIn(SpriteEffect::outline(
    Color::new(GREEN[0], GREEN[1], GREEN[2], 255),
    6,
  ))
}

#[test]
fn an_outline_reaching_into_view_from_a_sprite_just_off_screen_is_drawn() {
  // A 4 px red square from -8 to -4, wholly left of the view, outlined 6
  // px: its outline reaches to 2, so columns 0 and 1 of its rows are green
  let Some(frame) = render(64, 64, |ctx| {
    let image = ctx
      .assets()
      .add(Image::solid_rect(4, 4, RED[0], RED[1], RED[2], RED[3]));
    ctx.world.spawn((
      Transform::from_xy(-8.0, 30.0),
      Renderable::image(image)
        .with_anchor(Anchor::TopLeft)
        .with_shader(outline()),
    ));
  }) else {
    return;
  };
  assert_eq!(columns_near(&frame, 32, 8, GREEN), [0, 1]);
}

#[test]
fn an_outline_reaching_into_view_from_a_skeleton_s_region_just_off_screen_is_drawn() {
  // The same square as a skeleton's region, a run of one quad, centred on
  // its bone at -6
  let Some(frame) = render(64, 64, |ctx| {
    let image = ctx
      .assets()
      .add(Image::solid_rect(4, 4, RED[0], RED[1], RED[2], RED[3]));
    let shown = SlotSetup {
      attachment: Some("dot".into()),
      ..SlotSetup::default()
    };
    let builder = SkeletonData::builder()
      .bone("root", None, BoneSetup::default())
      .slot("dot", "root", shown)
      .region(SkeletonRegion {
        name: "dot".into(),
        image,
        x: 0,
        y: 0,
        width: 4,
        height: 4,
      });
    let index = builder.region_index("dot").unwrap_or_default();
    let data = builder
      .attachment(
        SkeletonData::DEFAULT_SKIN,
        "dot",
        "dot",
        SkeletonAttachment::Region(RegionAttachment::new(index, 4.0, 4.0)),
      )
      .build()
      .expect("a valid skeleton");
    let data = ctx.assets().add(data);
    ctx.world.spawn((
      Transform::from_xy(-6.0, 32.0),
      Skeleton::new(data),
      Renderable::skeleton().with_shader(outline()),
    ));
  }) else {
    return;
  };
  assert_eq!(columns_near(&frame, 32, 8, GREEN), [0, 1]);
}
