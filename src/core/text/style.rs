//! Text styles and layout options

use std::hash::{Hash, Hasher};

use glam::Vec2;

use crate::core::text::{FontId, FontRole};
use crate::core::ui::{Anchor, Color};

/// Which font a style asks for
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TextFont {
  /// A built-in role's font (the game can point a role at its own font)
  Role(FontRole),
  /// A family by name
  Family(String),
  /// One exact face: its family, at its own weight and slant (the style's
  /// `weight` and `italic` do not apply)
  Id(FontId),
}

impl Default for TextFont {
  fn default() -> Self {
    Self::Role(FontRole::Ui)
  }
}

/// A line around each glyph
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Outline {
  /// Width in text units (pixels in the UI)
  pub width: f32,
  pub color: Color,
}

/// A copy of the text under it
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shadow {
  /// Offset in text units, +y down
  pub offset: Vec2,
  pub color: Color,
  /// Blur radius in text units (MSDF fonts only; raster shadows are hard)
  pub softness: f32,
}

/// A soft halo around the glyphs (MSDF fonts only)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Glow {
  pub radius: f32,
  pub color: Color,
}

/// How text looks
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct TextStyle {
  pub font: TextFont,
  /// Tried in order when the font lacks a character; after them come the
  /// UI font, then the mono font
  pub fallbacks: Vec<TextFont>,
  /// Size in text units: pixels in the UI, world units on entities
  pub size: f32,
  /// 100 (thin) to 900 (black); 400 is regular
  pub weight: f32,
  pub italic: bool,
  pub color: Color,
  /// Line height as a multiple of the size
  pub line_height: f32,
  /// Extra space between characters, in text units
  pub letter_spacing: f32,
  pub outline: Option<Outline>,
  pub shadow: Option<Shadow>,
  pub glow: Option<Glow>,
}

impl Default for TextStyle {
  fn default() -> Self {
    Self {
      font: TextFont::default(),
      fallbacks: Vec::new(),
      size: 14.0,
      weight: 400.0,
      italic: false,
      color: Color::WHITE,
      line_height: 1.2,
      letter_spacing: 0.0,
      outline: None,
      shadow: None,
      glow: None,
    }
  }
}

impl TextStyle {
  pub fn new() -> Self {
    Self::default()
  }
  pub fn font(mut self, font: TextFont) -> Self {
    self.font = font;
    self
  }
  pub fn role(self, role: FontRole) -> Self {
    self.font(TextFont::Role(role))
  }
  pub fn family(self, family: impl Into<String>) -> Self {
    self.font(TextFont::Family(family.into()))
  }
  pub fn size(mut self, size: f32) -> Self {
    self.size = size;
    self
  }
  pub fn weight(mut self, weight: f32) -> Self {
    self.weight = weight;
    self
  }
  pub fn bold(self) -> Self {
    self.weight(700.0)
  }
  pub fn italic(mut self) -> Self {
    self.italic = true;
    self
  }
  pub fn color(mut self, color: Color) -> Self {
    self.color = color;
    self
  }
  pub fn line_height(mut self, line_height: f32) -> Self {
    self.line_height = line_height;
    self
  }
  pub fn letter_spacing(mut self, spacing: f32) -> Self {
    self.letter_spacing = spacing;
    self
  }
  pub fn outline(mut self, width: f32, color: Color) -> Self {
    self.outline = Some(Outline { width, color });
    self
  }
  pub fn shadow(mut self, offset: Vec2, color: Color, softness: f32) -> Self {
    self.shadow = Some(Shadow {
      offset,
      color,
      softness,
    });
    self
  }
  pub fn glow(mut self, radius: f32, color: Color) -> Self {
    self.glow = Some(Glow { radius, color });
    self
  }
  pub fn fallback(mut self, font: TextFont) -> Self {
    self.fallbacks.push(font);
    self
  }

  /// Size and line height usable for layout
  pub(crate) fn is_valid(&self) -> bool {
    self.invalid_field().is_none()
  }

