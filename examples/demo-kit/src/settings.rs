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
  pub(crate) fn new(selected: usize, adjust: i32) -> Self {
    Self {
      selected,
      adjust,
      rows: Vec::new(),
    }
  }

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

  /// A number changed by `step` per press, snapped to a multiple of `step`
  /// and kept within `range`
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

  /// An angle in degrees changed by `step` per press, wrapping round past
  /// 0 and 360 (shown as "deg": the shell's font is ASCII)
  pub fn angle(&mut self, label: &str, degrees: &mut f32, step: f32) {
    let steps = self.steps();
    let mut value = *degrees + steps as f32 * step;
    if steps != 0 && step > 0.0 {
      value = (value / step).round() * step;
    }
    *degrees = if value.is_finite() {
      value.rem_euclid(360.0)
    } else {
      0.0
    };
    let places = decimals(step);
    self.push(
      label,
      format!("{:.*} deg", places, *degrees),
      "0 - 360 deg, wrapping".to_string(),
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

/// Decimal places that show every digit of `step` (0.25 needs two), up to
/// three
fn decimals(step: f32) -> usize {
  (0..3)
    .find(|&places| {
      let scaled = f64::from(step.abs()) * 10f64.powi(places as i32);
      // A whole number, to within float error
      scaled >= 0.5 && (scaled - scaled.round()).abs() < 1e-4
    })
    .unwrap_or(3)
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
  fn angles_wrap_round_both_ways() {
    let mut degrees = 355.0;
    let mut s = Settings::new(0, 1);
    s.angle("Spot", &mut degrees, 5.0);
    assert_eq!(degrees, 0.0);
    assert_eq!(values(s), ["0 deg"]);

    let mut s = Settings::new(0, -1);
    s.angle("Spot", &mut degrees, 5.0);
    assert_eq!(degrees, 355.0);

    // Out-of-range starting values come back into 0..360
    let mut degrees = 725.0;
    let s = &mut Settings::new(1, 0);
    s.angle("Spot", &mut degrees, 5.0);
    assert_eq!(degrees, 5.0);
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
  fn float_shows_every_digit_of_its_step() {
    let mut zoom = 0.75;
    let mut s = Settings::new(9, 0);
    s.float("Zoom", &mut zoom, 0.5..=4.0, 0.25);
    s.float("Rate", &mut 0.75, 0.0..=5.0, 2.5);
    s.float("Count", &mut 3.0, 0.0..=10.0, 1.0);
    s.float("Fine", &mut 0.005, 0.0..=1.0, 0.005);
    s.float("Finer", &mut 0.0002, 0.0..=1.0, 0.0002);
    assert_eq!(values(s), ["0.75", "0.8", "3", "0.005", "0.000"]);
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
