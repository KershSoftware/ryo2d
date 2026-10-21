//! The dedicated UI renderer against the old UI path: two engines driven alike
//! draw the same pixels every frame

use std::sync::{Arc, Mutex};

use ryo2d::prelude::*;
use ryo2d::testing::{PixelPair, StepOutcome};

/// The old path (`a`) and the new renderer (`b`) on engines `make` builds
fn old_and_new(make: impl Fn() -> EngineBuilder) -> Option<PixelPair> {
  PixelPair::between(make, |b| b.ui_dedicated(false), |b| b.ui_dedicated(true))
}

fn lock<T>(shared: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
  shared
    .lock()
    .unwrap_or_else(|poisoned| poisoned.into_inner())
}

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

#[test]
fn a_hud_draws_the_same_on_the_new_renderer() {
  let Some(mut pair) = old_and_new(|| {
    EngineBuilder::new().resolution(320, 240).start_scene(Hud {
      counting: true,
      frame: 0,
    })
  }) else {
    return;
  };
  pair.frames(30, "a counting HUD");
}

#[test]
fn the_settings_screen_draws_the_same_in_one_or_two_draws() {
  let Some(mut pair) = old_and_new(settings) else {
    return;
  };
  // The first frame draws the whole screen (later ones keep its image)
  pair.frames(1, "the settings screen opening");
  let draws = pair.b().ui_draw_count();
  assert!((1..=2).contains(&draws), "{draws} draws");
  pair.frames(19, "the settings screen");
  // `b` is the new renderer
  assert!(
    pair.b().ui_draw_count() <= 2,
    "{} draws",
    pair.b().ui_draw_count()
  );
}

/// A panel and a label while the shared flag is set, nothing otherwise
struct Toggle(Arc<Mutex<bool>>);

impl Scene for Toggle {
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    if *lock(&self.0) {
      ui.draw_rect(
        Rect::new(10.0, 10.0, 120.0, 40.0),
        Color::new(40, 60, 90, 255),
      );
      ui.draw_text("Shown", 20.0, 20.0, Color::WHITE);
    }
    SceneAction::Continue
  }
}

#[test]
fn an_empty_ui_draws_nothing_and_the_next_draws_in_full() {
  let shown = Arc::new(Mutex::new(false));
  let make = {
    let shown = shown.clone();
    move || {
      EngineBuilder::new()
        .resolution(320, 240)
        .start_scene(Toggle(shown.clone()))
    }
  };
  let Some(mut pair) = old_and_new(make) else {
    return;
  };
  for round in 0..2 {
    *lock(&shown) = false;
    for frame in 0..5 {
      pair.frames(1, "hidden");
      let draws = pair.b().ui_draw_count();
      assert_eq!(draws, 0, "round {round}, hidden frame {frame}");
    }
    *lock(&shown) = true;
    pair.frames(1, "shown");
    let draws = pair.b().ui_draw_count();
    assert!(draws >= 1, "round {round}: the shown UI is drawn");
    pair.frames(4, "shown");
  }
}

/// 3,000 short labels, one of them different every frame
struct Labels {
  frame: u32,
}

/// Labels in a grid of this many columns
const COLUMNS: u32 = 60;
const LABELS: u32 = 3_000;

impl Scene for Labels {
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    self.frame += 1;
    let changed = self.frame % LABELS;
    let style = TextStyle::default().size(8.0).color(Color::WHITE);
    for i in 0..LABELS {
      let (x, y) = ((i % COLUMNS) as f32 * 10.5, (i / COLUMNS) as f32 * 9.5);
      let text = if i == changed { "##" } else { "ab" };
      ui.draw_text_styled(text, x, y, &style);
    }
    SceneAction::Continue
  }
}

#[test]
fn a_ui_bigger_than_the_buffer_grows_it() {
  let make = || {
    EngineBuilder::new()
      .resolution(640, 480)
      .start_scene(Labels { frame: 0 })
  };
  let Some(mut pair) = old_and_new(make) else {
    return;
  };
  pair.frames(6, "3,000 labels");
  assert!(pair.b().ui_draw_count() >= 1, "the labels are drawn");
}

/// A sprite of `width` x `height` with varied colour and alpha
fn gradient(width: u32, height: u32, name: &str) -> SpriteHandle {
  let mut data = Vec::with_capacity((width * height * 4) as usize);
  for y in 0..height {
    for x in 0..width {
      let a = if (x + y) % 3 == 0 { 128 } else { 255 };
      data.extend_from_slice(&[
        (x * 40 % 256) as u8,
        (y * 50 % 256) as u8,
        (255 - x * 15 % 256) as u8,
        a,
      ]);
    }
  }
  SpriteHandle::from_sprite(Sprite::from_rgba(data, width, height), name)
}

