//! A text field's editing state: the caret, the selection, the horizontal
//! scroll and the blink, over a string the caller owns

use std::ops::Range;

use winit::keyboard::KeyCode;

use crate::core::ui::immediate::UiInput;

/// The caret shows this long, then hides as long (1 Hz)
pub(crate) const CARET_BLINK: f32 = 0.5;

/// Room kept between the caret and the field's right edge when it scrolls
const CARET_MARGIN: f32 = 2.0;

/// What one frame of keys did to the text
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct EditOutcome {
  /// The text changed
  pub(crate) changed: bool,
  /// Enter was pressed
  pub(crate) submitted: bool,
}

/// A text field's editing state over a string it does not own: the caret
/// and the selection anchor (byte indices into the string), the horizontal
/// scroll in UI units, the caret's blink and the paste guard. Kept in the
/// UI's memory under the field's id.
#[derive(Debug, Clone, Default)]
pub(crate) struct TextEdit {
  /// The caret (byte index)
  cursor: usize,
  /// The selection's other end (byte index); the caret when nothing is
  /// selected
  anchor: usize,
  /// How far the text is scrolled left, in UI units
  scroll: f32,
  /// Seconds since the caret last moved, for its blink
  blink: f32,
  /// Set when a paste fires and cleared when V is released, so a paste
  /// happens once per press. It also blocks typed text while set: on
  /// Windows an IME may commit the clipboard's text alongside Ctrl+V.
  paste_guard: bool,
}

impl TextEdit {
  /// The caret (byte index)
  pub(crate) fn cursor(&self) -> usize {
    self.cursor
  }

  /// The selected bytes, in order (`None` when nothing is selected)
  pub(crate) fn selection(&self) -> Option<Range<usize>> {
    (self.cursor != self.anchor).then(|| self.cursor.min(self.anchor)..self.cursor.max(self.anchor))
  }

