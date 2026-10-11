//! `RYO_INPUT`: scripted keyboard and mouse input for dev runs.
//!
//! Pairs with `RYO_CAPTURE_*` so an agent can open a demo's pages and capture
//! them without a person at the keyboard:
//!
//! ```text
//! RYO_INPUT="60:Digit2 120:ArrowRight 200-260:ArrowUp 300:Click@320x180" RYO_CAPTURE_FRAMES=90,150,250,310 ryo2d dev
//! ```
//!
//! - `N:Key` taps `Key` on presented frame `N`: it goes down before that frame
//!   and comes up once a game update has seen the press.
//! - `A-B:Key` holds `Key` from frame `A` until just before frame `B`.
//! - Keys use winit's `KeyCode` names (`KeyA`, `Digit1`, `ArrowLeft`, `Space`,
//!   `Enter`, `Escape`, `Tab`, `F1`...), in any case.
//! - `N:Mouse@XxY` moves the mouse to game coordinates. `Click` and
//!   `RightClick` press a mouse button like a key, after moving to `@XxY` if
//!   given; a range (`A-B:Click@XxY`) holds it, so a `Mouse` entry inside the
//!   range drags.
//! - Entries are separated by spaces or commas.
//!
//! Read in dev builds only; `HeadlessEngine` tests ignore it.

use winit::event::{ElementState, MouseButton};
use winit::keyboard::KeyCode;

use crate::core::input::InputState;

macro_rules! key_table {
  ($($key:ident),* $(,)?) => {
    &[$((stringify!($key), KeyCode::$key)),*]
  };
}

/// The keys `RYO_INPUT` can name, by winit `KeyCode` name
const KEYS: &[(&str, KeyCode)] = key_table![
  KeyA,
  KeyB,
  KeyC,
  KeyD,
  KeyE,
  KeyF,
  KeyG,
  KeyH,
  KeyI,
  KeyJ,
  KeyK,
  KeyL,
  KeyM,
  KeyN,
  KeyO,
  KeyP,
  KeyQ,
  KeyR,
  KeyS,
  KeyT,
  KeyU,
  KeyV,
  KeyW,
  KeyX,
  KeyY,
  KeyZ,
  Digit0,
  Digit1,
  Digit2,
  Digit3,
  Digit4,
  Digit5,
  Digit6,
  Digit7,
  Digit8,
  Digit9,
  ArrowUp,
  ArrowDown,
  ArrowLeft,
  ArrowRight,
  Space,
  Enter,
  Tab,
  Escape,
  Backspace,
  Delete,
  Insert,
  Home,
  End,
  PageUp,
  PageDown,
  Minus,
  Equal,
  BracketLeft,
  BracketRight,
  Backslash,
  Semicolon,
  Quote,
  Backquote,
  Comma,
  Period,
  Slash,
  ShiftLeft,
  ShiftRight,
  ControlLeft,
  ControlRight,
  AltLeft,
  AltRight,
  F1,
  F2,
  F3,
  F4,
  F5,
  F6,
  F7,
  F8,
  F9,
  F10,
  F11,
  F12,
  Numpad0,
  Numpad1,
  Numpad2,
  Numpad3,
  Numpad4,
  Numpad5,
  Numpad6,
  Numpad7,
  Numpad8,
  Numpad9,
  NumpadAdd,
  NumpadSubtract,
  NumpadMultiply,
  NumpadDivide,
  NumpadDecimal,
  NumpadEnter,
];

/// When a scripted press comes back up
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Release {
  /// Once a game update has seen the press (a tap)
  Seen,
  /// Before this presented frame (a hold)
  Before(u64),
}

/// Something the script can hold down
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Control {
  Key(KeyCode),
  Button(MouseButton),
}

#[derive(Debug, Clone, Copy)]
enum Action {
  /// Press a control, first moving the mouse if a position is given
  Press {
    control: Control,
    at: Option<(f32, f32)>,
    release: Release,
  },
  /// Move the mouse to game coordinates
  MoveMouse(f32, f32),
}

#[derive(Debug, Clone, Copy)]
struct Entry {
  frame: u64,
  action: Action,
}

/// Key presses and mouse input to inject at given presented frames.
#[derive(Debug, Default)]
pub(crate) struct InputScript {
  /// Entries in firing order
  entries: Vec<Entry>,
  next: usize,
  /// Due presses of a control the script still holds, in order; each fires
  /// once its control is up, without holding back the entries after it
  waiting: Vec<Entry>,
  /// Controls the script holds down, and when each comes up
  held: Vec<(Control, Release)>,
}

