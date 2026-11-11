//! Logger window: a second OS window showing the log as it is written.
//!
//! It follows the log's ring by sequence number in its engine's log scope
//! (through a [`LogFeed`], which does the reading and filtering) for as long
//! as it is open, and draws what the feed holds.

use std::sync::Arc;

use winit::event::{MouseScrollDelta, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::window::WindowId;

use super::dev_window::{CHAR_HEIGHT, CHAR_WIDTH, DevWindowConfig, DevWindowRenderer, colors};
use super::log_feed::{LogFeed, Toggle, level_letter, origin_letter, row_text};
use crate::core::logging::{LogRecord, Origin, clock};

/// The height of a row, in the window's pixels.
const ROW_PITCH: i32 = CHAR_HEIGHT + 2;
const HEADER_HEIGHT: i32 = 30;
const FOOTER_HEIGHT: i32 = 24;
const PADDING: i32 = 8;
/// Rows a wheel's notch scrolls.
const ROWS_A_NOTCH: f32 = 3.0;

/// The rows a scroll moves, positive toward older rows: three a wheel notch;
/// a trackpad's pixel delta divided by the window's scale factor (logical
/// pixels), then by the row pitch. The part of a row left over is kept in
/// `carry` for the next delta.
fn scroll_rows(delta: MouseScrollDelta, scale_factor: f64, carry: &mut f64) -> i32 {
  match delta {
    MouseScrollDelta::LineDelta(_, y) => (y * ROWS_A_NOTCH).round() as i32,
    MouseScrollDelta::PixelDelta(position) => {
      let scale = if scale_factor > 0.0 {
        scale_factor
      } else {
        1.0
      };
      *carry += position.y / scale / f64::from(ROW_PITCH);
      let rows = carry.trunc();
      *carry -= rows;
      rows as i32
    }
  }
}

/// A header button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Button {
  Filter(Toggle),
  /// Empties the window's list (not the log)
  Clear,
  /// Back to the newest row, following again
  Bottom,
}

impl Button {
  const ALL: [Button; 10] = [
    Button::Filter(Toggle::ALL[0]),
    Button::Filter(Toggle::ALL[1]),
    Button::Filter(Toggle::ALL[2]),
    Button::Filter(Toggle::ALL[3]),
    Button::Filter(Toggle::ALL[4]),
    Button::Filter(Toggle::ALL[5]),
    Button::Filter(Toggle::ALL[6]),
    Button::Filter(Toggle::ALL[7]),
    Button::Clear,
    Button::Bottom,
  ];

  fn label(self) -> &'static str {
    match self {
      Button::Filter(toggle) => toggle.label(),
      Button::Clear => "CLR",
      Button::Bottom => "BTM",
    }
  }

  fn color(self) -> [u8; 4] {
    match self {
      Button::Filter(Toggle::Level(level)) => level_color(level),
      Button::Filter(Toggle::Engine) => colors::ENGINE_CONTEXT,
      Button::Filter(Toggle::Game) => colors::GAME_CONTEXT,
      Button::Filter(Toggle::Library) | Button::Clear | Button::Bottom => colors::TEXT_DIM,
    }
  }
}

fn level_color(level: log::Level) -> [u8; 4] {
  match level {
    log::Level::Trace => colors::LOG_TRACE,
    log::Level::Debug => colors::LOG_DEBUG,
    log::Level::Info => colors::LOG_INFO,
    log::Level::Warn => colors::LOG_WARN,
    log::Level::Error => colors::LOG_ERROR,
  }
}

fn origin_color(origin: Origin) -> [u8; 4] {
  match origin {
    Origin::Engine => colors::ENGINE_CONTEXT,
    Origin::Game => colors::GAME_CONTEXT,
    Origin::Library => colors::TEXT_DIM,
  }
}

/// Text selection state.
#[derive(Clone, Default)]
struct TextSelection {
  /// Is selection active (mouse is being dragged)?
  active: bool,
  /// Starting position (line index, char index within rendered text).
  start: Option<(usize, usize)>,
  /// Ending position (line index, char index within rendered text).
  end: Option<(usize, usize)>,
}

impl TextSelection {
  /// Get the normalized selection range (start <= end).
  fn normalized(&self) -> Option<((usize, usize), (usize, usize))> {
    match (self.start, self.end) {
      (Some(s), Some(e)) => {
        if s.0 < e.0 || (s.0 == e.0 && s.1 <= e.1) {
          Some((s, e))
        } else {
          Some((e, s))
        }
      }
      _ => None,
    }
  }

