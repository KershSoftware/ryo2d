//! The differential oracle: caching is not observable
//!
//! Each scenario runs in two harnesses, the node cache on in one and off in
//! the other, through the same setup and steps. After every frame the two
//! must agree on everything a player or the game can see
//! (`UiHarness::frame_snapshot`: draw commands, records, focus, events,
//! component values), and the cached one must never find a child unclean
//! inside its parent's replay (a miss) or a dirty node under a clean parent.

use std::sync::Arc;

use winit::keyboard::KeyCode;

use crate::core::asset::SpriteHandle;
use crate::core::ecs::kernel::{ChildOf, Children, Entity, World};
use crate::core::graphics::Sprite;
use crate::core::input::{Binding, GamepadButton, InputDevice, PadFamily};
use crate::core::testing::fixtures::settings_tree;
use crate::core::testing::{UiCacheStats, UiHarness};
use crate::core::text::{FontRole, Fonts, TextReveal};
use crate::core::ui::nodes::*;
use crate::core::ui::test_util::prompts_with;
use crate::core::ui::widgets::rebind::note_swap;
use crate::core::ui::{Rect, Theme, UiContext};

/// One scripted step: input or a game write for the next frame, or a frame
#[derive(Clone)]
enum Step {
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
}

use Step::*;

/// A scenario's `ui`: the closure drawn each frame after the node trees,
/// given the frame's number (from 0)
type SceneUi<'s> = &'s dyn Fn(&mut UiContext<'_>, &mut World, usize);

/// What the game does after each frame (binding a list's rows, closing a
/// dialog its answer closes)
type After<'s> = &'s dyn Fn(&mut UiHarness);

/// What a scenario's cached run did, for its own checks
struct Run {
  /// The cached harness, after the last step
  on: UiHarness,
  /// The tree nodes it replayed, over every frame
  replayed: usize,
  /// The frames run
  frames: usize,
  /// What its walks did in each frame
  stats: Vec<UiCacheStats>,
}

/// Run `steps` after `setup` on a harness with caching on and one with it
/// off, and assert after every frame that they show and did the same
fn oracle(setup: impl Fn(&mut UiHarness), steps: &[Step]) -> Run {
  oracle_with(setup, &|_, _, _| {}, &|_| {}, steps)
}

/// [`oracle`], with the scenario's `ui` drawn each frame and what the game
/// does `after` each frame
fn oracle_with(
  setup: impl Fn(&mut UiHarness),
  ui: SceneUi<'_>,
  after: After<'_>,
  steps: &[Step],
) -> Run {
  let mut on = UiHarness::new(640.0, 480.0);
  let mut off = UiHarness::new(640.0, 480.0);
  on.set_ui_cache(true);
  off.set_ui_cache(false);
  setup(&mut on);
  setup(&mut off);
  let (mut frame, mut replayed, mut stats) = (0, 0, Vec::new());
  let mut since = 0;
  for (i, step) in steps.iter().enumerate() {
    let ran = apply(&mut on, step, ui, frame);
    apply(&mut off, step, ui, frame);
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
    let (a, b) = (on.frame_snapshot(), off.frame_snapshot());
    if a != b {
      panic!(
        "step {i} (frame {frame}, after {input:?}): caching on and off differ\n{}",
        first_difference(&a, &b)
      );
    }
    replayed += on.ui_cache_stats().replayed;
    stats.push(on.ui_cache_stats());
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

/// Apply `step` to `h`; whether it ran a frame
fn apply(h: &mut UiHarness, step: &Step, ui: SceneUi<'_>, frame: usize) -> bool {
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
  }
}

/// The node named `name` in `h`'s trees
fn named(h: &UiHarness, name: &str) -> Entity {
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
fn missed(h: &UiHarness) -> usize {
  h.world()
    .get_resource::<UiNodes>()
    .map_or(0, |nodes| nodes.cache.stats.missed)
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
fn frames(n: usize) -> Vec<Step> {
  vec![Frame; n]
}

/// A script: steps added in order
#[derive(Default)]
struct Script(Vec<Step>);

impl Script {
  /// `n` frames
  fn frames(mut self, n: usize) -> Script {
    self.0.extend(frames(n));
    self
  }

  /// `step`, then a frame
  fn then(mut self, step: Step) -> Script {
    self.0.push(step);
    self.0.push(Frame);
    self
  }

  /// `step` then a frame, `n` times
  fn repeat(mut self, step: Step, n: usize) -> Script {
    for _ in 0..n {
      self.0.push(step.clone());
      self.0.push(Frame);
    }
    self
  }

  /// `step` and no frame
  fn input(mut self, step: Step) -> Script {
    self.0.push(step);
    self
  }

  fn steps(&self) -> &[Step] {
    &self.0
  }
}

// Finding what the steps point at

/// The `i`th root spawned in `h`
fn root(h: &UiHarness, i: usize) -> Entity {
  h.world().resource::<UiNodes>().roots[i].1
}

/// `node`'s children
fn kids(h: &UiHarness, node: Entity) -> Vec<Entity> {
  h.world()
    .get::<Children>(node)
    .map(|children| children.to_vec())
    .unwrap_or_default()
}

/// `node`'s rect as last drawn
fn rect_of(h: &UiHarness, node: Entity) -> Rect {
  let id = h
    .ui_id(node)
    .unwrap_or_else(|| panic!("{node:?} has been drawn"));
  h.record_of(id)
    .unwrap_or_else(|| panic!("{node:?} was drawn last frame"))
    .rect
}

/// Move the pointer to `node`'s centre, moved by (dx, dy)
fn point_at(h: &mut UiHarness, node: Entity, dx: f32, dy: f32) {
  let rect = rect_of(h, node);
  h.move_to(
    rect.x + rect.width / 2.0 + dx,
    rect.y + rect.height / 2.0 + dy,
  );
}

/// The settings tree's scroll area (the first root's second child)
fn settings_scroll(h: &UiHarness) -> Entity {
  kids(h, root(h, 0))[1]
}

/// The control of the settings tree's row `row`: a slider, checkbox,
/// dropdown or toggle by `row % 4`
fn settings_control(h: &UiHarness, row: usize) -> Entity {
  kids(h, kids(h, settings_scroll(h))[row])[1]
}

fn spawn_settings(h: &mut UiHarness) {
  h.spawn_ui(settings_tree());
}

/// A scenario's own check on its cached run: it replayed, and so the oracle
/// compared a cached run with an uncached one
fn assert_cached(run: &Run) {
  assert!(
    run.replayed > 0,
    "the cached run replayed nothing over {} frames",
    run.frames
  );
}

// Scenarios

#[test]
fn the_settings_tree_idle_looks_the_same_cached() {
  let run = oracle(spawn_settings, &frames(30));
  assert_cached(&run);
  assert!(run.on.focused().is_some(), "the scope took the focus");
}

#[test]
fn the_settings_tree_walked_by_pad_looks_the_same_cached() {
  let script = Script::default()
    .frames(2)
    // From the tabs down to row 39, up to row 29; Back finds nothing to
    // close
    .repeat(Action("ui.down"), 40)
    .repeat(Action("ui.up"), 10)
    .then(Action("ui.back"))
    .frames(10)
    // Right goes on beside row 29's checkbox, to row 44's slider, which
    // takes Right and Left as steps
    .repeat(Action("ui.right"), 3)
    .then(Action("ui.left"))
    // Row 45's checkbox, toggled
    .then(Action("ui.down"))
    .then(Action("ui.confirm"))
    // Row 46's dropdown: opened, walked, picked; opened again, closed by Back
    .then(Action("ui.down"))
    .then(Action("ui.confirm"))
    .frames(2)
    .repeat(Action("ui.down"), 2)
    .then(Action("ui.confirm"))
    .frames(2)
    .then(Action("ui.confirm"))
    .frames(2)
    .then(Action("ui.back"))
    .frames(2)
    // Row 47's toggle and row 49's checkbox, toggled
    .then(Action("ui.down"))
    .then(Action("ui.confirm"))
    .repeat(Action("ui.down"), 2)
    .then(Action("ui.confirm"))
    // The shoulder buttons switch tabs
    .then(Action("ui.tab_next"))
    .then(Action("ui.tab_next"))
    .then(Action("ui.tab_prev"))
    .then(Action("ui.next"))
    // A focus request (ctx.focus_ui) for a node in a row nothing changed
    .then(Do(|h| {
      let node = settings_control(h, 20);
      h.world_mut().resource_mut::<UiNodes>().pending_focus = Some(node);
    }))
    .frames(20);
  let run = oracle(spawn_settings, script.steps());
  assert_cached(&run);
}

#[test]
fn the_settings_tree_under_the_pointer_looks_the_same_cached() {
  let script = Script::default()
    .frames(3)
    // Across the rows, a frame at each
    .then(Move(120.0, 60.0))
    .then(Move(120.0, 90.0))
    .then(Move(300.0, 120.0))
    .then(Move(300.0, 150.0))
    .then(Move(420.0, 180.0))
    .then(Move(420.0, 210.0))
    .then(Move(1000.0, 1000.0))
    // A press and release on row 3's toggle
    .then(Do(|h| point_at(h, settings_control(h, 3), 0.0, 0.0)))
    .then(Press)
    .then(Release)
    .frames(2)
    // A drag of row 0's slider
    .then(Do(|h| point_at(h, settings_control(h, 0), -40.0, 0.0)))
    .then(Press)
    .then(Do(|h| point_at(h, settings_control(h, 0), 0.0, 0.0)))
    .then(Do(|h| point_at(h, settings_control(h, 0), 60.0, 0.0)))
    .then(Release)
    .frames(2)
    // A click on row 1's checkbox, a double click on row 5's
    .then(Do(|h| point_at(h, settings_control(h, 1), 0.0, 0.0)))
    .then(Press)
    .then(Release)
    .then(Do(|h| point_at(h, settings_control(h, 5), 0.0, 0.0)))
    .then(Press)
    .then(Release)
    .then(Press)
    .then(Release)
    // Row 2's dropdown: open it, hover its options, pick one
    .then(Do(|h| point_at(h, settings_control(h, 2), 0.0, 0.0)))
    .then(Press)
    .then(Release)
    .frames(2)
    .then(Do(|h| point_at(h, settings_control(h, 2), 0.0, 40.0)))
    .then(Do(|h| point_at(h, settings_control(h, 2), 0.0, 70.0)))
    .then(Press)
    .then(Release)
    .frames(2)
    // The wheel over the scroll area, eased
    .then(Do(|h| point_at(h, settings_scroll(h), 0.0, 0.0)))
    .then(Wheel(-3.0))
    .frames(8)
    .then(Wheel(-5.0))
    .frames(4)
    .then(Wheel(2.0))
    .frames(20)
    // A drag of the scrollbar, at the area's right edge
    .then(Do(|h| {
      let area = rect_of(h, settings_scroll(h));
      h.move_to(area.x + area.width - 3.0, area.y + 40.0);
    }))
    .then(Press)
    .then(Do(|h| {
      let area = rect_of(h, settings_scroll(h));
      h.move_to(area.x + area.width - 3.0, area.y + 140.0);
    }))
    .frames(3)
    .then(Do(|h| {
      let area = rect_of(h, settings_scroll(h));
      h.move_to(area.x + area.width - 3.0, area.y + 300.0);
    }))
    .then(Release)
    .frames(20);
  let run = oracle(spawn_settings, script.steps());
  assert_cached(&run);
}

/// A focus scope of bound widgets: a caption and two sliders showing
/// "volume", a checkbox and its caption, a button with a tooltip, a box to
/// hide, a score
fn bound_tree(play: &str) -> UiNode {
  UiNode::column()
    .focus_scope()
    .gap(4.0)
    .name("form")
    .child(UiNode::text("Volume {volume:.0}").bound().name("caption"))
    .child(
      UiNode::slider(0.0..=100.0, 50.0)
        .bind("volume")
        .name("s1")
        .default_focus(),
    )
    .child(
      UiNode::row()
        .name("pair")
        .gap(6.0)
        .child(UiNode::slider(0.0..=100.0, 50.0).bind("volume").name("s2"))
        .child(UiNode::progress(0.5).name("bar")),
    )
    .child(UiNode::checkbox("Music", true).bind("music").name("music"))
    .child(UiNode::text("Music {music}").bound().name("music caption"))
    .child(UiNode::button(play).name("play").tooltip("Start the game"))
    .child(
      UiNode::column()
        .name("box")
        .child(UiNode::button("Inside").name("inside")),
    )
    .child(UiNode::text("Score {score}").bound().name("score"))
}

#[test]
fn bindings_and_game_writes_look_the_same_cached() {
  let script = Script::default()
    .frames(3)
    // A name two sliders and a caption show
    .then(Set("form", "volume", BindValue::Number(30.0)))
    .frames(2)
    .then(Set("form", "volume", BindValue::Number(30.0)))
    .then(Set("form", "score", BindValue::Number(10.0)))
    .then(Set("form", "music", BindValue::Bool(false)))
    // The player steps the focused slider: its sibling drawn after it
    // follows that frame, the caption drawn before it the next
    .repeat(Action("ui.right"), 2)
    .then(Do(|h| point_at(h, named(h, "s1"), -30.0, 0.0)))
    .then(Press)
    .then(Do(|h| point_at(h, named(h, "s1"), 10.0, 0.0)))
    .then(Do(|h| point_at(h, named(h, "s1"), 40.0, 0.0)))
    .then(Release)
    .then(Move(1000.0, 1000.0))
    // The game writes components between frames
    .then(Write("play", |w, e| {
      w.get_mut::<UiLabel>(e).unwrap().0 = "Start".into()
    }))
    .then(Write("bar", |w, e| {
      w.get_mut::<UiValue>(e).unwrap().0 = 0.8
    }))
    .then(Write("s2", |w, e| {
      w.get_mut::<UiValue>(e).unwrap().0 = 20.0
    }))
    .frames(2)
    // A branch hidden, changed while hidden, shown
    .then(Write("box", |w, e| {
      w.entity_mut(e).insert(UiHidden);
    }))
    .then(Write("inside", |w, e| {
      w.get_mut::<UiLabel>(e).unwrap().0 = "Changed".into()
    }))
    .then(Write("box", |w, e| {
      w.entity_mut(e).remove::<UiHidden>();
    }))
    // Disabled and enabled again
    .then(Write("play", |w, e| {
      w.entity_mut(e).insert(UiDisabled);
    }))
    .then(Write("play", |w, e| {
      w.entity_mut(e).remove::<UiDisabled>();
    }))
    // Reparented, despawned, a child added
    .then(Write("inside", |w, _| move_under(w, "inside", "pair")))
    .then(Write("bar", |w, e| {
      w.despawn(e);
    }))
    .then(Write("box", |w, e| {
      spawn_ui_in(w, e, UiNode::button("Added").name("added"));
    }))
    // A subtree moves: its clean child is built in its new id scope
    .then(Write("box", |w, _| move_under(w, "box", "pair")))
    // The game moves the root and widens the second slider through its kind
    .then(Write("form", |w, e| {
      w.get_mut::<UiRoot>(e).unwrap().offset = glam::Vec2::new(30.0, 12.0);
    }))
    .then(Write("s2", |w, e| {
      w.get_mut::<UiNodeKind>(e).unwrap().layout.width = Some(260.0.into());
    }))
    // A tooltip shows over a hovered button
    .then(Write("play", |w, e| {
      w.entity_mut(e).insert(UiTooltip("New tip".into()));
    }))
    .then(Do(|h| point_at(h, named(h, "play"), 0.0, 0.0)))
    .frames(50)
    .then(Move(1000.0, 1000.0))
    // The tree brought in line in place: a label changed
    .then(Do(|h| {
      let form = named(h, "form");
      h.update_ui(form, bound_tree("Begin"));
    }))
    .then(Set("form", "volume", BindValue::Number(75.0)))
    .frames(3);
  let run = oracle(
    |h| {
      h.spawn_ui(bound_tree("Play"));
    },
    script.steps(),
  );
  assert_cached(&run);
  let caption = named(&run.on, "caption");
  let shown = &run.on.world().get::<UiLabel>(caption).unwrap().0;
  assert_eq!(shown, "Volume 75");
}

/// Button `i` of the open confirm: 0 is Yes, 1 No
fn confirm_button(h: &UiHarness, i: usize) -> Entity {
  let dialog = named(h, "confirm");
  let buttons = *kids(h, dialog).last().unwrap();
  kids(h, buttons)[i]
}

/// Close the confirm when one of its buttons was clicked, as a game would
fn close_answered(h: &mut UiHarness) {
  let answered = h
    .events()
    .iter()
    .any(|e| e.kind == UiEventKind::Clicked && matches!(e.action.as_deref(), Some("yes" | "no")));
  if answered && let Some(dialog) = h.find_ui("confirm") {
    h.world_mut().despawn(dialog);
  }
}

fn open_confirm(h: &mut UiHarness) {
  h.spawn_ui(UiNode::confirm("Quit?", "Your progress is saved.", "Yes", "No").name("confirm"));
}

#[test]
fn a_modal_over_the_settings_tree_looks_the_same_cached() {
  let script = Script::default()
    .frames(3)
    .repeat(Action("ui.down"), 2)
    .then(Do(open_confirm))
    .frames(3)
    .then(Action("ui.left"))
    .then(Action("ui.right"))
    .then(Action("ui.left"))
    // Back answers No, which closes it; the focus goes back to the page
    .then(Action("ui.back"))
    .frames(4)
    .then(Action("ui.down"))
    .then(Do(open_confirm))
    .frames(3)
    // The pointer: over the dim, a press there, then Yes clicked
    .then(Move(20.0, 20.0))
    .then(Press)
    .then(Release)
    .then(Do(|h| point_at(h, confirm_button(h, 0), 0.0, 0.0)))
    .then(Press)
    .then(Release)
    .frames(4)
    .then(Action("ui.down"))
    .frames(3)
    // A note: nothing in it holds a role, so it replays until Back closes it
    .then(Move(1000.0, 1000.0))
    .then(Do(|h| {
      h.spawn_ui(
        UiNode::panel()
          .modal()
          .action("note")
          .name("note")
          .child(UiNode::text("Saved")),
      );
    }))
    .frames(4)
    .then(Action("ui.back"))
    .frames(3);
  let run = oracle_with(
    spawn_settings,
    &|_, _, _| {},
    &|h| {
      close_answered(h);
      let back = h.events().iter().any(|e| e.kind == UiEventKind::Back);
      if back && let Some(note) = h.find_ui("note") {
        h.world_mut().despawn(note);
      }
    },
    script.steps(),
  );
  assert_cached(&run);
  assert!(run.on.find_ui("confirm").is_none(), "answered and closed");
  assert!(run.on.find_ui("note").is_none(), "Back closed the note");
}

/// A name field bound to a greeting, a password field and a button in a
/// focus scope, and a field outside any scope
fn form() -> UiNode {
  UiNode::column()
    .focus_scope()
    .gap(4.0)
    .anchor(crate::core::ui::Anchor::TopLeft)
    .child(UiNode::text("Name"))
    .child(
      UiNode::text_input("")
        .placeholder("Your name")
        .max_len(12)
        .bind("name")
        .name("field"),
    )
    .child(UiNode::text("Hello {name}").bound().name("greeting"))
    .child(UiNode::text_input("secret").password().name("pass"))
    .child(UiNode::button("OK").name("ok"))
}

#[test]
fn a_text_field_looks_the_same_cached() {
  let script = Script::default()
    .frames(3)
    .then(Type("Ryo"))
    .then(Type("2D"))
    // The caret blinks
    .frames(70)
    .then(Key(KeyCode::Backspace))
    .then(Key(KeyCode::ArrowLeft))
    .then(Type("x"))
    .then(Key(KeyCode::Home))
    .then(Do(|h| h.shift = true))
    .then(Key(KeyCode::ArrowRight))
    .then(Key(KeyCode::ArrowRight))
    .then(Do(|h| h.shift = false))
    .then(Type("Z"))
    .then(Key(KeyCode::End))
    .then(Type("abcdefghijklmnop"))
    .then(Key(KeyCode::Enter))
    // Escape leaves it; typing then goes nowhere
    .then(Key(KeyCode::Escape))
    .then(Type("ignored"))
    .then(Action("ui.down"))
    .then(Action("ui.down"))
    .then(Type("!"))
    // The field outside any scope takes the keys by a press
    .then(Do(|h| point_at(h, named(h, "loose"), 0.0, 0.0)))
    .then(Press)
    .then(Release)
    .then(Type("free"))
    .then(Key(KeyCode::Enter))
    .then(Move(1000.0, 1000.0))
    .frames(5);
  let run = oracle(
    |h| {
      h.spawn_ui(form());
      h.spawn_ui(
        UiNode::column()
          .anchor(crate::core::ui::Anchor::BottomRight)
          .child(UiNode::text_input("").name("loose")),
      );
    },
    script.steps(),
  );
  assert_cached(&run);
  let field = named(&run.on, "field");
  let text = &run.on.world().get::<UiTextValue>(field).unwrap().0;
  assert_eq!(text, "Zox2abcdefgh", "typed, edited and cut at 12");
  let loose = named(&run.on, "loose");
  assert_eq!(run.on.world().get::<UiTextValue>(loose).unwrap().0, "free");
}

/// A focus scope holding a title and a list of `count` rows, each a button
/// and a caption bound by name, and a checkbox bound to a caption after it
fn board(count: usize) -> UiNode {
  let row = || {
    UiNode::row()
      .gap(6.0)
      .child(UiNode::button("{name}").bound().name("pick"))
      .child(UiNode::text("#{n}").bound())
      .child(UiNode::checkbox("On", false).bind("on").name("on"))
      .child(UiNode::text("{on}").bound())
  };
  UiNode::column()
    .focus_scope()
    .gap(4.0)
    .child(UiNode::text("Board").name("title"))
    .child(
      UiNode::list(count, 24.0)
        .height(240.0)
        .width(300.0)
        .row_shape(row)
        .name("board"),
    )
}

/// Bind every row the last frame asked for, as a game would
fn bind_rows(h: &mut UiHarness) {
  let binds: Vec<UiListBind> = h.messages::<UiListBind>().collect();
  for bind in binds {
    h.ui_set(bind.row, "name", format!("Item {}", bind.index));
    h.ui_set(bind.row, "n", bind.index);
    h.ui_set(bind.row, "on", bind.index % 3 == 0);
  }
}

/// The pooled row of the list named `list` holding its `k`th item in the
/// pool's order
fn pooled(h: &UiHarness, list: &str, k: usize) -> Entity {
  let list = named(h, list);
  h.world().get::<super::list::UiList>(list).unwrap().rows[k].1
}

#[test]
fn a_node_list_of_a_thousand_rows_looks_the_same_cached() {
  let script = Script::default()
    .frames(3)
    .then(Do(|h| point_at(h, named(h, "board"), 0.0, 0.0)))
    .then(Wheel(-3.0))
    .frames(10)
    .then(Wheel(-10.0))
    .frames(10)
    .then(Jump("board", 500))
    .frames(30)
    .then(Jump("board", 999))
    .frames(30)
    .then(Count("board", 400))
    .frames(10)
    .then(Refresh("board"))
    .frames(5)
    .then(Count("board", 1000))
    .frames(3)
    .then(Do(|h| {
      let list = named(h, "board");
      h.ui_list_refresh_row(list, 395);
    }))
    .then(Jump("board", 0))
    .frames(30)
    // The pad walks into the list
    .repeat(Action("ui.down"), 6)
    .then(Do(|h| {
      let list = named(h, "board");
      h.ui_list_focus(list, 200);
    }))
    .frames(20)
    .repeat(Action("ui.down"), 3)
    .then(Action("ui.confirm"))
    // A game write to a pooled row, and a click on a row
    .then(Do(|h| {
      let row = pooled(h, "board", 2);
      h.ui_set(row, "name", "Changed");
    }))
    .then(Do(|h| point_at(h, named(h, "board"), -40.0, 30.0)))
    .then(Press)
    .then(Release)
    // The game writes a row's bound checkbox itself: its caption follows
    // as the row is drawn
    .then(Do(|h| {
      let row = pooled(h, "board", 3);
      let check = kids(h, row)[2];
      let on = h.world().get::<UiChecked>(check).unwrap().0;
      h.world_mut().get_mut::<UiChecked>(check).unwrap().0 = !on;
    }))
    .then(Count("board", 3))
    .frames(10)
    .then(Count("board", 0))
    .frames(3)
    .then(Count("board", 1000))
    .frames(5);
  let run = oracle_with(
    |h| {
      h.spawn_ui(board(1000));
    },
    &|_, _, _| {},
    &bind_rows,
    script.steps(),
  );
  assert_cached(&run);
}

/// A prompt for "jump", and a row that rebinds it
fn controls() -> UiNode {
  UiNode::column()
    .focus_scope()
    .gap(4.0)
    .anchor(crate::core::ui::Anchor::BottomLeft)
    .child(UiNode::prompt("jump"))
    .child(UiNode::rebind("jump").name("rebind"))
    .child(UiNode::text("Press [prompt=jump] to jump").markup())
}

fn with_prompts(h: &mut UiHarness) {
  h.prompts = Arc::new(prompts_with(
    InputDevice::Keyboard,
    &[(
      "jump",
      vec![
        Binding::Key(KeyCode::Space),
        Binding::Pad(GamepadButton::South),
      ],
    )],
  ));
}

#[test]
fn theme_scale_and_device_changes_look_the_same_cached() {
  let script = Script::default()
    .frames(3)
    .then(Device(InputDevice::Keyboard, PadFamily::Xbox))
    .then(Action("ui.down"))
    .frames(2)
    .then(Theme(Theme::light))
    .frames(3)
    .then(Theme(Theme::pixel))
    .frames(3)
    .then(Resize(800.0, 600.0))
    .frames(3)
    .then(Resize(500.0, 400.0))
    .frames(3)
    .then(Device(InputDevice::Gamepad, PadFamily::PlayStation))
    .frames(3)
    .then(Device(InputDevice::Gamepad, PadFamily::Nintendo))
    .then(Action("ui.down"))
    .then(Device(InputDevice::Mouse, PadFamily::Nintendo))
    // The UI font changes: the mono font takes the UI role, then goes back
    .then(Do(|h| {
      let mut fonts = Fonts::with_builtins();
      if let Some(mono) = fonts.role(FontRole::Mono) {
        fonts.set_role(FontRole::Ui, mono);
      }
      h.text.sync(&fonts);
    }))
    .frames(2)
    .then(Do(|h| h.text.sync(&Fonts::with_builtins())))
    .then(Theme(Theme::dark))
    .then(Resize(640.0, 480.0))
    // The rebinding row captures a key and shows its note
    .then(Do(|h| point_at(h, named(h, "rebind"), 0.0, 0.0)))
    .then(Press)
    .then(Release)
    .frames(2)
    .then(Key(KeyCode::KeyR))
    // The capture swapped with another action, as the engine notes it;
    // the focus and the pointer leave the row while its note shows
    .then(Move(1000.0, 1000.0))
    .then(Action("ui.up"))
    .frames(130);
  let run = oracle_with(
    |h| {
      with_prompts(h);
      spawn_settings(h);
      h.spawn_ui(controls());
    },
    &|_, _, _| {},
    &|h| {
      if let Some(rebind) = h.rebinds.first().cloned() {
        let now = h.memory.interact.now;
        note_swap(&mut h.memory, &rebind, "dash", now);
      }
    },
    script.steps(),
  );
  assert_cached(&run);
}

fn icon(r: u8, g: u8, b: u8) -> SpriteHandle {
  SpriteHandle::from_sprite(
    Sprite::solid_rect(16, 16, r, g, b, 255),
    format!("oracle-icon-{r}-{g}-{b}"),
  )
}

/// An inventory: a grid of eight slots in a focus scope, three holding
/// items, every one a drag source and all but the last a drop target
fn inventory() -> UiNode {
  let slot = |i: usize| {
    let mut slot = UiNode::slot().drag_source().name(format!("s{i}"));
    if i < 7 {
      slot = slot.drop_target();
    }
    match i {
      0 => slot.icon(icon(200, 50, 50)).count(3).default_focus(),
      2 => slot.icon(icon(50, 200, 50)),
      5 => slot.icon(icon(50, 50, 200)).count(12),
      _ => slot,
    }
  };
  UiNode::grid(4)
    .focus_scope()
    .gap(6.0)
    .anchor(crate::core::ui::Anchor::Center)
    .children((0..8).map(slot))
}

/// Move what a drop carried into the slot it was dropped on, swapping, as
/// an inventory game would
fn move_dropped(h: &mut UiHarness) {
  let drops: Vec<(Entity, Entity)> = h
    .events()
    .iter()
    .filter_map(|e| match e.kind {
      UiEventKind::Dropped { source } => Some((source, e.node)),
      _ => None,
    })
    .collect();
  for (source, target) in drops {
    let world = h.world_mut();
    let from = world.get::<UiSlot>(source).cloned().unwrap_or_default();
    let to = world.get::<UiSlot>(target).cloned().unwrap_or_default();
    *world.get_mut::<UiSlot>(source).unwrap() = to;
    *world.get_mut::<UiSlot>(target).unwrap() = from;
  }
}

#[test]
fn drag_and_drop_in_an_inventory_looks_the_same_cached() {
  let at = |name: &'static str| -> fn(&mut UiHarness) {
    match name {
      "s0" => |h| point_at(h, named(h, "s0"), 0.0, 0.0),
      "s1" => |h| point_at(h, named(h, "s1"), 0.0, 0.0),
      "s2" => |h| point_at(h, named(h, "s2"), 0.0, 0.0),
      "s3" => |h| point_at(h, named(h, "s3"), 0.0, 0.0),
      "s4" => |h| point_at(h, named(h, "s4"), 0.0, 0.0),
      "s7" => |h| point_at(h, named(h, "s7"), 0.0, 0.0),
      _ => |h| point_at(h, named(h, "s0"), 6.0, 6.0),
    }
  };
  let script = Script::default()
    .frames(3)
    // The mouse carries s0's item over several targets and drops it on s4
    .then(Do(at("s0")))
    .then(Press)
    .then(Do(at("s0+")))
    .then(Do(at("s1")))
    .then(Do(at("s3")))
    .then(Do(at("s1")))
    .then(Do(at("s7")))
    .then(Move(20.0, 20.0))
    .then(Move(40.0, 400.0))
    .then(Do(at("s4")))
    .then(Release)
    .frames(2)
    // Picked up, released over nothing: no drop
    .then(Do(at("s2")))
    .then(Press)
    .then(Move(10.0, 10.0))
    .then(Release)
    .frames(2)
    // The pad: picked up, carried, cancelled with Back
    .then(Action("ui.right"))
    .then(Action("ui.right"))
    .then(Action("ui.confirm"))
    .then(Action("ui.right"))
    .then(Action("ui.down"))
    .then(Action("ui.back"))
    .frames(2)
    // Picked up again and dropped on a target
    .then(Action("ui.up"))
    .then(Action("ui.confirm"))
    .then(Action("ui.left"))
    .then(Action("ui.confirm"))
    .frames(3);
  let run = oracle_with(
    |h| {
      h.spawn_ui(inventory());
    },
    &|_, _, _| {},
    &move_dropped,
    script.steps(),
  );
  assert_cached(&run);
  let full: Vec<&str> = ["s0", "s1", "s2", "s3", "s4", "s5", "s6", "s7"]
    .into_iter()
    .filter(|name| {
      let slot = named(&run.on, name);
      run.on.world().get::<UiSlot>(slot).unwrap().icon.is_some()
    })
    .collect();
  assert_ne!(full, ["s0", "s2", "s5"], "items were moved");
}

/// A small xorshift: the random run is the same on every machine
struct XorShift(u64);

impl XorShift {
  fn next(&mut self) -> u64 {
    let mut x = self.0;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    self.0 = x;
    x
  }

  fn below(&mut self, n: u64) -> u64 {
    self.next() % n
  }

  fn pick<T: Copy>(&mut self, of: &[T]) -> T {
    of[self.below(of.len() as u64) as usize]
  }
}

/// A HUD over the settings tree: a score caption and a bar bound by name
fn hud() -> UiNode {
  UiNode::column()
    .anchor(crate::core::ui::Anchor::TopLeft)
    .name("hud")
    .child(UiNode::text("Score {score}").bound())
    .child(UiNode::progress(0.5).bind("hp"))
}

/// `frames` frames of random input over the settings tree and a HUD
fn random_steps(seed: u64, frames: usize) -> Vec<Step> {
  const ACTIONS: [&str; 9] = [
    "ui.up",
    "ui.down",
    "ui.left",
    "ui.right",
    "ui.confirm",
    "ui.back",
    "ui.next",
    "ui.tab_next",
    "ui.tab_prev",
  ];
  const THEMES: [fn() -> Theme; 3] = [Theme::light, Theme::pixel, Theme::dark];
  let mut rng = XorShift(seed);
  let mut steps = Vec::new();
  for frame in 0..frames {
    match rng.below(100) {
      0..20 => steps.push(Move(rng.below(640) as f32, rng.below(480) as f32)),
      20..30 => steps.push(Control(rng.below(50) as usize)),
      30..36 => steps.push(Press),
      36..42 => steps.push(Release),
      42..48 => steps.push(Wheel(rng.pick(&[-3.0, -2.0, -1.0, 1.0, 2.0, 3.0]))),
      48..56 => steps.push(Action(rng.pick(&["ui.up", "ui.down"]))),
      56..70 => steps.push(Action(rng.pick(&ACTIONS))),
      70..72 => {
        let score = BindValue::Number(frame as f64);
        steps.push(Set("hud", "score", score));
      }
      72..73 => {
        let hp = BindValue::Number(rng.below(100) as f64 / 100.0);
        steps.push(Set("hud", "hp", hp));
      }
      _ => {}
    }
    if rng.below(400) == 0 {
      steps.push(Theme(rng.pick(&THEMES)));
    }
    steps.push(Frame);
  }
  steps
}

#[test]
fn three_thousand_frames_of_random_input_look_the_same_cached() {
  let steps = random_steps(0x5eed_cafe_f00d_d00d, 3000);
  let run = oracle(
    |h| {
      spawn_settings(h);
      h.spawn_ui(hud());
    },
    &steps,
  );
  assert_eq!(run.frames, 3000);
  assert_cached(&run);
}

/// The scene tree beside the embedded ones: a column of buttons, no focus
/// scope
fn menu() -> UiNode {
  UiNode::column()
    .gap(4.0)
    .anchor(crate::core::ui::Anchor::TopLeft)
    .child(UiNode::text("Menu").name("title"))
    .child(UiNode::button("Play").name("play"))
    .child(
      UiNode::row()
        .name("pair")
        .child(UiNode::button("A").name("a"))
        .child(UiNode::button("B").name("b")),
    )
}

/// The embedded tree named `name` (drawn by the scenario's `ui`)
fn embedded(world: &World, name: &str) -> Entity {
  find_ui(world, 1, name).unwrap_or_else(|| panic!("no node is named {name}"))
}

#[test]
fn an_embedded_tree_drawn_from_two_call_sites_in_turn_looks_the_same_cached() {
  // Every other two frames the closure draws the tree from another
  // unkeyed column: the column's id, and so the tree's id scope, changes
  let ui = |ui: &mut UiContext<'_>, world: &mut World, frame: usize| {
    let root = embedded(world, "hud");
    if frame == 9 {
      let hp = embedded(world, "hp");
      world.get_mut::<UiLabel>(hp).unwrap().0 = "Hp 9".into();
    }
    if (frame / 2).is_multiple_of(2) {
      ui.column().show(|ui| {
        ui.node(root, world);
      });
    } else {
      ui.column().gap(2.0).show(|ui| {
        ui.node(root, world);
      });
    }
  };
  let script = Script::default()
    .frames(12)
    .then(Do(|h| point_at(h, named(h, "ea"), 0.0, 0.0)))
    .frames(3)
    .then(Press)
    .then(Release)
    .then(Move(1000.0, 1000.0))
    .then(Write("hp", |w, e| {
      w.get_mut::<UiLabel>(e).unwrap().0 = "Hp 7".into()
    }))
    .frames(6);
  let run = oracle_with(
    |h| {
      h.spawn_ui(menu());
      h.spawn_ui(
        UiNode::column()
          .embedded()
          .gap(4.0)
          .name("hud")
          .child(UiNode::text("Hp").name("hp"))
          .child(
            UiNode::row()
              .name("bar")
              .child(UiNode::button("A").name("ea"))
              .child(UiNode::button("B").name("eb")),
          ),
      );
    },
    &ui,
    &|_| {},
    script.steps(),
  );
  assert_cached(&run);
  let hud = named(&run.on, "hud");
  let built = |frame: usize| run.stats[frame].rebuilt.contains(&hud);
  assert!(
    built(4) && built(6),
    "a new call site builds the tree again"
  );
  assert!(!built(5) && !built(7), "the same call site replays it");
}

/// The embedded board: a focus scope holding a list of 200 bound rows
fn embedded_board() -> UiNode {
  let row = || {
    UiNode::row()
      .gap(6.0)
      .child(UiNode::button("{name}").bound().name("pick"))
      .child(UiNode::text("#{n}").bound().name("caption"))
  };
  UiNode::column()
    .embedded()
    .focus_scope()
    .gap(4.0)
    .name("hud")
    .child(UiNode::text("Board"))
    .child(
      UiNode::list(200, 24.0)
        .height(120.0)
        .width(240.0)
        .row_shape(row)
        .name("elist"),
    )
}

/// The embedded tree named "hud" drawn in a closure column at a fixed rect
fn draw_hud(ui: &mut UiContext<'_>, world: &mut World) {
  let root = embedded(world, "hud");
  ui.column()
    .id("hud")
    .at(Rect::new(300.0, 100.0, 260.0, 200.0))
    .show(|ui| {
      ui.node(root, world);
    });
}

#[test]
fn an_embedded_list_beside_a_scene_tree_looks_the_same_cached() {
  let ui = |ui: &mut UiContext<'_>, world: &mut World, frame: usize| {
    if frame == 36 {
      let list = embedded(world, "elist");
      let row = world.get::<super::list::UiList>(list).unwrap().rows[3].1;
      let pick = world.get::<Children>(row).unwrap()[0];
      world.resource_mut::<UiNodes>().pending_focus = Some(pick);
    }
    // The closure writes a row's caption before the tree is drawn
    if frame == 30 {
      let list = embedded(world, "elist");
      let row = world.get::<super::list::UiList>(list).unwrap().rows[1].1;
      let caption = world.get::<Children>(row).unwrap()[1];
      world.get_mut::<UiLabel>(caption).unwrap().0 = "Written".into();
    }
    draw_hud(ui, world);
  };
  let script = Script::default()
    .frames(4)
    .then(Do(|h| point_at(h, named(h, "elist"), 0.0, 0.0)))
    .then(Wheel(-3.0))
    .frames(6)
    .then(Move(630.0, 470.0))
    .then(Refresh("elist"))
    .frames(3)
    .then(Do(|h| {
      let row = pooled(h, "elist", 2);
      h.ui_set(row, "name", "Changed");
    }))
    .frames(2)
    .repeat(Action("ui.down"), 4)
    .then(Do(|h| point_at(h, named(h, "elist"), -40.0, 20.0)))
    .frames(3)
    .then(Move(630.0, 470.0))
    .frames(12)
    .then(Do(|h| point_at(h, named(h, "play"), 0.0, 0.0)))
    .then(Press)
    .then(Release)
    .frames(3);
  let run = oracle_with(
    |h| {
      h.spawn_ui(menu());
      h.spawn_ui(embedded_board());
    },
    &ui,
    &bind_rows,
    script.steps(),
  );
  assert_cached(&run);
  let list = named(&run.on, "elist");
  let rows_replayed = run.stats.iter().any(|stats| {
    let pool = &run
      .on
      .world()
      .get::<super::list::UiList>(list)
      .unwrap()
      .rows;
    stats.rebuilt.contains(&list) && pool.iter().any(|(_, row)| !stats.rebuilt.contains(row))
  });
  assert!(rows_replayed, "the embedded list's rows replay");
}

#[test]
fn a_rerun_pass_with_an_embedded_tree_looks_the_same_cached() {
  // The scene's `ui` draws the embedded tree, writes labels in both trees
  // and switches scenes, and the pass runs again in the same frame
  let ui = |ui: &mut UiContext<'_>, world: &mut World, frame: usize| {
    draw_hud(ui, world);
    if !matches!(frame, 5 | 9 | 12 | 16) {
      return;
    }
    let (hp, go) = (embedded(world, "hp"), embedded(world, "go"));
    world.get_mut::<UiLabel>(hp).unwrap().0 = format!("Hp {frame}");
    world.get_mut::<UiLabel>(go).unwrap().0 = format!("Go {frame}");
    let shelf = embedded(world, "shelf");
    match frame {
      9 => {
        world.entity_mut(shelf).insert(UiHidden);
      }
      // Hidden, and its text changed, before this frame: the first pass
      // draws it hidden, and it shows again before the frame ends
      12 | 16 => {
        world.entity_mut(shelf).remove::<UiHidden>();
      }
      _ => {}
    }
    let scene = ui.scene().0;
    ui.end_scene();
    ui.memory.restart_pass();
    ui.begin_scene(scene, true);
    begin_pass(world);
    let events = draw_nodes(world, ui, scene);
    deliver(world, events);
    draw_hud(ui, world);
  };
  let script = Script::default()
    .frames(8)
    .then(Do(|h| point_at(h, named(h, "go"), 0.0, 0.0)))
    .frames(2)
    .then(Move(630.0, 470.0))
    .frames(4)
    .input(Write("shelf", |w, e| {
      w.entity_mut(e).insert(UiHidden);
    }))
    .input(Write("shelf text", |w, e| {
      w.get_mut::<UiLabel>(e).unwrap().0 = "Moved".into();
    }))
    .frames(5);
  let run = oracle_with(
    |h| {
      h.spawn_ui(
        UiNode::column()
          .anchor(crate::core::ui::Anchor::TopLeft)
          .child(UiNode::button("Go").name("go"))
          .child(
            UiNode::column()
              .name("shelf")
              .child(UiNode::text("Shelf").name("shelf text")),
          ),
      );
      h.spawn_ui(
        UiNode::column()
          .embedded()
          .name("hud")
          .child(UiNode::text("Hp").name("hp"))
          .child(UiNode::button("Use").name("use")),
      );
    },
    &ui,
    &|_| {},
    script.steps(),
  );
  assert_cached(&run);
  let hp = named(&run.on, "hp");
  assert_eq!(run.on.world().get::<UiLabel>(hp).unwrap().0, "Hp 16");
  assert!(run.on.frame_snapshot().contains("\"Moved\""), "drawn");
}

