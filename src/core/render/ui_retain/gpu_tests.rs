//! Retention with what only the engine can do: an image's pixels replaced
//! in its store, as a reload does, and world text evicting a glyph page a
//! kept fragment run samples

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};

use crate::core::engine::EngineBuilder;
use crate::core::graphics::Image;
use crate::core::testing::{HeadlessEngine, PixelPair};
use crate::prelude::*;

/// An image the test changes from outside, which each engine's scene keeps
/// in its own world: added on enter, and replaced at the scene's next
/// update once the test sets another (a reload, in both engines alike)
#[derive(Clone)]
struct Swatch(Arc<Mutex<(u32, Image)>>);

impl Swatch {
  fn new(image: Image) -> Self {
    Self(Arc::new(Mutex::new((0, image))))
  }

  /// The image each scene takes at its next update
  fn set(&self, image: Image) {
    let mut held = self.0.lock().unwrap();
    *held = (held.0 + 1, image);
  }

  /// The image as it is now, added to the scene's world, with its version
  fn add(&self, ctx: &mut SceneContext) -> (Handle<Image>, u32) {
    let (version, image) = self.0.lock().unwrap().clone();
    (ctx.assets().add(image), version)
  }

  /// `held` (an image `add` gave) brought in line with the image the test set
  fn sync(&self, ctx: &mut SceneContext, held: &mut (Handle<Image>, u32)) {
    let (version, image) = self.0.lock().unwrap().clone();
    if version != held.1 {
      *ctx.assets().get_mut(&held.0).unwrap() = image;
      held.1 = version;
    }
  }
}

/// A sprite and a label
struct SpriteUi {
  swatch: Swatch,
  held: Option<(Handle<Image>, u32)>,
}

impl SpriteUi {
  fn new(swatch: &Swatch) -> Self {
    Self {
      swatch: swatch.clone(),
      held: None,
    }
  }
}

impl Scene for SpriteUi {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    self.held = Some(self.swatch.add(ctx));
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    if let Some(held) = &mut self.held {
      self.swatch.sync(ctx, held);
    }
    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    if let Some((sprite, _)) = &self.held {
      ui.draw_sprite(sprite, 10.0, 10.0);
    }
    ui.draw_text("Sprite", 40.0, 10.0, Color::WHITE);
    SceneAction::Continue
  }
}

#[test]
fn a_sprite_updated_in_place_redraws() {
  let swatch = Swatch::new(Image::solid_rect(8, 8, 255, 0, 0, 255));
  let make = {
    let swatch = swatch.clone();
    move || {
      EngineBuilder::new()
        .resolution(160, 120)
        .start_scene(SpriteUi::new(&swatch))
    }
  };
  let Some(mut pair) = PixelPair::new(make) else {
    return;
  };
  pair.frames(10, "red");
  assert!(pair.on.ui_render_stats().kept);
  swatch.set(Image::solid_rect(8, 8, 0, 0, 255, 255));
  pair.frames(1, "blue, the same size");
  assert!(!pair.on.ui_render_stats().kept, "the update redraws the UI");
  pair.frames(5, "blue");
  swatch.set(Image::solid_rect(16, 16, 0, 255, 0, 255));
  pair.frames(5, "green and larger");
}

/// A label in a bitmap font, in a UI that otherwise never changes
struct BitmapUi {
  sheet: Swatch,
  held: Option<(Handle<Image>, u32)>,
  font: Option<FontId>,
}

impl Scene for BitmapUi {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let held = self.sheet.add(ctx);
    self.font = ctx
      .fonts()
      .add_bitmap("Grid", held.0.clone(), (4, 4), "#.")
      .ok();
    self.held = Some(held);
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    if let Some(held) = &mut self.held {
      self.sheet.sync(ctx, held);
    }
    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    if let Some(font) = self.font {
      let style = TextStyle::new().font(TextFont::Id(font)).size(16.0);
      ui.draw_text_styled("#.#.#", 8.0, 8.0, &style);
    }
    SceneAction::Continue
  }
}

