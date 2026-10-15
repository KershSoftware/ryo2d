//! Glyph bitmaps: swash for raster, stroked and pixel glyphs, fdsm for MSDF.
//! The renderer's glyph atlas stores what this returns.

use swash::FontRef;
use swash::scale::image::Content;
use swash::scale::{Render, ScaleContext, Source, StrikeWith};
use swash::zeno::{Angle, Format, Join, Stroke, Transform, Vector};

use crate::core::text::{FaceSource, FontId, Fonts};

/// Glyphs larger than this (pixels) are refused
pub(crate) const MAX_GLYPH_PX: f32 = 1024.0;

/// Sizes above this rasterize unhinted (hinting costs about 10 us a glyph
/// and matters little at large sizes)
const HINT_UP_TO: f32 = 48.0;

/// Subpixel bins across one pixel
const SUBPIXEL_BINS: u8 = 4;

/// What kind of image a glyph key asks for
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum GlyphKind {
  Fill,
  /// An outline band `width64 / 64` pixels wide, centred on the outline
  Stroke {
    width64: u32,
  },
  /// Coverage thresholded to fully on or off
  Pixel,
  /// A distance field with a `range64 / 64` pixel range at the base size
  Msdf {
    range64: u32,
  },
}

/// One glyph image in the atlas
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct GlyphKey {
  pub(crate) font: FontId,
  pub(crate) glyph: u16,
  /// Pixel size x 64 (the base size for MSDF)
  pub(crate) size64: u32,
  pub(crate) subpixel: u8,
  pub(crate) kind: GlyphKind,
  pub(crate) coords: u64,
  pub(crate) skew: bool,
  pub(crate) embolden: bool,
}

fn fixed(value: f32) -> u32 {
  (value * 64.0).round().clamp(0.0, u32::MAX as f32) as u32
}

impl GlyphKey {
  /// A filled glyph at `px`, its pen at `x_fraction` (0..1) of a pixel
  pub(crate) fn fill(font: FontId, glyph: u16, px: f32, x_fraction: f32) -> Self {
    let bin = (x_fraction.rem_euclid(1.0) * SUBPIXEL_BINS as f32).round() as u8 % SUBPIXEL_BINS;
    Self {
      font,
      glyph,
      size64: fixed(px),
      subpixel: bin,
      kind: GlyphKind::Fill,
      coords: 0,
      skew: false,
      embolden: false,
    }
  }

  /// Its outline band, `width_px` wide
  pub(crate) fn stroke(font: FontId, glyph: u16, px: f32, x_fraction: f32, width_px: f32) -> Self {
    Self {
      kind: GlyphKind::Stroke {
        width64: fixed(width_px),
      },
      ..Self::fill(font, glyph, px, x_fraction)
    }
  }

  /// A pixel-font glyph (no subpixel position)
  pub(crate) fn pixel(font: FontId, glyph: u16, px: f32) -> Self {
    Self {
      subpixel: 0,
      kind: GlyphKind::Pixel,
      ..Self::fill(font, glyph, px, 0.0)
    }
  }

  /// An MSDF glyph made at `base` pixels with a `range` pixel distance range
  pub(crate) fn msdf(font: FontId, glyph: u16, base: f32, range: f32) -> Self {
    Self {
      subpixel: 0,
      kind: GlyphKind::Msdf {
        range64: fixed(range),
      },
      ..Self::fill(font, glyph, base, 0.0)
    }
  }

  pub(crate) fn px(&self) -> f32 {
    self.size64 as f32 / 64.0
  }
}

/// Which texture kind a glyph image goes in
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum GlyphFormat {
  /// R8 coverage
  Mask,
  /// RGBA8, straight alpha, sRGB (colour glyphs)
  Color,
  /// RGBA8 distances, linear
  Msdf,
}

/// A glyph's bitmap and where it sits relative to the pen on the baseline
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GlyphImage {
  pub(crate) format: GlyphFormat,
  pub(crate) width: u32,
  pub(crate) height: u32,
  /// Pixels from the pen to the image's left edge
  pub(crate) left: i32,
  /// Pixels from the baseline up to the image's top edge
  pub(crate) top: i32,
  pub(crate) data: Vec<u8>,
  /// The pixel size the image was made at
  pub(crate) base: f32,
}

