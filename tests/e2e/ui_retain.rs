//! UI render retention in the engine: with it on and off, two engines driven
//! alike draw the same pixels every frame; an unchanged UI keeps its image,
//! and a changing one rebuilds only its changed texts

use std::sync::{Arc, Mutex};

use ryo2d::prelude::*;
use ryo2d::testing::{HeadlessEngine, PixelPair};

use crate::common::ui_scenes::{Culled, Toggle, lock};

/// Ten panels with labels, and a frame counter when `counting`
struct Hud {
  counting: bool,
  frame: u32,
}

impl Scene for Hud {
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    for i in 0..10 {
      let y = 8.0 + i as f32 * 20.0;
      ui.draw_rect(
        Rect::new(4.0, y - 2.0, 150.0, 18.0),
        Color::new(20, 20, 30, 200),
      );
      ui.draw_text(&format!("Label {i}"), 8.0, y, Color::WHITE);
    }
    if self.counting {
      self.frame += 1;
      ui.draw_text(&format!("Frame {}", self.frame), 200.0, 8.0, Color::WHITE);
    }
    SceneAction::Continue
  }
}

fn hud(counting: bool) -> impl Fn() -> EngineBuilder {
  move || {
    EngineBuilder::new()
      .resolution(320, 240)
      .start_scene(Hud { counting, frame: 0 })
  }
}

#[test]
fn an_unchanged_ui_keeps_its_image() {
  let Some(mut pair) = PixelPair::new(hud(false)) else {
    return;
  };
  pair.frames(10, "the HUD opening");
  let before = pair.kept();
  pair.frames(10, "the HUD unchanged");
  assert_eq!(
    pair.kept() - before,
    10,
    "every unchanged frame kept the image"
  );
}

#[test]
fn a_changing_label_redraws_every_frame() {
  let Some(mut pair) = PixelPair::new(hud(true)) else {
    return;
  };
  pair.frames(5, "counting");
  for _ in 0..10 {
    pair.frames(1, "counting");
    assert!(!pair.on.ui_render_stats().kept);
  }
}

#[test]
fn a_resize_redraws_the_ui() {
  let Some(mut pair) = PixelPair::new(hud(false)) else {
    return;
  };
  pair.frames(10, "the HUD");
  assert!(pair.on.ui_render_stats().kept);
  pair.each(|engine| engine.resize(400, 300));
  pair.frames(1, "resized");
  assert!(!pair.on.ui_render_stats().kept, "a new UI image is drawn");
  pair.frames(10, "resized, unchanged");
  assert!(pair.on.ui_render_stats().kept);
}

#[test]
fn a_ui_drawing_nothing_keeps_its_image() {
  let make = || {
    EngineBuilder::new()
      .resolution(320, 240)
      .start_scene(Culled)
  };
  let Some(mut pair) = PixelPair::new(make) else {
    return;
  };
  pair.frames(1, "the culled UI's first frame");
  assert!(!pair.on.ui_render_stats().kept, "the first frame draws it");
  let before = pair.kept();
  pair.frames(10, "the culled UI unchanged");
  assert_eq!(
    pair.kept() - before,
    10,
    "every unchanged frame kept the image"
  );
}

#[test]
fn the_ui_hidden_then_shown_comes_back() {
  let shown = Arc::new(Mutex::new(true));
  let make = {
    let shown = shown.clone();
    move || {
      EngineBuilder::new()
        .resolution(320, 240)
        .start_scene(Toggle(shown.clone()))
    }
  };
  let Some(mut pair) = PixelPair::new(make) else {
    return;
  };
  pair.frames(10, "shown");
  *lock(&shown) = false;
  pair.frames(5, "hidden");
  *lock(&shown) = true;
  pair.frames(5, "shown again");
  assert!(pair.on.ui_render_stats().kept);
}

#[test]
fn a_disabled_ui_pass_hides_the_kept_ui() {
  let Some(mut pair) = PixelPair::new(hud(false)) else {
    return;
  };
  pair.frames(10, "the HUD");
  let set_ui_pass = |pair: &mut PixelPair, on: bool| {
    pair.each(|engine| {
      engine
        .world_mut()
        .resource_mut::<RenderPipeline>()
        .set_enabled(pass_ids::UI, on)
        .expect("the ui pass exists");
    });
  };
  set_ui_pass(&mut pair, false);
  let before = pair.kept();
  pair.frames(5, "the ui pass off");
  assert!(pair.kept() > before, "frames with the pass off kept the UI");
  set_ui_pass(&mut pair, true);
  let before = pair.kept();
  pair.frames(5, "the ui pass on again");
  assert!(pair.kept() > before, "frames with the pass on kept the UI");
}

