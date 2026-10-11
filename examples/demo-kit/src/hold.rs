//! Hold-to-repeat for the settings panel's Left/Right keys

/// Seconds a key is held before it starts repeating
pub(crate) const REPEAT_DELAY: f32 = 0.35;
/// Repeats per second once repeating
pub(crate) const REPEAT_RATE: f32 = 12.0;

/// Turns a key's press and held state into adjustment steps.
///
/// A key only repeats after this tracker has seen it pressed, so a key still
/// held when a page is rebuilt does nothing until pressed again.
#[derive(Debug, Default)]
pub(crate) struct HoldRepeat {
  armed: bool,
  held: f32,
  repeats: u32,
}

impl HoldRepeat {
  /// Steps to apply this update, given whether the key was just pressed,
  /// whether it is down, and the update's unscaled time step.
  pub(crate) fn steps(&mut self, pressed: bool, down: bool, dt: f32) -> u32 {
    if pressed {
      self.armed = true;
      self.held = 0.0;
      self.repeats = 0;
      return 1;
    }
    if !down || !self.armed {
      self.armed = false;
      self.held = 0.0;
      return 0;
    }
    self.held += dt;
    if self.held < REPEAT_DELAY {
      return 0;
    }
    let due = ((self.held - REPEAT_DELAY) * REPEAT_RATE) as u32 + 1;
    let new = due.saturating_sub(self.repeats);
    self.repeats = due;
    new
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn a_press_steps_once_then_repeats_after_the_delay() {
    let mut hold = HoldRepeat::default();
    assert_eq!(hold.steps(true, true, 0.0), 1);
    assert_eq!(hold.steps(false, true, 0.3), 0); // held 0.3 s, still inside the delay
    assert_eq!(hold.steps(false, true, 0.2), 2); // held 0.5 s: 0.15 s past the delay
    assert_eq!(hold.steps(false, true, 0.25), 3); // held 0.75 s
  }

  #[test]
  fn releasing_stops_repeats() {
    let mut hold = HoldRepeat::default();
    hold.steps(true, true, 0.0);
    assert_eq!(hold.steps(false, false, 1.0), 0);
  }

  #[test]
  fn down_without_a_press_never_repeats() {
    let mut hold = HoldRepeat::default();
    for _ in 0..100 {
      assert_eq!(hold.steps(false, true, 0.1), 0);
    }
  }
}
