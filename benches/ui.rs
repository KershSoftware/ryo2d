//! UI frame costs without a GPU: a 200-widget settings screen (as a closure
//! and as a node tree, moving its focus and unchanged), a 2,000-node static
//! tree, an embedded node tree in closure UI, 64 embedded trees cached and
//! uncached, a 1,000-row scroll list,
//! virtual lists in both styles (10,000 and 100 closure rows, 10,000 node
//! rows) and an empty frame, each one `UiHarness` frame (resolve, the node
//! walk, the widgets, layout and commands); the node cache's dirty check
//! alone; and the settings tree updated unchanged against spawning it.

use std::time::{Duration, Instant};

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use ryo2d::prelude::*;
use ryo2d::testing::UiHarness;
use ryo2d::testing::fixtures::settings_tree;

const TABS: [&str; 3] = ["Audio", "Video", "Controls"];
const OPTIONS: [&str; 3] = ["Low", "Medium", "High"];
/// Settings rows: each a row, a label, a control and a value text
const ROWS: usize = 50;
/// Buttons in the long list
const LIST: usize = 1000;
/// Nodes in the static tree: text and buttons, alternating
const STATIC: usize = 2000;

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

/// The guard: the closure settings screen stays under 60 µs, and this node
/// screen within the larger of 10% or 10 µs of it: the cost of walking the
/// node entities on top of the same builders.
fn settings_nodes_frame(c: &mut Criterion) {
  let mut harness = UiHarness::new(640.0, 480.0);
  harness.spawn_ui(settings_tree());
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

/// The settings tree drawn again with no input, its scope's first tab
/// focused: the cached frame replays every node but the focused path and
/// the scroll area (the target: under 35 µs)
fn settings_nodes_unchanged(c: &mut Criterion) {
  let mut harness = UiHarness::new(640.0, 480.0);
  harness.spawn_ui(settings_tree());
  for _ in 0..10 {
    harness.frame(|_| {});
  }
  assert!(harness.focused().is_some(), "the screen's scope took focus");
  let stats = harness.ui_cache_stats();
  assert!(stats.replayed > 0, "the unchanged tree replays");
  println!(
    "settings nodes, unchanged: {} built, {} replayed",
    stats.built, stats.replayed
  );
  c.bench_function(
    "ui frame, the 200-widget settings screen as nodes, unchanged",
    |b| b.iter(|| harness.frame(|_| {})),
  );
}

/// A grid of 2,000 nodes, text and buttons alternating, 20 to a row
fn static_tree() -> UiNode {
  UiNode::grid(20).gap(2.0).children((0..STATIC).map(|i| {
    if i % 2 == 0 {
      UiNode::text(format!("Item {i}"))
    } else {
      UiNode::button(format!("Go {i}"))
    }
  }))
}

/// The static tree drawn again with no input and the pointer over nothing:
/// every node replays (the cost per replayed node is this over the count)
fn static_tree_frame(c: &mut Criterion) {
  let mut harness = UiHarness::new(640.0, 480.0);
  harness.spawn_ui(static_tree());
  harness.move_to(-100.0, -100.0);
  for _ in 0..5 {
    harness.frame(|_| {});
  }
  let stats = harness.ui_cache_stats();
  assert_eq!(stats.built, 0, "nothing in the static tree builds");
  println!("static tree: {} replayed", stats.replayed);
  c.bench_function("ui frame, a 2,000-node static tree", |b| {
    b.iter(|| harness.frame(|_| {}))
  });
}

/// The `ui` demo's Mixed page: a closure panel around an embedded grid of 8
/// node buttons, its own Add button focused
fn mixed(ui: &mut UiContext<'_>, world: &mut World, grid: Entity, status: &str) {
  let heading = ui.theme().text.heading.style();
  let muted = ui.theme().palette.muted;
  ui.panel()
    .focus_scope()
    .anchor(Anchor::Center)
    .width(420.0)
    .gap(10.0)
    .show(|ui| {
      ui.text("Mixed").style(&heading);
      ui.row().gap(8.0).show(|ui| {
        ui.button("Add").default_focus().show();
        ui.button("Clear").show();
      });
      ui.node(grid, world);
      ui.text(status).color(muted);
      ui.button("Done").show();
    });
}

/// The Mixed page drawn again with no input: the embedded tree's dirty check
/// and its replay inside the closure's panel
fn embedded_frame(c: &mut Criterion) {
  let mut harness = UiHarness::new(640.0, 480.0);
  let cells = (1..=8).map(|n| {
    UiNode::button(format!("Cell {n}"))
      .action("cell")
      .width(80.0)
  });
  let grid = harness.spawn_ui(UiNode::grid(4).gap(6.0).embedded().children(cells));
  let status = "Add a cell, or pick one.".to_string();
  for _ in 0..5 {
    harness.frame_with_world(|ui, world| mixed(ui, world, grid, &status));
  }
  assert!(harness.focused().is_some(), "the panel's scope took focus");
  let stats = harness.ui_cache_stats();
  assert!(stats.replayed > 0, "the embedded tree replays");
  c.bench_function("ui frame, an embedded node tree in closure UI", |b| {
    b.iter(|| harness.frame_with_world(|ui, world| mixed(ui, world, grid, &status)))
  });
}

/// Embedded trees in the many-trees frame
const EMBEDDED: usize = 64;

/// `EMBEDDED` embedded trees of three nodes (a column of a text and a
/// button), warmed up with caching on or off and the pointer over nothing
fn embedded_trees(cache: bool) -> (UiHarness, Vec<Entity>) {
  let mut harness = UiHarness::new(640.0, 480.0);
  harness.set_ui_cache(cache);
  let roots: Vec<Entity> = (0..EMBEDDED)
    .map(|i| {
      harness.spawn_ui(
        UiNode::column()
          .embedded()
          .child(UiNode::text(format!("Item {i}")))
          .child(UiNode::button("Use")),
      )
    })
    .collect();
  harness.move_to(-100.0, -100.0);
  for _ in 0..5 {
    trees_frame(&mut harness, &roots);
  }
  (harness, roots)
}

/// One frame of the embedded trees, drawn with `ui.node` in a closure grid
fn trees_frame(harness: &mut UiHarness, roots: &[Entity]) {
  harness.frame_with_world(|ui, world| {
    ui.grid(8).gap(4.0).show(|ui| {
      for &root in roots {
        ui.node(root, world);
      }
    });
  });
}

/// The time per frame of `cached` and of `uncached`, interleaved so load and
/// drift hit both alike: each the median of 101 rounds of 20 frames
fn per_frame(
  cached: &mut UiHarness,
  uncached: &mut UiHarness,
  roots: &[Entity],
) -> (Duration, Duration) {
  let time = |harness: &mut UiHarness| {
    let start = Instant::now();
    for _ in 0..20 {
      trees_frame(harness, roots);
    }
    start.elapsed() / 20
  };
  let (mut on, mut off) = (Vec::new(), Vec::new());
  for round in 0..101 {
    if round % 2 == 0 {
      on.push(time(cached));
      off.push(time(uncached));
    } else {
      off.push(time(uncached));
      on.push(time(cached));
    }
  }
  on.sort_unstable();
  off.sort_unstable();
  (on[50], off[50])
}

/// 64 embedded trees in closure UI, unchanged, cached and uncached. The
/// guard: cached is no slower than uncached (each `ui.node` checks its own
/// tree, not every node in the world).
fn embedded_trees_frame(c: &mut Criterion) {
  let (mut cached, roots) = embedded_trees(true);
  let (mut uncached, _) = embedded_trees(false);
  let stats = cached.ui_cache_stats();
  assert_eq!(stats.built, 0, "nothing in the trees builds: {stats:?}");
  let (on, off) = per_frame(&mut cached, &mut uncached, &roots);
  println!("{EMBEDDED} embedded trees, medians: {on:?} cached, {off:?} uncached");
  assert!(
    on <= off,
    "{EMBEDDED} embedded trees: {on:?} a frame cached, {off:?} uncached"
  );
  let mut group = c.benchmark_group("ui frame, 64 embedded three-node trees");
  group.bench_function("cached", |b| b.iter(|| trees_frame(&mut cached, &roots)));
  group.bench_function("uncached", |b| {
    b.iter(|| trees_frame(&mut uncached, &roots))
  });
  group.finish();
}

/// The node cache's dirty check on the settings tree, alone: each check
/// follows an unmeasured unchanged frame, as the walk's does
fn dirty_check(c: &mut Criterion) {
  let mut harness = UiHarness::new(640.0, 480.0);
  harness.spawn_ui(settings_tree());
  for _ in 0..10 {
    harness.frame(|_| {});
  }
  let mut group = c.benchmark_group("ui dirty check");
  // Each measured check costs a frame besides
  group.warm_up_time(Duration::from_millis(200));
  group.measurement_time(Duration::from_secs(1));
  group.bench_function("the 200-widget settings tree", |b| {
    b.iter_custom(|iters| {
      let mut total = Duration::ZERO;
      for _ in 0..iters {
        harness.frame(|_| {});
        let start = Instant::now();
        harness.ui_dirty_check();
        total += start.elapsed();
      }
      total
    })
  });
  group.finish();
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

/// A closure list of `count` buttons (the long list's row) in a view 300 wide
/// and 400 high: the items built this frame
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
  let next = harness.frame(|ui| virtual_list(ui, count));
  assert_eq!(next.range, list.range, "the scroll has settled");
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
  for _ in 0..60 {
    harness.frame(|_| {});
    bind_rows(&mut harness);
  }
  // Bind every pooled row again: they all sit around the middle
  harness.ui_list_refresh(list);
  harness.frame(|_| {});
  let pooled: Vec<usize> = harness
    .messages::<UiListBind>()
    .map(|bind| bind.index)
    .collect();
  assert!(
    pooled.contains(&5_000) && pooled.iter().all(|i| i.abs_diff(5_000) < 30),
    "the node list scrolled to its middle ({pooled:?})"
  );
  bind_rows(&mut harness);
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
  let mut harness = UiHarness::new(640.0, 480.0);
  let root = harness.spawn_ui(settings_tree());
  harness.frame(|_| {});
  c.bench_function("ui update, the 200-widget settings tree unchanged", |b| {
    b.iter_batched(
      settings_tree,
      |tree| harness.update_ui(root, tree),
      BatchSize::SmallInput,
    )
  });
}

fn settings_spawn(c: &mut Criterion) {
  let mut harness = UiHarness::new(640.0, 480.0);
  c.bench_function(
    "ui spawn, the 200-widget settings tree (and despawn)",
    |b| {
      b.iter_batched(
        settings_tree,
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
  settings_nodes_unchanged,
  static_tree_frame,
  embedded_frame,
  embedded_trees_frame,
  dirty_check,
  list_frame,
  virtual_list_frame,
  virtual_list_100_frame,
  node_list_frame,
  empty_frame,
  settings_update_unchanged,
  settings_spawn
);
criterion_main!(benches);
