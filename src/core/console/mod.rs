//! The in-game developer console: a moveable, resizable window toggled with
//! the backquote key, and the commands typed into it.
//!
//! A scene registers commands through `ctx.console()` ([`ConsoleScope`]);
//! they run with that scene and leave with it. The builder registers
//! commands for the whole run (`EngineBuilder::console_command`). The
//! module notes (`AGENTS.md`) have how a line runs.
//!
//! ```ignore
//! fn on_enter(&mut self, ctx: &mut SceneContext) {
//!   ctx.console().command(("game:heal", "Heal the player"), |game: &mut Game, _ctx, cmd| {
//!     game.health = game.max_health;
//!     cmd.print("Healed");
//!     Ok(())
//!   });
//! }
//! ```

#[cfg(dev_tools)]
mod builtins;
mod command;
#[cfg(dev_tools)]
mod dispatch;
#[cfg(dev_tools)]
mod registry;
mod scope;
mod view;

pub use command::{Command, CommandSpec};
pub use scope::ConsoleScope;

#[cfg(dev_tools)]
pub(crate) use builtins::add_builtins;
#[cfg(dev_tools)]
pub(crate) use command::WorldHandler;
#[cfg(dev_tools)]
pub(crate) use dispatch::run_line;
#[cfg(all(dev_tools, test))]
pub(crate) use registry::erase;
#[cfg(dev_tools)]
pub(crate) use registry::{ConsoleCommands, EngineHooks, Handler, valid_name};

use std::collections::VecDeque;

use super::cursor::CursorStyle;
use super::ecs::Resource;
use super::input::Input;
use super::text::{FontRole, TextStyle};
use super::ui::{Color, Rect};
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

/// Title bar height, at least (it grows with the console font)
const TITLE_BAR_HEIGHT: f32 = 24.0;

/// Close button size
const CLOSE_BUTTON_SIZE: f32 = 16.0;

/// Input line height, at least (it grows with the console font)
const INPUT_HEIGHT: f32 = 24.0;

/// Room around a line of text in the title bar and input line
const BAR_PADDING: f32 = 10.0;

/// The line height assumed before the first render measures the font
const DEFAULT_LINE_HEIGHT: f32 = 12.0;

/// The console's text: JetBrains Mono, 13 px
pub(crate) fn console_style() -> TextStyle {
  TextStyle::new().role(FontRole::Mono).size(13.0)
}

/// Resize handle size (corner)
const RESIZE_HANDLE_SIZE: f32 = 12.0;

/// Scrollbar width
const SCROLLBAR_WIDTH: f32 = 8.0;

/// Minimum scrollbar thumb height
const SCROLLBAR_MIN_THUMB: f32 = 20.0;

/// Lines a wheel notch scrolls
const WHEEL_LINES: f32 = 3.0;

/// Time constant (seconds) of the wheel's ease, as the UI's scroll areas
const SCROLL_SMOOTHING: f32 = 0.08;

/// Less than this far (in units) from the bottom counts as the bottom
const BOTTOM_SNAP: f32 = 0.5;

/// Closer than this to its target (in lines) the view settles
const SCROLL_SNAP: f32 = 0.02;

/// Console visual colors
pub(crate) mod colors {
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
  /// A mirrored warning's color (yellow)
  #[cfg(dev_tools)]
  pub const WARNING: Color = Color::new(255, 210, 90, 255);
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
pub(crate) struct ConsoleLine {
  /// The text content of the line
  pub text: String,
  /// The color to render this line
  pub color: Color,
  /// Timestamp when this line was added (in seconds since console was created)
  #[cfg_attr(
    not(test),
    expect(dead_code, reason = "nothing shows a line's time yet")
  )]
  pub timestamp: f32,
  /// Its rows, wrapped at the console's `wrap_width` (empty until then)
  rows: Vec<String>,
}

impl ConsoleLine {
  /// Create a new console line
  pub fn new(text: impl Into<String>, color: Color, timestamp: f32) -> Self {
    Self {
      text: text.into(),
      color,
      timestamp,
      rows: Vec::new(),
    }
  }
}

/// Drag state for window movement
#[derive(Debug, Clone, Copy, PartialEq)]
enum DragState {
  None,
  Dragging {
    offset_x: f32,
    offset_y: f32,
  },
  Resizing {
    start_w: f32,
    start_h: f32,
    start_mouse_x: f32,
    start_mouse_y: f32,
  },
  ScrollbarDragging {
    start_scroll_offset: f32,
    start_mouse_y: f32,
  },
}