  /// Check if any text is selected.
  fn has_selection(&self) -> bool {
    matches!((self.start, self.end), (Some(s), Some(e)) if s != e)
  }

  /// Clear the selection.
  fn clear(&mut self) {
    self.active = false;
    self.start = None;
    self.end = None;
  }
}

/// Manages the logger window and its rendering.
pub struct LoggerWindow {
  renderer: DevWindowRenderer,
  /// The log as this window reads it
  feed: LogFeed,
  /// Rows scrolled up from the newest (0 follows the log)
  scroll_offset: usize,
  /// Follows new rows while at the bottom
  auto_scroll: bool,
  /// A trackpad's part of a row, kept for its next delta
  scroll_carry: f64,
  /// The hovered row, an index into `visible_rows`
  hovered_row: Option<usize>,
  hovered_button: Option<Button>,
  dragging_scrollbar: bool,
  /// The rows the filter showed at the last render (for the scrollbar)
  last_shown_count: usize,
  /// The rows on screen at the last render, for copying
  visible_rows: Vec<LogRecord>,
  /// The text of each row on screen, for selecting and copying
  rendered_lines: Vec<String>,
  /// Copy feedback message and timer
  copy_feedback: Option<(String, web_time::Instant)>,
  selection: TextSelection,
}

impl LoggerWindow {
  /// Opens the window on the records of the log scope `scope` (its engine's),
  /// from the oldest the log still holds.
  pub fn new(
    event_loop: &ActiveEventLoop,
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    instance: &wgpu::Instance,
    adapter: &wgpu::Adapter,
    scope: u64,
  ) -> Option<Self> {
    let config = DevWindowConfig {
      title: "Ryo2D Logger".to_string(),
      width: 800,
      height: 500,
      render_throttle_ms: 50, // ~20 FPS for responsive log updates
    };

    let renderer = DevWindowRenderer::new(event_loop, device, queue, instance, adapter, config)?;
    log::info!("Logger window opened");

    Some(Self {
      renderer,
      feed: LogFeed::new(scope),
      scroll_offset: 0,
      auto_scroll: true,
      scroll_carry: 0.0,
      hovered_row: None,
      hovered_button: None,
      dragging_scrollbar: false,
      last_shown_count: 0,
      visible_rows: Vec::new(),
      rendered_lines: Vec::new(),
      copy_feedback: None,
      selection: TextSelection::default(),
    })
  }

  /// Get the window ID for event routing.
  pub fn window_id(&self) -> WindowId {
    self.renderer.window_id()
  }

  /// Handle a window event.
  pub fn handle_event(&mut self, event: &WindowEvent) -> bool {
    // Handle keyboard events (Ctrl+C for copy)
    if let WindowEvent::KeyboardInput {
      event: key_event, ..
    } = event
      && key_event.state == winit::event::ElementState::Pressed
      && let winit::keyboard::PhysicalKey::Code(keycode) = key_event.physical_key
      && keycode == winit::keyboard::KeyCode::KeyC
    {
      let modifiers = self.renderer.modifiers();
      if modifiers.control_key() {
        self.copy_selection();
        return true;
      }
    }

    if let WindowEvent::MouseWheel { delta, .. } = event {
      let rows = scroll_rows(*delta, self.renderer.scale_factor(), &mut self.scroll_carry);
      if rows > 0 {
        // Up, toward older rows
        self.scroll_offset = self
          .scroll_offset
          .saturating_add(rows.unsigned_abs() as usize);
        self.auto_scroll = false;
      } else if rows < 0 {
        self.scroll_offset = self
          .scroll_offset
          .saturating_sub(rows.unsigned_abs() as usize);
        if self.scroll_offset == 0 {
          self.auto_scroll = true;
        }
      }
      self.renderer.request_redraw();
      return true;
    }

    // Handle mouse button events for scrollbar dragging, filter buttons, and text selection
    if let WindowEvent::MouseInput { state, button, .. } = event {
      if *button == winit::event::MouseButton::Left {
        if *state == winit::event::ElementState::Pressed {
          let mouse_x = self.renderer.mouse_x();
          let mouse_y = self.renderer.mouse_y();
          let width = self.renderer.width() as i32;
          let height = self.renderer.height() as i32;
          let scrollbar_x = width - 14;
          let in_content =
            mouse_y >= HEADER_HEIGHT as f32 && mouse_y <= (height - FOOTER_HEIGHT) as f32;

          // Check if clicking on scrollbar
          if mouse_x >= scrollbar_x as f32 && in_content {
            self.dragging_scrollbar = true;
            self.selection.clear();
            self.update_scroll_from_mouse(mouse_y);
            self.renderer.request_redraw();
            return true;
          }

          if let Some(button) = self.hovered_button {
            self.press(button);
            self.selection.clear();
            self.renderer.request_redraw();
            return true;
          }

          // Start text selection in content area
          if in_content {
            let pos = self.mouse_to_text_position(mouse_x, mouse_y);
            self.selection.clear();
            self.selection.active = true;
            self.selection.start = Some(pos);
            self.selection.end = Some(pos);
            self.renderer.request_redraw();
            return true;
          }
        } else {
          // Mouse released
          self.dragging_scrollbar = false;
          self.selection.active = false;
        }
      }

      // Right-click to copy hovered entry (if no selection)
      if *button == winit::event::MouseButton::Right
        && *state == winit::event::ElementState::Pressed
      {
        if self.selection.has_selection() {
          self.copy_selection();
        } else if let Some(record) = self.hovered_row.and_then(|row| self.visible_rows.get(row)) {
          let text = row_text(record);
          self.copy_to_clipboard(&text);
        }
        return true;
      }
    }

    // Handle mouse movement for scrollbar dragging and text selection
    if let WindowEvent::CursorMoved { position, .. } = event {
      if self.dragging_scrollbar {
        self.update_scroll_from_mouse(position.y as f32);
        self.renderer.request_redraw();
      } else if self.selection.active {
        let pos = self.mouse_to_text_position(position.x as f32, position.y as f32);
        self.selection.end = Some(pos);
        self.renderer.request_redraw();
      }
    }

    self.renderer.handle_event(event)
  }

