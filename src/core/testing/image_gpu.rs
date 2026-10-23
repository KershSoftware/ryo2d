//! Images on the GPU in a running engine: an image's texture goes with its
//! last handle, a scene change that keeps an image keeps its texture, a
//! reloaded image draws its new pixels (at its new size), and an image that
//! is not loaded draws the checker in debug builds.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use super::HeadlessEngine;
use crate::core::engine::EngineBuilder;
use crate::prelude::*;

const RED: [u8; 4] = [220, 30, 40, 255];
const GREEN: [u8; 4] = [30, 200, 60, 255];

/// A PNG file of `width` x `height` pixels of `colour`
fn png(width: u32, height: u32, colour: [u8; 4]) -> Vec<u8> {
  let image = image::RgbaImage::from_pixel(width, height, image::Rgba(colour));
  let mut bytes = Vec::new();
  image
    .write_to(
      &mut std::io::Cursor::new(&mut bytes),
      image::ImageFormat::Png,
    )
    .expect("encode a png");
  bytes
}

/// A headless engine 160 x 120 reading `files`, running `scene`
fn engine(files: MemorySource, scene: impl Scene + 'static) -> Option<HeadlessEngine> {
  HeadlessEngine::new_or_skip(
    EngineBuilder::new()
      .resolution(160, 120)
      .asset_source(files)
      .start_scene(scene),
  )
}

/// `image`'s sprite with its top-left at (20, 20), one pixel per texel, its
/// colours exact (no tonemap curve)
fn spawn_sprite(ctx: &mut SceneContext, image: Handle<Image>) -> Entity {
  ctx.look().tonemap = Tonemap::Clamp;
  ctx
    .world
    .spawn((
      Transform::from_xy(20.0, 20.0),
      Renderable::image(image).with_anchor(Anchor::TopLeft),
    ))
    .id()
}

/// The frame's pixel at (x, y)
fn pixel(engine: &HeadlessEngine, x: u32, y: u32) -> [u8; 4] {
  engine.frame().expect("a frame").get_pixel(x, y).0
}

/// Draws an image added in code until `drop` is set, then despawns the
/// sprite: the only handle goes with it
struct Drops {
  sprite: Option<Entity>,
  drop: Arc<AtomicBool>,
}

impl Scene for Drops {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let image = ctx
      .assets()
      .add(Image::solid_rect(64, 64, 220, 30, 40, 255));
    self.sprite = Some(spawn_sprite(ctx, image));
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    if self.drop.load(Ordering::SeqCst)
      && let Some(sprite) = self.sprite.take()
    {
      ctx.world.despawn(sprite);
    }
    SceneAction::Continue
  }
}

#[test]
fn an_unheld_image_frees_its_texture() {
  let drop = Arc::new(AtomicBool::new(false));
  let scene = Drops {
    sprite: None,
    drop: drop.clone(),
  };
  let Some(mut engine) = engine(MemorySource::new(), scene) else {
    return;
  };
  engine.step_frames(3).unwrap();
  assert_eq!(pixel(&engine, 50, 50), RED, "drawn");
  let (textures, rects) = (engine.texture_count(), engine.atlas_rects());
  drop.store(true, Ordering::SeqCst);
  engine.step_frames(2).unwrap();
  assert_eq!(
    engine.texture_count(),
    textures - 1,
    "the renderer no longer has its texture"
  );
  assert_eq!(engine.atlas_rects(), rects - 1, "its atlas room is free");
}

/// Loads `ship.png` and draws it; switches to [`Keeps`] when `leave` is set
struct Leaves {
  leave: Arc<AtomicBool>,
  kept: Arc<Mutex<Option<AssetId<Image>>>>,
}

impl Scene for Leaves {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let ship = ctx
      .assets()
      .load_now::<Image>("ship.png")
      .expect("ship.png loads");
    *self.kept.lock().unwrap() = Some(ship.id());
    spawn_sprite(ctx, ship);
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    if self.leave.load(Ordering::SeqCst) {
      let next = Keeps {
        kept: self.kept.clone(),
      };
      return SceneAction::Switch(Box::new(next));
    }
    SceneAction::Continue
  }
}

/// Loads `ship.png` on enter, as the scene before it did, and draws it
struct Keeps {
  kept: Arc<Mutex<Option<AssetId<Image>>>>,
}

impl Scene for Keeps {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let ship = ctx
      .assets()
      .load_now::<Image>("ship.png")
      .expect("ship.png loads");
    let mut kept = self.kept.lock().unwrap();
    assert_eq!(*kept, Some(ship.id()), "the same image, by id");
    *kept = Some(ship.id());
    spawn_sprite(ctx, ship);
  }
}

