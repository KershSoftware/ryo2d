//! The UI renderer in the engine: how many draws a UI takes, an empty UI,
//! one larger than the instance buffer, and a UI drawing nothing

use std::sync::{Arc, Mutex};

use ryo2d::prelude::*;
use ryo2d::testing::{HeadlessEngine, StepOutcome};

fn lock<T>(shared: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
  shared
    .lock()
    .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Run one frame of `engine`
fn frame(engine: &mut HeadlessEngine, what: &str) {
  match engine.step() {
    Ok(StepOutcome::Running) => {}
    other => panic!("{what}: {other:?}"),
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
fn the_settings_screen_draws_in_one_or_two_draws() {
  let Some(mut engine) = HeadlessEngine::new_or_skip(settings()) else {
    return;
  };
  // The first frame draws the whole screen (later ones keep its image)
  frame(&mut engine, "the settings screen opening");
  let draws = engine.ui_draw_count();
  assert!((1..=2).contains(&draws), "{draws} draws");
  for _ in 0..19 {
    frame(&mut engine, "the settings screen");
    let draws = engine.ui_draw_count();
    assert!(draws <= 2, "{draws} draws");
  }
}

#[test]
fn retention_off_draws_the_settings_screen_every_frame() {
  let make = settings().ui_retain(false);
  let Some(mut engine) = HeadlessEngine::new_or_skip(make) else {
    return;
  };
  for n in 0..10 {
    frame(&mut engine, "the settings screen, retention off");
    let stats = engine.ui_render_stats();
    assert!(
      !stats.kept && stats.texts_reused == 0 && stats.texts_built > 0,
      "frame {n}: everything built every frame: {stats:?}"
    );
    let draws = engine.ui_draw_count();
    assert!((1..=2).contains(&draws), "frame {n}: {draws} draws");
  }
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
  let make = EngineBuilder::new()
    .resolution(320, 240)
    .start_scene(Toggle(shown.clone()));
  let Some(mut engine) = HeadlessEngine::new_or_skip(make) else {
    return;
  };
  // Inside the panel, left of its label
  let panel = |engine: &HeadlessEngine| {
    let image = engine.frame().expect("a frame");
    let [r, g, b, _] = image.get_pixel(14, 44).0;
    [r, g, b]
  };
  let near = |got: [u8; 3], want: [u8; 3]| got.iter().zip(want).all(|(g, w)| g.abs_diff(w) <= 1);
  for round in 0..2 {
    *lock(&shown) = false;
    for n in 0..5 {
      frame(&mut engine, "hidden");
      let draws = engine.ui_draw_count();
      assert_eq!(draws, 0, "round {round}, hidden frame {n}");
      let under = panel(&engine);
      assert!(!near(under, [40, 60, 90]), "round {round}: {under:?}");
    }
    *lock(&shown) = true;
    frame(&mut engine, "shown");
    let draws = engine.ui_draw_count();
    assert!(draws >= 1, "round {round}: the shown UI is drawn");
    let drawn = panel(&engine);
    assert!(near(drawn, [40, 60, 90]), "round {round}: {drawn:?}");
    for _ in 0..4 {
      frame(&mut engine, "shown");
    }
  }
}

/// Commands that draw nothing: off the UI image, outside their clip, and
/// under an empty clip
struct Culled;

impl Scene for Culled {
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    ui.draw_rect(Rect::new(-500.0, -500.0, 40.0, 20.0), Color::WHITE);
    ui.draw_text("Far away", 900.0, 900.0, Color::WHITE);
    ui.push_clip(Rect::new(10.0, 10.0, 50.0, 50.0));
    ui.draw_rect(Rect::new(100.0, 100.0, 40.0, 20.0), Color::WHITE);
    ui.pop_clip();
    ui.push_clip(Rect::new(10.0, 10.0, 0.0, 50.0));
    ui.draw_text("Clipped away", 12.0, 12.0, Color::WHITE);
    ui.pop_clip();
    SceneAction::Continue
  }
}

/// Recording nothing at all
struct NoUi;

impl Scene for NoUi {
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }
}

#[test]
fn a_ui_drawing_nothing_shows_what_no_ui_shows() {
  let at = || EngineBuilder::new().resolution(320, 240);
  let (Some(mut culled), Some(mut none)) = (
    HeadlessEngine::new_or_skip(at().start_scene(Culled)),
    HeadlessEngine::new_or_skip(at().start_scene(NoUi)),
  ) else {
    return;
  };
  // Drawn by the `ui` pass, then kept
  for n in 0..5 {
    frame(&mut culled, "the culled UI");
    frame(&mut none, "no UI");
    assert_eq!(culled.ui_draw_count(), 0, "frame {n}");
    let (a, b) = (
      culled.frame().expect("a frame"),
      none.frame().expect("a frame"),
    );
    assert!(a.as_raw() == b.as_raw(), "frame {n}: the culled UI shows");
  }
  assert!(culled.ui_render_stats().kept, "the culled UI is kept");
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
  let make = EngineBuilder::new()
    .resolution(640, 480)
    .start_scene(Labels { frame: 0 });
  let Some(mut engine) = HeadlessEngine::new_or_skip(make) else {
    return;
  };
  for n in 0..6 {
    frame(&mut engine, "3,000 labels");
    assert!(
      engine.ui_draw_count() >= 1,
      "frame {n}: the labels are drawn"
    );
  }
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
fn interleaved_textures_break_runs() {
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
  let make = EngineBuilder::new()
    .resolution(1280, 720)
    .start_scene(Interleaved { fillers, frame: 0 });
  let Some(mut engine) = HeadlessEngine::new_or_skip(make) else {
    return;
  };
  for n in 0..10 {
    frame(
      &mut engine,
      "two glyph pages and two atlas pages interleaved",
    );
    assert_eq!(
      engine.ui_draw_count(),
      INTERLEAVED_RUNS,
      "frame {n}: the runs"
    );
  }
}
