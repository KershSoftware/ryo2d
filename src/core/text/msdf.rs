//! MSDF glyphs: swash's outline as an fdsm shape, then a distance field

use fdsm::bezier::scanline::FillRule;
use fdsm::bezier::{Point, Segment};
use fdsm::correct_error::{ErrorCorrectionConfig, correct_error_msdf};
use fdsm::generate::generate_msdf;
use fdsm::render::correct_sign_msdf;
use fdsm::shape::{Contour, Shape};
use image::{ImageBuffer, Rgb};
use swash::FontRef;
use swash::scale::ScaleContext;
use swash::zeno::Verb;

use crate::core::text::raster::{GlyphFormat, GlyphImage, MAX_GLYPH_PX};

/// Largest MSDF image edge (pixels); a bigger one means a bad setting
const MAX_IMAGE: u32 = 4096;

/// An MSDF of `glyph` made at `base` pixels per em with a total distance
/// range of `range` pixels; `None` for an empty glyph
pub(crate) fn generate(
  scale_cx: &mut ScaleContext,
  font: FontRef<'_>,
  coords: Option<&[i16]>,
  cache_id: [u64; 2],
  glyph: u16,
  base: f32,
  range: f32,
) -> Option<GlyphImage> {
  let units = f32::from(font.metrics(&[]).units_per_em);
  if units <= 0.0 || !(range.is_finite() && range > 0.0 && range <= MAX_GLYPH_PX) {
    return None;
  }
  let mut builder = scale_cx
    .builder_with_id(font, cache_id)
    .size(units)
    .hint(false);
  if let Some(coords) = coords {
    builder = builder.normalized_coords(coords);
  }
  let outline = builder.build().scale_outline(glyph)?;
  let bounds = outline.bounds();
  if bounds.is_empty() {
    return None;
  }
  let scale = f64::from(base / units);
  let pad = (f64::from(range) / 2.0).ceil() + 1.0;
  let (x0, y0, x1, y1) = (
    f64::from(bounds.min.x),
    f64::from(bounds.min.y),
    f64::from(bounds.max.x),
    f64::from(bounds.max.y),
  );
  // Whole-pixel placement first, then the outline is positioned from it:
  // image x = pen x - left, image y = top - baseline-up y
  let left = (x0 * scale - pad).floor();
  let top = (y1 * scale + pad).ceil();
  let width = ((x1 * scale + pad) - left).ceil();
  let height = (top - (y0 * scale - pad)).ceil();
  if !(width >= 1.0
    && height >= 1.0
    && width <= f64::from(MAX_IMAGE)
    && height <= f64::from(MAX_IMAGE))
  {
    return None;
  }
  let (width, height) = (width as u32, height as u32);
  let shape = shape(outline.verbs(), outline.points(), |x, y| {
    Point::new(f64::from(x) * scale - left, top - f64::from(y) * scale)
  });
  let colored = Shape::edge_coloring_simple(shape, 0.03, 69_441_337_420);
  let prepared = colored.prepare();
  let mut field = ImageBuffer::<Rgb<f32>, Vec<f32>>::new(width, height);
  let range = f64::from(range);
  generate_msdf(&prepared, range, &mut field);
  correct_error_msdf(
    &mut field,
    &colored,
    &prepared,
    range,
    &ErrorCorrectionConfig::default(),
  );
  correct_sign_msdf(&mut field, &prepared, FillRule::Nonzero);
  let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
  let mut data = Vec::with_capacity((width * height * 4) as usize);
  for pixel in field.pixels() {
    data.extend_from_slice(&[byte(pixel[0]), byte(pixel[1]), byte(pixel[2]), 255]);
  }
  Some(GlyphImage {
    format: GlyphFormat::Msdf,
    width,
    height,
    left: left as i32,
    top: top as i32,
    data,
    base,
  })
}

/// swash's outline (font units, y up) as an fdsm shape, each point mapped
/// by `to_image`
fn shape(
  verbs: &[Verb],
  points: &[swash::zeno::Point],
  to_image: impl Fn(f32, f32) -> Point,
) -> Shape<Contour> {
  let mut shape = Shape::<Contour>::default();
  let point = |i: usize| points.get(i).map(|p| to_image(p.x, p.y));
  let mut next = 0;
  let mut start = Point::new(0.0, 0.0);
  let mut last = start;
  let mut open = false;
  let push = |shape: &mut Shape<Contour>, segment: Segment| {
    if let Some(contour) = shape.contours.last_mut() {
      contour.segments.push(segment);
    }
  };
  for verb in verbs {
    match verb {
      Verb::MoveTo => {
        if open && (last - start).norm() > 1e-9 {
          push(&mut shape, Segment::line(last, start));
        }
        let Some(p) = point(next) else { break };
        next += 1;
        start = p;
        last = p;
        shape.contours.push(Contour::default());
        open = true;
      }
      Verb::LineTo => {
        let Some(p) = point(next) else { break };
        next += 1;
        push(&mut shape, Segment::line(last, p));
        last = p;
      }
      Verb::QuadTo => {
        let (Some(c), Some(p)) = (point(next), point(next + 1)) else {
          break;
        };
        next += 2;
        push(&mut shape, Segment::quad(last, c, p));
        last = p;
      }
      Verb::CurveTo => {
        let (Some(c1), Some(c2), Some(p)) = (point(next), point(next + 1), point(next + 2)) else {
          break;
        };
        next += 3;
        push(&mut shape, Segment::cubic(last, c1, c2, p));
        last = p;
      }
      Verb::Close => {
        if (last - start).norm() > 1e-9 {
          push(&mut shape, Segment::line(last, start));
        }
        last = start;
        open = false;
      }
    }
  }
  if open && (last - start).norm() > 1e-9 {
    push(&mut shape, Segment::line(last, start));
  }
  // A contour with no segments (a lone MoveTo) has no edges to colour
  shape.contours.retain(|c| !c.segments.is_empty());
  shape
}

#[cfg(test)]
mod tests {
  use super::*;
  use swash::zeno::Point as P;

  #[test]
  fn contours_without_segments_are_dropped() {
    let verbs = [
      Verb::MoveTo,
      Verb::MoveTo,
      Verb::LineTo,
      Verb::LineTo,
      Verb::Close,
      Verb::MoveTo,
      Verb::Close,
    ];
    let points = [
      P::new(5.0, 5.0),
      P::new(0.0, 0.0),
      P::new(10.0, 0.0),
      P::new(10.0, 10.0),
      P::new(7.0, 7.0),
    ];
    let shape = shape(&verbs, &points, |x, y| {
      Point::new(f64::from(x), f64::from(y))
    });
    assert_eq!(shape.contours.len(), 1);
    assert!(shape.contours.iter().all(|c| !c.segments.is_empty()));
  }
}
