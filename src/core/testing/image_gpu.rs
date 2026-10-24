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

const BLUE: [u8; 4] = [40, 60, 230, 255];

/// A large image's side: over the atlas's 256 px, so it gets its own
/// texture, and its levels (about 280 KB) take many frames at the budgets
/// these tests set
const LARGE: (u32, u32) = (300, 150);

/// Loads `path` and draws it twice: at one pixel per texel with its
/// top-left at (10, 10), and at a quarter of its size from (330, 10),
/// filtered linearly, where it samples the smaller levels. The sprites go
/// in `sprites`
struct Large {
  path: &'static str,
  sprites: Arc<Mutex<Vec<Entity>>>,
}

impl Large {
  fn new(path: &'static str) -> (Large, Arc<Mutex<Vec<Entity>>>) {
    let sprites = Arc::<Mutex<Vec<Entity>>>::default();
    let scene = Large {
      path,
      sprites: sprites.clone(),
    };
    (scene, sprites)
  }
}

impl Scene for Large {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let image = ctx.assets().load_now::<Image>(self.path).expect("it loads");
    let whole = spawn_sprite(ctx, image.clone());
    if let Some(mut at) = ctx.world.get_mut::<Transform>(whole) {
      *at = Transform::from_xy(10.0, 10.0);
    }
    let quarter = spawn_sprite(ctx, image);
    if let Some(mut at) = ctx.world.get_mut::<Transform>(quarter) {
      *at = Transform::from_xy(330.0, 10.0).with_scale(Vec2::splat(0.25));
    }
    // Linear, so it samples the smaller levels (nearest keeps to the first)
    if let Some(mut renderable) = ctx.world.get_mut::<Renderable>(quarter) {
      renderable.filter = Filter::Linear;
    }
    self.sprites.lock().unwrap().extend([whole, quarter]);
  }
}

/// A headless engine 480 x 320 reading `files`, running `scene`
fn large_engine(files: MemorySource, scene: impl Scene + 'static) -> Option<HeadlessEngine> {
  HeadlessEngine::new_or_skip(
    EngineBuilder::new()
      .resolution(480, 320)
      .asset_source(files)
      .start_scene(scene),
  )
}

/// What an engine drawing `width` x `height` px of `colour` from the
/// start shows: the frame a reload to those pixels must end in
fn drawn_from_the_start((width, height): (u32, u32), colour: [u8; 4]) -> image::RgbaImage {
  let files = MemorySource::new();
  files.insert("ship.png", png(width, height, colour));
  let (scene, _) = Large::new("ship.png");
  let mut engine = large_engine(files, scene).expect("a GPU, as the test's own engine had");
  engine.step_frames(2).unwrap();
  engine.frame().expect("a frame")
}

/// Image `path`'s id, held by the scene
fn image_id(engine: &mut HeadlessEngine, path: &str) -> AssetId<Image> {
  engine
    .assets()
    .load_now::<Image>(path)
    .expect("loaded")
    .id()
}

/// Image `id`'s version in the store
fn version(engine: &HeadlessEngine, id: AssetId<Image>) -> Option<u64> {
  engine.world().resource::<Assets<Image>>().version(id)
}

/// Puts `bytes` at `path` and reloads it, stepping until its new value
/// lands (the last frame stepped), or two seconds pass
fn reload_until_landed(
  engine: &mut HeadlessEngine,
  files: &MemorySource,
  (path, id): (&str, AssetId<Image>),
  bytes: Vec<u8>,
) {
  let before = version(engine, id);
  files.insert(path, bytes);
  engine.assets().server().reload(path);
  let start = std::time::Instant::now();
  while version(engine, id) == before {
    assert!(
      start.elapsed() < std::time::Duration::from_secs(2),
      "{path} never landed"
    );
    engine.step().unwrap();
  }
}

/// Steps while image `id`'s new version is still being written, asserting
/// every frame draws `old`; how many frames that took (at most 2,000)
fn frames_drawing(engine: &mut HeadlessEngine, id: AssetId<Image>, old: &image::RgbaImage) -> u32 {
  let mut frames = 0;
  while engine.image_upload_pending(id) {
    assert!(
      engine.frame().expect("a frame") == *old,
      "frame {frames} of the upload draws something other than the old pixels"
    );
    assert!(frames < 2_000, "the upload never ends");
    engine.step().unwrap();
    frames += 1;
  }
  frames
}