#[test]
fn one_changed_label_builds_one_text() {
  let Some(mut pair) = PixelPair::new(hud(true)) else {
    return;
  };
  pair.frames(5, "counting");
  for frame in 0..10 {
    pair.frames(1, "counting");
    let on = pair.on.ui_render_stats();
    assert_eq!(
      (on.kept, on.texts_built, on.texts_reused),
      (false, 1, 10),
      "frame {frame}: {on:?}"
    );
    let off = pair.off.ui_render_stats();
    assert_eq!(
      (off.texts_built, off.texts_reused),
      (11, 0),
      "retention off builds every text"
    );
  }
}

/// A waving greeting and three still labels
struct Wavy;

impl Scene for Wavy {
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    ui.draw_text_styled(
      RichText::markup("[wave]Hello[/wave]"),
      8.0,
      8.0,
      &TextStyle::default().size(24.0),
    );
    for i in 0..3 {
      ui.draw_text(
        &format!("Still {i}"),
        8.0,
        60.0 + i as f32 * 20.0,
        Color::WHITE,
      );
    }
    SceneAction::Continue
  }
}

#[test]
fn an_effect_text_is_built_every_frame() {
  let make = || EngineBuilder::new().resolution(320, 240).start_scene(Wavy);
  let Some(mut pair) = PixelPair::new(make) else {
    return;
  };
  pair.frames(5, "waving");
  for frame in 0..10 {
    pair.frames(1, "waving");
    let on = pair.on.ui_render_stats();
    assert_eq!(
      (on.kept, on.texts_built, on.texts_reused),
      (false, 1, 3),
      "frame {frame}: {on:?}"
    );
  }
}

#[test]
fn a_new_ui_space_rebuilds_every_text() {
  let Some(mut pair) = PixelPair::new(hud(true)) else {
    return;
  };
  pair.frames(5, "counting");
  pair.each(|engine| engine.resize(400, 300));
  pair.frames(1, "resized");
  let on = pair.on.ui_render_stats();
  assert_eq!((on.texts_built, on.texts_reused), (11, 0), "{on:?}");
  pair.frames(1, "resized, counting");
  assert_eq!(pair.on.ui_render_stats().texts_reused, 10);
}

/// The 200-widget settings node screen
struct SettingsScreen;

impl Scene for SettingsScreen {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.spawn_ui(ryo2d::testing::fixtures::settings_tree());
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }
}

fn settings() -> EngineBuilder {
  EngineBuilder::new()
    .resolution(1280, 720)
    .start_scene(SettingsScreen)
}

/// The centre of the `n`-th widget the pad can reach
fn widget(pair: &PixelPair, n: usize) -> (f32, f32) {
  let targets = pair.on.ui_focusables();
  let (_, rect) = targets[n % targets.len()];
  (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0)
}

#[test]
fn hover_scroll_a_dropdown_and_a_theme_change_match() {
  let Some(mut pair) = PixelPair::new(settings) else {
    return;
  };
  pair.frames(10, "the screen opening");
  for n in [1, 4, 7] {
    let (x, y) = widget(&pair, n);
    pair.each(|engine| engine.mouse_move(x, y));
    pair.frames(3, "hovering");
  }
  // The dropdown (focus order: three tabs, a slider, a checkbox, then it)
  // opened by its trigger, then closed; before the scroll, which would
  // move it
  let (x, y) = widget(&pair, 5);
  let widgets = pair.on.ui_focusables().len();
  pair.each(|engine| engine.mouse_move(x, y));
  pair.frames(2, "over the dropdown");
  pair.each(|engine| engine.press_mouse(MouseButton::Left));
  pair.frames(2, "pressing");
  pair.each(|engine| engine.release_mouse(MouseButton::Left));
  pair.frames(5, "the dropdown open");
  // The open dropdown's options are the only focus targets
  let open = pair.on.ui_focusables().len();
  assert!(open < widgets, "the dropdown opened: {open} of {widgets}");
  pair.each(|engine| engine.tap_key(KeyCode::Escape));
  pair.frames(5, "the dropdown closed");
  assert_eq!(pair.on.ui_focusables().len(), widgets, "Escape closed it");
  pair.each(|engine| engine.wheel(0.0, -3.0));
  pair.frames(20, "scrolling down");
  pair.each(|engine| {
    engine
      .world_mut()
      .resource_mut::<UiTheme>()
      .set(Theme::light())
  });
  pair.frames(10, "the light theme");
  assert!(pair.kept() > 0, "an unchanged frame kept the image");
  assert!(pair.reused() > 0, "a changing frame reused texts");
}

