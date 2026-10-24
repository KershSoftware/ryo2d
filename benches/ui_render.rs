//! The UI's render-side cost with a GPU (`HeadlessEngine`, 1280x720): the
//! 200-widget settings node screen unchanged, with a label changing every
//! frame, and scrolling, with UI render retention on and off, and an empty
//! frame. The unchanged and changing-label screens are the one the targets
//! were set on, its list overflowing the panel
//! (`settings_tree_overflowing`); the scrolling screen's list fills the
//! panel, so the wheel scrolls it (`settings_tree`).
//!
//! Before criterion runs, a guard measures the render step of each
//! (`HeadlessEngine::render_time`, the wall time of building, uploading and
//! encoding the frame; not the UI pass) against an empty frame. An engine
//! stepped alone is paced by the GPU: when its CPU work is small (an empty
//! frame most of all) its render step waits on the GPU's backlog, and that
//! wait reads as cost. Stepping the engines in turn does not cure it alone,
//! as the GPU takes longer to finish a frame (about 200 to 300 µs on an
//! Apple GPU) than the CPU takes to step one. So the guard steps its engines
//! in turn, one frame each (each round starting at the next engine, and
//! every other round in reverse, so no engine always follows the same one
//! and none steps twice in a row), and lets the GPU finish all submitted
//! work before each measured step
//! (outside the measure), as a frame at 60 Hz finds it. Each median is taken
//! over the same rounds, so a change in the machine's load moves them all
//! alike. Wall time here equals the render step's thread CPU time to within
//! about 0.5 µs.
//!
//! The wait also runs wgpu's cleanup of the submissions the GPU finished
//! (freeing their staging buffers and command buffers), which a 60 Hz frame
//! does inside its next submit. So the guard reads a little low, a UI frame
//! more than an empty one, as it leaves more to clean up.
//!
//! The guard prints every median, and holds one changing label and the
//! scrolling list to being no slower retained than not (4g; the moving
//! list's texts are laid again from their kept glyphs), the scrolling list
//! to moving every frame, and the retained screens to 4i's targets: the
//! render step over an empty frame's under 5 µs idle (4g) and 25 µs with
//! one label changing, and the UI pass (`HeadlessEngine::ui_pass_time`, CPU
//! time before the render step) under 18 µs idle and 25 µs with the label.
//! Scrolling missed the spec's 20 µs for both, and is held to the numbers
//! accepted at the end of 4i instead: the render step under 35 µs, about
//! 10% over its final median (+31.3 µs), and the UI pass under 27 µs, about
//! 20% over its (22.5 µs) as a busy machine reads it up to 25.4, each
//! printed beside the 20 µs goal still open. The UI pass is
//! read from each engine stepped alone afterwards (2,000 frames, after as
//! many to warm up), as a game runs its one engine and as the targets were
//! set: stepped in turn, each engine's caches are evicted by the others'
//! work, and its UI pass reads 1.4 to 2 times as long (also printed). A
//! miss fails the run once criterion's benches have run, printing the
//! command to profile it with. The targets are absolute, and these medians
//! move by up to 2x with the machine's load: run the guard on a quiet
//! machine. It runs only where criterion measures a retained settings
//! screen: with no filter or one naming `settings_idle`, `settings_label`
//! or `settings_scrolling`, and not for `--list`, `--test` or
//! `--profile-time` (the profile command).
//!
//! The guard also steps a kept skinned menu: twenty buttons drawn with a
//! nine-slice skin, each beside a label in a bitmap font, so the UI image
//! it keeps shows hundreds of sprite commands of two textures. A kept frame
//! marks the textures it shows used (`SpriteGpuRenderer::mark_shown`); the
//! menu's render step over an empty frame's is printed, with no target.

mod guards;

use std::time::Duration;

use criterion::{Criterion, criterion_group};
use ryo2d::prelude::*;
use ryo2d::testing::fixtures::{settings_tree, settings_tree_overflowing};
use ryo2d::testing::{HeadlessEngine, shared_gpu};

