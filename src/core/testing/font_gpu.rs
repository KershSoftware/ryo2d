//! Fonts on the GPU in a running engine: a freed font's glyphs leave the
//! glyph atlas, a glyph page it alone used is freed, and a reloaded font
//! draws its new glyphs, never the old ones.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use super::HeadlessEngine;
use crate::core::engine::EngineBuilder;
use crate::prelude::*;

const PRESS_START: &[u8] =
  include_bytes!("../../../examples/games/breakout/assets/fonts/PressStart2P-Regular.ttf");

/// `font` with its units per em set to `units`: the same glyphs and
/// character map, every outline drawn at another scale
fn with_units_per_em(font: &[u8], units: u16) -> Vec<u8> {
  let mut changed = font.to_vec();
  let tables = u16::from_be_bytes([font[4], font[5]]) as usize;
  for table in 0..tables {
    let record = 12 + 16 * table;
    if &font[record..record + 4] == b"head" {
      let offset = u32::from_be_bytes([
        font[record + 8],
        font[record + 9],
        font[record + 10],
        font[record + 11],
      ]) as usize;
      changed[offset + 18..offset + 20].copy_from_slice(&units.to_be_bytes());
      return changed;
    }
  }
  panic!("the font has no head table");
}

/// The units per em of `font`
fn units_per_em(font: &[u8]) -> u16 {
  let tables = u16::from_be_bytes([font[4], font[5]]) as usize;
  (0..tables)
    .map(|table| 12 + 16 * table)
    .find(|&record| &font[record..record + 4] == b"head")
    .map(|record| {
      let offset = u32::from_be_bytes([
        font[record + 8],
        font[record + 9],
        font[record + 10],
        font[record + 11],
      ]) as usize;
      u16::from_be_bytes([font[offset + 18], font[offset + 19]])
    })
    .expect("a head table")
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

/// Press Start 2P added in code (drawn as MSDF if `msdf`) and a label in
/// it, beside a label in the UI font, until `drop` is set: then the label
/// and the font go
struct TwoFonts {
  font: Option<Handle<Font>>,
  msdf: bool,
  drop: Arc<AtomicBool>,
  face: Arc<Mutex<Option<FontId>>>,
}

impl TwoFonts {
  fn new(msdf: bool) -> (Self, Arc<AtomicBool>, Arc<Mutex<Option<FontId>>>) {
    let drop = Arc::new(AtomicBool::new(false));
    let face = Arc::new(Mutex::new(None));
    let scene = TwoFonts {
      font: None,
      msdf,
      drop: drop.clone(),
      face: face.clone(),
    };
    (scene, drop, face)
  }
}

impl Scene for TwoFonts {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let font = ctx
      .assets()
      .add(Font::from_bytes(PRESS_START).expect("Press Start 2P is a font"));
    let mut fonts = ctx.fonts();
    let face = fonts.face_of(&font);
    if self.msdf
      && let Some(face) = face
    {
      fonts.set_render(face, FontRender::msdf());
    }
    *self.face.lock().unwrap() = face;
    self.font = Some(font);
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    if self.drop.load(Ordering::SeqCst) {
      self.font = None;
    }
    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    ui.draw_text("Inter", 10.0, 60.0, Color::WHITE);
    if let Some(font) = &self.font {
      let style = TextStyle::new()
        .font(TextFont::Face(font.clone()))
        .size(16.0);
      ui.draw_text_styled("Press", 10.0, 10.0, &style);
    }
    SceneAction::Continue
  }
}

/// The face the world's fonts give the UI role
fn ui_face(engine: &HeadlessEngine) -> FontId {
  engine
    .world()
    .resource::<Fonts>()
    .role(FontRole::Ui)
    .expect("a UI font")
}

#[test]
fn a_freed_fonts_glyphs_leave_the_atlas() {
  let (scene, drop, face) = TwoFonts::new(false);
  let Some(mut engine) = engine(MemorySource::new(), scene) else {
    return;
  };
  engine.step_frames(3).unwrap();
  let face = face.lock().unwrap().expect("registered when added");
  let ui = ui_face(&engine);
  let (pressed, inter) = (engine.glyph_count(face), engine.glyph_count(ui));
  assert!(pressed > 0 && inter > 0, "both drawn: {pressed}, {inter}");
  drop.store(true, Ordering::SeqCst);
  engine.step_frames(4).unwrap();
  assert_eq!(
    engine.world().resource::<Fonts>().family(face),
    None,
    "freed"
  );
  assert_eq!(engine.glyph_count(face), 0, "its glyphs are gone");
  assert_eq!(engine.glyph_count(ui), inter, "the UI font's stay");
}

