//! Headless tests of the demo shell's keys, pages and settings

use std::sync::{Arc, Mutex};

use demo_kit::prelude::*;
use ryo2d::testing::{HeadlessEngine, StepOutcome};

#[derive(Debug, Default, Clone)]
struct Seen {
  enters: Vec<&'static str>,
  exits: Vec<&'static str>,
  level: f32,
  count: i32,
  mode: usize,
  on: bool,
  /// Left clicks the page's updates saw
  clicks: u32,
  /// W held, as the page's last update saw it
  w_down: bool,
  /// The page's content area, from its last UI frame
  content: Option<Rect>,
  /// Clicks the Clicky page's button saw
  button_clicks: u32,
  /// The Typing page's field text, once drawn, and whether it has focus
  typed: Option<String>,
  field_focused: bool,
  /// The Dialog page's dialog is open, as its last update saw it
  dialog_open: bool,
}

type Shared = Arc<Mutex<Seen>>;

fn seen(shared: &Shared) -> Seen {
  shared.lock().map(|s| s.clone()).unwrap_or_default()
}

/// A page with one row of each kind (Level, Count, Mode, On); Mode "C" hides
/// the On toggle row. Optional text input and dropdown are drawn in `ui`.
struct Probe {
  name: &'static str,
  shared: Shared,
  level: f32,
  count: i32,
  mode: usize,
  on: bool,
  text: Option<TextInputState>,
  dropdown: Option<DropdownState>,
}

impl Probe {
  fn new(name: &'static str, shared: &Shared) -> Self {
    Self {
      name,
      shared: shared.clone(),
      level: 0.5,
      count: 3,
      mode: 0,
      on: false,
      text: None,
      dropdown: None,
    }
  }
}

impl DemoPage for Probe {
  fn name(&self) -> &str {
    self.name
  }

  fn settings(&mut self, s: &mut Settings) {
    s.float("Level", &mut self.level, 0.0..=1.0, 0.1);
    s.int("Count", &mut self.count, 1..=5);
    // Mode "C" hides the toggle row
    s.choice("Mode", &mut self.mode, &["A", "B", "C"]);
    if self.mode != 2 {
      s.toggle("On", &mut self.on);
    }
  }

  fn enter(&mut self, _ctx: &mut SceneContext) {
    if let Ok(mut seen) = self.shared.lock() {
      seen.enters.push(self.name);
    }
  }

  fn exit(&mut self, _ctx: &mut SceneContext) {
    if let Ok(mut seen) = self.shared.lock() {
      seen.exits.push(self.name);
    }
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) {
    if let Ok(mut seen) = self.shared.lock() {
      seen.level = self.level;
      seen.count = self.count;
      seen.mode = self.mode;
      seen.on = self.on;
      seen.clicks += u32::from(ctx.input.mouse_pressed(MouseButton::Left));
      seen.w_down = ctx.input.key_down(KeyCode::KeyW);
    }
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) {
    if let Ok(mut seen) = self.shared.lock() {
      seen.content = Some(content_area(ui));
    }
    if let Some(text) = &mut self.text {
      ui.text_input(text, Rect::new(10.0, 100.0, 120.0, 20.0), "", 0.0);
    }
    if let Some(dropdown) = &mut self.dropdown {
      ui.dropdown(dropdown, Rect::new(10.0, 130.0, 120.0, 20.0), &["x", "y"]);
    }
  }
}

fn three_pages(shared: &Shared) -> Demo {
  let (a, b, c) = (shared.clone(), shared.clone(), shared.clone());
  Demo::new("Probe")
    .resolution(320, 240)
    .page(move || Probe::new("one", &a))
    .page(move || Probe::new("two", &b))
    .page(move || Probe::new("three", &c))
}

fn start(demo: Demo) -> Option<HeadlessEngine> {
  HeadlessEngine::new_or_skip(demo.engine_builder())
}