#[test]
fn a_bitmap_font_sheet_updated_in_place_redraws() {
  // Two 4x4 cells: "#" solid, "." empty
  let sheet_of = |rgba: [u8; 4]| {
    let mut pixels = vec![0u8; 8 * 4 * 4];
    for y in 0..4 {
      for x in 0..4 {
        pixels[(y * 8 + x) * 4..][..4].copy_from_slice(&rgba);
      }
    }
    Image::from_rgba(pixels, 8, 4)
  };
  let sheet = Swatch::new(sheet_of([255, 0, 0, 255]));
  let make = {
    let sheet = sheet.clone();
    move || {
      EngineBuilder::new()
        .resolution(160, 120)
        .start_scene(BitmapUi {
          sheet: sheet.clone(),
          held: None,
          font: None,
        })
    }
  };
  let Some(mut pair) = PixelPair::new(make) else {
    return;
  };
  pair.frames(10, "red cells");
  assert!(pair.on.ui_render_stats().kept);
  sheet.set(sheet_of([0, 0, 255, 255]));
  pair.frames(1, "blue cells, the same size");
  assert!(!pair.on.ui_render_stats().kept, "the update redraws the UI");
  pair.frames(5, "blue cells");
}

/// The settings screen as a node tree
struct NodeUi;

impl Scene for NodeUi {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.spawn_ui(crate::core::testing::fixtures::settings_tree());
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }
}

#[test]
fn the_rebuilds_overlay_keeps_an_idle_ui_image() {
  let builder = EngineBuilder::new()
    .resolution(640, 480)
    .start_scene(NodeUi);
  let Some(mut engine) = HeadlessEngine::new_or_skip(builder) else {
    return;
  };
  engine.debug_config_mut().show_ui_rebuilds = true;
  let _ = engine.step_frames(120);
  let mut kept = 0;
  for _ in 0..120 {
    let _ = engine.step();
    kept += u32::from(engine.ui_render_stats().kept);
  }
  assert!(
    kept >= 110,
    "the overlay kept the UI image in {kept} of 120 frames"
  );
}

#[test]
fn an_idle_scrolled_node_list_keeps_its_ui_image() {
  // Debug builds run the spot check: each frame it builds a clean node, a
  // row of the list or a part of one, in place of its replay. Built, it is
  // kept as the fragment its replay would be, culled by the list's
  // interest as every kept row is, so the frame's commands stay the same.
  let builder = EngineBuilder::new()
    .resolution(640, 480)
    .start_scene(NodeUi);
  let Some(mut engine) = HeadlessEngine::new_or_skip(builder) else {
    return;
  };
  let _ = engine.step_frames(3);
  // Scrolled by a fractional trackpad step, then the pointer off the panel
  engine.mouse_move(320.0, 300.0);
  let _ = engine.step();
  engine.trackpad(0.0, -137.3);
  let _ = engine.step_frames(10);
  engine.mouse_move(636.0, 476.0);
  let _ = engine.step_frames(30);
  assert!(
    !engine
      .ui_snapshot()
      .contains("ScrollState { offset: Vec2(0.0, 0.0)"),
    "the list scrolled"
  );
  let (mut kept, mut checked) = (0, 0);
  for _ in 0..120 {
    let _ = engine.step();
    kept += u32::from(engine.ui_render_stats().kept);
    checked += u32::from(engine.ui_cache_stats().spot_checked.is_some());
  }
  if cfg!(debug_assertions) {
    assert!(checked >= 100, "the spot check ran in {checked} frames");
  }
  assert_eq!(kept, 120, "the UI image was kept in {kept} of 120 frames");
}

/// A node tree drawing a sprite beside a label
struct NodeSpriteUi(SpriteUi);

impl Scene for NodeSpriteUi {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let held = self.0.swatch.add(ctx);
    ctx.spawn_ui(
      UiNode::column()
        .child(
          UiNode::row()
            .child(UiNode::image(held.0.clone()).image_size(16.0, 16.0))
            .child(UiNode::text("Sprite")),
        )
        .child(UiNode::text("Below")),
    );
    self.0.held = Some(held);
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    self.0.update(ctx, dt)
  }
}

