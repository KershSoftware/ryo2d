//! UI frame costs without a GPU: a 200-widget settings screen, a 1,000-row
//! scroll list and an empty frame, each one `UiHarness` frame (resolve,
//! the widgets, layout and commands).

use criterion::{Criterion, criterion_group, criterion_main};
use ryo2d::prelude::*;
use ryo2d::testing::UiHarness;

const TABS: [&str; 3] = ["Audio", "Video", "Controls"];
const OPTIONS: [&str; 3] = ["Low", "Medium", "High"];
/// Settings rows: each a row, a label, a control and a value text
const ROWS: usize = 50;
/// Buttons in the long list
const LIST: usize = 1000;

/// The settings screen's values, one of each kind per row
struct Settings {
  tab: usize,
  labels: Vec<String>,
  values: Vec<f32>,
  flags: Vec<bool>,
  choices: Vec<usize>,
}

impl Settings {
  fn new() -> Settings {
    Settings {
      tab: 0,
      labels: (0..ROWS).map(|i| format!("Setting {i}")).collect(),
      values: vec![0.5; ROWS],
      flags: vec![true; ROWS],
      choices: vec![1; ROWS],
    }
  }

  /// One panel, a focus scope that wraps (so a step down past the last row
  /// goes back to the top, and every step moves the focus and scrolls),
  /// holding tabs and a scroll area of 50 rows: a label, a slider,
  /// checkbox, dropdown or toggle (cycling) and a value
  fn ui(&mut self, ui: &mut UiContext<'_>) {
    ui.panel()
      .focus_scope()
      .wrap_focus()
      .anchor(Anchor::Center)
      .width(600.0)
      .height(460.0)
      .gap(8.0)
      .show(|ui| {
        ui.tabs(&mut self.tab, &TABS).show();
        ui.scroll().fill_width().grow(1.0).gap(6.0).show(|ui| {
          for i in 0..ROWS {
            let label = self.labels[i].as_str();
            let (value, flag, choice) = (
              &mut self.values[i],
              &mut self.flags[i],
              &mut self.choices[i],
            );
            ui.row().id(i).gap(12.0).align(Align::Center).show(|ui| {
              ui.text(label);
              match i % 4 {
                0 => {
                  ui.slider(value, 0.0..=1.0).width(200.0).show();
                  ui.text(format!("{:.0}%", *value * 100.0));
                }
                1 => {
                  ui.checkbox(flag, "Enabled").show();
                  ui.text(if *flag { "On" } else { "Off" });
                }
                2 => {
                  ui.dropdown(choice, &OPTIONS).width(200.0).show();
                  ui.text(OPTIONS[*choice]);
                }
                _ => {
                  ui.toggle(flag, "Enabled").show();
                  ui.text(if *flag { "On" } else { "Off" });
                }
              }
            });
          }
        });
      });
  }
}

/// 1,000 buttons in a 400-unit scroll area: its scroll offset and content
/// height, as last laid out
fn list(ui: &mut UiContext<'_>, labels: &[String]) -> (f32, f32) {
  let (mut first, mut last) = (Rect::default(), Rect::default());
  let area = ui.scroll().height(400.0).width(300.0).show(|ui| {
    for (i, label) in labels.iter().enumerate() {
      let rect = ui.button(label.as_str()).id(i).fill_width().show().rect;
      if i == 0 {
        first = rect;
      }
      last = rect;
    }
  });
  let offset = ui.scroll_offset(area.id).y;
  (offset, last.y + last.height - first.y)
}

fn settings_frame(c: &mut Criterion) {
  let mut harness = UiHarness::new(640.0, 480.0);
  let mut settings = Settings::new();
  for _ in 0..3 {
    harness.frame(|ui| settings.ui(ui));
  }
  assert!(harness.focused().is_some(), "the screen's scope took focus");
  // The panel wraps, so every step down moves the focus, past the last row
  // too: the measured steps keep moving it and scrolling
  for _ in 0..ROWS + 5 {
    let before = harness.focused();
    harness.action("ui.down");
    harness.frame(|ui| settings.ui(ui));
    assert_ne!(harness.focused(), before, "ui.down moved the focus");
  }
  let mut frame = 0u64;
  c.bench_function("ui frame, a 200-widget settings screen", |b| {
    b.iter(|| {
      // Navigation is part of the cost: a step down every 10th frame
      frame += 1;
      if frame.is_multiple_of(10) {
        harness.action("ui.down");
      }
      harness.frame(|ui| settings.ui(ui));
    })
  });
}

fn list_frame(c: &mut Criterion) {
  let mut harness = UiHarness::new(640.0, 480.0);
  let labels: Vec<String> = (0..LIST).map(|i| format!("Item {i}")).collect();
  let (_, content) = harness.frame(|ui| list(ui, &labels));
  // Wheel down to the middle of the list, ten lines a frame, then let the
  // smoothing settle
  let middle = (content - 400.0) / 2.0;
  harness.move_to(150.0, 200.0);
  for _ in 0..1000 {
    let (offset, _) = harness.frame(|ui| list(ui, &labels));
    if offset >= middle {
      break;
    }
    harness.wheel(0.0, -10.0);
  }
  for _ in 0..60 {
    harness.frame(|ui| list(ui, &labels));
  }
  let (offset, _) = harness.frame(|ui| list(ui, &labels));
  assert!(
    offset >= middle * 0.9 && offset <= middle * 1.1 + 400.0,
    "the list scrolled to its middle ({offset} of {content})"
  );
  c.bench_function("ui frame, a 1,000-row scroll list", |b| {
    b.iter(|| harness.frame(|ui| list(ui, &labels)))
  });
}

fn empty_frame(c: &mut Criterion) {
  let mut harness = UiHarness::new(640.0, 480.0);
  harness.frame(|_| ());
  c.bench_function("ui frame, an empty frame", |b| {
    b.iter(|| harness.frame(|_| ()))
  });
}

criterion_group!(benches, settings_frame, list_frame, empty_frame);
criterion_main!(benches);