#[test]
fn a_large_reload_draws_its_old_pixels_until_the_new_are_whole_then_the_new() {
  let files = MemorySource::new();
  files.insert("ship.png", png(LARGE.0, LARGE.1, RED));
  let (scene, _) = Large::new("ship.png");
  let Some(mut engine) = large_engine(files.clone(), scene) else {
    return;
  };
  // 16 KiB a frame: the image's levels take about 18 frames
  engine.set_upload_budget(16 * 1024);
  engine.step_frames(2).unwrap();
  let old = engine.frame().expect("a frame");
  assert_eq!(old.get_pixel(150, 80).0, RED);
  let id = image_id(&mut engine, "ship.png");
  let green = png(LARGE.0, LARGE.1, GREEN);
  reload_until_landed(&mut engine, &files, ("ship.png", id), green);
  assert!(
    engine.frame().expect("a frame") == old,
    "the frame the new value lands in draws the old pixels"
  );
  assert!(engine.image_upload_pending(id), "it is written over frames");
  engine.step().unwrap();
  let frames = frames_drawing(&mut engine, id, &old);
  assert!(frames > 2, "written over {frames} more frames");
  // The new pixels at once, every level of them, and then as they are
  let new = drawn_from_the_start(LARGE, GREEN);
  for _ in 0..3 {
    assert!(
      engine.frame().expect("a frame") == new,
      "the new pixels, whole"
    );
    engine.step().unwrap();
  }
  assert_eq!(engine.spread_uploads(), 0);
}

#[test]
fn a_reload_landing_during_a_spread_upload_drops_it_and_writes_the_newest() {
  let files = MemorySource::new();
  files.insert("ship.png", png(LARGE.0, LARGE.1, RED));
  let (scene, _) = Large::new("ship.png");
  let Some(mut engine) = large_engine(files.clone(), scene) else {
    return;
  };
  // About a row a frame: the upload outlasts the next reload's decode
  engine.set_upload_budget(2 * 1024);
  engine.step_frames(2).unwrap();
  let old = engine.frame().expect("a frame");
  let id = image_id(&mut engine, "ship.png");
  let green = png(LARGE.0, LARGE.1, GREEN);
  reload_until_landed(&mut engine, &files, ("ship.png", id), green);
  assert!(
    engine.image_upload_pending(id),
    "the green is being written"
  );
  let blue = png(LARGE.0, LARGE.1, BLUE);
  reload_until_landed(&mut engine, &files, ("ship.png", id), blue);
  assert!(
    engine.image_upload_pending(id),
    "the blue landed while an upload was still being written"
  );
  assert_eq!(
    engine.spread_uploads(),
    1,
    "the green's partial texture went"
  );
  frames_drawing(&mut engine, id, &old);
  assert!(
    engine.frame().expect("a frame") == drawn_from_the_start(LARGE, BLUE),
    "the newest pixels, whole, and never the green"
  );
}

#[test]
fn an_image_freed_during_a_spread_upload_loses_both_its_textures_at_once() {
  let files = MemorySource::new();
  files.insert("ship.png", png(LARGE.0, LARGE.1, RED));
  let (scene, sprites) = Large::new("ship.png");
  let Some(mut engine) = large_engine(files.clone(), scene) else {
    return;
  };
  engine.set_upload_budget(2 * 1024);
  engine.step_frames(2).unwrap();
  let (textures, rects) = (engine.texture_count(), engine.atlas_rects());
  let id = image_id(&mut engine, "ship.png");
  let green = png(LARGE.0, LARGE.1, GREEN);
  reload_until_landed(&mut engine, &files, ("ship.png", id), green);
  assert!(
    engine.image_upload_pending(id),
    "the green is being written"
  );
  // The sprites held the only handles
  for sprite in sprites.lock().unwrap().drain(..) {
    engine.world_mut().despawn(sprite);
  }
  engine.step_frames(2).unwrap();
  assert!(version(&engine, id).is_none(), "freed");
  assert_eq!(engine.spread_uploads(), 0, "the new texture went");
  assert!(!engine.image_upload_pending(id));
  assert_eq!(engine.texture_count(), textures - 1, "the old texture went");
  assert_eq!(engine.atlas_rects(), rects - 1);
}

