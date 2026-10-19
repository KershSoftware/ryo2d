//! The node cache in the engine: a scene pushed over a cached tree and
//! popped again draws and behaves the same with caching on and off, frame by
//! frame, and the popped scene's records go with it

use std::sync::{Arc, Mutex};

use ryo2d::prelude::*;
use ryo2d::testing::{HeadlessEngine, StepOutcome};

/// What the two scenes share: the menu's root, and the overlay's counter
/// label while it shows
#[derive(Default)]
struct Shared {
  menu: Option<Entity>,
  label: Option<Entity>,
  /// What the menu last wrote to it
  written: Option<String>,
}

fn shared(shared: &Arc<Mutex<Shared>>) -> std::sync::MutexGuard<'_, Shared> {
  shared
    .lock()
    .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The scene's own clicks since it last read
fn clicked(ctx: &mut SceneContext, action: &str) -> bool {
  ctx
    .ui_events()
    .into_iter()
    .any(|e| e.kind == UiEventKind::Clicked && e.action.as_deref() == Some(action))
}

/// A title and Open (the default focus), which pushes the overlay. While the
/// overlay shows, this scene's `ui`, run first (it is below), writes the
/// overlay's label each frame: the overlay must draw this frame's value.
struct Menu {
  shared: Arc<Mutex<Shared>>,
  count: u32,
}

impl Scene for Menu {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let root = ctx.spawn_ui(
      UiNode::column()
        .focus_scope()
        .gap(4.0)
        .anchor(Anchor::TopLeft)
        .child(UiNode::text("Menu"))
        .child(
          UiNode::button("Open")
            .action("open")
            .name("open")
            .default_focus(),
        ),
    );
    shared(&self.shared).menu = Some(root);
  }

  fn ui(&mut self, _ui: &mut UiContext, ctx: &mut SceneContext) -> SceneAction {
    let label = shared(&self.shared).label;
    if let Some(label) = label
      && let Some(mut text) = ctx.world.get_mut::<UiLabel>(label)
    {
      self.count += 1;
      text.0 = format!("Count {}", self.count);
      shared(&self.shared).written = Some(text.0.clone());
    }
    if clicked(ctx, "open") {
      return SceneAction::Push(Box::new(Overlay {
        shared: Arc::clone(&self.shared),
      }));
    }
    SceneAction::Continue
  }
}

/// A transparent panel of the counter label and Close (the default focus),
/// which pops it
struct Overlay {
  shared: Arc<Mutex<Shared>>,
}

impl Scene for Overlay {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.spawn_ui(
      UiNode::panel()
        .focus_scope()
        .anchor(Anchor::Center)
        .child(UiNode::text("Count 0").name("count"))
        .child(UiNode::button("Close").action("close").default_focus()),
    );
    shared(&self.shared).label = ctx.find_ui("count");
  }

  fn on_exit(&mut self, _ctx: &mut SceneContext) {
    shared(&self.shared).label = None;
  }

  fn ui(&mut self, _ui: &mut UiContext, ctx: &mut SceneContext) -> SceneAction {
    if clicked(ctx, "close") {
      return SceneAction::Pop;
    }
    SceneAction::Continue
  }

  fn is_transparent(&self) -> bool {
    true
  }
}

/// An engine with caching on or off, starting on the menu
fn engine(cache: bool) -> Option<(HeadlessEngine, Arc<Mutex<Shared>>)> {
  let shared = Arc::new(Mutex::new(Shared::default()));
  let menu = Menu {
    shared: Arc::clone(&shared),
    count: 0,
  };
  let builder = EngineBuilder::new()
    .resolution(320, 240)
    .ui_cache(cache)
    .start_scene(menu);
  Some((HeadlessEngine::new_or_skip(builder)?, shared))
}

/// The two engines, driven alike
struct Pair {
  on: HeadlessEngine,
  off: HeadlessEngine,
  /// What the cached engine's scenes share
  shared: Arc<Mutex<Shared>>,
  frames: usize,
  /// The cached engine's node records after each frame
  records: Vec<usize>,
  /// Frames where the overlay showed: its label was built with the value
  /// the menu wrote that frame
  counted: usize,
  /// Of those, the frames the menu's tree, below it, was not built
  menu_cached: usize,
  /// Tree nodes the cached engine replayed, over the run
  replayed: usize,
  /// Frames the cached engine's debug spot check built a clean node in
  spot_checked: usize,
}