/// A scroll area of rows (an icon, a label, a checkbox and a bar each)
/// inside a clipped panel inside another, and a horizontal strip of buttons
/// scrolling under both clips
struct NestedScroll {
  icon: SpriteHandle,
  checks: Vec<bool>,
}

impl Scene for NestedScroll {
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    let outer = Rect::new(10.5, 12.25, 300.0, 210.0);
    ui.draw_rect(outer, Color::new(25, 25, 40, 255));
    ui.push_clip(outer);
    // The inner panel reaches past the outer one: its clip is where they meet
    let inner = Rect::new(30.0, 30.5, 320.0, 170.0);
    ui.draw_rounded_rect(inner, 6.0, Color::new(50, 40, 70, 240));
    ui.push_clip(inner);
    let icon = self.icon.clone();
    let checks = &mut self.checks;
    ui.scroll()
      .at(Rect::new(20.0, 40.0, 200.0, 190.0))
      .gap(4.0)
      .show(|ui| {
        for (i, checked) in checks.iter_mut().enumerate() {
          ui.row().gap(6.0).align(Align::Center).show(|ui| {
            ui.image(&icon).size(14.0, 14.0);
            ui.text(format!("Row {i}"));
            ui.checkbox(checked, "On").show();
            ui.progress(i as f32 / 30.0).width(60.0);
          });
        }
      });
    ui.scroll()
      .horizontal()
      .at(Rect::new(230.0, 150.0, 120.0, 70.0))
      .gap(4.0)
      .show(|ui| {
        for j in 0..12 {
          ui.button(format!("Card {j}"));
        }
      });
    ui.pop_clip();
    ui.pop_clip();
    SceneAction::Continue
  }
}

#[test]
fn scroll_clipping_and_nested_clips_match() {
  let icon = gradient(12, 12, "scroll-icon");
  let make = {
    let icon = icon.clone();
    move || {
      EngineBuilder::new()
        .resolution(400, 260)
        .start_scene(NestedScroll {
          icon: icon.clone(),
          checks: (0..30).map(|i| i % 3 != 1).collect(),
        })
    }
  };
  let Some(mut pair) = old_and_new(make) else {
    return;
  };
  pair.frames(5, "the nested clips opening");
  // The rows, wheeled down through their range, then a trackpad back up
  pair.each(|engine| engine.mouse_move(120.0, 100.0));
  pair.frames(2, "over the rows");
  for _ in 0..20 {
    pair.each(|engine| engine.wheel(0.0, -2.0));
    pair.frames(3, "wheeling the rows down");
  }
  for _ in 0..40 {
    pair.each(|engine| engine.trackpad(0.0, 23.5));
    pair.frames(1, "the trackpad moving the rows up");
  }
  // The strip, sideways both ways
  pair.each(|engine| engine.mouse_move(270.0, 170.0));
  pair.frames(2, "over the strip");
  for _ in 0..50 {
    pair.each(|engine| engine.trackpad(-17.25, 0.0));
    pair.frames(1, "the trackpad moving the strip");
  }
  for _ in 0..25 {
    pair.each(|engine| engine.trackpad(19.5, 0.0));
    pair.frames(1, "the trackpad moving the strip back");
  }
}

/// The most a line pixel's channel may differ between the paths, in the
/// owner's accepted class (see [`lines_match`]): 0.36 coverage, Task 4's
/// measured worst (the text demo page's zigzag, at its miter tips). Measured
/// on an Apple GPU (Metal), this scene's worst is 65/255 over its 24 frames,
/// and a sweep of probe lines over 64 sub-pixel shifts reached 77/255.
const LINE_AA_SLACK: u8 = 92;

/// Fewer line pixels than this in either frame means the lines did not draw
/// (about 6,100 do in every frame)
const LINES_DRAWN: usize = 5_000;

/// Lines `ui.draw_line` draws over a panel, moved by a sub-pixel shift each
/// frame: straight ones thin and thick at several slants, gentle arcs, closed
/// outlines and zigzags with long miters. Beside the panel a checked
/// checkbox, a dropdown and another the test opens: their checkmark and
/// chevrons.
struct Lines {
  frame: u32,
  checked: bool,
  first: usize,
  second: usize,
}