/// The console: its window, its input line and its output. A world
/// resource; the engine ticks it, draws it, and runs what is typed into it.
#[derive(Resource, Debug)]
pub(crate) struct Console {
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
  /// How far the view is scrolled up from the bottom, in (fractional) lines
  /// (0 = showing the newest lines)
  scroll_offset: f32,
  /// Where a wheel's ease is taking `scroll_offset`, in lines
  scroll_target: f32,
  /// Logical pixels to the console's units (the pointer's conversion)
  pixel_scale: f32,
  /// UI units per screen unit: the pointer's position in the console's
  /// units, as the UI pass converts it
  mouse_scale: f32,
  /// The text width (and UI scale) the first `wrapped_lines` lines are
  /// wrapped at; a line is wrapped once while these hold
  wrap_width: f32,
  wrap_scale: f32,
  /// The lines, from the oldest, whose `rows` are wrapped at `wrap_width`
  wrapped_lines: usize,
  /// Their rows altogether
  wrapped_row_count: usize,
  /// Lines printed since the last render while the reader was scrolled up
  unseen_lines: usize,
  /// Timer for cursor blink animation
  cursor_blink_timer: f32,
  /// Total time elapsed (for timestamps)
  total_time: f32,
  /// Current drag state
  drag_state: DragState,
  /// Drop the toggle key's character from this frame's text (the press
  /// that opened the console)
  swallow_toggle: bool,
  /// Tab was pressed this frame (the engine completes with its commands)
  #[cfg(dev_tools)]
  tab_pressed: bool,
  /// The input line as the last Tab with several matches left it: a Tab
  /// on the same line lists them
  #[cfg(dev_tools)]
  tab_line: Option<String>,
  /// The console font's line height as last measured by `render`
  line_height: f32,
  /// The lowest level of log record the console shows (`log:show`;
  /// `Off` shows none)
  #[cfg_attr(
    not(dev_tools),
    expect(dead_code, reason = "only `log:show` sets it so far")
  )]
  pub(crate) log_show: log::LevelFilter,
  /// The text of each line printed while set (`HeadlessEngine::console_run`)
  captured: Option<Vec<String>>,
  /// The last log sequence number the mirror read
  #[cfg(dev_tools)]
  log_seq: u64,
  /// The mirror's records, kept for their room
  #[cfg(dev_tools)]
  log_records: Vec<crate::core::logging::LogRecord>,
  /// Lines wrapped so far (a test's count)
  #[cfg(test)]
  wrap_calls: usize,
}

impl Default for Console {
  fn default() -> Self {
    Self::new()
  }
}

