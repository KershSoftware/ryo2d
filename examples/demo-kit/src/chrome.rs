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

/// Most legend lines; help lists every control
const MAX_LEGEND_LINES: usize = 3;

/// Legend items in at most `max_lines` lines of `available` width. When they
/// don't all fit, the last line keeps room for a "+N more" item `more_w(N)`
/// wide; returns the lines and how many items were left out.
pub(crate) fn fit_legend(
  widths: &[f32],
  gap: f32,
  available: f32,
  max_lines: usize,
  more_w: impl Fn(usize) -> f32,
) -> (Vec<Vec<usize>>, usize) {
  let mut lines = wrap_items(widths, gap, available);
  if lines.len() <= max_lines {
    return (lines, 0);
  }
  lines.truncate(max_lines.max(1));
  let before_last: usize = lines[..lines.len() - 1].iter().map(Vec::len).sum();
  let hidden = match lines.last_mut() {
    Some(last) => loop {
      let hidden = widths.len() - before_last - last.len();
      let used: f32 =
        last.iter().map(|&i| widths[i]).sum::<f32>() + gap * last.len().saturating_sub(1) as f32;
      let more = if last.is_empty() { 0.0 } else { gap } + more_w(hidden);
      if used + more <= available || last.is_empty() {
        break hidden;
      }
      last.pop();
    },
    None => 0,
  };
  (lines, hidden)
}

/// How many of `total` help lines to draw when `capacity` fit, and how many
/// a final "N more" line stands for when they don't all fit
pub(crate) fn help_fit(total: usize, capacity: usize) -> (usize, Option<usize>) {
  if total <= capacity || capacity == 0 {
    return (total.min(capacity), None);
  }
  let shown = capacity - 1;
  (shown, Some(total - shown))
}

