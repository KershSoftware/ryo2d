//! The coverage model of a line's edges, and a CPU reference to measure it
//! against.
//!
//! A pixel's coverage across an edge is the one-dimensional box filter of
//! its signed distance from the edge, perpendicular to it ([`cover_edge`]);
//! across a strip about a centre line it is the overlap of that box with
//! the strip ([`cover_strip`]). Where two edges meet at a corner the covers
//! multiply. The reference is a 16 x 16 supersample of a square pixel
//! ([`supersample`]): the truth the model is held to.
//!
//! Tests: `an_axis_aligned_edge_covers_by_its_distance`,
//! `a_diagonal_edge_through_the_centre_covers_half`,
//! `a_right_angle_tip_at_the_centre_covers_a_quarter`,
//! `a_centred_strip_covers_by_its_width` and
//! `the_strip_matches_the_supersampler_along_the_axes`.

// The model and the reference are for the line tests
#![allow(dead_code)]

use glam::Vec2;

/// Samples across a pixel, each way
const SAMPLES: u32 = 16;

/// The coverage of a pixel whose centre is `e` pixels inside an edge
/// (negative outside)
pub(crate) fn cover_edge(e: f32) -> f32 {
  (e + 0.5).clamp(0.0, 1.0)
}

/// The coverage of a pixel whose centre is `d` pixels from the centre line
/// of a strip of half width `hw`
pub(crate) fn cover_strip(d: f32, hw: f32) -> f32 {
  ((d + 0.5).min(hw) - (d - 0.5).max(-hw)).clamp(0.0, 1.0)
}

/// Whether `point` lies inside `polygon` by the nonzero winding rule, so a
/// polygon wound either way counts the same. A point on an edge may fall
/// either side.
fn inside(polygon: &[Vec2], point: Vec2) -> bool {
  let mut winding = 0;
  for (i, &a) in polygon.iter().enumerate() {
    let b = polygon[(i + 1) % polygon.len()];
    let side = (b - a).perp_dot(point - a);
    if a.y <= point.y {
      if b.y > point.y && side > 0.0 {
        winding += 1;
      }
    } else if b.y <= point.y && side < 0.0 {
      winding -= 1;
    }
  }
  winding != 0
}

/// The coverage of the square pixel centred at `centre` (one unit wide) by
/// the union of `polygons`, from 16 x 16 samples at the middles of its
/// sub-squares. A line's outline is given as the polygons it is built of
/// (its strip, wedges and caps), each inside by the nonzero rule.
pub(crate) fn supersample(polygons: &[&[Vec2]], centre: Vec2) -> f32 {
  let step = 1.0 / SAMPLES as f32;
  let mut hit = 0;
  for row in 0..SAMPLES {
    for column in 0..SAMPLES {
      let at = centre
        + Vec2::new(
          (column as f32 + 0.5) * step - 0.5,
          (row as f32 + 0.5) * step - 0.5,
        );
      if polygons.iter().any(|polygon| inside(polygon, at)) {
        hit += 1;
      }
    }
  }
  hit as f32 / (SAMPLES * SAMPLES) as f32
}

#[cfg(test)]
mod tests {
  use super::*;

  const FAR: f32 = 100.0;

  #[test]
  fn an_axis_aligned_edge_covers_by_its_distance() {
    // The edge is a quarter of a pixel past the centre
    assert_eq!(cover_edge(0.25), 0.75);
    let half_plane = [
      Vec2::new(-FAR, -FAR),
      Vec2::new(0.25, -FAR),
      Vec2::new(0.25, FAR),
      Vec2::new(-FAR, FAR),
    ];
    assert_eq!(supersample(&[&half_plane], Vec2::ZERO), 0.75);
    assert_eq!(cover_edge(-0.5), 0.0);
    assert_eq!(cover_edge(0.5), 1.0);
  }

  #[test]
  fn a_diagonal_edge_through_the_centre_covers_half() {
    assert_eq!(cover_edge(0.0), 0.5);
    // x + y <= 0; the samples on the diagonal itself fall either side
    let half_plane = [
      Vec2::new(-FAR, FAR),
      Vec2::new(FAR, -FAR),
      Vec2::new(-FAR, -FAR),
    ];
    let covered = supersample(&[&half_plane], Vec2::ZERO);
    assert!((covered - 0.5).abs() <= 1.0 / 16.0, "{covered}");
  }

  #[test]
  fn a_right_angle_tip_at_the_centre_covers_a_quarter() {
    assert_eq!(cover_edge(0.0) * cover_edge(0.0), 0.25);
    let quadrant = [
      Vec2::new(0.0, 0.0),
      Vec2::new(-FAR, 0.0),
      Vec2::new(-FAR, -FAR),
      Vec2::new(0.0, -FAR),
    ];
    assert_eq!(supersample(&[&quadrant], Vec2::ZERO), 0.25);
  }

  #[test]
  fn a_centred_strip_covers_by_its_width() {
    assert_eq!(cover_strip(0.0, 0.25), 0.5);
    let strip = [
      Vec2::new(-FAR, -0.25),
      Vec2::new(FAR, -0.25),
      Vec2::new(FAR, 0.25),
      Vec2::new(-FAR, 0.25),
    ];
    assert_eq!(supersample(&[&strip], Vec2::ZERO), 0.5);
  }

  #[test]
  fn the_strip_matches_the_supersampler_along_the_axes() {
    // Sixteenths, where the samples resolve an edge exactly
    for half_width in [1, 2, 4, 8, 12, 16, 24] {
      for offset in -28..=28 {
        let (hw, d) = (half_width as f32 / 16.0, offset as f32 / 16.0);
        let strip = [
          Vec2::new(-FAR, -hw),
          Vec2::new(FAR, -hw),
          Vec2::new(FAR, hw),
          Vec2::new(-FAR, hw),
        ];
        let sampled = supersample(&[&strip], Vec2::new(0.0, d));
        assert!(
          (sampled - cover_strip(d, hw)).abs() <= 1.0 / 256.0,
          "hw {hw} d {d}: {sampled} against {}",
          cover_strip(d, hw)
        );
      }
    }
  }
}