  /// The selected part of `text`
  pub(crate) fn selected_text<'t>(&self, text: &'t str) -> Option<&'t str> {
    self.selection().and_then(|range| text.get(range))
  }

  /// How far the text is scrolled left, in UI units
  pub(crate) fn scroll(&self) -> f32 {
    self.scroll
  }

  /// Keep the caret and the anchor on `text` after the caller changed it:
  /// at most its length, on a character boundary
  pub(crate) fn clamp_to(&mut self, text: &str) {
    self.cursor = boundary_at_or_before(text, self.cursor);
    self.anchor = boundary_at_or_before(text, self.anchor);
  }

  /// Put the caret at `byte` of `text` (snapped back to a character
  /// boundary, and to the end). With `select` the selection runs from the
  /// anchor to it; otherwise nothing is selected.
  pub(crate) fn set_cursor(&mut self, text: &str, byte: usize, select: bool) {
    self.cursor = boundary_at_or_before(text, byte);
    if !select {
      self.anchor = self.cursor;
    }
    self.reset_blink();
  }

  /// Put the caret before character `index` of `text` (the end past it)
  pub(crate) fn set_cursor_char(&mut self, text: &str, index: usize, select: bool) {
    let byte = text
      .char_indices()
      .nth(index)
      .map_or(text.len(), |(b, _)| b);
    self.set_cursor(text, byte, select);
  }

  /// Keep the caret (`caret_x`, UI units from the text's start, as the
  /// text engine measures it) inside a field `width` UI units wide,
  /// scrolling as little as possible
  pub(crate) fn ensure_caret_visible(&mut self, caret_x: f32, width: f32) {
    if !(caret_x.is_finite() && width.is_finite()) {
      return;
    }
    if caret_x - self.scroll > width - CARET_MARGIN {
      self.scroll = caret_x - width + CARET_MARGIN;
    } else if caret_x < self.scroll {
      self.scroll = (caret_x - CARET_MARGIN).max(0.0);
    }
    self.scroll = self.scroll.max(0.0);
  }

  /// Count `dt` seconds of the caret's blink
  pub(crate) fn tick(&mut self, dt: f32) {
    if dt.is_finite() && dt > 0.0 {
      self.blink = (self.blink + dt) % (2.0 * CARET_BLINK);
    }
  }

  /// Whether the caret is in the shown half of its blink
  pub(crate) fn caret_shown(&self) -> bool {
    self.blink < CARET_BLINK
  }

  /// Show the caret from now (it moved or the text changed)
  fn reset_blink(&mut self) {
    self.blink = 0.0;
  }

  /// Select all of `text`
  pub(crate) fn select_all(&mut self, text: &str) {
    self.anchor = 0;
    self.cursor = text.len();
  }

  /// Put `insert` in `text` at the caret, replacing the selection. With a
  /// `max_len` (in characters) the insertion is cut to the room left.
  /// Whether `text` changed.
  pub(crate) fn insert(&mut self, text: &mut String, insert: &str, max_len: Option<usize>) -> bool {
    self.clamp_to(text);
    let insert = match max_len {
      Some(max) => {
        let selected = self.selected_text(text).map_or(0, |s| s.chars().count());
        let room = max.saturating_sub(text.chars().count() - selected);
        let end = insert
          .char_indices()
          .nth(room)
          .map_or(insert.len(), |(b, _)| b);
        &insert[..end]
      }
      None => insert,
    };
    if insert.is_empty() {
      return false;
    }
    self.delete_selection(text);
    text.insert_str(self.cursor, insert);
    self.cursor += insert.len();
    self.anchor = self.cursor;
    self.reset_blink();
    true
  }

  /// Delete the selected text. Whether there was any.
  pub(crate) fn delete_selection(&mut self, text: &mut String) -> bool {
    let Some(range) = self.selection() else {
      return false;
    };
    if text.get(range.clone()).is_none() {
      return false;
    }
    text.drain(range.clone());
    self.cursor = range.start;
    self.anchor = range.start;
    self.reset_blink();
    true
  }

  /// Delete the selection, or the character before the caret. Whether
  /// `text` changed.
  pub(crate) fn backspace(&mut self, text: &mut String) -> bool {
    self.clamp_to(text);
    if self.selection().is_some() {
      return self.delete_selection(text);
    }
    if self.cursor == 0 {
      return false;
    }
    let start = prev_boundary(text, self.cursor);
    text.drain(start..self.cursor);
    self.cursor = start;
    self.anchor = start;
    self.reset_blink();
    true
  }

  /// Delete the selection, or the character after the caret. Whether
  /// `text` changed.
  pub(crate) fn delete(&mut self, text: &mut String) -> bool {
    self.clamp_to(text);
    if self.selection().is_some() {
      return self.delete_selection(text);
    }
    if self.cursor >= text.len() {
      return false;
    }
    let end = next_boundary(text, self.cursor);
    text.drain(self.cursor..end);
    self.reset_blink();
    true
  }

  /// Move the caret one character left; without `select` a selection
  /// collapses to its left end instead
  pub(crate) fn move_left(&mut self, text: &str, select: bool) {
    if !select && self.selection().is_some() {
      self.cursor = self.cursor.min(self.anchor);
      self.anchor = self.cursor;
    } else if self.cursor > 0 {
      self.cursor = prev_boundary(text, self.cursor);
      if !select {
        self.anchor = self.cursor;
      }
    }
    self.reset_blink();
  }

  /// Move the caret one character right; without `select` a selection
  /// collapses to its right end instead
  pub(crate) fn move_right(&mut self, text: &str, select: bool) {
    if !select && self.selection().is_some() {
      self.cursor = self.cursor.max(self.anchor);
      self.anchor = self.cursor;
    } else if self.cursor < text.len() {
      self.cursor = next_boundary(text, self.cursor);
      if !select {
        self.anchor = self.cursor;
      }
    }
    self.reset_blink();
  }

  /// Move the caret to the start
  pub(crate) fn move_home(&mut self, select: bool) {
    self.cursor = 0;
    if !select {
      self.anchor = self.cursor;
    }
    self.reset_blink();
  }

  /// Move the caret to the end
  pub(crate) fn move_end(&mut self, text: &str, select: bool) {
    self.cursor = text.len();
    if !select {
      self.anchor = self.cursor;
    }
    self.reset_blink();
  }

  /// Put the selected text on the system clipboard. Whether it was copied.
  pub(crate) fn copy(&self, text: &str) -> bool {
    self
      .selected_text(text)
      .is_some_and(|selected| crate::core::clipboard::set_text(selected).is_ok())
  }

  /// Copy the selected text to the system clipboard and delete it. Whether
  /// it was cut.
  pub(crate) fn cut(&mut self, text: &mut String) -> bool {
    self.copy(text) && self.delete_selection(text)
  }

  /// Insert the clipboard's text at the caret, its line breaks as spaces
  /// and other control characters dropped (cut to `max_len`). Whether
  /// `text` changed.
  pub(crate) fn paste(&mut self, text: &mut String, max_len: Option<usize>) -> bool {
    let Ok(pasted) = crate::core::clipboard::get_text() else {
      return false;
    };
    let clean: String = pasted
      .chars()
      .filter_map(|c| match c {
        '\n' | '\r' => Some(' '),
        c if c.is_control() => None,
        c => Some(c),
      })
      .collect();
    self.insert(text, &clean, max_len)
  }

  /// Apply the frame's keys and typed text to `text`: typing (the IME's
  /// committed text included), Left and Right (Shift selects), Home, End,
  /// Backspace, Delete, and with Ctrl (Cmd on macOS) A, C, X and V. Enter
  /// submits. Typing past `max_len` characters is cut off.
  pub(crate) fn handle_input(
    &mut self,
    text: &mut String,
    input: &UiInput,
    max_len: Option<usize>,
  ) -> EditOutcome {
    self.clamp_to(text);
    let mut out = EditOutcome::default();
    let shift = input.key_down(KeyCode::ShiftLeft) || input.key_down(KeyCode::ShiftRight);
    // Ctrl on Windows and Linux, Cmd on macOS
    let ctrl = input.key_down(KeyCode::ControlLeft)
      || input.key_down(KeyCode::ControlRight)
      || input.key_down(KeyCode::SuperLeft)
      || input.key_down(KeyCode::SuperRight);

    // One paste per press of V
    if !input.key_down(KeyCode::KeyV) {
      self.paste_guard = false;
    }

    // Typed and committed text, unless it is a shortcut (Ctrl held) or the
    // IME's copy of a paste
    let typed = input.text_input();
    if !typed.is_empty() && !ctrl && !self.paste_guard {
      out.changed |= self.insert(text, typed, max_len);
    }

    // Held arrows, Backspace and Delete repeat
    if input.key_repeat(KeyCode::ArrowLeft) {
      self.move_left(text, shift);
    }
    if input.key_repeat(KeyCode::ArrowRight) {
      self.move_right(text, shift);
    }
    if input.key_pressed(KeyCode::Home) {
      self.move_home(shift);
    }
    if input.key_pressed(KeyCode::End) {
      self.move_end(text, shift);
    }
    if input.key_repeat(KeyCode::Backspace) {
      out.changed |= self.backspace(text);
    }
    if input.key_repeat(KeyCode::Delete) {
      out.changed |= self.delete(text);
    }

    if ctrl && input.key_pressed(KeyCode::KeyA) {
      self.select_all(text);
    }
    if ctrl && input.key_pressed(KeyCode::KeyC) {
      self.copy(text);
    }
    if ctrl && input.key_pressed(KeyCode::KeyX) {
      out.changed |= self.cut(text);
    }
    if ctrl && input.key_pressed(KeyCode::KeyV) && !self.paste_guard {
      self.paste_guard = true;
      out.changed |= self.paste(text, max_len);
    }

    out.submitted = input.key_pressed(KeyCode::Enter) || input.key_pressed(KeyCode::NumpadEnter);
    out
  }
}

