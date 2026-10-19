//! Breakout's menus and HUD with caching on and off: two engines driven by
//! the same input must draw and behave the same, frame by frame

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::OnceLock;

use breakout::scenes::{GameScene, MenuScene};
use ryo2d::prelude::*;
use ryo2d::testing::{HeadlessEngine, StepOutcome};

/// A folder holding only the game's levels to run from, so the high score
/// is saved there and the player's is never touched, and no UI file is on
/// disk, so the embedded copies load. The working directory is the
/// process's, so the tests share one folder.
fn game_folder() {
  static FOLDER: OnceLock<PathBuf> = OnceLock::new();
  FOLDER.get_or_init(|| {
    let folder = std::env::temp_dir().join(format!("breakout-cache-{}", std::process::id()));
    let levels = folder.join("levels");
    std::fs::create_dir_all(&levels).unwrap();
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("levels");
    for entry in std::fs::read_dir(source).unwrap() {
      let path = entry.unwrap().path();
      std::fs::copy(&path, levels.join(path.file_name().unwrap())).unwrap();
    }
    std::env::set_current_dir(&folder).unwrap();
    folder
  });
}

/// One engine with caching on and one with it off, both starting on the
/// scene `start` makes, as the game runs (`main.rs`)
fn both<S: Scene + 'static>(start: fn() -> S) -> Option<Pair> {
  game_folder();
  let make = |cache: bool| {
    let builder = EngineBuilder::new()
      .resolution(1280, 720)
      .fixed_hz(120)
      .ui_cache(cache)
      .start_scene(start());
    HeadlessEngine::new_or_skip(builder)
  };
  Some(Pair {
    on: make(true)?,
    off: make(false)?,
    frames: 0,
    replayed: 0,
    seen: BTreeSet::new(),
  })
}

/// What the script does, and the frames run after it (each compared)
#[derive(Debug, Clone, Copy)]
enum Act {
  /// A key tapped, then 3 frames
  Key(KeyCode),
  /// A key held down from the next frame, then 1 frame
  Hold(KeyCode),
  /// A held key let go, then 1 frame
  Let(KeyCode),
  /// The pointer to (x, y), then 2 frames
  Move(f32, f32),
  /// The pointer to the centre of the n-th widget the pad can reach (as
  /// last drawn), then 2 frames
  Point(usize),
  /// The left button pressed and held, then 2 frames
  Press,
  /// The left button released, then 2 frames
  Release,
  /// A press, 2 frames, a release, 2 frames
  Click,
  /// Frames with no input
  Wait(u32),
}

/// The two engines, and what the run saw
struct Pair {
  on: HeadlessEngine,
  off: HeadlessEngine,
  frames: usize,
  /// Tree nodes the cached engine replayed, over the run
  replayed: usize,
  /// The kinds of node events the run sent
  seen: BTreeSet<String>,
}

impl Pair {
  /// Drive both engines through `script`; their UI must match after every
  /// frame
  fn run(&mut self, script: &[Act]) {
    self.frames(6, "the scene opening");
    for (i, &act) in script.iter().enumerate() {
      let what = format!("step {i} ({act:?})");
      let frames = self.apply(act, &what);
      self.frames(frames, &what);
    }
  }

