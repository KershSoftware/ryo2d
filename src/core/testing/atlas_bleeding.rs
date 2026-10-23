//! Sprites sharing an atlas page never show a neighbour's pixels.

use std::sync::{Arc, Mutex};

use super::HeadlessEngine;
use crate::core::engine::EngineBuilder;
use crate::prelude::*;

const RED: [u8; 4] = [220, 30, 40, 255];
const BLUE: [u8; 4] = [30, 60, 230, 255];

/// Uploads red then blue (side by side in the atlas) and draws only red.
struct Bleed {
  /// Red and blue, added on enter (and their ids, for the test)
  images: Option<[Handle<Image>; 2]>,
  ids: Arc<Mutex<Option<[AssetId<Image>; 2]>>>,
  centre: f32,
  scale: f32,
  rotation: f32,
  flip_x: bool,
}

impl Bleed {
  fn new(centre: f32, scale: f32, rotation: f32, flip_x: bool) -> Self {
    Self {
      images: None,
      ids: Arc::default(),
      centre,
      scale,
      rotation,
      flip_x,
    }
  }
}

impl Scene for Bleed {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let solid = |c: [u8; 4]| Image::solid_rect(4, 4, c[0], c[1], c[2], c[3]);
    let images = [ctx.assets().add(solid(RED)), ctx.assets().add(solid(BLUE))];
    *self.ids.lock().unwrap() = Some(images.each_ref().map(Handle::id));
    self.images = Some(images);
  }

  fn render(&mut self, gpu: &mut GpuRenderContext) {
    let Some([red, blue]) = &self.images else {
      return;
    };
    let size = 4.0 * self.scale;
    let mut instance =
      SpriteInstance::new().with_transform(self.centre, self.centre, self.rotation, size, size);
    if self.flip_x {
      instance = instance.with_uv(1.0, 0.0, 0.0, 1.0);
    }
    gpu.add_sprite(red, instance, SpriteDraw::default());
    // Blue off screen, so it is uploaded next to red in the atlas
    let off_screen = SpriteInstance::new().with_transform(-100.0, -100.0, 0.0, 4.0, 4.0);
    gpu.add_sprite(blue, off_screen, SpriteDraw::default());
  }
}

/// Draw a scene and check the frame; returns the frame for further checks.
fn draw(scene: Bleed) -> Option<image::RgbaImage> {
  let ids = Arc::clone(&scene.ids);
  let builder = EngineBuilder::new().resolution(64, 64).start_scene(scene);
  let mut engine = HeadlessEngine::new_or_skip(builder)?;
  engine.step_frames(2).unwrap();

  // Bleeding needs the two sprites on one atlas page
  let [scene_red, scene_blue] = ids.lock().unwrap().expect("added on enter");
  let gpu = engine.engine.sprite_gpu.as_ref().expect("renderer");
  let red = gpu.lookup_image(scene_red).expect("red resident");
  let blue = gpu.lookup_image(scene_blue).expect("blue resident");
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