/// Rounds each median is taken over: one frame of every engine a round
const FRAMES: usize = 2000;
/// Wheel lines each way: fewer than the list scrolls, so it never rests at
/// its far end, and every frame moves it
const SWING: u32 = 20;
/// Over a row of the scroll area (the panel centre is not over it)
const OVER_LIST: (f32, f32) = (520.0, 238.0);
/// An idle settings screen's render step over an empty frame's (4g)
const IDLE_TARGET: Check = Check::target(5);
/// One changing label's render step over an empty frame's (4i)
const LABEL_TARGET: Check = Check::target(25);
/// The scrolling screen's render step over an empty frame's: the limit
/// accepted at the end of 4i, and the spec's goal, still open
const SCROLLING_LIMIT: Check = Check::limit(35, 20);
/// The UI pass of the idle and one-label screens (4i)
const IDLE_PASS_TARGET: Check = Check::target(18);
const LABEL_PASS_TARGET: Check = Check::target(25);
/// The scrolling screen's UI pass: the limit accepted at the end of 4i, and
/// the spec's goal, still open
const SCROLLING_PASS_LIMIT: Check = Check::limit(27, 20);

/// What a median is held to: under `limit`, or the run fails. A `goal`
/// under the limit is a target not met when the limit was set, printed
/// beside it.
#[derive(Clone, Copy)]
struct Check {
  limit: Duration,
  goal: Duration,
}

impl Check {
  /// Under `us` µs
  const fn target(us: u64) -> Check {
    Check::limit(us, us)
  }

  /// Under `us` µs, with the goal of under `goal` µs
  const fn limit(us: u64, goal: u64) -> Check {
    Check {
      limit: Duration::from_micros(us),
      goal: Duration::from_micros(goal),
    }
  }

  /// Whether `time` is under the limit, and what it is held to, with each
  /// verdict
  fn verdict(self, time: Duration) -> (bool, String) {
    let met = time < self.limit;
    let verdict = if met { "met" } else { "missed" };
    let limit = self.limit.as_micros();
    if self.goal >= self.limit {
      return (met, format!("target {limit} µs: {verdict}"));
    }
    let goal = if time < self.goal {
      "met"
    } else {
      "still open"
    };
    let text = format!(
      "limit {limit} µs: {verdict}; the spec's goal {} µs: {goal}",
      self.goal.as_micros()
    );
    (met, text)
  }
}

struct Empty;

impl Scene for Empty {
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }
}

/// Twenty buttons drawn with a nine-slice skin, each beside a label in a
/// bitmap font: a kept UI image of a few hundred sprite commands, every one
/// of them of the skin's image or the font's sheet
#[derive(Default)]
struct Skinned {
  font: Option<Handle<BitmapFont>>,
}

/// The skinned menu's bitmap font: 8 px cells, sixteen to a row
const SHEET_CHARS: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ 0123456789";

impl Scene for Skinned {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let mut assets = ctx.assets();
    let skin = assets.add(Image::solid_rect(24, 24, 200, 150, 90, 255));
    let sheet = assets.add(Image::solid_rect(128, 24, 255, 255, 255, 255));
    self.font = Some(assets.add(BitmapFont::new("Sheet", sheet, (8, 8), SHEET_CHARS)));
    let mut theme = Theme::dark().skin("wood", Skin::from_image(skin, [8.0; 4], Filter::Nearest));
    theme.widgets.button.normal.skin = Some("wood".into());
    ctx.ui_theme().set(theme);
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    let style = TextStyle::new().family("Sheet").size(16.0);
    for row in 0..20u32 {
      let y = 20.0 + row as f32 * 34.0;
      let (button, label) = (format!("ITEM {row}"), format!("OPTION NUMBER {row}"));
      ui.column().at(Rect::new(40.0, y, 160.0, 28.0)).show(|ui| {
        ui.button(button.as_str()).width(160.0).height(28.0);
      });
      ui.draw_text_styled(label.as_str(), 220.0, y + 6.0, &style);
    }
    SceneAction::Continue
  }
}

/// A settings screen, `tree`; with `label`, a counter over it changes every
/// frame
struct Settings {
  tree: fn() -> UiNode,
  label: bool,
  frame: u32,
}

impl Scene for Settings {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.spawn_ui((self.tree)());
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    if self.label {
      self.frame += 1;
      ui.draw_text(&format!("Frame {}", self.frame), 8.0, 8.0, Color::WHITE);
    }
    SceneAction::Continue
  }
}