/// Tap a key, then run two frames so a page switch has entered
fn tap(engine: &mut HeadlessEngine, key: KeyCode) -> StepOutcome {
  engine.tap_key(key);
  let first = engine.step().unwrap_or(StepOutcome::Quit);
  if first == StepOutcome::Quit {
    return first;
  }
  engine.step().unwrap_or(StepOutcome::Quit)
}

#[test]
fn brackets_and_page_keys_change_pages_and_wrap() {
  let shared = Shared::default();
  let Some(mut engine) = start(three_pages(&shared)) else {
    return;
  };
  engine.step_frames(1).unwrap();
  tap(&mut engine, KeyCode::BracketRight);
  tap(&mut engine, KeyCode::BracketLeft);
  tap(&mut engine, KeyCode::BracketLeft);
  tap(&mut engine, KeyCode::PageDown);
  tap(&mut engine, KeyCode::PageUp);
  assert_eq!(
    seen(&shared).enters,
    ["one", "two", "one", "three", "one", "three"]
  );
}

#[test]
fn a_page_exits_when_it_is_left_or_reset() {
  let shared = Shared::default();
  let Some(mut engine) = start(three_pages(&shared)) else {
    return;
  };
  engine.step_frames(2).unwrap();
  tap(&mut engine, KeyCode::BracketRight);
  assert_eq!(seen(&shared).exits, ["one"]);
  tap(&mut engine, KeyCode::KeyR);
  assert_eq!(seen(&shared).exits, ["one", "two"]);
  assert_eq!(seen(&shared).enters, ["one", "two", "two"]);
}

#[test]
fn start_page_opens_that_page() {
  let shared = Shared::default();
  let Some(mut engine) = start(three_pages(&shared).start_page(2)) else {
    return;
  };
  engine.step_frames(1).unwrap();
  assert_eq!(seen(&shared).enters, ["three"]);
}

#[test]
fn arrows_select_rows_and_change_values() {
  let shared = Shared::default();
  let Some(mut engine) = start(three_pages(&shared)) else {
    return;
  };
  engine.step_frames(1).unwrap();
  tap(&mut engine, KeyCode::ArrowRight); // Level 0.5 -> 0.6
  tap(&mut engine, KeyCode::ArrowDown); // Count
  for _ in 0..3 {
    tap(&mut engine, KeyCode::ArrowRight); // 3 -> 5, clamped
  }
  tap(&mut engine, KeyCode::ArrowDown); // Mode
  tap(&mut engine, KeyCode::ArrowLeft); // A -> C (wraps)
  tap(&mut engine, KeyCode::ArrowLeft); // C -> B
  tap(&mut engine, KeyCode::ArrowDown); // On
  tap(&mut engine, KeyCode::ArrowRight); // flips on
  let s = seen(&shared);
  assert!((s.level - 0.6).abs() < 1e-6, "{s:?}");
  assert_eq!((s.count, s.mode, s.on), (5, 1, true), "{s:?}");
}

#[test]
fn holding_right_repeats_and_releasing_stops() {
  let shared = Shared::default();
  let Some(mut engine) = start(three_pages(&shared)) else {
    return;
  };
  engine.step_frames(1).unwrap();
  // 0.6 s at 60 Hz: the first step, then repeats after 0.35 s, stopping
  // short of Level's 1.0 limit so a repeat after release would show
  engine.press_key(KeyCode::ArrowRight);
  engine.step_frames(36).unwrap();
  engine.release_key(KeyCode::ArrowRight);
  engine.step_frames(1).unwrap();
  let released = seen(&shared).level;
  assert!(released > 0.65, "repeated past one step: {released}");
  assert!(released < 1.0, "still below the limit: {released}");
  engine.step_frames(60).unwrap();
  assert_eq!(seen(&shared).level, released, "no change after release");
}