#[test]
fn an_emptied_glyph_page_is_freed() {
  // Its MSDF glyphs are alone on their page; the UI font's share the mask page
  let (scene, drop, face) = TwoFonts::new(true);
  let Some(mut engine) = engine(MemorySource::new(), scene) else {
    return;
  };
  engine.step_frames(3).unwrap();
  let face = face.lock().unwrap().expect("registered when added");
  assert!(engine.glyph_count(face) > 0);
  assert_eq!(engine.glyph_page_count(), 2, "a mask page and an MSDF page");
  drop.store(true, Ordering::SeqCst);
  engine.step_frames(4).unwrap();
  assert_eq!(
    engine.glyph_page_count(),
    1,
    "the MSDF page went, its texture too"
  );
  assert!(
    engine.glyph_count(ui_face(&engine)) > 0,
    "the mask page stays"
  );
}

/// Switches to [`Enters`] from its first `ui`: the new scene enters, and the
/// UI pass runs again for it, within the frame
struct Opens {
  added: Arc<Mutex<Option<AssetId<Font>>>>,
}

impl Scene for Opens {
  fn ui(&mut self, _ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    SceneAction::Switch(Box::new(Enters {
      font: None,
      added: self.added.clone(),
    }))
  }
}

/// Adds Press Start 2P on entering, without asking for `ctx.fonts()`, and
/// draws a line in it
struct Enters {
  font: Option<Handle<Font>>,
  added: Arc<Mutex<Option<AssetId<Font>>>>,
}

impl Scene for Enters {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let font = ctx
      .assets()
      .add(Font::from_bytes(PRESS_START).expect("Press Start 2P is a font"));
    *self.added.lock().unwrap() = Some(font.id());
    self.font = Some(font);
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    if let Some(font) = &self.font {
      let style = TextStyle::new()
        .font(TextFont::Face(font.clone()))
        .size(16.0);
      ui.draw_text_styled("Press", 10.0, 10.0, &style);
    }
    SceneAction::Continue
  }
}

#[test]
fn a_font_a_scene_adds_on_entering_draws_in_its_first_frame() {
  let added = Arc::new(Mutex::new(None));
  let scene = Opens {
    added: added.clone(),
  };
  let Some(mut engine) = engine(MemorySource::new(), scene) else {
    return;
  };
  engine.step().unwrap();
  let id = added.lock().unwrap().expect("the next scene entered");
  let face = engine.world().resource::<Fonts>().face_of(id);
  let face = face.expect("registered for the UI pass that drew it");
  assert!(engine.glyph_count(face) > 0, "drawn in its own glyphs");
}

/// Loads `fonts/press.ttf` and draws a line in it
struct Pressed;

impl Scene for Pressed {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let font = ctx
      .assets()
      .load_now::<Font>("fonts/press.ttf")
      .expect("press.ttf loads");
    ctx.world.insert_resource(HeldFont(font));
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) -> SceneAction {
    if let Some(held) = ctx.world.get_resource::<HeldFont>() {
      let style = TextStyle::new()
        .font(TextFont::Face(held.0.clone()))
        .size(16.0);
      ui.draw_text_styled("AAAA AAAA", 8.0, 8.0, &style);
    }
    SceneAction::Continue
  }
}

#[derive(Resource)]
struct HeldFont(Handle<Font>);

#[test]
fn a_reloaded_font_draws_its_new_glyphs() {
  // The reloaded file has the same glyphs at half the scale: a glyph image
  // left from before the reload would draw at the old size
  let units = units_per_em(PRESS_START);
  let halved = with_units_per_em(PRESS_START, units * 2);
  let files = MemorySource::new();
  files.insert("fonts/press.ttf", PRESS_START);
  let Some(mut engine) = engine(files.clone(), Pressed) else {
    return;
  };
  engine.step_frames(3).unwrap();
  let before = engine.frame().unwrap();

  files.insert("fonts/press.ttf", halved.clone());
  engine.assets().server().reload("fonts/press.ttf");
  let held = engine.world().resource::<HeldFont>().0.id();
  let version = engine.world().resource::<Assets<Font>>().version(held);
  let start = std::time::Instant::now();
  while engine.world().resource::<Assets<Font>>().version(held) == version {
    assert!(
      start.elapsed() < std::time::Duration::from_secs(5),
      "never reloaded"
    );
    engine.step().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(2));
  }
  engine.step_frames(2).unwrap();
  let reloaded = engine.frame().unwrap();

  // What an engine that only ever had the new file draws
  let fresh = MemorySource::new();
  fresh.insert("fonts/press.ttf", halved);
  let Some(mut reference) = engine_for(fresh) else {
    return;
  };
  reference.step_frames(3).unwrap();
  let expected = reference.frame().unwrap();
  assert_ne!(before, expected, "the new file draws other pixels");
  assert!(
    reloaded == expected,
    "the reloaded font draws only its new glyphs"
  );
}

fn engine_for(files: MemorySource) -> Option<HeadlessEngine> {
  engine(files, Pressed)
}