  /// The first field that keeps the style from being laid out, and its
  /// value: a size or line height that is not a positive number, or a
  /// weight or letter spacing that is not a number
  pub(crate) fn invalid_field(&self) -> Option<(&'static str, f32)> {
    let positive = |v: f32| v.is_finite() && v > 0.0;
    [
      ("size", self.size, positive(self.size)),
      ("line height", self.line_height, positive(self.line_height)),
      ("weight", self.weight, self.weight.is_finite()),
      (
        "letter spacing",
        self.letter_spacing,
        self.letter_spacing.is_finite(),
      ),
    ]
    .into_iter()
    .find(|&(_, _, ok)| !ok)
    .map(|(field, value, _)| (field, value))
  }

  /// Feed everything that changes layout or look into a cache key
  pub(crate) fn hash_into(&self, h: &mut impl Hasher) {
    self.font.hash(h);
    self.fallbacks.hash(h);
    for value in [
      self.size,
      self.weight,
      self.line_height,
      self.letter_spacing,
    ] {
      value.to_bits().hash(h);
    }
    self.italic.hash(h);
    self.color.hash(h);
    // Each effect hashes as an `Option`, so its presence is part of the key
    // (an outline and a glow with the same numbers differ)
    hash_outline(&self.outline, h);
    self
      .shadow
      .map(|shadow| {
        (
          shadow.offset.x.to_bits(),
          shadow.offset.y.to_bits(),
          shadow.softness.to_bits(),
          shadow.color,
        )
      })
      .hash(h);
    self
      .glow
      .map(|glow| (glow.radius.to_bits(), glow.color))
      .hash(h);
  }
}

pub(crate) fn hash_outline(outline: &Option<Outline>, h: &mut impl Hasher) {
  outline
    .map(|outline| (outline.width.to_bits(), outline.color))
    .hash(h);
}

/// Horizontal alignment of each line within the block
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TextAlign {
  #[default]
  Start,
  Center,
  End,
  /// Stretch spaces so every line but the last fills the wrap width
  Justify,
}

/// What happens to text past `max_lines`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TextOverflow {
  #[default]
  Clip,
  /// The last kept line ends in "…"
  Ellipsis,
}

/// How a text block is broken and placed
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct TextLayoutOptions {
  /// Wrap width in text units; `None` keeps each line whole
  pub wrap: Option<f32>,
  pub align: TextAlign,
  /// Lines past this many are dropped (`overflow` says how the last kept
  /// one ends); 0 lays out an empty block of size zero
  pub max_lines: Option<u32>,
  pub overflow: TextOverflow,
  /// The point of the block that sits on the text's position
  pub anchor: Anchor,
}

impl Default for TextLayoutOptions {
  fn default() -> Self {
    Self {
      wrap: None,
      align: TextAlign::Start,
      max_lines: None,
      overflow: TextOverflow::Clip,
      anchor: Anchor::TopLeft,
    }
  }
}

impl TextLayoutOptions {
  pub fn wrap(mut self, width: f32) -> Self {
    self.wrap = Some(width);
    self
  }
  pub fn align(mut self, align: TextAlign) -> Self {
    self.align = align;
    self
  }
  pub fn max_lines(mut self, lines: u32) -> Self {
    self.max_lines = Some(lines);
    self
  }
  pub fn overflow(mut self, overflow: TextOverflow) -> Self {
    self.overflow = overflow;
    self
  }
  pub fn anchor(mut self, anchor: Anchor) -> Self {
    self.anchor = anchor;
    self
  }

  pub(crate) fn is_valid(&self) -> bool {
    self.wrap.is_none_or(|w| w.is_finite() && w >= 0.0)
  }

  pub(crate) fn hash_into(&self, h: &mut impl Hasher) {
    self.wrap.map(f32::to_bits).hash(h);
    self.align.hash(h);
    self.max_lines.hash(h);
    self.overflow.hash(h);
    // The anchor only places the block; it never changes the layout
  }
}

#[cfg(test)]
mod tests {
  use std::hash::DefaultHasher;

  use super::*;

  fn key(style: &TextStyle) -> u64 {
    let mut h = DefaultHasher::new();
    style.hash_into(&mut h);
    h.finish()
  }

  #[test]
  fn the_style_key_tells_an_outline_from_a_glow() {
    let outline = TextStyle::default().outline(2.0, Color::BLACK);
    let glow = TextStyle::default().glow(2.0, Color::BLACK);
    assert_ne!(key(&outline), key(&glow));
    assert_ne!(key(&outline), key(&TextStyle::default()));
  }
}