/// Where the lines are drawn, and what is under them
const LINE_PANEL: Rect = Rect {
  x: 10.0,
  y: 10.0,
  width: 340.0,
  height: 230.0,
};

/// The lines of frame `frame`
fn lines_at(frame: u32) -> Vec<(Vec<Vec2>, f32)> {
  let shift = Vec2::new(
    (frame % 4) as f32 * 0.25 + 0.0625,
    (frame / 4 % 4) as f32 * 0.25,
  );
  let at = |points: &[(f32, f32)]| -> Vec<Vec2> {
    points
      .iter()
      .map(|&(x, y)| Vec2::new(x, y) + shift)
      .collect()
  };
  let around = |centre: (f32, f32), radius: f32, from: f32, step: f32, n: usize| {
    let points: Vec<(f32, f32)> = (0..=n)
      .map(|i| {
        let a = (from + i as f32 * step).to_radians();
        (centre.0 + a.cos() * radius, centre.1 + a.sin() * radius)
      })
      .collect();
    at(&points)
  };
  let zigzag = |x: f32, y: f32, dx: f32, dy: f32, n: usize| {
    let points: Vec<(f32, f32)> = (0..=n)
      .map(|i| (x + i as f32 * dx, y + if i % 2 == 0 { dy } else { 0.0 }))
      .collect();
    at(&points)
  };
  vec![
    (at(&[(20.3, 20.3), (140.6, 27.4)]), 1.0),
    (at(&[(20.5, 34.2), (140.1, 52.7)]), 1.5),
    (at(&[(150.4, 20.2), (155.9, 90.7)]), 2.0),
    (at(&[(166.2, 21.1), (210.6, 88.3)]), 3.0),
    (at(&[(220.7, 24.8), (330.0, 40.5)]), 6.0),
    (around((80.0, 110.0), 40.0, 200.0, 10.0, 14), 2.0),
    (around((270.0, 110.0), 45.0, 200.0, 10.0, 14), 5.0),
    (
      at(&[
        (20.5, 140.5),
        (90.5, 140.5),
        (90.5, 175.5),
        (20.5, 175.5),
        (20.5, 140.5),
      ]),
      2.0,
    ),
    (around((130.0, 160.0), 20.3, 7.0, 60.0, 6), 3.0),
    (around((185.0, 162.0), 18.7, -90.0, 120.0, 3), 2.0),
    (zigzag(220.0, 140.0, 20.0, 36.0, 5), 4.0),
    (zigzag(20.0, 190.0, 40.0, 34.0, 7), 6.0),
    (zigzag(20.0, 190.0, 40.0, 34.0, 7), 2.0),
  ]
}

impl Scene for Lines {
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    ui.draw_rect(LINE_PANEL, Color::new(30, 32, 48, 255));
    for (points, width) in lines_at(self.frame) {
      ui.draw_line(&points, width, Color::new(240, 236, 250, 255));
    }
    self.frame += 1;
    ui.checkbox(&mut self.checked, "Checked")
      .at(Rect::new(362.0, 14.0, 110.0, 24.0))
      .show();
    ui.dropdown(&mut self.first, &["one", "two"])
      .at(Rect::new(362.0, 50.0, 110.0, 28.0))
      .show();
    ui.dropdown(&mut self.second, &["three", "four"])
      .at(Rect::new(362.0, 90.0, 110.0, 28.0))
      .show();
    SceneAction::Continue
  }
}