#[test]
fn a_sprite_in_a_kept_fragment_updated_in_place_redraws() {
  // The sprite's fragment is copied with its version: its run is the same
  // but for the sprite's stamp
  let sprite = Swatch::new(Image::solid_rect(8, 8, 255, 0, 0, 255));
  let make = {
    let sprite = sprite.clone();
    move || {
      EngineBuilder::new()
        .resolution(160, 120)
        .start_scene(NodeSpriteUi(SpriteUi::new(&sprite)))
    }
  };
  let Some(mut pair) = PixelPair::new(make) else {
    return;
  };
  pair.each(|engine| engine.set_ui_spot_check(false));
  pair.frames(10, "red");
  assert!(pair.on.ui_render_stats().kept);
  assert!(pair.on.ui_cache_stats().reused > 0, "the tree is copied");
  let runs = pair.on.ui_runs_snapshot();
  sprite.set(Image::solid_rect(8, 8, 0, 0, 255, 255));
  pair.frames(1, "blue, the same size");
  assert!(!pair.on.ui_render_stats().kept, "the update redraws the UI");
  assert_eq!(pair.on.ui_runs_snapshot(), runs, "the runs are the same");
  pair.frames(5, "blue");
  assert!(pair.on.ui_render_stats().kept);
}

#[test]
fn a_sprite_updated_in_place_under_a_kept_fragment_draws_as_a_rebuild() {
  // Fragments on against a full rebuild, the spot check off so the tree is
  // copied: the sprite is replaced at its size, then at another
  let sprite = Swatch::new(Image::solid_rect(8, 8, 255, 0, 0, 255));
  let make = {
    let sprite = sprite.clone();
    move || {
      EngineBuilder::new()
        .resolution(160, 120)
        .start_scene(NodeSpriteUi(SpriteUi::new(&sprite)))
    }
  };
  let Some(mut pair) = PixelPair::fragments(make) else {
    return;
  };
  pair.each(|engine| engine.set_ui_spot_check(false));
  pair.frames(10, "red");
  let copied = pair.fragments_reused();
  assert!(copied > 0, "the tree is copied");
  let red = pair.on.frame().expect("a frame");
  sprite.set(Image::solid_rect(8, 8, 0, 0, 255, 255));
  pair.frames(5, "blue, the same size");
  let blue = pair.on.frame().expect("a frame");
  assert_ne!(red.as_raw(), blue.as_raw(), "the update shows");
  sprite.set(Image::solid_rect(16, 16, 0, 255, 0, 255));
  pair.frames(5, "green and larger");
  let green = pair.on.frame().expect("a frame");
  assert_ne!(blue.as_raw(), green.as_raw(), "the second update shows");
  assert!(
    pair.fragments_reused() > copied,
    "the tree is copied after the updates"
  );
}

/// Characters cycled through at large sizes to fill the glyph pages
const CHARS: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";

/// A node tree's labels, a swatch whose shade the test sets and, while
/// `churn` is set, large world text that changes every frame
struct Pressure {
  shade: Arc<AtomicU8>,
  churn: Arc<AtomicBool>,
  text: Option<Entity>,
  frame: u32,
}

impl Scene for Pressure {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.camera().set_position(0.0, 0.0);
    ctx.spawn_ui(
      UiNode::column()
        .child(UiNode::text("A kept label"))
        .child(UiNode::text("And another one")),
    );
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    if let Some(text) = self.text.take() {
      ctx.world.despawn(text);
    }
    if self.churn.load(Ordering::Relaxed) {
      self.frame += 1;
      // Sizes up to 240, under the 256 px the atlas rasterises at
      let start = (self.frame as usize * 8) % CHARS.len();
      let text: String = CHARS.chars().cycle().skip(start).take(8).collect();
      let size = 130.0 + (self.frame * 29 % 111) as f32;
      let spawned = ctx.world.spawn((
        Transform::from_xy(0.0, 0.0),
        Text2d::new(text).with_style(TextStyle::new().size(size)),
      ));
      self.text = Some(spawned.id());
    }
    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    let shade = self.shade.load(Ordering::Relaxed);
    ui.draw_rect(
      Rect::new(300.0, 8.0, 20.0, 20.0),
      Color::new(shade, 40, 40, 255),
    );
    SceneAction::Continue
  }
}