#[test]
fn reset_rebuilds_the_page_with_defaults() {
  let shared = Shared::default();
  let Some(mut engine) = start(three_pages(&shared)) else {
    return;
  };
  engine.step_frames(1).unwrap();
  tap(&mut engine, KeyCode::ArrowRight);
  tap(&mut engine, KeyCode::KeyR);
  let s = seen(&shared);
  assert_eq!(s.enters, ["one", "one"]);
  assert_eq!(s.level, 0.5);
}

#[test]
fn reset_keeps_the_selected_row() {
  let shared = Shared::default();
  let Some(mut engine) = start(three_pages(&shared)) else {
    return;
  };
  engine.step_frames(2).unwrap();
  // Pick the Mode row (third), reset, then change the selected value
  tap(&mut engine, KeyCode::ArrowDown);
  tap(&mut engine, KeyCode::ArrowDown);
  tap(&mut engine, KeyCode::KeyR);
  tap(&mut engine, KeyCode::ArrowRight);
  assert_eq!(seen(&shared).mode, 1, "Mode changed");
  assert_eq!(seen(&shared).level, 0.5, "Level untouched");
}

#[test]
fn a_new_page_shows_its_panel_on_its_first_frame() {
  let shared = Shared::default();
  let Some(mut engine) = start(three_pages(&shared)) else {
    return;
  };
  engine.step_frames(2).unwrap();
  let Some(content) = seen(&shared).content else {
    panic!("the page drew no UI frame");
  };
  // The frame that switches page, then the next one
  engine.tap_key(KeyCode::BracketRight);
  engine.step().unwrap();
  let first = engine.frame().unwrap();
  engine.step().unwrap();
  let second = engine.frame().unwrap();
  let panel_x = (content.right() - 40.0) as u32..content.right() as u32;
  let panel_y = content.y as u32..(content.y + 30.0) as u32;
  for y in panel_y {
    for x in panel_x.clone() {
      assert_eq!(
        first.get_pixel(x, y),
        second.get_pixel(x, y),
        "panel differs at ({x}, {y}) on the page's first frame"
      );
    }
  }
}

#[test]
fn holding_right_through_a_reset_does_not_keep_adjusting() {
  let shared = Shared::default();
  let Some(mut engine) = start(three_pages(&shared)) else {
    return;
  };
  engine.step_frames(1).unwrap();
  engine.press_key(KeyCode::ArrowRight);
  engine.step_frames(2).unwrap();
  tap(&mut engine, KeyCode::KeyR);
  engine.step_frames(60).unwrap(); // still held, 1 s
  assert_eq!(seen(&shared).level, 0.5);
}

#[test]
fn selection_clamps_when_rows_disappear() {
  let shared = Shared::default();
  let Some(mut engine) = start(three_pages(&shared)) else {
    return;
  };
  engine.step_frames(1).unwrap();
  for _ in 0..3 {
    tap(&mut engine, KeyCode::ArrowDown); // On (row 4 of 4)
  }
  tap(&mut engine, KeyCode::ArrowUp); // Mode
  tap(&mut engine, KeyCode::ArrowLeft); // A -> C: the On row disappears
  tap(&mut engine, KeyCode::ArrowDown); // stays on Mode, the last row
  tap(&mut engine, KeyCode::ArrowRight); // C -> A
  let s = seen(&shared);
  assert_eq!(s.mode, 0, "{s:?}");
  assert!(!s.on, "{s:?}");
}

#[test]
fn help_closes_on_escape_before_escape_quits() {
  let shared = Shared::default();
  let Some(mut engine) = start(three_pages(&shared)) else {
    return;
  };
  engine.step_frames(1).unwrap();
  assert_eq!(tap(&mut engine, KeyCode::KeyH), StepOutcome::Running);
  assert_eq!(tap(&mut engine, KeyCode::Escape), StepOutcome::Running);
  assert_eq!(tap(&mut engine, KeyCode::Escape), StepOutcome::Quit);
}

