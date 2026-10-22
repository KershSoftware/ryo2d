//! UI fragments in the engine: two engines driven alike, one keeping its
//! node trees' clean subtrees as fragments and one with the node cache off,
//! draw and behave the same every frame while scenes are pushed over a page,
//! popped and switched (a switch from `ui` runs the UI pass again), and a
//! click lands on the top scene

use std::sync::{Arc, Mutex};

use ryo2d::prelude::*;
use ryo2d::testing::{HeadlessEngine, StepOutcome, UiCacheStats};

use crate::common::ui_scenes::lock;

/// The actions a run's scenes were clicked with, by scene
#[derive(Default)]
struct Clicks {
  page: Vec<String>,
  overlay: Vec<String>,
}

type Log = Arc<Mutex<Clicks>>;

/// The actions of the scene's own clicks since it last read
fn clicked(ctx: &mut SceneContext) -> Vec<String> {
  ctx
    .ui_events()
    .into_iter()
    .filter(|e| e.kind == UiEventKind::Clicked)
    .filter_map(|e| e.action)
    .collect()
}

/// A page of node trees: a centred column of a title, a large button over
/// the screen's centre and a row of two, and a footer. Its `ui` pushes the overlay at frame `push` (the
/// pass runs again that frame), and with `churn` draws one to three labels
/// after its trees, so the orders of everything the overlay draws move each
/// frame.
struct Page {
  log: Log,
  frame: u32,
  push: Option<u32>,
  /// The frame the overlay pops itself at, counted from its first
  pop: Option<u32>,
  churn: bool,
}

impl Scene for Page {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.spawn_ui(
      UiNode::column()
        .gap(4.0)
        .anchor(Anchor::Center)
        .child(UiNode::text("Page"))
        .child(
          UiNode::button("Play")
            .action("play")
            .width(200.0)
            .height(80.0),
        )
        .child(
          UiNode::row()
            .gap(6.0)
            .child(UiNode::button("Options").action("options"))
            .child(UiNode::button("Quit").action("quit")),
        )
        .child(UiNode::text("Footer")),
    );
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) -> SceneAction {
    let frame = self.frame;
    self.frame += 1;
    lock(&self.log).page.extend(clicked(ctx));
    if self.churn {
      for i in 0..=frame % 3 {
        ui.draw_text(
          &format!("Note {i}"),
          4.0,
          4.0 + i as f32 * 14.0,
          Color::WHITE,
        );
      }
    }
    if self.push == Some(frame) {
      return SceneAction::Push(Box::new(Overlay {
        log: Arc::clone(&self.log),
        frame: 0,
        pop: self.pop,
      }));
    }
    SceneAction::Continue
  }
}

/// A transparent panel over the page's centre, a focus scope: a title and
/// Close, which pops it. Its `ui` draws a label that changes every frame.
struct Overlay {
  log: Log,
  frame: u32,
  pop: Option<u32>,
}

impl Scene for Overlay {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.spawn_ui(
      UiNode::panel()
        .focus_scope()
        .anchor(Anchor::Center)
        .gap(4.0)
        .child(UiNode::text("Overlay"))
        .child(UiNode::button("Close").action("close"))
        .child(UiNode::text("Over the page")),
    );
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) -> SceneAction {
    let frame = self.frame;
    self.frame += 1;
    ui.draw_text(&format!("Frame {frame}"), 200.0, 4.0, Color::WHITE);
    let clicks = clicked(ctx);
    let close = clicks.iter().any(|action| action == "close");
    lock(&self.log).overlay.extend(clicks);
    if close || self.pop == Some(frame) {
      return SceneAction::Pop;
    }
    SceneAction::Continue
  }

  fn is_transparent(&self) -> bool {
    true
  }
}

/// A scene of one node tree that switches to another such scene from its
/// `ui` at frame `switch`: the UI pass runs again that frame
struct Switching {
  frame: u32,
  switch: u32,
  title: &'static str,
}

impl Scene for Switching {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.spawn_ui(
      UiNode::column()
        .gap(4.0)
        .anchor(Anchor::TopLeft)
        .child(UiNode::text(self.title))
        .child(
          UiNode::row()
            .gap(6.0)
            .child(UiNode::button("One"))
            .child(UiNode::button("Two")),
        )
        .child(UiNode::text("Below")),
    );
  }

  fn ui(&mut self, _ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    let frame = self.frame;
    self.frame += 1;
    if frame == self.switch {
      return SceneAction::Switch(Box::new(Switching {
        frame: 0,
        switch: u32::MAX,
        title: "Switched to",
      }));
    }
    SceneAction::Continue
  }
}

/// Two engines built alike by `make`, one with the node cache (and so
/// fragments) on and one with it off, driven together and compared after
/// every frame
struct Pair {
  on: HeadlessEngine,
  off: HeadlessEngine,
  frames: usize,
  /// What the cached engine's walk did in each frame
  stats: Vec<UiCacheStats>,
  /// The clicks each engine's scenes took (cached, uncached)
  logs: [Log; 2],
}