  /// Give both engines `act`; the frames to run after it
  fn apply(&mut self, act: Act, what: &str) -> u32 {
    match act {
      Act::Point(n) => {
        let targets = self.on.ui_focusables();
        let Some(&(_, rect)) = targets.get(n) else {
          panic!("{what}: no widget {n} to reach, only {targets:?}");
        };
        let (x, y) = (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
        return self.apply(Act::Move(x, y), what);
      }
      Act::Click => {
        self.apply(Act::Press, what);
        self.frames(2, what);
        return self.apply(Act::Release, what);
      }
      _ => {}
    }
    for engine in [&mut self.on, &mut self.off] {
      match act {
        Act::Key(key) => engine.tap_key(key),
        Act::Hold(key) => engine.press_key(key),
        Act::Let(key) => engine.release_key(key),
        Act::Move(x, y) => engine.mouse_move(x, y),
        Act::Press => engine.press_mouse(MouseButton::Left),
        Act::Release => engine.release_mouse(MouseButton::Left),
        Act::Point(_) | Act::Click | Act::Wait(_) => {}
      }
    }
    match act {
      Act::Key(_) => 3,
      Act::Hold(_) | Act::Let(_) => 1,
      Act::Wait(n) => n,
      _ => 2,
    }
  }

  /// Run `n` frames on both, comparing their UI after each
  fn frames(&mut self, n: u32, what: &str) {
    for _ in 0..n {
      let (on, off) = (self.on.step(), self.off.step());
      self.frames += 1;
      let frame = self.frames;
      assert_eq!(
        (on.unwrap(), off.unwrap()),
        (StepOutcome::Running, StepOutcome::Running),
        "{what}, frame {frame}: the game quit"
      );
      let (on, off) = (self.on.ui_snapshot(), self.off.ui_snapshot());
      if on != off {
        panic!(
          "{what}, frame {frame}: caching on and off differ\n{}",
          difference(&on, &off)
        );
      }
      assert_eq!(
        self.off.ui_cache_stats().replayed,
        0,
        "{what}, frame {frame}: caching off replayed"
      );
      self.replayed += self.on.ui_cache_stats().replayed;
      note_events(&on, &mut self.seen);
      assert!(
        !on
          .lines()
          .any(|l| l.starts_with("state ") && l.ends_with(" ?")),
        "{what}, frame {frame}: a widget state the snapshot cannot show"
      );
    }
  }

  /// The run replayed (so caching was compared), sent each of `events`,
  /// and ends on the same pixels both ways
  fn finish(&self, events: &[&str]) {
    eprintln!(
      "{} frames compared, {} nodes replayed, events {:?}",
      self.frames, self.replayed, self.seen
    );
    assert!(self.replayed > 0, "caching replayed nothing");
    for kind in events {
      assert!(
        self.seen.contains(*kind),
        "the script never sent a {kind} event: {:?}",
        self.seen
      );
    }
    let (Ok(on), Ok(off)) = (self.on.frame(), self.off.frame()) else {
      panic!("no frame presented");
    };
    assert!(
      on.as_raw() == off.as_raw(),
      "the last frames' pixels differ"
    );
  }

  /// The text of the HUD's labels (and any other UI label) in the cached
  /// engine, sorted
  fn labels(&mut self) -> Vec<String> {
    let world = self.on.world_mut();
    let mut labels: Vec<String> = world
      .query::<&UiLabel>()
      .iter(world)
      .map(|label| label.0.clone())
      .collect();
    labels.sort();
    labels
  }
}

/// The node event kinds in a snapshot's events line
fn note_events(snapshot: &str, seen: &mut BTreeSet<String>) {
  let Some(events) = snapshot.lines().find(|l| l.starts_with("events ")) else {
    return;
  };
  for part in events.split("kind: ").skip(1) {
    let kind: String = part.chars().take_while(|c| c.is_alphanumeric()).collect();
    seen.insert(kind);
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
  let around = |s: &str| {
    let from = s.floor_char_boundary(at.saturating_sub(200));
    let to = s.ceil_char_boundary((at + 300).min(s.len()));
    s[from..to].to_string()
  };
  format!(
    "line {line} ({:?}), from byte {at}:\n  on:  ...{}...\n  off: ...{}...",
    x.split(' ').next().unwrap_or(""),
    around(x),
    around(y)
  )
}

use Act::{Click, Hold, Key, Let, Move, Point, Press, Release, Wait};

#[test]
fn the_main_menu_looks_the_same_cached() {
  let Some(mut pair) = both(MenuScene::new) else {
    return;
  };
  pair.run(&[
    // The arrows on the one button, the pointer over it and away
    Key(KeyCode::ArrowDown),
    Key(KeyCode::ArrowUp),
    Point(0),
    Wait(20),
    Move(100.0, 100.0),
    Wait(10),
    // Escape asks to quit, on NO: the arrows walk it, Enter on NO answers
    Key(KeyCode::Escape),
    Wait(10),
    Key(KeyCode::ArrowLeft),
    Key(KeyCode::ArrowRight),
    Key(KeyCode::Enter),
    Wait(10),
    // Again, NO clicked
    Key(KeyCode::Escape),
    Wait(10),
    Point(1),
    Click,
    Wait(10),
    // Enter starts the game: the menu fades out and the HUD in
    Key(KeyCode::Enter),
    Wait(90),
  ]);
  assert!(
    pair.labels().iter().any(|l| l == "Score: 0"),
    "the game started: {:?}",
    pair.labels()
  );
  pair.finish(&["Clicked"]);
}

#[test]
fn the_pause_menu_looks_the_same_cached() {
  let Some(mut pair) = both(|| GameScene::new(0, None)) else {
    return;
  };
  pair.run(&[
    // Escape pauses: the menu opens on RESUME
    Key(KeyCode::Escape),
    Wait(10),
    // Up to the volume sliders, each stepped (their bound texts follow)
    Key(KeyCode::ArrowUp),
    Key(KeyCode::ArrowLeft),
    Key(KeyCode::ArrowLeft),
    Key(KeyCode::ArrowUp),
    Key(KeyCode::ArrowRight),
    Key(KeyCode::ArrowUp),
    Key(KeyCode::ArrowLeft),
    Key(KeyCode::ArrowDown),
    Key(KeyCode::ArrowDown),
    Key(KeyCode::ArrowDown),
    Key(KeyCode::ArrowDown),
    // The pointer: over the buttons, a slider dragged
    Point(4),
    Wait(10),
    Point(0),
    Press,
    Move(560.0, 300.0),
    Move(700.0, 300.0),
    Release,
    Move(100.0, 100.0),
    Wait(10),
    // RESUME clicked, the game runs, Escape pauses again, Escape resumes
    Point(3),
    Click,
    Wait(30),
    Key(KeyCode::Escape),
    Wait(10),
    Key(KeyCode::Escape),
    Wait(30),
    // Paused once more: QUIT TO MENU with the arrows and Enter
    Key(KeyCode::Escape),
    Wait(10),
    Key(KeyCode::ArrowDown),
    Key(KeyCode::Enter),
    Wait(90),
  ]);
  assert!(
    pair.labels().iter().any(|l| l.starts_with("High Score")),
    "back on the main menu: {:?}",
    pair.labels()
  );
  pair.finish(&["Clicked", "Changed"]);
}

#[test]
fn the_hud_during_play_looks_the_same_cached() {
  let Some(mut pair) = both(|| GameScene::new(0, None)) else {
    return;
  };
  let mut script = vec![
    // Space launches the ball; the paddle runs left and right under it
    Key(KeyCode::Space),
    Wait(30),
  ];
  for _ in 0..6 {
    script.extend([
      Hold(KeyCode::KeyA),
      Wait(40),
      Let(KeyCode::KeyA),
      Hold(KeyCode::KeyD),
      Wait(60),
      Let(KeyCode::KeyD),
      Wait(40),
    ]);
  }
  script.extend([
    Key(KeyCode::Escape),
    Wait(10),
    Key(KeyCode::Escape),
    Wait(60),
  ]);
  pair.run(&script);
  let labels = pair.labels();
  assert!(
    labels
      .iter()
      .any(|l| l.starts_with("Score: ") && l != "Score: 0"),
    "blocks broke, and the HUD shows the score: {labels:?}"
  );
  pair.finish(&[]);
}