/// What a measured engine shows
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Screen {
  Empty,
  /// The settings screen the targets were set on, unchanged
  Idle,
  /// The settings screen the targets were set on, and a label changing
  /// every frame
  Label,
  /// The settings screen whose list scrolls, turned by the wheel every frame
  Scrolling,
  /// The skinned menu, unchanged
  Skinned,
}

/// An engine on `screen`, warmed up; scrolling, the pointer over the list
fn engine(screen: Screen, retain: bool) -> Option<HeadlessEngine> {
  let builder = EngineBuilder::new().resolution(1280, 720).ui_retain(retain);
  let settings = |tree: fn() -> UiNode, label: bool| Settings {
    tree,
    label,
    frame: 0,
  };
  let builder = match screen {
    Screen::Empty => builder.start_scene(Empty),
    Screen::Idle => builder.start_scene(settings(settings_tree_overflowing, false)),
    Screen::Label => builder.start_scene(settings(settings_tree_overflowing, true)),
    Screen::Scrolling => builder.start_scene(settings(settings_tree, false)),
    Screen::Skinned => builder.start_scene(Skinned::default()),
  };
  let mut engine = HeadlessEngine::new_or_skip(builder)?;
  if screen == Screen::Scrolling {
    engine.mouse_move(OVER_LIST.0, OVER_LIST.1);
  }
  engine.step_frames(60).expect("the warm-up frames");
  Some(engine)
}

/// The wheel of a scrolling screen: one line a frame, `SWING` down from
/// where the list starts (its top), then `SWING` back up to it, and again
#[derive(Default)]
struct Wheel {
  /// Lines turned so far
  turned: u32,
}

impl Wheel {
  fn turn(&mut self, engine: &mut HeadlessEngine) {
    let dy = if (self.turned / SWING).is_multiple_of(2) {
      -1.0
    } else {
      1.0
    };
    self.turned += 1;
    engine.wheel(0.0, dy);
  }
}

/// One engine the guard steps, and its render steps so far
struct Probe {
  screen: Screen,
  retain: bool,
  engine: HeadlessEngine,
  wheel: Wheel,
  times: Vec<Duration>,
  /// The UI pass's CPU time each frame
  ui_times: Vec<Duration>,
  /// Frames whose UI image was kept
  kept: usize,
}

impl Probe {
  fn new(screen: Screen, retain: bool) -> Option<Probe> {
    Some(Probe {
      screen,
      retain,
      engine: engine(screen, retain)?,
      wheel: Wheel::default(),
      times: Vec::with_capacity(FRAMES),
      ui_times: Vec::with_capacity(FRAMES),
      kept: 0,
    })
  }

  fn step(&mut self) {
    if self.screen == Screen::Scrolling {
      self.wheel.turn(&mut self.engine);
    }
    self.engine.step().expect("a guard frame");
    self.times.push(self.engine.render_time());
    self.ui_times.push(self.engine.ui_pass_time());
    self.kept += usize::from(self.engine.ui_render_stats().kept);
  }

  fn median(&mut self) -> Duration {
    self.times.sort_unstable();
    self.times[self.times.len() / 2]
  }

  /// The median UI pass (CPU time before the render step) of the frames
  /// stepped in turn with the other engines
  fn ui_pass_median(&mut self) -> Duration {
    self.ui_times.sort_unstable();
    self.ui_times[self.ui_times.len() / 2]
  }

  /// The median UI pass of `FRAMES` frames stepped alone, after as many
  /// again to warm up: as a game's one engine runs it, its caches its own
  /// (stepped in turn, the other engines' work evicts them)
  fn lone_ui_pass(&mut self) -> Duration {
    let mut times = Vec::with_capacity(FRAMES);
    for frame in 0..2 * FRAMES {
      if self.screen == Screen::Scrolling {
        self.wheel.turn(&mut self.engine);
      }
      self.engine.step().expect("a guard frame");
      if frame >= FRAMES {
        times.push(self.engine.ui_pass_time());
      }
    }
    times.sort_unstable();
    times[times.len() / 2]
  }
}

/// `time` over `empty`, in µs, signed
fn over(time: Duration, empty: Duration) -> f64 {
  (time.as_secs_f64() - empty.as_secs_f64()) * 1e6
}

