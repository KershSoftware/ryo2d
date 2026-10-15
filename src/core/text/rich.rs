//! Rich text: one string with styled spans, inline icons, typewriter pauses
//! and effect ranges. Built by appending, so spans never overlap.

use std::hash::{Hash, Hasher};
use std::ops::Range;

use glam::Vec2;

use crate::core::asset::SpriteHandle;
use crate::core::text::style::{Outline, TextFont, hash_outline};
use crate::core::ui::Color;

/// Overrides for one run of text; unset fields keep the base style
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub struct Span {
  pub color: Option<Color>,
  pub size: Option<f32>,
  pub font: Option<TextFont>,
  pub weight: Option<f32>,
  pub italic: Option<bool>,
  pub outline: Option<Outline>,
  /// A per-glyph effect by name (`wave`, `shake`, `rainbow`, or one the
  /// game registered in `TextEffects`)
  pub effect: Option<String>,
}

impl Span {
  pub fn new() -> Self {
    Self::default()
  }
  pub fn color(color: Color) -> Self {
    Self::new().with_color(color)
  }
  pub fn size(size: f32) -> Self {
    Self::new().with_size(size)
  }
  pub fn font(font: TextFont) -> Self {
    Self::new().with_font(font)
  }
  pub fn bold() -> Self {
    Self::new().with_weight(700.0)
  }
  pub fn italic() -> Self {
    Self::new().with_italic(true)
  }
  pub fn outline(width: f32, color: Color) -> Self {
    Self::new().with_outline(width, color)
  }
  pub fn effect(name: impl Into<String>) -> Self {
    Self::new().with_effect(name)
  }
  pub fn with_color(mut self, color: Color) -> Self {
    self.color = Some(color);
    self
  }
  pub fn with_size(mut self, size: f32) -> Self {
    self.size = Some(size);
    self
  }
  pub fn with_font(mut self, font: TextFont) -> Self {
    self.font = Some(font);
    self
  }
  pub fn with_weight(mut self, weight: f32) -> Self {
    self.weight = Some(weight);
    self
  }
  pub fn with_italic(mut self, italic: bool) -> Self {
    self.italic = Some(italic);
    self
  }
  pub fn with_outline(mut self, width: f32, color: Color) -> Self {
    self.outline = Some(Outline { width, color });
    self
  }
  pub fn with_effect(mut self, name: impl Into<String>) -> Self {
    self.effect = Some(name.into());
    self
  }

  /// This span laid over another (the markup parser nests tags with it)
  #[cfg_attr(not(test), allow(dead_code))]
  pub(crate) fn over(&self, base: &Span) -> Span {
    Span {
      color: self.color.or(base.color),
      size: self.size.or(base.size),
      font: self.font.clone().or_else(|| base.font.clone()),
      weight: self.weight.or(base.weight),
      italic: self.italic.or(base.italic),
      outline: self.outline.or(base.outline),
      effect: self.effect.clone().or_else(|| base.effect.clone()),
    }
  }

  fn hash_into(&self, h: &mut impl Hasher) {
    self.color.hash(h);
    self.size.map(f32::to_bits).hash(h);
    self.font.hash(h);
    self.weight.map(f32::to_bits).hash(h);
    self.italic.hash(h);
    hash_outline(&self.outline, h);
    self.effect.hash(h);
  }
}

/// A picture inline with text
#[derive(Debug, Clone)]
pub enum Icon {
  /// A sprite; `size` in text units, default a square the line's height
  Sprite {
    sprite: SpriteHandle,
    size: Option<Vec2>,
  },
  /// A name looked up in `TextIcons` when the text is drawn
  Named { name: String, size: Option<Vec2> },
}

// `SpriteHandle` has no `PartialEq`: two sprite icons are equal when they
// name the same handle (its serial)
impl PartialEq for Icon {
  fn eq(&self, other: &Self) -> bool {
    match (self, other) {
      (
        Self::Sprite {
          sprite: a,
          size: sa,
        },
        Self::Sprite {
          sprite: b,
          size: sb,
        },
      ) => a.serial() == b.serial() && sa == sb,
      (Self::Named { name: a, size: sa }, Self::Named { name: b, size: sb }) => a == b && sa == sb,
      _ => false,
    }
  }
}

impl Icon {
  pub(crate) fn size(&self) -> Option<Vec2> {
    match self {
      Self::Sprite { size, .. } | Self::Named { size, .. } => *size,
    }
  }