#[test]
fn a_scene_change_that_keeps_an_image_does_not_upload_it_again() {
  // Review Focus 3: the scene that held the image is dropped in the frame
  // the next one loads it
  let files = MemorySource::new();
  files.insert("ship.png", png(64, 64, RED));
  let leave = Arc::new(AtomicBool::new(false));
  let scene = Leaves {
    leave: leave.clone(),
    kept: Arc::default(),
  };
  let Some(mut engine) = engine(files, scene) else {
    return;
  };
  engine.step_frames(3).unwrap();
  let uploads = engine.image_uploads();
  assert!(uploads > 0, "the ship was uploaded");
  leave.store(true, Ordering::SeqCst);
  engine.step_frames(3).unwrap();
  assert_eq!(pixel(&engine, 50, 50), RED, "the next scene draws it");
  assert_eq!(engine.image_uploads(), uploads, "not uploaded again");
}

/// Loads `ship.png` and draws it
struct Ship;

impl Scene for Ship {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let ship = ctx
      .assets()
      .load_now::<Image>("ship.png")
      .expect("ship.png loads");
    spawn_sprite(ctx, ship);
  }
}

/// Steps until the pixel at (x, y) is `colour`, or a second has passed
fn step_until(engine: &mut HeadlessEngine, (x, y): (u32, u32), colour: [u8; 4]) -> [u8; 4] {
  let start = std::time::Instant::now();
  loop {
    engine.step().unwrap();
    let now = pixel(engine, x, y);
    if now == colour || start.elapsed() > std::time::Duration::from_secs(1) {
      return now;
    }
    std::thread::sleep(std::time::Duration::from_millis(2));
  }
}

#[test]
fn a_reloaded_image_draws_its_new_pixels() {
  let files = MemorySource::new();
  files.insert("ship.png", png(64, 64, RED));
  let Some(mut engine) = engine(files.clone(), Ship) else {
    return;
  };
  engine.step_frames(2).unwrap();
  assert_eq!(pixel(&engine, 50, 50), RED);
  files.insert("ship.png", png(64, 64, GREEN));
  engine.assets().server().reload("ship.png");
  assert_eq!(step_until(&mut engine, (50, 50), GREEN), GREEN, "redrawn");
  assert_eq!(pixel(&engine, 20, 20), GREEN, "its top-left");
  assert_eq!(pixel(&engine, 83, 83), GREEN, "its bottom-right");
  assert_ne!(pixel(&engine, 84, 84), GREEN, "no bigger");
}

#[test]
fn a_reloaded_image_of_another_size_is_placed_again() {
  let files = MemorySource::new();
  files.insert("ship.png", png(64, 64, RED));
  let Some(mut engine) = engine(files.clone(), Ship) else {
    return;
  };
  engine.step_frames(2).unwrap();
  assert_eq!(pixel(&engine, 50, 50), RED);
  assert_ne!(pixel(&engine, 100, 100), RED, "64 px: not that far");
  files.insert("ship.png", png(128, 128, GREEN));
  engine.assets().server().reload("ship.png");
  assert_eq!(step_until(&mut engine, (100, 100), GREEN), GREEN);
  // 128 px from (20, 20): the whole of it, in the new pixels
  for at in [(20, 20), (50, 50), (100, 100), (147, 119)] {
    assert_eq!(pixel(&engine, at.0, at.1), GREEN, "{at:?}");
  }
}

/// Draws an image that will never load: its file is missing
struct Missing;

impl Scene for Missing {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let ghost = ctx.assets().load::<Image>("ghost.png");
    let sprite = spawn_sprite(ctx, ghost);
    // Four times its size from (28, 28), its texels whole
    if let Some(mut at) = ctx.world.get_mut::<Transform>(sprite) {
      *at = Transform::from_xy(28.0, 28.0).with_scale(Vec2::splat(4.0));
    }
    if let Some(mut renderable) = ctx.world.get_mut::<Renderable>(sprite) {
      renderable.filter = Filter::Nearest;
    }
  }
}

#[test]
fn an_unloaded_image_draws_the_checker_in_debug() {
  let Some(mut engine) = engine(MemorySource::new(), Missing) else {
    return;
  };
  engine.step_frames(3).unwrap();
  let background = pixel(&engine, 2, 2);
  // The checker's 16 texels at 4 px each from (28, 28): texel (8, 8), a
  // magenta square, at the sprite's centre; texel (4, 8) is black
  let centre = pixel(&engine, 61, 61);
  let beside = pixel(&engine, 45, 61);
  if cfg!(debug_assertions) {
    assert_eq!(centre, [255, 0, 255, 255], "magenta at the centre");
    assert_eq!(beside, [0, 0, 0, 255], "black beside it");
  } else {
    assert_eq!(centre, background, "nothing drawn");
    assert_eq!(beside, background, "nothing drawn");
  }
}