/// `byte` moved back to a character boundary of `text`, at most its length
fn boundary_at_or_before(text: &str, byte: usize) -> usize {
  let mut byte = byte.min(text.len());
  while !text.is_char_boundary(byte) {
    byte -= 1;
  }
  byte
}

/// The character boundary before `pos`
fn prev_boundary(text: &str, pos: usize) -> usize {
  text
    .get(..boundary_at_or_before(text, pos))
    .and_then(|before| before.char_indices().next_back())
    .map_or(0, |(b, _)| b)
}

/// The character boundary after `pos`
fn next_boundary(text: &str, pos: usize) -> usize {
  text
    .get(pos..)
    .and_then(|rest| rest.chars().next())
    .map_or(text.len(), |c| pos + c.len_utf8())
}

#[cfg(test)]
mod tests {
  use super::*;
  use arboard::Clipboard;
  use serial_test::serial;

  /// `text` with the caret at its end (as a field shows a string it was
  /// given)
  fn with_text(text: &str) -> (String, TextEdit) {
    let text = text.to_string();
    let mut edit = TextEdit::default();
    edit.set_cursor(&text, text.len(), false);
    (text, edit)
  }

  /// The caret and the anchor at `byte`
  fn place(edit: &mut TextEdit, byte: usize) {
    edit.cursor = byte;
    edit.anchor = byte;
  }

