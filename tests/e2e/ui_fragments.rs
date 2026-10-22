//! UI fragments in the engine: two engines driven alike, one keeping its
//! node trees' clean subtrees as fragments and one with the node cache off,
//! draw and behave the same every frame while scenes are pushed over a page,
//! popped and switched (a switch from `ui` runs the UI pass again), and a
//! click lands on the top scene. The debug spot check is off in both unless
//! a test turns it on: each frame it builds a clean node and captures the
//! fragment around it afresh, so a tree of one fragment would never be
//! copied. And the render cases (`PixelPair::fragments`, the spot check on):
//! a font render change, and more scroll areas than the renderer's slots. A
//! sprite updated in place under a kept fragment is the engine's own test
//! (`render/ui_retain/gpu_tests.rs`): only the engine replaces a sprite's
//! pixels in place, as hot reload does.

use std::sync::{Arc, Mutex};

use ryo2d::prelude::*;
use ryo2d::testing::fixtures::settings_tree;
use ryo2d::testing::{HeadlessEngine, PixelPair, StepOutcome, UiCacheStats};

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
  /// The debug spot check is on in both
  spot: bool,
}

impl Pair {
  /// `make` is given the log the engine's scenes write their clicks to; the
  /// spot check is off in both
  fn new(make: impl Fn(Log) -> EngineBuilder) -> Option<Pair> {
    Pair::with_spot_check(make, false)
  }

  /// [`new`](Pair::new), with the debug spot check `spot` in both
  fn with_spot_check(make: impl Fn(Log) -> EngineBuilder, spot: bool) -> Option<Pair> {
    let logs = [Log::default(), Log::default()];
    let mut pair = Pair {
      on: HeadlessEngine::new_or_skip(
        make(Arc::clone(&logs[0])).ui_cache(true).ui_fragments(true),
      )?,
      off: HeadlessEngine::new_or_skip(make(Arc::clone(&logs[1])).ui_cache(false))?,
      frames: 0,
      stats: Vec::new(),
      logs,
      spot,
    };
    pair.each(|engine| engine.set_ui_spot_check(spot));
    Some(pair)
  }

  fn each(&mut self, mut f: impl FnMut(&mut HeadlessEngine)) {
    f(&mut self.on);
    f(&mut self.off);
  }

  /// Run `n` frames on both: their UI (commands, records, focus, events,
  /// states) and their pixels must be the same after each, and each
  /// fragment the cached engine drew pushed one run of its own draws
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
      let runs = self.on.ui_runs_snapshot();
      if a != b {
        panic!(
          "{what}, frame {frame}: fragments on and the cache off differ\n{}\n\
           with fragments on, {runs}",
          difference(&a, &b)
        );
      }
      let stats = self.on.ui_cache_stats();
      assert!(
        runs.starts_with(&format!("runs {} held", runs.lines().count() - 1)),
        "{what}, frame {frame}: each run holds its own draws\n{runs}"
      );
      // The spot checks capture fragments they count apart
      if !self.spot {
        let drawn = stats.reused + stats.resolved_again + stats.captured;
        assert_eq!(
          runs.lines().count() - 1,
          drawn,
          "{what}, frame {frame}: one run per fragment drawn\n{runs}"
        );
      }
      let (Ok(a), Ok(b)) = (self.on.frame(), self.off.frame()) else {
        panic!("{what}, frame {frame}: no frame presented");
      };
      assert!(
        a.as_raw() == b.as_raw(),
        "{what}, frame {frame}: fragments on and the cache off draw other pixels"
      );
      self.stats.push(stats);
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
fn the_top_scene_changes_with_the_spot_check_on() {
  // As above, with the debug spot check on in both engines, as in a debug
  // game: each frame it builds a clean node and captures the fragment
  // around it afresh, and captures a kept fragment in place of its copy.
  // Both must draw and do what the copies would.
  let Some(mut pair) = Pair::with_spot_check(page(5, Some(5), true), true) else {
    return;
  };
  pair.frames(5, "the page");
  pair.frames(6, "the overlay over the page");
  pair.frames(6, "the page again");
  let differed: usize = pair.stats.iter().map(|s| s.spot_fragment_differed).sum();
  assert_eq!(differed, 0, "no kept fragment differs from its resolve");
  if cfg!(debug_assertions) {
    let checked = pair.stats.iter().filter(|s| s.spot_checked.is_some());
    assert!(checked.count() > 10, "the spot check built a node");
    let kept: usize = pair.stats.iter().map(|s| s.spot_kept).sum();
    assert!(
      kept > 0,
      "the spot checks captured fragments found the same"
    );
  }
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

/// The settings screen as a node tree
struct SettingsScreen;

impl Scene for SettingsScreen {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.spawn_ui(settings_tree());
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }
}

/// Draw the UI font of both engines with `render`
fn render_ui_font(pair: &mut PixelPair, render: FontRender) {
  pair.each(|engine| {
    let mut fonts = engine.world_mut().resource_mut::<Fonts>();
    let ui = fonts.role(FontRole::Ui).expect("a UI font");
    fonts.set_render(ui, render);
  });
}

#[test]
fn a_font_render_change_draws_as_a_rebuild() {
  // The settings screen in raster text, then in MSDF, scrolled by the
  // wheel and the trackpad, then in raster text again; the pointer over
  // the list. Copying resumes after each change.
  let make = || {
    EngineBuilder::new()
      .resolution(1280, 720)
      .start_scene(SettingsScreen)
  };
  let Some(mut pair) = PixelPair::fragments(make) else {
    return;
  };
  pair.each(|engine| engine.mouse_move(520.0, 238.0));
  pair.frames(10, "raster text");
  let raster = pair.on.frame().expect("a frame");
  let copied = pair.fragments_reused();
  render_ui_font(&mut pair, FontRender::msdf());
  pair.frames(10, "MSDF text");
  let msdf = pair.on.frame().expect("a frame");
  assert_ne!(raster.as_raw(), msdf.as_raw(), "the render change shows");
  assert!(pair.fragments_reused() > copied, "copying resumed in MSDF");
  // The list is the screen's one scroll area
  let list = |pair: &PixelPair| pair.on.ui_scroll_slots().get(1).map(|&(_, offset)| offset);
  let still = list(&pair);
  pair.each(|engine| engine.wheel(0.0, -3.0));
  pair.frames(15, "MSDF text, the wheel turned");
  let wheeled = list(&pair);
  assert_ne!(wheeled, still, "the wheel scrolled the list");
  pair.each(|engine| engine.trackpad(0.0, -7.3));
  pair.frames(10, "MSDF text, the trackpad moved");
  assert_ne!(list(&pair), wheeled, "the trackpad scrolled the list");
  let copied = pair.fragments_reused();
  render_ui_font(&mut pair, FontRender::Raster);
  pair.frames(10, "raster text again");
  assert!(
    pair.fragments_reused() > copied,
    "copying resumed in raster text"
  );
}

/// Scroll areas in the many-areas tree: more than the renderer's 256 slots
const AREAS: usize = 300;
/// Areas to a row of the grid, and each area's size and its gap
const AREAS_ACROSS: usize = 20;
const AREA: (f32, f32) = (56.0, 40.0);
const AREA_GAP: f32 = 4.0;

/// A grid of `AREAS` small scroll areas, each holding four labels taller
/// than it, so each scrolls
struct ManyAreas;

impl Scene for ManyAreas {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let areas = (0..AREAS).map(|i| {
      UiNode::scroll()
        .width(AREA.0)
        .height(AREA.1)
        .children((0..4).map(move |row| UiNode::text(format!("{i}.{row}"))))
    });
    ctx.spawn_ui(
      UiNode::grid(AREAS_ACROSS as u16)
        .gap(AREA_GAP)
        .anchor(Anchor::TopLeft)
        .children(areas),
    );
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }
}

