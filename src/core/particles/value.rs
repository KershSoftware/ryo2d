//! Random ranges, curves and gradients for particle effects.
//!
//! Curves and gradients keep their keys and a 64-entry table; the particle
//! step reads the table, and `sample` evaluates the keys exactly.

use std::fmt;

use serde::de::{self, SeqAccess, Visitor};
use serde::ser::SerializeTuple;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::rng::ParticleRng;
use crate::core::ecs::{Ease, Lerp};
use crate::core::ui::Color;

const TABLE_SIZE: usize = 64;

/// A random number between `min` and `max`.
///
/// Reads from RON as a number (fixed) or `(min, max)`. A reversed or NaN range
/// samples its `min`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(from = "ValueRangeRepr", into = "ValueRangeRepr")]
pub struct ValueRange {
  /// Lowest value drawn
  pub min: f32,
  /// Highest value drawn
  pub max: f32,
}

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum ValueRangeRepr {
  One(f32),
  Pair(f32, f32),
}

impl From<ValueRangeRepr> for ValueRange {
  fn from(repr: ValueRangeRepr) -> Self {
    match repr {
      ValueRangeRepr::One(value) => Self::constant(value),
      ValueRangeRepr::Pair(min, max) => Self::new(min, max),
    }
  }
}

impl From<ValueRange> for ValueRangeRepr {
  fn from(range: ValueRange) -> Self {
    if range.min == range.max {
      Self::One(range.min)
    } else {
      Self::Pair(range.min, range.max)
    }
  }
}

impl ValueRange {
  /// A range from `min` to `max`.
  pub const fn new(min: f32, max: f32) -> Self {
    Self { min, max }
  }

  /// A range that always gives `value`.
  pub const fn constant(value: f32) -> Self {
    Self {
      min: value,
      max: value,
    }
  }

  /// Draws a value from the range.
  pub(crate) fn sample(&self, rng: &mut ParticleRng) -> f32 {
    rng.range(self.min, self.max)
  }

  /// The largest value the range can give (0 when not finite).
  pub(crate) fn largest(&self) -> f32 {
    if !self.min.is_finite() {
      0.0
    } else if self.max.is_finite() {
      self.min.max(self.max)
    } else {
      self.min
    }
  }
}

impl From<f32> for ValueRange {
  fn from(value: f32) -> Self {
    Self::constant(value)
  }
}

impl From<(f32, f32)> for ValueRange {
  fn from((min, max): (f32, f32)) -> Self {
    Self::new(min, max)
  }
}

/// A random colour between `a` and `b`, one blend for all four channels.
///
/// Reads from RON as a `Color` (fixed) or `(Color, Color)`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(from = "ColorRangeRepr", into = "ColorRangeRepr")]
pub struct ColorRange {
  /// One end of the range
  pub a: Color,
  /// The other end of the range
  pub b: Color,
}

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum ColorRangeRepr {
  Pair(Color, Color),
  One(Color),
}

impl From<ColorRangeRepr> for ColorRange {
  fn from(repr: ColorRangeRepr) -> Self {
    match repr {
      ColorRangeRepr::One(color) => Self::constant(color),
      ColorRangeRepr::Pair(a, b) => Self::new(a, b),
    }
  }
}

impl From<ColorRange> for ColorRangeRepr {
  fn from(range: ColorRange) -> Self {
    if range.a == range.b {
      Self::One(range.a)
    } else {
      Self::Pair(range.a, range.b)
    }
  }
}

impl ColorRange {
  /// A range from `a` to `b`.
  pub const fn new(a: Color, b: Color) -> Self {
    Self { a, b }
  }

  /// A range that always gives `color`.
  pub const fn constant(color: Color) -> Self {
    Self { a: color, b: color }
  }

  /// Draws a colour as channels in 0..1.
  pub(crate) fn sample(&self, rng: &mut ParticleRng) -> [f32; 4] {
    let t = if self.a == self.b {
      0.0
    } else {
      rng.next_f32()
    };
    let (a, b) = (channels(self.a), channels(self.b));
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
  }
}

impl From<Color> for ColorRange {
  fn from(color: Color) -> Self {
    Self::constant(color)
  }
}

impl From<(Color, Color)> for ColorRange {
  fn from((a, b): (Color, Color)) -> Self {
    Self::new(a, b)
  }
}

fn channels(color: Color) -> [f32; 4] {
  [color.r, color.g, color.b, color.a].map(|c| f32::from(c) / 255.0)
}

/// One key of a [`Curve`]: a value at time `t`, reached with `ease`.
///
/// Reads from RON as `(t, value)` or `(t, value, Ease)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurveKey {
  /// Time along the curve, 0 to 1
  pub t: f32,
  /// The curve's value at `t`
  pub value: f32,
  /// How the segment ending at this key eases
  pub ease: Ease,
}

