//! The shell's drawing: top bar, description, settings panel, legend and help
//!
//! Text is ASCII only: the bitmap font covers characters 32-126.

use ryo2d::prelude::*;

use crate::Control;
use crate::settings::Row;

/// Everything the shell draws for one frame
pub(crate) struct View<'a> {
  pub title: &'a str,
  pub index: usize,
  pub count: usize,
  pub page_name: &'a str,
  pub description: &'a str,
  pub rows: &'a [Row],
  pub selected: usize,
  pub controls: &'a [Control],
  pub help_open: bool,
}

/// Where the shell's parts go on a screen
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Layout {
  pub line_h: f32,
  pub pad: f32,
  pub top_bar: Rect,
  pub description_y: f32,
  /// Below the description, above the legend, full width
  pub content: Rect,
}

pub(crate) fn layout(screen_w: f32, screen_h: f32, line_h: f32) -> Layout {
  let pad = (line_h * 0.5).round().max(2.0);
  let bar_h = line_h + 2.0 * pad;
  let description_y = bar_h + pad;
  let content_y = description_y + line_h + pad;
  let legend_y = screen_h - pad - line_h;
  Layout {
    line_h,
    pad,
    top_bar: Rect::new(0.0, 0.0, screen_w, bar_h),
    description_y,
    content: Rect::new(
      pad,
      content_y,
      (screen_w - 2.0 * pad).max(0.0),
      (legend_y - pad - content_y).max(0.0),
    ),
  }
}

/// The area a page can use: below the shell's top bar and description and
/// above its legend. The settings panel, when shown, covers its right side.
pub fn content_area(ui: &UiContext) -> Rect {
  let (w, h) = ui.screen_size();
  layout(w as f32, h as f32, line_height(ui)).content
}

fn line_height(ui: &UiContext) -> f32 {
  ui.text_size("Ag").1 as f32
}

fn text_w(ui: &UiContext, text: &str) -> f32 {
  ui.text_size(text).0 as f32
}

/// Group items, in order, into lines no wider than `available`
pub(crate) fn wrap_items(widths: &[f32], gap: f32, available: f32) -> Vec<Vec<usize>> {
  let mut lines: Vec<Vec<usize>> = Vec::new();
  let mut used = 0.0;
  for (i, &width) in widths.iter().enumerate() {
    match lines.last_mut() {
      Some(line) if used + gap + width <= available => {
        line.push(i);
        used += gap + width;
      }
      _ => {
        lines.push(vec![i]);
        used = width;
      }
    }
  }
  lines
}

/// `text`, shortened with `...` until `width` says it fits in `max`
pub(crate) fn truncate_to(text: &str, max: f32, width: impl Fn(&str) -> f32) -> String {
  if width(text) <= max {
    return text.to_string();
  }
  let mut chars: Vec<char> = text.chars().collect();
  while !chars.is_empty() {
    chars.pop();
    let candidate: String = chars.iter().collect::<String>() + "...";
    if width(&candidate) <= max {
      return candidate;
    }
  }
  String::new()
}

fn bar_color() -> Color {
  Color::new(12, 12, 20, 225)
}
fn panel_color() -> Color {
  Color::new(12, 12, 20, 205)
}
fn highlight_color() -> Color {
  Color::new(60, 60, 95, 230)
}
fn dim_text() -> Color {
  Color::new(170, 170, 185, 255)
}
fn key_text() -> Color {
  Color::YELLOW
}

pub(crate) fn draw(ui: &mut UiContext, view: &View) {
  let (w, h) = ui.screen_size();
  let (w, h) = (w as f32, h as f32);
  let l = layout(w, h, line_height(ui));
  draw_top_bar(ui, view, &l, w);
  draw_description(ui, view, &l, w);
  draw_panel(ui, view, &l, w);
  draw_legend(ui, view, &l, w, h);
  if view.help_open {
    draw_help(ui, view, &l, w, h);
  }
}

fn draw_top_bar(ui: &mut UiContext, view: &View, l: &Layout, w: f32) {
  ui.draw_rect(l.top_bar, bar_color());
  let left = if view.count > 1 {
    format!(
      "{} - {}/{} {}",
      view.title,
      view.index + 1,
      view.count,
      view.page_name
    )
  } else {
    view.title.to_string()
  };
  let right = if view.count > 1 {
    "[ ] page   H help   Esc quit"
  } else {
    "H help   Esc quit"
  };
  let y = l.pad;
  ui.draw_text(&left, l.pad, y, Color::WHITE);
  let right_w = text_w(ui, right);
  let right_x = w - l.pad - right_w;
  // Only when it clears the left text
  if right_x > l.pad + text_w(ui, &left) + l.pad {
    ui.draw_text(right, right_x, y, dim_text());
  }
}