/// What a node reads of other nodes, each reader left holding no role (no
/// focus: the popup traps it and holds nothing focusable; the pointer away):
/// a popup's anchor, a named neighbour, and a filling text field's own last
/// rect
fn readers() -> UiNode {
  let rows = (0..12).map(|i| UiNode::button(format!("Row {i}")).name(format!("r{i}")));
  UiNode::column()
    .focus_scope()
    .gap(4.0)
    .anchor(crate::core::ui::Anchor::TopLeft)
    .child(
      UiNode::row()
        .name("top row")
        .child(UiNode::button("First").name("first").nav_down("far"))
        .child(UiNode::button("Second").name("second").default_focus()),
    )
    .child(
      UiNode::scroll()
        .height(120.0)
        .width(220.0)
        .name("area")
        .child(UiNode::button("More").name("more"))
        .children(rows),
    )
    .child(
      UiNode::popup("more")
        .name("popup")
        .child(UiNode::text("Nothing to pick here")),
    )
    .child(
      UiNode::column()
        .name("shelf")
        .child(UiNode::button("Far").name("far")),
    )
    .child(UiNode::column().name("other shelf"))
    .child(
      UiNode::row()
        .width(320.0)
        .gap(4.0)
        .name("field row")
        .child(UiNode::button("Go").name("go"))
        .child(
          UiNode::text_input("a line far too long to fit in the field it is typed in")
            .fill_width()
            .name("wide"),
        ),
    )
}

