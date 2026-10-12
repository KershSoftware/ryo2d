//! Focus management for UI widgets
//!
//! This module provides a unified focus system for retained-mode widgets,
//! enabling Tab/Shift+Tab navigation across different widget types.
//!
//! # Key Types
//!
//! - [`Focusable`] - Trait implemented by widgets that can receive focus
//! - [`FocusTrap`] - Constrains Tab navigation within a modal dialog
//!
//! # Usage
//!
//! ```ignore
//! // In Scene::ui:
//! ui.handle_tab_navigation(&mut [
//!     &mut self.username_input,    // TextInputState
//!     &mut self.difficulty,        // DropdownState
//!     &mut self.volume_slider,     // Can add more focusable types
//! ]);
//! ```

/// Trait for widgets that can receive keyboard focus.
///
/// Implement this for any retained widget to enable unified Tab navigation.
pub trait Focusable {
  /// Check if this widget currently has focus.
  fn is_focused(&self) -> bool;

  /// Set the focus state of this widget.
  fn set_focused(&mut self, focused: bool);

  /// Check if this widget should block Tab navigation (e.g., open dropdown).
  ///
  /// When a widget is "capturing" input, Tab navigation is suspended
  /// to allow the widget to handle input internally (like arrow keys
  /// in an open dropdown).
  fn is_capturing(&self) -> bool {
    false
  }

  /// Called when focus is gained.
  ///
  /// Override to perform actions like resetting blink timers.
  fn on_focus(&mut self) {}

  /// Called when focus is lost.
  ///
  /// Override to perform cleanup like clearing selection.
  fn on_blur(&mut self) {}
}

/// Focus trap for modal dialogs.
///
/// Wraps a slice of focusable widgets and ensures Tab navigation
/// cycles within the group rather than escaping to other widgets.
///
/// # Example
///
/// ```ignore
/// // In a modal dialog scene's Scene::ui:
/// let tab = ui.input().key_pressed(KeyCode::Tab);
/// let shift = ui.input().key_down(KeyCode::ShiftLeft)
///     || ui.input().key_down(KeyCode::ShiftRight);
///
/// let mut widgets: [&mut dyn Focusable; 2] = [&mut self.name_input, &mut self.difficulty];
/// let mut trap = FocusTrap::new(&mut widgets);
///
/// // Tab cycles within the trap unless a widget (an open dropdown) captures input
/// if tab && !trap.is_capturing() {
///     if shift { trap.focus_prev() } else { trap.focus_next() }
/// }
/// ```
pub struct FocusTrap<'a> {
  widgets: &'a mut [&'a mut dyn Focusable],
}