impl InputScript {
  /// Read `RYO_INPUT`, returning the script and a warning if it was not used.
  pub(crate) fn from_env() -> (Self, Option<String>) {
    let Ok(raw) = std::env::var("RYO_INPUT") else {
      return (Self::default(), None);
    };
    if !cfg!(debug_assertions) {
      let warning = "RYO_INPUT is ignored in release builds".to_string();
      return (Self::default(), Some(warning));
    }
    match Self::parse(&raw) {
      Ok(script) => (script, None),
      Err(warning) => (Self::default(), Some(warning)),
    }
  }

  /// Parse a `RYO_INPUT` value; an invalid entry rejects the whole script.
  pub(crate) fn parse(raw: &str) -> Result<Self, String> {
    let mut entries = Vec::new();
    let tokens = raw
      .split(|c: char| c.is_whitespace() || c == ',')
      .filter(|token| !token.is_empty());
    for token in tokens {
      let entry = parse_entry(token).ok_or_else(|| {
        format!(
          "RYO_INPUT entry {token:?} is invalid (expected FRAME:Input or FROM-TO:Input, where Input is a winit KeyCode name, Click[@XxY], RightClick[@XxY] or Mouse@XxY, e.g. \"60:Digit2\", \"60-90:ArrowUp\" or \"60:Click@320x180\"); scripted input disabled"
        )
      })?;
      entries.push(entry);
    }
    // Stable, so entries on one frame keep their written order
    entries.sort_by_key(|entry| entry.frame);
    Ok(Self {
      entries,
      ..Self::default()
    })
  }

  /// Apply the input due before presented frame `frame` (1-based).
  pub(crate) fn before_frame(&mut self, frame: u64, input: &mut InputState) {
    self.held.retain(|&(control, release)| {
      let ends = matches!(release, Release::Before(end) if end <= frame);
      if ends {
        set(input, control, ElementState::Released);
      }
      !ends
    });

    // Presses that were waiting go first, in order, then newly due entries
    let waiting = std::mem::take(&mut self.waiting);
    for entry in waiting {
      self.fire(entry, input);
    }
    while let Some(&entry) = self.entries.get(self.next) {
      if entry.frame > frame {
        break;
      }
      self.fire(entry, input);
      self.next += 1;
    }
  }

  /// Apply a due entry, or keep it waiting while its control is still held
  /// (a control the script holds is pressed again only once it is up)
  fn fire(&mut self, entry: Entry, input: &mut InputState) {
    match entry.action {
      Action::MoveMouse(x, y) => input.handle_mouse_move(f64::from(x), f64::from(y)),
      Action::Press {
        control,
        at,
        release,
      } => {
        let held = self.held.iter().any(|(held, _)| *held == control);
        // An earlier press of the same control is already waiting: keep order
        let queued = self.waiting.iter().any(
          |waiting| matches!(waiting.action, Action::Press { control: c, .. } if c == control),
        );
        if held || queued {
          self.waiting.push(entry);
          return;
        }
        if let Some((x, y)) = at {
          input.handle_mouse_move(f64::from(x), f64::from(y));
        }
        set(input, control, ElementState::Pressed);
        self.held.push((control, release));
      }
    }
  }

  /// Release taps whose press a game update has seen.
  pub(crate) fn after_frame(&mut self, input: &mut InputState) {
    self.held.retain(|&(control, release)| {
      let seen = release == Release::Seen && !press_pending(input, control);
      if seen {
        set(input, control, ElementState::Released);
      }
      !seen
    });
  }
}

fn set(input: &mut InputState, control: Control, state: ElementState) {
  match control {
    Control::Key(key) => input.handle_keyboard(key, state),
    Control::Button(button) => input.handle_mouse_button(button, state),
  }
}

fn press_pending(input: &InputState, control: Control) -> bool {
  match control {
    Control::Key(key) => input.key_press_pending(key),
    Control::Button(button) => input.mouse_press_pending(button),
  }
}