/// The glyph id of `c` in a font (outline fonts only)
pub(crate) fn glyph_id(fonts: &Fonts, font: FontId, c: char) -> Option<u16> {
  let FaceSource::Outline { data, index } = &fonts.face(font)?.source else {
    return None;
  };
  let font_ref = FontRef::from_index(data, *index as usize)?;
  let id = font_ref.charmap().map(c);
  (id != 0).then_some(id)
}

/// The pixel size raster world text draws at: exact to 32 px, 2 px steps to
/// 64, 4 px steps above, never below 1
pub(crate) fn raster_size_step(px: f32) -> f32 {
  let px = px.max(1.0);
  if px <= 32.0 {
    px.round().max(1.0)
  } else if px <= 64.0 {
    (px / 2.0).round() * 2.0
  } else {
    (px / 4.0).round() * 4.0
  }
}

/// Makes glyph images; owns swash's scaling caches.
///
/// One rasterizer serves one `Fonts`: swash caches each font's parsed data
/// under the id `[FontId, face index]`, and another registry's fonts reuse
/// those ids for different files. The engine owns one of each.
pub(crate) struct GlyphRasterizer {
  scale: ScaleContext,
}

impl GlyphRasterizer {
  pub(crate) fn new() -> Self {
    Self {
      scale: ScaleContext::new(),
    }
  }

  /// The image a key names; `None` for an unknown font or glyph, an empty
  /// glyph (a space), a bitmap font (drawn from its sprite instead) or a
  /// size past `MAX_GLYPH_PX`
  pub(crate) fn rasterize(
    &mut self,
    fonts: &Fonts,
    coords: Option<&[i16]>,
    key: &GlyphKey,
  ) -> Option<GlyphImage> {
    let px = key.px();
    if !(px.is_finite() && px > 0.0 && px <= MAX_GLYPH_PX) {
      return None;
    }
    let FaceSource::Outline { data, index } = &fonts.face(key.font)?.source else {
      return None;
    };
    let font = FontRef::from_index(data, *index as usize)?;
    // A stable id lets swash reuse its parsed font data between glyphs
    let cache_id = [u64::from(key.font.0), u64::from(*index)];
    match key.kind {
      GlyphKind::Msdf { range64 } => crate::core::text::msdf::generate(
        &mut self.scale,
        font,
        coords,
        cache_id,
        key.glyph,
        px,
        range64 as f32 / 64.0,
      ),
      GlyphKind::Fill | GlyphKind::Stroke { .. } | GlyphKind::Pixel => {
        self.swash(font, cache_id, coords, key, px)
      }
    }
  }

  fn swash(
    &mut self,
    font: FontRef<'_>,
    cache_id: [u64; 2],
    coords: Option<&[i16]>,
    key: &GlyphKey,
    px: f32,
  ) -> Option<GlyphImage> {
    let pixel = matches!(key.kind, GlyphKind::Pixel);
    let hint = !pixel && px <= HINT_UP_TO;
    let mut builder = self
      .scale
      .builder_with_id(font, cache_id)
      .size(px)
      .hint(hint);
    if let Some(coords) = coords {
      builder = builder.normalized_coords(coords);
    }
    let mut scaler = builder.build();
    let fill_sources = [
      Source::ColorOutline(0),
      Source::ColorBitmap(StrikeWith::BestFit),
      Source::Outline,
    ];
    let outline_only = [Source::Outline];
    let mut render = match key.kind {
      GlyphKind::Stroke { .. } => Render::new(&outline_only),
      _ => Render::new(&fill_sources),
    };
    render.format(Format::Alpha);
    render.offset(Vector::new(
      f32::from(key.subpixel) / f32::from(SUBPIXEL_BINS),
      0.0,
    ));
    let mut stroke = Stroke::new(0.0);
    if let GlyphKind::Stroke { width64 } = key.kind {
      let width = width64 as f32 / 64.0;
      if !(width.is_finite() && width > 0.0 && width <= MAX_GLYPH_PX) {
        return None;
      }
      stroke.width = width;
      stroke.join(Join::Round);
      render.style(stroke);
    }
    if key.skew {
      render.transform(Some(Transform::skew(
        Angle::from_degrees(14.0),
        Angle::ZERO,
      )));
    }
    if key.embolden {
      render.embolden(px * 0.02);
    }
    let image = render.render(&mut scaler, key.glyph)?;
    if image.placement.width == 0 || image.placement.height == 0 {
      return None;
    }
    let (format, mut data) = match image.content {
      Content::Mask => (GlyphFormat::Mask, image.data),
      Content::Color => (GlyphFormat::Color, image.data),
      // Format::Alpha never asks for subpixel masks; treat one as a mask
      Content::SubpixelMask => (
        GlyphFormat::Mask,
        image.data.as_chunks::<4>().0.iter().map(|p| p[1]).collect(),
      ),
    };
    if pixel && format == GlyphFormat::Mask {
      for coverage in &mut data {
        *coverage = if *coverage >= 128 { 255 } else { 0 };
      }
    }
    Some(GlyphImage {
      format,
      width: image.placement.width,
      height: image.placement.height,
      left: image.placement.left,
      top: image.placement.top,
      data,
      base: px,
    })
  }
}
#[cfg(all(test, feature = "default-fonts"))]
mod tests {
  use super::*;
  use crate::core::text::{FontRole, Fonts};