  #[test]
  fn new_creates_empty_state() {
    let (text, edit) = (String::new(), TextEdit::default());
    assert_eq!(text, "");
    assert_eq!(edit.cursor(), 0);
    assert!(edit.selection().is_none());
  }

  #[test]
  fn with_text_sets_initial_text() {
    let (text, edit) = with_text("hello");
    assert_eq!(text, "hello");
    assert_eq!(edit.cursor(), 5); // At end
  }

  #[test]
  fn insert_adds_text_at_cursor() {
    let (mut text, mut edit) = (String::new(), TextEdit::default());
    edit.insert(&mut text, "hello", None);
    assert_eq!(text, "hello");
    assert_eq!(edit.cursor(), 5);

    place(&mut edit, 0);
    edit.insert(&mut text, "say ", None);
    assert_eq!(text, "say hello");
    assert_eq!(edit.cursor(), 4);
  }

  #[test]
  fn insert_replaces_selection() {
    let (mut text, mut edit) = with_text("hello world");
    edit.cursor = 6;
    edit.anchor = 11; // Select "world"
    edit.insert(&mut text, "rust", None);
    assert_eq!(text, "hello rust");
  }

  #[test]
  fn backspace_removes_char_before_cursor() {
    let (mut text, mut edit) = with_text("hello");
    edit.backspace(&mut text);
    assert_eq!(text, "hell");

    place(&mut edit, 0);
    edit.backspace(&mut text); // No-op at start
    assert_eq!(text, "hell");
  }

  #[test]
  fn backspace_removes_selection() {
    let (mut text, mut edit) = with_text("hello");
    edit.cursor = 0;
    edit.anchor = 5; // Select all
    edit.backspace(&mut text);
    assert_eq!(text, "");
  }

  #[test]
  fn delete_removes_char_after_cursor() {
    let (mut text, mut edit) = with_text("hello");
    place(&mut edit, 0);
    edit.delete(&mut text);
    assert_eq!(text, "ello");

    // Move the caret to the end, the anchor too, so nothing is selected
    place(&mut edit, 4);
    edit.delete(&mut text); // No-op at end
    assert_eq!(text, "ello");
  }

  #[test]
  fn move_left_moves_cursor() {
    let (text, mut edit) = with_text("hello");
    edit.move_left(&text, false);
    assert_eq!(edit.cursor(), 4);
    assert!(edit.selection().is_none());
  }

