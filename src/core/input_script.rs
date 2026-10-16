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
//! - `N:Wheel@DXxDY` scrolls the wheel by `DX`,`DY` lines on frame `N`.
//! - `Pad<Button>` (`PadSouth`, `PadDPadDown`, `PadLeftBumper`...) presses a
//!   gamepad button on a virtual pad, like a key (tap or `A-B` hold).
//! - `A-B:LeftStick@XxY` and `A-B:RightStick@XxY` hold a stick position (-1..1,
//!   +y up) from frame `A` and centre it before frame `B`.
//! - Entries are separated by spaces or commas.
//!
//! Read in dev builds only; `HeadlessEngine` tests ignore it.

use winit::event::{ElementState, MouseButton, MouseScrollDelta};
use winit::keyboard::KeyCode;

use crate::core::input::actions::{key_from_name, pad_button_from_name};
use crate::core::input::{GamepadButton, Input, InputEvent, Stick};

/// When a scripted press comes back up
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Release {
  /// Once a game update has seen the press (a tap)
  Seen,
  /// Before this presented frame (a hold)
  Before(u64),
}

/// Something the script can hold down
#[derive(Debug, Clone, Copy, PartialEq)]
enum Control {
  Key(KeyCode),
  Button(MouseButton),
  Pad(GamepadButton),
  /// A stick held at a position
  Stick(Stick, f32, f32),
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
  /// Scroll the wheel by lines
  Wheel(f32, f32),
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
          "RYO_INPUT entry {token:?} is invalid (expected FRAME:Input or FROM-TO:Input, where Input is a winit KeyCode name, Click[@XxY], RightClick[@XxY], Mouse@XxY, Wheel@DXxDY, Pad<Button> (e.g. PadSouth, PadDPadDown), LeftStick@XxY or RightStick@XxY, e.g. \"60:Digit2\", \"60-90:ArrowUp\", \"60:Click@320x180\" or \"90:Wheel@0x-1\"); scripted input disabled"
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

  /// Queue the input due before presented frame `frame` (1-based); call it
  /// before the frame publishes input.
  pub(crate) fn before_frame(&mut self, frame: u64, input: &mut Input) {
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
  fn fire(&mut self, entry: Entry, input: &mut Input) {
    match entry.action {
      Action::MoveMouse(x, y) => input.queue(InputEvent::MouseMove {
        x: f64::from(x),
        y: f64::from(y),
      }),
      Action::Wheel(dx, dy) => input.queue(InputEvent::Scroll(MouseScrollDelta::LineDelta(dx, dy))),
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
          input.queue(InputEvent::MouseMove {
            x: f64::from(x),
            y: f64::from(y),
          });
        }
        set(input, control, ElementState::Pressed);
        self.held.push((control, release));
      }
    }
  }

  /// Queue the release of taps whose press a fixed update has seen.
  pub(crate) fn after_frame(&mut self, input: &mut Input) {
    self.held.retain(|&(control, release)| {
      let seen = release == Release::Seen && !press_pending(input, control);
      if seen {
        set(input, control, ElementState::Released);
      }
      !seen
    });
  }
}

/// Queue a press or release; it takes effect when the frame publishes input.
fn set(input: &mut Input, control: Control, state: ElementState) {
  let pressed = state == ElementState::Pressed;
  input.queue(match control {
    Control::Key(key) => InputEvent::Key { key, pressed },
    Control::Button(button) => InputEvent::MouseButton { button, pressed },
    Control::Pad(button) => InputEvent::PadButton { button, pressed },
    Control::Stick(stick, x, y) => {
      let (x, y) = if pressed { (x, y) } else { (0.0, 0.0) };
      InputEvent::PadStick { stick, x, y }
    }
  });
}

