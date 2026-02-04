//! In-game developer console for Ryo2D
//!
//! A moveable, resizable developer console window, toggled with the tilde (~) key.
//! Provides runtime debugging, configuration, and command execution.
//!
//! # Features
//!
//! - Toggle visibility with tilde key
//! - Draggable title bar, resizable window
//! - Command history with up/down arrows
//! - Built-in commands (help, clear, quit, fps, etc.)
//! - Custom command registration via game API
//!
//! # Configuration
//!
//! Enable in `Ryo.toml`:
//! ```toml
//! [system]
//! console = true  # Works in both debug and release builds
//! ```
//!
//! # Example
//!
//! ```ignore
//! // Register a custom command in your game
//! fn init(&mut self, ctx: &mut GameContext) {
//!     ctx.register_console_command(
//!         "spawn_enemy",
//!         "Spawn an enemy at the player's position",
//!         |args, cmd_ctx| {
//!             cmd_ctx.console.print("Spawning enemy...");
//!             CommandResult::Ok
//!         }
//!     );
//! }
//! ```

mod builtins;
mod commands;

pub use builtins::register_builtins;
pub use commands::{CommandContext, CommandFn, CommandInfo, CommandRegistry, CommandResult};

use std::collections::VecDeque;

use super::cursor::CursorStyle;
use super::input::InputState;
use super::ui::{Color, Rect, UiContext};
use winit::event::MouseButton;
use winit::keyboard::KeyCode;

/// Maximum number of output lines to keep in history
const MAX_OUTPUT_LINES: usize = 1000;

/// Maximum number of commands to keep in history
const MAX_COMMAND_HISTORY: usize = 100;

/// Minimum console window dimensions
const MIN_WIDTH: f32 = 300.0;
const MIN_HEIGHT: f32 = 150.0;

/// Default console window dimensions
const DEFAULT_WIDTH: f32 = 600.0;
const DEFAULT_HEIGHT: f32 = 400.0;

/// Title bar height
const TITLE_BAR_HEIGHT: f32 = 24.0;

/// Close button size
const CLOSE_BUTTON_SIZE: f32 = 16.0;

/// Input line height
const INPUT_HEIGHT: f32 = 24.0;

/// Resize handle size (corner)
const RESIZE_HANDLE_SIZE: f32 = 12.0;

/// Scrollbar width
const SCROLLBAR_WIDTH: f32 = 8.0;

/// Minimum scrollbar thumb height
const SCROLLBAR_MIN_THUMB: f32 = 20.0;

/// Console visual colors
mod colors {
  use super::Color;

  /// Console background when focused (dark, mostly opaque)
  pub const BACKGROUND_FOCUSED: Color = Color::new(20, 20, 30, 245);
  /// Console background when unfocused (more transparent)
  pub const BACKGROUND_UNFOCUSED: Color = Color::new(20, 20, 30, 160);
  /// Title bar background
  pub const TITLE_BAR: Color = Color::new(40, 40, 55, 255);
  /// Title bar when unfocused
  pub const TITLE_BAR_UNFOCUSED: Color = Color::new(40, 40, 55, 180);
  /// Title bar when dragging
  pub const TITLE_BAR_ACTIVE: Color = Color::new(50, 50, 70, 255);
  /// Input line background
  pub const INPUT_BG: Color = Color::new(25, 25, 35, 255);
  /// Input line background when unfocused
  pub const INPUT_BG_UNFOCUSED: Color = Color::new(25, 25, 35, 180);
  /// Normal text color
  pub const TEXT: Color = Color::WHITE;
  /// Text color when unfocused
  pub const TEXT_UNFOCUSED: Color = Color::new(255, 255, 255, 180);
  /// Command echo color (light blue)
  pub const COMMAND: Color = Color::new(100, 180, 255, 255);
  /// Error message color
  pub const ERROR: Color = Color::new(255, 100, 100, 255);
  /// System message color (gray)
  pub const SYSTEM: Color = Color::new(150, 150, 150, 255);
  /// Cursor color
  pub const CURSOR: Color = Color::new(200, 200, 200, 255);
  /// Border color
  pub const BORDER: Color = Color::new(60, 60, 80, 255);
  /// Border color when unfocused
  pub const BORDER_UNFOCUSED: Color = Color::new(60, 60, 80, 140);
  /// Resize handle color
  pub const RESIZE_HANDLE: Color = Color::new(80, 80, 100, 255);
  /// Scrollbar track color
  pub const SCROLLBAR_TRACK: Color = Color::new(30, 30, 40, 200);
  /// Scrollbar thumb color
  pub const SCROLLBAR_THUMB: Color = Color::new(80, 80, 100, 255);
  /// Scrollbar thumb color when hovered/active (for future drag support)
  pub const _SCROLLBAR_THUMB_HOVER: Color = Color::new(100, 100, 130, 255);
}

/// A single line of console output
#[derive(Debug, Clone)]
pub struct ConsoleLine {
  /// The text content of the line
  pub text: String,
  /// The color to render this line
  pub color: Color,
  /// Timestamp when this line was added (in seconds since console was created)
  pub timestamp: f32,
}

impl ConsoleLine {
  /// Create a new console line
  pub fn new(text: impl Into<String>, color: Color, timestamp: f32) -> Self {
    Self {
      text: text.into(),
      color,
      timestamp,
    }
  }
}

/// Drag state for window movement
#[derive(Debug, Clone, Copy, PartialEq)]
enum DragState {
  None,
  Dragging { offset_x: f32, offset_y: f32 },
  Resizing { start_w: f32, start_h: f32, start_mouse_x: f32, start_mouse_y: f32 },
  ScrollbarDragging { start_scroll_offset: usize, start_mouse_y: f32 },
}

/// Console state and rendering
#[derive(Debug)]
pub struct ConsoleState {
  /// Whether the console is currently visible
  pub visible: bool,
  /// Whether the console is currently focused (receives input)
  pub focused: bool,
  /// Window position (top-left)
  x: f32,
  y: f32,
  /// Window dimensions
  width: f32,
  height: f32,
  /// Current input buffer
  input_buffer: String,
  /// Cursor position in the input buffer (byte offset)
  cursor_pos: usize,
  /// Output lines (newest at the end)
  output_lines: VecDeque<ConsoleLine>,
  /// Command history (newest at the end)
  command_history: Vec<String>,
  /// Current position in command history (None = not browsing)
  history_index: Option<usize>,
  /// Saved input when browsing history
  saved_input: String,
  /// Scroll offset from the bottom (0 = showing newest lines)
  scroll_offset: usize,
  /// Timer for cursor blink animation
  cursor_blink_timer: f32,
  /// Total time elapsed (for timestamps)
  total_time: f32,
  /// Current drag state
  drag_state: DragState,
  /// Skip text input this frame (to avoid inserting toggle key)
  skip_text_input: bool,
}