/// Move the node named `node` under the node named `parent`
fn move_under(w: &mut World, node: &str, parent: &str) {
  let (node, parent) = (find_ui(w, 1, node).unwrap(), find_ui(w, 1, parent).unwrap());
  w.entity_mut(node).insert(ChildOf(parent));
}

#[test]
fn reads_of_other_nodes_look_the_same_cached() {
  let script = Script::default()
    .frames(3)
    // The popup's anchor scrolls under it, then moves to another container
    // (another id): the popup follows it
    .then(Do(|h| point_at(h, named(h, "area"), 0.0, 30.0)))
    .then(Wheel(-1.0))
    .then(Move(630.0, 470.0))
    .frames(10)
    .then(Write("more", |w, _| move_under(w, "more", "other shelf")))
    .frames(3)
    // The named neighbour moves: its id changes, and the node naming it
    // records the new one
    .then(Write("far", |w, _| move_under(w, "far", "other shelf")))
    .frames(2)
    // The field's sibling grows: the field, sized by its last rect, follows
    .then(Write("go", |w, e| {
      w.get_mut::<UiLabel>(e).unwrap().0 = "Go further, much further".into();
    }))
    .frames(3)
    // The popup closes; the pad goes down from the neighbour's reader
    .then(Write("popup", |w, e| {
      w.entity_mut(e).insert(UiHidden);
    }))
    .frames(2)
    .then(Do(|h| {
      let first = named(h, "first");
      if let Some(id) = h.ui_id(first) {
        h.memory.request_focus(id);
      }
    }))
    .then(Action("ui.down"))
    .frames(2)
    .then(Write("popup", |w, e| {
      w.entity_mut(e).remove::<UiHidden>();
    }))
    .frames(3);
  let run = oracle(
    |h| {
      h.spawn_ui(readers());
      h.move_to(630.0, 470.0);
    },
    script.steps(),
  );
  assert_cached(&run);
  let first = named(&run.on, "first");
  let built = |frame: usize| run.stats[frame].rebuilt.contains(&first);
  assert!(
    !built(3),
    "the neighbour's reader replays while nothing it reads moves"
  );
}

