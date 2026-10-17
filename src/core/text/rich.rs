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
  /// A sprite; `size` in text units, default a square as high as the font
  /// size
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

/// An input prompt before a byte: its action, and the style of the markup
/// span it was written in, which its label takes even at the span's edge
/// (where no span covers the empty prompt)
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PromptMark {
  pub(crate) at: usize,
  pub(crate) action: String,
  /// The span around it in markup (`None` in the base style)
  pub(crate) span: Option<Span>,
}

impl PromptMark {
  fn hash_into(&self, h: &mut impl Hasher) {
    self.at.hash(h);
    self.action.hash(h);
    self.span.is_some().hash(h);
    if let Some(span) = &self.span {
      span.hash_into(h);
    }
  }
}

/// In tests a mark in the base style reads as its (byte, action)
#[cfg(test)]
impl PartialEq<(usize, String)> for PromptMark {
  fn eq(&self, (at, action): &(usize, String)) -> bool {
    self.at == *at && self.action == *action && self.span.is_none()
  }
}

/// Text with styled spans, inline icons, typewriter pauses and input prompts
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RichText {
  pub(crate) text: String,
  pub(crate) spans: Vec<SpanMark>,
  pub(crate) icons: Vec<IconMark>,
  pub(crate) pauses: Vec<PauseMark>,
  /// Input prompts, replaced by their action's binding before the text is
  /// laid out (`resolve_prompts`). At the same byte, icons come before
  /// prompts.
  pub(crate) prompts: Vec<PromptMark>,
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

  /// Append a sprite: a square as high as the font size
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

  /// Append a prompt for `action`: the key or button that triggers it on
  /// the player's last device, as a registered glyph or a label such as
  /// `[A]` (the `[prompt=action]` markup tag)
  pub fn prompt(mut self, action: &str) -> Self {
    self.prompts.push(PromptMark {
      at: self.text.len(),
      action: action.to_string(),
      span: None,
    });
    self
  }

  /// The characters (icons and markup excluded)
  pub fn as_str(&self) -> &str {
    &self.text
  }

  pub fn is_empty(&self) -> bool {
    self.text.is_empty() && self.icons.is_empty() && self.prompts.is_empty()
  }

  /// Insert `icon` before byte `at`, after the icons already there.
  /// Skipped when `at` is not a character boundary.
  pub(crate) fn insert_icon(&mut self, at: usize, icon: Icon) {
    if !self.text.is_char_boundary(at) {
      return;
    }
    let place = self
      .icons
      .iter()
      .position(|mark| mark.at > at)
      .unwrap_or(self.icons.len());
    self.icons.insert(place, IconMark { at, icon });
  }

  /// Insert `text` at byte `at` with `span` (laid over a span it falls
  /// inside, which splits around it). Marks after `at` move with the text;
  /// marks at `at` stay before it. Skipped when `at` is not a character
  /// boundary.
  pub(crate) fn insert_text(&mut self, at: usize, text: &str, span: Span) {
    if text.is_empty() || !self.text.is_char_boundary(at) {
      return;
    }
    self.text.insert_str(at, text);
    let len = text.len();
    let mut inserted = SpanMark {
      range: at..at + len,
      span,
    };
    let mut spans = Vec::with_capacity(self.spans.len() + 2);
    for mark in self.spans.drain(..) {
      let Range { start, end } = mark.range;
      if end <= at {
        spans.push(mark);
      } else if start >= at {
        spans.push(SpanMark {
          range: start + len..end + len,
          span: mark.span,
        });
      } else {
        // `at` falls inside: the span goes on either side of the text,
        // which takes what the span sets and it does not
        inserted.span = inserted.span.over(&mark.span);
        spans.push(SpanMark {
          range: start..at,
          span: mark.span.clone(),
        });
        spans.push(SpanMark {
          range: at + len..end + len,
          span: mark.span,
        });
      }
    }
    let place = spans
      .iter()
      .position(|mark| mark.range.start > at)
      .unwrap_or(spans.len());
    spans.insert(place, inserted);
    self.spans = spans;
    for mark in &mut self.icons {
      if mark.at > at {
        mark.at += len;
      }
    }
    for mark in &mut self.pauses {
      if mark.at > at {
        mark.at += len;
      }
    }
    for mark in &mut self.prompts {
      if mark.at > at {
        mark.at += len;
      }
    }
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
    // Counts first, so the spans and icons cannot run into each other
    (self.spans.len(), self.icons.len()).hash(h);
    for mark in &self.spans {
      (mark.range.start, mark.range.end).hash(h);
      mark.span.hash_into(h);
    }
    for mark in &self.icons {
      mark.at.hash(h);
      mark.icon.hash_into(h);
    }
    // Pauses are part of the layout (it carries them for the reveal)
    self.pauses.len().hash(h);
    for mark in &self.pauses {
      mark.at.hash(h);
      mark.seconds.to_bits().hash(h);
    }
    // Prompts too, so texts differing only in their prompts never share
    self.prompts.len().hash(h);
    for mark in &self.prompts {
      mark.hash_into(h);
    }
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

  #[test]
  fn inserted_text_moves_the_marks_after_it() {
    let mut text = RichText::new("ab")
      .icon_named("at")
      .pause(1.0)
      .prompt("p")
      .span("cd", Span::italic())
      .icon_named("after");
    text.insert_text(2, "XY", Span::bold());
    assert_eq!(text.as_str(), "abXYcd");
    let ranges: Vec<_> = text.spans.iter().map(|m| m.range.clone()).collect();
    assert_eq!(ranges, vec![2..4, 4..6], "in order, the italic moved");
    assert_eq!(text.spans[0].span.weight, Some(700.0));
    assert_eq!(text.icons[0].at, 2, "a mark at the byte stays before");
    assert_eq!(text.icons[1].at, 6);
    assert_eq!(text.pauses[0].at, 2);
    assert_eq!(text.prompts, vec![(2, "p".to_string())]);
    text.insert_text(1, "\u{e9}", Span::bold());
    text.insert_text(2, "z", Span::bold());
    assert_eq!(
      text.as_str(),
      "a\u{e9}bXYcd",
      "off a char boundary: skipped"
    );
  }

  #[test]
  fn prompts_count_in_the_hash_and_emptiness() {
    let hash = |text: &RichText| {
      let mut h = std::hash::DefaultHasher::new();
      text.hash_into(&mut h);
      h.finish()
    };
    let jump = RichText::new("Press ").prompt("jump");
    let fire = RichText::new("Press ").prompt("fire");
    assert_ne!(hash(&jump), hash(&fire));
    assert!(!RichText::new("").prompt("jump").is_empty());
  }
}