/// Step both engines once and compare their frames: identical, but that a
/// line pixel (in the line panel, drawn by a line in either frame) may
/// differ by up to [`LINE_AA_SLACK`]. Both frames must show the lines.
fn line_frame(pair: &mut PixelPair, what: &str) {
  for engine in [&mut pair.on, &mut pair.off] {
    assert!(
      matches!(engine.step(), Ok(StepOutcome::Running)),
      "{what}: a step failed"
    );
  }
  let (Ok(a), Ok(b)) = (pair.a().frame(), pair.b().frame()) else {
    panic!("{what}: no frame presented");
  };
  // The panel's corner, which no line reaches: what is under the lines
  let under = *a.get_pixel(LINE_PANEL.x as u32 + 1, LINE_PANEL.y as u32 + 1);
  let panel = |x: u32, y: u32| {
    let (x, y) = (x as f32 + 0.5, y as f32 + 0.5);
    x > LINE_PANEL.x && x < LINE_PANEL.right() && y > LINE_PANEL.y && y < LINE_PANEL.bottom()
  };
  let drawn = |frame: &image::RgbaImage| {
    frame
      .enumerate_pixels()
      .filter(|&(x, y, pixel)| panel(x, y) && *pixel != under)
      .count()
  };
  let (in_a, in_b) = (drawn(&a), drawn(&b));
  assert!(
    in_a >= LINES_DRAWN && in_b >= LINES_DRAWN,
    "{what}: the lines drew {in_a} pixels in a and {in_b} in b"
  );
  let mut wrong = Vec::new();
  for ((x, y, old), new) in a.enumerate_pixels().zip(b.pixels()) {
    if old == new {
      continue;
    }
    let slack = old
      .0
      .iter()
      .zip(new.0)
      .all(|(o, n)| o.abs_diff(n) <= LINE_AA_SLACK);
    let line = panel(x, y) && (*old != under || *new != under);
    if !(line && slack) {
      wrong.push(((x, y), old.0, new.0));
    }
  }
  if !wrong.is_empty() {
    let folder = std::env::temp_dir().join("ryo2d-ui-retain");
    let name: String = what
      .chars()
      .map(|c| if c.is_alphanumeric() { c } else { '-' })
      .collect();
    let _ = std::fs::create_dir_all(&folder);
    let _ = a.save(folder.join(format!("{name}-a.png")));
    let _ = b.save(folder.join(format!("{name}-b.png")));
    panic!(
      "{what}: {} pixels (a, b) differ beyond line anti-aliasing: {:?} (saved in {})",
      wrong.len(),
      &wrong[..wrong.len().min(8)],
      folder.display()
    );
  }
}

/// UI lines, old against new. On some GPUs (Apple, Metal) a line's pixels
/// differ between the paths, and the owner accepted that class on
/// 2026-10-20: the batch draws a line as an indexed mesh whose triangles
/// share vertices, such a GPU takes `fwidth` differently across them, so a
/// line's anti-aliased coverage can differ along it, most at sharp miter
/// tips. The UI renderer draws each triangle apart. The unit test
/// `lines_draw_as_the_batch_draws_them_with_no_vertex_shared` (ui_renderer's
/// gpu_tests.rs) proves everything else exact: drawn by the batch with no
/// vertex shared, every line is the new path's pixel for pixel. So here only
/// line pixels may differ, by no more than the class's measured worst
/// ([`LINE_AA_SLACK`]); every other pixel, the checkmark and the chevrons
/// included, must be the same.
#[test]
fn lines_match() {
  let make = || {
    EngineBuilder::new()
      .resolution(480, 260)
      .start_scene(Lines {
        frame: 0,
        checked: true,
        first: 0,
        second: 1,
      })
  };
  let Some(mut pair) = old_and_new(make) else {
    return;
  };
  for _ in 0..3 {
    line_frame(&mut pair, "the lines opening");
  }
  // The second dropdown opened (its chevron turns up) by a click
  pair.each(|engine| engine.mouse_move(417.0, 104.0));
  line_frame(&mut pair, "over the dropdown");
  pair.each(|engine| engine.press_mouse(MouseButton::Left));
  line_frame(&mut pair, "pressing");
  pair.each(|engine| engine.release_mouse(MouseButton::Left));
  for _ in 0..20 {
    line_frame(&mut pair, "the lines, the dropdown open");
  }
  // The open list's two options are the only focus targets
  let open = pair.a().ui_focusables().len();
  assert_eq!(open, 2, "the dropdown's list is open");
}

/// Where macOS keeps its colour emoji font: the only colour font a test can
/// reach (the engine's built-in fonts have no colour glyphs)
const COLOUR_FONT: &str = "/System/Library/Fonts/Apple Color Emoji.ttc";

/// Colour emoji (when `emoji` is a colour font), and outlined and shadowed
/// text in the UI font, which the test switches to MSDF
struct ColourAndMsdf {
  emoji: Option<Arc<[u8]>>,
  font: Option<FontId>,
}