impl Default for ConsoleState {
  fn default() -> Self {
    Self::new()
  }
}

impl ConsoleState {
  /// Create a new console state
  pub fn new() -> Self {
    let mut console = Self {
      visible: false,
      focused: true,
      x: 50.0,
      y: 50.0,
      width: DEFAULT_WIDTH,
      height: DEFAULT_HEIGHT,
      input_buffer: String::new(),
      cursor_pos: 0,
      output_lines: VecDeque::with_capacity(MAX_OUTPUT_LINES),
      command_history: Vec::with_capacity(MAX_COMMAND_HISTORY),
      history_index: None,
      saved_input: String::new(),
      scroll_offset: 0,
      cursor_blink_timer: 0.0,
      total_time: 0.0,
      drag_state: DragState::None,
      skip_text_input: false,
    };

    // Print welcome message
    console.print_system("Welcome to Ryo2D Developer Console");
    console.print_system("Type 'help' for available commands");
    console
  }

  /// Print a line to the console with a specific color
  pub fn print_colored(&mut self, text: impl Into<String>, color: Color) {
    let line = ConsoleLine::new(text, color, self.total_time);
    self.output_lines.push_back(line);

    // Trim old lines if we exceed the maximum
    while self.output_lines.len() > MAX_OUTPUT_LINES {
      self.output_lines.pop_front();
    }

    // Reset scroll to bottom when new content is added
    self.scroll_offset = 0;
  }

  /// Print a normal text line
  pub fn print(&mut self, text: impl Into<String>) {
    self.print_colored(text, colors::TEXT);
  }

  /// Print a command echo (light blue)
  pub fn print_command(&mut self, text: impl Into<String>) {
    self.print_colored(text, colors::COMMAND);
  }

  /// Print an error message (red)
  pub fn print_error(&mut self, text: impl Into<String>) {
    self.print_colored(text, colors::ERROR);
  }

  /// Print a system message (gray)
  pub fn print_system(&mut self, text: impl Into<String>) {
    self.print_colored(text, colors::SYSTEM);
  }

  /// Clear all output lines
  pub fn clear(&mut self) {
    self.output_lines.clear();
    self.scroll_offset = 0;
  }

  /// Toggle console visibility
  pub fn toggle(&mut self) {
    self.visible = !self.visible;
    if self.visible {
      self.focused = true;
      self.cursor_blink_timer = 0.0;
      // Skip text input this frame to avoid inserting the toggle key
      self.skip_text_input = true;
    }
  }

  /// Update cursor blink and timers
  pub fn update(&mut self, dt: f32) {
    self.total_time += dt;
    if self.visible {
      self.cursor_blink_timer += dt;
      if self.cursor_blink_timer > 1.0 {
        self.cursor_blink_timer -= 1.0;
      }
    }
  }

  /// Get the title bar bounds
  fn title_bar_rect(&self) -> Rect {
    Rect::new(self.x, self.y, self.width, TITLE_BAR_HEIGHT)
  }

  /// Get the resize handle bounds (bottom-right corner)
  fn resize_handle_rect(&self) -> Rect {
    Rect::new(
      self.x + self.width - RESIZE_HANDLE_SIZE,
      self.y + self.height - RESIZE_HANDLE_SIZE,
      RESIZE_HANDLE_SIZE,
      RESIZE_HANDLE_SIZE,
    )
  }

  /// Get the close button bounds (top-right corner of title bar)
  fn close_button_rect(&self) -> Rect {
    let padding = (TITLE_BAR_HEIGHT - CLOSE_BUTTON_SIZE) / 2.0;
    Rect::new(
      self.x + self.width - CLOSE_BUTTON_SIZE - padding,
      self.y + padding,
      CLOSE_BUTTON_SIZE,
      CLOSE_BUTTON_SIZE,
    )
  }

  /// Get the desired cursor style based on current state and mouse position.
  ///
  /// Returns `Some(CursorStyle)` if the console wants to override the cursor,
  /// or `None` if the console doesn't need to control the cursor.
  pub fn desired_cursor(&self, mouse_x: f32, mouse_y: f32) -> Option<CursorStyle> {
    if !self.visible {
      return None;
    }

    // Check if currently resizing (always show resize cursor during resize)
    if matches!(self.drag_state, DragState::Resizing { .. }) {
      return Some(CursorStyle::ResizeNwSe);
    }

    // Check if currently dragging
    if matches!(self.drag_state, DragState::Dragging { .. }) {
      return Some(CursorStyle::Move);
    }

    // Check if hovering over resize handle
    let resize_rect = self.resize_handle_rect();
    if resize_rect.contains(mouse_x, mouse_y) {
      return Some(CursorStyle::ResizeNwSe);
    }

    // If focused and mouse is over console window, use default cursor
    // If unfocused, let the game control the cursor
    if self.focused {
      let window_rect = self.window_rect();
      if window_rect.contains(mouse_x, mouse_y) {
        return Some(CursorStyle::Default);
      }
    }

    None
  }

  /// Get the full window bounds
  fn window_rect(&self) -> Rect {
    Rect::new(self.x, self.y, self.width, self.height)
  }

  /// Get the output area bounds (between title bar and input line)
  fn output_area(&self) -> (f32, f32, f32, f32) {
    let padding = 8.0;
    let input_y = self.y + self.height - INPUT_HEIGHT;
    let output_top = self.y + TITLE_BAR_HEIGHT + padding;
    let output_bottom = input_y - padding;
    let output_height = output_bottom - output_top;
    (output_top, output_bottom, output_height, padding)
  }

  /// Get the scrollbar track bounds
  fn scrollbar_track_rect(&self) -> Rect {
    let (output_top, _, output_height, _) = self.output_area();
    let scrollbar_x = self.x + self.width - SCROLLBAR_WIDTH - 4.0;
    Rect::new(scrollbar_x, output_top, SCROLLBAR_WIDTH, output_height)
  }