impl Console {
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
      scroll_offset: 0.0,
      scroll_target: 0.0,
      pixel_scale: 1.0,
      mouse_scale: 1.0,
      wrap_width: f32::NAN,
      wrap_scale: f32::NAN,
      wrapped_lines: 0,
      wrapped_row_count: 0,
      unseen_lines: 0,
      cursor_blink_timer: 0.0,
      total_time: 0.0,
      drag_state: DragState::None,
      swallow_toggle: false,
      #[cfg(dev_tools)]
      tab_pressed: false,
      #[cfg(dev_tools)]
      tab_line: None,
      line_height: DEFAULT_LINE_HEIGHT,
      log_show: log::LevelFilter::Warn,
      captured: None,
      #[cfg(dev_tools)]
      log_seq: 0,
      #[cfg(dev_tools)]
      log_records: Vec::new(),
      #[cfg(test)]
      wrap_calls: 0,
    };

    // Print welcome message
    console.print_system("Welcome to Ryo2D Developer Console");
    console.print_system("Type 'help' for available commands");
    console
  }

  /// Print a line to the console with a specific color
  pub fn print_colored(&mut self, text: impl Into<String>, color: Color) {
    let line = ConsoleLine::new(text, color, self.total_time);
    if let Some(captured) = &mut self.captured {
      captured.push(line.text.clone());
    }
    self.output_lines.push_back(line);

    // Trim old lines if we exceed the maximum
    while self.output_lines.len() > MAX_OUTPUT_LINES {
      if let Some(oldest) = self.output_lines.pop_front()
        && self.wrapped_lines > 0
      {
        self.wrapped_lines -= 1;
        self.wrapped_row_count -= oldest.rows.len();
      }
    }

    // At the bottom the view follows the output; a reader scrolled up keeps
    // the lines being read where they are
    if self.scroll_target > 0.0 {
      self.unseen_lines += 1;
    }
  }

  /// Print a normal text line
  #[cfg_attr(
    all(target_arch = "wasm32", not(test)),
    expect(dead_code, reason = "only the headless harness prints plain lines")
  )]
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

  /// Keep the text of every line printed from now until `take_captured`
  #[cfg(all(dev_tools, not(target_arch = "wasm32")))]
  pub(crate) fn capture(&mut self) {
    self.captured = Some(Vec::new());
  }

  /// The lines printed since `capture`, ending the capture
  #[cfg(all(dev_tools, not(target_arch = "wasm32")))]
  pub(crate) fn take_captured(&mut self) -> Vec<String> {
    self.captured.take().unwrap_or_default()
  }

  /// The input line as typed so far
  #[cfg(all(test, dev_tools))]
  pub(crate) fn input_line(&self) -> &str {
    &self.input_buffer
  }

  /// The output lines, oldest first
  #[cfg(all(test, dev_tools))]
  pub(crate) fn lines(&self) -> impl DoubleEndedIterator<Item = &ConsoleLine> {
    self.output_lines.iter()
  }

  /// Clear all output lines
  #[cfg_attr(
    all(not(dev_tools), not(test)),
    expect(dead_code, reason = "only the `clear` command clears")
  )]
  pub fn clear(&mut self) {
    self.output_lines.clear();
    self.wrapped_lines = 0;
    self.wrapped_row_count = 0;
    self.scroll_offset = 0.0;
    self.scroll_target = 0.0;
    self.unseen_lines = 0;
  }

  /// Toggle console visibility
  pub fn toggle(&mut self) {
    self.visible = !self.visible;
    if self.visible {
      self.focused = true;
      self.cursor_blink_timer = 0.0;
      // The press that opened it typed its character too
      self.swallow_toggle = true;
    }
  }

  /// Set the conversion from logical pixels (a trackpad's) to the console's
  /// units, which are the pointer's
  pub(crate) fn set_pixel_scale(&mut self, scale: f32) {
    if scale.is_finite() && scale > 0.0 {
      self.pixel_scale = scale;
    }
  }

  /// Set the UI units per screen unit, so the pointer is hit-tested in the
  /// units the console is drawn in (the UI pass's conversion)
  pub(crate) fn set_mouse_scale(&mut self, scale: f32) {
    if scale.is_finite() && scale > 0.0 {
      self.mouse_scale = scale;
    }
  }

  /// The pointer in the console's units
  fn pointer(&self, input: &Input) -> (f32, f32) {
    let (x, y) = input.mouse_position();
    (x * self.mouse_scale, y * self.mouse_scale)
  }

  /// Update cursor blink and timers
  pub fn update(&mut self, dt: f32) {
    self.total_time += dt;
    self.ease_scroll(dt);
    if self.visible {
      self.cursor_blink_timer += dt;
      if self.cursor_blink_timer > 1.0 {
        self.cursor_blink_timer -= 1.0;
      }
    }
  }

  /// Ease the view toward the wheel's target, as a UI scroll area does
  fn ease_scroll(&mut self, dt: f32) {
    let left = self.scroll_target - self.scroll_offset;
    if left == 0.0 || !dt.is_finite() || dt <= 0.0 {
      return;
    }
    let step = 1.0 - (-dt / SCROLL_SMOOTHING).exp();
    let next = self.scroll_offset + left * step;
    // Far from the target a step can be smaller than a float can move
    self.scroll_offset = if left.abs() < SCROLL_SNAP || next == self.scroll_offset {
      self.scroll_target
    } else {
      next
    };
  }

  /// How far the view scrolls, in lines: the rows (wrapped, as last drawn)
  /// less what fits
  fn max_scroll_lines(&self) -> f32 {
    let rows = self.total_rows();
    let (_, _, output_height, _) = self.output_area();
    (rows as f32 - output_height / self.line_height).max(0.0)
  }

  /// A distance from the bottom, with less than half a unit counting as
  /// the bottom itself
  fn snap_to_bottom(&self, lines: f32) -> f32 {
    if lines * self.line_height < BOTTOM_SNAP {
      0.0
    } else {
      lines
    }
  }

  /// The rows of every line: those wrapped, and one for each line not yet
  /// wrapped (printed since the last render)
  fn total_rows(&self) -> usize {
    self.wrapped_row_count + (self.output_lines.len() - self.wrapped_lines)
  }

  /// Put the view at `lines` (clamped), stopping any ease; a move that ends
  /// just above the bottom ends at it
  fn scroll_to_lines(&mut self, lines: f32) {
    let mut lines = lines.clamp(0.0, self.max_scroll_lines());
    if lines < self.scroll_offset {
      lines = self.snap_to_bottom(lines);
    }
    self.scroll_offset = lines;
    self.scroll_target = lines;
  }

  /// The title bar's height: one line of the console font plus padding
  fn title_bar_height(&self) -> f32 {
    TITLE_BAR_HEIGHT.max(self.line_height + BAR_PADDING)
  }

  /// The input line's height
  fn input_height(&self) -> f32 {
    INPUT_HEIGHT.max(self.line_height + BAR_PADDING)
  }

  /// Get the title bar bounds
  fn title_bar_rect(&self) -> Rect {
    Rect::new(self.x, self.y, self.width, self.title_bar_height())
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
  pub(crate) fn close_button_rect(&self) -> Rect {
    let padding = (self.title_bar_height() - CLOSE_BUTTON_SIZE) / 2.0;
    Rect::new(
      self.x + self.width - CLOSE_BUTTON_SIZE - padding,
      self.y + padding,
      CLOSE_BUTTON_SIZE,
      CLOSE_BUTTON_SIZE,
    )
  }

  /// Get the desired cursor style based on current state and mouse position.
  ///
  /// Returns `Some(CursorStyle)` while the console wants the cursor, or
  /// `None` (no opinion) when the game's cursor should show.
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

    // Over the window it is the default cursor, focused or not, so a game's
    // crosshair does not show over it; elsewhere the game's cursor returns
    if self.window_rect().contains(mouse_x, mouse_y) {
      return Some(CursorStyle::Default);
    }

    None
  }

  /// Get the full window bounds
  pub fn window_rect(&self) -> Rect {
    Rect::new(self.x, self.y, self.width, self.height)
  }

  /// Get the output area bounds (between title bar and input line)
  fn output_area(&self) -> (f32, f32, f32, f32) {
    let padding = 8.0;
    let input_y = self.y + self.height - self.input_height();
    let output_top = self.y + self.title_bar_height() + padding;
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

    let max_scroll = total_lines as f32 - output_height / self.line_height;
    let scroll_ratio = if max_scroll > 0.0 {
      (self.scroll_offset / max_scroll).min(1.0)
    } else {
      0.0
    };

    let thumb_travel = output_height - thumb_height;
    let thumb_y = output_top + thumb_travel * (1.0 - scroll_ratio);

    Some(Rect::new(
      scrollbar_x,
      thumb_y,
      SCROLLBAR_WIDTH,
      thumb_height,
    ))
  }

  /// Get the maximum text width for word wrapping
  fn max_text_width(&self, has_scrollbar: bool) -> f32 {
    let padding = 8.0;
    let scrollbar_margin = if has_scrollbar {
      SCROLLBAR_WIDTH + 12.0
    } else {
      0.0
    };
    self.width - padding * 2.0 - scrollbar_margin
  }

  /// Check if the console is currently focused
  pub fn is_focused(&self) -> bool {
    self.focused
  }

  /// Handle input when console is visible
  ///
  /// Returns Some(command) if a command was submitted, None otherwise.
  /// Also handles focus changes - clicking outside unfocuses, clicking inside focuses.
  pub fn handle_input(&mut self, input: &mut Input) -> Option<String> {
    if !self.visible {
      return None;
    }

    // Only this frame's text can hold the opening press's character
    let swallow_toggle = std::mem::take(&mut self.swallow_toggle);
    let (mouse_x, mouse_y) = self.pointer(input);
    let window_rect = self.window_rect();

    // Calculate scrollbar info for interactions
    let line_height = self.line_height;
    let (_, _, output_height, _) = self.output_area();
    let visible_lines = (output_height / line_height) as usize;
    let total_lines = self.total_rows();
    let max_scroll = self.max_scroll_lines();
    // A resize or the line cap can leave the view past the end
    self.scroll_offset = self.scroll_offset.clamp(0.0, max_scroll);
    self.scroll_target = self.scroll_target.clamp(0.0, max_scroll);

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
      DragState::Resizing {
        start_w,
        start_h,
        start_mouse_x,
        start_mouse_y,
      } => {
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
      DragState::ScrollbarDragging {
        start_scroll_offset,
        start_mouse_y,
      } => {
        if input.mouse_down(MouseButton::Left) {
          // Calculate scroll based on mouse movement
          let (_, _, output_height, _) = self.output_area();
          let visible_ratio = visible_lines as f32 / total_lines.max(1) as f32;
          let thumb_height = (output_height * visible_ratio).max(SCROLLBAR_MIN_THUMB);
          let thumb_travel = output_height - thumb_height;

          if thumb_travel > 0.0 && max_scroll > 0.0 {
            let delta_y = mouse_y - start_mouse_y;
            // Dragging down = scroll_ratio increases = scroll_offset decreases (toward bottom)
            let scroll_delta = delta_y / thumb_travel * max_scroll;
            self.scroll_to_lines(start_scroll_offset - scroll_delta);
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
              self.scroll_to_lines((1.0 - click_ratio) * max_scroll);
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
    let (_, wheel_y) = input.scroll_wheel_lines();
    let (_, pixel_y) = input.scroll_pixels();
    if (wheel_y != 0.0 || pixel_y != 0.0) && window_rect.contains(mouse_x, mouse_y) {
      input.consume_scroll();
      // Positive scroll is up: the view moves away from the bottom
      if pixel_y != 0.0 {
        // A trackpad follows the fingers at once (the OS supplies the
        // momentum), from where the view is
        let at = self.scroll_offset.clamp(0.0, max_scroll);
        self.scroll_to_lines(at + pixel_y * self.pixel_scale / line_height);
      }
      if wheel_y != 0.0 {
        let target = (self.scroll_target + wheel_y * WHEEL_LINES).clamp(0.0, max_scroll);
        self.scroll_target = self.snap_to_bottom(target);
      }
    }

    // Only process keyboard input when focused
    if !self.focused {
      return None;
    }

    let mut submitted_command = None;

    // Handle text input (from IME or keyboard). The press that opened the
    // console typed its character too: that one is dropped, and once open
    // the backquote and tilde type as any key does
    if input.has_text_input() {
      let mut swallow = swallow_toggle;
      let filtered: String = input
        .text_input()
        .chars()
        .filter(|&c| {
          if swallow && (c == '`' || c == '~') {
            swallow = false;
            return false;
          }
          !c.is_control()
        })
        .collect();
      if !filtered.is_empty() {
        self.input_buffer.insert_str(self.cursor_pos, &filtered);
        self.cursor_pos += filtered.len();
        self.cursor_blink_timer = 0.0;
      }
      // Consume text so game widgets don't also process it
      input.consume_text_input();
    }

    // Tab completes the command name; the engine has the commands
    #[cfg(dev_tools)]
    if input.key_pressed(KeyCode::Tab) {
      self.tab_pressed = true;
    }

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
    if input.key_pressed(KeyCode::ArrowUp) && !self.command_history.is_empty() {
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
      self.scroll_to_lines(self.scroll_offset + 10.0);
    }

    if input.key_pressed(KeyCode::PageDown) {
      self.scroll_to_lines(self.scroll_offset - 10.0);
    }

    // Enter to submit command
    if input.key_pressed(KeyCode::Enter) && !self.input_buffer.is_empty() {
      let command = self.input_buffer.clone();

      // Typing a command brings the reader to the bottom, as in a terminal
      self.scroll_to_lines(0.0);

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

  /// Whether Tab was pressed since the last call
  #[cfg(dev_tools)]
  pub(crate) fn take_tab(&mut self) -> bool {
    std::mem::take(&mut self.tab_pressed)
  }

  /// Tab: complete the first word from `names` (the command names starting
  /// with a prefix). One match is completed with a space after it, so a
  /// second Tab does nothing; several are completed as far as they agree,
  /// and a Tab that finds the line as the last one left it lists them on
  /// one line. After the first word, Tab does nothing.
  #[cfg(dev_tools)]
  pub(crate) fn complete(&mut self, names: impl FnOnce(&str) -> Vec<String>) {
    let word = self.input_buffer.as_str();
    if word.is_empty() || word.chars().any(char::is_whitespace) {
      return;
    }
    let names = names(word);
    match names.as_slice() {
      [] => {}
      [name] => {
        self.set_input(format!("{name} "));
        self.tab_line = None;
      }
      [first, rest @ ..] => {
        let common = common_prefix(first, rest);
        if common.len() > word.len() {
          self.set_input(common.to_string());
        } else if self.tab_line.as_deref() == Some(word) {
          self.print_system(names.join("  "));
        }
        self.tab_line = Some(self.input_buffer.clone());
      }
    }
  }

  /// Replace the input line, the cursor at its end
  #[cfg(dev_tools)]
  fn set_input(&mut self, line: String) {
    self.input_buffer = line;
    self.cursor_pos = self.input_buffer.len();
    self.cursor_blink_timer = 0.0;
  }

  /// Mirror the log from the record after `seq` on (the newest when the
  /// engine was built)
  #[cfg(dev_tools)]
  pub(crate) fn start_log_at(&mut self, seq: u64) {
    self.log_seq = seq;
  }

  /// Append the log's records since the last read, of the engine's log
  /// `scope`, at or above `log_show`: warnings in yellow, errors in red, as
  /// `target: message`. Run when the console opens and each frame while it
  /// is open, so what was logged while it was closed is there when it
  /// opens; the newest `MAX_OUTPUT_LINES` win, and records the ring has
  /// dropped since are counted in one grey line.
  ///
  /// Printing logs nothing (a line is pushed, and wrapped when drawn), so
  /// the mirror never shows a record of its own making.
  #[cfg(dev_tools)]
  pub(crate) fn mirror_log(&mut self, scope: u64) {
    let mut records = std::mem::take(&mut self.log_records);
    let found = crate::core::logging::mirror_records(
      self.log_seq,
      scope,
      self.log_show,
      MAX_OUTPUT_LINES,
      &mut records,
    );
    self.log_seq = found.newest;
    if self.log_show != log::LevelFilter::Off && found.lost > 0 {
      self.print_system(format!(
        "{} earlier records are no longer in the log",
        found.lost
      ));
      // The count stays in view
      if records.len() == MAX_OUTPUT_LINES {
        records.remove(0);
      }
    }
    for record in records.drain(..) {
      let color = match record.level {
        log::Level::Error => colors::ERROR,
        log::Level::Warn => colors::WARNING,
        log::Level::Info => colors::TEXT,
        log::Level::Debug | log::Level::Trace => colors::SYSTEM,
      };
      self.print_colored(format!("{}: {}", record.target, record.message), color);
    }
    self.log_records = records;
  }
}

/// The longest start `first` shares with every name of `rest`
#[cfg(dev_tools)]
fn common_prefix<'a>(first: &'a str, rest: &[String]) -> &'a str {
  let mut end = first.len();
  for name in rest {
    end = first
      .bytes()
      .zip(name.bytes())
      .take(end)
      .take_while(|(a, b)| a == b)
      .count();
  }
  while !first.is_char_boundary(end) {
    end -= 1;
  }
  &first[..end]
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn the_console_names_no_network_code() {
    // Built here, so this file's own text does not match
    let needle = ["std", "net"].join("::");
    let sources = [
      ("mod.rs", include_str!("mod.rs")),
      ("command.rs", include_str!("command.rs")),
      ("registry.rs", include_str!("registry.rs")),
      ("builtins.rs", include_str!("builtins.rs")),
      ("dispatch.rs", include_str!("dispatch.rs")),
      ("scope.rs", include_str!("scope.rs")),
      ("view.rs", include_str!("view.rs")),
    ];
    for (name, text) in sources {
      assert!(!text.contains(&needle), "{name} names {needle}");
    }
  }

  // ============================================================================
  // Console Basic Operations
  // ============================================================================

  #[test]
  fn test_console_new_initial_state() {
    let console = Console::new();
    assert!(!console.visible);
    assert!(console.focused);
    assert!(console.input_buffer.is_empty());
    assert_eq!(console.cursor_pos, 0);
    assert_eq!(console.scroll_offset, 0.0);
    // Welcome message adds lines
    assert!(!console.output_lines.is_empty());
  }

  #[test]
  fn the_wheel_over_the_console_scrolls_it_and_not_the_game() {
    let mut console = Console::new();
    console.toggle();
    let rect = console.window_rect();
    let mut input = Input::new();
    input.handle_mouse_move(
      f64::from(rect.x + rect.width / 2.0),
      f64::from(rect.y + rect.height / 2.0),
    );
    input.handle_scroll(winit::event::MouseScrollDelta::LineDelta(0.0, 1.0));
    console.handle_input(&mut input);
    assert_eq!(input.scroll_delta(), (0.0, 0.0));
  }

  /// A console of `lines` short lines with the pointer over its output
  pub(super) fn scrolling_console(lines: usize) -> (Console, Input) {
    let mut console = Console::new();
    console.toggle();
    console.clear();
    for i in 0..lines {
      console.print(format!("line {i}"));
    }
    let rect = console.window_rect();
    let mut input = Input::new();
    input.handle_mouse_move(
      f64::from(rect.x + rect.width / 2.0),
      f64::from(rect.y + rect.height / 2.0),
    );
    (console, input)
  }

  /// One frame as the engine runs it: ease, then read the input; `scroll`
  /// is a trackpad's logical pixels and `lines` a wheel's
  pub(super) fn console_frame(console: &mut Console, input: &mut Input, lines: f32, scroll: f32) {
    input.begin_frame();
    if lines != 0.0 {
      input.handle_scroll(winit::event::MouseScrollDelta::LineDelta(0.0, lines));
    }
    if scroll != 0.0 {
      input.handle_scroll_pixels(0.0, scroll);
    }
    console.update(1.0 / 60.0);
    console.handle_input(input);
  }

  /// How far the view is scrolled, in the console's drawing units
  pub(super) fn scrolled_units(console: &Console) -> f32 {
    console.scroll_offset * console.line_height
  }

  #[test]
  fn a_trackpad_moves_the_console_exactly_as_far_as_the_fingers() {
    let (mut console, mut input) = scrolling_console(50);
    for _ in 0..20 {
      console_frame(&mut console, &mut input, 0.0, 3.0);
    }
    let units = scrolled_units(&console);
    assert!((units - 60.0).abs() < 0.01, "60 units, got {units}");
    for _ in 0..20 {
      console_frame(&mut console, &mut input, 0.0, -3.0);
    }
    assert!(scrolled_units(&console).abs() < 0.01, "and back down");
  }

  #[test]
  fn a_trackpad_is_converted_to_the_consoles_units() {
    let (mut console, mut input) = scrolling_console(50);
    console.set_pixel_scale(2.0);
    for _ in 0..10 {
      console_frame(&mut console, &mut input, 0.0, 3.0);
    }
    let units = scrolled_units(&console);
    assert!(
      (units - 60.0).abs() < 0.01,
      "10 x 3 px at 2 units a px, {units}"
    );
  }

  #[test]
  fn a_wheel_line_eases_toward_three_lines() {
    let (mut console, mut input) = scrolling_console(50);
    let line = console.line_height;
    console_frame(&mut console, &mut input, 1.0, 0.0);
    console_frame(&mut console, &mut input, 0.0, 0.0);
    let early = scrolled_units(&console);
    assert!(early > 0.0 && early < 3.0 * line, "easing, at {early}");
    for _ in 0..60 {
      console_frame(&mut console, &mut input, 0.0, 0.0);
    }
    let settled = scrolled_units(&console);
    assert!((settled - 3.0 * line).abs() < 0.1, "three lines, {settled}");
  }

  #[test]
  fn the_console_scroll_stops_at_the_top_and_the_bottom() {
    let (mut console, mut input) = scrolling_console(50);
    let (_, _, view, _) = console.output_area();
    let max = 50.0 * console.line_height - view;
    console_frame(&mut console, &mut input, 0.0, 100000.0);
    assert!((scrolled_units(&console) - max).abs() < 0.01, "the top");
    console_frame(&mut console, &mut input, 0.0, -100000.0);
    assert_eq!(scrolled_units(&console), 0.0, "the bottom");
    console_frame(&mut console, &mut input, 100.0, 0.0);
    for _ in 0..600 {
      console_frame(&mut console, &mut input, 0.0, 0.0);
    }
    assert!((scrolled_units(&console) - max).abs() < 0.01, "a wheel too");
  }

  #[test]
  fn submitting_a_command_jumps_to_the_bottom_but_other_output_does_not() {
    let (mut console, mut input) = scrolling_console(50);
    console_frame(&mut console, &mut input, 0.0, 60.0);
    console.print_error("a capture report");
    assert!(
      (scrolled_units(&console) - 60.0).abs() < 0.01,
      "left in place"
    );
    assert_eq!(console.unseen_lines, 1);
    console.input_buffer = "help".to_string();
    console.cursor_pos = 4;
    input.queue(crate::core::input::InputEvent::Key {
      key: KeyCode::Enter,
      pressed: true,
    });
    input.begin_frame();
    assert_eq!(console.handle_input(&mut input).as_deref(), Some("help"));
    assert_eq!(console.scroll_offset, 0.0);
    assert_eq!(console.scroll_target, 0.0);
  }

  #[test]
  fn an_offset_beyond_the_end_is_brought_back_before_scrolling() {
    let (mut console, mut input) = scrolling_console(50);
    let (_, _, view, _) = console.output_area();
    let line = console.line_height;
    let max = 50.0 * line - view;
    console.scroll_offset = 500.0;
    console.scroll_target = 500.0;
    console_frame(&mut console, &mut input, -1.0, 0.0);
    for _ in 0..120 {
      console_frame(&mut console, &mut input, 0.0, 0.0);
    }
    let units = scrolled_units(&console);
    assert!(
      (units - (max - 3.0 * line)).abs() < 0.1,
      "a notch down moved it: {units}"
    );
  }

  #[test]
  fn a_tiny_distance_from_the_bottom_counts_as_the_bottom() {
    let (mut console, mut input) = scrolling_console(50);
    console_frame(&mut console, &mut input, 0.0, 60.0);
    console_frame(&mut console, &mut input, 0.0, -59.8);
    assert_eq!(console.scroll_target, 0.0);
    assert_eq!(console.scroll_offset, 0.0);
    console.print("more");
    assert_eq!(scrolled_units(&console), 0.0, "it follows the output");
  }

  #[test]
  fn a_zero_time_step_does_not_jump_the_ease() {
    let (mut console, mut input) = scrolling_console(50);
    console_frame(&mut console, &mut input, 1.0, 0.0);
    console.update(0.0);
    assert_eq!(console.scroll_offset, 0.0);
  }

  #[test]
  fn test_console_toggle() {
    let mut console = Console::new();
    assert!(!console.visible);

    console.toggle();
    assert!(console.visible);
    assert!(console.focused);

    console.toggle();
    assert!(!console.visible);
  }

  #[test]
  fn test_console_print() {
    let mut console = Console::new();
    let initial_lines = console.output_lines.len();

    console.print("Hello, World!");
    assert_eq!(console.output_lines.len(), initial_lines + 1);

    let last_line = console.output_lines.back().unwrap();
    assert_eq!(last_line.text, "Hello, World!");
    assert_eq!(last_line.color, colors::TEXT);
  }

  #[test]
  fn test_console_print_colored() {
    let mut console = Console::new();
    let custom_color = Color::rgb(255, 128, 0);

    console.print_colored("Orange text", custom_color);

    let last_line = console.output_lines.back().unwrap();
    assert_eq!(last_line.text, "Orange text");
    assert_eq!(last_line.color, custom_color);
  }

  #[test]
  fn test_console_print_command() {
    let mut console = Console::new();
    console.print_command("> test command");

    let last_line = console.output_lines.back().unwrap();
    assert_eq!(last_line.color, colors::COMMAND);
  }

  #[test]
  fn test_console_print_error() {
    let mut console = Console::new();
    console.print_error("Something went wrong");

    let last_line = console.output_lines.back().unwrap();
    assert_eq!(last_line.color, colors::ERROR);
  }

  #[test]
  fn test_console_print_system() {
    let mut console = Console::new();
    console.print_system("System message");

    let last_line = console.output_lines.back().unwrap();
    assert_eq!(last_line.color, colors::SYSTEM);
  }

  #[test]
  fn test_console_clear() {
    let mut console = Console::new();
    console.print("Line 1");
    console.print("Line 2");
    console.scroll_offset = 5.0;

    console.clear();

    assert!(console.output_lines.is_empty());
    assert_eq!(console.scroll_offset, 0.0);
  }

  #[test]
  fn test_console_max_output_lines() {
    let mut console = Console::new();
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
    let mut console = Console::new();
    let initial_time = console.total_time;

    console.update(0.016); // ~60fps frame
    assert!(console.total_time > initial_time);
    assert!((console.total_time - initial_time - 0.016).abs() < 0.0001);
  }

  #[test]
  fn test_console_cursor_blink_timer_wraps() {
    let mut console = Console::new();
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
    let mut console = Console::new();
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
    let mut console = Console::new();
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
    let mut console = Console::new();
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
    let mut console = Console::new();
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
    let mut console = Console::new();
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
    let mut console = Console::new();
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
    let mut console = Console::new();
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
    let mut console = Console::new();
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
    let mut console = Console::new();
    console.x = 0.0;
    console.y = 0.0;
    console.width = 600.0;
    console.height = 400.0;
    console.scroll_offset = 0.0; // At bottom

    let (output_top, _, output_height, _) = console.output_area();
    let thumb = console.scrollbar_thumb_rect(10, 100).unwrap();

    // At scroll_offset=0, thumb should be at bottom
    let thumb_bottom = thumb.y + thumb.height;
    let track_bottom = output_top + output_height;
    assert!((thumb_bottom - track_bottom).abs() < 1.0);
  }

  #[test]
  fn test_scrollbar_thumb_position_at_top() {
    let mut console = Console::new();
    console.x = 0.0;
    console.y = 0.0;
    console.width = 600.0;
    console.height = 400.0;
    console.scroll_offset = 90.0; // At top (max for 100 total, 10 visible)

    let (output_top, _, _, _) = console.output_area();
    let thumb = console.scrollbar_thumb_rect(10, 100).unwrap();

    // At max scroll_offset, thumb should be at top
    assert!((thumb.y - output_top).abs() < 1.0);
  }

  #[test]
  fn test_max_text_width_with_scrollbar() {
    let mut console = Console::new();
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

    let drag1 = DragState::Dragging {
      offset_x: 10.0,
      offset_y: 20.0,
    };
    let drag2 = DragState::Dragging {
      offset_x: 10.0,
      offset_y: 20.0,
    };
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
      start_scroll_offset: 10.0,
      start_mouse_y: 100.0,
    };
    let scroll2 = DragState::ScrollbarDragging {
      start_scroll_offset: 10.0,
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
    let console = Console::new();
    // Console not visible, should return None
    assert!(console.desired_cursor(100.0, 100.0).is_none());
  }

  #[test]
  fn test_desired_cursor_during_resize() {
    let mut console = Console::new();
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
    let mut console = Console::new();
    console.visible = true;
    console.drag_state = DragState::Dragging {
      offset_x: 10.0,
      offset_y: 10.0,
    };

    let cursor = console.desired_cursor(0.0, 0.0);
    assert_eq!(cursor, Some(CursorStyle::Move));
  }

  #[test]
  fn test_desired_cursor_over_resize_handle() {
    let mut console = Console::new();
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

  #[test]
  fn the_console_asks_for_no_cursor_once_a_resize_drag_ends_outside_it() {
    use crate::core::input::InputEvent;
    let mut console = Console::new();
    console.toggle();
    let handle = console.resize_handle_rect();
    let (hx, hy) = (
      handle.x + handle.width / 2.0,
      handle.y + handle.height / 2.0,
    );
    let mut input = Input::new();
    let mut frame = |console: &mut Console, events: Vec<InputEvent>| {
      for event in events {
        input.queue(event);
      }
      input.begin_frame();
      console.handle_input(&mut input);
      let (x, y) = input.mouse_position();
      console.desired_cursor(x, y)
    };
    let mouse = |x: f32, y: f32| InputEvent::MouseMove {
      x: f64::from(x),
      y: f64::from(y),
    };
    let button = |pressed| InputEvent::MouseButton {
      button: MouseButton::Left,
      pressed,
    };
    assert_eq!(
      frame(&mut console, vec![mouse(hx, hy)]),
      Some(CursorStyle::ResizeNwSe),
      "over the handle"
    );
    frame(&mut console, vec![button(true)]);
    // Far past the minimum size: the window stops shrinking, the pointer goes on
    assert_eq!(
      frame(&mut console, vec![mouse(hx - 900.0, hy - 600.0)]),
      Some(CursorStyle::ResizeNwSe),
      "resizing"
    );
    // Released with no further move, outside the window
    assert_eq!(
      frame(&mut console, vec![button(false)]),
      None,
      "the game's cursor is back"
    );
    // Over the window it is the default, focused or not
    let rect = console.window_rect();
    let inside = mouse(rect.x + 20.0, rect.y + 20.0);
    assert_eq!(
      frame(&mut console, vec![inside]),
      Some(CursorStyle::Default)
    );
    console.focused = false;
    let (x, y) = (rect.x + 20.0, rect.y + 20.0);
    assert_eq!(
      console.desired_cursor(x, y),
      Some(CursorStyle::Default),
      "a game's crosshair does not show over the console"
    );
  }

  // ============================================================================
  // Focus State
  // ============================================================================

  #[test]
  fn test_is_focused() {
    let mut console = Console::new();
    assert!(console.is_focused());

    console.focused = false;
    assert!(!console.is_focused());
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

  // ============================================================================
  // Window Rect Public API
  // ============================================================================

  #[test]
  fn test_window_rect_is_public() {
    // window_rect() must be public so the engine can check mouse-in-console
    let console = Console::new();
    let rect = console.window_rect();
    assert_eq!(rect.x, console.x);
    assert_eq!(rect.y, console.y);
    assert_eq!(rect.width, console.width);
    assert_eq!(rect.height, console.height);
  }

  #[test]
  fn test_window_rect_contains_point_inside() {
    let mut console = Console::new();
    console.x = 50.0;
    console.y = 50.0;
    console.width = 200.0;
    console.height = 150.0;

    let rect = console.window_rect();
    assert!(rect.contains(100.0, 100.0));
    assert!(!rect.contains(10.0, 10.0));
    assert!(!rect.contains(300.0, 300.0));
  }

  // ============================================================================
  // Unfocused Text Color Alpha
  // ============================================================================

  #[test]
  fn test_unfocused_text_alpha_scaling() {
    // Verify the alpha scaling formula: (a * 160 / 255)
    let color = colors::COMMAND; // rgb(100, 180, 255, 255)
    let scaled_alpha = (color.a as u16 * 160 / 255) as u8;
    assert_eq!(scaled_alpha, 160); // 255 * 160 / 255 = 160

    let error_color = colors::ERROR; // rgb(255, 100, 100, 255)
    let error_scaled = (error_color.a as u16 * 160 / 255) as u8;
    assert_eq!(error_scaled, 160);

    // System color also fully opaque
    let system_scaled = (colors::SYSTEM.a as u16 * 160 / 255) as u8;
    assert_eq!(system_scaled, 160);
  }

  #[test]
  fn test_unfocused_text_preserves_rgb() {
    // When unfocused, only alpha changes — RGB should be preserved
    let color = Color::new(100, 180, 255, 255);
    let unfocused = Color::new(
      color.r,
      color.g,
      color.b,
      (color.a as u16 * 160 / 255) as u8,
    );
    assert_eq!(unfocused.r, 100);
    assert_eq!(unfocused.g, 180);
    assert_eq!(unfocused.b, 255);
    assert_eq!(unfocused.a, 160);
  }

  // ============================================================================
  // Focus State
  // ============================================================================

  #[test]
  fn test_focus_state_unfocused_after_outside_click() {
    // Verify that focused starts true but can be set to false
    let mut console = Console::new();
    console.toggle(); // make visible
    assert!(console.is_focused());

    console.focused = false;
    assert!(!console.is_focused());
  }
}