fn draw_description(ui: &mut UiContext, view: &View, l: &Layout, w: f32) {
  let first = view.description.lines().next().unwrap_or("");
  if first.is_empty() {
    return;
  }
  let line = truncate_to(first, w - 2.0 * l.pad, |s| text_w(ui, s));
  ui.draw_text(&line, l.pad, l.description_y, dim_text());
}

fn draw_panel(ui: &mut UiContext, view: &View, l: &Layout, w: f32) {
  if view.rows.is_empty() {
    return;
  }
  let gap = l.pad * 2.0;
  let hint = "Up/Down pick  Left/Right set";
  let widest_row = view
    .rows
    .iter()
    .map(|row| text_w(ui, "> ") + text_w(ui, &row.label) + gap + text_w(ui, &row.value))
    .fold(0.0, f32::max);
  let panel_w = (widest_row.max(text_w(ui, hint)) + 2.0 * l.pad).min(w * 0.45);
  let row_h = l.line_h + l.pad * 0.5;
  // Header + rows + hint, within the content area
  let wanted_h = (view.rows.len() as f32 + 2.0) * row_h + 2.0 * l.pad;
  let panel_h = wanted_h.min(l.content.height);
  let x = w - l.pad - panel_w;
  let y = l.content.y;
  if panel_h < row_h * 2.0 || panel_w <= 0.0 {
    return;
  }
  ui.draw_rect(Rect::new(x, y, panel_w, panel_h), panel_color());
  ui.draw_text("SETTINGS", x + l.pad, y + l.pad, Color::WHITE);

  let rows_fit = ((panel_h - 2.0 * l.pad) / row_h).floor() as usize;
  let visible = rows_fit.saturating_sub(2).min(view.rows.len());
  // Keep the selected row in view
  let first = view.selected.saturating_sub(visible.saturating_sub(1));
  let inner = panel_w - 2.0 * l.pad;
  for (slot, (i, row)) in view
    .rows
    .iter()
    .enumerate()
    .skip(first)
    .take(visible)
    .enumerate()
  {
    let row_y = y + l.pad + row_h * (slot as f32 + 1.0);
    let selected = i == view.selected;
    if selected {
      ui.draw_rect(
        Rect::new(x + 2.0, row_y - l.pad * 0.25, panel_w - 4.0, row_h),
        highlight_color(),
      );
    }
    let marker = if selected { "> " } else { "  " };
    let label = truncate_to(&format!("{marker}{}", row.label), inner * 0.6, |s| {
      text_w(ui, s)
    });
    ui.draw_text(
      &label,
      x + l.pad,
      row_y,
      if selected { key_text() } else { Color::WHITE },
    );
    let value = truncate_to(&row.value, inner * 0.4, |s| text_w(ui, s));
    let value_x = x + panel_w - l.pad - text_w(ui, &value);
    ui.draw_text(&value, value_x, row_y, Color::WHITE);
  }
  let hint_y = y + panel_h - l.pad - l.line_h;
  let hint = truncate_to(hint, inner, |s| text_w(ui, s));
  ui.draw_text(&hint, x + l.pad, hint_y, dim_text());
}

/// Legend items: the page's controls, then `R reset`
fn legend_items(view: &View) -> Vec<(String, String)> {
  let mut items: Vec<(String, String)> = view
    .controls
    .iter()
    .map(|control| (control.label.clone(), control.description.clone()))
    .collect();
  items.push(("R".to_string(), "reset".to_string()));
  items
}

fn draw_legend(ui: &mut UiContext, view: &View, l: &Layout, w: f32, h: f32) {
  let items = legend_items(view);
  let gap = l.pad * 3.0;
  let widths: Vec<f32> = items
    .iter()
    .map(|(key, desc)| text_w(ui, key) + text_w(ui, " ") + text_w(ui, desc))
    .collect();
  // Leave the bottom-left corner to the engine's FPS counter
  let fps_reserve = text_w(ui, "FPS: 0000") + 2.0 * l.pad;
  let available = (w - 2.0 * l.pad - fps_reserve).max(1.0);
  let lines = wrap_items(&widths, gap, available);
  let line_step = l.line_h + l.pad * 0.5;
  for (n, line) in lines.iter().enumerate() {
    let from_bottom = (lines.len() - 1 - n) as f32;
    let y = h - l.pad - l.line_h - from_bottom * line_step;
    if y < l.content.y {
      continue;
    }
    let line_w: f32 =
      line.iter().map(|&i| widths[i]).sum::<f32>() + gap * line.len().saturating_sub(1) as f32;
    let mut x = (w - l.pad - line_w).max(l.pad);
    for &i in line {
      let (key, desc) = &items[i];
      ui.draw_text(key, x, y, key_text());
      let desc_x = x + text_w(ui, key) + text_w(ui, " ");
      ui.draw_text(desc, desc_x, y, dim_text());
      x += widths[i] + gap;
    }
  }
}