impl Pair {
  fn new() -> Option<Pair> {
    let (on, shared) = engine(true)?;
    let (off, _) = engine(false)?;
    Some(Pair {
      on,
      off,
      shared,
      frames: 0,
      records: Vec::new(),
      counted: 0,
      menu_cached: 0,
      replayed: 0,
      spot_checked: 0,
    })
  }

  /// South tapped on both
  fn south(&mut self) {
    self.on.tap_pad(GamepadButton::South);
    self.off.tap_pad(GamepadButton::South);
  }

  /// Run `n` frames on both, their UI compared after each
  fn frames(&mut self, n: u32, what: &str) {
    for _ in 0..n {
      let (on, off) = (self.on.step(), self.off.step());
      let frame = self.frames;
      self.frames += 1;
      assert_eq!(
        (on.unwrap(), off.unwrap()),
        (StepOutcome::Running, StepOutcome::Running),
        "{what}, frame {frame}"
      );
      let (on, off) = (self.on.ui_snapshot(), self.off.ui_snapshot());
      if on != off {
        panic!(
          "{what}, frame {frame}: caching on and off differ\n{}",
          difference(&on, &off)
        );
      }
      let stats = self.on.ui_cache_stats();
      self.records.push(stats.records);
      self.replayed += stats.replayed;
      self.spot_checked += usize::from(stats.spot_checked.is_some());
      let (menu, label, written) = {
        let shared = shared(&self.shared);
        (shared.menu, shared.label, shared.written.clone())
      };
      if let Some(label) = label {
        let shown = self.on.world().get::<UiLabel>(label).map(|l| l.0.clone());
        assert_eq!(shown, written, "{what}, frame {frame}: this frame's value");
        assert!(
          stats.rebuilt.contains(&label),
          "{what}, frame {frame}: the overlay draws what the menu wrote below it"
        );
        self.counted += 1;
        self.menu_cached += usize::from(menu.is_some_and(|menu| !stats.rebuilt.contains(&menu)));
      }
    }
  }
}

/// Where two snapshots first differ: the line, and the text around the
/// first differing character in each
fn difference(on: &str, off: &str) -> String {
  let (a, b): (Vec<&str>, Vec<&str>) = (on.lines().collect(), off.lines().collect());
  let line = (0..a.len().max(b.len()))
    .find(|&i| a.get(i) != b.get(i))
    .unwrap_or(0);
  let (x, y) = (a.get(line).unwrap_or(&""), b.get(line).unwrap_or(&""));
  let at = x
    .char_indices()
    .zip(y.chars())
    .find(|((_, p), q)| p != q)
    .map_or(x.len().min(y.len()), |((i, _), _)| i);
  let around = |s: &str| -> String { s.chars().skip(at.saturating_sub(200)).take(500).collect() };
  format!(
    "line {line} ({:?}), from char {at}:\n  on:  ...{}...\n  off: ...{}...",
    x.split(' ').next().unwrap_or(""),
    around(x),
    around(y)
  )
}

#[test]
fn a_scene_pushed_over_a_cached_tree_and_popped_looks_the_same_cached() {
  let Some(mut pair) = Pair::new() else {
    return;
  };
  pair.frames(4, "the menu opening");
  let menu = *pair.records.last().unwrap();
  assert!(menu > 0, "the menu's tree is cached");
  // Open pushes the overlay: the menu's tree is drawn below it (its top flag
  // moves), and its `ui` writes the overlay's label each frame
  pair.south();
  pair.frames(5, "the overlay open");
  assert!(shared(&pair.shared).label.is_some(), "the overlay shows");
  let open = *pair.records.last().unwrap();
  assert!(open > menu, "the overlay's tree is cached too");
  // Close pops it: the menu is on top again
  pair.south();
  pair.frames(1, "the overlay closing");
  assert!(shared(&pair.shared).label.is_none(), "the overlay left");
  pair.frames(1, "the menu again");
  assert_eq!(
    *pair.records.last().unwrap(),
    menu,
    "a frame after it left, the overlay's records are gone"
  );
  pair.frames(3, "the menu again");
  assert_eq!(
    *pair.records.last().unwrap(),
    menu,
    "the overlay's records went with it"
  );
  assert!(
    pair.counted >= 5,
    "the overlay showed {} frames",
    pair.counted
  );
  // Built again as it went below (and the frame after, as its focus left),
  // then replayed there
  assert!(pair.menu_cached >= 2, "the menu's tree replayed below");
  assert!(pair.replayed > 0, "caching replayed nothing");
  // The engine's spot check, on in debug builds, changed no frame
  assert_eq!(pair.spot_checked > 0, cfg!(debug_assertions));
}