impl Scene for ColourAndMsdf {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    if let Some(bytes) = self.emoji.clone() {
      self.font = ctx.fonts().add_face(bytes, 0).ok();
    }
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    if let Some(font) = self.font {
      for (i, size) in [20.0, 32.0, 47.5].into_iter().enumerate() {
        let style = TextStyle::new().font(TextFont::Id(font)).size(size);
        ui.draw_text_styled("😀🎮🌟", 8.3, 8.6 + i as f32 * 56.0, &style);
      }
      // Clipped through the middle
      ui.push_clip(Rect::new(200.0, 10.0, 41.3, 60.0));
      let style = TextStyle::new().font(TextFont::Id(font)).size(40.0);
      ui.draw_text_styled("🍉🚀", 180.4, 14.2, &style);
      ui.pop_clip();
    }
    let outlined = TextStyle::new()
      .size(30.0)
      .color(Color::new(250, 220, 120, 255))
      .outline(1.5, Color::BLACK);
    ui.draw_text_styled("Outlined MSDF", 8.4, 180.3, &outlined);
    let shadowed =
      TextStyle::new()
        .size(19.0)
        .shadow(Vec2::new(2.0, 2.0), Color::new(0, 0, 0, 180), 1.0);
    ui.draw_text_styled("A soft shadow, small", 8.0, 222.7, &shadowed);
    ui.draw_text_styled(
      "Huge",
      240.2,
      150.4,
      &TextStyle::new()
        .size(96.0)
        .outline(2.0, Color::new(40, 60, 200, 255)),
    );
    SceneAction::Continue
  }
}

#[test]
fn colour_glyphs_and_msdf_match() {
  let emoji: Option<Arc<[u8]>> = std::fs::read(COLOUR_FONT).ok().map(Into::into);
  if emoji.is_none() {
    eprintln!("no colour font at {COLOUR_FONT}: colour glyphs are not compared");
  }
  let make = {
    let emoji = emoji.clone();
    move || {
      EngineBuilder::new()
        .resolution(400, 260)
        .start_scene(ColourAndMsdf {
          emoji: emoji.clone(),
          font: None,
        })
    }
  };
  let Some(mut pair) = old_and_new(make) else {
    return;
  };
  pair.frames(5, "colour glyphs and raster text");
  if emoji.is_some() {
    // Colour glyphs drew: the emoji's corner has pixels far from grey
    let frame = pair.b().frame().expect("a frame");
    let coloured = frame
      .enumerate_pixels()
      .filter(|(x, y, _)| *x < 170 && *y < 170)
      .filter(|(_, _, p)| {
        let [r, g, b, _] = p.0;
        r.max(g).max(b) - r.min(g).min(b) > 80
      })
      .count();
    assert!(coloured > 200, "{coloured} coloured pixels");
  }
  pair.each(|engine| {
    let mut fonts = engine.world_mut().resource_mut::<Fonts>();
    let ui = fonts.role(FontRole::Ui).expect("a UI font");
    fonts.set_render(ui, FontRender::msdf());
  });
  pair.frames(10, "MSDF text beside colour glyphs");
}

/// A counter changing every frame, a label in a bitmap font (its cells are
/// sprites), and pixel-font text (glyphs sampled nearest) at fractional
/// places, one clipped and one magnified
struct BitmapAndPixelText {
  font: Option<FontId>,
  frame: u32,
}

impl Scene for BitmapAndPixelText {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    // Two 4x4 cells, "#" solid and "." a ring
    let mut pixels = vec![0u8; 8 * 4 * 4];
    for y in 0..4 {
      for x in 0..8 {
        let ring = x >= 4 && (x == 4 || x == 7 || y == 0 || y == 3);
        if x < 4 || ring {
          pixels[(y * 8 + x) * 4..][..4].copy_from_slice(&[255, 200, 120, 255]);
        }
      }
    }
    let sheet = SpriteHandle::from_sprite(Sprite::from_rgba(pixels, 8, 4), "renderer-grid");
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
      let style = TextStyle::new().font(TextFont::Id(font)).size(13.0);
      ui.draw_text_styled("..##..", 8.4, 40.7, &style);
    }
    for (i, size) in [16.0, 24.0, 35.0].into_iter().enumerate() {
      let style = TextStyle::new()
        .role(FontRole::Pixel)
        .size(size)
        .color(Color::new(180, 255, 200, 255))
        .outline(1.0, Color::BLACK);
      ui.draw_text_styled("PIXEL text 42", 8.3, 70.6 + i as f32 * 40.0, &style);
    }
    ui.push_clip(Rect::new(0.0, 190.0, 77.7, 40.0));
    let style = TextStyle::new().role(FontRole::Pixel).size(32.0);
    ui.draw_text_styled("Clipped", 10.25, 195.5, &style);
    ui.pop_clip();
    // Past the UI's 512 px glyph cap a pixel glyph is a smaller image scaled
    // up (here twice): sampled nearest
    let style = TextStyle::new()
      .role(FontRole::Pixel)
      .size(600.0)
      .color(Color::new(255, 150, 200, 200));
    ui.draw_text_styled("Px", 150.3, -120.6, &style);
    SceneAction::Continue
  }
}