/// The UI's render targets over an empty frame, on the screen they were set
/// on: an idle settings screen adds under 5 µs (4g), a changing frame is no
/// slower retained than not (4g), one changing label adds under 25 µs (4i)
/// and the scrolling list under its accepted 35 µs; and the retained
/// screens' UI pass targets and limit (4i). Every median is printed, and
/// each check with its verdict, the scrolling ones with the spec's goal;
/// what is missed, these and that one label and the scrolling list are no
/// slower retained and the list moves every frame, is noted for the run to
/// fail on.
fn guard() {
  let screens = [
    (Screen::Empty, true),
    (Screen::Idle, true),
    (Screen::Label, true),
    (Screen::Scrolling, true),
    (Screen::Idle, false),
    (Screen::Label, false),
    (Screen::Scrolling, false),
    (Screen::Skinned, true),
  ];
  let mut probes = Vec::new();
  for (screen, retain) in screens {
    let Some(probe) = Probe::new(screen, retain) else {
      return;
    };
    probes.push(probe);
  }
  // Every headless engine draws on the one shared device
  let Ok((device, _)) = shared_gpu() else {
    return;
  };
  // Round robin: each round starts at the next engine, and every other
  // round runs in reverse, starting one further on so that no engine steps
  // twice in a row. Within a forward round each engine follows its lower
  // neighbour, within a reversed one its upper; a forward round's first
  // engine follows its upper neighbour, a reversed round's first the engine
  // three below it
  let count = probes.len();
  for round in 0..FRAMES {
    for i in 0..count {
      device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("the GPU finishes its work");
      let at = if round.is_multiple_of(2) {
        (round + i) % count
      } else {
        (round + 1 + count - i) % count
      };
      probes[at].step();
    }
  }
  let mut median = |screen: Screen, retain: bool| {
    let probe = probes
      .iter_mut()
      .find(|probe| probe.screen == screen && probe.retain == retain)
      .expect("a probe of each screen");
    (probe.median(), probe.kept)
  };
  let (empty, _) = median(Screen::Empty, true);
  let (idle, idle_kept) = median(Screen::Idle, true);
  let (label, _) = median(Screen::Label, true);
  let (scrolling, scrolling_kept) = median(Screen::Scrolling, true);
  let (idle_off, _) = median(Screen::Idle, false);
  let (label_off, _) = median(Screen::Label, false);
  let (scrolling_off, _) = median(Screen::Scrolling, false);
  let (skinned, skinned_kept) = median(Screen::Skinned, true);
  let us = |time: Duration| over(time, empty);
  let verdict = |met: bool| if met { "met" } else { "missed" };
  let against = |time: Duration, check: Check| {
    let (_, held) = check.verdict(time.saturating_sub(empty));
    format!("{:+.1} µs ({held})", us(time))
  };
  let off = |time: Duration, retained: Duration| {
    let met = retained <= time;
    format!("{:+.1} µs (retained no slower: {})", us(time), verdict(met))
  };
  eprintln!(
    "ui_render guard, render step over an empty frame ({:.1} µs), medians of {FRAMES} \
     interleaved frames. The screen the targets were set on: idle {} ({idle_kept} kept), \
     one label {}; retention off: idle {:+.1} µs, one label {}. The scrolling screen: \
     scrolling {} ({scrolling_kept} kept); retention off {}",
    empty.as_secs_f64() * 1e6,
    against(idle, IDLE_TARGET),
    against(label, LABEL_TARGET),
    us(idle_off),
    off(label_off, label),
    against(scrolling, SCROLLING_LIMIT),
    off(scrolling_off, scrolling),
  );
  eprintln!(
    "ui_render guard, a kept skinned menu (twenty nine-slice buttons with bitmap-font \
     labels): render step {:+.1} µs over an empty frame ({skinned_kept} kept)",
    us(skinned)
  );
  let mut missed = Vec::new();
  for (screen, time, check) in [
    (Screen::Idle, idle, IDLE_TARGET),
    (Screen::Label, label, LABEL_TARGET),
    (Screen::Scrolling, scrolling, SCROLLING_LIMIT),
  ] {
    if !check.verdict(time.saturating_sub(empty)).0 {
      missed.push(format!(
        "the render step, {screen:?} {}",
        against(time, check)
      ));
    }
  }
  let passes: Vec<String> = probes
    .iter_mut()
    .map(|probe| {
      let median = probe.ui_pass_median();
      format!(
        "{:?}{} {:.1} µs",
        probe.screen,
        if probe.retain { "" } else { " (unretained)" },
        median.as_secs_f64() * 1e6
      )
    })
    .collect();
  eprintln!(
    "ui_render guard, UI pass medians (CPU, not the render step), stepped in turn: {}",
    passes.join(", ")
  );
  let mut lone = Vec::new();
  for (screen, check) in [
    (Screen::Idle, IDLE_PASS_TARGET),
    (Screen::Label, LABEL_PASS_TARGET),
    (Screen::Scrolling, SCROLLING_PASS_LIMIT),
  ] {
    let probe = probes
      .iter_mut()
      .find(|probe| probe.screen == screen && probe.retain)
      .expect("a retained probe of each screen");
    let median = probe.lone_ui_pass();
    let (met, held) = check.verdict(median);
    let pass = format!("{screen:?} {:.1} µs ({held})", median.as_secs_f64() * 1e6);
    if !met {
      missed.push(format!("the UI pass, {pass}"));
    }
    lone.push(pass);
  }
  eprintln!(
    "ui_render guard, UI pass medians, each engine stepped alone: {}",
    lone.join(", ")
  );
  if scrolling_kept > 0 {
    missed.push(format!(
      "the scrolling list rested in {scrolling_kept} frames"
    ));
  }
  if label > label_off {
    missed.push(format!(
      "one label: retained {label:?}, not retained {label_off:?}"
    ));
  }
  if scrolling > scrolling_off {
    missed.push(format!(
      "scrolling: retained {scrolling:?}, not retained {scrolling_off:?}"
    ));
  }
  for miss in missed {
    guards::miss(miss);
  }
}

