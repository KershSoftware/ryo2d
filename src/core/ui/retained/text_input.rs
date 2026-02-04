//! Text input widget with cursor, selection, and editing support.
//!
//! This is a retained-mode widget - you create a `TextInputState` and persist it
//! across frames, then call `text_input()` each frame to render and update it.
//!
//! # Example
//!
//! ```ignore
//! struct MyScene {
//!     username_input: TextInputState,
//! }
//!
//! impl MyScene {
//!     fn new() -> Self {
//!         Self {
//!             username_input: TextInputState::new(),
//!         }
//!     }
//!
//!     fn render_ui(&mut self, ui: &mut UiContext, dt: f32) {
//!         let rect = Rect::new(100.0, 100.0, 200.0, 30.0);
//!         if ui.text_input(&mut self.username_input, rect, "Enter name...", dt) {
//!             println!("Text changed: {}", self.username_input.text());
//!         }
//!     }
//! }
//! ```

use arboard::Clipboard;
use winit::keyboard::KeyCode;

use super::focus::Focusable;
use crate::core::input::InputState;

/// State for a text input widget.
///
/// Persists the text buffer, cursor position, selection, and blink state
/// across frames. Create one per text field and pass it to `UiContext::text_input()`.
#[derive(Debug, Clone)]
pub struct TextInputState {
  /// The text content
  text: String,
  /// Cursor position (byte index into text)
  cursor: usize,
  /// Selection anchor (byte index). If different from cursor, text is selected.
  selection_anchor: usize,
  /// Whether this input is focused
  focused: bool,
  /// Cursor blink timer (seconds)
  blink_timer: f32,
  /// Whether cursor is currently visible (toggles during blink)
  cursor_visible: bool,
}

/// Cursor blink rate in seconds (on/off cycle)
const CURSOR_BLINK_RATE: f32 = 0.53;

impl Default for TextInputState {
  fn default() -> Self {
    Self::new()
  }
}

impl TextInputState {
  /// Create a new empty text input state.
  pub fn new() -> Self {
    Self {
      text: String::new(),
      cursor: 0,
      selection_anchor: 0,
      focused: false,
      blink_timer: 0.0,
      cursor_visible: true,
    }
  }

  /// Create a text input state with initial text.
  pub fn with_text(text: impl Into<String>) -> Self {
    let text = text.into();
    let len = text.len();
    Self {
      text,
      cursor: len,
      selection_anchor: len,
      focused: false,
      blink_timer: 0.0,
      cursor_visible: true,
    }
  }

  /// Get the current text content.
  pub fn text(&self) -> &str {
    &self.text
  }

  /// Set the text content, moving cursor to end.
  pub fn set_text(&mut self, text: impl Into<String>) {
    self.text = text.into();
    self.cursor = self.text.len();
    self.selection_anchor = self.cursor;
  }

  /// Get the cursor position (byte index).
  pub fn cursor(&self) -> usize {
    self.cursor
  }

  /// Check if this input is focused.
  pub fn is_focused(&self) -> bool {
    self.focused
  }

  /// Set focus state.
  pub fn set_focused(&mut self, focused: bool) {
    self.focused = focused;
    if focused {
      // Reset blink on focus
      self.blink_timer = 0.0;
      self.cursor_visible = true;
    }
  }

  /// Check if there's a selection (cursor != anchor).
  pub fn has_selection(&self) -> bool {
    self.cursor != self.selection_anchor
  }

  /// Get the selection range as (start, end) byte indices.
  /// Returns `None` if no selection.
  pub fn selection_range(&self) -> Option<(usize, usize)> {
    if self.cursor == self.selection_anchor {
      None
    } else {
      Some((
        self.cursor.min(self.selection_anchor),
        self.cursor.max(self.selection_anchor),
      ))
    }
  }

  /// Get the selected text, if any.
  pub fn selected_text(&self) -> Option<&str> {
    self
      .selection_range()
      .map(|(start, end)| &self.text[start..end])
  }

  /// Select all text.
  pub fn select_all(&mut self) {
    self.selection_anchor = 0;
    self.cursor = self.text.len();
  }

  /// Clear the selection (keep cursor position).
  pub fn clear_selection(&mut self) {
    self.selection_anchor = self.cursor;
  }

  // =========================================================================
  // Clipboard Operations
  // =========================================================================

  /// Copy the selected text to the system clipboard.
  ///
  /// Returns `true` if text was copied successfully.
  pub fn copy(&self) -> bool {
    if let Some(selected) = self.selected_text()
      && let Ok(mut clipboard) = Clipboard::new()
    {
      return clipboard.set_text(selected).is_ok();
    }
    false
  }