#[test]
fn a_large_reload_of_another_size_is_placed_again_when_it_is_whole() {
  const OTHER: (u32, u32) = (260, 220);
  let files = MemorySource::new();
  files.insert("ship.png", png(LARGE.0, LARGE.1, RED));
  let (scene, _) = Large::new("ship.png");
  let Some(mut engine) = large_engine(files.clone(), scene) else {
    return;
  };
  engine.set_upload_budget(16 * 1024);
  engine.step_frames(2).unwrap();
  let old = engine.frame().expect("a frame");
  let id = image_id(&mut engine, "ship.png");
  let green = png(OTHER.0, OTHER.1, GREEN);
  reload_until_landed(&mut engine, &files, ("ship.png", id), green);
  assert!(
    engine.frame().expect("a frame") == old,
    "the frame the new value lands in draws the old pixels at the old size"
  );
  assert!(engine.image_upload_pending(id), "it is written over frames");
  engine.step().unwrap();
  let frames = frames_drawing(&mut engine, id, &old);
  assert!(frames > 2, "written over {frames} more frames");
  assert!(
    engine.frame().expect("a frame") == drawn_from_the_start(OTHER, GREEN),
    "the new pixels at the new size"
  );
}

/// [`Large`], and 65 more images over 256 px added in code, each drawn
/// small: with the ship, two over the cap on textures of their own
struct Crowded {
  large: Large,
}

impl Scene for Crowded {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    self.large.on_enter(ctx);
    for n in 0..65u8 {
      let image = ctx
        .assets()
        .add(Image::solid_rect(260, 260, n, 100, 100, 255));
      let x = 10.0 + f32::from(n % 32) * 14.0;
      let y = 280.0 + f32::from(n / 32) * 12.0;
      ctx.world.spawn((
        Transform::from_xy(x, y).with_scale(Vec2::splat(0.03)),
        Renderable::image(image).with_anchor(Anchor::TopLeft),
      ));
    }
  }
}

#[test]
fn an_image_the_standalone_cap_evicts_during_its_spread_upload_is_written_whole_when_drawn_again() {
  let files = MemorySource::new();
  files.insert("ship.png", png(LARGE.0, LARGE.1, RED));
  let (large, sprites) = Large::new("ship.png");
  let Some(mut engine) = large_engine(files.clone(), Crowded { large }) else {
    return;
  };
  engine.set_upload_budget(2 * 1024);
  engine.step_frames(2).unwrap();
  let id = image_id(&mut engine, "ship.png");
  let green = png(LARGE.0, LARGE.1, GREEN);
  reload_until_landed(&mut engine, &files, ("ship.png", id), green);
  assert!(
    engine.image_upload_pending(id),
    "the green is being written"
  );
  // Not drawn for a frame, the ship is the least recently used of the
  // textures over the cap
  let sprites = sprites.lock().unwrap().clone();
  let mut places = Vec::new();
  for &sprite in &sprites {
    let mut at = engine.world_mut().get_mut::<Transform>(sprite).unwrap();
    places.push(*at);
    at.translation.x = -10_000.0;
  }
  engine.step().unwrap();
  assert!(!engine.image_upload_pending(id), "evicted, with its upload");
  assert_eq!(engine.spread_uploads(), 0);
  for (sprite, place) in sprites.into_iter().zip(places) {
    *engine.world_mut().get_mut::<Transform>(sprite).unwrap() = place;
  }
  engine.step().unwrap();
  assert_eq!(
    pixel(&engine, 150, 80),
    GREEN,
    "drawn again, the newest pixels are uploaded at once"
  );
  assert_eq!(pixel(&engine, 340, 20), GREEN, "every level of them");
}

#[test]
fn a_large_image_changed_in_code_draws_its_new_pixels_at_once() {
  let files = MemorySource::new();
  files.insert("ship.png", png(LARGE.0, LARGE.1, RED));
  let (scene, _) = Large::new("ship.png");
  let Some(mut engine) = large_engine(files, scene) else {
    return;
  };
  engine.set_upload_budget(2 * 1024);
  engine.step_frames(2).unwrap();
  let id = image_id(&mut engine, "ship.png");
  {
    let mut images = engine.world_mut().resource_mut::<Assets<Image>>();
    let image = images.get_mut(id).expect("loaded");
    for texel in image.data_mut().as_chunks_mut::<4>().0 {
      *texel = GREEN;
    }
  }
  engine.step().unwrap();
  assert!(!engine.image_upload_pending(id), "made on the main thread");
  assert!(
    engine.frame().expect("a frame") == drawn_from_the_start(LARGE, GREEN),
    "the new pixels, whole"
  );
}