/// A dialogue: a line revealed by a typewriter over three choices
fn dialogue() -> UiNode {
  UiNode::column()
    .focus_scope()
    .gap(6.0)
    .anchor(crate::core::ui::Anchor::BottomLeft)
    .child(
      UiNode::text("Hello traveller. What brings you to [b]Ryo[/b]?")
        .markup()
        .typewriter(
          TextReveal::new(30.0)
            .with_fade(0.2)
            .with_punctuation_pause(0.15),
        )
        .wrap(300.0)
        .name("line"),
    )
    .child(UiNode::button("Skip").name("skip").default_focus())
    .child(UiNode::choice_list(&["Trade", "Talk", "Leave"]).name("choices"))
}

#[test]
fn a_dialogue_with_a_typewriter_and_number_keys_looks_the_same_cached() {
  let script = Script::default()
    .frames(70)
    .then(Key(KeyCode::Digit2))
    .then(Write("line", |w, e| {
      w.get_mut::<UiLabel>(e).unwrap().0 = "Then let us talk, friend.".into();
      w.get_mut::<TextReveal>(e).unwrap().restart();
    }))
    .frames(20)
    .then(Write("line", |w, e| {
      w.get_mut::<TextReveal>(e).unwrap().finish()
    }))
    .frames(3)
    .then(Action("ui.down"))
    .then(Action("ui.confirm"))
    .then(Key(KeyCode::Digit3))
    .then(Key(KeyCode::Digit9))
    // The last choice, at the column's foot (a choice list has no id)
    .then(Do(|h| {
      let column = rect_of(h, root(h, 0));
      h.move_to(column.x + 20.0, column.y + column.height - 8.0);
    }))
    .then(Press)
    .then(Release)
    .frames(3)
    // The whole tree hidden (frames that draw nothing keep every record),
    // changed while hidden, shown again
    .then(Do(|h| {
      let dialogue = root(h, 0);
      h.world_mut().entity_mut(dialogue).insert(UiHidden);
    }))
    .then(Write("skip", |w, e| {
      w.get_mut::<UiLabel>(e).unwrap().0 = "Skip all".into();
    }))
    .then(Do(|h| {
      let dialogue = root(h, 0);
      h.world_mut().entity_mut(dialogue).remove::<UiHidden>();
    }))
    .frames(3);
  let run = oracle(
    |h| {
      h.spawn_ui(dialogue());
    },
    script.steps(),
  );
  assert_cached(&run);
}