  /// Cut the selected text to the system clipboard.
  ///
  /// Returns `true` if text was cut successfully.
  pub fn cut(&mut self) -> bool {
    if self.copy() {
      self.delete_selection();
      true
    } else {
      false
    }
  }

  /// Paste text from the system clipboard at the cursor position.
  ///
  /// Returns `true` if text was pasted successfully.
  pub fn paste(&mut self) -> bool {
    if let Ok(mut clipboard) = Clipboard::new()
      && let Ok(text) = clipboard.get_text()
    {
      // Filter out control characters except newlines (which we'll convert to space)
      let clean_text: String = text
        .chars()
        .filter_map(|c| {
          if c == '\n' || c == '\r' {
            Some(' ') // Convert newlines to spaces for single-line input
          } else if c.is_control() {
            None // Remove other control characters
          } else {
            Some(c)
          }
        })
        .collect();

      if !clean_text.is_empty() {
        self.insert(&clean_text);
        return true;
      }
    }
    false
  }

  /// Update the blink timer. Call once per frame with delta time.
  pub fn update_blink(&mut self, dt: f32) {
    if !self.focused {
      return;
    }
    self.blink_timer += dt;
    if self.blink_timer >= CURSOR_BLINK_RATE {
      self.blink_timer -= CURSOR_BLINK_RATE;
      self.cursor_visible = !self.cursor_visible;
    }
  }

  /// Reset the blink timer (makes cursor visible).
  pub fn reset_blink(&mut self) {
    self.blink_timer = 0.0;
    self.cursor_visible = true;
  }

  /// Check if the cursor should be drawn (for blinking effect).
  pub fn should_draw_cursor(&self) -> bool {
    self.focused && self.cursor_visible
  }

  /// Insert text at cursor, replacing any selection.
  pub fn insert(&mut self, text: &str) {
    // Delete selection first if any
    if self.has_selection() {
      self.delete_selection();
    }

    // Insert at cursor
    self.text.insert_str(self.cursor, text);
    self.cursor += text.len();
    self.selection_anchor = self.cursor;
    self.reset_blink();
  }

  /// Delete the selected text.
  pub fn delete_selection(&mut self) {
    if let Some((start, end)) = self.selection_range() {
      self.text.drain(start..end);
      self.cursor = start;
      self.selection_anchor = start;
      self.reset_blink();
    }
  }

  /// Delete the character before the cursor (backspace).
  pub fn backspace(&mut self) {
    if self.has_selection() {
      self.delete_selection();
    } else if self.cursor > 0 {
      // Find the previous character boundary
      let prev_char_start = self.prev_char_boundary(self.cursor);
      self.text.drain(prev_char_start..self.cursor);
      self.cursor = prev_char_start;
      self.selection_anchor = self.cursor;
      self.reset_blink();
    }
  }

  /// Delete the character after the cursor (delete key).
  pub fn delete(&mut self) {
    if self.has_selection() {
      self.delete_selection();
    } else if self.cursor < self.text.len() {
      // Find the next character boundary
      let next_char_end = self.next_char_boundary(self.cursor);
      self.text.drain(self.cursor..next_char_end);
      self.reset_blink();
    }
  }

  /// Move cursor left by one character.
  pub fn move_left(&mut self, select: bool) {
    if !select && self.has_selection() {
      // Collapse selection to left edge
      self.cursor = self.cursor.min(self.selection_anchor);
      self.selection_anchor = self.cursor;
    } else if self.cursor > 0 {
      self.cursor = self.prev_char_boundary(self.cursor);
      if !select {
        self.selection_anchor = self.cursor;
      }
    }
    self.reset_blink();
  }

  /// Move cursor right by one character.
  pub fn move_right(&mut self, select: bool) {
    if !select && self.has_selection() {
      // Collapse selection to right edge
      self.cursor = self.cursor.max(self.selection_anchor);
      self.selection_anchor = self.cursor;
    } else if self.cursor < self.text.len() {
      self.cursor = self.next_char_boundary(self.cursor);
      if !select {
        self.selection_anchor = self.cursor;
      }
    }
    self.reset_blink();
  }

  /// Move cursor to start of text.
  pub fn move_home(&mut self, select: bool) {
    self.cursor = 0;
    if !select {
      self.selection_anchor = self.cursor;
    }
    self.reset_blink();
  }

  /// Move cursor to end of text.
  pub fn move_end(&mut self, select: bool) {
    self.cursor = self.text.len();
    if !select {
      self.selection_anchor = self.cursor;
    }
    self.reset_blink();
  }