  #[test]
  fn move_left_with_shift_selects() {
    let (text, mut edit) = with_text("hello");
    edit.move_left(&text, true);
    assert_eq!(edit.cursor(), 4);
    assert_eq!(edit.anchor, 5);
    assert!(edit.selection().is_some());
  }

  #[test]
  fn move_right_moves_cursor() {
    let (text, mut edit) = with_text("hello");
    place(&mut edit, 0);
    edit.move_right(&text, false);
    assert_eq!(edit.cursor(), 1);
  }

  #[test]
  fn move_home_end() {
    let (text, mut edit) = with_text("hello world");
    place(&mut edit, 5);

    edit.move_home(false);
    assert_eq!(edit.cursor(), 0);

    edit.move_end(&text, false);
    assert_eq!(edit.cursor(), 11);
  }

  #[test]
  fn select_all() {
    let (text, mut edit) = with_text("hello");
    place(&mut edit, 2);
    edit.select_all(&text);
    assert_eq!(edit.anchor, 0);
    assert_eq!(edit.cursor(), 5);
    assert_eq!(edit.selected_text(&text), Some("hello"));
  }

  #[test]
  fn selection_range_returns_ordered() {
    let (_text, mut edit) = with_text("hello");
    // Selection from right to left
    edit.cursor = 1;
    edit.anchor = 4;
    assert_eq!(edit.selection(), Some(1..4));

    // Selection from left to right
    edit.cursor = 4;
    edit.anchor = 1;
    assert_eq!(edit.selection(), Some(1..4));
  }

  #[test]
  fn blink_updates() {
    let mut edit = TextEdit::default();
    assert!(edit.caret_shown());

    edit.tick(CARET_BLINK);
    assert!(!edit.caret_shown());

    edit.tick(CARET_BLINK);
    assert!(edit.caret_shown());
  }

  #[test]
  fn unicode_navigation() {
    // Test with multi-byte characters
    let (text, mut edit) = with_text("héllo");
    assert_eq!(edit.cursor(), 6); // 'é' is 2 bytes

    edit.move_left(&text, false);
    assert_eq!(edit.cursor(), 5); // Before 'o'

    place(&mut edit, 0);
    edit.move_right(&text, false);
    assert_eq!(edit.cursor(), 1); // After 'h'

    edit.move_right(&text, false);
    assert_eq!(edit.cursor(), 3); // After 'é' (2 bytes)
  }

  #[test]
  fn set_cursor_char_positions_correctly() {
    let (text, mut edit) = with_text("hello world");

    // Click at character 5 (the space)
    edit.set_cursor_char(&text, 5, false);
    assert_eq!(edit.cursor(), 5);
    assert_eq!(edit.anchor, 5);
    assert!(edit.selection().is_none());

    // Click at start
    edit.set_cursor_char(&text, 0, false);
    assert_eq!(edit.cursor(), 0);

    // Click past end - should clamp
    edit.set_cursor_char(&text, 100, false);
    assert_eq!(edit.cursor(), 11); // "hello world" is 11 chars
  }

  #[test]
  fn set_cursor_char_with_selection() {
    let (text, mut edit) = with_text("hello");
    edit.set_cursor_char(&text, 2, false); // Position at 'l'

    // Shift-click at position 4 to select
    edit.set_cursor_char(&text, 4, true);
    assert_eq!(edit.cursor(), 4);
    assert_eq!(edit.anchor, 2);
    assert!(edit.selection().is_some());
    assert_eq!(edit.selected_text(&text), Some("ll"));
  }

  #[test]
  fn set_cursor_char_with_unicode() {
    // "héllo" - 'é' is 2 bytes, so 6 bytes total for 5 characters
    let (text, mut edit) = with_text("héllo");

    // Click at character 2 (first 'l')
    edit.set_cursor_char(&text, 2, false);
    assert_eq!(edit.cursor(), 3); // After 'é' which is 2 bytes

    // Click at character 0
    edit.set_cursor_char(&text, 0, false);
    assert_eq!(edit.cursor(), 0);

    // Click at character 5 (end)
    edit.set_cursor_char(&text, 5, false);
    assert_eq!(edit.cursor(), 6); // Full byte length
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
    let (text, mut edit) = with_text("hello world");
    edit.cursor = 0;
    edit.anchor = 5;

    let copied = edit.copy(&text);
    assert!(copied);

    // Verify we can paste it back
    let mut clipboard = Clipboard::new().unwrap();
    assert_eq!(clipboard.get_text().unwrap(), "hello");
  }