/// A page whose dialog starts open; while it is open the page wants Escape,
/// lists it, and closes the dialog with it
struct Dialog {
  shared: Shared,
  open: bool,
}

impl DemoPage for Dialog {
  fn name(&self) -> &str {
    "Dialog"
  }

  fn controls(&self) -> Vec<Control> {
    if self.open {
      vec![Control::key(KeyCode::Escape, "Close the dialog")]
    } else {
      Vec::new()
    }
  }

  fn wants_escape(&self) -> bool {
    self.open
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) {
    if self.open && ctx.input.key_pressed(KeyCode::Escape) {
      self.open = false;
    }
    if let Ok(mut seen) = self.shared.lock() {
      seen.dialog_open = self.open;
    }
  }
}

fn dialog(shared: &Shared) -> Demo {
  let shared = shared.clone();
  Demo::new("Dialog")
    .resolution(320, 240)
    .page(move || Dialog {
      shared: shared.clone(),
      open: true,
    })
}

#[test]
fn a_page_that_wants_escape_gets_it_instead_of_quitting() {
  let shared = Shared::default();
  let Some(mut engine) = start(dialog(&shared)) else {
    return;
  };
  engine.step_frames(1).unwrap();
  assert!(seen(&shared).dialog_open);
  assert_eq!(tap(&mut engine, KeyCode::Escape), StepOutcome::Running);
  assert!(!seen(&shared).dialog_open);
  // The page no longer wants it: Escape quits again
  assert_eq!(tap(&mut engine, KeyCode::Escape), StepOutcome::Quit);
}

#[test]
fn help_closes_before_a_page_gets_escape() {
  let shared = Shared::default();
  let Some(mut engine) = start(dialog(&shared)) else {
    return;
  };
  engine.step_frames(1).unwrap();
  assert_eq!(tap(&mut engine, KeyCode::KeyH), StepOutcome::Running);
  assert_eq!(tap(&mut engine, KeyCode::Escape), StepOutcome::Running);
  assert!(seen(&shared).dialog_open, "help took the first Escape");
  assert_eq!(tap(&mut engine, KeyCode::Escape), StepOutcome::Running);
  assert!(!seen(&shared).dialog_open);
}

#[test]
fn shell_keys_are_ignored_while_typing() {
  let shared = Shared::default();
  let typing = shared.clone();
  let demo = Demo::new("Typing").resolution(320, 240).page(move || {
    let mut page = Probe::new("typing", &typing);
    let mut text = TextInputState::new();
    text.set_focused(true);
    page.text = Some(text);
    page
  });
  let Some(mut engine) = start(demo) else {
    return;
  };
  engine.step_frames(2).unwrap();
  for key in [
    KeyCode::KeyR,
    KeyCode::BracketRight,
    KeyCode::ArrowRight,
    KeyCode::KeyH,
    KeyCode::Escape,
  ] {
    assert_eq!(tap(&mut engine, key), StepOutcome::Running, "{key:?}");
  }
  let s = seen(&shared);
  assert_eq!(s.enters, ["typing"]);
  assert_eq!(s.level, 0.5);
}

#[test]
fn escape_with_an_open_dropdown_does_not_quit() {
  let shared = Shared::default();
  let picking = shared.clone();
  let demo = Demo::new("Dropdown").resolution(320, 240).page(move || {
    let mut page = Probe::new("dropdown", &picking);
    let mut dropdown = DropdownState::with_id(7);
    dropdown.open();
    page.dropdown = Some(dropdown);
    page
  });
  let Some(mut engine) = start(demo) else {
    return;
  };
  engine.step_frames(2).unwrap();
  assert_eq!(tap(&mut engine, KeyCode::Escape), StepOutcome::Running);
}