/// Loads `ship.png` and draws it in the UI, over a fixed rect
struct InTheUi {
  ship: Option<Handle<Image>>,
}

impl Scene for InTheUi {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.look().tonemap = Tonemap::Clamp;
    self.ship = Some(
      ctx
        .assets()
        .load_now::<Image>("ship.png")
        .expect("ship.png loads"),
    );
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    if let Some(ship) = &self.ship {
      ui.draw_sprite_rect(ship, Rect::new(10.0, 10.0, 300.0, 150.0), Color::WHITE);
    }
    SceneAction::Continue
  }
}

#[test]
fn a_large_reload_in_the_ui_draws_its_new_pixels_once_they_are_whole() {
  let files = MemorySource::new();
  files.insert("ship.png", png(LARGE.0, LARGE.1, RED));
  let Some(mut engine) = large_engine(files.clone(), InTheUi { ship: None }) else {
    return;
  };
  engine.set_upload_budget(16 * 1024);
  engine.step_frames(2).unwrap();
  let old = engine.frame().expect("a frame");
  assert_eq!(old.get_pixel(150, 80).0, RED);
  let id = image_id(&mut engine, "ship.png");
  let green = png(LARGE.0, LARGE.1, GREEN);
  reload_until_landed(&mut engine, &files, ("ship.png", id), green);
  assert!(engine.image_upload_pending(id), "it is written over frames");
  frames_drawing(&mut engine, id, &old);
  // The UI image kept while nothing changed is drawn again for the new
  // texture
  let files = MemorySource::new();
  files.insert("ship.png", png(LARGE.0, LARGE.1, GREEN));
  let mut new = large_engine(files, InTheUi { ship: None }).expect("a GPU");
  new.step_frames(2).unwrap();
  let new = new.frame().expect("a frame");
  for _ in 0..3 {
    assert!(engine.frame().expect("a frame") == new, "the new pixels");
    engine.step().unwrap();
  }
}

/// Loads a 64 px image on the workers and draws it at a 16th of its size,
/// filtered linearly, from (100, 100): its smallest levels
struct Small;

impl Scene for Small {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let dot = ctx.assets().load::<Image>("dot.png");
    let sprite = spawn_sprite(ctx, dot);
    if let Some(mut at) = ctx.world.get_mut::<Transform>(sprite) {
      *at = Transform::from_xy(100.0, 100.0).with_scale(Vec2::splat(1.0 / 16.0));
    }
    if let Some(mut renderable) = ctx.world.get_mut::<Renderable>(sprite) {
      renderable.filter = Filter::Linear;
    }
  }
}

#[test]
fn a_small_image_given_a_texture_of_its_own_keeps_every_level() {
  let files = MemorySource::new();
  files.insert("dot.png", png(64, 64, GREEN));
  let Some(mut engine) = large_engine(files, Small) else {
    return;
  };
  // As when every page is full: it gets a texture of its own, with more
  // levels than a page's four its loader made
  engine.refuse_atlas_pages();
  // Its handle, leaving it to the workers (`load_now` would load it here)
  let id = engine.assets().load::<Image>("dot.png").id();
  let start = std::time::Instant::now();
  while version(&engine, id).is_none() {
    assert!(
      start.elapsed() < std::time::Duration::from_secs(2),
      "loaded"
    );
    engine.step().unwrap();
  }
  engine.step_frames(2).unwrap();
  assert_eq!(
    pixel(&engine, 102, 102),
    GREEN,
    "drawn at a 16th, from levels below the page's four"
  );
}

/// Loads a 300 x 150 sheet of two 150 px cells as the bitmap font "Sheet",
/// and writes "A" in the UI with it
#[derive(Default)]
struct SheetUi {
  font: Option<Handle<BitmapFont>>,
}