  /// Get the scrollbar thumb bounds (returns None if no scrollbar needed)
  fn scrollbar_thumb_rect(&self, visible_lines: usize, total_lines: usize) -> Option<Rect> {
    if total_lines <= visible_lines {
      return None;
    }

    let (output_top, _, output_height, _) = self.output_area();
    let scrollbar_x = self.x + self.width - SCROLLBAR_WIDTH - 4.0;

    let visible_ratio = visible_lines as f32 / total_lines as f32;
    let thumb_height = (output_height * visible_ratio).max(SCROLLBAR_MIN_THUMB);

    let max_scroll = total_lines.saturating_sub(visible_lines);
    let scroll_ratio = if max_scroll > 0 {
      self.scroll_offset as f32 / max_scroll as f32
    } else {
      0.0
    };

    let thumb_travel = output_height - thumb_height;
    let thumb_y = output_top + thumb_travel * (1.0 - scroll_ratio);

    Some(Rect::new(scrollbar_x, thumb_y, SCROLLBAR_WIDTH, thumb_height))
  }

  /// Calculate max scroll offset
  fn max_scroll_offset(&self, visible_lines: usize, total_lines: usize) -> usize {
    total_lines.saturating_sub(visible_lines)
  }

  /// Get the maximum text width for word wrapping
  fn max_text_width(&self, has_scrollbar: bool) -> f32 {
    let padding = 8.0;
    let scrollbar_margin = if has_scrollbar { SCROLLBAR_WIDTH + 12.0 } else { 0.0 };
    self.width - padding * 2.0 - scrollbar_margin
  }