  fn hash_into(&self, h: &mut impl Hasher) {
    match self {
      Self::Sprite { sprite, size } => {
        0u8.hash(h);
        sprite.serial().hash(h);
        size.map(|s| (s.x.to_bits(), s.y.to_bits())).hash(h);
      }
      Self::Named { name, size } => {
        1u8.hash(h);
        name.hash(h);
        size.map(|s| (s.x.to_bits(), s.y.to_bits())).hash(h);
      }
    }
  }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SpanMark {
  pub(crate) range: Range<usize>,
  pub(crate) span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct IconMark {
  /// Byte index the icon sits before
  pub(crate) at: usize,
  pub(crate) icon: Icon,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PauseMark {
  pub(crate) at: usize,
  pub(crate) seconds: f32,
}

/// Text with styled spans, inline icons and typewriter pauses
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RichText {
  pub(crate) text: String,
  pub(crate) spans: Vec<SpanMark>,
  pub(crate) icons: Vec<IconMark>,
  pub(crate) pauses: Vec<PauseMark>,
}

impl RichText {
  /// Plain text in the base style
  pub fn new(text: impl Into<String>) -> Self {
    Self {
      text: text.into(),
      ..Self::default()
    }
  }

  /// Append text in the base style
  pub fn plain(mut self, text: &str) -> Self {
    self.text.push_str(text);
    self
  }

  /// Append text with overrides
  pub fn span(mut self, text: &str, span: Span) -> Self {
    let start = self.text.len();
    self.text.push_str(text);
    if !text.is_empty() {
      self.spans.push(SpanMark {
        range: start..self.text.len(),
        span,
      });
    }
    self
  }

  /// Append a sprite as big as the line is high
  pub fn icon(self, sprite: SpriteHandle) -> Self {
    self.push_icon(Icon::Sprite { sprite, size: None })
  }

  /// Append a sprite at a size in text units
  pub fn icon_sized(self, sprite: SpriteHandle, size: Vec2) -> Self {
    self.push_icon(Icon::Sprite {
      sprite,
      size: Some(size),
    })
  }

  /// Append the icon `TextIcons` maps this name to
  pub fn icon_named(self, name: impl Into<String>) -> Self {
    self.push_icon(Icon::Named {
      name: name.into(),
      size: None,
    })
  }

  /// The characters (icons and markup excluded)
  pub fn as_str(&self) -> &str {
    &self.text
  }

  pub fn is_empty(&self) -> bool {
    self.text.is_empty() && self.icons.is_empty()
  }

  fn push_icon(mut self, icon: Icon) -> Self {
    self.icons.push(IconMark {
      at: self.text.len(),
      icon,
    });
    self
  }

  pub(crate) fn hash_into(&self, h: &mut impl Hasher) {
    self.text.hash(h);
    for mark in &self.spans {
      (mark.range.start, mark.range.end).hash(h);
      mark.span.hash_into(h);
    }
    for mark in &self.icons {
      mark.at.hash(h);
      mark.icon.hash_into(h);
    }
    // Pauses only time the reveal; they never change the layout
  }
}

impl From<&str> for RichText {
  fn from(text: &str) -> Self {
    Self::new(text)
  }
}

impl From<String> for RichText {
  fn from(text: String) -> Self {
    Self::new(text)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn a_span_over_another_keeps_what_it_does_not_set() {
    let red = Color::rgb(255, 0, 0);
    let inner = Span::bold().with_size(20.0);
    let outer = Span::color(red).with_size(10.0).with_effect("wave");
    let nested = inner.over(&outer);
    assert_eq!(nested.color, Some(red));
    assert_eq!(nested.size, Some(20.0), "the inner span wins");
    assert_eq!(nested.weight, Some(700.0));
    assert_eq!(nested.effect.as_deref(), Some("wave"));
  }

  #[test]
  fn appending_spans_and_icons_marks_their_bytes() {
    let text = RichText::new("Hi ")
      .span("there", Span::italic())
      .icon_named("star")
      .span("", Span::bold());
    assert_eq!(text.as_str(), "Hi there");
    assert_eq!(text.spans.len(), 1, "an empty span marks nothing");
    assert_eq!(text.spans[0].range, 3..8);
    assert_eq!(text.icons[0].at, 8);
    assert!(!RichText::new("").icon_named("star").is_empty());
  }
}
