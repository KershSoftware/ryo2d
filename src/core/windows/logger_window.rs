//! Logger window - A dedicated window showing runtime logs.
//!
//! This module provides a separate window that displays real-time log messages
//! from both engine and game code. Built on the generic dev_window framework.

use std::sync::Arc;

use winit::event::{MouseScrollDelta, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::window::WindowId;

use super::dev_window::{CHAR_HEIGHT, CHAR_WIDTH, DevWindowConfig, DevWindowRenderer, colors};
use crate::core::dev_log::{LogContext, LogEntry, LogLevel, entry_count, get_entries_since};
use crate::engine_info;

/// Filter settings for the logger.
#[derive(Clone)]
struct LogFilter {
  show_trace: bool,
  show_debug: bool,
  show_info: bool,
  show_warn: bool,
  show_error: bool,
  show_engine: bool,
  show_game: bool,
  show_external: bool,
  search_text: String,
}

impl Default for LogFilter {
  fn default() -> Self {
    Self {
      show_trace: false,
      show_debug: true,
      show_info: true,
      show_warn: true,
      show_error: true,
      show_engine: true,
      show_game: true,
      show_external: true,
      search_text: String::new(),
    }
  }
}

impl LogFilter {
  fn matches(&self, entry: &LogEntry) -> bool {
    // Level filter
    let level_ok = match entry.level {
      LogLevel::Trace => self.show_trace,
      LogLevel::Debug => self.show_debug,
      LogLevel::Info => self.show_info,
      LogLevel::Warn => self.show_warn,
      LogLevel::Error => self.show_error,
    };

    // Context filter
    let context_ok = match entry.context {
      LogContext::Engine => self.show_engine,
      LogContext::Game => self.show_game,
      LogContext::External => self.show_external,
    };

    // Search filter
    let search_ok = self.search_text.is_empty()
      || entry
        .message
        .to_lowercase()
        .contains(&self.search_text.to_lowercase());

    level_ok && context_ok && search_ok
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
  // Cached log entries for display
  cached_entries: Vec<LogEntry>,
  // Number of entries we've processed
  processed_count: usize,
  // Scroll position (0 = bottom, positive = scrolled up)
  scroll_offset: usize,
  // Auto-scroll when new entries arrive
  auto_scroll: bool,
  // Filter settings
  filter: LogFilter,
  // Hovered entry index for details
  hovered_entry: Option<usize>,
  // Selected filter button for toggle
  hovered_button: Option<usize>,
  // Scrollbar dragging state
  dragging_scrollbar: bool,
  // Track filtered entry count for stable scrolling when paused
  last_filtered_count: usize,
  // Store the currently visible entries for copy functionality
  visible_entries: Vec<LogEntry>,
  // Store rendered text for each visible line (for selection/copy)
  rendered_lines: Vec<String>,
  // Copy feedback message and timer
  copy_feedback: Option<(String, std::time::Instant)>,
  // Text selection state
  selection: TextSelection,
}

impl LoggerWindow {
  /// Create a new logger window.
  pub fn new(
    event_loop: &ActiveEventLoop,
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    instance: &wgpu::Instance,
    adapter: &wgpu::Adapter,
  ) -> Option<Self> {
    let config = DevWindowConfig {
      title: "Ryo2D Logger".to_string(),
      width: 800,
      height: 500,
      render_throttle_ms: 50, // ~20 FPS for responsive log updates
    };

    let renderer = DevWindowRenderer::new(event_loop, device, queue, instance, adapter, config)?;

    // Fetch all existing log entries immediately so we don't miss logs from before window opened
    let existing_entries = crate::core::dev_log::get_entries();
    let processed_count = existing_entries.len();

    engine_info!(module: "Logger", "Window opened ({} existing entries)", processed_count);

    Some(Self {
      renderer,
      cached_entries: existing_entries,
      processed_count,
      scroll_offset: 0,
      auto_scroll: true,
      filter: LogFilter::default(),
      hovered_entry: None,
      hovered_button: None,
      dragging_scrollbar: false,
      last_filtered_count: 0,
      visible_entries: Vec::new(),
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
    {
      if key_event.state == winit::event::ElementState::Pressed {
        if let winit::keyboard::PhysicalKey::Code(keycode) = key_event.physical_key {
          // Ctrl+C to copy selection
          if keycode == winit::keyboard::KeyCode::KeyC {
            let modifiers = self.renderer.modifiers();
            if modifiers.control_key() {
              self.copy_selection();
              return true;
            }
          }
        }
      }
    }

    // Handle scroll events
    if let WindowEvent::MouseWheel { delta, .. } = event {
      let scroll_amount = match delta {
        MouseScrollDelta::LineDelta(_, y) => (*y * 3.0) as i32,
        MouseScrollDelta::PixelDelta(pos) => (pos.y / 10.0) as i32,
      };

      if scroll_amount > 0 {
        // Scroll up (show older entries)
        self.scroll_offset = self
          .scroll_offset
          .saturating_add(scroll_amount.unsigned_abs() as usize);
        self.auto_scroll = false;
      } else if scroll_amount < 0 {
        // Scroll down (show newer entries)
        self.scroll_offset = self
          .scroll_offset
          .saturating_sub(scroll_amount.unsigned_abs() as usize);
        // Re-enable auto-scroll if at bottom
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
          let header_height = 30;
          let footer_height = 24;
          let height = self.renderer.height() as i32;
          let scrollbar_x = width - 14;

          // Check if clicking on scrollbar
          if mouse_x >= scrollbar_x as f32
            && mouse_y >= header_height as f32
            && mouse_y <= (height - footer_height) as f32
          {
            self.dragging_scrollbar = true;
            self.selection.clear();
            self.update_scroll_from_mouse(mouse_y);
            self.renderer.request_redraw();
            return true;
          }

          // Check filter buttons
          if let Some(button_idx) = self.hovered_button {
            self.toggle_filter(button_idx);
            self.selection.clear();
            self.renderer.request_redraw();
            return true;
          }

          // Start text selection in content area
          if mouse_y >= header_height as f32 && mouse_y <= (height - footer_height) as f32 {
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
          // Copy selection instead
          self.copy_selection();
        } else if let Some(entry_idx) = self.hovered_entry {
          if entry_idx < self.visible_entries.len() {
            let entry = &self.visible_entries[entry_idx];
            let text = self.format_entry_for_copy(entry);
            self.copy_to_clipboard(&text);
          }
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
    let header_height = 30;
    let line_height = CHAR_HEIGHT + 2;
    let padding = 8;

    // Calculate line index
    let relative_y = (mouse_y - header_height as f32).max(0.0);
    let line_idx = (relative_y / line_height as f32) as usize;
    let line_idx = line_idx.min(self.rendered_lines.len().saturating_sub(1));

    // Calculate char index
    let relative_x = (mouse_x - padding as f32).max(0.0);
    let char_idx = (relative_x / CHAR_WIDTH as f32) as usize;

    // Clamp to line length
    let max_chars = self
      .rendered_lines
      .get(line_idx)
      .map(|s| s.len())
      .unwrap_or(0);
    let char_idx = char_idx.min(max_chars);

    (line_idx, char_idx)
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

      let line_start = if line_idx == start_line {
        start_char
      } else {
        0
      };
      let line_end = if line_idx == end_line {
        end_char
      } else {
        line.len()
      };

      // Safe substring extraction
      let chars: Vec<char> = line.chars().collect();
      let line_start = line_start.min(chars.len());
      let line_end = line_end.min(chars.len());

      if line_start < line_end {
        let selected: String = chars[line_start..line_end].iter().collect();
        if !text.is_empty() {
          text.push('\n');
        }
        text.push_str(&selected);
      }
    }

    if !text.is_empty() {
      self.copy_to_clipboard(&text);
    }
  }

  /// Update scroll position based on mouse Y position on scrollbar.
  fn update_scroll_from_mouse(&mut self, mouse_y: f32) {
    let header_height = 30;
    let footer_height = 24;
    let height = self.renderer.height() as i32;
    let content_height = height - header_height - footer_height;
    let scrollbar_y = header_height + 2;
    let scrollbar_height = content_height - 4;

    // Calculate scroll ratio from mouse position (inverted: top = max scroll, bottom = 0)
    let relative_y = mouse_y - scrollbar_y as f32;
    let scroll_ratio = 1.0 - (relative_y / scrollbar_height as f32).clamp(0.0, 1.0);

    // Apply to scroll offset
    let max_scroll = self
      .last_filtered_count
      .saturating_sub((content_height / (CHAR_HEIGHT + 2)) as usize);
    self.scroll_offset = (scroll_ratio * max_scroll as f32) as usize;
    self.auto_scroll = self.scroll_offset == 0;
  }

  /// Toggle a filter button by index.
  fn toggle_filter(&mut self, idx: usize) {
    match idx {
      0 => self.filter.show_trace = !self.filter.show_trace,
      1 => self.filter.show_debug = !self.filter.show_debug,
      2 => self.filter.show_info = !self.filter.show_info,
      3 => self.filter.show_warn = !self.filter.show_warn,
      4 => self.filter.show_error = !self.filter.show_error,
      5 => self.filter.show_engine = !self.filter.show_engine,
      6 => self.filter.show_game = !self.filter.show_game,
      7 => {
        // Clear button
        crate::core::dev_log::clear();
        self.cached_entries.clear();
        self.processed_count = 0;
        self.scroll_offset = 0;
      }
      8 => {
        // Scroll to bottom
        self.scroll_offset = 0;
        self.auto_scroll = true;
      }
      _ => {}
    }
    // Refilter entries
    self.update_filtered_entries();
  }

  /// Update filtered entries from cache.
  fn update_filtered_entries(&mut self) {
    // We'll refilter on render
  }

  /// Format a log entry for copying to clipboard.
  fn format_entry_for_copy(&self, entry: &LogEntry) -> String {
    let elapsed = format!("{:.2}", entry.timestamp_ms as f64 / 1000.0);
    let level = entry.level.short_name();
    let context = match entry.context {
      LogContext::Engine => "E",
      LogContext::Game => "G",
      LogContext::External => "X",
    };
    let module = entry
      .module
      .as_ref()
      .map(|m| format!("[{}] ", m))
      .unwrap_or_default();
    format!(
      "{} {} {} {} {}{}",
      entry.iso_timestamp, elapsed, level, context, module, entry.message
    )
  }

  /// Copy text to system clipboard.
  fn copy_to_clipboard(&mut self, text: &str) {
    match arboard::Clipboard::new() {
      Ok(mut clipboard) => match clipboard.set_text(text) {
        Ok(_) => {
          self.copy_feedback = Some(("Copied!".to_string(), std::time::Instant::now()));
        }
        Err(e) => {
          self.copy_feedback = Some((format!("Copy failed: {}", e), std::time::Instant::now()));
        }
      },
      Err(e) => {
        self.copy_feedback = Some((format!("Clipboard error: {}", e), std::time::Instant::now()));
      }
    }
    self.renderer.request_redraw();
  }

  /// Request a redraw.
  pub fn request_redraw(&self) {
    self.renderer.request_redraw();
  }

  /// Render the logger window.
  pub fn render(&mut self) {
    // Check throttling
    if !self.renderer.should_render() {
      return;
    }
    self.renderer.mark_rendered();

    // Fetch new log entries
    self.fetch_new_entries();

    // Clear pixel buffer
    self.renderer.clear(colors::BACKGROUND);

    // Draw header with filters
    self.draw_header();

    // Draw log entries
    self.draw_entries();

    // Draw footer with stats
    self.draw_footer();

    // Draw copy feedback tooltip if recent
    if let Some((ref msg, instant)) = self.copy_feedback {
      if instant.elapsed().as_millis() < 1500 {
        let width = self.renderer.width() as i32;
        let height = self.renderer.height() as i32;
        let msg_width = (msg.len() as i32 * CHAR_WIDTH) + 16;
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

    // Upload and present
    self.renderer.present();
  }

  /// Fetch new log entries from the global buffer.
  fn fetch_new_entries(&mut self) {
    let total = entry_count();
    if total > self.processed_count {
      let new_entries = get_entries_since(self.processed_count);
      let new_count = new_entries.len();

      // Count how many of the new entries pass the filter
      let new_filtered_count = new_entries
        .iter()
        .filter(|e| self.filter.matches(e))
        .count();

      self.cached_entries.extend(new_entries);
      self.processed_count = total;

      // If NOT auto-scrolling, increase scroll_offset to keep view stable
      if !self.auto_scroll && new_filtered_count > 0 {
        self.scroll_offset += new_filtered_count;
      }

      // Limit cache size
      if self.cached_entries.len() > 10000 {
        let drain_count = 1000;
        // Adjust scroll offset if we're removing entries
        let removed_filtered = self.cached_entries[0..drain_count]
          .iter()
          .filter(|e| self.filter.matches(e))
          .count();
        self.scroll_offset = self.scroll_offset.saturating_sub(removed_filtered);
        self.cached_entries.drain(0..drain_count);
      }

      // Suppress unused variable warning
      let _ = new_count;
    }
  }

  /// Draw the header with filter buttons.
  fn draw_header(&mut self) {
    let header_height = 30;
    let width = self.renderer.width() as i32;
    let padding = 8;
    let mouse_x = self.renderer.mouse_x();
    let mouse_y = self.renderer.mouse_y();

    // Draw header background
    self
      .renderer
      .fill_rect(0, 0, width, header_height, colors::HEADER_BG);

    // Draw filter buttons
    let buttons = [
      ("T", self.filter.show_trace, colors::LOG_TRACE),
      ("D", self.filter.show_debug, colors::LOG_DEBUG),
      ("I", self.filter.show_info, colors::LOG_INFO),
      ("W", self.filter.show_warn, colors::LOG_WARN),
      ("E", self.filter.show_error, colors::LOG_ERROR),
      ("ENG", self.filter.show_engine, colors::ENGINE_CONTEXT),
      ("GAME", self.filter.show_game, colors::GAME_CONTEXT),
      ("CLR", true, colors::TEXT_DIM),
      ("BTM", true, colors::TEXT_DIM),
    ];

    let mut x = padding;
    self.hovered_button = None;

    for (idx, (label, active, color)) in buttons.iter().enumerate() {
      let btn_width = (label.len() as i32 * CHAR_WIDTH) + 8;
      let btn_height = 18;
      let btn_y = 6;

      // Check hover
      let hovered = mouse_x >= x as f32
        && mouse_x <= (x + btn_width) as f32
        && mouse_y >= btn_y as f32
        && mouse_y <= (btn_y + btn_height) as f32;

      if hovered {
        self.hovered_button = Some(idx);
      }

      // Draw button background
      let bg_color = if *active {
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

      // Draw button border if active
      if *active {
        self
          .renderer
          .draw_rect_outline(x, btn_y, btn_width, btn_height, *color);
      }

      // Draw button text
      let text_color = if *active { *color } else { colors::TEXT_DIM };
      self.renderer.draw_text(label, x + 4, btn_y + 3, text_color);

      x += btn_width + 4;
    }

    // Draw divider
    self
      .renderer
      .fill_rect(0, header_height - 1, width, 1, colors::DIVIDER);
  }

  /// Draw log entries.
  fn draw_entries(&mut self) {
    let header_height = 30;
    let footer_height = 24;
    let padding = 8;
    let line_height = CHAR_HEIGHT + 2;
    let width = self.renderer.width() as i32;
    let height = self.renderer.height() as i32;
    let content_height = height - header_height - footer_height;
    let visible_lines = (content_height / line_height) as usize;
    let _mouse_x = self.renderer.mouse_x();
    let mouse_y = self.renderer.mouse_y();

    // Filter entries
    let filtered: Vec<&LogEntry> = self
      .cached_entries
      .iter()
      .filter(|e| self.filter.matches(e))
      .collect();

    // Track filtered count for scrollbar calculations
    self.last_filtered_count = filtered.len();

    // Clear rendered lines for selection
    self.rendered_lines.clear();

    if filtered.is_empty() {
      let y = header_height + padding;
      self
        .renderer
        .draw_text("No log entries", padding, y, colors::TEXT_DIM);
      if self.cached_entries.is_empty() {
        self.renderer.draw_text(
          "Use engine_info!() or game_info!() to log messages",
          padding,
          y + line_height,
          colors::TEXT_DIM,
        );
      }
      return;
    }

    // Clamp scroll offset
    let max_scroll = filtered.len().saturating_sub(visible_lines);
    self.scroll_offset = self.scroll_offset.min(max_scroll);

    // Calculate which entries to show
    let start_idx = filtered
      .len()
      .saturating_sub(visible_lines + self.scroll_offset);
    let end_idx = filtered.len().saturating_sub(self.scroll_offset);

    // Store visible entries for copy functionality
    self.visible_entries.clear();
    for idx in start_idx..end_idx {
      self.visible_entries.push(filtered[idx].clone());
    }

    self.hovered_entry = None;

    // Get normalized selection for highlighting
    let selection_range = self.selection.normalized();

    for (display_idx, entry) in self.visible_entries.iter().enumerate() {
      let y = header_height + (display_idx as i32 * line_height);

      // Check hover - display_idx is now the index into visible_entries
      let hovered = mouse_y >= y as f32 && mouse_y < (y + line_height) as f32;
      if hovered && !self.selection.active {
        self.hovered_entry = Some(display_idx);
        // Draw hover highlight
        self
          .renderer
          .fill_rect(0, y, width, line_height, [40, 40, 40, 255]);
      }

      // Build the full rendered line text for selection
      let mut line_text = String::new();

      // ISO timestamp first (HH:MM:SS.mmm)
      let iso_ts = &entry.iso_timestamp;
      line_text.push_str(iso_ts);
      line_text.push(' ');

      // Time since start
      let elapsed = format!("{:>8.2}", entry.timestamp_ms as f64 / 1000.0);
      line_text.push_str(&elapsed);
      line_text.push(' ');

      // Level
      line_text.push_str(entry.level.short_name());
      line_text.push(' ');

      // Context
      let context_label = match entry.context {
        LogContext::Engine => "E",
        LogContext::Game => "G",
        LogContext::External => "X",
      };
      line_text.push_str(context_label);
      line_text.push(' ');

      // Module
      if let Some(ref module) = entry.module {
        line_text.push('[');
        line_text.push_str(module);
        line_text.push_str("] ");
      }

      // Message
      line_text.push_str(&entry.message);

      // Store for selection
      self.rendered_lines.push(line_text.clone());

      // Draw selection highlight if this line is in the selection range
      if let Some(((start_line, start_char), (end_line, end_char))) = selection_range {
        if display_idx >= start_line && display_idx <= end_line {
          let line_start = if display_idx == start_line {
            start_char
          } else {
            0
          };
          let line_end = if display_idx == end_line {
            end_char
          } else {
            line_text.len()
          };

          if line_start < line_end {
            let sel_x = padding + (line_start as i32 * CHAR_WIDTH);
            let sel_width = ((line_end - line_start) as i32 * CHAR_WIDTH).min(width - sel_x);
            self
              .renderer
              .fill_rect(sel_x, y, sel_width, line_height, [60, 80, 120, 255]);
          }
        }
      }

      let mut x = padding;

      // Draw ISO timestamp (HH:MM:SS.mmm) - dimmer
      self
        .renderer
        .draw_text(iso_ts, x, y + 1, [100, 100, 100, 255]);
      x += (iso_ts.len() as i32 + 1) * CHAR_WIDTH;

      // Draw elapsed timestamp
      self
        .renderer
        .draw_text(&elapsed, x, y + 1, colors::TEXT_DIM);
      x += (elapsed.len() as i32 + 1) * CHAR_WIDTH;

      // Draw level indicator
      let level_color = match entry.level {
        LogLevel::Trace => colors::LOG_TRACE,
        LogLevel::Debug => colors::LOG_DEBUG,
        LogLevel::Info => colors::LOG_INFO,
        LogLevel::Warn => colors::LOG_WARN,
        LogLevel::Error => colors::LOG_ERROR,
      };
      self
        .renderer
        .draw_text(entry.level.short_name(), x, y + 1, level_color);
      x += 2 * CHAR_WIDTH;

      // Draw context indicator
      let context_color = match entry.context {
        LogContext::Engine => colors::ENGINE_CONTEXT,
        LogContext::Game => colors::GAME_CONTEXT,
        LogContext::External => colors::TEXT_DIM,
      };
      self
        .renderer
        .draw_text(context_label, x, y + 1, context_color);
      x += 2 * CHAR_WIDTH;

      // Draw module if present
      if let Some(ref module) = entry.module {
        let module_text = format!("[{}]", module);
        self
          .renderer
          .draw_text(&module_text, x, y + 1, colors::TEXT_DIM);
        x += (module_text.len() as i32 + 1) * CHAR_WIDTH;
      }

      // Draw message (truncate if needed)
      let available_width = width - x - padding;
      let max_chars = (available_width / CHAR_WIDTH) as usize;
      let message = if entry.message.len() > max_chars {
        format!("{}...", &entry.message[..max_chars.saturating_sub(3)])
      } else {
        entry.message.clone()
      };
      self.renderer.draw_text(&message, x, y + 1, colors::TEXT);
    }

    // Draw scroll indicator if needed
    if max_scroll > 0 {
      let scrollbar_height = content_height - 4;
      let scrollbar_x = width - 10;
      let scrollbar_y = header_height + 2;
      let scrollbar_width = 8;

      // Check if mouse is hovering over scrollbar area
      let mouse_x = self.renderer.mouse_x();
      let mouse_y = self.renderer.mouse_y();
      let hovering_scrollbar = mouse_x >= (scrollbar_x - 2) as f32
        && mouse_y >= scrollbar_y as f32
        && mouse_y <= (scrollbar_y + scrollbar_height) as f32;

      // Draw scrollbar track
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

      // Draw scrollbar thumb
      let thumb_height =
        ((visible_lines as f32 / filtered.len() as f32) * scrollbar_height as f32).max(20.0) as i32;
      let scroll_ratio = if max_scroll > 0 {
        self.scroll_offset as f32 / max_scroll as f32
      } else {
        0.0
      };
      let thumb_y =
        scrollbar_y + ((1.0 - scroll_ratio) * (scrollbar_height - thumb_height) as f32) as i32;

      // Thumb color: highlight when dragging or hovering, dim when auto-scrolling
      let thumb_color = if self.dragging_scrollbar {
        [140, 140, 160, 255]
      } else if hovering_scrollbar {
        colors::SCROLLBAR_THUMB_HOVER
      } else if self.auto_scroll {
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
  }

  /// Draw the footer with stats.
  fn draw_footer(&mut self) {
    let footer_height = 24;
    let footer_y = self.renderer.height() as i32 - footer_height;
    let padding = 8;
    let width = self.renderer.width() as i32;

    self
      .renderer
      .fill_rect(0, footer_y, width, footer_height, colors::HEADER_BG);
    self
      .renderer
      .fill_rect(0, footer_y, width, 1, colors::DIVIDER);

    // Count by level
    let (trace, debug, info, warn, error) =
      self
        .cached_entries
        .iter()
        .fold((0, 0, 0, 0, 0), |(t, d, i, w, e), entry| {
          match entry.level {
            LogLevel::Trace => (t + 1, d, i, w, e),
            LogLevel::Debug => (t, d + 1, i, w, e),
            LogLevel::Info => (t, d, i + 1, w, e),
            LogLevel::Warn => (t, d, i, w + 1, e),
            LogLevel::Error => (t, d, i, w, e + 1),
          }
        });

    // Show stats or hint based on state
    let (footer_text, text_color) = if self.selection.has_selection() {
      (
        "Ctrl+C to copy selection | Right-click to copy".to_string(),
        [180, 220, 180, 255],
      )
    } else if self.hovered_entry.is_some() {
      (
        "Drag to select | Right-click to copy line".to_string(),
        [180, 180, 220, 255],
      )
    } else {
      (
        format!(
          "Total: {} | T:{} D:{} I:{} W:{} E:{}",
          self.cached_entries.len(),
          trace,
          debug,
          info,
          warn,
          error
        ),
        colors::TEXT_DIM,
      )
    };
    self
      .renderer
      .draw_text(&footer_text, padding, footer_y + 6, text_color);

    // Show scroll status
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