  /// Set cursor position from a character index.
  ///
  /// If `select` is true, extends selection from the current anchor.
  /// If `select` is false, moves both cursor and anchor (no selection).
  pub fn set_cursor_char(&mut self, char_index: usize, select: bool) {
    // Convert character index to byte position
    let mut byte_pos = 0;
    for (i, c) in self.text.chars().enumerate() {
      if i >= char_index {
        break;
      }
      byte_pos += c.len_utf8();
    }
    self.cursor = byte_pos.min(self.text.len());
    if !select {
      self.selection_anchor = self.cursor;
    }
    self.reset_blink();
  }

  /// Handle keyboard input for text editing.
  ///
  /// Returns `true` if the text was modified.
  pub fn handle_input(&mut self, input: &InputState) -> bool {
    if !self.focused {
      return false;
    }

    let mut modified = false;
    let shift = input.key_down(KeyCode::ShiftLeft) || input.key_down(KeyCode::ShiftRight);
    // Check both Ctrl (Windows/Linux) and Super/Cmd (macOS) for shortcuts
    let ctrl = input.key_down(KeyCode::ControlLeft)
      || input.key_down(KeyCode::ControlRight)
      || input.key_down(KeyCode::SuperLeft)
      || input.key_down(KeyCode::SuperRight);

    // Handle committed text from IME (but not when Ctrl/Cmd is held - those are shortcuts)
    let text = input.text_input();
    if !text.is_empty() && !ctrl {
      self.insert(text);
      modified = true;
    }

    // Arrow keys (use key_repeat for held-key behavior)
    if input.key_repeat(KeyCode::ArrowLeft) {
      self.move_left(shift);
    }
    if input.key_repeat(KeyCode::ArrowRight) {
      self.move_right(shift);
    }

    // Home/End
    if input.key_pressed(KeyCode::Home) {
      self.move_home(shift);
    }
    if input.key_pressed(KeyCode::End) {
      self.move_end(shift);
    }

    // Backspace (use key_repeat for held-key behavior)
    if input.key_repeat(KeyCode::Backspace) {
      self.backspace();
      modified = true;
    }

    // Delete (use key_repeat for held-key behavior)
    if input.key_repeat(KeyCode::Delete) {
      self.delete();
      modified = true;
    }

    // Select all (Ctrl+A)
    if ctrl && input.key_pressed(KeyCode::KeyA) {
      self.select_all();
    }

    // Copy (Ctrl+C)
    if ctrl && input.key_pressed(KeyCode::KeyC) {
      self.copy();
    }

    // Cut (Ctrl+X)
    if ctrl && input.key_pressed(KeyCode::KeyX) && self.cut() {
      modified = true;
    }

    // Paste (Ctrl+V)
    if ctrl && input.key_pressed(KeyCode::KeyV) && self.paste() {
      modified = true;
    }

    modified
  }

  // =========================================================================
  // Private helpers
  // =========================================================================

  /// Find the previous character boundary before `pos`.
  fn prev_char_boundary(&self, pos: usize) -> usize {
    let mut idx = pos.saturating_sub(1);
    while idx > 0 && !self.text.is_char_boundary(idx) {
      idx -= 1;
    }
    idx
  }

  /// Find the next character boundary after `pos`.
  fn next_char_boundary(&self, pos: usize) -> usize {
    let mut idx = pos + 1;
    while idx < self.text.len() && !self.text.is_char_boundary(idx) {
      idx += 1;
    }
    idx.min(self.text.len())
  }
}

// ============================================================================
// Focusable implementation
// ============================================================================

impl Focusable for TextInputState {
  fn is_focused(&self) -> bool {
    self.focused
  }

  fn set_focused(&mut self, focused: bool) {
    TextInputState::set_focused(self, focused);
  }

  fn on_focus(&mut self) {
    self.blink_timer = 0.0;
    self.cursor_visible = true;
  }

