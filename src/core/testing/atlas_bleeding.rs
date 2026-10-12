//! Sprites sharing an atlas page never show a neighbour's pixels.

use super::HeadlessEngine;
use crate::core::engine::EngineBuilder;
use crate::prelude::*;

const RED: [u8; 4] = [220, 30, 40, 255];
const BLUE: [u8; 4] = [30, 60, 230, 255];

/// Uploads red then blue (side by side in the atlas) and draws only red.
struct Bleed {
  red: SpriteHandle,
  blue: SpriteHandle,
  centre: f32,
  scale: f32,
  rotation: f32,
  flip_x: bool,
}

impl Bleed {
  fn new(centre: f32, scale: f32, rotation: f32, flip_x: bool) -> Self {
    let solid = |name: &str, c: [u8; 4]| {
      SpriteHandle::from_sprite(Sprite::solid_rect(4, 4, c[0], c[1], c[2], c[3]), name)
    };
    Self {
      red: solid("bleed-red", RED),
      blue: solid("bleed-blue", BLUE),
      centre,
      scale,
      rotation,
      flip_x,
    }
  }
}

impl Scene for Bleed {
  fn render(&mut self, gpu: &mut GpuRenderContext) {
    gpu.upload_sprite(&self.red);
    gpu.upload_sprite(&self.blue);
    let size = 4.0 * self.scale;
    let mut instance =
      SpriteInstance::new().with_transform(self.centre, self.centre, self.rotation, size, size);
    if self.flip_x {
      instance = instance.with_uv(1.0, 0.0, 0.0, 1.0);
    }
    gpu.add_sprite_instance(&self.red.id(), instance, 0, false);
  }
}

/// Draw a scene and check the frame; returns the frame for further checks.
fn draw(scene: Bleed) -> Option<image::RgbaImage> {
  let builder = EngineBuilder::new().resolution(64, 64).start_scene(scene);
  let mut engine = HeadlessEngine::new_or_skip(builder)?;
  engine.step_frames(2).unwrap();

  // Bleeding needs the two sprites on one atlas page
  let gpu = engine.engine.sprite_gpu.as_ref().expect("renderer");
  let red = gpu.lookup("bleed-red").expect("red resident");
  let blue = gpu.lookup("bleed-blue").expect("blue resident");
  assert_eq!(red.page, blue.page, "sprites must share an atlas page");

  Some(engine.frame().unwrap())
}

/// Every pixel is the sprite's colour or the background; returns the red count.
fn assert_only_red_or_background(frame: &image::RgbaImage) -> usize {
  let background = frame.get_pixel(0, 0).0;
  let mut reds = 0;
  for (x, y, p) in frame.enumerate_pixels() {
    if p.0 == RED {
      reds += 1;
    } else {
      assert_eq!(p.0, background, "({x},{y}) is neither red nor background");
    }
  }
  reds
}

#[test]
fn fractional_scale_and_position_do_not_bleed() {
  let Some(frame) = draw(Bleed::new(32.4, 3.3, 0.0, false)) else {
    return;
  };
  assert!(assert_only_red_or_background(&frame) > 100);
}

#[test]
fn rotation_does_not_bleed() {
  let Some(frame) = draw(Bleed::new(32.4, 3.3, 7f32.to_radians(), false)) else {
    return;
  };
  assert!(assert_only_red_or_background(&frame) > 100);
}

#[test]
fn flipped_edges_on_pixel_centres_do_not_bleed() {
  // Edges at x/y = 26.5 and 38.5 sit exactly on pixel centres, so a flipped
  // sprite samples exactly on its rect's far edge, where the next atlas
  // texel (the neighbour) begins
  let Some(frame) = draw(Bleed::new(32.5, 3.0, 0.0, true)) else {
    return;
  };
  assert_only_red_or_background(&frame);
  for y in 26..38 {
    for x in 26..38 {
      assert_eq!(frame.get_pixel(x, y).0, RED, "({x},{y})");
    }
  }
}