fn draw_help(ui: &mut UiContext, view: &View, l: &Layout, w: f32, h: f32) {
  ui.draw_modal_overlay(Color::new(0, 0, 0, 170));
  let box_w = (w - 4.0 * l.pad).max(0.0);
  let box_h = (h - 4.0 * l.pad).max(0.0);
  let (x, y) = (2.0 * l.pad, 2.0 * l.pad);
  ui.draw_rect(Rect::new(x, y, box_w, box_h), panel_color());

  let mut lines: Vec<(String, Color)> = Vec::new();
  let page = if view.count > 1 {
    format!(
      "{} - page {}/{}: {}",
      view.title,
      view.index + 1,
      view.count,
      view.page_name
    )
  } else {
    format!("{}: {}", view.title, view.page_name)
  };
  lines.push((page, Color::WHITE));
  for line in view.description.lines() {
    lines.push((line.to_string(), dim_text()));
  }
  lines.push((String::new(), Color::WHITE));
  lines.push(("Demo keys".to_string(), key_text()));
  if view.count > 1 {
    lines.push(("  [ ]  previous / next page".to_string(), Color::WHITE));
  }
  lines.push((
    "  Up/Down  pick a setting   Left/Right  change it".to_string(),
    Color::WHITE,
  ));
  lines.push((
    "  R  reset this page   H  help   Esc  close help / quit".to_string(),
    Color::WHITE,
  ));
  if !view.controls.is_empty() {
    lines.push((String::new(), Color::WHITE));
    lines.push(("This page".to_string(), key_text()));
    for control in view.controls {
      lines.push((
        format!("  {}  {}", control.label, control.description),
        Color::WHITE,
      ));
    }
  }
  if !view.rows.is_empty() {
    lines.push((String::new(), Color::WHITE));
    lines.push(("Settings".to_string(), key_text()));
    for row in view.rows {
      lines.push((format!("  {}  {}", row.label, row.range), Color::WHITE));
    }
  }

  let inner = box_w - 2.0 * l.pad;
  let step = l.line_h + l.pad * 0.5;
  let mut line_y = y + l.pad;
  for (text, color) in lines {
    if line_y + l.line_h > y + box_h - l.pad {
      break;
    }
    let text = truncate_to(&text, inner, |s| text_w(ui, s));
    ui.draw_text(&text, x + l.pad, line_y, color);
    line_y += step;
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn layout_stacks_bar_description_content_and_legend() {
    let l = layout(800.0, 600.0, 10.0);
    assert_eq!(l.top_bar, Rect::new(0.0, 0.0, 800.0, 20.0));
    assert_eq!(l.description_y, 25.0);
    assert_eq!(l.content.y, 40.0);
    assert_eq!(l.content.x, 5.0);
    // The legend line sits at 600 - 5 - 10 = 585; content ends a pad above it
    assert_eq!(l.content.y + l.content.height, 580.0);
  }

  #[test]
  fn layout_never_goes_negative_on_a_tiny_screen() {
    let l = layout(40.0, 30.0, 10.0);
    assert!(l.content.width >= 0.0 && l.content.height >= 0.0);
  }

  #[test]
  fn items_wrap_into_lines_that_fit() {
    assert_eq!(
      wrap_items(&[30.0, 30.0, 30.0], 10.0, 100.0),
      vec![vec![0, 1], vec![2]]
    );
    assert_eq!(wrap_items(&[30.0, 30.0], 10.0, 70.0), vec![vec![0, 1]]);
    // An item wider than the space still gets a line of its own
    assert_eq!(
      wrap_items(&[500.0, 10.0], 10.0, 100.0),
      vec![vec![0], vec![1]]
    );
    assert!(wrap_items(&[], 10.0, 100.0).is_empty());
  }

  #[test]
  fn truncation_keeps_text_within_the_width() {
    let width = |s: &str| s.len() as f32 * 8.0;
    assert_eq!(truncate_to("short", 100.0, width), "short");
    let cut = truncate_to("a long description line", 80.0, width);
    assert!(width(&cut) <= 80.0, "{cut}");
    assert!(cut.ends_with("..."), "{cut}");
  }
}