  #[test]
  #[ignore] // Requires system clipboard access
  #[serial]
  fn copy_without_selection_returns_false() {
    let (text, edit) = with_text("hello");
    // No selection (cursor == anchor)
    assert!(!edit.copy(&text));
  }

  #[test]
  #[ignore] // Requires system clipboard access
  #[serial]
  fn cut_removes_selection_and_copies() {
    let (mut text, mut edit) = with_text("hello world");
    edit.cursor = 6;
    edit.anchor = 11; // Select "world"

    let cut_result = edit.cut(&mut text);
    assert!(cut_result);
    assert_eq!(text, "hello ");
    assert_eq!(edit.cursor(), 6);

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

    let (mut text, mut edit) = with_text("hello world");
    place(&mut edit, 6);

    let pasted = edit.paste(&mut text, None);
    assert!(pasted);
    assert_eq!(text, "hello insertedworld");
  }

  #[test]
  #[ignore] // Requires system clipboard access
  #[serial]
  fn paste_replaces_selection() {
    let mut clipboard = Clipboard::new().unwrap();
    clipboard.set_text("there").unwrap();

    let (mut text, mut edit) = with_text("hello world");
    edit.cursor = 6;
    edit.anchor = 11; // Select "world"

    let pasted = edit.paste(&mut text, None);
    assert!(pasted);
    assert_eq!(text, "hello there");
  }

  #[test]
  #[ignore] // Requires system clipboard access
  #[serial]
  fn paste_converts_newlines_to_spaces() {
    let mut clipboard = Clipboard::new().unwrap();
    clipboard.set_text("line1\nline2\r\nline3").unwrap();

    let (mut text, mut edit) = (String::new(), TextEdit::default());
    let pasted = edit.paste(&mut text, None);
    assert!(pasted);
    assert_eq!(text, "line1 line2  line3"); // \r\n becomes two spaces
  }

  // =========================================================================
  // Paste guard tests (multi-render & IME double-paste prevention)
  // =========================================================================

  #[test]
  fn paste_guard_starts_false() {
    let edit = TextEdit::default();
    assert!(!edit.paste_guard);
  }

  #[test]
  fn paste_guard_blocks_second_paste() {
    // Simulate: paste fires, then handle_input runs again (second render)
    // First render: paste fires, guard is set
    let edit = TextEdit {
      paste_guard: true,
      ..TextEdit::default()
    };

    // Second render: key_pressed(KeyV) would still be true, but guard blocks
    let ctrl = true;
    let key_v_pressed = true;
    if ctrl && key_v_pressed && !edit.paste_guard {
      panic!("paste should NOT fire when guard is active");
    }
  }

  #[test]
  fn paste_guard_blocks_ime_text_insertion() {
    let (mut text, mut edit) = (String::new(), TextEdit::default());
    edit.paste_guard = true;

    // Simulate IME committing clipboard text while paste_guard is active
    let typed = "clipboard content";
    let ctrl = false; // Ctrl may be released by now
    if !typed.is_empty() && !ctrl && !edit.paste_guard {
      edit.insert(&mut text, typed, None);
    }

    assert_eq!(text, ""); // Text was NOT inserted
  }

  #[test]
  fn paste_guard_resets_when_v_released() {
    let mut edit = TextEdit {
      paste_guard: true,
      ..TextEdit::default()
    };

    // Simulate: V is no longer held (key_down returns false)
    let v_key_down = false;
    if !v_key_down {
      edit.paste_guard = false;
    }

    assert!(!edit.paste_guard);
  }

