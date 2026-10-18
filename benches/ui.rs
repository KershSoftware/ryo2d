//! UI frame costs without a GPU: a 200-widget settings screen (as a closure
//! and as a node tree), a 1,000-row scroll list and an empty frame, each one
//! `UiHarness` frame (resolve, the node walk, the widgets, layout and
//! commands); and the settings tree updated unchanged against spawning it.

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
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

/// The settings screen as a node tree: the same panel, tabs and 50 rows
fn settings_nodes(settings: &Settings) -> UiNode {
  let rows = (0..ROWS).map(|i| {
    let label = settings.labels[i].as_str();
    let (widget, value) = match i % 4 {
      0 => (
        UiNode::slider(0.0..=1.0, settings.values[i]).width(200.0),
        format!("{:.0}%", settings.values[i] * 100.0),
      ),
      1 => (
        UiNode::checkbox("Enabled", settings.flags[i]),
        "On".to_string(),
      ),
      2 => (
        UiNode::dropdown(&OPTIONS, settings.choices[i]).width(200.0),
        OPTIONS[settings.choices[i]].to_string(),
      ),
      _ => (
        UiNode::toggle("Enabled", settings.flags[i]),
        "On".to_string(),
      ),
    };
    UiNode::row()
      .gap(12.0)
      .align(Align::Center)
      .child(UiNode::text(label))
      .child(widget)
      .child(UiNode::text(value))
  });
  UiNode::panel()
    .focus_scope()
    .wrap_focus()
    .anchor(Anchor::Center)
    .width(600.0)
    .height(460.0)
    .gap(8.0)
    .child(UiNode::tabs(&TABS, settings.tab))
    .child(
      UiNode::scroll()
        .fill_width()
        .grow(1.0)
        .gap(6.0)
        .children(rows),
    )
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

/// The guard: the closure settings screen stays under 60 µs, and this node
/// screen within the larger of 10% or 10 µs of it: the cost of walking the
/// node entities on top of the same builders.
fn settings_nodes_frame(c: &mut Criterion) {
  let mut harness = UiHarness::new(640.0, 480.0);
  harness.spawn_ui(settings_nodes(&Settings::new()));
  for _ in 0..3 {
    harness.frame(|_| {});
  }
  assert!(harness.focused().is_some(), "the screen's scope took focus");
  for _ in 0..ROWS + 5 {
    let before = harness.focused();
    harness.action("ui.down");
    harness.frame(|_| {});
    assert_ne!(harness.focused(), before, "ui.down moved the focus");
  }
  let mut frame = 0u64;
  c.bench_function("ui frame, the 200-widget settings screen as nodes", |b| {
    b.iter(|| {
      frame += 1;
      if frame.is_multiple_of(10) {
        harness.action("ui.down");
      }
      harness.frame(|_| {});
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

/// A closure list of `count` buttons (the long list's row) in a 400 by 300
/// view: the items built this frame
fn virtual_list(ui: &mut UiContext<'_>, count: usize) -> ListResponse {
  ui.list(count)
    .row_height(30.0)
    .gap(2.0)
    .height(400.0)
    .width(300.0)
    .id("bench list")
    .show(|ui, i| {
      ui.button(format!("Item {i}")).id(i).fill_width().show();
    })
}

/// Jump a closure list to its middle and let the eased scroll settle
fn settle_virtual_list(count: usize) -> UiHarness {
  let mut harness = UiHarness::new(640.0, 480.0);
  let list = harness.frame(|ui| virtual_list(ui, count));
  let middle = count / 2;
  harness.frame(|ui| ui.list_scroll_to(list.id, middle));
  for _ in 0..60 {
    harness.frame(|ui| virtual_list(ui, count));
  }
  let list = harness.frame(|ui| virtual_list(ui, count));
  assert!(
    list.range.contains(&middle),
    "the list scrolled to its middle ({:?} of {count})",
    list.range
  );
  harness
}

fn virtual_list_frame(c: &mut Criterion) {
  let mut harness = settle_virtual_list(10_000);
  c.bench_function("ui frame, a 10,000-row virtual list", |b| {
    b.iter(|| harness.frame(|ui| virtual_list(ui, 10_000)))
  });
}

fn virtual_list_100_frame(c: &mut Criterion) {
  let mut harness = settle_virtual_list(100);
  c.bench_function("ui frame, a 100-row virtual list", |b| {
    b.iter(|| harness.frame(|ui| virtual_list(ui, 100)))
  });
}

/// Bind every row the last frame asked for, as a game would
fn bind_rows(harness: &mut UiHarness) -> usize {
  let binds: Vec<UiListBind> = harness.messages::<UiListBind>().collect();
  for bind in &binds {
    harness.ui_set(bind.row, "name", format!("Item {}", bind.index));
  }
  binds.len()
}

fn node_list_frame(c: &mut Criterion) {
  let mut harness = UiHarness::new(640.0, 480.0);
  harness.spawn_ui(
    UiNode::list(10_000, 30.0)
      .gap(2.0)
      .height(400.0)
      .width(300.0)
      .row_shape(|| UiNode::button("{name}").bound().fill_width())
      .name("board"),
  );
  let list = harness.find_ui("board").expect("spawned");
  harness.frame(|_| {});
  bind_rows(&mut harness);
  harness.ui_list_scroll_to(list, 5_000);
  let mut reached_middle = false;
  for _ in 0..60 {
    harness.frame(|_| {});
    reached_middle |= harness
      .messages::<UiListBind>()
      .any(|bind| bind.index.abs_diff(5_000) < 20);
    bind_rows(&mut harness);
  }
  harness.frame(|_| {});
  bind_rows(&mut harness);
  // Rows bound while it eased there: the last ones are the middle's
  assert!(reached_middle, "the node list scrolled to its middle");
  c.bench_function("ui frame, a 10,000-row node list", |b| {
    b.iter(|| {
      harness.frame(|_| {});
      bind_rows(&mut harness)
    })
  });
}

fn empty_frame(c: &mut Criterion) {
  let mut harness = UiHarness::new(640.0, 480.0);
  harness.frame(|_| ());
  c.bench_function("ui frame, an empty frame", |b| {
    b.iter(|| harness.frame(|_| ()))
  });
}

fn settings_update_unchanged(c: &mut Criterion) {
  let settings = Settings::new();
  let mut harness = UiHarness::new(640.0, 480.0);
  let root = harness.spawn_ui(settings_nodes(&settings));
  harness.frame(|_| {});
  c.bench_function("ui update, the 200-widget settings tree unchanged", |b| {
    b.iter_batched(
      || settings_nodes(&settings),
      |tree| harness.update_ui(root, tree),
      BatchSize::SmallInput,
    )
  });
}

fn settings_spawn(c: &mut Criterion) {
  let settings = Settings::new();
  let mut harness = UiHarness::new(640.0, 480.0);
  c.bench_function(
    "ui spawn, the 200-widget settings tree (and despawn)",
    |b| {
      b.iter_batched(
        || settings_nodes(&settings),
        |tree| {
          let root = harness.spawn_ui(tree);
          harness.world_mut().despawn(root);
        },
        BatchSize::SmallInput,
      )
    },
  );
}

criterion_group!(
  benches,
  settings_frame,
  settings_nodes_frame,
  list_frame,
  virtual_list_frame,
  virtual_list_100_frame,
  node_list_frame,
  empty_frame,
  settings_update_unchanged,
  settings_spawn
);
criterion_main!(benches);