  /// Convert mouse position to text position (line index, char index).
  fn mouse_to_text_position(&self, mouse_x: f32, mouse_y: f32) -> (usize, usize) {
    let relative_y = (mouse_y - HEADER_HEIGHT as f32).max(0.0);
    let line_idx = (relative_y / ROW_PITCH as f32) as usize;
    let line_idx = line_idx.min(self.rendered_lines.len().saturating_sub(1));

    let relative_x = (mouse_x - PADDING as f32).max(0.0);
    let char_idx = (relative_x / CHAR_WIDTH as f32) as usize;

    // Clamp to the line's length in characters
    let max_chars = self
      .rendered_lines
      .get(line_idx)
      .map_or(0, |s| s.chars().count());
    (line_idx, char_idx.min(max_chars))
  }

  /// Copy the current text selection to clipboard.
  fn copy_selection(&mut self) {
    if !self.selection.has_selection() {
      return;
    }

    let Some(((start_line, start_char), (end_line, end_char))) = self.selection.normalized() else {
      return;
    };

    let mut text = String::new();

    for (line_idx, line) in self.rendered_lines.iter().enumerate() {
      if line_idx < start_line || line_idx > end_line {
        continue;
      }
      let chars: Vec<char> = line.chars().collect();
      let line_start = if line_idx == start_line {
        start_char
      } else {
        0
      }
      .min(chars.len());
      let line_end = if line_idx == end_line {
        end_char
      } else {
        chars.len()
      }
      .min(chars.len());

      if line_start < line_end {
        if !text.is_empty() {
          text.push('\n');
        }
        text.extend(&chars[line_start..line_end]);
      }
    }

    if !text.is_empty() {
      self.copy_to_clipboard(&text);
    }
  }

  /// Update scroll position based on mouse Y position on scrollbar.
  fn update_scroll_from_mouse(&mut self, mouse_y: f32) {
    let height = self.renderer.height() as i32;
    let content_height = height - HEADER_HEIGHT - FOOTER_HEIGHT;
    let scrollbar_y = HEADER_HEIGHT + 2;
    let scrollbar_height = content_height - 4;

    // Calculate scroll ratio from mouse position (inverted: top = max scroll, bottom = 0)
    let relative_y = mouse_y - scrollbar_y as f32;
    let scroll_ratio = 1.0 - (relative_y / scrollbar_height as f32).clamp(0.0, 1.0);

    let max_scroll = self
      .last_shown_count
      .saturating_sub((content_height / ROW_PITCH).max(0) as usize);
    self.scroll_offset = (scroll_ratio * max_scroll as f32) as usize;
    self.auto_scroll = self.scroll_offset == 0;
  }