impl<'a> FocusTrap<'a> {
  /// Create a new focus trap containing the given widgets.
  pub fn new(widgets: &'a mut [&'a mut dyn Focusable]) -> Self {
    Self { widgets }
  }

  /// Get the index of the currently focused widget, if any.
  pub fn focused_index(&self) -> Option<usize> {
    self.widgets.iter().position(|w| w.is_focused())
  }

  /// Focus the first widget in the trap.
  pub fn focus_first(&mut self) {
    if self.widgets.is_empty() {
      return;
    }

    // Clear all focus first
    for w in self.widgets.iter_mut() {
      w.set_focused(false);
    }

    // Focus the first widget
    self.widgets[0].set_focused(true);
    self.widgets[0].on_focus();
  }

  /// Focus the last widget in the trap.
  pub fn focus_last(&mut self) {
    if self.widgets.is_empty() {
      return;
    }

    let last_idx = self.widgets.len() - 1;

    // Clear all focus first
    for w in self.widgets.iter_mut() {
      w.set_focused(false);
    }

    // Focus the last widget
    self.widgets[last_idx].set_focused(true);
    self.widgets[last_idx].on_focus();
  }

  /// Move focus to the next widget (wrapping at end).
  pub fn focus_next(&mut self) {
    if self.widgets.is_empty() {
      return;
    }

    let current = self.focused_index();
    let next = match current {
      Some(idx) => (idx + 1) % self.widgets.len(),
      None => 0,
    };

    // Blur current
    if let Some(idx) = current {
      self.widgets[idx].set_focused(false);
      self.widgets[idx].on_blur();
    }

    // Focus next
    self.widgets[next].set_focused(true);
    self.widgets[next].on_focus();
  }

  /// Move focus to the previous widget (wrapping at start).
  pub fn focus_prev(&mut self) {
    if self.widgets.is_empty() {
      return;
    }

    let current = self.focused_index();
    let prev = match current {
      Some(idx) => {
        if idx == 0 {
          self.widgets.len() - 1
        } else {
          idx - 1
        }
      }
      None => self.widgets.len() - 1,
    };

    // Blur current
    if let Some(idx) = current {
      self.widgets[idx].set_focused(false);
      self.widgets[idx].on_blur();
    }

    // Focus prev
    self.widgets[prev].set_focused(true);
    self.widgets[prev].on_focus();
  }

  /// Check if any widget is currently capturing input.
  pub fn is_capturing(&self) -> bool {
    self.widgets.iter().any(|w| w.is_capturing())
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  // Simple test focusable widget
  struct TestWidget {
    focused: bool,
    capturing: bool,
  }

  impl TestWidget {
    fn new() -> Self {
      Self {
        focused: false,
        capturing: false,
      }
    }
  }

  impl Focusable for TestWidget {
    fn is_focused(&self) -> bool {
      self.focused
    }
    fn set_focused(&mut self, focused: bool) {
      self.focused = focused;
    }
    fn is_capturing(&self) -> bool {
      self.capturing
    }
  }

  #[test]
  fn focus_first_sets_focus() {
    let mut w1 = TestWidget::new();
    let mut w2 = TestWidget::new();

    {
      let mut widgets: [&mut dyn Focusable; 2] = [&mut w1, &mut w2];
      let mut trap = FocusTrap::new(&mut widgets);
      trap.focus_first();
    }

    assert!(w1.is_focused());
    assert!(!w2.is_focused());
  }

  #[test]
  fn focus_last_sets_focus() {
    let mut w1 = TestWidget::new();
    let mut w2 = TestWidget::new();

    {
      let mut widgets: [&mut dyn Focusable; 2] = [&mut w1, &mut w2];
      let mut trap = FocusTrap::new(&mut widgets);
      trap.focus_last();
    }

    assert!(!w1.is_focused());
    assert!(w2.is_focused());
  }

  #[test]
  fn focus_next_wraps_around() {
    let mut w1 = TestWidget::new();
    let mut w2 = TestWidget::new();
    w2.focused = true; // Start at last

    {
      let mut widgets: [&mut dyn Focusable; 2] = [&mut w1, &mut w2];
      let mut trap = FocusTrap::new(&mut widgets);
      trap.focus_next();
    }

    assert!(w1.is_focused());
    assert!(!w2.is_focused());
  }

  #[test]
  fn focus_prev_wraps_around() {
    let mut w1 = TestWidget::new();
    let mut w2 = TestWidget::new();
    w1.focused = true; // Start at first

    {
      let mut widgets: [&mut dyn Focusable; 2] = [&mut w1, &mut w2];
      let mut trap = FocusTrap::new(&mut widgets);
      trap.focus_prev();
    }

    assert!(!w1.is_focused());
    assert!(w2.is_focused());
  }

  #[test]
  fn is_capturing_checks_all_widgets() {
    let mut w1 = TestWidget::new();
    let mut w2 = TestWidget {
      focused: false,
      capturing: true,
    };
    let mut widgets: [&mut dyn Focusable; 2] = [&mut w1, &mut w2];

    let trap = FocusTrap::new(&mut widgets);
    assert!(trap.is_capturing());
  }

  #[test]
  fn empty_trap_handles_gracefully() {
    let mut widgets: [&mut dyn Focusable; 0] = [];
    let mut trap = FocusTrap::new(&mut widgets);

    trap.focus_first();
    trap.focus_next();
    trap.focus_prev();
    assert_eq!(trap.focused_index(), None);
  }
}