impl Serialize for CurveKey {
  fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
    if self.ease == Ease::Linear {
      let mut tuple = serializer.serialize_tuple(2)?;
      tuple.serialize_element(&self.t)?;
      tuple.serialize_element(&self.value)?;
      tuple.end()
    } else {
      let mut tuple = serializer.serialize_tuple(3)?;
      tuple.serialize_element(&self.t)?;
      tuple.serialize_element(&self.value)?;
      tuple.serialize_element(&self.ease)?;
      tuple.end()
    }
  }
}

impl<'de> Deserialize<'de> for CurveKey {
  fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
    struct KeyVisitor;

    impl<'de> Visitor<'de> for KeyVisitor {
      type Value = CurveKey;

      fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a curve key (t, value) or (t, value, ease)")
      }

      fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<CurveKey, A::Error> {
        let t = seq
          .next_element()?
          .ok_or_else(|| de::Error::invalid_length(0, &self))?;
        let value = seq
          .next_element()?
          .ok_or_else(|| de::Error::invalid_length(1, &self))?;
        let ease = seq.next_element()?.unwrap_or_default();
        Ok(CurveKey { t, value, ease })
      }
    }

    deserializer.deserialize_tuple(3, KeyVisitor)
  }
}

/// A value over particle life (0 to 1) built from keys.
///
/// Keys are sorted by `t` and clamped to 0..1; keys with a non-finite part are
/// dropped. No keys gives a constant 1. Reads from RON as its key list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "Vec<CurveKey>", into = "Vec<CurveKey>")]
pub struct Curve {
  keys: Vec<CurveKey>,
  table: [f32; TABLE_SIZE],
}

impl From<Vec<CurveKey>> for Curve {
  fn from(keys: Vec<CurveKey>) -> Self {
    let mut keys: Vec<CurveKey> = keys
      .into_iter()
      .filter(|key| key.t.is_finite() && key.value.is_finite())
      .map(|key| CurveKey {
        t: key.t.clamp(0.0, 1.0),
        ..key
      })
      .collect();
    keys.sort_by(|a, b| a.t.total_cmp(&b.t));
    let mut curve = Self {
      keys,
      table: [1.0; TABLE_SIZE],
    };
    curve.table = std::array::from_fn(|i| curve.sample(i as f32 / (TABLE_SIZE - 1) as f32));
    curve
  }
}

impl From<Curve> for Vec<CurveKey> {
  fn from(curve: Curve) -> Self {
    curve.keys
  }
}

impl Curve {
  /// A linear curve through `(t, value)` keys.
  pub fn new(keys: impl IntoIterator<Item = (f32, f32)>) -> Self {
    Self::eased(keys.into_iter().map(|(t, value)| (t, value, Ease::Linear)))
  }

  /// A curve through `(t, value, ease)` keys; each segment eases by the key it
  /// ends at.
  pub fn eased(keys: impl IntoIterator<Item = (f32, f32, Ease)>) -> Self {
    keys
      .into_iter()
      .map(|(t, value, ease)| CurveKey { t, value, ease })
      .collect::<Vec<_>>()
      .into()
  }

  /// A curve that is `value` throughout.
  pub fn constant(value: f32) -> Self {
    Self::new([(0.0, value)])
  }

  /// The keys, sorted and clamped.
  pub fn keys(&self) -> &[CurveKey] {
    &self.keys
  }

  /// The curve's value at `t` (clamped to 0..1), evaluated exactly.
  pub fn sample(&self, t: f32) -> f32 {
    let t = if t.is_nan() { 0.0 } else { t.clamp(0.0, 1.0) };
    let (Some(first), Some(last)) = (self.keys.first(), self.keys.last()) else {
      return 1.0;
    };
    if t <= first.t {
      return first.value;
    }
    if t >= last.t {
      return last.value;
    }
    let end = self.keys.partition_point(|key| key.t <= t);
    let (a, b) = (self.keys[end - 1], self.keys[end]);
    let span = b.t - a.t;
    let u = if span > 0.0 { (t - a.t) / span } else { 1.0 };
    f32::lerp(&a.value, &b.value, b.ease.apply(u))
  }

  /// The largest magnitude [`lookup`](Self::lookup) gives anywhere (an
  /// eased segment can overshoot its keys)
  pub(crate) fn largest(&self) -> f32 {
    self
      .table
      .iter()
      .fold(0.0, |most, value| most.max(value.abs()))
  }

  /// The value at `t` read from the 64-entry table.
  pub(crate) fn lookup(&self, t: f32) -> f32 {
    let (i, frac) = table_position(t);
    f32::lerp(
      &self.table[i],
      &self.table[(i + 1).min(TABLE_SIZE - 1)],
      frac,
    )
  }
}