impl Scene for SheetUi {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.look().tonemap = Tonemap::Clamp;
    let sheet = ctx
      .assets()
      .load_now::<Image>("ship.png")
      .expect("the sheet loads");
    self.font = Some(
      ctx
        .assets()
        .add(BitmapFont::new("Sheet", sheet, (150, 150), "AB")),
    );
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    let style = TextStyle::new().family("Sheet").size(150.0);
    ui.draw_text_styled("A", 10.0, 10.0, &style);
    SceneAction::Continue
  }
}

/// What an engine running `scene` over `width` x `height` px of `colour`
/// in `ship.png` shows from the start
fn scene_from_the_start(
  scene: impl Scene + 'static,
  (width, height): (u32, u32),
  colour: [u8; 4],
) -> image::RgbaImage {
  let files = MemorySource::new();
  files.insert("ship.png", png(width, height, colour));
  let mut engine = large_engine(files, scene).expect("a GPU, as the test's own engine had");
  engine.step_frames(2).unwrap();
  engine.frame().expect("a frame")
}

#[test]
fn a_large_reload_of_a_bitmap_font_sheet_redraws_a_kept_ui_once_it_is_whole() {
  let files = MemorySource::new();
  files.insert("ship.png", png(LARGE.0, LARGE.1, RED));
  let Some(mut engine) = large_engine(files.clone(), SheetUi::default()) else {
    return;
  };
  engine.set_upload_budget(16 * 1024);
  engine.step_frames(3).unwrap();
  let old = engine.frame().expect("a frame");
  let new = scene_from_the_start(SheetUi::default(), LARGE, GREEN);
  assert!(old != new, "the sheet's pixels show");
  let id = image_id(&mut engine, "ship.png");
  let green = png(LARGE.0, LARGE.1, GREEN);
  reload_until_landed(&mut engine, &files, ("ship.png", id), green);
  assert!(engine.image_upload_pending(id), "it is written over frames");
  frames_drawing(&mut engine, id, &old);
  for _ in 0..3 {
    assert!(
      engine.frame().expect("a frame") == new,
      "the text in the new sheet's pixels"
    );
    engine.step().unwrap();
  }
}

/// [`Large`], and an image over 256 px drawn below it whose pixels change
/// every frame, so every frame writes more than the tests' budget
struct Busy {
  large: Large,
  canvas: Option<Handle<Image>>,
}

impl Scene for Busy {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    self.large.on_enter(ctx);
    let canvas = ctx
      .assets()
      .add(Image::solid_rect(LARGE.0, LARGE.1, 0, 0, 200, 255));
    let sprite = spawn_sprite(ctx, canvas.clone());
    if let Some(mut at) = ctx.world.get_mut::<Transform>(sprite) {
      *at = Transform::from_xy(10.0, 170.0);
    }
    self.canvas = Some(canvas);
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    if let Some(canvas) = &self.canvas
      && let Some(image) = ctx.assets().get_mut(canvas)
    {
      image.data_mut()[0] ^= 1;
    }
    SceneAction::Continue
  }
}

#[test]
fn a_spread_upload_ends_while_other_writes_use_the_whole_budget() {
  let files = MemorySource::new();
  files.insert("ship.png", png(LARGE.0, LARGE.1, RED));
  let (large, _) = Large::new("ship.png");
  let scene = Busy {
    large,
    canvas: None,
  };
  let Some(mut engine) = large_engine(files.clone(), scene) else {
    return;
  };
  engine.set_upload_budget(16 * 1024);
  engine.step_frames(2).unwrap();
  let id = image_id(&mut engine, "ship.png");
  let green = png(LARGE.0, LARGE.1, GREEN);
  reload_until_landed(&mut engine, &files, ("ship.png", id), green);
  assert!(engine.image_upload_pending(id), "it is written over frames");
  let mut frames = 0;
  while engine.image_upload_pending(id) {
    assert!(
      frames < 1_500,
      "the canvas's writes leave the upload no room"
    );
    assert_eq!(pixel(&engine, 150, 80), RED, "the old pixels meanwhile");
    engine.step().unwrap();
    frames += 1;
  }
  assert_eq!(pixel(&engine, 150, 80), GREEN, "then the new");
  assert_eq!(pixel(&engine, 340, 20), GREEN, "every level of them");
}

/// Loads `a.png` and `b.png` and draws them at one pixel per texel, from
/// (10, 10) and (10, 170)
struct Pair;