  #[test]
  fn paste_guard_allows_next_paste_after_release() {
    // First paste: guard set
    let mut edit = TextEdit {
      paste_guard: true,
      ..TextEdit::default()
    };

    // V released: guard cleared
    edit.paste_guard = false;

    // Second paste: guard is false, paste allowed
    let ctrl = true;
    let key_v_pressed = true;
    let mut paste_fired = false;
    if ctrl && key_v_pressed && !edit.paste_guard {
      paste_fired = true;
      edit.paste_guard = true;
    }
    assert!(paste_fired);
    assert!(edit.paste_guard);
  }

  // =========================================================================
  // Scroll offset tests (text overflow navigation)
  // =========================================================================

  #[test]
  fn scroll_offset_starts_at_zero() {
    let edit = TextEdit::default();
    assert_eq!(edit.scroll(), 0.0);
  }

  /// Where the caret is, in pixels, at 8 px a character
  fn caret_px(text: &str, edit: &TextEdit) -> f32 {
    text[..edit.cursor()].chars().count() as f32 * 8.0
  }

  #[test]
  fn ensure_caret_visible_no_scroll_when_fits() {
    let (text, mut edit) = with_text("hello");
    // Caret at 40 px in a 100 px field
    edit.ensure_caret_visible(caret_px(&text, &edit), 100.0);
    assert_eq!(edit.scroll(), 0.0);
  }

  #[test]
  fn ensure_caret_visible_scrolls_right_when_cursor_overflows() {
    let (text, mut edit) = with_text("hello world test");
    // Caret at 128 px in an 80 px field: just inside the right margin
    edit.ensure_caret_visible(caret_px(&text, &edit), 80.0);
    assert_eq!(edit.scroll(), 128.0 - 80.0 + 2.0);
  }

  #[test]
  fn ensure_caret_visible_scrolls_left_when_cursor_before_view() {
    let (text, mut edit) = with_text("hello world test");
    edit.scroll = 80.0; // Scrolled far right
    place(&mut edit, 0); // Cursor at start

    edit.ensure_caret_visible(caret_px(&text, &edit), 80.0);
    assert_eq!(edit.scroll(), 0.0);
  }

  #[test]
  fn ensure_caret_visible_tracks_cursor_movement() {
    let (text, mut edit) = with_text("abcdefghijklmnop");
    let width = 40.0; // Only 5 chars visible

    // Caret at the end (128 px)
    edit.ensure_caret_visible(caret_px(&text, &edit), width);
    let scroll_at_end = edit.scroll();
    assert!(scroll_at_end > 0.0);

    // Caret at 24 px, left of the view: scrolls back to show it
    place(&mut edit, 3);
    edit.ensure_caret_visible(caret_px(&text, &edit), width);
    assert!(edit.scroll() < scroll_at_end);
    assert!(edit.scroll() <= 24.0);

    // Caret at the start
    place(&mut edit, 0);
    edit.ensure_caret_visible(caret_px(&text, &edit), width);
    assert_eq!(edit.scroll(), 0.0);
  }

  #[test]
  fn ensure_caret_visible_scroll_never_negative() {
    let (text, mut edit) = with_text("hi");
    place(&mut edit, 0);
    edit.ensure_caret_visible(caret_px(&text, &edit), 200.0);
    assert_eq!(edit.scroll(), 0.0);
  }

  #[test]
  fn ensure_caret_visible_with_insert_and_backspace() {
    let (mut text, mut edit) = (String::new(), TextEdit::default());
    let width = 40.0; // 5 chars visible

    // Type 10 characters: the caret at 80 px scrolls the field
    edit.insert(&mut text, "0123456789", None);
    edit.ensure_caret_visible(caret_px(&text, &edit), width);
    assert!(edit.scroll() > 0.0);

    // Backspace all the way back
    for _ in 0..10 {
      edit.backspace(&mut text);
      edit.ensure_caret_visible(caret_px(&text, &edit), width);
    }
    assert_eq!(text, "");
    assert_eq!(edit.scroll(), 0.0);
  }

