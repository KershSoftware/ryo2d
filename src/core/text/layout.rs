//! A laid-out text block: positioned glyphs and icon boxes in layout pixels
//! (y down, origin at the block's top-left)

use std::ops::Range;

use glam::Vec2;

use crate::core::text::FontId;
use crate::core::text::rich::Icon;
use crate::core::text::style::{Glow, Outline, Shadow};
use crate::core::ui::{Color, Rect};

/// The look of one run, resolved from the base style and its span
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GlyphStyle {
  pub(crate) color: Color,
  pub(crate) outline: Option<Outline>,
  pub(crate) shadow: Option<Shadow>,
  pub(crate) glow: Option<Glow>,
  pub(crate) effect: Option<String>,
}

/// One glyph, placed
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PlacedGlyph {
  pub(crate) font: FontId,
  /// Glyph id in its font (for a bitmap font: the cell index)
  pub(crate) glyph: u16,
  /// The glyph's origin on its baseline, layout pixels
  pub(crate) pos: Vec2,
  /// Font size, layout pixels
  pub(crate) size: f32,
  /// Index into `TextLayout::styles`
  pub(crate) style: u16,
  /// Its character cluster in logical order (reveal and effects count these)
  pub(crate) cluster: u32,
  /// Synthesized italic (a skew) and bold (an embolden)
  pub(crate) skew: bool,
  pub(crate) embolden: bool,
  /// Hash of a variable font's normalized coordinates; 0 for none
  pub(crate) coords: u64,
}

/// An inline icon's box
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PlacedIcon {
  pub(crate) icon: Icon,
  pub(crate) rect: Rect,
  /// Revealed together with this cluster
  pub(crate) cluster: u32,
}

/// One line of the block
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LineInfo {
  pub(crate) top: f32,
  pub(crate) baseline: f32,
  pub(crate) height: f32,
  pub(crate) range: Range<usize>,
}

/// A text block ready to draw
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct TextLayout {
  /// Layout pixels
  pub(crate) size: Vec2,
  /// Layout pixels per text unit
  pub(crate) scale: f32,
  pub(crate) glyphs: Vec<PlacedGlyph>,
  pub(crate) icons: Vec<PlacedIcon>,
  pub(crate) lines: Vec<LineInfo>,
  pub(crate) styles: Vec<GlyphStyle>,
  /// Character clusters in the block (the reveal's unit)
  pub(crate) clusters: u32,
  /// Typewriter pauses: (cluster the pause comes before, seconds)
  pub(crate) pauses: Vec<(u32, f32)>,
  /// Clusters that are punctuation (`. , ! ? ; :` and their full-width forms)
  pub(crate) punctuation: Vec<u32>,
  /// Every glyph is a pixel or bitmap font's
  pub(crate) pixel: bool,
}