/// Spawn `tree` as a root of scene `scene`
fn spawn_in_scene(h: &mut UiHarness, scene: u64, tree: UiNode) -> Entity {
  spawn_ui(h.world_mut(), scene, tree)
}

#[test]
fn two_scenes_drawn_in_one_frame_look_the_same_cached() {
  // The closure draws the other scene's trees as a lower scene, after the
  // harness's own (the top); the steps swap which scene is on top
  let ui = |ui: &mut UiContext<'_>, world: &mut World, _: usize| {
    let top = ui.scene().0;
    let other = if top == 1 { 2 } else { 1 };
    ui.end_scene();
    ui.begin_scene(other, false);
    let events = draw_nodes(world, ui, other);
    deliver(world, events);
    ui.end_scene();
    ui.begin_scene(top, true);
  };
  let script = Script::default()
    .frames(3)
    .repeat(Action("ui.down"), 2)
    .then(Do(|h| point_at(h, named(h, "play"), 0.0, 0.0)))
    .then(Do(|h| h.scene = 2))
    .frames(3)
    .then(Write("form", |w, e| {
      w.entity_mut(e).insert(UiDisabled);
    }))
    .then(Do(|h| {
      let play = find_ui(h.world(), 1, "play").unwrap();
      h.world_mut().get_mut::<UiLabel>(play).unwrap().0 = "Played".into();
    }))
    .repeat(Action("ui.down"), 2)
    .then(Do(|h| h.scene = 1))
    .frames(3)
    .then(Move(1000.0, 1000.0))
    .frames(3);
  let run = oracle_with(
    |h| {
      spawn_in_scene(h, 1, menu().focus_scope());
      spawn_in_scene(h, 2, bound_tree("Play"));
    },
    &ui,
    &|_| {},
    script.steps(),
  );
  assert_cached(&run);
}