  /// Wrap text to fit within the given max width, returning wrapped lines
  fn wrap_text<'a>(&self, text: &'a str, max_width: f32, ui: &UiContext) -> Vec<&'a str> {
    let mut lines = Vec::new();
    let mut remaining = text;

    while !remaining.is_empty() {
      let (text_width, _) = ui.text_size(remaining);
      if (text_width as f32) <= max_width {
        lines.push(remaining);
        break;
      }

      // Find break point - try to break at word boundary
      let mut break_pos = remaining.len();
      let mut last_space = None;

      for (i, c) in remaining.char_indices() {
        let prefix = &remaining[..i + c.len_utf8()];
        let (prefix_width, _) = ui.text_size(prefix);
        if prefix_width as f32 > max_width {
          // Use last space if available, otherwise break here
          break_pos = last_space.unwrap_or(i.max(1));
          break;
        }
        if c == ' ' {
          last_space = Some(i + 1); // Include space at end of previous line
        }
      }

      // If we couldn't find a good break point, break at max width
      if break_pos == remaining.len() && (ui.text_size(remaining).0 as f32) > max_width {
        break_pos = 1; // At minimum, break after first char
      }

      let (line, rest) = remaining.split_at(break_pos);
      lines.push(line.trim_end());
      remaining = rest.trim_start();
    }

    if lines.is_empty() {
      lines.push("");
    }
    lines
  }

  /// Check if the console is currently focused
  pub fn is_focused(&self) -> bool {
    self.focused
  }

  /// Handle input when console is visible
  ///
  /// Returns Some(command) if a command was submitted, None otherwise.
  /// Also handles focus changes - clicking outside unfocuses, clicking inside focuses.
  pub fn handle_input(&mut self, input: &mut InputState) -> Option<String> {
    if !self.visible {
      return None;
    }

    let (mouse_x, mouse_y) = input.mouse_position();
    let window_rect = self.window_rect();

    // Calculate scrollbar info for interactions
    let line_height = 12.0;
    let (_, _, output_height, _) = self.output_area();
    let visible_lines = (output_height / line_height) as usize;
    let total_lines = self.output_lines.len();
    let max_scroll = self.max_scroll_offset(visible_lines, total_lines);

    // Handle window dragging/resizing/scrollbar (works even when unfocused)
    match self.drag_state {
      DragState::Dragging { offset_x, offset_y } => {
        if input.mouse_down(MouseButton::Left) {
          self.x = mouse_x - offset_x;
          self.y = mouse_y - offset_y;
        } else {
          self.drag_state = DragState::None;
        }
        // Don't process other input while dragging
        return None;
      }
      DragState::Resizing { start_w, start_h, start_mouse_x, start_mouse_y } => {
        if input.mouse_down(MouseButton::Left) {
          let delta_x = mouse_x - start_mouse_x;
          let delta_y = mouse_y - start_mouse_y;
          self.width = (start_w + delta_x).max(MIN_WIDTH);
          self.height = (start_h + delta_y).max(MIN_HEIGHT);
        } else {
          self.drag_state = DragState::None;
        }
        // Don't process other input while resizing
        return None;
      }
      DragState::ScrollbarDragging { start_scroll_offset, start_mouse_y } => {
        if input.mouse_down(MouseButton::Left) {
          // Calculate scroll based on mouse movement
          let (_, _, output_height, _) = self.output_area();
          let visible_ratio = visible_lines as f32 / total_lines.max(1) as f32;
          let thumb_height = (output_height * visible_ratio).max(SCROLLBAR_MIN_THUMB);
          let thumb_travel = output_height - thumb_height;

          if thumb_travel > 0.0 && max_scroll > 0 {
            let delta_y = mouse_y - start_mouse_y;
            // Dragging down = scroll_ratio increases = scroll_offset decreases (toward bottom)
            let scroll_delta = (delta_y / thumb_travel * max_scroll as f32).round() as i32;
            let new_offset = (start_scroll_offset as i32 - scroll_delta).max(0).min(max_scroll as i32);
            self.scroll_offset = new_offset as usize;
          }
        } else {
          self.drag_state = DragState::None;
        }
        return None;
      }
      DragState::None => {}
    }

    // Handle mouse clicks for focus and window interactions
    if input.mouse_pressed(MouseButton::Left) {
      if window_rect.contains(mouse_x, mouse_y) {
        // Clicked inside console window - focus it
        self.focused = true;

        let close_rect = self.close_button_rect();
        let resize_rect = self.resize_handle_rect();
        let title_rect = self.title_bar_rect();
        let scrollbar_track = self.scrollbar_track_rect();

        // Check close button first (it overlaps with title bar)
        if close_rect.contains(mouse_x, mouse_y) {
          self.visible = false;
          return None;
        } else if resize_rect.contains(mouse_x, mouse_y) {
          self.drag_state = DragState::Resizing {
            start_w: self.width,
            start_h: self.height,
            start_mouse_x: mouse_x,
            start_mouse_y: mouse_y,
          };
          return None;
        } else if total_lines > visible_lines && scrollbar_track.contains(mouse_x, mouse_y) {
          // Clicked on scrollbar track - start dragging
          if let Some(thumb_rect) = self.scrollbar_thumb_rect(visible_lines, total_lines) {
            if thumb_rect.contains(mouse_x, mouse_y) {
              // Clicked directly on thumb - start dragging from current position
              self.drag_state = DragState::ScrollbarDragging {
                start_scroll_offset: self.scroll_offset,
                start_mouse_y: mouse_y,
              };
            } else {
              // Clicked on track above or below thumb - jump to that position
              let (output_top, _, output_height, _) = self.output_area();
              let click_ratio = (mouse_y - output_top) / output_height;
              // Invert because scroll_offset=0 is bottom, max is top
              let new_scroll = ((1.0 - click_ratio) * max_scroll as f32).round() as usize;
              self.scroll_offset = new_scroll.min(max_scroll);
              // Start dragging from new position
              self.drag_state = DragState::ScrollbarDragging {
                start_scroll_offset: self.scroll_offset,
                start_mouse_y: mouse_y,
              };
            }
          }
          return None;
        } else if title_rect.contains(mouse_x, mouse_y) {
          self.drag_state = DragState::Dragging {
            offset_x: mouse_x - self.x,
            offset_y: mouse_y - self.y,
          };
          return None;
        }
      } else {
        // Clicked outside console window - unfocus it
        self.focused = false;
        return None;
      }
    }

    // Mouse wheel scrolling works even when unfocused (as long as mouse is over console)
    let (_, scroll_y) = input.scroll_delta();
    if scroll_y.abs() > 0.01 && window_rect.contains(mouse_x, mouse_y) {
      // Scroll up (positive scroll_y) increases offset, scroll down decreases
      let scroll_lines = (scroll_y * 3.0).round() as i32;
      if scroll_lines > 0 {
        self.scroll_offset = (self.scroll_offset + scroll_lines as usize)
          .min(self.output_lines.len().saturating_sub(1));
      } else {
        self.scroll_offset = self.scroll_offset.saturating_sub((-scroll_lines) as usize);
      }
    }

    // Only process keyboard input when focused
    if !self.focused {
      return None;
    }

    let mut submitted_command = None;

    // Handle text input (from IME or keyboard)
    // Skip on the frame console was opened to avoid inserting toggle key
    if !self.skip_text_input && input.has_text_input() {
      let text = input.text_input();
      // Filter out backtick/tilde characters (the toggle key)
      let filtered: String = text.chars().filter(|&c| c != '`' && c != '~').collect();
      if !filtered.is_empty() {
        self.input_buffer.insert_str(self.cursor_pos, &filtered);
        self.cursor_pos += filtered.len();
        self.cursor_blink_timer = 0.0;
      }
    }
    self.skip_text_input = false;

    // Handle special keys
    if input.key_repeat(KeyCode::Backspace) && self.cursor_pos > 0 {
      // Find the byte position of the previous character
      let prev_char_boundary = self.input_buffer[..self.cursor_pos]
        .char_indices()
        .last()
        .map(|(i, _)| i)
        .unwrap_or(0);
      self.input_buffer.drain(prev_char_boundary..self.cursor_pos);
      self.cursor_pos = prev_char_boundary;
      self.cursor_blink_timer = 0.0;
    }

    if input.key_repeat(KeyCode::Delete) && self.cursor_pos < self.input_buffer.len() {
      // Find the byte position of the next character
      let next_char_boundary = self.input_buffer[self.cursor_pos..]
        .char_indices()
        .nth(1)
        .map(|(i, _)| self.cursor_pos + i)
        .unwrap_or(self.input_buffer.len());
      self.input_buffer.drain(self.cursor_pos..next_char_boundary);
      self.cursor_blink_timer = 0.0;
    }

    // Left/Right arrow keys for cursor movement
    if input.key_repeat(KeyCode::ArrowLeft) && self.cursor_pos > 0 {
      // Find previous character boundary
      let prev_char_boundary = self.input_buffer[..self.cursor_pos]
        .char_indices()
        .last()
        .map(|(i, _)| i)
        .unwrap_or(0);
      self.cursor_pos = prev_char_boundary;
      self.cursor_blink_timer = 0.0;
    }

    if input.key_repeat(KeyCode::ArrowRight) && self.cursor_pos < self.input_buffer.len() {
      // Find next character boundary
      let next_char_boundary = self.input_buffer[self.cursor_pos..]
        .char_indices()
        .nth(1)
        .map(|(i, _)| self.cursor_pos + i)
        .unwrap_or(self.input_buffer.len());
      self.cursor_pos = next_char_boundary;
      self.cursor_blink_timer = 0.0;
    }

    // Home/End keys
    if input.key_pressed(KeyCode::Home) {
      self.cursor_pos = 0;
      self.cursor_blink_timer = 0.0;
    }

    if input.key_pressed(KeyCode::End) {
      self.cursor_pos = self.input_buffer.len();
      self.cursor_blink_timer = 0.0;
    }

    // Up/Down arrows for command history
    if input.key_pressed(KeyCode::ArrowUp)
      && !self.command_history.is_empty()
    {
      match self.history_index {
        None => {
          // Start browsing history
          self.saved_input = self.input_buffer.clone();
          self.history_index = Some(self.command_history.len() - 1);
          self.input_buffer = self.command_history.last().unwrap().clone();
        }
        Some(idx) if idx > 0 => {
          // Move up in history
          self.history_index = Some(idx - 1);
          self.input_buffer = self.command_history[idx - 1].clone();
        }
        _ => {}
      }
      self.cursor_pos = self.input_buffer.len();
      self.cursor_blink_timer = 0.0;
    }

    if input.key_pressed(KeyCode::ArrowDown)
      && let Some(idx) = self.history_index
    {
      if idx + 1 < self.command_history.len() {
        // Move down in history
        self.history_index = Some(idx + 1);
        self.input_buffer = self.command_history[idx + 1].clone();
      } else {
        // Exit history browsing, restore saved input
        self.history_index = None;
        self.input_buffer = std::mem::take(&mut self.saved_input);
      }
      self.cursor_pos = self.input_buffer.len();
      self.cursor_blink_timer = 0.0;
    }

    // Page Up/Down for scrolling
    if input.key_pressed(KeyCode::PageUp) {
      self.scroll_offset = (self.scroll_offset + 10).min(self.output_lines.len().saturating_sub(1));
    }

    if input.key_pressed(KeyCode::PageDown) {
      self.scroll_offset = self.scroll_offset.saturating_sub(10);
    }

    // Enter to submit command
    if input.key_pressed(KeyCode::Enter) && !self.input_buffer.is_empty() {
      let command = self.input_buffer.clone();

      // Echo the command
      self.print_command(format!("> {}", command));

      // Add to history
      if self
        .command_history
        .last()
        .map(|last| last != &command)
        .unwrap_or(true)
      {
        self.command_history.push(command.clone());
        if self.command_history.len() > MAX_COMMAND_HISTORY {
          self.command_history.remove(0);
        }
      }

      // Clear input state
      self.input_buffer.clear();
      self.cursor_pos = 0;
      self.history_index = None;
      self.saved_input.clear();

      submitted_command = Some(command);
    }

    // Escape to close console
    if input.key_pressed(KeyCode::Escape) {
      self.visible = false;
    }

    submitted_command
  }

  /// Render the console window
  pub fn render(&self, ui: &mut UiContext, frame_width: u32, frame_height: u32) {
    if !self.visible {
      return;
    }

    let padding = 8.0;
    let line_height = 12.0;

    // Choose colors based on focus state
    let bg_color = if self.focused {
      colors::BACKGROUND_FOCUSED
    } else {
      colors::BACKGROUND_UNFOCUSED
    };
    let border_color = if self.focused {
      colors::BORDER
    } else {
      colors::BORDER_UNFOCUSED
    };
    let title_bar_color = if matches!(self.drag_state, DragState::Dragging { .. }) {
      colors::TITLE_BAR_ACTIVE
    } else if self.focused {
      colors::TITLE_BAR
    } else {
      colors::TITLE_BAR_UNFOCUSED
    };
    let input_bg_color = if self.focused {
      colors::INPUT_BG
    } else {
      colors::INPUT_BG_UNFOCUSED
    };
    let text_color = if self.focused {
      colors::TEXT
    } else {
      colors::TEXT_UNFOCUSED
    };

    // Clamp window position to screen bounds
    let x = self.x.max(0.0).min((frame_width as f32 - self.width).max(0.0));
    let y = self.y.max(0.0).min((frame_height as f32 - self.height).max(0.0));

    // Draw window background
    let bg_rect = Rect::new(x, y, self.width, self.height);
    ui.draw_rect(bg_rect, bg_color);

    // Draw border
    ui.draw_rect_outline(bg_rect, border_color, 1.0);

    // Draw title bar
    let title_rect = Rect::new(x, y, self.width, TITLE_BAR_HEIGHT);
    ui.draw_rect(title_rect, title_bar_color);
    ui.draw_text("Developer Console", x + padding, y + 6.0, text_color);

    // Draw close button (X) in top-right corner
    let close_padding = (TITLE_BAR_HEIGHT - CLOSE_BUTTON_SIZE) / 2.0;
    let close_x = x + self.width - CLOSE_BUTTON_SIZE - close_padding;
    let close_y = y + close_padding;
    let close_rect = Rect::new(close_x, close_y, CLOSE_BUTTON_SIZE, CLOSE_BUTTON_SIZE);
    let close_btn_color = if self.focused {
      Color::new(80, 40, 40, 255)
    } else {
      Color::new(80, 40, 40, 160)
    };
    ui.draw_rect(close_rect, close_btn_color);
    // Draw X centered in the button
    let (x_width, x_height) = ui.text_size("X");
    let x_text_x = close_x + (CLOSE_BUTTON_SIZE - x_width as f32) / 2.0;
    let x_text_y = close_y + (CLOSE_BUTTON_SIZE - x_height as f32) / 2.0;
    ui.draw_text("X", x_text_x, x_text_y, text_color);

    // Draw input line background
    let input_y = y + self.height - INPUT_HEIGHT;
    let input_rect = Rect::new(x, input_y, self.width, INPUT_HEIGHT);
    ui.draw_rect(input_rect, input_bg_color);

    // Draw input prompt and text
    let prompt = "> ";
    let prompt_x = x + padding;
    let input_text_y = input_y + 6.0;
    ui.draw_text(prompt, prompt_x, input_text_y, colors::SYSTEM);

    let (prompt_width, _) = ui.text_size(prompt);
    let text_x = prompt_x + prompt_width as f32;
    ui.draw_text(&self.input_buffer, text_x, input_text_y, text_color);

    // Draw cursor (blinking) - only when focused
    if self.focused && self.cursor_blink_timer < 0.5 {
      let cursor_text = &self.input_buffer[..self.cursor_pos];
      let (cursor_offset, _) = ui.text_size(cursor_text);
      let cursor_x = text_x + cursor_offset as f32;
      let cursor_rect = Rect::new(cursor_x, input_y + 4.0, 2.0, INPUT_HEIGHT - 8.0);
      ui.draw_rect(cursor_rect, colors::CURSOR);
    }

    // Draw resize handle (bottom-right corner)
    let resize_rect = Rect::new(
      x + self.width - RESIZE_HANDLE_SIZE,
      y + self.height - RESIZE_HANDLE_SIZE,
      RESIZE_HANDLE_SIZE,
      RESIZE_HANDLE_SIZE,
    );
    // Draw diagonal lines to indicate resize handle
    for i in 0..3 {
      let offset = (i as f32) * 4.0 + 2.0;
      let line_rect = Rect::new(
        resize_rect.x + offset,
        resize_rect.y + RESIZE_HANDLE_SIZE - 2.0,
        RESIZE_HANDLE_SIZE - offset,
        1.0,
      );
      ui.draw_rect(line_rect, colors::RESIZE_HANDLE);
    }

    // Calculate visible output area (leave room for scrollbar on right)
    let output_top = y + TITLE_BAR_HEIGHT + padding;
    let output_bottom = input_y - padding;
    let output_height = output_bottom - output_top;
    let visible_line_count = (output_height / line_height) as usize;

    // First pass: calculate total wrapped lines to determine if scrollbar is needed
    // We need to know if there's a scrollbar to calculate max text width correctly
    let raw_line_count = self.output_lines.len();
    let has_scrollbar_estimate = raw_line_count > visible_line_count;
    let max_text_width = self.max_text_width(has_scrollbar_estimate);

    // Build list of wrapped lines with their colors
    let mut wrapped_lines: Vec<(String, Color)> = Vec::new();
    for line in &self.output_lines {
      let parts = self.wrap_text(&line.text, max_text_width, ui);
      for part in parts {
        wrapped_lines.push((part.to_string(), line.color));
      }
    }

    let total_lines = wrapped_lines.len();
    let has_scrollbar = total_lines > visible_line_count;

    // Draw scrollbar if there's more content than visible
    let scrollbar_x = x + self.width - SCROLLBAR_WIDTH - 4.0;
    if has_scrollbar {
      // Draw scrollbar track
      let track_rect = Rect::new(scrollbar_x, output_top, SCROLLBAR_WIDTH, output_height);
      ui.draw_rect(track_rect, colors::SCROLLBAR_TRACK);

      // Calculate thumb size and position
      let visible_ratio = visible_line_count as f32 / total_lines as f32;
      let thumb_height = (output_height * visible_ratio).max(SCROLLBAR_MIN_THUMB);

      // scroll_offset=0 means at bottom, max scroll_offset means at top
      let max_scroll = total_lines.saturating_sub(visible_line_count);
      let scroll_ratio = if max_scroll > 0 {
        self.scroll_offset.min(max_scroll) as f32 / max_scroll as f32
      } else {
        0.0
      };

      // Thumb position: at bottom when scroll_offset=0, at top when scroll_offset=max
      let thumb_travel = output_height - thumb_height;
      let thumb_y = output_top + thumb_travel * (1.0 - scroll_ratio);

      // Draw thumb
      let thumb_rect = Rect::new(scrollbar_x, thumb_y, SCROLLBAR_WIDTH, thumb_height);
      ui.draw_rect(thumb_rect, colors::SCROLLBAR_THUMB);
    }

    // Draw output lines (from bottom to top, accounting for scroll)
    // Clamp scroll_offset to valid range
    let max_scroll = total_lines.saturating_sub(visible_line_count);
    let effective_scroll = self.scroll_offset.min(max_scroll);

    let start_idx = total_lines.saturating_sub(visible_line_count + effective_scroll);
    let end_idx = total_lines.saturating_sub(effective_scroll);

    let mut text_y = output_bottom - line_height;
    for i in (start_idx..end_idx).rev() {
      if text_y < output_top {
        break;
      }
      if let Some((text, color)) = wrapped_lines.get(i) {
        ui.draw_text(text, x + padding, text_y, *color);
        text_y -= line_height;
      }
    }

    // Draw scroll indicator if not at bottom (now shown in scrollbar area)
    if self.scroll_offset > 0 && !has_scrollbar {
      // Only show text indicator if no scrollbar (few lines but scrolled)
      let indicator = format!("[{} more lines below]", self.scroll_offset);
      let (indicator_width, _) = ui.text_size(&indicator);
      let indicator_x = x + (self.width - indicator_width as f32) / 2.0;
      ui.draw_text(&indicator, indicator_x, output_bottom - line_height, colors::SYSTEM);
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  // ============================================================================
  // ConsoleState Basic Operations
  // ============================================================================

  #[test]
  fn test_console_new_initial_state() {
    let console = ConsoleState::new();
    assert!(!console.visible);
    assert!(console.focused);
    assert!(console.input_buffer.is_empty());
    assert_eq!(console.cursor_pos, 0);
    assert_eq!(console.scroll_offset, 0);
    // Welcome message adds lines
    assert!(!console.output_lines.is_empty());
  }

  #[test]
  fn test_console_toggle() {
    let mut console = ConsoleState::new();
    assert!(!console.visible);

    console.toggle();
    assert!(console.visible);
    assert!(console.focused);

    console.toggle();
    assert!(!console.visible);
  }

  #[test]
  fn test_console_print() {
    let mut console = ConsoleState::new();
    let initial_lines = console.output_lines.len();

    console.print("Hello, World!");
    assert_eq!(console.output_lines.len(), initial_lines + 1);

    let last_line = console.output_lines.back().unwrap();
    assert_eq!(last_line.text, "Hello, World!");
    assert_eq!(last_line.color, colors::TEXT);
  }

  #[test]
  fn test_console_print_colored() {
    let mut console = ConsoleState::new();
    let custom_color = Color::rgb(255, 128, 0);

    console.print_colored("Orange text", custom_color);

    let last_line = console.output_lines.back().unwrap();
    assert_eq!(last_line.text, "Orange text");
    assert_eq!(last_line.color, custom_color);
  }

  #[test]
  fn test_console_print_command() {
    let mut console = ConsoleState::new();
    console.print_command("> test command");

    let last_line = console.output_lines.back().unwrap();
    assert_eq!(last_line.color, colors::COMMAND);
  }

  #[test]
  fn test_console_print_error() {
    let mut console = ConsoleState::new();
    console.print_error("Something went wrong");

    let last_line = console.output_lines.back().unwrap();
    assert_eq!(last_line.color, colors::ERROR);
  }

  #[test]
  fn test_console_print_system() {
    let mut console = ConsoleState::new();
    console.print_system("System message");

    let last_line = console.output_lines.back().unwrap();
    assert_eq!(last_line.color, colors::SYSTEM);
  }

  #[test]
  fn test_console_clear() {
    let mut console = ConsoleState::new();
    console.print("Line 1");
    console.print("Line 2");
    console.scroll_offset = 5;

    console.clear();

    assert!(console.output_lines.is_empty());
    assert_eq!(console.scroll_offset, 0);
  }

  #[test]
  fn test_console_max_output_lines() {
    let mut console = ConsoleState::new();
    console.clear();

    // Add more than MAX_OUTPUT_LINES
    for i in 0..MAX_OUTPUT_LINES + 100 {
      console.print(format!("Line {}", i));
    }

    assert_eq!(console.output_lines.len(), MAX_OUTPUT_LINES);
    // Oldest lines should be removed, newest kept
    let first_line = console.output_lines.front().unwrap();
    assert!(first_line.text.starts_with("Line 100")); // First 100 lines removed
  }

  #[test]
  fn test_console_update_increments_time() {
    let mut console = ConsoleState::new();
    let initial_time = console.total_time;

    console.update(0.016); // ~60fps frame
    assert!(console.total_time > initial_time);
    assert!((console.total_time - initial_time - 0.016).abs() < 0.0001);
  }

  #[test]
  fn test_console_cursor_blink_timer_wraps() {
    let mut console = ConsoleState::new();
    console.visible = true;
    console.cursor_blink_timer = 0.9;

    console.update(0.2); // Push past 1.0

    assert!(console.cursor_blink_timer < 0.2); // Should wrap around
  }

  // ============================================================================
  // Geometry and Rectangle Calculations
  // ============================================================================

  #[test]
  fn test_window_rect() {
    let mut console = ConsoleState::new();
    console.x = 100.0;
    console.y = 50.0;
    console.width = 400.0;
    console.height = 300.0;

    let rect = console.window_rect();
    assert_eq!(rect.x, 100.0);
    assert_eq!(rect.y, 50.0);
    assert_eq!(rect.width, 400.0);
    assert_eq!(rect.height, 300.0);
  }

  #[test]
  fn test_title_bar_rect() {
    let mut console = ConsoleState::new();
    console.x = 100.0;
    console.y = 50.0;
    console.width = 400.0;

    let rect = console.title_bar_rect();
    assert_eq!(rect.x, 100.0);
    assert_eq!(rect.y, 50.0);
    assert_eq!(rect.width, 400.0);
    assert_eq!(rect.height, TITLE_BAR_HEIGHT);
  }

  #[test]
  fn test_close_button_rect_position() {
    let mut console = ConsoleState::new();
    console.x = 100.0;
    console.y = 50.0;
    console.width = 400.0;

    let rect = console.close_button_rect();

    // Close button should be in top-right of title bar
    let expected_right = 100.0 + 400.0;
    let button_right = rect.x + rect.width + (TITLE_BAR_HEIGHT - CLOSE_BUTTON_SIZE) / 2.0;
    assert!((expected_right - button_right).abs() < 0.01);
    assert_eq!(rect.width, CLOSE_BUTTON_SIZE);
    assert_eq!(rect.height, CLOSE_BUTTON_SIZE);
  }

  #[test]
  fn test_resize_handle_rect_position() {
    let mut console = ConsoleState::new();
    console.x = 100.0;
    console.y = 50.0;
    console.width = 400.0;
    console.height = 300.0;

    let rect = console.resize_handle_rect();

    // Resize handle should be in bottom-right corner
    assert_eq!(rect.x, 100.0 + 400.0 - RESIZE_HANDLE_SIZE);
    assert_eq!(rect.y, 50.0 + 300.0 - RESIZE_HANDLE_SIZE);
    assert_eq!(rect.width, RESIZE_HANDLE_SIZE);
    assert_eq!(rect.height, RESIZE_HANDLE_SIZE);
  }

  #[test]
  fn test_output_area_calculation() {
    let mut console = ConsoleState::new();
    console.x = 0.0;
    console.y = 0.0;
    console.width = 600.0;
    console.height = 400.0;

    let (output_top, output_bottom, output_height, padding) = console.output_area();

    // Output area should be between title bar and input line
    assert_eq!(padding, 8.0);
    assert_eq!(output_top, TITLE_BAR_HEIGHT + padding);
    assert_eq!(output_bottom, 400.0 - INPUT_HEIGHT - padding);
    assert_eq!(output_height, output_bottom - output_top);
    assert!(output_height > 0.0);
  }

  #[test]
  fn test_scrollbar_track_rect() {
    let mut console = ConsoleState::new();
    console.x = 0.0;
    console.y = 0.0;
    console.width = 600.0;
    console.height = 400.0;

    let track = console.scrollbar_track_rect();
    let (output_top, _, output_height, _) = console.output_area();

    // Track should be within output area
    assert_eq!(track.y, output_top);
    assert_eq!(track.height, output_height);
    assert_eq!(track.width, SCROLLBAR_WIDTH);
    // Track should be on the right side
    assert!((track.x - (600.0 - SCROLLBAR_WIDTH - 4.0)).abs() < 0.01);
  }

  #[test]
  fn test_scrollbar_thumb_rect_no_scrollbar_needed() {
    let mut console = ConsoleState::new();
    console.clear();
    console.x = 0.0;
    console.y = 0.0;
    console.width = 600.0;
    console.height = 400.0;

    // Add fewer lines than visible
    console.print("Line 1");
    console.print("Line 2");

    // With few lines, no scrollbar needed
    let thumb = console.scrollbar_thumb_rect(20, 2);
    assert!(thumb.is_none());
  }

  #[test]
  fn test_scrollbar_thumb_rect_with_scrollbar() {
    let mut console = ConsoleState::new();
    console.x = 0.0;
    console.y = 0.0;
    console.width = 600.0;
    console.height = 400.0;

    // Scrollbar needed when total > visible
    let thumb = console.scrollbar_thumb_rect(10, 100);
    assert!(thumb.is_some());

    let rect = thumb.unwrap();
    assert!(rect.height >= SCROLLBAR_MIN_THUMB);
    assert_eq!(rect.width, SCROLLBAR_WIDTH);
  }

  #[test]
  fn test_scrollbar_thumb_position_at_bottom() {
    let mut console = ConsoleState::new();
    console.x = 0.0;
    console.y = 0.0;
    console.width = 600.0;
    console.height = 400.0;
    console.scroll_offset = 0; // At bottom

    let (output_top, _, output_height, _) = console.output_area();
    let thumb = console.scrollbar_thumb_rect(10, 100).unwrap();

    // At scroll_offset=0, thumb should be at bottom
    let thumb_bottom = thumb.y + thumb.height;
    let track_bottom = output_top + output_height;
    assert!((thumb_bottom - track_bottom).abs() < 1.0);
  }

  #[test]
  fn test_scrollbar_thumb_position_at_top() {
    let mut console = ConsoleState::new();
    console.x = 0.0;
    console.y = 0.0;
    console.width = 600.0;
    console.height = 400.0;
    console.scroll_offset = 90; // At top (max for 100 total, 10 visible)

    let (output_top, _, _, _) = console.output_area();
    let thumb = console.scrollbar_thumb_rect(10, 100).unwrap();

    // At max scroll_offset, thumb should be at top
    assert!((thumb.y - output_top).abs() < 1.0);
  }

  #[test]
  fn test_max_scroll_offset() {
    let console = ConsoleState::new();

    // 100 total lines, 20 visible -> max scroll is 80
    assert_eq!(console.max_scroll_offset(20, 100), 80);

    // 10 total lines, 20 visible -> max scroll is 0
    assert_eq!(console.max_scroll_offset(20, 10), 0);

    // Equal -> max scroll is 0
    assert_eq!(console.max_scroll_offset(20, 20), 0);
  }

  #[test]
  fn test_max_text_width_with_scrollbar() {
    let mut console = ConsoleState::new();
    console.width = 600.0;

    let width_with_scrollbar = console.max_text_width(true);
    let width_without_scrollbar = console.max_text_width(false);

    // Width should be smaller when scrollbar is present
    assert!(width_with_scrollbar < width_without_scrollbar);
    assert!(width_with_scrollbar > 0.0);
    assert!(width_without_scrollbar > 0.0);
  }

  // ============================================================================
  // Drag State
  // ============================================================================

  #[test]
  fn test_drag_state_equality() {
    assert_eq!(DragState::None, DragState::None);

    let drag1 = DragState::Dragging { offset_x: 10.0, offset_y: 20.0 };
    let drag2 = DragState::Dragging { offset_x: 10.0, offset_y: 20.0 };
    assert_eq!(drag1, drag2);

    let resize1 = DragState::Resizing {
      start_w: 100.0,
      start_h: 200.0,
      start_mouse_x: 50.0,
      start_mouse_y: 60.0,
    };
    let resize2 = DragState::Resizing {
      start_w: 100.0,
      start_h: 200.0,
      start_mouse_x: 50.0,
      start_mouse_y: 60.0,
    };
    assert_eq!(resize1, resize2);

    let scroll1 = DragState::ScrollbarDragging {
      start_scroll_offset: 10,
      start_mouse_y: 100.0,
    };
    let scroll2 = DragState::ScrollbarDragging {
      start_scroll_offset: 10,
      start_mouse_y: 100.0,
    };
    assert_eq!(scroll1, scroll2);

    assert_ne!(DragState::None, drag1);
  }

  // ============================================================================
  // ConsoleLine
  // ============================================================================

  #[test]
  fn test_console_line_creation() {
    let line = ConsoleLine::new("Test text", Color::WHITE, 1.5);

    assert_eq!(line.text, "Test text");
    assert_eq!(line.color, Color::WHITE);
    assert_eq!(line.timestamp, 1.5);
  }

  #[test]
  fn test_console_line_into_string() {
    let line = ConsoleLine::new(String::from("Owned string"), Color::WHITE, 0.0);
    assert_eq!(line.text, "Owned string");
  }

  // ============================================================================
  // Cursor Style
  // ============================================================================

  #[test]
  fn test_desired_cursor_when_not_visible() {
    let console = ConsoleState::new();
    // Console not visible, should return None
    assert!(console.desired_cursor(100.0, 100.0).is_none());
  }

  #[test]
  fn test_desired_cursor_during_resize() {
    let mut console = ConsoleState::new();
    console.visible = true;
    console.drag_state = DragState::Resizing {
      start_w: 600.0,
      start_h: 400.0,
      start_mouse_x: 100.0,
      start_mouse_y: 100.0,
    };

    let cursor = console.desired_cursor(0.0, 0.0);
    assert_eq!(cursor, Some(CursorStyle::ResizeNwSe));
  }

  #[test]
  fn test_desired_cursor_during_drag() {
    let mut console = ConsoleState::new();
    console.visible = true;
    console.drag_state = DragState::Dragging { offset_x: 10.0, offset_y: 10.0 };

    let cursor = console.desired_cursor(0.0, 0.0);
    assert_eq!(cursor, Some(CursorStyle::Move));
  }

  #[test]
  fn test_desired_cursor_over_resize_handle() {
    let mut console = ConsoleState::new();
    console.visible = true;
    console.x = 100.0;
    console.y = 100.0;
    console.width = 600.0;
    console.height = 400.0;

    // Mouse over resize handle (bottom-right corner)
    let mouse_x = 100.0 + 600.0 - RESIZE_HANDLE_SIZE / 2.0;
    let mouse_y = 100.0 + 400.0 - RESIZE_HANDLE_SIZE / 2.0;

    let cursor = console.desired_cursor(mouse_x, mouse_y);
    assert_eq!(cursor, Some(CursorStyle::ResizeNwSe));
  }

  // ============================================================================
  // Focus State
  // ============================================================================

  #[test]
  fn test_is_focused() {
    let mut console = ConsoleState::new();
    assert!(console.is_focused());

    console.focused = false;
    assert!(!console.is_focused());
  }

  // ============================================================================
  // Print resets scroll offset
  // ============================================================================

  #[test]
  fn test_print_resets_scroll_offset() {
    let mut console = ConsoleState::new();
    console.scroll_offset = 10;

    console.print("New message");

    // Adding new content should reset scroll to bottom
    assert_eq!(console.scroll_offset, 0);
  }

  // ============================================================================
  // Color Constants
  // ============================================================================

  #[test]
  fn test_color_constants_are_valid() {
    // Just verify the color constants are accessible and reasonable
    assert_eq!(colors::TEXT, Color::WHITE);
    // Use const blocks for compile-time assertions on constants
    const _: () = assert!(colors::BACKGROUND_FOCUSED.a > colors::BACKGROUND_UNFOCUSED.a);
    const _: () = assert!(colors::ERROR.r > colors::ERROR.g); // Red > green for error color
    const _: () = assert!(colors::COMMAND.b > colors::COMMAND.r); // Blue > red for command color
  }

  // ============================================================================
  // Constants
  // ============================================================================

  #[test]
  fn test_constants_are_reasonable() {
    // Use const blocks for compile-time assertions on constants
    const _: () = assert!(MAX_OUTPUT_LINES > 100);
    const _: () = assert!(MAX_COMMAND_HISTORY > 10);
    const _: () = assert!(MIN_WIDTH > 0.0);
    const _: () = assert!(MIN_HEIGHT > 0.0);
    const _: () = assert!(DEFAULT_WIDTH >= MIN_WIDTH);
    const _: () = assert!(DEFAULT_HEIGHT >= MIN_HEIGHT);
    const _: () = assert!(TITLE_BAR_HEIGHT > 0.0);
    const _: () = assert!(INPUT_HEIGHT > 0.0);
    const _: () = assert!(SCROLLBAR_WIDTH > 0.0);
    const _: () = assert!(SCROLLBAR_MIN_THUMB > 0.0);
  }
}
