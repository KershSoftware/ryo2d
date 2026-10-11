//! The settings panel's immediate-mode API

use std::ops::RangeInclusive;

/// One row of the settings panel, as drawn
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Row {
  pub label: String,
  pub value: String,
  /// The range or the options, for the help overlay
  pub range: String,
}

/// Tweakable values for the current page, declared every frame in
/// `DemoPage::settings`.
///
/// Up/Down select a row and Left/Right change it. Every value is kept within
/// its range, selected or not.
pub struct Settings {
  selected: usize,
  adjust: i32,
  rows: Vec<Row>,
}

impl Settings {
  #[allow(dead_code)] // used by the runtime from Task 3
  pub(crate) fn new(selected: usize, adjust: i32) -> Self {
    Self {
      selected,
      adjust,
      rows: Vec::new(),
    }
  }

  #[allow(dead_code)] // used by the runtime from Task 3
  pub(crate) fn into_rows(self) -> Vec<Row> {
    self.rows
  }

  /// Steps for the row being declared now
  fn steps(&self) -> i32 {
    if self.rows.len() == self.selected {
      self.adjust
    } else {
      0
    }
  }

  fn push(&mut self, label: &str, value: String, range: String) {
    self.rows.push(Row {
      label: label.to_string(),
      value,
      range,
    });
  }

  /// A number changed by `step` per press and kept within `range`
  pub fn float(&mut self, label: &str, value: &mut f32, range: RangeInclusive<f32>, step: f32) {
    let (lo, hi) = (*range.start(), *range.end());
    let steps = self.steps();
    if steps != 0 {
      let raw = *value + steps as f32 * step;
      *value = if step > 0.0 {
        (raw / step).round() * step
      } else {
        raw
      };
    }
    // max/min rather than clamp: never panics on an inverted range or NaN
    *value = value.max(lo).min(hi);
    let places = decimals(step);
    self.push(
      label,
      format!("{:.*}", places, *value),
      format!("{:.*} - {:.*}", places, lo, places, hi),
    );
  }

  /// A whole number changed by one per press and kept within `range`
  pub fn int(&mut self, label: &str, value: &mut i32, range: RangeInclusive<i32>) {
    let (lo, hi) = (*range.start(), *range.end());
    *value = value.saturating_add(self.steps()).max(lo).min(hi);
    self.push(label, value.to_string(), format!("{lo} - {hi}"));
  }

  /// One of `options`, cycling past either end
  pub fn choice(&mut self, label: &str, index: &mut usize, options: &[&str]) {
    if options.is_empty() {
      self.push(label, "-".to_string(), String::new());
      return;
    }
    let count = options.len() as i64;
    let current = (*index).min(options.len() - 1) as i64;
    *index = (current + i64::from(self.steps())).rem_euclid(count) as usize;
    self.push(label, options[*index].to_string(), options.join(", "));
  }

  /// On or off; each press flips it
  pub fn toggle(&mut self, label: &str, value: &mut bool) {
    if self.steps() % 2 != 0 {
      *value = !*value;
    }
    let text = if *value { "On" } else { "Off" };
    self.push(label, text.to_string(), "On, Off".to_string());
  }
}

/// Decimal places that show a value changed by `step`
fn decimals(step: f32) -> usize {
  if step >= 1.0 {
    0
  } else if step >= 0.1 {
    1
  } else if step >= 0.01 {
    2
  } else {
    3
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn values(settings: Settings) -> Vec<String> {
    settings
      .into_rows()
      .into_iter()
      .map(|row| row.value)
      .collect()
  }

  #[test]
  fn float_steps_snaps_and_clamps() {
    let mut level = 0.5;
    let mut s = Settings::new(0, 1);
    s.float("Level", &mut level, 0.0..=1.0, 0.1);
    assert!((level - 0.6).abs() < 1e-6);
    assert_eq!(values(s), ["0.6"]);

    let mut s = Settings::new(0, 10);
    s.float("Level", &mut level, 0.0..=1.0, 0.1);
    assert_eq!(level, 1.0);
  }

  #[test]
  fn unselected_rows_are_clamped_but_not_changed() {
    let mut level = 3.0;
    let mut count = 2;
    let mut s = Settings::new(1, 1);
    s.float("Level", &mut level, 0.0..=1.0, 0.1);
    s.int("Count", &mut count, 1..=5);
    assert_eq!(level, 1.0);
    assert_eq!(count, 3);
  }

  #[test]
  fn int_clamps_to_its_range() {
    let mut count = 4;
    let mut s = Settings::new(0, 5);
    s.int("Count", &mut count, 1..=5);
    assert_eq!(count, 5);
    let mut s = Settings::new(0, -9);
    s.int("Count", &mut count, 1..=5);
    assert_eq!(count, 1);
  }

  #[test]
  fn choice_wraps_both_ways() {
    let mut mode = 0;
    let mut s = Settings::new(0, -1);
    s.choice("Mode", &mut mode, &["A", "B", "C"]);
    assert_eq!(mode, 2);
    let mut s = Settings::new(0, 2);
    s.choice("Mode", &mut mode, &["A", "B", "C"]);
    assert_eq!(mode, 1);
    assert_eq!(values(s), ["B"]);
  }

  #[test]
  fn choice_without_options_is_shown_as_a_dash() {
    let mut mode = 3;
    let mut s = Settings::new(0, 1);
    s.choice("Mode", &mut mode, &[]);
    assert_eq!(values(s), ["-"]);
  }

  #[test]
  fn toggle_flips_once_per_step() {
    let mut on = false;
    let mut s = Settings::new(0, 1);
    s.toggle("On", &mut on);
    assert!(on);
    let mut s = Settings::new(0, 2);
    s.toggle("On", &mut on);
    assert!(on);
    assert_eq!(values(s), ["On"]);
  }

  #[test]
  fn a_selection_past_the_rows_changes_nothing() {
    let mut level = 0.5;
    let mut s = Settings::new(4, 1);
    s.float("Level", &mut level, 0.0..=1.0, 0.1);
    assert_eq!(level, 0.5);
  }

  #[test]
  fn rows_record_labels_and_ranges() {
    let mut level = 0.25;
    let mut s = Settings::new(0, 0);
    s.float("Level", &mut level, 0.0..=1.0, 0.05);
    let rows = s.into_rows();
    assert_eq!(rows[0].label, "Level");
    assert_eq!(rows[0].value, "0.25");
    assert_eq!(rows[0].range, "0.00 - 1.00");
  }
}