const MENU_FILE: &str = r#"(root: Panel(focus_scope: true, gap: 4, children: [
  Text(text: "Menu", name: "title"),
  Button(label: "Play", name: "play", default_focus: true),
  Button(label: "Quit", name: "quit"),
]))"#;

#[test]
fn a_reloaded_ui_file_looks_the_same_cached() {
  let dir = tempfile::tempdir().unwrap();
  let path = dir.path().join("menu.ui.ron");
  std::fs::write(&path, MENU_FILE).unwrap();
  // The file changes on disk and is read again; the next frame brings the
  // tree in line (a label, a gap, a new button; then a kind)
  let ui = |_: &mut UiContext<'_>, world: &mut World, frame: usize| {
    let text = match frame {
      4 => MENU_FILE
        .replace("gap: 4", "gap: 10")
        .replace("\"Menu\"", "\"Main menu\"")
        .replace(
          "Button(label: \"Quit\"",
          "Button(label: \"Options\", name: \"options\"),\n  Button(label: \"Quit\"",
        ),
      10 => MENU_FILE.replace("Button(label: \"Quit\"", "Text(text: \"Quit\""),
      _ => return,
    };
    std::fs::write(&path, text).unwrap();
    world
      .resource_mut::<crate::core::asset::AssetManager>()
      .reload_ui("menu.ui.ron")
      .unwrap();
  };
  let script = Script::default()
    .frames(3)
    .then(Action("ui.down"))
    .frames(4)
    .then(Action("ui.down"))
    .frames(6)
    .then(Action("ui.up"))
    .frames(3);
  let run = oracle_with(
    |h| {
      h.assets().set_base_path(dir.path());
      h.spawn_ui_file("menu.ui.ron").unwrap();
    },
    &ui,
    &|_| {},
    script.steps(),
  );
  assert_cached(&run);
  let title = named(&run.on, "title");
  assert_eq!(run.on.world().get::<UiLabel>(title).unwrap().0, "Menu");
  assert!(
    run.on.find_ui("options").is_none(),
    "the last reload took it out"
  );
}

/// Names the kitchen sink's random pointer goes to
const KITCHEN: [&str; 24] = [
  "tabs", "radio", "amount", "colour", "field", "tip", "off", "picture", "rebind", "outer", "more",
  "inner", "popup", "g0", "g1", "g2", "g4", "klist", "line", "kitchen", "heal", "ehud", "left",
  "right",
];

