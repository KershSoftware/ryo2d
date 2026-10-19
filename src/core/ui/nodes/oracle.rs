//! The differential oracle: caching is not observable
//!
//! A scenario runs in two harnesses, the node cache on in one and off in
//! the other, through the same setup and steps. After every frame the two
//! must agree on everything a player or the game can see
//! (`UiHarness::frame_snapshot`: draw commands, records, focus, events,
//! component values), and the cached one must never find a child unclean
//! inside its parent's replay (a miss) or a dirty node under a clean parent.
//! The scenarios are in `oracle_tests.rs`; the cache's edge cases use it too.

use std::cell::Cell;
use std::sync::Arc;

use winit::keyboard::KeyCode;

use crate::core::ecs::kernel::{ChildOf, Children, Entity, World};
use crate::core::input::{InputDevice, PadFamily};
use crate::core::testing::fixtures::settings_tree;
use crate::core::testing::{UiCacheStats, UiHarness};
use crate::core::ui::nodes::*;
use crate::core::ui::{Rect, Theme, UiContext};

/// One scripted step: input or a game write for the next frame, or a frame
#[derive(Clone)]
pub(super) enum Step {
  /// Run one frame (the scenario's `ui`, then what the game does after it)
  Frame,
  /// Move the pointer to (x, y)
  Move(f32, f32),
  /// Press the left button
  Press,
  /// Release the left button
  Release,
  /// A vertical wheel step, in lines (+ scrolls up)
  Wheel(f32),
  /// Press a UI action
  Action(&'static str),
  /// `ui_set` on the tree of the node named `.0`: name `.1` to `.2`
  Set(&'static str, &'static str, BindValue),
  /// The game writes the node named `.0`
  Write(&'static str, fn(&mut World, Entity)),
  /// Draw with this theme from the next frame
  Theme(fn() -> Theme),
  /// Resize the UI
  Resize(f32, f32),
  /// `ui_list_count` of the list named `.0`
  Count(&'static str, usize),
  /// Type text
  Type(&'static str),
  /// Press a key
  Key(KeyCode),
  /// `ui_list_scroll_to` the list named `.0`
  Jump(&'static str, usize),
  /// `ui_list_refresh` the list named `.0`
  Refresh(&'static str),
  /// The player's last device and pad family change, as the engine syncs
  /// them into the prompts
  Device(InputDevice, PadFamily),
  /// The pointer to the centre of the settings tree's control in row `.0`
  Control(usize),
  /// The pointer to the centre of the node named `.0`, if it was drawn
  At(&'static str),
  /// Anything else the game does to the harness
  Do(fn(&mut UiHarness)),
  /// Turn caching on or off in the cached harness (the other stays off)
  Cache(bool),
}

pub(super) use Step::*;

/// A scenario's `ui`: the closure drawn each frame after the node trees,
/// given the frame's number (from 0)
pub(super) type SceneUi<'s> = &'s dyn Fn(&mut UiContext<'_>, &mut World, usize);

/// What the game does after each frame (binding a list's rows, closing a
/// dialog its answer closes)
pub(super) type After<'s> = &'s dyn Fn(&mut UiHarness);

/// What a scenario's cached run did, for its own checks
pub(super) struct Run {
  /// The cached harness, after the last step
  pub(super) on: UiHarness,
  /// The tree nodes it replayed, over every frame
  pub(super) replayed: usize,
  /// The frames run
  pub(super) frames: usize,
  /// What its walks did in each frame
  pub(super) stats: Vec<UiCacheStats>,
}

thread_local! {
  /// While set, the oracle runs with the debug spot check on in both
  /// harnesses, counting here the frames it checked a node in
  static SPOT_CHECKS: Cell<Option<usize>> = const { Cell::new(None) };
}

/// Run `scenarios` with the debug spot check on in both harnesses: every
/// frame must still agree, and no check may find a record that differs from
/// its build. The frames a node was checked in.
#[cfg(debug_assertions)]
pub(super) fn with_spot_checks(scenarios: &[fn()]) -> usize {
  SPOT_CHECKS.with(|checks| checks.set(Some(0)));
  for scenario in scenarios {
    scenario();
  }
  SPOT_CHECKS.with(|checks| checks.take()).unwrap_or(0)
}

/// Run `steps` after `setup` on a harness with caching on and one with it
/// off, and assert after every frame that they show and did the same
pub(super) fn oracle(setup: impl Fn(&mut UiHarness), steps: &[Step]) -> Run {
  oracle_with(setup, &|_, _, _| {}, &|_| {}, steps)
}

/// [`oracle`], with the scenario's `ui` drawn each frame and what the game
/// does `after` each frame
pub(super) fn oracle_with(
  setup: impl Fn(&mut UiHarness),
  ui: SceneUi<'_>,
  after: After<'_>,
  steps: &[Step],
) -> Run {
  let mut on = UiHarness::new(640.0, 480.0);
  let mut off = UiHarness::new(640.0, 480.0);
  on.set_ui_cache(true);
  off.set_ui_cache(false);
  let spot = SPOT_CHECKS.with(Cell::get).is_some();
  on.set_ui_spot_check(spot);
  off.set_ui_spot_check(spot);
  setup(&mut on);
  setup(&mut off);
  let (mut frame, mut replayed, mut stats) = (0, 0, Vec::new());
  let mut since = 0;
  for (i, step) in steps.iter().enumerate() {
    let ran = apply(&mut on, true, step, ui, frame);
    apply(&mut off, false, step, ui, frame);
    if !ran {
      continue;
    }
    let input: Vec<String> = steps[since..i].iter().map(name).collect();
    since = i + 1;
    assert_eq!(
      off.ui_cache_stats().replayed,
      0,
      "caching off replays nothing"
    );
    assert_eq!(
      missed(&on),
      0,
      "step {i} (frame {frame}, after {input:?}): a child missed"
    );
    assert_dirty_ancestors(&on, i);
    assert_eq!(
      spot_differed(&on),
      0,
      "step {i} (frame {frame}, after {input:?}): a record differs from its build"
    );
    let (a, b) = (on.frame_snapshot(), off.frame_snapshot());
    if a != b {
      panic!(
        "step {i} (frame {frame}, after {input:?}): caching on and off differ\n{}",
        first_difference(&a, &b)
      );
    }
    let done = on.ui_cache_stats();
    replayed += done.replayed;
    if done.spot_checked.is_some() {
      SPOT_CHECKS.with(|checks| checks.set(checks.get().map(|n| n + 1)));
    }
    stats.push(done);
    after(&mut on);
    after(&mut off);
    frame += 1;
  }
  Run {
    on,
    replayed,
    frames: frame,
    stats,
  }
}

/// Apply `step` to `h`, the cached harness if `cached`; whether it ran a
/// frame
fn apply(h: &mut UiHarness, cached: bool, step: &Step, ui: SceneUi<'_>, frame: usize) -> bool {
  match step {
    Frame => {
      h.frame_with_world(|context, world| ui(context, world, frame));
      return true;
    }
    Move(x, y) => h.move_to(*x, *y),
    Press => h.press(),
    Release => h.release(),
    Wheel(dy) => h.wheel(0.0, *dy),
    Action(name) => h.action(name),
    Set(node, name, value) => {
      let node = named(h, node);
      h.ui_set(node, name, value.clone());
    }
    Write(node, write) => {
      let node = named(h, node);
      write(h.world_mut(), node);
    }
    Theme(theme) => h.set_theme(theme()),
    Resize(width, height) => h.resize(*width, *height),
    Count(list, count) => {
      let list = named(h, list);
      h.ui_list_count(list, *count);
    }
    Type(text) => h.type_text(text),
    Key(key) => h.keys.push(*key),
    Jump(list, index) => {
      let list = named(h, list);
      h.ui_list_scroll_to(list, *index);
    }
    Refresh(list) => {
      let list = named(h, list);
      h.ui_list_refresh(list);
    }
    Device(device, family) => {
      h.nav.device = *device;
      let prompts = Arc::make_mut(&mut h.prompts);
      prompts.device = *device;
      prompts.family = *family;
      prompts.generation += 1;
    }
    Control(row) => point_at(h, settings_control(h, *row), 0.0, 0.0),
    At(node) => {
      let rect = h
        .find_ui(node)
        .and_then(|node| h.ui_id(node))
        .and_then(|id| h.record_of(id));
      if let Some(record) = rect {
        let rect = record.rect;
        h.move_to(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
      }
    }
    Do(change) => change(h),
    Cache(on) => {
      if cached {
        h.set_ui_cache(*on);
      }
    }
  }
  false
}

/// A step's name, for a failure's message
fn name(step: &Step) -> String {
  match step {
    Frame => "Frame".into(),
    Move(x, y) => format!("Move({x}, {y})"),
    Press => "Press".into(),
    Release => "Release".into(),
    Wheel(dy) => format!("Wheel({dy})"),
    Action(action) => format!("Action({action})"),
    Set(node, name, value) => format!("Set({node}, {name}, {value:?})"),
    Write(node, _) => format!("Write({node})"),
    Theme(_) => "Theme".into(),
    Resize(width, height) => format!("Resize({width}, {height})"),
    Count(list, count) => format!("Count({list}, {count})"),
    Type(text) => format!("Type({text:?})"),
    Key(key) => format!("Key({key:?})"),
    Jump(list, index) => format!("Jump({list}, {index})"),
    Refresh(list) => format!("Refresh({list})"),
    Device(device, family) => format!("Device({device:?}, {family:?})"),
    Control(row) => format!("Control({row})"),
    At(node) => format!("At({node})"),
    Do(_) => "Do".into(),
    Cache(on) => format!("Cache({on})"),
  }
}

/// The node named `name` in `h`'s trees
pub(super) fn named(h: &UiHarness, name: &str) -> Entity {
  h.find_ui(name)
    .unwrap_or_else(|| panic!("no node is named {name}"))
}

/// The first line on which `a` and `b` differ, around the first differing
/// character
fn first_difference(a: &str, b: &str) -> String {
  let (mut lines_a, mut lines_b) = (a.lines(), b.lines());
  let mut line = 0;
  loop {
    match (lines_a.next(), lines_b.next()) {
      (Some(x), Some(y)) if x == y => line += 1,
      (x, y) => {
        let (x, y) = (x.unwrap_or("<none>"), y.unwrap_or("<none>"));
        let at = x
          .char_indices()
          .zip(y.chars())
          .find(|((_, p), q)| p != q)
          .map_or(x.len().min(y.len()), |((i, _), _)| i);
        let from = at.saturating_sub(300);
        let around = |s: &str| -> String { s.chars().skip(from).take(700).collect() };
        let head = |s: &str| -> String { s.chars().take(60).collect() };
        return format!(
          "line {line} ({}..), char {at}\n  on:  ..{}..\n  off: ..{}..",
          head(x),
          around(x),
          around(y)
        );
      }
    }
  }
}

/// Children found unclean in a parent's replay in the last pass
pub(super) fn missed(h: &UiHarness) -> usize {
  h.world()
    .get_resource::<UiNodes>()
    .map_or(0, |nodes| nodes.cache.stats.missed)
}

/// Records the spot check found differing from their build in the last pass
fn spot_differed(h: &UiHarness) -> usize {
  h.world()
    .get_resource::<UiNodes>()
    .map_or(0, |nodes| nodes.cache.stats.spot_differed)
}

/// Every dirty node's ancestors are dirty
fn assert_dirty_ancestors(h: &UiHarness, step: usize) {
  let Some(nodes) = h.world().get_resource::<UiNodes>() else {
    return;
  };
  let dirty = &nodes.cache.dirty;
  for &node in dirty {
    let mut at = node;
    while let Some(parent) = h.world().get::<ChildOf>(at).map(ChildOf::parent) {
      if h.world().get::<UiNodeKind>(parent).is_none() {
        break;
      }
      assert!(
        dirty.contains(&parent),
        "step {step}: {node:?} is dirty under a clean {parent:?}"
      );
      at = parent;
    }
  }
}

// Building scripts

/// `n` frames
pub(super) fn frames(n: usize) -> Vec<Step> {
  vec![Frame; n]
}

/// A script: steps added in order
#[derive(Default)]
pub(super) struct Script(Vec<Step>);

impl Script {
  /// `n` frames
  pub(super) fn frames(mut self, n: usize) -> Script {
    self.0.extend(frames(n));
    self
  }

  /// `step`, then a frame
  pub(super) fn then(mut self, step: Step) -> Script {
    self.0.push(step);
    self.0.push(Frame);
    self
  }

  /// `step` then a frame, `n` times
  pub(super) fn repeat(mut self, step: Step, n: usize) -> Script {
    for _ in 0..n {
      self.0.push(step.clone());
      self.0.push(Frame);
    }
    self
  }

  /// `step` and no frame
  pub(super) fn input(mut self, step: Step) -> Script {
    self.0.push(step);
    self
  }

  pub(super) fn steps(&self) -> &[Step] {
    &self.0
  }
}

// Finding what the steps point at

/// The `i`th root spawned in `h`
pub(super) fn root(h: &UiHarness, i: usize) -> Entity {
  h.world().resource::<UiNodes>().roots[i].1
}

/// `node`'s children
pub(super) fn kids(h: &UiHarness, node: Entity) -> Vec<Entity> {
  h.world()
    .get::<Children>(node)
    .map(|children| children.to_vec())
    .unwrap_or_default()
}

/// `node`'s rect as last drawn
pub(super) fn rect_of(h: &UiHarness, node: Entity) -> Rect {
  let id = h
    .ui_id(node)
    .unwrap_or_else(|| panic!("{node:?} has been drawn"));
  h.record_of(id)
    .unwrap_or_else(|| panic!("{node:?} was drawn last frame"))
    .rect
}

/// Move the pointer to `node`'s centre, moved by (dx, dy)
pub(super) fn point_at(h: &mut UiHarness, node: Entity, dx: f32, dy: f32) {
  let rect = rect_of(h, node);
  h.move_to(
    rect.x + rect.width / 2.0 + dx,
    rect.y + rect.height / 2.0 + dy,
  );
}

/// The settings tree's scroll area (the first root's second child)
pub(super) fn settings_scroll(h: &UiHarness) -> Entity {
  kids(h, root(h, 0))[1]
}

/// The control of the settings tree's row `row`: a slider, checkbox,
/// dropdown or toggle by `row % 4`
pub(super) fn settings_control(h: &UiHarness, row: usize) -> Entity {
  kids(h, kids(h, settings_scroll(h))[row])[1]
}

pub(super) fn spawn_settings(h: &mut UiHarness) {
  h.spawn_ui(settings_tree());
}

/// A scenario's own check on its cached run: it replayed, and so the oracle
/// compared a cached run with an uncached one
pub(super) fn assert_cached(run: &Run) {
  assert!(
    run.replayed > 0,
    "the cached run replayed nothing over {} frames",
    run.frames
  );
}