/// Where the settings screen's focusable widgets are (UI units)
fn rows(pair: &PixelPair) -> Vec<Rect> {
  pair
    .on
    .ui_focusables()
    .into_iter()
    .map(|(_, rect)| rect)
    .collect()
}

/// Run one frame of each scroll in `steps` (applied by `scroll`), and sum
/// the texts the retaining engine built and reused in them
fn scrolled(
  pair: &mut PixelPair,
  steps: &[f32],
  scroll: fn(&mut HeadlessEngine, f32),
  what: &str,
) -> (u32, u32) {
  let (mut built, mut reused) = (0, 0);
  for &step in steps {
    pair.each(|engine| scroll(engine, step));
    pair.frames(1, what);
    let on = pair.on.ui_render_stats();
    assert!(!on.kept, "{what}: the list moves every frame");
    built += on.texts_built;
    reused += on.texts_reused;
  }
  (built, reused)
}

#[test]
fn a_scrolling_list_matches_and_reuses_its_moving_texts() {
  // The default UI scale, and one where a UI unit is 1.25 image pixels;
  // the pointer over the list's top rows (screen units)
  for (scale, pointer) in [(None, (520.0, 238.0)), (Some(1.25), (490.0, 207.5))] {
    let make = move || {
      EngineBuilder::new()
        .resolution(1280, 720)
        .ui_scale(scale)
        .start_scene(SettingsScreen)
    };
    let Some(mut pair) = PixelPair::new(make) else {
      return;
    };
    pair.each(|engine| engine.mouse_move(pointer.0, pointer.1));
    pair.frames(10, "the screen opening");
    let top = rows(&pair);
    // Wheel lines, eased: the list moves by fractions of a pixel
    let wheel = scrolled(
      &mut pair,
      &[-1.0; 12],
      |engine, dy| engine.wheel(0.0, dy),
      "wheel scrolling",
    );
    let wheeled = rows(&pair);
    assert_ne!(wheeled, top, "scale {scale:?}: the wheel scrolls the list");
    // A trackpad's fractional steps, both ways
    let steps = [-7.3, -2.6, 4.15, -11.9, 0.7, 3.35, -0.45, -5.05, 9.2, -1.3];
    let trackpad = scrolled(
      &mut pair,
      &steps,
      |engine, dy| engine.trackpad(0.0, dy),
      "trackpad scrolling",
    );
    assert_ne!(
      rows(&pair),
      wheeled,
      "scale {scale:?}: the trackpad scrolls the list"
    );
    // Texts moving with the list are reused; only rows coming into view
    // are built
    for ((built, reused), what) in [(wheel, "wheel"), (trackpad, "trackpad")] {
      assert!(
        reused > 8 * built,
        "scale {scale:?}, {what}: {built} texts built, {reused} reused"
      );
    }
  }
}

/// A prompt with a named icon, and a still label
struct Prompt;

impl Scene for Prompt {
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    ui.draw_text_styled(
      RichText::markup("Press [icon=confirm] to start"),
      8.0,
      8.0,
      &TextStyle::default().size(24.0),
    );
    ui.draw_text("Still", 8.0, 60.0, Color::WHITE);
    SceneAction::Continue
  }
}

#[test]
fn swapped_text_icons_show_at_once() {
  let swatch =
    |r, g, b, name| SpriteHandle::from_sprite(Sprite::solid_rect(16, 16, r, g, b, 255), name);
  let (red, blue) = (swatch(255, 0, 0, "red"), swatch(0, 0, 255, "blue"));
  let make = || {
    EngineBuilder::new()
      .resolution(320, 240)
      .start_scene(Prompt)
  };
  let Some(mut pair) = PixelPair::new(make) else {
    return;
  };
  pair.each(|engine| {
    engine
      .world_mut()
      .resource_mut::<TextIcons>()
      .insert("confirm", red.clone());
  });
  pair.frames(10, "the red icon");
  assert!(pair.on.ui_render_stats().kept);
  pair.each(|engine| {
    engine
      .world_mut()
      .resource_mut::<TextIcons>()
      .insert("confirm", blue.clone());
  });
  pair.frames(1, "the icon swapped");
  assert!(!pair.on.ui_render_stats().kept, "the swap redraws the UI");
  pair.frames(5, "the blue icon");
}