/// The centre of area `i` of the many-areas grid (screen units)
fn area_centre(i: usize) -> (f32, f32) {
  let (column, row) = ((i % AREAS_ACROSS) as f32, (i / AREAS_ACROSS) as f32);
  (
    column * (AREA.0 + AREA_GAP) + AREA.0 / 2.0,
    row * (AREA.1 + AREA_GAP) + AREA.1 / 2.0,
  )
}

/// The scroll slots whose origins moved between `before` and `after` (each
/// `HeadlessEngine::ui_scroll_slots`)
fn moved(before: &[(Option<Rect>, [i32; 2])], after: &[(Option<Rect>, [i32; 2])]) -> Vec<usize> {
  (0..before.len().max(after.len()))
    .filter(|&i| before.get(i).map(|slot| slot.1) != after.get(i).map(|slot| slot.1))
    .collect()
}

#[test]
fn more_scroll_areas_than_slots_draw_as_a_rebuild() {
  // Past the renderer's 256 slots an area's contents are built where they
  // show, on the CPU, and never kept: one area under them scrolled by the
  // wheel and the trackpad, then one past them
  let make = || {
    EngineBuilder::new()
      .resolution(1280, 720)
      .start_scene(ManyAreas)
  };
  let Some(mut pair) = PixelPair::fragments(make) else {
    return;
  };
  pair.each(|engine| engine.mouse_move(-10.0, -10.0));
  pair.frames(10, "the areas opening");
  let rest = pair.on.ui_scroll_slots();
  assert!(rest.len() > AREAS, "a slot for each area: {}", rest.len());
  for (area, past) in [(10, false), (290, true)] {
    let (x, y) = area_centre(area);
    pair.each(|engine| engine.mouse_move(x, y));
    pair.frames(3, &format!("over area {area}"));
    let before = pair.on.ui_scroll_slots();
    pair.each(|engine| engine.wheel(0.0, -1.0));
    pair.frames(15, &format!("area {area}, the wheel turned"));
    pair.each(|engine| engine.trackpad(0.0, 3.3));
    pair.frames(10, &format!("area {area}, the trackpad moved"));
    let scrolled = moved(&before, &pair.on.ui_scroll_slots());
    assert!(
      scrolled.len() == 1 && (scrolled[0] >= 256) == past,
      "area {area} scrolled, its slot past the shader's: {past}; slots moved {scrolled:?}"
    );
  }
  assert!(pair.fragments_reused() > 0, "the areas copied fragments");
}