fn parse_entry(token: &str) -> Option<Entry> {
  let (frames, input) = token.split_once(':')?;
  let (frame, release) = match frames.split_once('-') {
    Some((from, to)) => {
      let (from, to): (u64, u64) = (from.parse().ok()?, to.parse().ok()?);
      (to > from).then_some((from, Release::Before(to)))?
    }
    None => (frames.parse().ok()?, Release::Seen),
  };
  if frame < 1 {
    return None;
  }

  let (name, at) = match input.split_once('@') {
    Some((name, point)) => (name, Some(parse_point(point)?)),
    None => (input, None),
  };
  let action = if name.eq_ignore_ascii_case("Mouse") {
    // A move happens once, so it takes no range
    let (x, y) = at?;
    (release == Release::Seen).then_some(Action::MoveMouse(x, y))?
  } else {
    let control = if name.eq_ignore_ascii_case("Click") {
      Control::Button(MouseButton::Left)
    } else if name.eq_ignore_ascii_case("RightClick") {
      Control::Button(MouseButton::Right)
    } else if at.is_none() {
      let key = KEYS
        .iter()
        .find(|(known, _)| known.eq_ignore_ascii_case(name))?
        .1;
      Control::Key(key)
    } else {
      return None;
    };
    Action::Press {
      control,
      at,
      release,
    }
  };
  Some(Entry { frame, action })
}

/// `XxY` in game coordinates, e.g. `320x180`
fn parse_point(raw: &str) -> Option<(f32, f32)> {
  let (x, y) = raw.split_once('x')?;
  Some((x.parse().ok()?, y.parse().ok()?))
}

#[cfg(test)]
mod tests {
  use super::*;

  fn script(raw: &str) -> InputScript {
    InputScript::parse(raw).unwrap_or_else(|e| panic!("{raw:?}: {e}"))
  }

  /// One engine frame: inject, run an update if `update`, then release
  fn frame(script: &mut InputScript, input: &mut InputState, n: u64, update: bool) {
    script.before_frame(n, input);
    if update {
      input.begin_frame();
    }
    script.after_frame(input);
  }

  #[test]
  fn tap_goes_down_on_its_frame_and_up_after_an_update() {
    let mut s = script("3:Space");
    let mut input = InputState::new();

    s.before_frame(2, &mut input);
    assert!(!input.key_down(KeyCode::Space));
    s.after_frame(&mut input);

    s.before_frame(3, &mut input);
    assert!(input.key_pressed(KeyCode::Space));
    assert!(input.key_down(KeyCode::Space));
    input.begin_frame(); // the update saw it
    s.after_frame(&mut input);

    assert!(!input.key_down(KeyCode::Space));
    assert!(input.key_released(KeyCode::Space));
  }

  #[test]
  fn tap_waits_for_a_frame_that_runs_an_update() {
    let mut s = script("1:KeyA");
    let mut input = InputState::new();

    frame(&mut s, &mut input, 1, false);
    assert!(
      input.key_pressed(KeyCode::KeyA),
      "no update yet: still pending"
    );
    assert!(input.key_down(KeyCode::KeyA));

    frame(&mut s, &mut input, 2, true);
    assert!(!input.key_down(KeyCode::KeyA));
  }

  #[test]
  fn hold_stays_down_until_its_end_frame() {
    let mut s = script("2-4:ArrowUp");
    let mut input = InputState::new();

    frame(&mut s, &mut input, 1, true);
    assert!(!input.key_down(KeyCode::ArrowUp));
    for n in 2..4 {
      frame(&mut s, &mut input, n, true);
      assert!(input.key_down(KeyCode::ArrowUp), "frame {n}");
    }
    s.before_frame(4, &mut input);
    assert!(!input.key_down(KeyCode::ArrowUp));
    assert!(input.key_released(KeyCode::ArrowUp));
  }

  #[test]
  fn repeated_taps_of_one_key_each_register_a_press() {
    let mut s = script("1:Digit2,1:Digit2 2:Digit2");
    let mut input = InputState::new();
    let mut presses = 0;
    for n in 1..=6 {
      s.before_frame(n, &mut input);
      if input.key_pressed(KeyCode::Digit2) {
        presses += 1;
      }
      input.begin_frame();
      s.after_frame(&mut input);
    }
    assert_eq!(presses, 3);
  }

  #[test]
  fn a_press_waiting_on_a_held_key_delays_nothing_else() {
    // The second ArrowRight waits for the hold to end at frame 6; the Space
    // and the click due meanwhile still fire on their frames
    let mut s = script("2-6:ArrowRight 3:ArrowRight 4:Space 5:Click@10x10");
    let mut input = InputState::new();
    let mut space = None;
    let mut click = None;
    let mut second_right = None;
    for n in 1..=9 {
      s.before_frame(n, &mut input);
      if input.key_pressed(KeyCode::Space) {
        space = Some(n);
      }
      if input.mouse_pressed(MouseButton::Left) {
        click = Some(n);
      }
      if n > 2 && input.key_pressed(KeyCode::ArrowRight) {
        second_right = Some(n);
      }
      input.begin_frame();
      s.after_frame(&mut input);
    }
    assert_eq!(space, Some(4));
    assert_eq!(click, Some(5));
    assert_eq!(second_right, Some(6), "pressed again once the hold ends");
  }