#[test]
fn a_focused_but_closed_dropdown_does_not_block_the_shell_keys() {
  let shared = Shared::default();
  let (a, b) = (shared.clone(), shared.clone());
  let demo = Demo::new("Dropdown")
    .resolution(320, 240)
    .page(move || {
      let mut page = Probe::new("one", &a);
      let mut dropdown = DropdownState::with_id(7);
      dropdown.set_focused(true);
      page.dropdown = Some(dropdown);
      page
    })
    .page(move || Probe::new("two", &b));
  let Some(mut engine) = start(demo) else {
    return;
  };
  engine.step_frames(2).unwrap();
  tap(&mut engine, KeyCode::BracketRight);
  assert_eq!(seen(&shared).enters, ["one", "two"]);
}

#[test]
fn arrows_do_nothing_while_help_is_open() {
  let shared = Shared::default();
  let Some(mut engine) = start(three_pages(&shared)) else {
    return;
  };
  engine.step_frames(2).unwrap();
  tap(&mut engine, KeyCode::KeyH);
  tap(&mut engine, KeyCode::ArrowRight);
  tap(&mut engine, KeyCode::Escape);
  assert_eq!(seen(&shared).level, 0.5);
}

/// Click at `(x, y)` and run a frame
fn click(engine: &mut HeadlessEngine, x: f32, y: f32) {
  engine.mouse_move(x, y);
  engine.press_mouse(MouseButton::Left);
  let _ = engine.step();
  engine.release_mouse(MouseButton::Left);
  let _ = engine.step();
}

#[test]
fn a_click_on_the_settings_panel_does_not_reach_the_page() {
  let shared = Shared::default();
  let Some(mut engine) = start(three_pages(&shared)) else {
    return;
  };
  engine.step_frames(2).unwrap();
  let Some(content) = seen(&shared).content else {
    panic!("the page drew no UI frame");
  };
  // The panel sits at the content area's top right
  click(&mut engine, content.right() - 4.0, content.y + 4.0);
  assert_eq!(seen(&shared).clicks, 0);
  // Left of the panel, the page gets the click
  click(&mut engine, content.x + 4.0, content.y + 4.0);
  assert_eq!(seen(&shared).clicks, 1);
}

#[test]
fn the_page_sees_no_input_while_help_is_open() {
  let shared = Shared::default();
  let Some(mut engine) = start(three_pages(&shared)) else {
    return;
  };
  engine.step_frames(2).unwrap();
  let Some(content) = seen(&shared).content else {
    panic!("the page drew no UI frame");
  };
  tap(&mut engine, KeyCode::KeyH);
  click(&mut engine, content.x + 4.0, content.y + 4.0);
  engine.press_key(KeyCode::KeyW);
  let _ = engine.step();
  assert_eq!(seen(&shared).clicks, 0);
  assert!(!seen(&shared).w_down);
  // Closing help hands input back
  tap(&mut engine, KeyCode::Escape);
  assert!(seen(&shared).w_down);
}

/// A page whose text field starts focused, like the ui demo's Text Input page
struct Typing {
  shared: Shared,
  field: TextInputState,
}

impl Typing {
  fn new(shared: &Shared) -> Self {
    let mut field = TextInputState::new();
    field.set_focused(true);
    Self {
      shared: shared.clone(),
      field,
    }
  }
}

impl DemoPage for Typing {
  fn name(&self) -> &str {
    "Typing"
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) {
    ui.text_input(
      &mut self.field,
      Rect::new(10.0, 100.0, 120.0, 20.0),
      "",
      0.0,
    );
    if let Ok(mut seen) = self.shared.lock() {
      seen.typed = Some(self.field.text().to_string());
      seen.field_focused = self.field.is_focused();
    }
  }
}