#[test]
fn bitmap_font_text_matches() {
  let make = || {
    EngineBuilder::new()
      .resolution(320, 240)
      .start_scene(BitmapAndPixelText {
        font: None,
        frame: 0,
      })
  };
  let Some(mut pair) = old_and_new(make) else {
    return;
  };
  pair.frames(15, "bitmap and pixel-font text");
}

/// Waving, shaking and rainbow text, and a typewriter revealing a line
struct Effects {
  reveal: TextReveal,
}

impl Scene for Effects {
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    let style = TextStyle::default().size(24.0);
    ui.draw_text_styled(RichText::markup("[wave]Waving[/wave]"), 8.0, 8.0, &style);
    ui.draw_text_styled(
      RichText::markup("[shake=4]Shaking[/shake]"),
      160.0,
      8.0,
      &style,
    );
    ui.draw_text_styled(
      RichText::markup("A [rainbow=2]rainbow[/rainbow] word"),
      8.0,
      50.0,
      &style,
    );
    ui.text(RichText::markup(
      "Typed out,[pause=0.1] a letter at a time.",
    ))
    .at(8.0, 100.0)
    .wrap(280.0)
    .typewriter(&mut self.reveal)
    .draw();
    ui.draw_text("Still", 8.0, 200.0, Color::WHITE);
    SceneAction::Continue
  }
}

#[test]
fn text_effects_match() {
  let make = || {
    EngineBuilder::new()
      .resolution(320, 240)
      .start_scene(Effects {
        reveal: TextReveal::new(30.0).with_fade(0.1),
      })
  };
  let Some(mut pair) = old_and_new(make) else {
    return;
  };
  pair.frames(90, "text effects and a typewriter");
}

/// Atlas sprites sampled nearest and linear (magnified, minified, tinted,
/// a uv part of one, a nine-slice), and an image too large for the atlas,
/// some crossing a clip edge
struct Sprites {
  small: SpriteHandle,
  frame: SpriteHandle,
  large: SpriteHandle,
}

impl Scene for Sprites {
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    for (i, filter) in [Filter::Nearest, Filter::Linear].into_iter().enumerate() {
      let y = 8.3 + i as f32 * 70.0;
      ui.image(&self.small)
        .at(Rect::new(8.2, y, 61.7, 47.3))
        .filter(filter);
      ui.image(&self.small)
        .at(Rect::new(80.6, y, 5.3, 4.1))
        .filter(filter);
      ui.image(&self.small)
        .at(Rect::new(95.1, y, 40.0, 40.0))
        .tint(Color::new(255, 160, 90, 200))
        .uv([0.25, 0.1, 0.8, 0.7])
        .filter(filter);
      ui.image(&self.large)
        .at(Rect::new(150.4, y, 97.3, 60.6))
        .filter(filter);
    }
    ui.draw_nine_slice(
      &self.frame,
      Rect::new(8.5, 150.5, 120.0, 60.0),
      [4.0, 4.0, 4.0, 4.0],
      Color::WHITE,
    );
    // Clip edges through a magnified sprite and the large image
    ui.push_clip(Rect::new(140.3, 140.6, 100.0, 70.0));
    ui.draw_sprite_rect(
      &self.small,
      Rect::new(130.4, 129.7, 50.3, 49.6),
      Color::WHITE,
    );
    ui.draw_sprite_rect(
      &self.large,
      Rect::new(200.7, 150.2, 300.0, 280.0),
      Color::WHITE,
    );
    ui.pop_clip();
    SceneAction::Continue
  }
}

#[test]
fn sprites_and_a_standalone_image_match() {
  let (small, frame, large) = (
    gradient(17, 15, "renderer-small"),
    gradient(12, 12, "renderer-frame"),
    gradient(300, 280, "renderer-large"),
  );
  let make = {
    let sprites = (small.clone(), frame.clone(), large.clone());
    move || {
      let (small, frame, large) = sprites.clone();
      EngineBuilder::new()
        .resolution(320, 240)
        .start_scene(Sprites {
          small,
          frame,
          large,
        })
    }
  };
  let Some(mut pair) = old_and_new(make) else {
    return;
  };
  pair.frames(5, "atlas sprites and a standalone image");
}

/// Atlas sprites drawn outside the UI image first, so they are made resident
/// in order: 60 of 256 x 256 fill more than one 2048 px atlas page (at most
/// 56 of their 272 px bordered rects fit one) and, all one size, none fits
/// page 0 after the first that does not, so the first sits on page 0 and the
/// last on page 1
const FILLERS: usize = 60;