/// Every kind of node in one focus scope: tabs, a radio group and a slider
/// bound to captions, a dropdown, a text field, a tooltip, a disabled
/// button, an image, a rebinding row and a prompt; nested scroll areas
/// holding a popup's anchor; slots to drag between; a typewriter over a
/// choice list; a list of 100 rows
fn kitchen() -> UiNode {
  use crate::core::ui::Anchor;
  let left = UiNode::column()
    .gap(4.0)
    .name("left")
    .child(UiNode::tabs(&["One", "Two", "Three"], 0).name("tabs"))
    .child(
      UiNode::radio_group(&["Low", "Mid", "High"], 1)
        .bind("level")
        .name("radio"),
    )
    .child(UiNode::text("Level {level}").bound())
    .child(
      UiNode::slider(0.0..=10.0, 5.0)
        .step(1.0)
        .show_value()
        .bind("amount")
        .name("amount"),
    )
    .child(UiNode::text("Amount {amount}").bound())
    .child(
      UiNode::dropdown(&["Red", "Green", "Blue"], 0)
        .width(120.0)
        .name("colour"),
    )
    .child(
      UiNode::text_input("")
        .placeholder("Type")
        .max_len(16)
        .name("field"),
    )
    .child(UiNode::button("Tip").tooltip("A tip").name("tip"))
    .child(UiNode::button("Off").enabled(false).name("off"))
    .child(
      UiNode::image(icon(90, 90, 200))
        .image_size(24.0, 24.0)
        .tooltip("Picture")
        .name("picture"),
    )
    .child(UiNode::rebind("jump").name("rebind"))
    .child(UiNode::prompt("jump"));
  let inner = (0..5).map(|i| UiNode::button(format!("Inner {i}")));
  let outer = UiNode::scroll()
    .height(150.0)
    .width(180.0)
    .name("outer")
    .child(UiNode::text("Outer"))
    .child(UiNode::button("More").name("more"))
    .child(UiNode::scroll().height(60.0).name("inner").children(inner))
    .children((0..6).map(|i| UiNode::button(format!("Outer {i}"))));
  let slots = (0..6).map(|i| {
    let slot = UiNode::slot()
      .drag_source()
      .drop_target()
      .name(format!("g{i}"));
    match i {
      0 => slot.icon(icon(200, 90, 90)).count(2),
      4 => slot.icon(icon(90, 200, 90)),
      _ => slot,
    }
  });
  let middle = UiNode::column()
    .gap(4.0)
    .child(outer)
    .child(
      UiNode::popup("more")
        .name("popup")
        .hidden()
        .child(UiNode::button("Rename").default_focus())
        .child(UiNode::button("Delete")),
    )
    .child(UiNode::grid(3).gap(4.0).children(slots));
  let row = || {
    UiNode::row()
      .gap(4.0)
      .child(UiNode::button("{name}").bound())
      .child(UiNode::text("#{n}").bound())
  };
  let right = UiNode::column()
    .gap(4.0)
    .name("right")
    .child(
      UiNode::text("Hello traveller, what brings you here?")
        .typewriter(TextReveal::new(40.0))
        .wrap(180.0)
        .name("line"),
    )
    .child(UiNode::choice_list(&["Yes", "No", "Maybe"]).name("choices"))
    .child(
      UiNode::list(100, 22.0)
        .height(140.0)
        .width(180.0)
        .row_shape(row)
        .name("klist"),
    );
  UiNode::row()
    .focus_scope()
    .gap(8.0)
    .anchor(Anchor::TopLeft)
    .name("kitchen")
    .child(left)
    .child(middle)
    .child(right)
}

/// What the game does after each frame of the kitchen sink: binds the
/// list's rows, moves dropped items, opens and closes the popup and starts
/// the line again when a choice is picked
fn kitchen_game(h: &mut UiHarness) {
  bind_rows(h);
  move_dropped(h);
  let (more, popup, line, choices) = (
    named(h, "more"),
    named(h, "popup"),
    named(h, "line"),
    named(h, "choices"),
  );
  let events: Vec<UiEvent> = h.events().to_vec();
  for event in events {
    let world = h.world_mut();
    match event.kind {
      UiEventKind::Clicked if event.node == more => {
        if world.get::<UiHidden>(popup).is_some() {
          world.entity_mut(popup).remove::<UiHidden>();
        } else {
          world.entity_mut(popup).insert(UiHidden);
        }
      }
      UiEventKind::Back if event.node == popup => {
        world.entity_mut(popup).insert(UiHidden);
      }
      UiEventKind::Clicked if event.node == choices => {
        world.get_mut::<UiLabel>(line).unwrap().0 = "And then?".into();
        world.get_mut::<TextReveal>(line).unwrap().restart();
      }
      _ => {}
    }
  }
}

/// The embedded HUD the kitchen sink's closure draws
fn kitchen_hud() -> UiNode {
  UiNode::column()
    .embedded()
    .gap(4.0)
    .name("ehud")
    .child(UiNode::text("Hp {hp}").bound().name("ehp"))
    .child(UiNode::button("Heal").name("heal"))
}

/// `frames` frames of random input over the kitchen sink
fn random_kitchen_steps(seed: u64, frames: usize) -> Vec<Step> {
  const ACTIONS: [&str; 9] = [
    "ui.up",
    "ui.down",
    "ui.left",
    "ui.right",
    "ui.confirm",
    "ui.back",
    "ui.next",
    "ui.tab_next",
    "ui.tab_prev",
  ];
  const KEYS: [KeyCode; 9] = [
    KeyCode::Backspace,
    KeyCode::Enter,
    KeyCode::Escape,
    KeyCode::ArrowLeft,
    KeyCode::Home,
    KeyCode::Digit1,
    KeyCode::Digit2,
    KeyCode::Digit3,
    KeyCode::KeyQ,
  ];
  const TEXTS: [&str; 4] = ["a", "bc", "Ryo", " "];
  const THEMES: [fn() -> Theme; 3] = [Theme::light, Theme::pixel, Theme::dark];
  const WRITES: [fn(&mut World, Entity); 4] = [
    |w, e| w.get_mut::<UiLabel>(e).unwrap().0 = "Tip!".into(),
    |w, e| w.get_mut::<UiLabel>(e).unwrap().0 = "Tip".into(),
    |w, e| {
      w.entity_mut(e).insert(UiTooltip("Another tip".into()));
    },
    |w, e| {
      w.entity_mut(e).remove::<UiTooltip>();
    },
  ];
  const SIZES: [(f32, f32); 3] = [(640.0, 480.0), (720.0, 540.0), (600.0, 450.0)];
  let mut rng = XorShift(seed);
  let mut steps = Vec::new();
  for frame in 0..frames {
    match rng.below(100) {
      0..15 => steps.push(Move(rng.below(640) as f32, rng.below(480) as f32)),
      15..30 => steps.push(At(rng.pick(&KITCHEN))),
      30..36 => steps.push(Press),
      36..42 => steps.push(Release),
      42..47 => steps.push(Wheel(rng.pick(&[-3.0, -1.0, 1.0, 2.0]))),
      47..65 => steps.push(Action(rng.pick(&ACTIONS))),
      65..68 => steps.push(Type(rng.pick(&TEXTS))),
      68..71 => steps.push(Key(rng.pick(&KEYS))),
      71..72 => {
        let amount = BindValue::Number(rng.below(11) as f64);
        steps.push(Set("kitchen", "amount", amount));
      }
      72..73 => steps.push(Set("ehud", "hp", BindValue::Number(frame as f64))),
      73..74 => steps.push(Write("tip", rng.pick(&WRITES))),
      74..75 => steps.push(Count("klist", rng.below(120) as usize)),
      75..76 => steps.push(Jump("klist", rng.below(100) as usize)),
      76..77 => steps.push(Refresh("klist")),
      77..78 => steps.push(Write("picture", |w, e| {
        if w.get::<UiHidden>(e).is_some() {
          w.entity_mut(e).remove::<UiHidden>();
        } else {
          w.entity_mut(e).insert(UiHidden);
        }
      })),
      _ => {}
    }
    match rng.below(600) {
      0 => steps.push(Theme(rng.pick(&THEMES))),
      1 => {
        let (width, height) = rng.pick(&SIZES);
        steps.push(Resize(width, height));
      }
      2 => steps.push(Device(InputDevice::Gamepad, PadFamily::PlayStation)),
      3 => steps.push(Device(InputDevice::Keyboard, PadFamily::Xbox)),
      4 => steps.push(Device(InputDevice::Mouse, PadFamily::Xbox)),
      _ => {}
    }
    steps.push(Frame);
  }
  steps
}

#[test]
fn three_thousand_random_frames_over_every_kind_of_node_look_the_same_cached() {
  let ui = |ui: &mut UiContext<'_>, world: &mut World, _: usize| {
    let root = embedded(world, "ehud");
    ui.column()
      .id("ehud")
      .at(Rect::new(440.0, 330.0, 180.0, 120.0))
      .show(|ui| {
        ui.node(root, world);
      });
  };
  let steps = random_kitchen_steps(0x0dd_ba11_c0ff_ee42, 3000);
  let run = oracle_with(
    |h| {
      with_prompts(h);
      h.spawn_ui(kitchen());
      h.spawn_ui(kitchen_hud());
    },
    &ui,
    &kitchen_game,
    &steps,
  );
  assert_eq!(run.frames, 3000);
  assert_cached(&run);
}