  fn ui(fonts: &Fonts) -> FontId {
    fonts.role(FontRole::Ui).unwrap()
  }

  fn fill(fonts: &Fonts, c: char, px: f32) -> GlyphImage {
    let font = ui(fonts);
    let glyph = glyph_id(fonts, font, c).unwrap();
    GlyphRasterizer::new()
      .rasterize(fonts, None, &GlyphKey::fill(font, glyph, px, 0.0))
      .unwrap()
  }

  #[test]
  fn a_letter_rasterizes_to_a_coverage_mask() {
    let fonts = Fonts::with_builtins();
    let image = fill(&fonts, 'A', 16.0);
    assert_eq!(image.format, GlyphFormat::Mask);
    assert!(image.width > 4 && image.height > 6);
    assert_eq!(image.data.len(), (image.width * image.height) as usize);
    assert!(image.data.iter().any(|&c| c > 200), "solid coverage inside");
    assert!(image.top > 8, "an A stands above the baseline");
  }

  #[test]
  fn every_builtin_font_keeps_its_notdef_box() {
    // A character no font has draws glyph 0; a subset without
    // `--notdef-outline` would draw nothing there
    let fonts = Fonts::with_builtins();
    let mut raster = GlyphRasterizer::new();
    for role in [FontRole::Ui, FontRole::Mono, FontRole::Pixel] {
      let font = fonts.role(role).unwrap();
      for key in [
        GlyphKey::fill(font, 0, 32.0, 0.0),
        GlyphKey::msdf(font, 0, 32.0, 4.0),
      ] {
        assert!(
          raster.rasterize(&fonts, None, &key).is_some(),
          "{role:?} {key:?}"
        );
      }
    }
  }

  #[test]
  fn a_space_has_no_image() {
    let fonts = Fonts::with_builtins();
    let font = ui(&fonts);
    let glyph = glyph_id(&fonts, font, ' ').unwrap();
    assert!(
      GlyphRasterizer::new()
        .rasterize(&fonts, None, &GlyphKey::fill(font, glyph, 16.0, 0.0))
        .is_none()
    );
  }

  #[test]
  fn subpixel_bins_shift_the_glyph() {
    let fonts = Fonts::with_builtins();
    let font = ui(&fonts);
    let glyph = glyph_id(&fonts, font, 'l').unwrap();
    let mut raster = GlyphRasterizer::new();
    let a = raster
      .rasterize(&fonts, None, &GlyphKey::fill(font, glyph, 16.0, 0.0))
      .unwrap();
    let b = raster
      .rasterize(&fonts, None, &GlyphKey::fill(font, glyph, 16.0, 0.5))
      .unwrap();
    assert!(a.data != b.data || a.left != b.left);
    assert_eq!(GlyphKey::fill(font, glyph, 16.0, 0.6).subpixel, 2);
    assert_eq!(
      GlyphKey::fill(font, glyph, 16.0, 0.99).subpixel,
      0,
      "rounds up to the next whole pixel"
    );
  }