/// The top bar's title width limit and whether the shell keys show: they
/// show only beside the whole title, and the title may use the whole bar
/// otherwise
pub(crate) fn top_bar_fit(title_w: f32, keys_w: f32, room: f32, gap: f32) -> (f32, bool) {
  (room, title_w + gap + keys_w <= room)
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
/// Behind free-standing text so it reads on any page
fn backing_color() -> Color {
  Color::new(12, 12, 20, 170)
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

/// Draw the shell; returns where the settings panel went, if it was drawn
pub(crate) fn draw(ui: &mut UiContext, view: &View) -> Option<Rect> {
  let (w, h) = ui.screen_size();
  let (w, h) = (w as f32, h as f32);
  let l = layout(w, h, line_height(ui));
  draw_top_bar(ui, view, &l, w);
  draw_description(ui, view, &l, w);
  let panel = draw_panel(ui, view, &l, w);
  draw_legend(ui, view, &l, w, h);
  if view.help_open {
    draw_help(ui, view, &l, w, h);
  }
  panel
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
  let right_w = text_w(ui, right);
  let (left_max, show_right) = top_bar_fit(text_w(ui, &left), right_w, w - 2.0 * l.pad, l.pad);
  let left = truncate_to(&left, left_max, |s| text_w(ui, s));
  ui.draw_text(&left, l.pad, y, Color::WHITE);
  if show_right {
    ui.draw_text(right, w - l.pad - right_w, y, dim_text());
  }
}

fn draw_description(ui: &mut UiContext, view: &View, l: &Layout, w: f32) {
  let first = view.description.lines().next().unwrap_or("");
  if first.is_empty() {
    return;
  }
  let line = truncate_to(first, w - 2.0 * l.pad, |s| text_w(ui, s));
  let backing = Rect::new(
    l.pad / 2.0,
    l.description_y - l.pad / 2.0,
    text_w(ui, &line) + l.pad,
    l.line_h + l.pad,
  );
  ui.draw_rect(backing, backing_color());
  ui.draw_text(&line, l.pad, l.description_y, dim_text());
}

/// Widths a row's label and value may use within `inner`, separated by `gap`.
/// The value keeps its full width up to half of `inner`; the label gets the rest.
pub(crate) fn row_widths(inner: f32, value_w: f32, gap: f32) -> (f32, f32) {
  let value_max = value_w.min(inner * 0.5);
  let label_max = (inner - value_max - gap).max(0.0);
  (label_max, value_max)
}

fn draw_panel(ui: &mut UiContext, view: &View, l: &Layout, w: f32) -> Option<Rect> {
  if view.rows.is_empty() {
    return None;
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
    return None;
  }
  let panel = Rect::new(x, y, panel_w, panel_h);
  ui.draw_rect(panel, panel_color());
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
    let label = format!("{marker}{}", row.label);
    let (label_max, value_max) = row_widths(inner, text_w(ui, &row.value), gap);
    let label = truncate_to(&label, label_max, |s| text_w(ui, s));
    ui.draw_text(
      &label,
      x + l.pad,
      row_y,
      if selected { key_text() } else { Color::WHITE },
    );
    let value = truncate_to(&row.value, value_max, |s| text_w(ui, s));
    let value_x = x + panel_w - l.pad - text_w(ui, &value);
    ui.draw_text(&value, value_x, row_y, Color::WHITE);
  }
  let hint_y = y + panel_h - l.pad - l.line_h;
  let hint = truncate_to(hint, inner, |s| text_w(ui, s));
  ui.draw_text(&hint, x + l.pad, hint_y, dim_text());
  Some(panel)
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
  let gap = l.pad * 3.0;
  // Leave the bottom-left corner to the engine's FPS counter
  let fps_reserve = text_w(ui, "FPS: 0000") + 2.0 * l.pad;
  let available = (w - 2.0 * l.pad - fps_reserve).max(1.0);
  // An item wider than the screen is shortened rather than run off it
  let items: Vec<(String, String)> = legend_items(view)
    .into_iter()
    .map(|(key, desc)| {
      let room = available - text_w(ui, &key) - text_w(ui, " ");
      let desc = truncate_to(&desc, room, |s| text_w(ui, s));
      (key, desc)
    })
    .collect();
  let widths: Vec<f32> = items
    .iter()
    .map(|(key, desc)| text_w(ui, key) + text_w(ui, " ") + text_w(ui, desc))
    .collect();
  let more = |n: usize| format!("+{n} more (H)");
  let (lines, hidden) = fit_legend(&widths, gap, available, MAX_LEGEND_LINES, |n| {
    text_w(ui, &more(n))
  });
  let line_step = l.line_h + l.pad * 0.5;
  for (n, line) in lines.iter().enumerate() {
    let last = n + 1 == lines.len();
    let from_bottom = (lines.len() - 1 - n) as f32;
    let y = h - l.pad - l.line_h - from_bottom * line_step;
    if y < l.content.y {
      continue;
    }
    let mut line_w: f32 =
      line.iter().map(|&i| widths[i]).sum::<f32>() + gap * line.len().saturating_sub(1) as f32;
    if last && hidden > 0 {
      line_w += if line.is_empty() { 0.0 } else { gap } + text_w(ui, &more(hidden));
    }
    let mut x = (w - l.pad - line_w).max(l.pad);
    ui.draw_rect(
      Rect::new(
        x - l.pad / 2.0,
        y - l.pad / 2.0,
        line_w + l.pad,
        l.line_h + l.pad,
      ),
      backing_color(),
    );
    for &i in line {
      let (key, desc) = &items[i];
      ui.draw_text(key, x, y, key_text());
      let desc_x = x + text_w(ui, key) + text_w(ui, " ");
      ui.draw_text(desc, desc_x, y, dim_text());
      x += widths[i] + gap;
    }
    if last && hidden > 0 {
      ui.draw_text(&more(hidden), x, y, dim_text());
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
  // Lines that fit in the box: the first takes a line height, the rest a step
  let room = box_h - 2.0 * l.pad;
  let capacity = if room < l.line_h {
    0
  } else {
    ((room - l.line_h) / step).floor() as usize + 1
  };
  let (shown, more) = help_fit(lines.len(), capacity);
  let mut line_y = y + l.pad;
  for (text, color) in lines.into_iter().take(shown) {
    let text = truncate_to(&text, inner, |s| text_w(ui, s));
    ui.draw_text(&text, x + l.pad, line_y, color);
    line_y += step;
  }
  if let Some(more) = more {
    let text = format!("... {more} more lines (enlarge the window)");
    let text = truncate_to(&text, inner, |s| text_w(ui, s));
    ui.draw_text(&text, x + l.pad, line_y, dim_text());
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn row_widths_keep_a_row_that_fits() {
    assert_eq!(row_widths(200.0, 30.0, 10.0), (160.0, 30.0));
  }

  #[test]
  fn row_widths_cap_a_long_value_at_half() {
    assert_eq!(row_widths(200.0, 150.0, 10.0), (90.0, 100.0));
  }

  #[test]
  fn row_widths_never_go_negative() {
    assert_eq!(row_widths(10.0, 50.0, 20.0).0, 0.0);
  }

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
  fn a_legend_that_fits_is_left_alone() {
    let (lines, hidden) = fit_legend(&[50.0, 50.0, 50.0], 10.0, 200.0, 3, |_| 40.0);
    assert_eq!(lines, vec![vec![0, 1, 2]]);
    assert_eq!(hidden, 0);
  }

  #[test]
  fn a_long_legend_keeps_three_lines_and_counts_the_rest() {
    // Two 50s per 120 wide line: five lines wanted
    let widths = [50.0; 10];
    let (lines, hidden) = fit_legend(&widths, 10.0, 120.0, 3, |_| 40.0);
    // The last line makes room for the 40 wide "+N more"
    assert_eq!(lines, vec![vec![0, 1], vec![2, 3], vec![4]]);
    assert_eq!(hidden, 5);
  }

  #[test]
  fn help_shows_what_fits_and_says_how_much_is_left() {
    assert_eq!(help_fit(10, 12), (10, None));
    assert_eq!(help_fit(10, 10), (10, None));
    assert_eq!(help_fit(15, 10), (9, Some(6)));
    assert_eq!(help_fit(3, 0), (0, None));
  }

  #[test]
  fn the_top_bar_drops_its_keys_before_cutting_the_title() {
    // Both fit with the gap
    assert_eq!(top_bar_fit(100.0, 80.0, 200.0, 10.0), (200.0, true));
    // The keys go first, and the title may then use the whole bar
    assert_eq!(top_bar_fit(150.0, 80.0, 200.0, 10.0), (200.0, false));
    assert_eq!(top_bar_fit(300.0, 80.0, 200.0, 10.0), (200.0, false));
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
