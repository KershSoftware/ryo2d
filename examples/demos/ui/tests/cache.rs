//! The ui demo's node pages with caching on and off: two engines driven by
//! the same input must draw and behave the same, frame by frame

use std::collections::BTreeSet;

use demo_kit::prelude::*;
use ryo2d::testing::{HeadlessEngine, StepOutcome};

const FILE_MENUS: usize = 11;
const FILE_SETTINGS: usize = 12;
const FILE_INVENTORY: usize = 13;
const FILE_DIALOGUE: usize = 14;
const MIXED: usize = 15;
const BIG_LIST: usize = 16;

/// One engine with caching on and one with it off, both showing the demo's
/// page `page`
fn both(page: usize) -> Option<Pair> {
  let make = |cache: bool| {
    let demo = ui_demo::demo().start_page(page);
    HeadlessEngine::new_or_skip(demo.engine_builder().ui_cache(cache))
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
  /// A pad button tapped, then 3 frames
  Pad(GamepadButton),
  /// A key tapped, then 3 frames
  Key(KeyCode),
  /// Text typed, then 2 frames
  Type(&'static str),
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
  /// The wheel turned by `dy` lines (+ up), then 3 frames
  Wheel(f32),
  /// The focus given to the n-th widget the pad can reach, then 2 frames
  Focus(usize),
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
    self.frames(6, "the page opening");
    for (i, &act) in script.iter().enumerate() {
      let what = format!("step {i} ({act:?})");
      let frames = self.apply(act, &what);
      self.frames(frames, &what);
    }
  }

  /// Give both engines `act`; the frames to run after it
  fn apply(&mut self, act: Act, what: &str) -> u32 {
    let target = |pair: &Pair, n: usize| {
      let targets = pair.on.ui_focusables();
      match targets.get(n) {
        Some(&target) => target,
        None => panic!("{what}: no widget {n} to reach, only {targets:?}"),
      }
    };
    match act {
      Act::Point(n) => {
        let (_, rect) = target(self, n);
        let (x, y) = (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
        return self.apply(Act::Move(x, y), what);
      }
      Act::Focus(n) => {
        let (id, _) = target(self, n);
        self.each(|e| e.focus_ui(id));
        return 2;
      }
      Act::Click => {
        self.apply(Act::Press, what);
        self.frames(2, what);
        return self.apply(Act::Release, what);
      }
      _ => {}
    }
    self.each(|e| match act {
      Act::Pad(button) => e.tap_pad(button),
      Act::Key(key) => e.tap_key(key),
      Act::Type(text) => e.type_text(text),
      Act::Move(x, y) => e.mouse_move(x, y),
      Act::Press => e.press_mouse(MouseButton::Left),
      Act::Release => e.release_mouse(MouseButton::Left),
      Act::Wheel(dy) => e.wheel(0.0, dy),
      Act::Point(_) | Act::Focus(_) | Act::Click | Act::Wait(_) => {}
    });
    match act {
      Act::Pad(_) | Act::Key(_) | Act::Wheel(_) => 3,
      Act::Wait(n) => n,
      _ => 2,
    }
  }

  fn each(&mut self, mut f: impl FnMut(&mut HeadlessEngine)) {
    f(&mut self.on);
    f(&mut self.off);
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
        "{what}, frame {frame}: the demo quit"
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

use Act::{Click, Focus, Key, Move, Pad, Point, Press, Release, Type, Wait, Wheel};
use GamepadButton::{DPadDown, DPadLeft, DPadRight, DPadUp, East, LeftBumper, RightBumper, South};

#[test]
fn the_file_menu_page_looks_the_same_cached() {
  let Some(mut pair) = both(FILE_MENUS) else {
    return;
  };
  pair.run(&[
    // New Game enables Continue and writes the status; Up reaches it
    Pad(South),
    Pad(DPadUp),
    Pad(South),
    // Options: the slider stepped, the toggle and the checkbox flipped, Back
    Pad(DPadDown),
    Pad(DPadDown),
    Pad(South),
    Wait(4),
    Pad(DPadUp),
    Pad(DPadUp),
    Pad(DPadUp),
    Pad(DPadRight),
    Pad(DPadRight),
    Pad(DPadLeft),
    Pad(DPadDown),
    Pad(South),
    Pad(DPadDown),
    Pad(South),
    Pad(East),
    // Credits: lines spawned into its scroll area, the wheel, Back
    Pad(DPadDown),
    Pad(South),
    Wait(10),
    Move(512.0, 380.0),
    Wheel(-2.0),
    Wait(20),
    Pad(East),
    // The quit confirm: No by Back, then Yes
    Pad(DPadDown),
    Pad(South),
    Pad(East),
    Pad(South),
    Pad(DPadLeft),
    Pad(South),
    // The pointer: across the menu, a click opens Options, Escape closes it
    Move(512.0, 300.0),
    Wait(30),
    Point(2),
    Wait(5),
    Point(3),
    Click,
    Wait(10),
    Key(KeyCode::Escape),
    Point(1),
    Click,
    Wait(10),
    Click,
    // To the next page and back: the page's trees despawned and spawned
    Key(KeyCode::BracketRight),
    Wait(5),
    Key(KeyCode::BracketLeft),
    Wait(10),
    Pad(DPadDown),
    Pad(South),
    Wait(10),
  ]);
  pair.finish(&["Clicked", "Changed", "Back"]);
}

#[test]
fn the_file_settings_page_looks_the_same_cached() {
  let Some(mut pair) = both(FILE_SETTINGS) else {
    return;
  };
  pair.run(&[
    // Audio: the sliders stepped (their bound texts follow), the checkbox
    Focus(3),
    Pad(DPadRight),
    Pad(DPadRight),
    Pad(DPadLeft),
    Pad(DPadDown),
    Pad(DPadLeft),
    Pad(DPadDown),
    Pad(DPadLeft),
    Pad(DPadDown),
    Pad(DPadDown),
    Pad(South),
    // The Output dropdown opened, walked and picked, opened and closed by Back
    Pad(DPadDown),
    Pad(South),
    Pad(DPadDown),
    Pad(South),
    Pad(South),
    Pad(East),
    // Video: a column shown and the others hidden; Window's list, VSync, the
    // radio group and Brightness
    Pad(RightBumper),
    Wait(4),
    Focus(3),
    Pad(South),
    Pad(DPadDown),
    Pad(DPadDown),
    Pad(South),
    Focus(5),
    Pad(South),
    Pad(DPadDown),
    Pad(DPadRight),
    Pad(DPadDown),
    Pad(DPadRight),
    Pad(DPadRight),
    // Controls: a rebinding row captures R, then walks to Reset
    Pad(RightBumper),
    Wait(4),
    Pad(DPadDown),
    Pad(South),
    Wait(3),
    Key(KeyCode::KeyR),
    Wait(30),
    Pad(DPadDown),
    Pad(DPadDown),
    Pad(DPadDown),
    Pad(DPadDown),
    Pad(DPadDown),
    Pad(DPadDown),
    Pad(DPadDown),
    Wheel(-3.0),
    Wait(20),
    Pad(South),
    // Back to Audio with the pointer: the tab clicked, a slider dragged
    Point(0),
    Click,
    Wait(4),
    Point(4),
    Press,
    Move(700.0, 270.0),
    Move(620.0, 270.0),
    Release,
    Move(512.0, 340.0),
    Wheel(-2.0),
    Wait(20),
    Pad(LeftBumper),
    Wait(4),
    Pad(RightBumper),
    Wait(10),
  ]);
  pair.finish(&["Clicked", "Changed"]);
}

#[test]
fn the_file_inventory_page_looks_the_same_cached() {
  let Some(mut pair) = both(FILE_INVENTORY) else {
    return;
  };
  pair.run(&[
    // The pad picks the Sword up and puts it on Weapon
    Pad(South),
    Pad(DPadLeft),
    Pad(DPadDown),
    Pad(DPadDown),
    Pad(South),
    // Picked up and put back with Back
    Pad(DPadRight),
    Pad(South),
    Pad(DPadRight),
    Pad(East),
    // The pointer: hovered slots (the details panel follows), a drag from
    // one bag slot to another and onto Discard
    Point(4),
    Wait(10),
    Point(9),
    Wait(10),
    Point(4),
    Press,
    Move(470.0, 230.0),
    Move(520.0, 260.0),
    Point(10),
    Wait(4),
    Release,
    Wait(4),
    Point(10),
    Press,
    Move(600.0, 450.0),
    Point(28),
    Wait(4),
    Release,
    Wait(4),
    // Sort, with the pad and the pointer
    Point(27),
    Click,
    Wait(10),
    Move(100.0, 100.0),
    Wait(60),
    Pad(DPadDown),
    Pad(DPadDown),
    Pad(DPadDown),
    Pad(DPadDown),
    Pad(South),
    Wait(10),
  ]);
  pair.finish(&["Clicked", "Dropped"]);
}

#[test]
fn the_file_dialogue_page_looks_the_same_cached() {
  let Some(mut pair) = both(FILE_DIALOGUE) else {
    return;
  };
  pair.run(&[
    // The line types itself out, then Skip shows it whole and its choices
    Wait(40),
    Pad(South),
    Wait(10),
    // A choice picked by the pad
    Pad(DPadDown),
    Pad(South),
    Wait(60),
    Pad(South),
    Wait(10),
    // One picked by its number key
    Key(KeyCode::Digit1),
    Wait(90),
    Pad(South),
    Wait(10),
    // The pointer over the choices, one clicked
    Point(1),
    Wait(10),
    Point(0),
    Click,
    Wait(60),
    // Escape asks to leave; Back answers no
    Key(KeyCode::Escape),
    Wait(10),
    Pad(East),
    Wait(10),
    Key(KeyCode::Escape),
    Wait(10),
    Pad(DPadLeft),
    Pad(South),
    Wait(30),
  ]);
  pair.finish(&["Clicked"]);
}

#[test]
fn the_mixed_page_looks_the_same_cached() {
  let Some(mut pair) = both(MIXED) else {
    return;
  };
  pair.run(&[
    // Add spawns cells into the embedded grid
    Pad(South),
    Pad(South),
    Pad(South),
    // Into the grid and across it; a cell picked
    Pad(DPadDown),
    Pad(DPadRight),
    Pad(DPadDown),
    Pad(South),
    Pad(DPadDown),
    Pad(DPadDown),
    Pad(DPadDown),
    Pad(South),
    Pad(DPadUp),
    Pad(DPadUp),
    // The pointer: a cell hovered and clicked, Clear clicked
    Point(5),
    Wait(10),
    Click,
    Point(1),
    Click,
    Wait(5),
    Point(0),
    Click,
    Move(100.0, 100.0),
    Wait(20),
    Pad(DPadDown),
    Pad(DPadDown),
    Pad(South),
    Wait(10),
  ]);
  pair.finish(&["Clicked"]);
}

#[test]
fn the_big_list_page_looks_the_same_cached() {
  let Some(mut pair) = both(BIG_LIST) else {
    return;
  };
  pair.run(&[
    // Down the closure list, then the node list
    Pad(DPadDown),
    Pad(DPadDown),
    Pad(DPadDown),
    Pad(RightBumper),
    Wait(4),
    Pad(DPadDown),
    Pad(DPadDown),
    Pad(DPadDown),
    Pad(DPadDown),
    Pad(DPadDown),
    Pad(DPadDown),
    Wait(20),
    Pad(DPadLeft),
    Pad(DPadRight),
    // The wheel over the node list, and a row hovered and clicked
    Move(760.0, 400.0),
    Wheel(-5.0),
    Wait(20),
    Wheel(3.0),
    Wait(20),
    Click,
    Wait(4),
    // A jump: the field focused, its rank cleared, 5,000 typed, Go
    Focus(0),
    Key(KeyCode::End),
    Key(KeyCode::Backspace),
    Key(KeyCode::Backspace),
    Key(KeyCode::Backspace),
    Key(KeyCode::Backspace),
    Key(KeyCode::Backspace),
    Type("5000"),
    Wait(40),
    Key(KeyCode::Tab),
    Pad(South),
    Wait(30),
    // On from there, and back to the closure list
    Pad(DPadDown),
    Pad(DPadDown),
    Pad(LeftBumper),
    Wait(4),
    Pad(DPadUp),
    Pad(DPadUp),
    Wait(20),
    // The field submitted with Enter
    Focus(0),
    Key(KeyCode::End),
    Key(KeyCode::Backspace),
    Key(KeyCode::Backspace),
    Key(KeyCode::Backspace),
    Key(KeyCode::Backspace),
    Type("12"),
    Key(KeyCode::Enter),
    Wait(30),
  ]);
  pair.finish(&["Clicked"]);
}