#[test]
fn a_font_render_change_matches() {
  // A still UI, so the change must be what redraws it
  let Some(mut pair) = PixelPair::new(hud(false)) else {
    return;
  };
  pair.frames(10, "raster text");
  let raster = pair.on.frame().expect("a frame");
  pair.each(|engine| {
    let mut fonts = engine.world_mut().resource_mut::<Fonts>();
    let ui = fonts.role(FontRole::Ui).expect("a UI font");
    fonts.set_render(ui, FontRender::msdf());
  });
  pair.frames(10, "MSDF text");
  let msdf = pair.on.frame().expect("a frame");
  assert_ne!(raster.as_raw(), msdf.as_raw(), "the render change shows");
}

/// Still labels, and one that alternates between two words every frame
struct Blink {
  frame: u32,
}

impl Scene for Blink {
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    self.frame += 1;
    for i in 0..5 {
      ui.draw_text(
        &format!("Still {i}"),
        8.0,
        8.0 + i as f32 * 20.0,
        Color::WHITE,
      );
    }
    let word = if self.frame.is_multiple_of(2) {
      "Tick"
    } else {
      "Tock"
    };
    ui.draw_text(word, 200.0, 8.0, Color::WHITE);
    SceneAction::Continue
  }
}

#[test]
fn a_ui_alternating_every_frame_matches() {
  let make = || {
    EngineBuilder::new()
      .resolution(320, 240)
      .start_scene(Blink { frame: 0 })
  };
  let Some(mut pair) = PixelPair::new(make) else {
    return;
  };
  pair.frames(30, "ticking");
  assert!(pair.reused() > 0);
}

#[test]
fn a_pixel_art_ui_matches() {
  let make = || {
    EngineBuilder::new().pixel_art(320, 180).start_scene(Hud {
      counting: true,
      frame: 0,
    })
  };
  let Some(mut pair) = PixelPair::new(make) else {
    return;
  };
  // Covers the pixel-art configuration through the pixel oracle
  pair.frames(20, "a pixel-art HUD");
  let frame = pair.on.frame().expect("a frame");
  assert_eq!(
    (frame.width(), frame.height()),
    (320, 180),
    "the pixel-art target"
  );
  assert!(pair.reused() > 0, "texts were reused");
}

/// Enough characters that cycling them at large sizes fills the glyph pages
const CHARS: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";

/// Eight characters of `CHARS` for frame `frame`, and a size cycling
/// through five large ones
fn churn(frame: u32) -> (String, f32) {
  let start = (frame as usize * 8) % CHARS.len();
  let text: String = CHARS.chars().cycle().skip(start).take(8).collect();
  (text, 150.0 + (frame / 8 % 5) as f32 * 50.0)
}

/// A still large title, and large text that changes every frame
struct UiChurn {
  frame: u32,
}

impl Scene for UiChurn {
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    self.frame += 1;
    ui.draw_text_styled("Title", 8.0, 8.0, &TextStyle::default().size(120.0));
    let (text, size) = churn(self.frame);
    ui.draw_text_styled(text, 8.0, 200.0, &TextStyle::default().size(size));
    SceneAction::Continue
  }
}

#[test]
fn glyph_pressure_from_the_ui_matches() {
  let make = || {
    EngineBuilder::new()
      .resolution(640, 480)
      .start_scene(UiChurn { frame: 0 })
  };
  let Some(mut pair) = PixelPair::new(make) else {
    return;
  };
  pair.frames(120, "churning");
  assert!(
    pair.on.ui_render_stats().glyph_evictions > 0,
    "the run evicted glyph pages"
  );
  assert!(pair.reused() > 0, "the title was reused");
}

/// A still UI whose label the test can change, over world text that changes
/// every frame
struct WorldChurn {
  label: Arc<Mutex<String>>,
  text: Option<Entity>,
  frame: u32,
}

impl Scene for WorldChurn {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.camera().set_position(0.0, 0.0);
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    self.frame += 1;
    if let Some(text) = self.text.take() {
      ctx.world.despawn(text);
    }
    // Sizes up to 240, under the 256 px the atlas rasterises at. World text
    // above 256 px is scaled from its image with linear filtering, so
    // engines whose atlases are packed differently can differ by one level
    // at glyph edges; retention changes the packing (two engines, both
    // retention off, one with extra glyphs drawn early, differ so too)
    let (text, _) = churn(self.frame);
    let size = 130.0 + (self.frame * 29 % 111) as f32;
    let spawned = ctx.world.spawn((
      Transform::from_xy(0.0, 0.0),
      Text2d::new(text).with_style(TextStyle::new().size(size)),
    ));
    self.text = Some(spawned.id());
    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    for i in 0..5 {
      ui.draw_text(
        &format!("Still {i}"),
        8.0,
        8.0 + i as f32 * 20.0,
        Color::WHITE,
      );
    }
    let label = lock(&self.label).clone();
    ui.draw_text_styled(label, 8.0, 120.0, &TextStyle::default().size(48.0));
    SceneAction::Continue
  }
}