  #[test]
  fn set_cursor_takes_a_byte_and_snaps_to_a_char_boundary() {
    let (text, mut edit) = with_text("héllo");
    // Inside 'é' (bytes 1..3): back to its start
    edit.set_cursor(&text, 2, false);
    assert_eq!(edit.cursor(), 1);
    assert!(edit.selection().is_none());
    // Past the end: the end
    edit.set_cursor(&text, 100, false);
    assert_eq!(edit.cursor(), "héllo".len());
    // Extending selects from the anchor
    edit.set_cursor(&text, 1, false);
    edit.set_cursor(&text, 3, true);
    assert_eq!(edit.selected_text(&text), Some("é"));
  }

  // =========================================================================
  // The caller's string and the frame's keys
  // =========================================================================

  #[test]
  fn inserting_past_max_len_truncates_the_insertion() {
    let (mut text, mut edit) = with_text("abc");
    assert!(edit.insert(&mut text, "défgh", Some(5)));
    assert_eq!(text, "abcdé", "max_len counts chars, not bytes");
    assert!(
      !edit.insert(&mut text, "x", Some(5)),
      "full: nothing goes in"
    );
    assert_eq!(text, "abcdé");
    // A selection makes room for what replaces it
    edit.set_cursor(&text, 0, false);
    edit.set_cursor(&text, 2, true);
    edit.insert(&mut text, "XYZ", Some(5));
    assert_eq!(text, "XYcdé");
  }

  #[test]
  fn clamp_to_follows_a_string_the_caller_shortened() {
    let (mut text, mut edit) = with_text("héllo wörld");
    edit.set_cursor(&text, 0, false);
    edit.set_cursor(&text, text.len(), true);
    text.truncate(1);
    edit.clamp_to(&text);
    assert_eq!(edit.cursor(), 1);
    assert_eq!(edit.selection(), Some(0..1));
    // Into the middle of a character: back to its start
    let text = String::from("é");
    let mut edit = TextEdit {
      cursor: 1,
      anchor: 1,
      ..TextEdit::default()
    };
    edit.clamp_to(&text);
    assert_eq!(edit.cursor(), 0);
  }

  /// The frame's keys and text as the UI sees them
  fn keys(pressed: &[winit::keyboard::KeyCode], typed: &str) -> UiInput {
    use winit::event::ElementState;
    let mut input = crate::core::input::Input::new();
    for key in pressed {
      input.handle_keyboard(*key, ElementState::Pressed);
    }
    input.handle_text_input(typed);
    UiInput::snapshot(&input)
  }

  #[test]
  fn typing_edits_the_callers_string_and_enter_submits() {
    use winit::keyboard::KeyCode;
    let (mut text, mut edit) = with_text("ab");
    let out = edit.handle_input(&mut text, &keys(&[], "c"), None);
    assert_eq!(text, "abc");
    assert!(out.changed && !out.submitted);
    let out = edit.handle_input(&mut text, &keys(&[KeyCode::Backspace], ""), None);
    assert_eq!(text, "ab");
    assert!(out.changed);
    let out = edit.handle_input(&mut text, &keys(&[KeyCode::ArrowLeft], ""), None);
    assert!(!out.changed, "moving the caret changes nothing");
    assert_eq!(edit.cursor(), 1);
    let out = edit.handle_input(&mut text, &keys(&[KeyCode::Enter], ""), None);
    assert!(out.submitted && !out.changed);
    let out = edit.handle_input(&mut text, &keys(&[KeyCode::NumpadEnter], ""), None);
    assert!(out.submitted);
    // Backspace at the start changes nothing
    edit.set_cursor(&text, 0, false);
    let out = edit.handle_input(&mut text, &keys(&[KeyCode::Backspace], ""), None);
    assert!(!out.changed);
    assert_eq!(text, "ab");
  }
}