  fn press(&mut self, button: Button) {
    match button {
      Button::Filter(toggle) => self.feed.toggle(toggle),
      Button::Clear => {
        self.feed.clear();
        self.scroll_offset = 0;
        self.auto_scroll = true;
      }
      Button::Bottom => {
        self.scroll_offset = 0;
        self.auto_scroll = true;
      }
    }
  }

  /// Copy text to system clipboard.
  fn copy_to_clipboard(&mut self, text: &str) {
    let feedback = match crate::core::clipboard::set_text(text) {
      Ok(()) => "Copied!".to_string(),
      Err(e) => format!("Copy failed: {}", e),
    };
    self.copy_feedback = Some((feedback, web_time::Instant::now()));
    self.renderer.request_redraw();
  }

  /// Request a redraw.
  pub fn request_redraw(&self) {
    self.renderer.request_redraw();
  }

  /// Render the logger window.
  pub fn render(&mut self) {
    if !self.renderer.should_render() {
      return;
    }
    self.renderer.mark_rendered();

    // A scrolled-up view stays on the rows it shows
    let shown = self.feed.poll();
    if !self.auto_scroll {
      self.scroll_offset += shown;
    }

    self.renderer.clear(colors::BACKGROUND);
    self.draw_header();
    self.draw_rows();
    self.draw_footer();

    // Draw copy feedback tooltip if recent
    if let Some((ref msg, instant)) = self.copy_feedback {
      if instant.elapsed().as_millis() < 1500 {
        let width = self.renderer.width() as i32;
        let height = self.renderer.height() as i32;
        let msg_width = self.renderer.measure_text(msg) + 16;
        let x = (width - msg_width) / 2;
        let y = height / 2 - 20;
        self
          .renderer
          .fill_rect(x, y, msg_width, 24, [60, 120, 60, 240]);
        self
          .renderer
          .draw_rect_outline(x, y, msg_width, 24, [100, 180, 100, 255]);
        self
          .renderer
          .draw_text(msg, x + 8, y + 6, [200, 255, 200, 255]);
      } else {
        self.copy_feedback = None;
      }
    }

    self.renderer.present();
  }

  /// Draw the header with filter buttons.
  fn draw_header(&mut self) {
    let width = self.renderer.width() as i32;
    let mouse_x = self.renderer.mouse_x();
    let mouse_y = self.renderer.mouse_y();

    self
      .renderer
      .fill_rect(0, 0, width, HEADER_HEIGHT, colors::HEADER_BG);

    let mut x = PADDING;
    self.hovered_button = None;

    for button in Button::ALL {
      let label = button.label();
      let color = button.color();
      let active = match button {
        Button::Filter(toggle) => self.feed.is_on(toggle),
        Button::Clear | Button::Bottom => true,
      };
      let btn_width = self.renderer.measure_text(label) + 8;
      let btn_height = 18;
      let btn_y = 6;

      let hovered = mouse_x >= x as f32
        && mouse_x <= (x + btn_width) as f32
        && mouse_y >= btn_y as f32
        && mouse_y <= (btn_y + btn_height) as f32;
      if hovered {
        self.hovered_button = Some(button);
      }

      let bg_color = if active {
        if hovered {
          [
            color[0].saturating_sub(20),
            color[1].saturating_sub(20),
            color[2].saturating_sub(20),
            255,
          ]
        } else {
          [color[0] / 3, color[1] / 3, color[2] / 3, 255]
        }
      } else if hovered {
        [50, 50, 50, 255]
      } else {
        [35, 35, 35, 255]
      };

      self
        .renderer
        .fill_rect(x, btn_y, btn_width, btn_height, bg_color);
      if active {
        self
          .renderer
          .draw_rect_outline(x, btn_y, btn_width, btn_height, color);
      }
      let text_color = if active { color } else { colors::TEXT_DIM };
      self.renderer.draw_text(label, x + 4, btn_y + 3, text_color);

      x += btn_width + 4;
    }

    self
      .renderer
      .fill_rect(0, HEADER_HEIGHT - 1, width, 1, colors::DIVIDER);
  }