fn table_position(t: f32) -> (usize, f32) {
  let t = if t.is_nan() { 0.0 } else { t.clamp(0.0, 1.0) };
  let at = t * (TABLE_SIZE - 1) as f32;
  let i = (at as usize).min(TABLE_SIZE - 1);
  (i, at - i as f32)
}

/// One key of a [`Gradient`]: a colour at time `t`.
///
/// Reads from RON as `(t, Color)`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(from = "(f32, Color)", into = "(f32, Color)")]
pub struct GradientKey {
  /// Time along the gradient, 0 to 1
  pub t: f32,
  /// The colour at `t`
  pub color: Color,
}

impl From<(f32, Color)> for GradientKey {
  fn from((t, color): (f32, Color)) -> Self {
    Self { t, color }
  }
}

impl From<GradientKey> for (f32, Color) {
  fn from(key: GradientKey) -> Self {
    (key.t, key.color)
  }
}

/// A colour over particle life (0 to 1) built from keys.
///
/// Keys are sorted by `t` and clamped to 0..1; keys with a non-finite `t` are
/// dropped. No keys gives white. Channels mix as stored, rounding like
/// [`Lerp`] for `Color`. Reads from RON as its key list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "Vec<GradientKey>", into = "Vec<GradientKey>")]
pub struct Gradient {
  keys: Vec<GradientKey>,
  table: [[f32; 4]; TABLE_SIZE],
}

impl From<Vec<GradientKey>> for Gradient {
  fn from(keys: Vec<GradientKey>) -> Self {
    let mut keys: Vec<GradientKey> = keys
      .into_iter()
      .filter(|key| key.t.is_finite())
      .map(|key| GradientKey {
        t: key.t.clamp(0.0, 1.0),
        ..key
      })
      .collect();
    keys.sort_by(|a, b| a.t.total_cmp(&b.t));
    let mut gradient = Self {
      keys,
      table: [[1.0; 4]; TABLE_SIZE],
    };
    gradient.table =
      std::array::from_fn(|i| channels(gradient.sample(i as f32 / (TABLE_SIZE - 1) as f32)));
    gradient
  }
}

impl From<Gradient> for Vec<GradientKey> {
  fn from(gradient: Gradient) -> Self {
    gradient.keys
  }
}

impl Gradient {
  /// A gradient through `(t, colour)` keys.
  pub fn new(keys: impl IntoIterator<Item = (f32, Color)>) -> Self {
    keys
      .into_iter()
      .map(|(t, color)| GradientKey { t, color })
      .collect::<Vec<_>>()
      .into()
  }

  /// A gradient that is `color` throughout.
  pub fn constant(color: Color) -> Self {
    Self::new([(0.0, color)])
  }

  /// The keys, sorted and clamped.
  pub fn keys(&self) -> &[GradientKey] {
    &self.keys
  }

  /// The colour at `t` (clamped to 0..1), evaluated exactly.
  pub fn sample(&self, t: f32) -> Color {
    let t = if t.is_nan() { 0.0 } else { t.clamp(0.0, 1.0) };
    let (Some(first), Some(last)) = (self.keys.first(), self.keys.last()) else {
      return Color::WHITE;
    };
    if t <= first.t {
      return first.color;
    }
    if t >= last.t {
      return last.color;
    }
    let end = self.keys.partition_point(|key| key.t <= t);
    let (a, b) = (self.keys[end - 1], self.keys[end]);
    let span = b.t - a.t;
    let u = if span > 0.0 { (t - a.t) / span } else { 1.0 };
    Color::lerp(&a.color, &b.color, u)
  }

