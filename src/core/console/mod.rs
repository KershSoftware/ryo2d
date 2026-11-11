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
//!
//! The console is a developer tool: a build without the tools compiles only
//! `ConsoleScope`, `Command` and `CommandSpec`, whose calls do nothing.

#[cfg(dev_tools)]
mod builtins;
mod command;
#[cfg(dev_tools)]
mod dispatch;
#[cfg(dev_tools)]
mod registry;
mod scope;
#[cfg(dev_tools)]
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

#[cfg(dev_tools)]
use std::collections::VecDeque;

#[cfg(dev_tools)]
use super::cursor::CursorStyle;
#[cfg(dev_tools)]
use super::ecs::Resource;
#[cfg(dev_tools)]
use super::input::Input;
#[cfg(dev_tools)]
use super::text::{FontRole, TextStyle};
#[cfg(dev_tools)]
use super::ui::widgets::text_edit::TextEdit;
#[cfg(dev_tools)]
use super::ui::{Color, Rect};
#[cfg(dev_tools)]
use winit::event::MouseButton;
#[cfg(dev_tools)]
use winit::keyboard::KeyCode;

/// Maximum number of output lines to keep in history
#[cfg(dev_tools)]
const MAX_OUTPUT_LINES: usize = 1000;

/// Maximum number of commands to keep in history
#[cfg(dev_tools)]
const MAX_COMMAND_HISTORY: usize = 100;

/// The most characters the input line holds; the line has no other limit,
/// and a huge paste must not stall a frame
#[cfg(dev_tools)]
const MAX_INPUT_CHARS: usize = 4096;

/// Minimum console window dimensions
#[cfg(dev_tools)]
const MIN_WIDTH: f32 = 300.0;
#[cfg(dev_tools)]
const MIN_HEIGHT: f32 = 150.0;

/// Default console window dimensions
#[cfg(dev_tools)]
const DEFAULT_WIDTH: f32 = 600.0;
#[cfg(dev_tools)]
const DEFAULT_HEIGHT: f32 = 400.0;

/// Title bar height, at least (it grows with the console font)
#[cfg(dev_tools)]
const TITLE_BAR_HEIGHT: f32 = 24.0;

/// Close button size
#[cfg(dev_tools)]
const CLOSE_BUTTON_SIZE: f32 = 16.0;

/// Input line height, at least (it grows with the console font)
#[cfg(dev_tools)]
const INPUT_HEIGHT: f32 = 24.0;

/// Room around a line of text in the title bar and input line
#[cfg(dev_tools)]
const BAR_PADDING: f32 = 10.0;

/// The line height assumed before the first render measures the font
#[cfg(dev_tools)]
const DEFAULT_LINE_HEIGHT: f32 = 12.0;

/// The console's text: JetBrains Mono, 13 px
#[cfg(dev_tools)]
pub(crate) fn console_style() -> TextStyle {
  TextStyle::new().role(FontRole::Mono).size(13.0)
}

/// Resize handle size (corner)
#[cfg(dev_tools)]
const RESIZE_HANDLE_SIZE: f32 = 12.0;

/// Scrollbar width
#[cfg(dev_tools)]
const SCROLLBAR_WIDTH: f32 = 8.0;

/// Minimum scrollbar thumb height
#[cfg(dev_tools)]
const SCROLLBAR_MIN_THUMB: f32 = 20.0;

/// Lines a wheel notch scrolls
#[cfg(dev_tools)]
const WHEEL_LINES: f32 = 3.0;

/// Time constant (seconds) of the wheel's ease, as the UI's scroll areas
#[cfg(dev_tools)]
const SCROLL_SMOOTHING: f32 = 0.08;

/// Less than this far (in units) from the bottom counts as the bottom
#[cfg(dev_tools)]
const BOTTOM_SNAP: f32 = 0.5;

/// Closer than this to its target (in lines) the view settles
#[cfg(dev_tools)]
const SCROLL_SNAP: f32 = 0.02;