  fn on_blur(&mut self) {
    // Optionally clear selection on blur
    // self.clear_selection();
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use serial_test::serial;

  #[test]
  fn new_creates_empty_state() {
    let state = TextInputState::new();
    assert_eq!(state.text(), "");
    assert_eq!(state.cursor(), 0);
    assert!(!state.is_focused());
    assert!(!state.has_selection());
  }

  #[test]
  fn with_text_sets_initial_text() {
    let state = TextInputState::with_text("hello");
    assert_eq!(state.text(), "hello");
    assert_eq!(state.cursor(), 5); // At end
  }

  #[test]
  fn insert_adds_text_at_cursor() {
    let mut state = TextInputState::new();
    state.insert("hello");
    assert_eq!(state.text(), "hello");
    assert_eq!(state.cursor(), 5);

    state.cursor = 0;
    state.selection_anchor = 0;
    state.insert("say ");
    assert_eq!(state.text(), "say hello");
    assert_eq!(state.cursor(), 4);
  }

  #[test]
  fn insert_replaces_selection() {
    let mut state = TextInputState::with_text("hello world");
    state.cursor = 6;
    state.selection_anchor = 11; // Select "world"
    state.insert("rust");
    assert_eq!(state.text(), "hello rust");
  }

  #[test]
  fn backspace_removes_char_before_cursor() {
    let mut state = TextInputState::with_text("hello");
    state.backspace();
    assert_eq!(state.text(), "hell");

    state.cursor = 0;
    state.selection_anchor = 0;
    state.backspace(); // No-op at start
    assert_eq!(state.text(), "hell");
  }

  #[test]
  fn backspace_removes_selection() {
    let mut state = TextInputState::with_text("hello");
    state.cursor = 0;
    state.selection_anchor = 5; // Select all
    state.backspace();
    assert_eq!(state.text(), "");
  }

  #[test]
  fn delete_removes_char_after_cursor() {
    let mut state = TextInputState::with_text("hello");
    state.cursor = 0;
    state.selection_anchor = 0;
    state.delete();
    assert_eq!(state.text(), "ello");

    // Move cursor to end - also need to update anchor to avoid selection
    state.cursor = 4;
    state.selection_anchor = 4;
    state.delete(); // No-op at end
    assert_eq!(state.text(), "ello");
  }

  #[test]
  fn move_left_moves_cursor() {
    let mut state = TextInputState::with_text("hello");
    state.move_left(false);
    assert_eq!(state.cursor(), 4);
    assert!(!state.has_selection());
  }

  #[test]
  fn move_left_with_shift_selects() {
    let mut state = TextInputState::with_text("hello");
    state.move_left(true);
    assert_eq!(state.cursor(), 4);
    assert_eq!(state.selection_anchor, 5);
    assert!(state.has_selection());
  }

  #[test]
  fn move_right_moves_cursor() {
    let mut state = TextInputState::with_text("hello");
    state.cursor = 0;
    state.selection_anchor = 0;
    state.move_right(false);
    assert_eq!(state.cursor(), 1);
  }

  #[test]
  fn move_home_end() {
    let mut state = TextInputState::with_text("hello world");
    state.cursor = 5;
    state.selection_anchor = 5;

    state.move_home(false);
    assert_eq!(state.cursor(), 0);

    state.move_end(false);
    assert_eq!(state.cursor(), 11);
  }

  #[test]
  fn select_all() {
    let mut state = TextInputState::with_text("hello");
    state.cursor = 2;
    state.selection_anchor = 2;
    state.select_all();
    assert_eq!(state.selection_anchor, 0);
    assert_eq!(state.cursor(), 5);
    assert_eq!(state.selected_text(), Some("hello"));
  }

  #[test]
  fn selection_range_returns_ordered() {
    let mut state = TextInputState::with_text("hello");
    // Selection from right to left
    state.cursor = 1;
    state.selection_anchor = 4;
    assert_eq!(state.selection_range(), Some((1, 4)));

    // Selection from left to right
    state.cursor = 4;
    state.selection_anchor = 1;
    assert_eq!(state.selection_range(), Some((1, 4)));
  }

  #[test]
  fn blink_updates() {
    let mut state = TextInputState::new();
    state.set_focused(true);
    assert!(state.cursor_visible);

    state.update_blink(CURSOR_BLINK_RATE);
    assert!(!state.cursor_visible);

    state.update_blink(CURSOR_BLINK_RATE);
    assert!(state.cursor_visible);
  }

  #[test]
  fn unicode_navigation() {
    // Test with multi-byte characters
    let mut state = TextInputState::with_text("héllo");
    assert_eq!(state.cursor(), 6); // 'é' is 2 bytes

    state.move_left(false);
    assert_eq!(state.cursor(), 5); // Before 'o'

    state.cursor = 0;
    state.selection_anchor = 0;
    state.move_right(false);
    assert_eq!(state.cursor(), 1); // After 'h'

    state.move_right(false);
    assert_eq!(state.cursor(), 3); // After 'é' (2 bytes)
  }

  #[test]
  fn set_cursor_char_positions_correctly() {
    let mut state = TextInputState::with_text("hello world");

    // Click at character 5 (the space)
    state.set_cursor_char(5, false);
    assert_eq!(state.cursor(), 5);
    assert_eq!(state.selection_anchor, 5);
    assert!(!state.has_selection());

    // Click at start
    state.set_cursor_char(0, false);
    assert_eq!(state.cursor(), 0);

    // Click past end - should clamp
    state.set_cursor_char(100, false);
    assert_eq!(state.cursor(), 11); // "hello world" is 11 chars
  }

  #[test]
  fn set_cursor_char_with_selection() {
    let mut state = TextInputState::with_text("hello");
    state.set_cursor_char(2, false); // Position at 'l'

    // Shift-click at position 4 to select
    state.set_cursor_char(4, true);
    assert_eq!(state.cursor(), 4);
    assert_eq!(state.selection_anchor, 2);
    assert!(state.has_selection());
    assert_eq!(state.selected_text(), Some("ll"));
  }

  #[test]
  fn set_cursor_char_with_unicode() {
    // "héllo" - 'é' is 2 bytes, so 6 bytes total for 5 characters
    let mut state = TextInputState::with_text("héllo");

    // Click at character 2 (first 'l')
    state.set_cursor_char(2, false);
    assert_eq!(state.cursor(), 3); // After 'é' which is 2 bytes

    // Click at character 0
    state.set_cursor_char(0, false);
    assert_eq!(state.cursor(), 0);

    // Click at character 5 (end)
    state.set_cursor_char(5, false);
    assert_eq!(state.cursor(), 6); // Full byte length
  }

  // =========================================================================
  // Clipboard tests
  // =========================================================================
  // Note: These tests interact with the system clipboard and must run serially.
  // They are marked #[ignore] by default to avoid CI issues and use #[serial]
  // to prevent heap corruption from concurrent clipboard access on Windows.
  // Run with: cargo test clipboard -- --ignored

  #[test]
  #[ignore] // Requires system clipboard access
  #[serial]
  fn copy_with_selection_copies_to_clipboard() {
    let mut state = TextInputState::with_text("hello world");
    state.cursor = 0;
    state.selection_anchor = 5;

    let copied = state.copy();
    assert!(copied);

    // Verify we can paste it back
    let mut clipboard = Clipboard::new().unwrap();
    assert_eq!(clipboard.get_text().unwrap(), "hello");
  }

  #[test]
  #[ignore] // Requires system clipboard access
  #[serial]
  fn copy_without_selection_returns_false() {
    let state = TextInputState::with_text("hello");
    // No selection (cursor == anchor)
    assert!(!state.copy());
  }

  #[test]
  #[ignore] // Requires system clipboard access
  #[serial]
  fn cut_removes_selection_and_copies() {
    let mut state = TextInputState::with_text("hello world");
    state.cursor = 6;
    state.selection_anchor = 11; // Select "world"

    let cut_result = state.cut();
    assert!(cut_result);
    assert_eq!(state.text(), "hello ");
    assert_eq!(state.cursor(), 6);

    // Verify clipboard has the cut text
    let mut clipboard = Clipboard::new().unwrap();
    assert_eq!(clipboard.get_text().unwrap(), "world");
  }

  #[test]
  #[ignore] // Requires system clipboard access
  #[serial]
  fn paste_inserts_text_at_cursor() {
    let mut clipboard = Clipboard::new().unwrap();
    clipboard.set_text("inserted").unwrap();

    let mut state = TextInputState::with_text("hello world");
    state.cursor = 6;
    state.selection_anchor = 6;

    let pasted = state.paste();
    assert!(pasted);
    assert_eq!(state.text(), "hello insertedworld");
  }

  #[test]
  #[ignore] // Requires system clipboard access
  #[serial]
  fn paste_replaces_selection() {
    let mut clipboard = Clipboard::new().unwrap();
    clipboard.set_text("there").unwrap();

    let mut state = TextInputState::with_text("hello world");
    state.cursor = 6;
    state.selection_anchor = 11; // Select "world"

    let pasted = state.paste();
    assert!(pasted);
    assert_eq!(state.text(), "hello there");
  }

  #[test]
  #[ignore] // Requires system clipboard access
  #[serial]
  fn paste_converts_newlines_to_spaces() {
    let mut clipboard = Clipboard::new().unwrap();
    clipboard.set_text("line1\nline2\r\nline3").unwrap();

    let mut state = TextInputState::new();
    let pasted = state.paste();
    assert!(pasted);
    assert_eq!(state.text(), "line1 line2  line3"); // \r\n becomes two spaces
  }
}