  /// The colour at `t` as channels in 0..1, read from the 64-entry table.
  pub(crate) fn lookup(&self, t: f32) -> [f32; 4] {
    let (i, frac) = table_position(t);
    let (a, b) = (self.table[i], self.table[(i + 1).min(TABLE_SIZE - 1)]);
    std::array::from_fn(|c| a[c] + (b[c] - a[c]) * frac)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn ron_options() -> ron::Options {
    ron::Options::default()
  }

  #[test]
  fn a_range_reads_from_a_number_or_a_pair() {
    let one: ValueRange = ron_options().from_str("8.0").unwrap();
    assert_eq!(one, ValueRange::new(8.0, 8.0));
    let pair: ValueRange = ron_options().from_str("(8.0, 16.0)").unwrap();
    assert_eq!(pair, ValueRange::new(8.0, 16.0));
  }

  #[test]
  fn a_colour_range_reads_from_a_colour_or_a_pair() {
    let one: ColorRange = ron_options().from_str("(r: 1, g: 2, b: 3, a: 4)").unwrap();
    assert_eq!(one, ColorRange::constant(Color::new(1, 2, 3, 4)));
    let pair: ColorRange = ron_options()
      .from_str("((r: 1, g: 2, b: 3, a: 4), (r: 5, g: 6, b: 7, a: 8))")
      .unwrap();
    assert_eq!(
      pair,
      ColorRange::new(Color::new(1, 2, 3, 4), Color::new(5, 6, 7, 8))
    );
  }

  #[test]
  fn curves_and_gradients_read_from_key_lists_and_write_back() {
    let curve: Curve = ron_options()
      .from_str("[(0.0, 1.0), (1.0, 0.0, InQuad)]")
      .unwrap();
    assert_eq!(
      curve,
      Curve::eased([(0.0, 1.0, Ease::Linear), (1.0, 0.0, Ease::InQuad)])
    );
    let text = ron::to_string(&curve).unwrap();
    assert_eq!(ron_options().from_str::<Curve>(&text).unwrap(), curve);
    let gradient: Gradient = ron_options()
      .from_str("[(0.0, (r: 255, g: 0, b: 0, a: 255)), (1.0, (r: 0, g: 0, b: 255, a: 255))]")
      .unwrap();
    assert_eq!(gradient.keys().len(), 2);
    let text = ron::to_string(&gradient).unwrap();
    assert_eq!(ron_options().from_str::<Gradient>(&text).unwrap(), gradient);
  }

  #[test]
  fn a_reversed_or_nan_range_samples_its_min() {
    let mut rng = ParticleRng::new(1);
    for _ in 0..20 {
      assert_eq!(ValueRange::new(5.0, 1.0).sample(&mut rng), 5.0);
      assert_eq!(ValueRange::new(f32::NAN, 1.0).sample(&mut rng), 0.0);
    }
    assert_eq!(ValueRange::new(f32::NAN, 1.0).largest(), 0.0);
    assert_eq!(ValueRange::new(5.0, 1.0).largest(), 5.0);
  }

  #[test]
  fn a_colour_range_blends_all_channels_by_one_draw() {
    let mut rng = ParticleRng::new(2);
    let range = ColorRange::new(Color::new(0, 0, 0, 0), Color::new(255, 255, 255, 255));
    let [r, g, b, a] = range.sample(&mut rng);
    assert!((0.0..1.0).contains(&r));
    assert!(r == g && g == b && b == a);
  }

  #[test]
  fn a_curve_hits_its_keys_and_mixes_between() {
    let curve = Curve::new([(0.0, 1.0), (1.0, 0.0)]);
    assert_eq!(curve.sample(0.0), 1.0);
    assert_eq!(curve.sample(1.0), 0.0);
    assert_eq!(curve.sample(0.25), 0.75);
    for i in 0..=100 {
      let t = i as f32 / 100.0;
      assert!((curve.lookup(t) - curve.sample(t)).abs() < 1e-3, "{t}");
    }
  }

  #[test]
  fn an_eased_segment_follows_its_end_key_s_ease() {
    let curve = Curve::eased([(0.0, 0.0, Ease::Linear), (1.0, 1.0, Ease::InQuad)]);
    assert_eq!(curve.sample(0.5), 0.25);
  }

  #[test]
  fn curve_keys_out_of_order_or_range_are_sorted_and_clamped() {
    let curve = Curve::new([
      (2.0, 0.0),
      (0.5, 1.0),
      (f32::NAN, 7.0),
      (-1.0, 0.5),
      (0.5, f32::INFINITY),
    ]);
    let ts: Vec<f32> = curve.keys().iter().map(|key| key.t).collect();
    assert_eq!(ts, [0.0, 0.5, 1.0]);
    assert_eq!(curve.sample(0.25), 0.75);
    assert_eq!(curve.sample(5.0), 0.0);
    assert_eq!(curve.sample(f32::NAN), 0.5);
  }

  #[test]
  fn a_gradient_mixes_colours_as_stored() {
    let gradient = Gradient::new([
      (0.0, Color::new(255, 0, 0, 255)),
      (1.0, Color::new(0, 0, 255, 255)),
    ]);
    assert_eq!(gradient.sample(0.5), Color::new(128, 0, 128, 255));
    let [r, _, b, a] = gradient.lookup(0.5);
    assert!((r - 0.5).abs() < 0.01 && (b - 0.5).abs() < 0.01 && a == 1.0);
  }

  #[test]
  fn an_empty_curve_is_one_and_an_empty_gradient_white() {
    let curve = Curve::new([]);
    assert_eq!(curve.sample(0.3), 1.0);
    assert_eq!(curve.lookup(0.3), 1.0);
    let gradient = Gradient::new([]);
    assert_eq!(gradient.sample(0.3), Color::WHITE);
    assert_eq!(gradient.lookup(0.3), [1.0; 4]);
  }
}