/// An embedded tree holding a popup the game opens, and a slider bound to a
/// caption drawn before it
fn embedded_menu() -> UiNode {
  UiNode::column()
    .embedded()
    .gap(4.0)
    .name("hud")
    .child(UiNode::text("Gain {gain:.1}").bound().name("gain caption"))
    .child(UiNode::button("Menu").name("emenu"))
    .child(UiNode::slider(0.0..=1.0, 0.5).bind("gain").name("gain"))
    .child(
      UiNode::popup("emenu")
        .name("epopup")
        .hidden()
        .child(UiNode::button("Copy").default_focus())
        .child(UiNode::button("Paste")),
    )
}

#[test]
fn an_embedded_tree_in_a_closure_scroll_area_looks_the_same_cached() {
  // The closure draws the tree among keyed buttons in a scroll area, skips
  // it some frames and draws it in a popup's place others
  let ui = |ui: &mut UiContext<'_>, world: &mut World, frame: usize| {
    let root = embedded(world, "hud");
    ui.scroll()
      .id("pane")
      .height(150.0)
      .width(250.0)
      .at(Rect::new(350.0, 40.0, 250.0, 150.0))
      .show(|ui| {
        for i in 0..4 {
          ui.button(format!("Closure {i}")).id(i).show();
        }
        if !matches!(frame, 6 | 7 | 15) {
          ui.node(root, world);
        }
        for i in 4..8 {
          ui.button(format!("Closure {i}")).id(i).show();
        }
      });
  };
  let script = Script::default()
    .frames(3)
    .then(Move(475.0, 60.0))
    .then(Wheel(-2.0))
    .frames(6)
    .then(Do(|h| point_at(h, named(h, "emenu"), 0.0, 0.0)))
    .then(Press)
    .then(Release)
    .frames(3)
    .then(Move(475.0, 60.0))
    .then(Wheel(-1.0))
    .frames(4)
    // The slider dragged: its caption, drawn before it, follows next frame
    .then(Do(|h| point_at(h, named(h, "gain"), -20.0, 0.0)))
    .then(Press)
    .then(Do(|h| point_at(h, named(h, "gain"), 30.0, 0.0)))
    .then(Release)
    .frames(2)
    .then(Do(|h| point_at(h, named(h, "emenu"), 0.0, 0.0)))
    .then(Press)
    .then(Release)
    .then(Write("hud", |w, e| {
      w.entity_mut(e).insert(UiHidden);
    }))
    .frames(2)
    .then(Write("gain caption", |w, e| {
      w.get_mut::<UiLabel>(e).unwrap().0 = "Gain".into();
    }))
    .then(Write("hud", |w, e| {
      w.entity_mut(e).remove::<UiHidden>();
    }))
    .then(Move(1000.0, 1000.0))
    .frames(12);
  let run = oracle_with(
    |h| {
      h.spawn_ui(menu());
      h.spawn_ui(embedded_menu());
    },
    &ui,
    &|h| {
      let (menu, popup) = (named(h, "emenu"), named(h, "epopup"));
      let opened = h
        .events()
        .iter()
        .any(|e| e.kind == UiEventKind::Clicked && e.node == menu);
      if opened {
        let world = h.world_mut();
        if world.get::<UiHidden>(popup).is_some() {
          world.entity_mut(popup).remove::<UiHidden>();
        } else {
          world.entity_mut(popup).insert(UiHidden);
        }
      }
    },
    script.steps(),
  );
  assert_cached(&run);
}

/// A list of `count` items in three columns, its rows a dropdown and a
/// slider bound to a caption, inside an outer scroll area
fn shelf(count: usize, caption: &'static str) -> UiNode {
  let row = move || {
    UiNode::column()
      .gap(2.0)
      .child(UiNode::text(caption).bound())
      .child(
        UiNode::dropdown(&["A", "B", "C"], 0)
          .width(80.0)
          .name("pick"),
      )
      .child(UiNode::slider(0.0..=1.0, 0.0).bind("v").name("v"))
  };
  UiNode::scroll()
    .height(260.0)
    .width(420.0)
    .anchor(crate::core::ui::Anchor::TopLeft)
    .name("outer")
    .child(UiNode::text("Shelf"))
    .child(
      UiNode::list(count, 70.0)
        .columns(3)
        .gap(4.0)
        .height(220.0)
        .width(400.0)
        .row_shape(row)
        .name("shelf"),
    )
    .child(UiNode::spacer().height(200.0))
    .child(UiNode::button("End").name("end"))
}

#[test]
fn a_grid_list_in_a_scroll_area_with_popups_in_its_rows_looks_the_same_cached() {
  let script = Script::default()
    .frames(4)
    // The outer area, then the list
    .then(Do(|h| point_at(h, named(h, "outer"), 0.0, 100.0)))
    .then(Wheel(-1.0))
    .frames(6)
    .then(Do(|h| point_at(h, named(h, "shelf"), 0.0, 0.0)))
    .then(Wheel(-3.0))
    .frames(8)
    // A row's dropdown opened, an option picked
    .then(Do(|h| {
      let row = pooled(h, "shelf", 4);
      point_at(h, kids(h, row)[1], 0.0, 0.0);
    }))
    .then(Press)
    .then(Release)
    .frames(2)
    .then(Do(|h| {
      let row = pooled(h, "shelf", 4);
      point_at(h, kids(h, row)[1], 0.0, 48.0);
    }))
    .then(Press)
    .then(Release)
    .frames(2)
    // Opened again, and the list scrolled under it
    .then(Do(|h| {
      let row = pooled(h, "shelf", 5);
      point_at(h, kids(h, row)[1], 0.0, 0.0);
    }))
    .then(Press)
    .then(Release)
    .then(Do(|h| point_at(h, named(h, "outer"), 190.0, 120.0)))
    .then(Wheel(1.0))
    .frames(6)
    .then(Action("ui.back"))
    .frames(2)
    // A row's slider dragged: its caption, drawn before it, follows
    .then(Do(|h| {
      let row = pooled(h, "shelf", 6);
      point_at(h, kids(h, row)[2], -10.0, 0.0);
    }))
    .then(Press)
    .then(Do(|h| {
      let row = pooled(h, "shelf", 6);
      point_at(h, kids(h, row)[2], 20.0, 0.0);
    }))
    .then(Release)
    .frames(2)
    // A new row shape: the pooled rows are brought in line and bound again
    .then(Do(|h| {
      let outer = named(h, "outer");
      h.update_ui(outer, shelf(60, "Value {v:.2}!"));
    }))
    .frames(4)
    .then(Count("shelf", 7))
    .frames(4)
    .then(Jump("shelf", 5))
    .frames(20);
  let run = oracle_with(
    |h| {
      h.spawn_ui(shelf(60, "Value {v:.2}"));
    },
    &|_, _, _| {},
    &|h| {
      let binds: Vec<UiListBind> = h.messages::<UiListBind>().collect();
      for bind in binds {
        h.ui_set(bind.row, "v", (bind.index % 10) as f64 / 10.0);
      }
    },
    script.steps(),
  );
  assert_cached(&run);
}

/// A list of 20 rows, each a caption over a list of its own of 30 buttons
fn shelves() -> UiNode {
  let inner_row = || UiNode::row().child(UiNode::button("{name}").bound());
  let outer_row = move || {
    UiNode::column()
      .gap(2.0)
      .child(UiNode::text("{title}").bound())
      .child(
        UiNode::list(30, 20.0)
          .height(80.0)
          .width(200.0)
          .row_shape(inner_row)
          .name("inner"),
      )
  };
  UiNode::column()
    .focus_scope()
    .anchor(crate::core::ui::Anchor::TopLeft)
    .child(
      UiNode::list(20, 110.0)
        .height(330.0)
        .width(240.0)
        .row_shape(outer_row)
        .name("outer"),
    )
}

/// Bind the rows of the outer list and of the lists in its rows
fn bind_shelves(h: &mut UiHarness) {
  let outer = named(h, "outer");
  let binds: Vec<UiListBind> = h.messages::<UiListBind>().collect();
  for bind in binds {
    if bind.list == outer {
      h.ui_set(bind.row, "title", format!("Shelf {}", bind.index));
    } else {
      h.ui_set(bind.row, "name", format!("Book {}", bind.index));
    }
  }
}

#[test]
fn lists_in_a_lists_rows_look_the_same_cached() {
  let script = Script::default()
    .frames(4)
    .then(Do(|h| point_at(h, named(h, "outer"), 0.0, 120.0)))
    .then(Wheel(-2.0))
    .frames(8)
    // The wheel over an inner list, near the top of the outer view
    .then(Do(|h| point_at(h, named(h, "outer"), 0.0, -120.0)))
    .then(Wheel(-2.0))
    .frames(8)
    .repeat(Action("ui.down"), 5)
    .then(Move(1000.0, 1000.0))
    .then(Refresh("outer"))
    .frames(3)
    .then(Count("outer", 4))
    .frames(4)
    .then(Count("outer", 20))
    .frames(4);
  let run = oracle_with(
    |h| {
      h.spawn_ui(shelves());
    },
    &|_, _, _| {},
    &bind_shelves,
    script.steps(),
  );
  assert_cached(&run);
}
