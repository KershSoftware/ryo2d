//! Dropdown/select widget for choosing from a list of options
//!
//! A retained-mode widget that provides a dropdown selector. The user stores
//! a `DropdownState` and passes it each frame to render the dropdown.
//!
//! # Example
//!
//! ```ignore
//! // In your scene/game struct:
//! dropdown_state: DropdownState,
//!
//! // In render_ui:
//! let options = ["Easy", "Medium", "Hard"];
//! let rect = Rect::new(100.0, 100.0, 200.0, 30.0);
//!
//! if ui.dropdown(&mut self.dropdown_state, rect, &options) {
//!     println!("Selected: {}", options[self.dropdown_state.selected()]);
//! }
//! ```

/// State for a dropdown widget.
///
/// This is a retained-mode widget - you store this in your scene/game and
/// pass it to the UI context each frame.
#[derive(Debug, Clone)]
pub struct DropdownState {
  /// Currently selected index
  selected: usize,
  /// Whether the dropdown list is open
  open: bool,
  /// Keyboard navigation index (when open)
  hover_index: usize,
  /// Unique ID for this dropdown (for focus management)
  id: u32,
  /// Whether this dropdown is focused (for tab navigation)
  focused: bool,
  /// Whether this dropdown was just closed this frame (prevents re-open on same keypress)
  just_closed: bool,
}

impl Default for DropdownState {
  fn default() -> Self {
    Self::new()
  }
}

impl DropdownState {
  /// Create a new dropdown state with the first option selected.
  pub fn new() -> Self {
    Self {
      selected: 0,
      open: false,
      hover_index: 0,
      id: 0,
      focused: false,
      just_closed: false,
    }
  }

  /// Create a dropdown state with a specific initial selection.
  pub fn with_selected(selected: usize) -> Self {
    Self {
      selected,
      open: false,
      hover_index: selected,
      id: 0,
      focused: false,
      just_closed: false,
    }
  }

  /// Create a dropdown with a unique ID (for focus management).
  pub fn with_id(id: u32) -> Self {
    Self {
      selected: 0,
      open: false,
      hover_index: 0,
      id,
      focused: false,
      just_closed: false,
    }
  }

  /// Create a dropdown with a unique ID and initial selection.
  pub fn with_id_and_selected(id: u32, selected: usize) -> Self {
    Self {
      selected,
      open: false,
      hover_index: selected,
      id,
      focused: false,
      just_closed: false,
    }
  }

  /// Get the currently selected index.
  pub fn selected(&self) -> usize {
    self.selected
  }

  /// Set the selected index directly.
  pub fn set_selected(&mut self, index: usize) {
    self.selected = index;
    self.hover_index = index;
  }

  /// Check if the dropdown is currently open.
  pub fn is_open(&self) -> bool {
    self.open
  }

  /// Open the dropdown.
  pub fn open(&mut self) {
    self.open = true;
    self.hover_index = self.selected;
    self.just_closed = false;
  }

  /// Close the dropdown.
  pub fn close(&mut self) {
    self.open = false;
    self.just_closed = true;
  }

  /// Check if the dropdown was just closed this frame.
  /// Used to prevent re-opening on the same keypress.
  pub fn was_just_closed(&self) -> bool {
    self.just_closed
  }

  /// Clear the just_closed flag (call at start of each frame).
  pub fn clear_just_closed(&mut self) {
    self.just_closed = false;
  }

  /// Toggle the dropdown open/closed state.
  pub fn toggle(&mut self) {
    if self.open {
      self.close();
    } else {
      self.open();
    }
  }

  /// Get the dropdown's unique ID.
  pub fn id(&self) -> u32 {
    self.id
  }

  /// Check if this dropdown is focused (for tab navigation).
  pub fn is_focused(&self) -> bool {
    self.focused
  }

  /// Set whether this dropdown is focused.
  pub fn set_focused(&mut self, focused: bool) {
    self.focused = focused;
  }

  /// Get the current hover index (for keyboard navigation).
  pub fn hover_index(&self) -> usize {
    self.hover_index
  }

  /// Move hover up one item.
  pub fn hover_up(&mut self) {
    if self.hover_index > 0 {
      self.hover_index -= 1;
    }
  }

  /// Move hover down one item, clamped to max_index.
  pub fn hover_down(&mut self, max_index: usize) {
    if self.hover_index < max_index {
      self.hover_index += 1;
    }
  }