  #[test]
  fn a_stroke_is_wider_than_the_fill() {
    let fonts = Fonts::with_builtins();
    let font = ui(&fonts);
    let glyph = glyph_id(&fonts, font, 'O').unwrap();
    let mut raster = GlyphRasterizer::new();
    let filled = raster
      .rasterize(&fonts, None, &GlyphKey::fill(font, glyph, 24.0, 0.0))
      .unwrap();
    let stroked = raster
      .rasterize(&fonts, None, &GlyphKey::stroke(font, glyph, 24.0, 0.0, 4.0))
      .unwrap();
    assert!(
      stroked.width >= filled.width + 3,
      "{} vs {}",
      stroked.width,
      filled.width
    );
    assert!(stroked.left < filled.left);
  }

  #[test]
  fn pixel_glyphs_are_fully_on_or_off() {
    let fonts = Fonts::with_builtins();
    let font = fonts.role(FontRole::Pixel).unwrap();
    let glyph = glyph_id(&fonts, font, 'K').unwrap();
    let image = GlyphRasterizer::new()
      .rasterize(&fonts, None, &GlyphKey::pixel(font, glyph, 16.0))
      .unwrap();
    assert!(image.data.iter().all(|&c| c == 0 || c == 255));
    assert!(image.data.contains(&255));
  }

  #[test]
  fn an_msdf_glyph_has_an_inside_and_an_outside() {
    let fonts = Fonts::with_builtins();
    let font = ui(&fonts);
    let glyph = glyph_id(&fonts, font, 'A').unwrap();
    let image = GlyphRasterizer::new()
      .rasterize(&fonts, None, &GlyphKey::msdf(font, glyph, 32.0, 4.0))
      .unwrap();
    assert_eq!(image.format, GlyphFormat::Msdf);
    assert_eq!(image.base, 32.0);
    assert_eq!(image.data.len(), (image.width * image.height * 4) as usize);
    let median = |p: &[u8]| {
      let (r, g, b) = (p[0], p[1], p[2]);
      r.min(g).max(r.max(g).min(b))
    };
    let medians: Vec<u8> = image
      .data
      .as_chunks::<4>()
      .0
      .iter()
      .map(|p| median(p))
      .collect();
    assert!(medians.iter().any(|&m| m > 160), "inside");
    assert!(medians.iter().any(|&m| m < 90), "outside");
  }

  #[test]
  fn synthetic_italic_changes_the_image() {
    let fonts = Fonts::with_builtins();
    let font = ui(&fonts);
    let glyph = glyph_id(&fonts, font, 'I').unwrap();
    let mut raster = GlyphRasterizer::new();
    let upright = raster
      .rasterize(&fonts, None, &GlyphKey::fill(font, glyph, 32.0, 0.0))
      .unwrap();
    let mut key = GlyphKey::fill(font, glyph, 32.0, 0.0);
    key.skew = true;
    let slanted = raster.rasterize(&fonts, None, &key).unwrap();
    assert!(slanted.width > upright.width);
  }

  #[test]
  fn unknown_fonts_and_huge_sizes_give_nothing() {
    let fonts = Fonts::with_builtins();
    let mut raster = GlyphRasterizer::new();
    assert!(
      raster
        .rasterize(&fonts, None, &GlyphKey::fill(FontId(999), 36, 16.0, 0.0))
        .is_none()
    );
    let font = ui(&fonts);
    let glyph = glyph_id(&fonts, font, 'A').unwrap();
    assert!(
      raster
        .rasterize(&fonts, None, &GlyphKey::fill(font, glyph, 5000.0, 0.0))
        .is_none()
    );
  }

  #[test]
  fn raster_sizes_step_as_they_grow() {
    assert_eq!(raster_size_step(10.4), 10.0);
    assert_eq!(raster_size_step(31.6), 32.0);
    assert_eq!(raster_size_step(47.0), 48.0);
    assert_eq!(raster_size_step(63.0), 64.0);
    assert_eq!(raster_size_step(101.0), 100.0);
    assert_eq!(raster_size_step(0.2), 1.0);
  }