  /// Draw the rows the filter shows, scrolled.
  fn draw_rows(&mut self) {
    let width = self.renderer.width() as i32;
    let height = self.renderer.height() as i32;
    let content_height = height - HEADER_HEIGHT - FOOTER_HEIGHT;
    let visible_lines = (content_height / ROW_PITCH).max(0) as usize;
    let mouse_y = self.renderer.mouse_y();

    let shown = self.feed.visible().count();
    self.last_shown_count = shown;
    self.rendered_lines.clear();
    self.visible_rows.clear();
    self.hovered_row = None;

    if shown == 0 {
      let y = HEADER_HEIGHT + PADDING;
      self
        .renderer
        .draw_text("No log entries", PADDING, y, colors::TEXT_DIM);
      if self.feed.rows().is_empty() {
        self.renderer.draw_text(
          "Use log::info!() or ryo2d::info!() to log messages",
          PADDING,
          y + ROW_PITCH,
          colors::TEXT_DIM,
        );
      }
      return;
    }

    let max_scroll = shown.saturating_sub(visible_lines);
    self.scroll_offset = self.scroll_offset.min(max_scroll);
    let start = shown.saturating_sub(visible_lines + self.scroll_offset);
    let end = shown.saturating_sub(self.scroll_offset);
    self
      .visible_rows
      .extend(self.feed.visible().skip(start).take(end - start).cloned());

    let selection_range = self.selection.normalized();
    let rows = std::mem::take(&mut self.visible_rows);
    for (display_idx, record) in rows.iter().enumerate() {
      let y = HEADER_HEIGHT + (display_idx as i32 * ROW_PITCH);

      let hovered = mouse_y >= y as f32 && mouse_y < (y + ROW_PITCH) as f32;
      if hovered && !self.selection.active {
        self.hovered_row = Some(display_idx);
        self
          .renderer
          .fill_rect(0, y, width, ROW_PITCH, [40, 40, 40, 255]);
      }

      let line_text = row_text(record);
      let line_chars = line_text.chars().count();

      if let Some(((start_line, start_char), (end_line, end_char))) = selection_range
        && display_idx >= start_line
        && display_idx <= end_line
      {
        let line_start = if display_idx == start_line {
          start_char
        } else {
          0
        };
        let line_end = if display_idx == end_line {
          end_char
        } else {
          line_chars
        };
        if line_start < line_end {
          let sel_x = PADDING + (line_start as i32 * CHAR_WIDTH);
          let sel_width = ((line_end - line_start) as i32 * CHAR_WIDTH).min(width - sel_x);
          self
            .renderer
            .fill_rect(sel_x, y, sel_width, ROW_PITCH, [60, 80, 120, 255]);
        }
      }
      self.rendered_lines.push(line_text);

      self.draw_row(record, y, width);
    }
    self.visible_rows = rows;

    if max_scroll > 0 {
      self.draw_scrollbar(shown, visible_lines, max_scroll, content_height);
    }
  }

  /// One row: the time, the frame, the level and origin letters, then
  /// `target: message`, in the columns of [`row_text`].
  fn draw_row(&mut self, record: &LogRecord, y: i32, width: i32) {
    let mut x = PADDING;
    let mut column = |renderer: &mut DevWindowRenderer, text: &str, color: [u8; 4]| {
      renderer.draw_text(text, x, y + 1, color);
      x += (text.chars().count() as i32 + 1) * CHAR_WIDTH;
    };
    let time = clock(record.time);
    column(&mut self.renderer, &time, [100, 100, 100, 255]);
    let frame = record
      .frame
      .map_or_else(|| " ".repeat(7), |frame| format!("{frame:>7}"));
    column(&mut self.renderer, &frame, colors::TEXT_DIM);
    column(
      &mut self.renderer,
      level_letter(record.level),
      level_color(record.level),
    );
    column(
      &mut self.renderer,
      origin_letter(record.origin),
      origin_color(record.origin),
    );
    let target = format!("{}:", record.target);
    column(&mut self.renderer, &target, colors::TEXT_DIM);

    // The message, cut to fit
    let max_chars = ((width - x - PADDING) / CHAR_WIDTH).max(0) as usize;
    let message = if record.message.chars().count() > max_chars {
      let kept: String = record
        .message
        .chars()
        .take(max_chars.saturating_sub(3))
        .collect();
      format!("{kept}...")
    } else {
      record.message.clone()
    };
    self.renderer.draw_text(&message, x, y + 1, colors::TEXT);
  }

