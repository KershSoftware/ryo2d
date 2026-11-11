//! Drawing the console: its window, its input line and its output, each
//! line wrapped once for the width it is drawn at.

use super::{
  CLOSE_BUTTON_SIZE, Console, DragState, RESIZE_HANDLE_SIZE, SCROLLBAR_MIN_THUMB, SCROLLBAR_WIDTH,
  colors, console_style,
};
use crate::core::text::TextStyle;
use crate::core::ui::{Color, Rect, UiContext};
use glam::Vec2;

/// The input line's prompt
const PROMPT: &str = "> ";

impl Console {
  /// Render the console window
  pub fn render(&mut self, ui: &mut UiContext, frame_width: u32, frame_height: u32) {
    if !self.visible {
      return;
    }

    let padding = 8.0;
    let style = console_style();
    let line_height = ui.line_height(&style).ceil();
    self.line_height = line_height;
    let title_height = self.title_bar_height();
    let input_height = self.input_height();

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

    // Clamp window position to screen bounds, and keep it there, so the
    // hit tests find the window where it is drawn after the frame shrinks
    let x = self
      .x
      .max(0.0)
      .min((frame_width as f32 - self.width).max(0.0));
    let y = self
      .y
      .max(0.0)
      .min((frame_height as f32 - self.height).max(0.0));
    self.x = x;
    self.y = y;

    // Draw window background
    let bg_rect = Rect::new(x, y, self.width, self.height);
    ui.draw_rect(bg_rect, bg_color);

    // Draw border
    ui.draw_rect_outline(bg_rect, border_color, 1.0);

    // Draw title bar
    let title_rect = Rect::new(x, y, self.width, title_height);
    ui.draw_rect(title_rect, title_bar_color);
    draw_line(
      ui,
      "Developer Console",
      x + padding,
      y + (title_height - line_height) / 2.0,
      text_color,
    );

    // Draw close button (X) in top-right corner
    let close_padding = (title_height - CLOSE_BUTTON_SIZE) / 2.0;
    let close_x = x + self.width - CLOSE_BUTTON_SIZE - close_padding;
    let close_y = y + close_padding;
    let close_rect = Rect::new(close_x, close_y, CLOSE_BUTTON_SIZE, CLOSE_BUTTON_SIZE);
    let (mouse_x, mouse_y) = ui.input().mouse_position();
    let close_hovered = close_rect.contains(mouse_x, mouse_y);
    let close_btn_color = if close_hovered && self.focused {
      Color::new(140, 50, 50, 255)
    } else if self.focused {
      Color::new(80, 40, 40, 255)
    } else {
      Color::new(80, 40, 40, 160)
    };
    ui.draw_rect(close_rect, close_btn_color);
    // Draw X centered in the button
    let (x_size, prompt_width) = self.marks(ui, &style);
    let x_text_x = close_x + (CLOSE_BUTTON_SIZE - x_size.x) / 2.0;
    let x_text_y = close_y + (CLOSE_BUTTON_SIZE - x_size.y) / 2.0;
    let close_text_color = if close_hovered && self.focused {
      Color::WHITE
    } else {
      text_color
    };
    draw_line(ui, "X", x_text_x, x_text_y, close_text_color);

    // Draw input line background
    let input_y = y + self.height - input_height;
    let input_rect = Rect::new(x, input_y, self.width, input_height);
    ui.draw_rect(input_rect, input_bg_color);

    // Draw input prompt and text
    let prompt = PROMPT;
    let prompt_x = x + padding;
    let input_text_y = input_y + (input_height - line_height) / 2.0;
    let prompt_color = if self.focused {
      colors::SYSTEM
    } else {
      Color::new(colors::SYSTEM.r, colors::SYSTEM.g, colors::SYSTEM.b, 160)
    };
    draw_line(ui, prompt, prompt_x, input_text_y, prompt_color);

    let text_x = prompt_x + prompt_width;
    // The selection, behind the text; its edges are measured as the text is
    if self.focused
      && let Some(range) = self.edit.selection()
      && let (Some(before), Some(selected)) = (
        self.input_buffer.get(..range.start),
        self.input_buffer.get(range),
      )
    {
      let start_x = text_x + ui.measure_text(before, &style).x;
      let width = ui.measure_text(selected, &style).x;
      let rect = Rect::new(start_x, input_y + 4.0, width, input_height - 8.0);
      ui.draw_rect(rect, colors::SELECTION);
    }
    draw_line(ui, &self.input_buffer, text_x, input_text_y, text_color);

    // Draw cursor (blinking) - only when focused
    if self.focused && self.edit.caret_shown() {
      let cursor_text = self.input_buffer.get(..self.edit.cursor()).unwrap_or("");
      let cursor_x = text_x + ui.measure_text(cursor_text, &style).x;
      let cursor_rect = Rect::new(cursor_x, input_y + 4.0, 2.0, input_height - 8.0);
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
    let output_top = y + title_height + padding;
    let output_bottom = input_y - padding;
    let output_height = output_bottom - output_top;
    let visible_line_count = (output_height / line_height) as usize;

    // Wrap at the full width; the scrollbar takes room from the text only
    // when the rows overflow, so wrap narrower then (and back once the
    // narrower rows fit). Only lines new since the last frame are wrapped,
    // unless the width changed.
    let full = self.max_text_width(false);
    let narrow = self.max_text_width(true);
    let mut text_width = if self.wrap_width == narrow {
      narrow
    } else {
      full
    };
    self.wrap(ui, text_width, &style);
    if text_width == full && self.total_rows() > visible_line_count {
      text_width = narrow;
      self.wrap(ui, text_width, &style);
    } else if text_width == narrow && self.total_rows() <= visible_line_count {
      text_width = full;
      self.wrap(ui, text_width, &style);
    }

    // Lines printed while the reader was scrolled up push the rows they
    // wrap to under the view: move the view by those, so what is read stays
    let unseen = std::mem::take(&mut self.unseen_lines).min(self.output_lines.len());
    if unseen > 0 && self.scroll_target > 0.0 {
      let rows: usize = self
        .output_lines
        .iter()
        .rev()
        .take(unseen)
        .map(|line| line.rows.len())
        .sum();
      self.scroll_offset += rows as f32;
      self.scroll_target += rows as f32;
    }

    let total_lines = self.total_rows();
    let has_scrollbar = total_lines > visible_line_count;
    let max_scroll = (total_lines as f32 - output_height / line_height).max(0.0);
    let scroll_offset = self.scroll_offset.clamp(0.0, max_scroll);
    self.scroll_target = self.scroll_target.clamp(0.0, max_scroll);
    self.scroll_offset = scroll_offset;

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
      let scroll_ratio = if max_scroll > 0.0 {
        (scroll_offset / max_scroll).min(1.0)
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

    // Draw the output at the fractional position, bottom-aligned: the view
    // is `scroll_offset` lines above the newest row, and the lines cut by
    // the output area's edges are clipped, not skipped
    let first = (total_lines as f32 - scroll_offset - output_height / line_height)
      .floor()
      .max(0.0) as usize;
    let last = ((total_lines as f32 - scroll_offset).ceil().max(0.0) as usize).min(total_lines);
    ui.push_clip(Rect::new(x, output_top, self.width, output_height));
    // Lines from the newest back, each over its rows, until the first row
    // in view
    let mut end = total_lines;
    for line in self.output_lines.iter().rev() {
      if end <= first {
        break;
      }
      let start = end - line.rows.len();
      let line_color = if self.focused {
        line.color
      } else {
        Color::new(
          line.color.r,
          line.color.g,
          line.color.b,
          (line.color.a as u16 * 160 / 255) as u8,
        )
      };
      for (i, row) in (start..end).zip(&line.rows) {
        if i >= first && i < last {
          let text_y = output_bottom + (scroll_offset - (total_lines - i) as f32) * line_height;
          draw_line(ui, row, x + padding, text_y, line_color);
        }
      }
      end = start;
    }
    ui.pop_clip();
  }

  /// The close button's `X` and the input prompt's width, measured again
  /// only when the UI scale changes
  fn marks(&mut self, ui: &UiContext, style: &TextStyle) -> (Vec2, f32) {
    let scale = ui.ui_scale();
    if let Some((at, x, prompt)) = self.marks
      && at == scale
    {
      return (x, prompt);
    }
    let x = ui.measure_text("X", style);
    let prompt = ui.measure_text(PROMPT, style).x;
    self.marks = Some((scale, x, prompt));
    (x, prompt)
  }

  /// Wrap the lines not yet wrapped at `width` (every line when the width
  /// or the UI scale changed), keeping the row count
  fn wrap(&mut self, ui: &UiContext, width: f32, style: &TextStyle) {
    let scale = ui.ui_scale();
    if width != self.wrap_width || scale != self.wrap_scale {
      self.wrap_width = width;
      self.wrap_scale = scale;
      self.wrapped_lines = 0;
      self.wrapped_row_count = 0;
    }
    for line in self.output_lines.range_mut(self.wrapped_lines..) {
      line.rows = ui.wrap_text_styled(&line.text, width, style);
      // An empty line is still a row
      if line.rows.is_empty() {
        line.rows.push(String::new());
      }
      self.wrapped_row_count += line.rows.len();
      #[cfg(test)]
      {
        self.wrap_calls += 1;
      }
    }
    self.wrapped_lines = self.output_lines.len();
  }
}

/// One line of console text; `color`'s alpha fades it (layouts are cached
/// per colour, so alpha rides as opacity)
fn draw_line(ui: &mut UiContext, text: &str, x: f32, y: f32, color: Color) {
  ui.text(text)
    .style(&console_style().color(color.with_alpha(255)))
    .at(x, y)
    .opacity(f32::from(color.a) / 255.0)
    .draw();
}

#[cfg(all(test, feature = "default-fonts"))]
mod tests {
  use super::super::tests::{console_frame, scrolled_units, scrolling_console};
  use super::*;
  use crate::core::input::Input;
  use winit::event::MouseButton;

  #[test]
  fn long_output_lines_wrap_to_the_console_width() {
    use crate::core::ui::immediate::{UiCommand, UiCommands};
    let mut console = Console::new();
    console.toggle();
    console.print("word ".repeat(400));
    let (mut commands, mut text) = (
      UiCommands::default(),
      crate::core::ui::test_util::text_engine(),
    );
    let mut ui = crate::core::ui::test_util::context(&mut commands, &mut text, (1280.0, 720.0));
    console.render(&mut ui, 1280, 720);
    drop(ui);
    let output_width = console.max_text_width(true);
    let widths: Vec<f32> = commands
      .commands()
      .filter_map(|c| match c {
        UiCommand::Text { layout, .. } => Some(layout.size.x / layout.scale),
        _ => None,
      })
      .collect();
    assert!(widths.len() > 3, "the long line became several rows");
    assert!(
      widths.iter().all(|&w| w <= output_width + 0.5),
      "{widths:?} within {output_width}"
    );
  }

  #[test]
  fn the_console_draws_in_monospace_and_sizes_rows_by_the_font() {
    use crate::core::ui::immediate::{UiCommand, UiCommands};
    let mut console = Console::new();
    console.toggle();
    console.clear();
    console.print("iiiiiiiiii");
    console.print("MMMMMMMMMM");
    let (mut commands, mut text) = (
      UiCommands::default(),
      crate::core::ui::test_util::text_engine(),
    );
    let mut ui = crate::core::ui::test_util::context(&mut commands, &mut text, (1280.0, 720.0));
    console.render(&mut ui, 1280, 720);
    let expected = ui.line_height(&console_style()).ceil();
    drop(ui);
    let widths: Vec<f32> = commands
      .commands()
      .filter_map(|c| match c {
        UiCommand::Text { layout, .. } if layout.size.x > 60.0 && layout.size.x < 100.0 => {
          Some(layout.size.x)
        }
        _ => None,
      })
      .collect();
    assert_eq!(widths.len(), 2, "{widths:?}");
    assert!((widths[0] - widths[1]).abs() < 0.5, "monospace: {widths:?}");
    assert_eq!(console.line_height, expected);
    assert!(console.title_bar_height() >= expected + 10.0);
  }

  #[test]
  fn the_selection_is_drawn_behind_its_text() {
    use crate::core::ui::immediate::{UiCommand, UiCommands};
    let line = "a\u{e9}\u{1F600}b";
    let draw = |selected: std::ops::Range<usize>| {
      let mut console = Console::new();
      console.toggle();
      console.input_buffer = line.to_string();
      console.edit.set_cursor(line, selected.start, false);
      console.edit.set_cursor(line, selected.end, true);
      let (mut commands, mut text) = (
        UiCommands::default(),
        crate::core::ui::test_util::text_engine(),
      );
      let mut ui = crate::core::ui::test_util::context(&mut commands, &mut text, (1280.0, 720.0));
      console.render(&mut ui, 1280, 720);
      drop(ui);
      let drawn: Vec<UiCommand> = commands.commands().cloned().collect();
      drawn
    };
    let drawn = draw(1..3);
    let rect_at = drawn
      .iter()
      .position(|c| matches!(c, UiCommand::Rect { fill, .. } if *fill == colors::SELECTION));
    let text_at = drawn
      .iter()
      .position(|c| matches!(c, UiCommand::Text { layout, .. } if layout.text() == line));
    let (Some(rect_at), Some(text_at)) = (rect_at, text_at) else {
      panic!("a selection rectangle and the line's text are drawn");
    };
    assert!(rect_at < text_at, "behind the text");
    // `\u{e9}` is two bytes but one monospace cell, as wide as `a`
    let UiCommand::Rect { rect, .. } = &drawn[rect_at] else {
      unreachable!()
    };
    let UiCommand::Text { origin, .. } = &drawn[text_at] else {
      unreachable!()
    };
    assert!(rect.x > origin.x, "it starts after the `a`");
    assert!(rect.width > 0.0);
    // Nothing is drawn for no selection
    let none = draw(2..2);
    assert!(
      !none
        .iter()
        .any(|c| { matches!(c, UiCommand::Rect { fill, .. } if *fill == colors::SELECTION) })
    );
  }

  /// Draw `console` once into a `width` x `height` UI
  fn render_once(
    console: &mut Console,
    text: &mut crate::core::text::TextEngine,
    size: (u32, u32),
  ) {
    let mut commands = crate::core::ui::immediate::UiCommands::default();
    let mut ui =
      crate::core::ui::test_util::context(&mut commands, text, (size.0 as f32, size.1 as f32));
    console.render(&mut ui, size.0, size.1);
  }

  #[test]
  fn a_held_open_console_wraps_each_line_once() {
    let mut console = Console::new();
    console.toggle();
    console.clear();
    for i in 0..1000 {
      console.print(format!("line {i} {}", "word ".repeat(i % 30)));
    }
    let mut text = crate::core::ui::test_util::text_engine();
    for _ in 0..60 {
      render_once(&mut console, &mut text, (1280, 720));
    }
    // Once at the full width, and once more narrower when the scrollbar came
    assert!(
      matches!(console.wrap_calls, 1000 | 2000),
      "{} wraps",
      console.wrap_calls
    );
  }

  #[test]
  fn a_resize_wraps_again_and_new_lines_wrap_alone() {
    let mut console = Console::new();
    console.toggle();
    console.clear();
    for i in 0..5 {
      console.print(format!("line {i}"));
    }
    let mut text = crate::core::ui::test_util::text_engine();
    render_once(&mut console, &mut text, (1280, 720));
    assert_eq!(console.wrap_calls, 5);
    console.print("six");
    console.print("seven");
    render_once(&mut console, &mut text, (1280, 720));
    render_once(&mut console, &mut text, (1280, 720));
    assert_eq!(console.wrap_calls, 7, "only the new lines");
    console.width += 40.0;
    render_once(&mut console, &mut text, (1280, 720));
    assert_eq!(console.wrap_calls, 14, "every line at the new width");
    assert_eq!(console.total_rows(), 7);
  }

  #[test]
  fn a_console_past_a_shrunk_window_is_hit_where_it_is_drawn() {
    let mut console = Console::new();
    console.toggle();
    let mut text = crate::core::ui::test_util::text_engine();
    console.x = 900.0;
    render_once(&mut console, &mut text, (1280, 720));
    assert_eq!(console.window_rect().x, 1280.0 - console.width);
    // The window shrinks: the console is drawn, and hit, at its new edge
    render_once(&mut console, &mut text, (800, 600));
    assert_eq!(console.window_rect().x, 800.0 - console.width);
    let close = console.close_button_rect();
    let mut input = Input::new();
    input.handle_mouse_move(
      f64::from(close.x + close.width / 2.0),
      f64::from(close.y + close.height / 2.0),
    );
    input.queue(crate::core::input::InputEvent::MouseButton {
      button: MouseButton::Left,
      pressed: true,
    });
    input.begin_frame();
    console.handle_input(&mut input);
    assert!(
      !console.visible,
      "the close button drawn is the one clicked"
    );
  }

  /// Render the console and give each drawn row's y by its text's seed
  fn drawn_rows(console: &mut Console) -> std::collections::HashMap<u64, f32> {
    use crate::core::ui::immediate::{UiCommand, UiCommands};
    let (mut commands, mut text) = (
      UiCommands::default(),
      crate::core::ui::test_util::text_engine(),
    );
    let mut ui = crate::core::ui::test_util::context(&mut commands, &mut text, (1280.0, 720.0));
    console.render(&mut ui, 1280, 720);
    drop(ui);
    let (top, bottom, _, _) = console.output_area();
    commands
      .commands()
      .filter_map(|c| match c {
        UiCommand::Text { origin, seed, .. } if origin.y > top - 20.0 && origin.y < bottom => {
          Some((*seed, origin.y))
        }
        _ => None,
      })
      .collect()
  }

  #[test]
  fn new_output_keeps_a_console_at_the_bottom_and_a_reader_in_place() {
    let (mut console, mut input) = scrolling_console(50);
    console.print("more");
    assert_eq!(scrolled_units(&console), 0.0, "at the bottom it stays");

    drawn_rows(&mut console);
    console_frame(&mut console, &mut input, 0.0, 30.0);
    let before = drawn_rows(&mut console);
    // A line that wraps to several rows must not move what is being read
    console.print("word ".repeat(300));
    let after = drawn_rows(&mut console);
    let kept: Vec<_> = before
      .iter()
      .filter_map(|(seed, y)| after.get(seed).map(|y2| (*y, *y2)))
      .collect();
    assert!(kept.len() > 5, "the rows being read are still drawn");
    for (y, y2) in kept {
      assert!((y - y2).abs() < 0.01, "a row moved from {y} to {y2}");
    }
  }

  #[test]
  fn the_scrollbar_answers_to_the_wrapped_rows_it_draws() {
    let (mut console, mut input) = scrolling_console(0);
    for _ in 0..8 {
      console.print("word ".repeat(120));
    }
    drawn_rows(&mut console);
    let (_, _, view, _) = console.output_area();
    assert!(console.output_lines.len() as f32 * console.line_height < view);
    assert!(console.total_rows() as f32 * console.line_height > view);
    // Click the track near its top: the view jumps up
    let track = console.scrollbar_track_rect();
    input.handle_mouse_move(f64::from(track.x + 2.0), f64::from(track.y + 2.0));
    input.queue(crate::core::input::InputEvent::MouseButton {
      button: MouseButton::Left,
      pressed: true,
    });
    input.begin_frame();
    console.handle_input(&mut input);
    assert!(console.scroll_offset > 0.0, "the click found the scrollbar");
  }

  #[test]
  fn a_fractional_scroll_draws_the_cut_lines_clipped() {
    use crate::core::ui::immediate::{UiCommand, UiCommands};
    let rows_at = |lines: f32| {
      let (mut console, _) = scrolling_console(60);
      console.scroll_offset = lines;
      let (mut commands, mut text) = (
        UiCommands::default(),
        crate::core::ui::test_util::text_engine(),
      );
      let mut ui = crate::core::ui::test_util::context(&mut commands, &mut text, (1280.0, 720.0));
      console.render(&mut ui, 1280, 720);
      drop(ui);
      let (top, bottom, _, _) = console.output_area();
      let ys: Vec<f32> = commands
        .commands()
        .filter_map(|c| match c {
          UiCommand::Text { origin, clip, .. } if origin.y > top - 20.0 && origin.y < bottom => {
            assert_eq!(clip.map(|c| (c.y, c.height)), Some((top, bottom - top)));
            Some(origin.y)
          }
          _ => None,
        })
        .collect();
      (ys, console.line_height)
    };
    let (still, line) = rows_at(0.0);
    let (moved, _) = rows_at(0.5);
    let last = |ys: &[f32]| ys.iter().copied().fold(f32::MIN, f32::max);
    assert!(
      (last(&moved) - last(&still) - 0.5 * line).abs() < 0.01,
      "rows move by half a line: {still:?} {moved:?}"
    );
    assert!(
      moved.len() > still.len(),
      "a cut row at the top is drawn too"
    );
  }
}
