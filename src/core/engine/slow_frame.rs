//! Slow-frame warnings, held until the window is known to have stayed up.

use std::time::Duration;

use web_time::Instant;

/// How long a slow frame's warning waits for the window to hide: macOS says
/// a window is hidden a few milliseconds after the frame that waited on it
const HOLD: Duration = Duration::from_millis(100);

/// One slow frame's warning, held until the window is known to have stayed
/// up: a frame that stalled because the window was being hidden is not the
/// game running slow.
#[derive(Debug, Default)]
pub(super) struct SlowFrameHold {
  held: Option<(Instant, String)>,
}

// Slow-frame warnings are a debug-build check
#[cfg_attr(not(debug_assertions), allow(dead_code))]
impl SlowFrameHold {
  /// Hold the warning for a slow frame that ended at `at`. One still held is
  /// due now: two slow frames in a row are both the game's.
  pub(super) fn hold(&mut self, at: Instant, message: String) -> Option<String> {
    self.held.replace((at, message)).map(|(_, message)| message)
  }

  /// The held warning, once it has waited [`HOLD`] without the window hiding
  pub(super) fn due(&mut self, now: Instant) -> Option<String> {
    match &self.held {
      Some((at, _)) if now.saturating_duration_since(*at) >= HOLD => {
        self.held.take().map(|(_, message)| message)
      }
      _ => None,
    }
  }

  /// The window hid or lost focus, so a held warning was that and not the
  /// game; whether one was dropped
  pub(super) fn window_hidden(&mut self) -> bool {
    self.held.take().is_some()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
  }

  #[test]
  fn a_slow_frame_is_reported_once_the_window_has_stayed_up() {
    let t0 = Instant::now();
    let mut held = SlowFrameHold::default();
    assert_eq!(held.hold(t0, "slow".to_string()), None);
    assert_eq!(held.due(t0 + ms(50)), None, "still held");
    assert_eq!(held.due(t0 + ms(100)), Some("slow".to_string()));
    assert_eq!(held.due(t0 + ms(200)), None, "reported once");
  }

  #[test]
  fn a_slow_frame_the_window_hid_after_is_dropped() {
    // The frame waited on a window macOS was hiding; its notice came after
    let t0 = Instant::now();
    let mut held = SlowFrameHold::default();
    held.hold(t0, "stalled".to_string());
    assert!(held.window_hidden(), "a held frame was dropped");
    assert_eq!(held.due(t0 + ms(200)), None);
    assert!(!held.window_hidden(), "nothing left to drop");
  }

  #[test]
  fn a_second_slow_frame_reports_the_first() {
    let t0 = Instant::now();
    let mut held = SlowFrameHold::default();
    held.hold(t0, "first".to_string());
    assert_eq!(
      held.hold(t0 + ms(10), "second".to_string()),
      Some("first".to_string())
    );
    assert_eq!(held.due(t0 + ms(110)), Some("second".to_string()));
  }
}
