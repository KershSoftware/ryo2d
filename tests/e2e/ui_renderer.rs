//! The dedicated UI renderer against the old UI path: two engines driven alike
//! draw the same pixels every frame

use std::sync::{Arc, Mutex};

use ryo2d::prelude::*;
use ryo2d::testing::PixelPair;

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