#[test]
fn an_evicted_glyph_page_drops_a_fragments_instances() {
  let shade = Arc::new(AtomicU8::new(100));
  let churn = Arc::new(AtomicBool::new(false));
  let make = {
    let (shade, churn) = (shade.clone(), churn.clone());
    move || {
      EngineBuilder::new()
        .resolution(640, 480)
        .start_scene(Pressure {
          shade: shade.clone(),
          churn: churn.clone(),
          text: None,
          frame: 0,
        })
    }
  };
  let Some(mut pair) = PixelPair::fragments(make) else {
    return;
  };
  pair.each(|engine| engine.set_ui_spot_check(false));
  pair.frames(10, "the tree");
  assert!(pair.on.ui_render_stats().kept);
  // The swatch changes: the UI is built, the tree's run copied
  shade.store(120, Ordering::Relaxed);
  pair.frames(1, "the swatch changed");
  let stats = pair.on.ui_render_stats();
  assert_eq!(
    (stats.runs_copied, stats.runs_built),
    (1, 0),
    "the tree's run is copied"
  );
  let pages = pair
    .on
    .ui_renderer()
    .expect("a UI renderer")
    .kept_run_pages();
  assert!(!pages.is_empty(), "the tree's labels sit on a glyph page");
  // World text churns under the kept UI image until it evicts a page the
  // run's instances sample
  churn.store(true, Ordering::Relaxed);
  let evicted = (0..600).any(|_| {
    pair.frames(1, "world text churning");
    assert!(pair.on.ui_render_stats().kept, "the UI image is kept");
    pages
      .iter()
      .any(|&(page, generation)| pair.on.glyph_page_generation(page) != Some(generation))
  });
  assert!(evicted, "the world text evicted the run's glyph page");
  churn.store(false, Ordering::Relaxed);
  // The next frame that builds the UI builds the run again, its labels'
  // glyphs anew: as a full rebuild draws them
  shade.store(140, Ordering::Relaxed);
  pair.frames(1, "the swatch changed after the eviction");
  let stats = pair.on.ui_render_stats();
  assert_eq!(
    (stats.runs_copied, stats.runs_built),
    (0, 1),
    "the tree's run is built again"
  );
  shade.store(160, Ordering::Relaxed);
  pair.frames(1, "the swatch changed again");
  let stats = pair.on.ui_render_stats();
  assert_eq!((stats.runs_copied, stats.runs_built), (1, 0), "and copied");
  pair.frames(3, "after");
}

/// The settings screen as a node tree, and a swatch whose shade changes
/// every frame
struct NodeSwatch {
  frame: u8,
}

impl Scene for NodeSwatch {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.spawn_ui(crate::core::testing::fixtures::settings_tree());
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    self.frame = self.frame.wrapping_add(1);
    ui.draw_rect(
      Rect::new(600.0, 4.0, 20.0, 20.0),
      Color::new(self.frame, 40, 40, 255),
    );
    SceneAction::Continue
  }
}

#[test]
fn a_kept_trees_runs_are_copied_while_a_loose_draw_changes() {
  let make = || {
    EngineBuilder::new()
      .resolution(640, 480)
      .start_scene(NodeSwatch { frame: 0 })
  };
  let Some(mut pair) = PixelPair::fragments(make) else {
    return;
  };
  pair.each(|engine| engine.set_ui_spot_check(false));
  pair.frames(10, "the settings screen");
  for frame in 0..20 {
    pair.frames(1, "the swatch changing");
    let stats = pair.on.ui_render_stats();
    assert!(!stats.kept, "frame {frame}: the UI changed");
    assert!(stats.runs_copied > 0, "frame {frame}: {stats:?}");
    assert_eq!(stats.runs_built, 0, "frame {frame}: {stats:?}");
  }
}

/// The settings screen as a node tree: a list filling its panel, which the
/// wheel scrolls
struct NodeSettings;

impl Scene for NodeSettings {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.spawn_ui(crate::core::testing::fixtures::settings_tree());
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }
}

/// Over a row of the settings screen's list, on a screen `width` by
/// `height` (its panel is centred)
fn over_list(width: f32, height: f32) -> (f32, f32) {
  (width / 2.0 - 120.0, height / 2.0 - 122.0)
}