fn press_pending(input: &Input, control: Control) -> bool {
  match control {
    Control::Key(key) => input.key_press_pending(key),
    Control::Button(button) => input.mouse_press_pending(button),
    Control::Pad(button) => input.gamepad_press_pending(button),
    // A stick has no press to wait for: a tap centres before the next frame
    Control::Stick(..) => false,
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
  let action = if name.eq_ignore_ascii_case("Wheel") {
    // A wheel step happens once, so it takes a delta and no range
    let (dx, dy) = at?;
    (release == Release::Seen).then_some(Action::Wheel(dx, dy))?
  } else if name.eq_ignore_ascii_case("Mouse") {
    // A move happens once, so it takes no range
    let (x, y) = at?;
    (release == Release::Seen).then_some(Action::MoveMouse(x, y))?
  } else {
    let control = if name.eq_ignore_ascii_case("Click") {
      Control::Button(MouseButton::Left)
    } else if name.eq_ignore_ascii_case("RightClick") {
      Control::Button(MouseButton::Right)
    } else if name.eq_ignore_ascii_case("LeftStick") {
      let (x, y) = at?;
      Control::Stick(Stick::Left, x, y)
    } else if name.eq_ignore_ascii_case("RightStick") {
      let (x, y) = at?;
      Control::Stick(Stick::Right, x, y)
    } else if let Some(pad) = pad_prefix(name) {
      if at.is_some() {
        return None;
      }
      let button = pad_button_from_name(pad)?;
      Control::Pad(button)
    } else if at.is_none() {
      let key = key_from_name(name)?;
      Control::Key(key)
    } else {
      return None;
    };
    // A stick's position is its own, not a mouse move
    let at = if matches!(control, Control::Stick(..)) {
      None
    } else {
      at
    };
    Action::Press {
      control,
      at,
      release,
    }
  };
  Some(Entry { frame, action })
}

/// The part of `name` after a case-insensitive `Pad` prefix
fn pad_prefix(name: &str) -> Option<&str> {
  let head = name.get(..3)?;
  head.eq_ignore_ascii_case("Pad").then(|| &name[3..])
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

  /// Start engine frame `n`: the script queues, then `First` publishes input
  fn start(script: &mut InputScript, input: &mut Input, n: u64) {
    script.before_frame(n, input);
    input.begin_frame();
  }

  /// Finish the frame: a fixed step if `update`, then the script releases
  /// what an update saw (released at the next start)
  fn finish(script: &mut InputScript, input: &mut Input, update: bool) {
    if update {
      input.set_fixed_step(0);
    }
    input.end_fixed_steps(update);
    script.after_frame(input);
  }

  fn frame(script: &mut InputScript, input: &mut Input, n: u64, update: bool) {
    start(script, input, n);
    finish(script, input, update);
  }

  #[test]
  fn tap_goes_down_on_its_frame_and_up_after_an_update() {
    let mut s = script("3:Space");
    let mut input = Input::new();

    start(&mut s, &mut input, 2);
    assert!(!input.key_down(KeyCode::Space));
    finish(&mut s, &mut input, true);

    start(&mut s, &mut input, 3);
    assert!(input.key_pressed(KeyCode::Space));
    assert!(input.key_down(KeyCode::Space));
    finish(&mut s, &mut input, true); // the update saw it

    start(&mut s, &mut input, 4);
    assert!(!input.key_down(KeyCode::Space));
    assert!(input.key_released(KeyCode::Space));
  }

  #[test]
  fn tap_waits_for_a_frame_that_runs_an_update() {
    let mut s = script("1:KeyA");
    let mut input = Input::new();

    frame(&mut s, &mut input, 1, false);
    assert!(
      input.key_pressed(KeyCode::KeyA),
      "no update yet: still pending"
    );
    assert!(input.key_down(KeyCode::KeyA));

    frame(&mut s, &mut input, 2, true);
    start(&mut s, &mut input, 3);
    assert!(!input.key_down(KeyCode::KeyA));
  }

  #[test]
  fn hold_stays_down_until_its_end_frame() {
    let mut s = script("2-4:ArrowUp");
    let mut input = Input::new();

    frame(&mut s, &mut input, 1, true);
    assert!(!input.key_down(KeyCode::ArrowUp));
    for n in 2..4 {
      frame(&mut s, &mut input, n, true);
      assert!(input.key_down(KeyCode::ArrowUp), "frame {n}");
    }
    start(&mut s, &mut input, 4);
    assert!(!input.key_down(KeyCode::ArrowUp));
    assert!(input.key_released(KeyCode::ArrowUp));
  }

  #[test]
  fn repeated_taps_of_one_key_each_register_a_press() {
    let mut s = script("1:Digit2,1:Digit2 2:Digit2");
    let mut input = Input::new();
    let mut presses = 0;
    for n in 1..=6 {
      start(&mut s, &mut input, n);
      if input.key_pressed(KeyCode::Digit2) {
        presses += 1;
      }
      finish(&mut s, &mut input, true);
    }
    assert_eq!(presses, 3);
  }

  #[test]
  fn a_press_waiting_on_a_held_key_delays_nothing_else() {
    // The second ArrowRight waits for the hold to end at frame 6; the Space
    // and the click due meanwhile still fire on their frames
    let mut s = script("2-6:ArrowRight 3:ArrowRight 4:Space 5:Click@10x10");
    let mut input = Input::new();
    let mut space = None;
    let mut click = None;
    let mut second_right = None;
    for n in 1..=9 {
      start(&mut s, &mut input, n);
      if input.key_pressed(KeyCode::Space) {
        space = Some(n);
      }
      if input.mouse_pressed(MouseButton::Left) {
        click = Some(n);
      }
      if n > 2 && input.key_pressed(KeyCode::ArrowRight) {
        second_right = Some(n);
      }
      finish(&mut s, &mut input, true);
    }
    assert_eq!(space, Some(4));
    assert_eq!(click, Some(5));
    assert_eq!(second_right, Some(6), "pressed again once the hold ends");
  }

  #[test]
  fn a_late_frame_fires_everything_due() {
    let mut s = script("2:KeyQ 5:KeyW");
    let mut input = Input::new();
    start(&mut s, &mut input, 9);
    assert!(input.key_pressed(KeyCode::KeyQ));
    assert!(input.key_pressed(KeyCode::KeyW));
  }

  #[test]
  fn key_names_ignore_case() {
    let mut s = script("1:arrowright 1:ESCAPE 1:f1 1:digit0");
    let mut input = Input::new();
    start(&mut s, &mut input, 1);
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
    let mut input = Input::new();
    frame(&mut s, &mut input, 2, true);
    assert_eq!(input.mouse_position(), (0.0, 0.0));
    start(&mut s, &mut input, 3);
    assert_eq!(input.mouse_position(), (120.5, 40.0));
    assert!(!input.mouse_down(MouseButton::Left));
  }

  #[test]
  fn click_moves_then_taps_the_left_button() {
    let mut s = script("2:Click@64x32");
    let mut input = Input::new();
    start(&mut s, &mut input, 2);
    assert_eq!(input.mouse_position(), (64.0, 32.0));
    assert!(input.mouse_pressed(MouseButton::Left));
    finish(&mut s, &mut input, true);
    start(&mut s, &mut input, 3);
    assert!(!input.mouse_down(MouseButton::Left));
    assert!(input.mouse_released(MouseButton::Left));
  }

  #[test]
  fn held_click_drags_until_its_end_frame() {
    let mut s = script("2-5:Click@10x10 3:Mouse@30x10");
    let mut input = Input::new();
    frame(&mut s, &mut input, 2, true);
    assert!(input.mouse_down(MouseButton::Left));
    frame(&mut s, &mut input, 3, true);
    assert!(input.mouse_down(MouseButton::Left));
    assert_eq!(input.mouse_position(), (30.0, 10.0));
    start(&mut s, &mut input, 5);
    assert!(!input.mouse_down(MouseButton::Left));
  }

  #[test]
  fn right_click_without_coordinates_clicks_in_place() {
    let mut s = script("1:Mouse@7x9 2:RightClick");
    let mut input = Input::new();
    frame(&mut s, &mut input, 1, true);
    start(&mut s, &mut input, 2);
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
    let mut input = Input::new();
    s.before_frame(1, &mut input);
    s.after_frame(&mut input);
    input.handle_keyboard(KeyCode::Space, ElementState::Pressed);
    assert!(input.key_down(KeyCode::Space));
  }

  #[test]
  fn wheel_entries_scroll_on_their_frame() {
    let mut s = script("3:Wheel@0x-2");
    let mut input = Input::new();
    start(&mut s, &mut input, 2);
    assert_eq!(input.scroll_delta(), (0.0, 0.0));
    finish(&mut s, &mut input, true);
    start(&mut s, &mut input, 3);
    assert_eq!(input.scroll_delta(), (0.0, -2.0));
  }

  #[test]
  fn pad_taps_and_holds_press_the_virtual_pad() {
    let mut s = script("1:PadSouth 2-4:PadDPadDown");
    let mut input = Input::new();
    start(&mut s, &mut input, 1);
    assert!(input.gamepad_button_pressed(GamepadButton::South));
    finish(&mut s, &mut input, true);
    start(&mut s, &mut input, 2);
    assert!(
      !input.gamepad_button_down(GamepadButton::South),
      "a tap comes up once an update saw it"
    );
    assert!(input.gamepad_button_down(GamepadButton::DPadDown));
    finish(&mut s, &mut input, true);
    start(&mut s, &mut input, 3);
    assert!(input.gamepad_button_down(GamepadButton::DPadDown));
    finish(&mut s, &mut input, true);
    start(&mut s, &mut input, 4);
    assert!(
      !input.gamepad_button_down(GamepadButton::DPadDown),
      "held until just before frame 4"
    );
  }

  #[test]
  fn stick_ranges_hold_a_position_then_centre() {
    let mut s = script("1-3:LeftStick@0x1");
    let mut input = Input::new();
    start(&mut s, &mut input, 1);
    assert_eq!(input.left_stick_raw(), (0.0, 1.0));
    finish(&mut s, &mut input, true);
    start(&mut s, &mut input, 3);
    assert_eq!(input.left_stick_raw(), (0.0, 0.0));
  }

  #[test]
  fn pad_and_wheel_names_are_checked() {
    for bad in [
      "1:PadNope",
      "1:Wheel",
      "1-3:Wheel@0x1",
      "1:LeftStick",
      "1:PadSouth@1x1",
    ] {
      assert!(InputScript::parse(bad).is_err(), "{bad} must be rejected");
    }
    for good in [
      "1:padsouth",
      "1:PadDPadUp",
      "1:Wheel@1x0",
      "1:RightStick@-1x0",
    ] {
      assert!(InputScript::parse(good).is_ok(), "{good} must parse");
    }
  }
}