  fn draw_scrollbar(
    &mut self,
    shown: usize,
    visible_lines: usize,
    max_scroll: usize,
    content_height: i32,
  ) {
    let width = self.renderer.width() as i32;
    let scrollbar_height = content_height - 4;
    let scrollbar_x = width - 10;
    let scrollbar_y = HEADER_HEIGHT + 2;
    let scrollbar_width = 8;

    let mouse_x = self.renderer.mouse_x();
    let mouse_y = self.renderer.mouse_y();
    let hovering_scrollbar = mouse_x >= (scrollbar_x - 2) as f32
      && mouse_y >= scrollbar_y as f32
      && mouse_y <= (scrollbar_y + scrollbar_height) as f32;

    let track_color = if hovering_scrollbar || self.dragging_scrollbar {
      [50, 50, 50, 255]
    } else {
      colors::SCROLLBAR_BG
    };
    self.renderer.fill_rect(
      scrollbar_x,
      scrollbar_y,
      scrollbar_width,
      scrollbar_height,
      track_color,
    );

    let thumb_height =
      ((visible_lines as f32 / shown as f32) * scrollbar_height as f32).max(20.0) as i32;
    let scroll_ratio = self.scroll_offset as f32 / max_scroll as f32;
    let thumb_y =
      scrollbar_y + ((1.0 - scroll_ratio) * (scrollbar_height - thumb_height) as f32) as i32;

    // Highlight when dragging or hovering, dim when following
    let thumb_color = if self.dragging_scrollbar {
      [140, 140, 160, 255]
    } else if hovering_scrollbar || self.auto_scroll {
      colors::SCROLLBAR_THUMB_HOVER
    } else {
      colors::SCROLLBAR_THUMB
    };
    self.renderer.fill_rect(
      scrollbar_x,
      thumb_y,
      scrollbar_width,
      thumb_height,
      thumb_color,
    );
  }

  /// Draw the footer with stats.
  fn draw_footer(&mut self) {
    let footer_y = self.renderer.height() as i32 - FOOTER_HEIGHT;
    let width = self.renderer.width() as i32;

    self
      .renderer
      .fill_rect(0, footer_y, width, FOOTER_HEIGHT, colors::HEADER_BG);
    self
      .renderer
      .fill_rect(0, footer_y, width, 1, colors::DIVIDER);

    let (footer_text, text_color) = if self.selection.has_selection() {
      (
        "Ctrl+C to copy selection | Right-click to copy".to_string(),
        [180, 220, 180, 255],
      )
    } else if self.hovered_row.is_some() {
      (
        "Drag to select | Right-click to copy line".to_string(),
        [180, 180, 220, 255],
      )
    } else {
      // Rows by level: Error is 1 and Trace 5
      let mut counts = [0usize; 5];
      for record in self.feed.rows() {
        counts[record.level as usize - 1] += 1;
      }
      let [error, warn, info, debug, trace] = counts;
      let mut text = format!(
        "Total: {} | T:{trace} D:{debug} I:{info} W:{warn} E:{error}",
        self.feed.rows().len()
      );
      if self.feed.missed() > 0 {
        text.push_str(&format!(" | {} W/E missed", self.feed.missed()));
      }
      (text, colors::TEXT_DIM)
    };
    self
      .renderer
      .draw_text(&footer_text, PADDING, footer_y + 6, text_color);

    if self.auto_scroll {
      self
        .renderer
        .draw_text("AUTO", width - 40, footer_y + 6, colors::GAME_CONTEXT);
    } else {
      self
        .renderer
        .draw_text("PAUSED", width - 50, footer_y + 6, colors::LOG_WARN);
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use winit::dpi::PhysicalPosition;

  #[test]
  fn a_trackpad_delta_scrolls_by_logical_pixels() {
    assert_eq!(ROW_PITCH, 14);
    let mut carry = 0.0;
    let delta = MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, 28.0));
    assert_eq!(scroll_rows(delta, 2.0, &mut carry), 1, "28 / 2 / 14");
    // At scale 1 the same delta is two rows
    assert_eq!(scroll_rows(delta, 1.0, &mut 0.0), 2);
    // A part of a row waits for the next
    let half = MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, -14.0));
    let mut carry = 0.0;
    assert_eq!(scroll_rows(half, 2.0, &mut carry), 0);
    assert_eq!(scroll_rows(half, 2.0, &mut carry), -1);
    // A wheel notch is three rows
    assert_eq!(
      scroll_rows(MouseScrollDelta::LineDelta(0.0, 1.0), 2.0, &mut 0.0),
      3
    );
    assert_eq!(
      scroll_rows(MouseScrollDelta::LineDelta(0.0, -2.0), 1.0, &mut 0.0),
      -6
    );
  }
}