/// The texts `engine` drew in each of `frames` frames of the wheel turning
/// down over the settings screen's list, one line a frame: those built,
/// and those drawn from kept glyphs
fn wheeled_texts(engine: &mut HeadlessEngine, frames: usize) -> Vec<(u32, u32)> {
  let _ = engine.step_frames(10);
  (0..frames)
    .map(|_| {
      engine.wheel(0.0, -1.0);
      let _ = engine.step();
      let stats = engine.ui_render_stats();
      (stats.texts_built, stats.texts_reused)
    })
    .collect()
}

#[test]
fn with_retention_off_a_scrolling_list_builds_the_texts_a_rebuild_builds() {
  // Retention off, every text drawn is built. The list's kept rows reach
  // the renderer culled by its interest, three views tall; nothing is kept
  // of them, so they are culled as resolve culls the draws a full rebuild
  // sends (the node cache off), and only those are built
  let make = |fragments: bool| {
    EngineBuilder::new()
      .resolution(1280, 720)
      .ui_retain(false)
      .ui_cache(fragments)
      .ui_fragments(fragments)
      .start_scene(NodeSettings)
  };
  let (Some(mut on), Some(mut off)) = (
    HeadlessEngine::new_or_skip(make(true)),
    HeadlessEngine::new_or_skip(make(false)),
  ) else {
    return;
  };
  let (x, y) = over_list(1280.0, 720.0);
  let mut texts = Vec::new();
  for engine in [&mut on, &mut off] {
    engine.mouse_move(x, y);
    texts.push(wheeled_texts(engine, 30));
  }
  assert!(texts[0].iter().all(|&(_, reused)| reused == 0));
  assert_eq!(texts[0], texts[1], "with fragments, then a full rebuild");
}

#[test]
fn on_the_pixel_art_target_a_scrolling_list_draws_the_texts_a_rebuild_draws() {
  // On the pixel-art target a scroll slot's contents are built absolute
  // every frame and never kept, so they are culled as resolve culls the
  // draws a full rebuild sends
  let make = || {
    EngineBuilder::new()
      .pixel_art(640, 480)
      .start_scene(NodeSettings)
  };
  let Some(mut pair) = PixelPair::fragments(make) else {
    return;
  };
  let (x, y) = over_list(640.0, 480.0);
  pair.each(|engine| engine.mouse_move(x, y));
  pair.frames(10, "the settings screen");
  for frame in 0..30 {
    pair.each(|engine| engine.wheel(0.0, -1.0));
    pair.frames(1, "the wheel turning");
    let drawn = |engine: &HeadlessEngine| {
      let stats = engine.ui_render_stats();
      stats.texts_built + stats.texts_reused
    };
    assert_eq!(
      drawn(&pair.on),
      drawn(&pair.off),
      "frame {frame}: the texts drawn with fragments, then by a full rebuild"
    );
  }
  assert_eq!(pair.slot_runs_copied(), 0, "nothing is copied into a slot");
}

#[test]
fn a_scrolling_lists_interest_steps_build_only_the_rows_they_bring() {
  // The wheel turning the settings screen's list down a line a frame under
  // the pointer: its interest moves every half view (about 200 units, a
  // few lines). The rows that stay inside it push what they pushed, and
  // are copied; only the rows it brings (with those the pointer moves
  // over) are built
  let builder = EngineBuilder::new()
    .resolution(1280, 720)
    .start_scene(NodeSettings);
  let Some(mut engine) = HeadlessEngine::new_or_skip(builder) else {
    return;
  };
  engine.set_ui_spot_check(false);
  let (x, y) = over_list(1280.0, 720.0);
  engine.mouse_move(x, y);
  let _ = engine.step_frames(10);
  let mut most = 0;
  for frame in 0..40 {
    engine.wheel(0.0, -1.0);
    let _ = engine.step();
    let stats = engine.ui_render_stats();
    assert!(stats.runs_copied > 20, "frame {frame}: {stats:?}");
    most = most.max(stats.runs_built);
  }
  assert!(most <= 20, "{most} runs built in a frame");
}
