//! Monospace text for the dev windows' CPU pixel buffers (separate OS
//! windows that draw their own pixels), rasterized by swash from the
//! built-in mono font

use std::collections::HashMap;

use swash::FontRef;

use crate::core::text::{
  FaceSource, FontId, FontRole, Fonts, GlyphFormat, GlyphImage, GlyphKey, GlyphRasterizer, glyph_id,
};

pub(crate) struct CpuText {
  fonts: Fonts,
  font: FontId,
  px: f32,
  advance: i32,
  line_height: i32,
  ascent: i32,
  raster: GlyphRasterizer,
  glyphs: HashMap<char, Option<GlyphImage>>,
}

impl CpuText {
  /// The built-in monospace font at `px`; `None` without the `default-fonts`
  /// feature or when the font cannot be read
  pub(crate) fn mono(px: f32) -> Option<CpuText> {
    let fonts = Fonts::with_builtins();
    let font = fonts.role(FontRole::Mono)?;
    let FaceSource::Outline { data, index } = &fonts.face(font)?.source else {
      return None;
    };
    let font_ref = FontRef::from_index(data, *index as usize)?;
    let metrics = font_ref.metrics(&[]).scale(px);
    let zero = font_ref.charmap().map('0');
    let advance = font_ref
      .glyph_metrics(&[])
      .scale(px)
      .advance_width(zero)
      .round() as i32;
    let ascent = metrics.ascent.round() as i32;
    let line_height = ((metrics.ascent + metrics.descent).round() as i32).max(px.round() as i32);
    Some(CpuText {
      fonts,
      font,
      px,
      advance,
      line_height,
      ascent,
      raster: GlyphRasterizer::new(),
      glyphs: HashMap::new(),
    })
  }

  /// Pixels between one character's origin and the next
  #[cfg(test)]
  pub(crate) fn advance(&self) -> i32 {
    self.advance
  }

  /// Pixels between baselines of stacked lines
  pub(crate) fn line_height(&self) -> i32 {
    self.line_height
  }

  /// The width of `text` in pixels (characters, not bytes)
  pub(crate) fn measure(&self, text: &str) -> i32 {
    text.chars().count() as i32 * self.advance
  }

  /// Blend `text` into an RGBA8 `buffer` of `width` x `height`, its top-left
  /// at `(x, y)`; pixels outside the buffer are skipped
  #[allow(clippy::too_many_arguments)]
  pub(crate) fn draw(
    &mut self,
    buffer: &mut [u8],
    width: u32,
    height: u32,
    text: &str,
    x: i32,
    y: i32,
    color: [u8; 4],
  ) {
    let (width, height) = (width as i32, height as i32);
    let alpha = f32::from(color[3]) / 255.0;
    for (i, c) in text.chars().enumerate() {
      let pen_x = x.saturating_add((i as i32).saturating_mul(self.advance));
      if pen_x >= width || pen_x.saturating_add(self.advance) <= 0 {
        continue;
      }
      let (fonts, font, px, raster) = (&self.fonts, self.font, self.px, &mut self.raster);
      let Some(image) = self.glyphs.entry(c).or_insert_with(|| {
        let glyph = glyph_id(fonts, font, c).unwrap_or(0);
        raster
          .rasterize(fonts, None, &GlyphKey::fill(font, glyph, px, 0.0))
          .filter(|image| image.format == GlyphFormat::Mask)
      }) else {
        continue;
      };
      let left = pen_x + image.left;
      let top = y.saturating_add(self.ascent - image.top);
      for row in 0..image.height as i32 {
        let py = top.saturating_add(row);
        if py < 0 || py >= height {
          continue;
        }
        for col in 0..image.width as i32 {
          let pxl = left + col;
          if pxl < 0 || pxl >= width {
            continue;
          }
          let coverage = f32::from(image.data[(row as u32 * image.width + col as u32) as usize]);
          if coverage == 0.0 {
            continue;
          }
          let weight = coverage / 255.0 * alpha;
          let index = ((py * width + pxl) * 4) as usize;
          let Some(pixel) = buffer.get_mut(index..index + 4) else {
            continue;
          };
          for channel in 0..3 {
            let dst = f32::from(pixel[channel]);
            pixel[channel] = (dst + (f32::from(color[channel]) - dst) * weight).round() as u8;
          }
        }
      }
    }
  }
}

#[cfg(all(test, feature = "default-fonts"))]
mod tests {
  use super::*;

  #[test]
  fn monospace_text_measures_by_its_advance() {
    let text = CpuText::mono(12.0).unwrap();
    assert_eq!(
      text.advance(),
      7,
      "JetBrains Mono at 12 px advances 7 px, like the old font"
    );
    assert_eq!(text.measure("abcd"), 28);
    assert_eq!(
      text.measure("\u{00E9}t\u{00E9}"),
      21,
      "characters, not bytes"
    );
  }

  #[test]
  fn drawing_blends_glyphs_into_the_buffer() {
    let mut text = CpuText::mono(12.0).unwrap();
    let (w, h) = (40u32, 16u32);
    let mut buffer = vec![0u8; (w * h * 4) as usize];
    text.draw(&mut buffer, w, h, "Hi", 1, 1, [255, 255, 255, 255]);
    assert!(buffer.as_chunks::<4>().0.iter().any(|p| p[0] > 200));
    text.draw(&mut buffer, w, h, "far off", 500, 500, [255, 0, 0, 255]);
  }
}