impl Scene for Pair {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    for (path, y) in [("a.png", 10.0), ("b.png", 170.0)] {
      let image = ctx.assets().load_now::<Image>(path).expect("it loads");
      let sprite = spawn_sprite(ctx, image);
      if let Some(mut at) = ctx.world.get_mut::<Transform>(sprite) {
        *at = Transform::from_xy(10.0, y);
      }
    }
  }
}

#[test]
fn two_large_reloads_at_once_each_draw_their_old_pixels_then_their_new_whole() {
  let files = MemorySource::new();
  files.insert("a.png", png(LARGE.0, LARGE.1, RED));
  files.insert("b.png", png(LARGE.0, LARGE.1, BLUE));
  let Some(mut engine) = large_engine(files.clone(), Pair) else {
    return;
  };
  // A frame's budget writes a few rows: the first outlasts the second's
  // decode
  engine.set_upload_budget(4 * 1024);
  engine.step_frames(2).unwrap();
  let (a, b) = (
    image_id(&mut engine, "a.png"),
    image_id(&mut engine, "b.png"),
  );
  let before = (version(&engine, a), version(&engine, b));
  files.insert("a.png", png(LARGE.0, LARGE.1, GREEN));
  files.insert("b.png", png(LARGE.0, LARGE.1, GREEN));
  engine.assets().server().reload("a.png");
  engine.assets().server().reload("b.png");
  let start = std::time::Instant::now();
  while version(&engine, a) == before.0 || version(&engine, b) == before.1 {
    assert!(
      start.elapsed() < std::time::Duration::from_secs(2),
      "both land"
    );
    engine.step().unwrap();
  }
  let mut together = false;
  for frame in 0.. {
    assert!(frame < 3_000, "both uploads end");
    let drawn = engine.frame().expect("a frame");
    let pending = [
      (engine.image_upload_pending(a), RED, 10),
      (engine.image_upload_pending(b), BLUE, 170),
    ];
    together |= pending[0].0 && pending[1].0;
    for (pending, old, y) in pending {
      let want = if pending { old } else { GREEN };
      for (x, dy) in [(12, 2), (308, 2), (12, 148), (308, 148), (160, 75)] {
        assert_eq!(
          drawn.get_pixel(x, y + dy).0,
          want,
          "frame {frame}: the image at y {y}, whole, old or new"
        );
      }
    }
    if !pending[0].0 && !pending[1].0 {
      break;
    }
    engine.step().unwrap();
  }
  assert!(together, "the two were written over the same frames");
}

#[test]
fn a_large_reload_whose_event_frames_missed_starts_from_its_next_draw() {
  const OTHER: (u32, u32) = (260, 220);
  let files = MemorySource::new();
  files.insert("ship.png", png(LARGE.0, LARGE.1, RED));
  let (scene, _) = Large::new("ship.png");
  let Some(mut engine) = large_engine(files.clone(), scene) else {
    return;
  };
  engine.set_upload_budget(16 * 1024);
  engine.step_frames(2).unwrap();
  let old = engine.frame().expect("a frame");
  let id = image_id(&mut engine, "ship.png");
  // Minimised, frames run but draw nothing: the reload lands, and its
  // `Modified` is gone before a frame draws again
  engine.resize(0, 0);
  let green = png(OTHER.0, OTHER.1, GREEN);
  reload_until_landed(&mut engine, &files, ("ship.png", id), green);
  engine.step_frames(4).unwrap();
  assert!(
    !engine.image_upload_pending(id),
    "nothing drawn, nothing started"
  );
  engine.resize(480, 320);
  engine.step().unwrap();
  // The draw starts it: this one frame draws the old pixels at the new size
  assert!(
    engine.image_upload_pending(id),
    "written over frames from the draw"
  );
  assert_eq!(
    pixel(&engine, 50, 200),
    RED,
    "the old pixels, in the new height"
  );
  assert_ne!(pixel(&engine, 290, 50), RED, "and only the new width");
  engine.step().unwrap();
  let frames = frames_drawing(&mut engine, id, &old);
  assert!(
    frames > 2,
    "then at the old size, over {frames} more frames"
  );
  assert!(
    engine.frame().expect("a frame") == drawn_from_the_start(OTHER, GREEN),
    "then the new pixels at the new size"
  );
}