/// Console visual colors
#[cfg(dev_tools)]
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
  pub const WARNING: Color = Color::new(255, 210, 90, 255);
  /// System message color (gray)
  pub const SYSTEM: Color = Color::new(150, 150, 150, 255);
  /// The selected part of the input line
  pub const SELECTION: Color = Color::new(60, 90, 150, 255);
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
#[cfg(dev_tools)]
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

#[cfg(dev_tools)]
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
#[cfg(dev_tools)]
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
#[cfg(dev_tools)]
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
  /// The input line's cursor, selection, blink and paste guard (the text
  /// field's editing state, over `input_buffer`)
  edit: TextEdit,
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
  /// The close button's `X` and the input prompt as measured, and the UI
  /// scale they were measured at: measured once, not every frame
  marks: Option<(f32, glam::Vec2, f32)>,
  /// The lines, from the oldest, whose `rows` are wrapped at `wrap_width`
  wrapped_lines: usize,
  /// Their rows altogether
  wrapped_row_count: usize,
  /// Lines printed since the last render while the reader was scrolled up
  unseen_lines: usize,
  /// Total time elapsed (for timestamps)
  total_time: f32,
  /// Current drag state
  drag_state: DragState,
  /// The console key that last opened or closed the console is still
  /// down: its auto-repeats neither type nor toggle (`handle_debug_keys`)
  key_held: bool,
  /// Tab was pressed this frame (the engine completes with its commands)
  tab_pressed: bool,
  /// The input line as the last Tab with several matches left it: a Tab
  /// on the same line lists them
  tab_line: Option<String>,
  /// The console font's line height as last measured by `render`
  line_height: f32,
  /// The lowest level of log record the console shows (`log:show`;
  /// `Off` shows none)
  pub(crate) log_show: log::LevelFilter,
  /// The text of each line printed while set (`HeadlessEngine::console_run`)
  captured: Option<Vec<String>>,
  /// The last log sequence number the mirror read
  log_seq: u64,
  /// The engine scope's warnings and errors the ring had been given at the
  /// last read (the ring counts those it drops past it)
  log_mark: u64,
  /// Times the mirror read the ring (a test's count)
  #[cfg(test)]
  pub(crate) mirror_reads: usize,
  /// The mirror's records, kept for their room
  log_records: Vec<crate::core::logging::LogRecord>,
  /// Lines wrapped so far (a test's count)
  #[cfg(test)]
  wrap_calls: usize,
}

#[cfg(dev_tools)]
impl Default for Console {
  fn default() -> Self {
    Self::new()
  }
}