#[test]
fn arriving_on_a_focused_field_types_nothing() {
  let shared = Shared::default();
  let (a, b) = (shared.clone(), shared.clone());
  let demo = Demo::new("Typing")
    .resolution(320, 240)
    .page(move || Probe::new("one", &a))
    .page(move || Typing::new(&b));
  let Some(mut engine) = start(demo) else {
    return;
  };
  engine.step_frames(2).unwrap();
  // A real `]` press also types "]"
  engine.type_text("]");
  tap(&mut engine, KeyCode::BracketRight);
  assert_eq!(seen(&shared).typed.as_deref(), Some(""));
}

#[test]
fn escape_leaves_a_focused_field_before_it_quits() {
  let shared = Shared::default();
  let page = shared.clone();
  let demo = Demo::new("Typing")
    .resolution(320, 240)
    .page(move || Typing::new(&page));
  let Some(mut engine) = start(demo) else {
    return;
  };
  engine.step_frames(2).unwrap();
  assert!(seen(&shared).field_focused);
  assert_eq!(tap(&mut engine, KeyCode::Escape), StepOutcome::Running);
  assert!(
    !seen(&shared).field_focused,
    "the first Esc leaves the field"
  );
  assert_eq!(tap(&mut engine, KeyCode::Escape), StepOutcome::Quit);
}

/// A page with one button at (10, 100)-(130, 120)
struct Clicky {
  shared: Shared,
}

impl DemoPage for Clicky {
  fn name(&self) -> &str {
    "Clicky"
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) {
    if ui
      .button("Press")
      .at(Rect::new(10.0, 100.0, 120.0, 20.0))
      .clicked()
      && let Ok(mut seen) = self.shared.lock()
    {
      seen.button_clicks += 1;
    }
  }
}

#[test]
fn page_widgets_take_no_clicks_under_help() {
  let shared = Shared::default();
  let page = shared.clone();
  let demo = Demo::new("Clicky")
    .resolution(320, 240)
    .page(move || Clicky {
      shared: page.clone(),
    });
  let Some(mut engine) = start(demo) else {
    return;
  };
  engine.step_frames(2).unwrap();
  tap(&mut engine, KeyCode::KeyH);
  click(&mut engine, 40.0, 110.0);
  assert_eq!(seen(&shared).button_clicks, 0);
  tap(&mut engine, KeyCode::KeyH);
  click(&mut engine, 40.0, 110.0);
  assert_eq!(seen(&shared).button_clicks, 1);
}

struct Greedy;

impl DemoPage for Greedy {
  fn name(&self) -> &str {
    "Greedy"
  }
  fn controls(&self) -> Vec<Control> {
    vec![Control::key(KeyCode::KeyH, "Hop")]
  }
}

/// Lists H as a control once its setting is switched on
struct Shifty {
  late_h: bool,
}

impl DemoPage for Shifty {
  fn name(&self) -> &str {
    "Shifty"
  }
  fn settings(&mut self, s: &mut Settings) {
    s.toggle("Late H", &mut self.late_h);
  }
  fn controls(&self) -> Vec<Control> {
    if self.late_h {
      vec![Control::key(KeyCode::KeyH, "Hop")]
    } else {
      Vec::new()
    }
  }
}

#[test]
fn a_reserved_key_listed_later_is_caught_in_debug_builds() {
  let demo = Demo::new("Shifty")
    .resolution(320, 240)
    .page(|| Shifty { late_h: false });
  let Some(mut engine) = start(demo) else {
    return;
  };
  engine.step_frames(2).unwrap();
  let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
    tap(&mut engine, KeyCode::ArrowRight);
    engine.step_frames(2).unwrap();
  }));
  let message = result
    .err()
    .and_then(|panic| panic.downcast::<String>().ok())
    .map(|message| *message)
    .unwrap_or_default();
  assert!(
    message.contains("reserved"),
    "no reserved-key panic: {message:?}"
  );
}

#[test]
#[should_panic(expected = "reserved")]
fn a_page_listing_a_reserved_key_is_rejected() {
  let _ = Demo::new("Greedy").page(|| Greedy).engine_builder();
}