  /// Select the currently hovered item.
  pub fn select_hovered(&mut self) {
    self.selected = self.hover_index;
    self.close();
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn new_creates_default_state() {
    let state = DropdownState::new();
    assert_eq!(state.selected(), 0);
    assert!(!state.is_open());
  }

  #[test]
  fn with_selected_sets_initial_selection() {
    let state = DropdownState::with_selected(2);
    assert_eq!(state.selected(), 2);
    assert_eq!(state.hover_index(), 2);
  }

  #[test]
  fn open_sets_hover_to_selected() {
    let mut state = DropdownState::with_selected(3);
    state.open();
    assert!(state.is_open());
    assert_eq!(state.hover_index(), 3);
  }

  #[test]
  fn toggle_flips_open_state() {
    let mut state = DropdownState::new();
    assert!(!state.is_open());
    state.toggle();
    assert!(state.is_open());
    state.toggle();
    assert!(!state.is_open());
  }

  #[test]
  fn hover_navigation() {
    let mut state = DropdownState::new();
    state.open();

    // Move down
    state.hover_down(4);
    assert_eq!(state.hover_index(), 1);
    state.hover_down(4);
    assert_eq!(state.hover_index(), 2);

    // Move up
    state.hover_up();
    assert_eq!(state.hover_index(), 1);
    state.hover_up();
    assert_eq!(state.hover_index(), 0);

    // Can't go below 0
    state.hover_up();
    assert_eq!(state.hover_index(), 0);
  }

  #[test]
  fn hover_clamped_to_max() {
    let mut state = DropdownState::new();
    state.open();

    state.hover_down(2); // max is 2
    state.hover_down(2);
    state.hover_down(2); // Should be clamped
    assert_eq!(state.hover_index(), 2);
  }

  #[test]
  fn select_hovered_updates_selection_and_closes() {
    let mut state = DropdownState::new();
    state.open();
    state.hover_down(4);
    state.hover_down(4);
    state.select_hovered();

    assert_eq!(state.selected(), 2);
    assert!(!state.is_open());
  }

  #[test]
  fn set_selected_updates_both_indices() {
    let mut state = DropdownState::new();
    state.set_selected(5);
    assert_eq!(state.selected(), 5);
    assert_eq!(state.hover_index(), 5);
  }

  #[test]
  fn with_id_creates_state_with_id() {
    let state = DropdownState::with_id(42);
    assert_eq!(state.id(), 42);
    assert_eq!(state.selected(), 0);
    assert!(!state.is_focused());
  }

  #[test]
  fn with_id_and_selected_creates_state() {
    let state = DropdownState::with_id_and_selected(7, 3);
    assert_eq!(state.id(), 7);
    assert_eq!(state.selected(), 3);
    assert_eq!(state.hover_index(), 3);
  }

  #[test]
  fn focus_management() {
    let mut state = DropdownState::with_id(1);
    assert!(!state.is_focused());

    state.set_focused(true);
    assert!(state.is_focused());

    state.set_focused(false);
    assert!(!state.is_focused());
  }

  #[test]
  fn just_closed_flag_set_on_close() {
    let mut state = DropdownState::new();
    assert!(!state.was_just_closed());

    state.open();
    assert!(!state.was_just_closed());

    state.close();
    assert!(state.was_just_closed());
  }

  #[test]
  fn just_closed_cleared_on_open() {
    let mut state = DropdownState::new();
    state.open();
    state.close();
    assert!(state.was_just_closed());

    state.open();
    assert!(!state.was_just_closed());
  }

  #[test]
  fn just_closed_set_on_select_hovered() {
    let mut state = DropdownState::new();
    state.open();
    state.hover_down(4);
    state.select_hovered();

    assert!(state.was_just_closed());
    assert!(!state.is_open());
  }

  #[test]
  fn clear_just_closed_resets_flag() {
    let mut state = DropdownState::new();
    state.open();
    state.close();
    assert!(state.was_just_closed());

    state.clear_just_closed();
    assert!(!state.was_just_closed());
  }

  #[test]
  fn toggle_sets_just_closed_when_closing() {
    let mut state = DropdownState::new();
    state.toggle(); // open
    assert!(!state.was_just_closed());

    state.toggle(); // close
    assert!(state.was_just_closed());
  }
}