/// Where the scrolling settings screen's focusable widgets are: the list's
/// move as it scrolls
fn rows(engine: &HeadlessEngine) -> Vec<Rect> {
  engine
    .ui_focusables()
    .into_iter()
    .map(|(_, rect)| rect)
    .collect()
}

fn bench_ui_render(c: &mut Criterion) {
  if guards::runs(&["settings_idle", "settings_label", "settings_scrolling"]) {
    guard();
  }
  for (name, screen, retain) in [
    ("settings_idle", Screen::Idle, true),
    ("settings_idle_unretained", Screen::Idle, false),
    ("settings_label", Screen::Label, true),
    ("settings_label_unretained", Screen::Label, false),
  ] {
    let Some(mut engine) = engine(screen, retain) else {
      return;
    };
    c.bench_function(name, |b| b.iter(|| engine.step()));
  }
  for (name, retain) in [
    ("settings_scrolling", true),
    ("settings_scrolling_unretained", false),
  ] {
    let Some(mut engine) = engine(Screen::Scrolling, retain) else {
      return;
    };
    let mut wheel = Wheel::default();
    let before = rows(&engine);
    for _ in 0..SWING / 2 {
      wheel.turn(&mut engine);
      engine.step().expect("a scrolling frame");
    }
    assert_ne!(rows(&engine), before, "{name}: the wheel scrolls the list");
    c.bench_function(name, |b| {
      b.iter(|| {
        wheel.turn(&mut engine);
        engine.step()
      })
    });
  }
  let Some(mut engine) = engine(Screen::Empty, true) else {
    return;
  };
  c.bench_function("empty_frame", |b| b.iter(|| engine.step()));
}

criterion_group!(benches, bench_ui_render);

/// The guard and the benches, then the guard's verdict: a missed target
/// fails the run
fn main() {
  benches();
  Criterion::default().configure_from_args().final_summary();
  guards::verdict(
    "ui_render",
    "Profile a missed frame (the bench binary is the one `cargo bench --bench ui_render \
     --no-run` names; settings_idle, settings_label or settings_scrolling) with\n  xcrun \
     xctrace record --template 'Time Profiler' --output ui_render.trace --launch -- \
     target/release/deps/ui_render-<hash> --bench --profile-time 20 '^settings_label$'\nand \
     charge each sample's time to the nearest ryo2d function from the leaf up, as \
     docs/backlog/ui-retained-nodes.md's 4i section does; an idle or empty_frame profile is \
     the render step's baseline.",
  );
}
