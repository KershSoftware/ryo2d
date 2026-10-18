//! Allocations per UI frame, counted by a global allocator in this test binary
// This test binary must hold exactly one test: the allocator is global, and a second test on another thread would count into it

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use ryo2d::prelude::*;
use ryo2d::testing::UiHarness;

struct Counting;
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
  unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
    ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
    unsafe { System.alloc(layout) }
  }
  unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
    unsafe { System.dealloc(ptr, layout) }
  }
  unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
    ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
    unsafe { System.realloc(ptr, layout, size) }
  }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

const TABS: [&str; 3] = ["Audio", "Video", "Controls"];
const OPTIONS: [&str; 3] = ["Low", "Medium", "High"];
/// Settings rows: each a row, a label, a control and a value text
const ROWS: usize = 50;

/// The settings screen's values, one of each kind per row (`benches/ui.rs`)
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

  /// One panel, a focus scope that wraps, holding tabs and a scroll area of
  /// 50 rows: a label, a slider, checkbox, dropdown or toggle (cycling) and
  /// a value
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

#[test]
fn an_unchanged_settings_frame_allocates_little() {
  let mut harness = UiHarness::new(640.0, 480.0);
  let mut settings = Settings::new();
  for _ in 0..5 {
    harness.frame(|ui| settings.ui(ui));
  }
  let before = ALLOCATIONS.load(Ordering::Relaxed);
  harness.frame(|ui| settings.ui(ui));
  let made = ALLOCATIONS.load(Ordering::Relaxed) - before;
  println!("allocations in one frame: {made}");
  // 191 when written: each label copied into its text leaf, line points and
  // this screen's own `format!`s; less than one a row to spare
  assert!(
    made < 220,
    "an unchanged 200-widget frame made {made} allocations"
  );
}