/// Glyphs of two glyph pages interleaved in the painter's order with sprites
/// of two atlas pages. The glyphs are a large circle at 400 and 440 px:
/// either image is wider and taller than half a glyph page (1024 px), so
/// each takes a page of its own.
struct Interleaved {
  fillers: Vec<SpriteHandle>,
  frame: u32,
}

impl Scene for Interleaved {
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    for filler in &self.fillers {
      ui.draw_sprite_rect(filler, Rect::new(-500.0, -500.0, 16.0, 16.0), Color::WHITE);
    }
    let (Some(first), Some(last)) = (self.fillers.first(), self.fillers.last()) else {
      return SceneAction::Continue;
    };
    let circle = |size: f32| {
      TextStyle::default()
        .size(size)
        .color(Color::new(120, 200, 255, 160))
    };
    let panel = Color::new(60, 40, 50, 200);
    // In the painter's order, each step's textures: glyph page (mask),
    // atlas page (sprite)
    ui.draw_rect(Rect::new(4.0, 4.0, 300.0, 200.0), panel);
    ui.draw_text_styled("◯", 0.0, -60.0, &circle(400.0)); // mask 0
    ui.draw_sprite_rect(first, Rect::new(40.0, 40.0, 160.0, 160.0), Color::WHITE); // sprite 0
    ui.draw_sprite_rect(last, Rect::new(160.0, 80.0, 160.0, 160.0), Color::WHITE); // sprite 1: break
    ui.draw_text_styled("◯", 300.0, -100.0, &circle(400.0)); // mask 0
    ui.draw_text_styled("◯", 500.0, 0.0, &circle(440.0)); // mask 1: break
    ui.draw_rect(Rect::new(520.0, 300.0, 200.0, 100.0), panel);
    ui.draw_sprite_rect(first, Rect::new(600.0, 200.0, 200.0, 200.0), Color::WHITE); // sprite 0
    ui.draw_text_styled("◯", 700.0, 100.0, &circle(440.0)); // mask 1
    ui.draw_sprite_rect(last, Rect::new(900.0, 300.0, 200.0, 200.0), Color::WHITE); // sprite 1: break
    // A rect changing every frame, so every frame draws
    self.frame += 1;
    let width = 20.0 + (self.frame % 50) as f32;
    ui.draw_rect(Rect::new(1000.0, 20.0, width, 30.0), panel);
    SceneAction::Continue
  }
}

/// The draws `Interleaved` takes: a run ends where an instance needs another
/// texture in a slot its run already fills. Runs: [rect, mask 0, sprite 0],
/// [sprite 1, mask 0], [mask 1, rect, sprite 0, mask 1], [sprite 1, rect]
const INTERLEAVED_RUNS: u32 = 4;

#[test]
fn interleaved_textures_break_runs_and_draw_right() {
  let fillers: Vec<SpriteHandle> = (0..FILLERS)
    .map(|i| {
      // A colour of its own, and a white diagonal
      let mut pixels = Vec::with_capacity(256 * 256 * 4);
      for y in 0..256 {
        for x in 0..256 {
          let colour = if x == y {
            [255; 4]
          } else {
            [(i * 4) as u8, 120, (255 - i * 4) as u8, 255]
          };
          pixels.extend_from_slice(&colour);
        }
      }
      SpriteHandle::from_sprite(
        Sprite::from_rgba(pixels, 256, 256),
        format!("renderer-filler-{i}"),
      )
    })
    .collect();
  let make = {
    let fillers = fillers.clone();
    move || {
      EngineBuilder::new()
        .resolution(1280, 720)
        .start_scene(Interleaved {
          fillers: fillers.clone(),
          frame: 0,
        })
    }
  };
  let Some(mut pair) = old_and_new(make) else {
    return;
  };
  for frame in 0..10 {
    pair.frames(1, "two glyph pages and two atlas pages interleaved");
    assert_eq!(
      pair.b().ui_draw_count(),
      INTERLEAVED_RUNS,
      "frame {frame}: the runs"
    );
  }
}

/// Rects, text, pixel-font text, a line and a sprite on the pixel-art
/// target, with a clip whose edges cross them, and a counter
struct PixelArtUi {
  sprite: SpriteHandle,
  frame: u32,
}