#[cfg(dev_tools)]
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
      edit: TextEdit::single_line(),
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
      marks: None,
      wrapped_lines: 0,
      wrapped_row_count: 0,
      unseen_lines: 0,
      total_time: 0.0,
      drag_state: DragState::None,
      key_held: false,
      tab_pressed: false,
      tab_line: None,
      line_height: DEFAULT_LINE_HEIGHT,
      log_show: log::LevelFilter::Warn,
      captured: None,
      log_seq: 0,
      log_mark: 0,
      #[cfg(test)]
      mirror_reads: 0,
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
  #[cfg(not(target_arch = "wasm32"))]
  pub(crate) fn capture(&mut self) {
    self.captured = Some(Vec::new());
  }

  /// The lines printed since `capture`, ending the capture
  #[cfg(not(target_arch = "wasm32"))]
  pub(crate) fn take_captured(&mut self) -> Vec<String> {
    self.captured.take().unwrap_or_default()
  }

  /// The last ring sequence the mirror read
  #[cfg(test)]
  pub(crate) fn log_seq(&self) -> u64 {
    self.log_seq
  }

  /// The input line as typed so far
  #[cfg(test)]
  pub(crate) fn input_line(&self) -> &str {
    &self.input_buffer
  }

  /// The output lines, oldest first
  #[cfg(any(test, not(target_arch = "wasm32")))]
  pub(crate) fn lines(&self) -> impl DoubleEndedIterator<Item = &ConsoleLine> {
    self.output_lines.iter()
  }

  /// Clear all output lines
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
      self.edit.reset_blink();
    }
  }

  /// Whether the console key that last toggled the console is still down
  pub(crate) fn key_held(&self) -> bool {
    self.key_held
  }

  pub(crate) fn set_key_held(&mut self, held: bool) {
    self.key_held = held;
  }

  /// The input line changed other than by Tab: the next Tab starts afresh
  fn line_edited(&mut self) {
    self.tab_line = None;
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
      self.edit.tick(dt);
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

    // The input line edits like a text field: typing, the cursor and
    // selection keys, and Cmd or Ctrl with A, C, X and V. The console key's
    // own text never gets here when it toggles (`handle_debug_keys` takes
    // it). The text is taken either way, so game widgets do not get it
    let had_text = input.has_text_input();
    let edited = self
      .edit
      .handle_input(&mut self.input_buffer, input, Some(MAX_INPUT_CHARS));
    if had_text {
      input.consume_text_input();
    }
    if edited.changed {
      self.line_edited();
    }

    // Tab completes the command name; the engine has the commands
    if input.key_pressed(KeyCode::Tab) {
      self.tab_pressed = true;
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
      self.cursor_to_end();
      self.line_edited();
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
      self.cursor_to_end();
      self.line_edited();
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
      self.edit.set_cursor(&self.input_buffer, 0, false);
      self.history_index = None;
      self.saved_input.clear();
      self.line_edited();

      submitted_command = Some(command);
    }

    // Escape to close console
    if input.key_pressed(KeyCode::Escape) {
      self.visible = false;
    }

    submitted_command
  }

  /// Whether Tab was pressed since the last call
  pub(crate) fn take_tab(&mut self) -> bool {
    std::mem::take(&mut self.tab_pressed)
  }

  /// Tab: complete the first word from `names` (the command names starting
  /// with a prefix). One match is completed with a space after it, so a
  /// second Tab does nothing; several are completed as far as they agree,
  /// and a Tab that finds the line as the last one left it lists them on
  /// one line. After the first word, Tab does nothing.
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
  fn set_input(&mut self, line: String) {
    self.input_buffer = line;
    self.cursor_to_end();
  }

  /// The cursor after the line's last character, nothing selected
  fn cursor_to_end(&mut self) {
    self
      .edit
      .set_cursor(&self.input_buffer, self.input_buffer.len(), false);
  }

  /// Mirror the log from the record after `seq` on, with the ring's
  /// warning `mark` for the engine's scope (`logging::mirror_start`: just
  /// before the ring's oldest record when the engine is built, so the
  /// warnings of its build and of the game's `main` are shown)
  pub(crate) fn start_log(&mut self, seq: u64, mark: u64) {
    self.log_seq = seq;
    self.log_mark = mark;
  }

  /// Append the log's records since the last read, of the engine's log
  /// `scope`, at or above `log_show`: warnings in yellow, errors in red, as
  /// `target: message`. Run when the console opens and each frame while it
  /// is open, so what was logged while it was closed is there when it
  /// opens; the newest `MAX_OUTPUT_LINES` win, and the scope's warnings and
  /// errors it cannot show (dropped by the ring, or past the cap) are
  /// counted in one grey line. With nothing logged since, it takes no lock.
  ///
  /// Printing logs nothing (a line is pushed, and wrapped when drawn), so
  /// the mirror never shows a record of its own making.
  pub(crate) fn mirror_log(&mut self, scope: u64) {
    if !crate::core::logging::records_may_follow(self.log_seq) {
      return;
    }
    #[cfg(test)]
    {
      self.mirror_reads += 1;
    }
    let mut records = std::mem::take(&mut self.log_records);
    let found = crate::core::logging::mirror_records(
      self.log_seq,
      self.log_mark,
      scope,
      self.log_show,
      MAX_OUTPUT_LINES,
      &mut records,
    );
    self.log_seq = found.newest;
    self.log_mark = found.mark;
    let mut hidden = found.hidden;
    if self.log_show != log::LevelFilter::Off && hidden > 0 {
      // The count takes a line of the cap, so it stays in view
      if records.len() == MAX_OUTPUT_LINES {
        let oldest = records.remove(0);
        hidden += u64::from(oldest.level <= log::Level::Warn);
      }
      self.print_system(format!(
        "{hidden} earlier warnings and errors are no longer shown"
      ));
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

#[cfg(all(test, dev_tools))]
mod tests;