#[test]
fn world_text_churn_under_a_kept_ui_matches() {
  let label = Arc::new(Mutex::new("Before".to_string()));
  let make = {
    let label = label.clone();
    move || {
      EngineBuilder::new()
        .resolution(640, 480)
        .start_scene(WorldChurn {
          label: label.clone(),
          text: None,
          frame: 0,
        })
    }
  };
  let Some(mut pair) = PixelPair::new(make) else {
    return;
  };
  pair.frames(120, "the UI still over churning world text");
  assert!(pair.kept() > 100, "the UI image was kept: {}", pair.kept());
  assert!(
    pair.on.ui_render_stats().glyph_evictions > 0,
    "the world text evicted glyph pages"
  );
  *lock(&label) = "After".to_string();
  pair.frames(10, "the UI label changed");
}

#[test]
fn only_a_new_ui_image_redraws() {
  let Some(mut pair) = PixelPair::new(hud(false)) else {
    return;
  };
  pair.frames(10, "the HUD");
  assert!(pair.on.ui_render_stats().kept);
  // Back at the same size, but the UI image was recreated, so the epoch
  // alone forces the redraw
  pair.each(|engine| {
    engine.resize(400, 300);
    engine.resize(320, 240);
  });
  pair.frames(1, "resized away and back");
  assert!(!pair.on.ui_render_stats().kept, "a new UI image is drawn");
  pair.frames(5, "unchanged again");
  assert!(pair.on.ui_render_stats().kept);
}

/// A counter changing every frame, and a label in a bitmap font
struct BitmapLabel {
  font: Option<FontId>,
  frame: u32,
}

impl Scene for BitmapLabel {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    // Two 4x4 cells, "#" solid and "." empty
    let mut pixels = vec![0u8; 8 * 4 * 4];
    for y in 0..4 {
      for x in 0..4 {
        pixels[(y * 8 + x) * 4..][..4].copy_from_slice(&[255, 255, 255, 255]);
      }
    }
    let sheet = SpriteHandle::from_sprite(Sprite::from_rgba(pixels, 8, 4), "retain-grid");
    self.font = ctx.fonts().add_bitmap("Grid", sheet, (4, 4), "#.").ok();
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    self.frame += 1;
    ui.draw_text(&format!("Frame {}", self.frame), 200.0, 8.0, Color::WHITE);
    if let Some(font) = self.font {
      let style = TextStyle::new().font(TextFont::Id(font)).size(16.0);
      ui.draw_text_styled("#.#.#", 8.0, 8.0, &style);
    }
    SceneAction::Continue
  }
}

#[test]
fn a_bitmap_font_label_is_built_every_frame() {
  let make = || {
    EngineBuilder::new()
      .resolution(320, 240)
      .start_scene(BitmapLabel {
        font: None,
        frame: 0,
      })
  };
  let Some(mut pair) = PixelPair::new(make) else {
    return;
  };
  pair.frames(5, "warming up");
  for frame in 0..10 {
    pair.frames(1, "counting beside a bitmap label");
    let on = pair.on.ui_render_stats();
    assert!(
      !on.kept && on.texts_built >= 2,
      "frame {frame}: the counter and the bitmap label are built: {on:?}"
    );
  }
}

/// A static closure UI with a line, a checked checkbox and a dropdown: the
/// widgets allocate their line points anew every frame
struct LineUi {
  checked: bool,
  choice: usize,
}

impl Scene for LineUi {
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    ui.draw_line(
      &[Vec2::new(10.0, 200.0), Vec2::new(150.0, 220.0)],
      2.0,
      Color::WHITE,
    );
    ui.checkbox(&mut self.checked, "Checked").show();
    ui.dropdown(&mut self.choice, &["one", "two"]).show();
    SceneAction::Continue
  }
}

#[test]
fn a_closure_ui_with_lines_keeps_its_image() {
  let make = || {
    EngineBuilder::new()
      .resolution(320, 240)
      .start_scene(LineUi {
        checked: true,
        choice: 0,
      })
  };
  let Some(mut pair) = PixelPair::new(make) else {
    return;
  };
  pair.frames(10, "the lines opening");
  let before = pair.kept();
  pair.frames(10, "the lines unchanged");
  assert_eq!(
    pair.kept() - before,
    10,
    "every unchanged frame kept the image"
  );
}