  #[test]
  fn bad_widths_ranges_and_sizes_give_nothing() {
    let fonts = Fonts::with_builtins();
    let font = ui(&fonts);
    let glyph = glyph_id(&fonts, font, 'A').unwrap();
    let mut raster = GlyphRasterizer::new();
    for width in [0.0, -3.0, f32::NAN, f32::INFINITY, 1.0e9] {
      assert!(
        raster
          .rasterize(
            &fonts,
            None,
            &GlyphKey::stroke(font, glyph, 16.0, 0.0, width)
          )
          .is_none(),
        "{width}"
      );
    }
    for range in [0.0, -1.0, f32::NAN, f32::INFINITY, 1.0e9] {
      assert!(
        raster
          .rasterize(&fonts, None, &GlyphKey::msdf(font, glyph, 32.0, range))
          .is_none(),
        "{range}"
      );
    }
    for px in [0.0, -4.0, f32::NAN, f32::INFINITY] {
      assert!(
        raster
          .rasterize(&fonts, None, &GlyphKey::fill(font, glyph, px, 0.0))
          .is_none(),
        "{px}"
      );
      assert!(
        raster
          .rasterize(&fonts, None, &GlyphKey::msdf(font, glyph, px, 4.0))
          .is_none(),
        "{px}"
      );
    }
  }

  /// Ink bounds (left, top, right, bottom) in pen/baseline pixels, y up
  fn ink(image: &GlyphImage, inside: impl Fn(&[u8]) -> bool) -> (f32, f32, f32, f32) {
    let bpp = if image.format == GlyphFormat::Mask {
      1
    } else {
      4
    };
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
    for y in 0..image.height {
      for x in 0..image.width {
        let i = ((y * image.width + x) as usize) * bpp;
        if inside(&image.data[i..i + bpp]) {
          x0 = x0.min(x);
          y0 = y0.min(y);
          x1 = x1.max(x + 1);
          y1 = y1.max(y + 1);
        }
      }
    }
    let (l, t) = (image.left as f32, image.top as f32);
    (l + x0 as f32, t - y0 as f32, l + x1 as f32, t - y1 as f32)
  }

  #[test]
  fn msdf_ink_lands_where_the_fill_ink_does() {
    let fonts = Fonts::with_builtins();
    let font = ui(&fonts);
    let mut raster = GlyphRasterizer::new();
    for c in ['H', 'o', 'g'] {
      let glyph = glyph_id(&fonts, font, c).unwrap();
      // The fill is made unhinted (past the hinting limit) at twice the size
      // and halved, so grid-fitting does not move its edges
      let fill = raster
        .rasterize(&fonts, None, &GlyphKey::fill(font, glyph, 64.0, 0.0))
        .unwrap();
      let msdf = raster
        .rasterize(&fonts, None, &GlyphKey::msdf(font, glyph, 32.0, 4.0))
        .unwrap();
      let a = ink(&fill, |p| p[0] >= 128);
      let a = (a.0 / 2.0, a.1 / 2.0, a.2 / 2.0, a.3 / 2.0);
      let b = ink(&msdf, |p| {
        p[0].min(p[1]).max(p[0].max(p[1]).min(p[2])) >= 128
      });
      for (x, y) in [(a.0, b.0), (a.1, b.1), (a.2, b.2), (a.3, b.3)] {
        assert!((x - y).abs() <= 0.5, "{c}: fill {a:?} vs msdf {b:?}");
      }
    }
  }

  #[test]
  fn repeated_rasterizing_is_identical() {
    let fonts = Fonts::with_builtins();
    let font = ui(&fonts);
    let glyph = glyph_id(&fonts, font, 'g').unwrap();
    let mut raster = GlyphRasterizer::new();
    for key in [
      GlyphKey::fill(font, glyph, 16.0, 0.25),
      GlyphKey::msdf(font, glyph, 32.0, 4.0),
    ] {
      let first = raster.rasterize(&fonts, None, &key).unwrap();
      assert_eq!(raster.rasterize(&fonts, None, &key).unwrap(), first);
      assert_eq!(
        GlyphRasterizer::new()
          .rasterize(&fonts, None, &key)
          .unwrap(),
        first
      );
    }
  }
}