  #[test]
  fn a_late_frame_fires_everything_due() {
    let mut s = script("2:KeyQ 5:KeyW");
    let mut input = InputState::new();
    s.before_frame(9, &mut input);
    assert!(input.key_pressed(KeyCode::KeyQ));
    assert!(input.key_pressed(KeyCode::KeyW));
  }

  #[test]
  fn key_names_ignore_case() {
    let mut s = script("1:arrowright 1:ESCAPE 1:f1 1:digit0");
    let mut input = InputState::new();
    s.before_frame(1, &mut input);
    for key in [
      KeyCode::ArrowRight,
      KeyCode::Escape,
      KeyCode::F1,
      KeyCode::Digit0,
    ] {
      assert!(input.key_down(key), "{key:?}");
    }
  }

  #[test]
  fn invalid_entries_reject_the_script_and_name_the_entry() {
    for raw in [
      "abc",
      "60",
      "0:Space",
      "x:Space",
      "60:Nope",
      "60:",
      "90-60:Space",
      "60-60:Space",
      "60:Space 70:Bogus",
    ] {
      let err = InputScript::parse(raw)
        .err()
        .unwrap_or_else(|| panic!("{raw:?} accepted"));
      let bad = raw.split_whitespace().last().unwrap_or(raw);
      assert!(err.contains(bad), "{raw:?}: {err}");
    }
  }

  #[test]
  fn mouse_moves_to_game_coordinates_on_its_frame() {
    let mut s = script("3:Mouse@120.5x40");
    let mut input = InputState::new();
    frame(&mut s, &mut input, 2, true);
    assert_eq!(input.mouse_position(), (0.0, 0.0));
    s.before_frame(3, &mut input);
    assert_eq!(input.mouse_position(), (120.5, 40.0));
    assert!(!input.mouse_down(MouseButton::Left));
  }

  #[test]
  fn click_moves_then_taps_the_left_button() {
    let mut s = script("2:Click@64x32");
    let mut input = InputState::new();
    s.before_frame(2, &mut input);
    assert_eq!(input.mouse_position(), (64.0, 32.0));
    assert!(input.mouse_pressed(MouseButton::Left));
    input.begin_frame();
    s.after_frame(&mut input);
    assert!(!input.mouse_down(MouseButton::Left));
    assert!(input.mouse_released(MouseButton::Left));
  }

  #[test]
  fn held_click_drags_until_its_end_frame() {
    let mut s = script("2-5:Click@10x10 3:Mouse@30x10");
    let mut input = InputState::new();
    frame(&mut s, &mut input, 2, true);
    assert!(input.mouse_down(MouseButton::Left));
    frame(&mut s, &mut input, 3, true);
    assert!(input.mouse_down(MouseButton::Left));
    assert_eq!(input.mouse_position(), (30.0, 10.0));
    s.before_frame(5, &mut input);
    assert!(!input.mouse_down(MouseButton::Left));
  }

  #[test]
  fn right_click_without_coordinates_clicks_in_place() {
    let mut s = script("1:Mouse@7x9 2:RightClick");
    let mut input = InputState::new();
    frame(&mut s, &mut input, 1, true);
    s.before_frame(2, &mut input);
    assert!(input.mouse_pressed(MouseButton::Right));
    assert_eq!(input.mouse_position(), (7.0, 9.0));
  }

  #[test]
  fn invalid_mouse_entries_are_rejected() {
    for raw in [
      "1:Mouse",
      "1-3:Mouse@1x1",
      "1:Click@1x",
      "1:Click@ax2",
      "1:Click@1x2x3",
      "1:Wheel@1x2",
    ] {
      let err = InputScript::parse(raw)
        .err()
        .unwrap_or_else(|| panic!("{raw:?} accepted"));
      assert!(err.contains(raw), "{raw:?}: {err}");
    }
  }

  #[test]
  fn blank_script_does_nothing() {
    let mut s = script("  ");
    let mut input = InputState::new();
    s.before_frame(1, &mut input);
    s.after_frame(&mut input);
    input.handle_keyboard(KeyCode::Space, ElementState::Pressed);
    assert!(input.key_down(KeyCode::Space));
  }
}