impl Scene for PixelArtUi {
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    self.frame += 1;
    let panel = Color::new(40, 60, 90, 230);
    let draw = |ui: &mut UiContext, sprite: &SpriteHandle, dx: f32| {
      ui.draw_rounded_rect(Rect::new(30.3 + dx, 20.6, 60.4, 30.2), 4.0, panel);
      ui.draw_rect_bordered(
        Rect::new(50.7 + dx, 60.2, 40.0, 25.5),
        Color::new(200, 80, 60, 255),
        3.0,
        1.5,
        Color::WHITE,
      );
      ui.draw_text("Pixel art", 35.6 + dx, 95.3, Color::WHITE);
      let style = TextStyle::new().role(FontRole::Pixel).size(16.0);
      ui.draw_text_styled("PIX 7", 34.2 + dx, 110.8, &style);
      ui.draw_line(
        &[Vec2::new(30.0 + dx, 140.0), Vec2::new(120.0 + dx, 140.0)],
        2.0,
        Color::new(255, 230, 120, 255),
      );
      ui.draw_sprite_rect(
        sprite,
        Rect::new(25.4 + dx, 150.7, 34.0, 30.0),
        Color::WHITE,
      );
      // Off the pixel grid: the snap rounds its place and size
      ui.draw_sprite_rect(
        sprite,
        Rect::new(62.6 + dx, 148.3, 30.7, 26.6),
        Color::new(255, 255, 255, 220),
      );
    };
    // Clipped (its edges at fractional places cross each kind), then
    // unclipped beside it
    ui.push_clip(Rect::new(45.3, 30.6, 50.2, 135.7));
    draw(ui, &self.sprite, 0.0);
    ui.pop_clip();
    draw(ui, &self.sprite, 150.0);
    ui.draw_text(&format!("{}", self.frame), 280.0, 4.0, Color::WHITE);
    SceneAction::Continue
  }
}

#[test]
fn pixel_art_ui_matches() {
  let sprite = gradient(17, 15, "renderer-pixel-art");
  let make = {
    let sprite = sprite.clone();
    move || {
      EngineBuilder::new()
        .pixel_art(320, 180)
        .start_scene(PixelArtUi {
          sprite: sprite.clone(),
          frame: 0,
        })
    }
  };
  let Some(mut pair) = old_and_new(make) else {
    return;
  };
  pair.frames(20, "a pixel-art UI");
  let frame = pair.b().frame().expect("a frame");
  assert_eq!((frame.width(), frame.height()), (320, 180));
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
    // Sizes up to 240, under the 256 px the world rasterises at (see 4g's
    // retention test: larger world text is scaled with linear filtering, so
    // atlases packed differently can differ by a level at glyph edges)
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
fn glyph_pressure_matches() {
  let make = || {
    EngineBuilder::new()
      .resolution(640, 480)
      .start_scene(UiChurn { frame: 0 })
  };
  let Some(mut pair) = old_and_new(make) else {
    return;
  };
  pair.frames(120, "UI text churning the glyph pages");
  assert!(
    pair.b().ui_render_stats().glyph_evictions > 0,
    "the run evicted glyph pages"
  );

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
  let Some(mut pair) = old_and_new(make) else {
    return;
  };
  pair.frames(120, "the UI still over churning world text");
  assert!(
    pair.b().ui_render_stats().glyph_evictions > 0,
    "the world text evicted glyph pages"
  );
  *lock(&label) = "After".to_string();
  pair.frames(10, "the UI label changed");
}

#[test]
fn resize_and_theme_change_match() {
  let Some(mut pair) = old_and_new(settings) else {
    return;
  };
  pair.frames(5, "the settings screen");
  pair.each(|engine| engine.resize(1000, 640));
  pair.frames(5, "resized");
  pair.each(|engine| {
    engine
      .world_mut()
      .resource_mut::<UiTheme>()
      .set(Theme::light())
  });
  pair.frames(5, "the light theme");
  pair.each(|engine| engine.resize(853, 480));
  pair.frames(5, "resized again, light");
  pair.each(|engine| engine.wheel(0.0, -3.0));
  pair.frames(10, "scrolled, light");
}

#[test]
fn retention_off_matches() {
  let make = || settings().ui_retain(false);
  let Some(mut pair) = old_and_new(make) else {
    return;
  };
  pair.frames(10, "the settings screen, retention off");
  let stats = pair.b().ui_render_stats();
  assert!(
    !stats.kept && stats.texts_reused == 0 && stats.texts_built > 0,
    "everything built every frame: {stats:?}"
  );
  assert!(
    pair.b().ui_draw_count() <= 2,
    "{} draws",
    pair.b().ui_draw_count()
  );
}