impl Pair {
  /// `make` is given the log the engine's scenes write their clicks to
  fn new(make: impl Fn(Log) -> EngineBuilder) -> Option<Pair> {
    let logs = [Log::default(), Log::default()];
    Some(Pair {
      on: HeadlessEngine::new_or_skip(
        make(Arc::clone(&logs[0])).ui_cache(true).ui_fragments(true),
      )?,
      off: HeadlessEngine::new_or_skip(make(Arc::clone(&logs[1])).ui_cache(false))?,
      frames: 0,
      stats: Vec::new(),
      logs,
    })
  }

  fn each(&mut self, mut f: impl FnMut(&mut HeadlessEngine)) {
    f(&mut self.on);
    f(&mut self.off);
  }

  /// Run `n` frames on both: their UI (commands, records, focus, events,
  /// states) and their pixels must be the same after each
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
      let (a, b) = (self.on.ui_snapshot(), self.off.ui_snapshot());
      if a != b {
        panic!(
          "{what}, frame {frame}: fragments on and the cache off differ\n{}",
          difference(&a, &b)
        );
      }
      let (Ok(a), Ok(b)) = (self.on.frame(), self.off.frame()) else {
        panic!("{what}, frame {frame}: no frame presented");
      };
      assert!(
        a.as_raw() == b.as_raw(),
        "{what}, frame {frame}: fragments on and the cache off draw other pixels"
      );
      self.stats.push(self.on.ui_cache_stats());
    }
  }

  /// Fragments copied over `frames`
  fn reused(&self, frames: std::ops::Range<usize>) -> usize {
    self.stats[frames].iter().map(|s| s.reused).sum()
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

/// A page that pushes the overlay at frame `push`, its scenes' clicks
/// written to the log given
fn page(push: u32, pop: Option<u32>, churn: bool) -> impl Fn(Log) -> EngineBuilder {
  move |log| {
    EngineBuilder::new().resolution(320, 240).start_scene(Page {
      log,
      frame: 0,
      push: Some(push),
      pop,
      churn,
    })
  }
}

#[test]
fn the_top_scene_changes() {
  // The overlay is pushed at frame 5 and pops itself five frames later. The
  // page's records hold whether it was on top, so it builds again each time
  // that flips, and the fragments it kept before go untaken
  let Some(mut pair) = Pair::new(page(5, Some(5), false)) else {
    return;
  };
  pair.frames(5, "the page");
  assert!(pair.reused(2..5) > 0, "the page's tree was copied");
  pair.frames(6, "the overlay over the page");
  assert!(pair.reused(8..11) > 0, "both trees were copied");
  pair.frames(6, "the page again");
  assert!(pair.reused(13..17) > 0, "the page's tree was copied");
}

#[test]
fn scene_pushed_over_fragments() {
  // The page draws one to three labels after its trees each frame, so the
  // overlay's tree, kept, is copied at another base each frame, and its own
  // label changes each frame; the page's tree below keeps its fragments.
  // The records copied must keep the top scene's widgets above the page's
  // for hit tests.
  let Some(mut pair) = Pair::new(page(3, None, true)) else {
    return;
  };
  pair.frames(4, "the overlay pushed");
  pair.frames(8, "the overlay over the page");
  assert!(pair.reused(6..12) > 0, "the trees were copied");
  // A click on Close, which lies over the page's Play: the overlay takes it
  let targets = pair.on.ui_focusables();
  assert_eq!(targets.len(), 1, "the top scene's Close alone: {targets:?}");
  let (_, close) = targets[0];
  let (x, y) = (close.x + close.width / 2.0, close.y + close.height / 2.0);
  pair.each(|engine| engine.mouse_move(x, y));
  pair.frames(3, "over Close");
  pair.each(|engine| engine.press_mouse(MouseButton::Left));
  pair.frames(1, "pressing Close");
  pair.each(|engine| engine.release_mouse(MouseButton::Left));
  pair.frames(1, "releasing on Close");
  pair.frames(3, "the page again");
  for log in &pair.logs {
    let log = lock(log);
    assert_eq!(log.overlay, ["close"], "the overlay took the click");
    assert!(
      log.page.is_empty(),
      "the page below took none: {:?}",
      log.page
    );
  }
  assert!(
    !pair.on.ui_snapshot().contains("\"Over the page\""),
    "the overlay closed"
  );
  // The same click with the overlay gone lands on the page's Play
  pair.each(|engine| engine.press_mouse(MouseButton::Left));
  pair.frames(1, "pressing Play");
  pair.each(|engine| engine.release_mouse(MouseButton::Left));
  pair.frames(2, "releasing on Play");
  for log in &pair.logs {
    assert_eq!(lock(log).page, ["play"], "Play lay under Close");
  }
}

#[test]
fn a_rerun_pass() {
  // The scene switches from its `ui` at frame 4: the pass runs again that
  // frame, drawing the new scene, and keeps only fragments carried from the
  // frame before
  let make = |_| {
    EngineBuilder::new()
      .resolution(320, 240)
      .start_scene(Switching {
        frame: 0,
        switch: 4,
        title: "Switching",
      })
  };
  let Some(mut pair) = Pair::new(make) else {
    return;
  };
  pair.frames(4, "before the switch");
  assert!(pair.reused(2..4) > 0, "the first scene's tree was copied");
  pair.frames(1, "switched, the pass run again");
  pair.frames(6, "after the switch");
  assert!(pair.reused(7..11) > 0, "the new scene's tree was copied");
}
