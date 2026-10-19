//! UI layout: the style vocabulary (lengths, edges, alignment) and the
//! engine that lays containers out with flexbox and grid. Only `engine.rs`
//! names the layout crate.

use std::hash::{Hash, Hasher};

use glam::Vec2;

use crate::core::ecs::Anchor;
use crate::core::hash::FastHasher;

pub(crate) mod engine;

/// A size along one axis
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Length {
  /// A fixed size in UI units
  Px(f32),
  /// A fraction of the parent, 0.0 to 1.0
  Pct(f32),
  /// Take what the parent has left (the rest of the main axis, all of the
  /// cross axis). A widget of its own size (a button, a slider, a progress
  /// bar, an image, a slot) is never smaller than that size, or than its
  /// `max_width`/`max_height` when that is smaller
  Fill,
  /// Fit the content
  #[default]
  Fit,
}

impl From<f32> for Length {
  fn from(px: f32) -> Length {
    Length::Px(px)
  }
}

/// Padding or margin on each side
#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Edges {
  /// Left side
  pub left: f32,
  /// Top side
  pub top: f32,
  /// Right side
  pub right: f32,
  /// Bottom side
  pub bottom: f32,
}

impl Edges {
  /// No edges
  pub const ZERO: Edges = Edges {
    left: 0.0,
    top: 0.0,
    right: 0.0,
    bottom: 0.0,
  };

  /// The same on every side
  pub fn all(v: f32) -> Edges {
    Edges::new(v, v, v, v)
  }

  /// `x` on the left and right, `y` on the top and bottom
  pub fn xy(x: f32, y: f32) -> Edges {
    Edges::new(x, y, x, y)
  }

  /// Each side
  pub fn new(left: f32, top: f32, right: f32, bottom: f32) -> Edges {
    Edges {
      left,
      top,
      right,
      bottom,
    }
  }

  /// Left plus right
  pub fn width(&self) -> f32 {
    self.left + self.right
  }

  /// Top plus bottom
  pub fn height(&self) -> f32 {
    self.top + self.bottom
  }
}

impl From<f32> for Edges {
  fn from(v: f32) -> Edges {
    Edges::all(v)
  }
}

impl From<(f32, f32)> for Edges {
  fn from((x, y): (f32, f32)) -> Edges {
    Edges::xy(x, y)
  }
}

/// Where children sit across a container's flow
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Align {
  /// At the start
  Start,
  /// Centred
  Center,
  /// At the end
  End,
  /// Stretched to fill
  #[default]
  Stretch,
}

/// How children are spread along a container's flow
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Justify {
  /// Packed at the start
  #[default]
  Start,
  /// Packed in the middle
  Center,
  /// Packed at the end
  End,
  /// Space between children, none at the ends
  SpaceBetween,
  /// Space around each child
  SpaceAround,
  /// Equal space between and around children
  SpaceEvenly,
}

/// How a container places its children
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) enum Flow {
  /// Top to bottom
  #[default]
  Column,
  /// Left to right
  Row,
  /// Cells in columns
  Grid(Tracks),
  /// All children overlap, placed by their anchors
  Stack,
}

/// A grid's column widths
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Tracks {
  /// This many columns sharing the width equally
  Equal(u16),
  /// One column per width
  Fixed(Vec<f32>),
}

/// Where a node's own size comes from
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Measure {
  /// From its style and children
  None,
  /// A fixed content size, for the style's `Fit` axes
  Fixed(Vec2),
  /// Sized by the measure callback (wrapped text)
  Text,
}

/// Everything layout reads from a node
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct UiStyle {
  pub(crate) flow: Flow,
  pub(crate) width: Length,
  pub(crate) height: Length,
  pub(crate) min_width: Option<f32>,
  pub(crate) max_width: Option<f32>,
  pub(crate) min_height: Option<f32>,
  pub(crate) max_height: Option<f32>,
  pub(crate) padding: Edges,
  pub(crate) margin: Edges,
  pub(crate) gap: f32,
  pub(crate) align: Align,
  pub(crate) justify: Justify,
  pub(crate) align_self: Option<Align>,
  pub(crate) grow: f32,
  pub(crate) wrap: bool,
  pub(crate) aspect: Option<f32>,
  pub(crate) scroll_x: bool,
  pub(crate) scroll_y: bool,
  /// Placement in a stack or the scene root
  pub(crate) anchor: Anchor,
  /// Applied after layout, in a stack or the scene root
  pub(crate) offset: Vec2,
  /// Out of its parent's flow: it takes no room and moves no sibling
  pub(crate) absolute: bool,
}

impl Flow {
  /// Feeds the flow and its tracks to `h`
  pub(crate) fn hash_into(&self, h: &mut FastHasher) {
    match self {
      Flow::Column => h.write_u8(0),
      Flow::Row => h.write_u8(1),
      Flow::Stack => h.write_u8(2),
      Flow::Grid(Tracks::Equal(n)) => {
        h.write_u8(3);
        h.write_u16(*n);
      }
      Flow::Grid(Tracks::Fixed(widths)) => {
        h.write_u8(4);
        h.write_usize(widths.len());
        for w in widths {
          h.write_u32(w.to_bits());
        }
      }
    }
  }
}

// `UiStyle::hash_into` packs `Align` into 3 bits (and `align_self`, one more
// than it, into 3), `Justify` into 3 and `Anchor` into 4. Each match below
// is exhaustive, so a new variant fails to compile here; the match then
// names the last variant, whose discriminant must still fit, or the field
// must be widened.
const fn last_align(align: Align) -> Align {
  match align {
    Align::Start | Align::Center | Align::End | Align::Stretch => Align::Stretch,
  }
}
const fn last_justify(justify: Justify) -> Justify {
  match justify {
    Justify::Start
    | Justify::Center
    | Justify::End
    | Justify::SpaceBetween
    | Justify::SpaceAround
    | Justify::SpaceEvenly => Justify::SpaceEvenly,
  }
}
const fn last_anchor(anchor: Anchor) -> Anchor {
  match anchor {
    Anchor::TopLeft
    | Anchor::TopCenter
    | Anchor::TopRight
    | Anchor::CenterLeft
    | Anchor::Center
    | Anchor::CenterRight
    | Anchor::BottomLeft
    | Anchor::BottomCenter
    | Anchor::BottomRight => Anchor::BottomRight,
  }
}
const _: () = assert!(
  (last_align(Align::Start) as u64) < 7,
  "Align outgrew its bits"
);
const _: () = assert!(
  (last_justify(Justify::Start) as u64) < 8,
  "Justify outgrew its bits"
);
const _: () = assert!(
  (last_anchor(Anchor::Center) as u64) < 16,
  "Anchor outgrew its bits"
);

impl UiStyle {
  /// A hash of every field, floats by their bits
  pub(crate) fn shape_hash(&self) -> u64 {
    let mut h = FastHasher::default();
    self.hash_into(&mut h);
    h.finish()
  }

  /// Every field into `h`, packed into few words (each write is a
  /// multiply): the tags and flags in one, the floats' bits two to a word
  fn hash_into(&self, h: &mut FastHasher) {
    fn bits(v: f32) -> u64 {
      u64::from(v.to_bits())
    }
    fn length(l: Length) -> (u64, u64) {
      match l {
        Length::Px(v) => (0, bits(v)),
        Length::Pct(v) => (1, bits(v)),
        Length::Fill => (2, 0),
        Length::Fit => (3, 0),
      }
    }
    fn opt(v: Option<f32>) -> (u64, u64) {
      v.map_or((0, 0), |v| (1, bits(v)))
    }
    let (width_tag, width) = length(self.width);
    let (height_tag, height) = length(self.height);
    let (min_w_tag, min_w) = opt(self.min_width);
    let (max_w_tag, max_w) = opt(self.max_width);
    let (min_h_tag, min_h) = opt(self.min_height);
    let (max_h_tag, max_h) = opt(self.max_height);
    let (aspect_tag, aspect) = opt(self.aspect);
    let align_self = self.align_self.map_or(0, |a| 1 + a as u64);
    let tags = width_tag
      | height_tag << 2
      | min_w_tag << 4
      | max_w_tag << 5
      | min_h_tag << 6
      | max_h_tag << 7
      | aspect_tag << 8
      | (self.align as u64) << 9
      | (self.justify as u64) << 12
      | align_self << 15
      | (self.anchor as u64) << 18
      | u64::from(self.wrap) << 22
      | u64::from(self.scroll_x) << 23
      | u64::from(self.scroll_y) << 24
      | u64::from(self.absolute) << 25;
    let (p, m) = (self.padding, self.margin);
    self.flow.hash_into(h);
    for word in [
      tags,
      width | height << 32,
      min_w | max_w << 32,
      min_h | max_h << 32,
      bits(p.left) | bits(p.top) << 32,
      bits(p.right) | bits(p.bottom) << 32,
      bits(m.left) | bits(m.top) << 32,
      bits(m.right) | bits(m.bottom) << 32,
      bits(self.gap) | bits(self.grow) << 32,
      aspect | bits(self.offset.x) << 32,
      bits(self.offset.y),
    ] {
      h.write_u64(word);
    }
  }
}

impl Default for UiStyle {
  fn default() -> UiStyle {
    UiStyle {
      flow: Flow::Column,
      width: Length::Fit,
      height: Length::Fit,
      min_width: None,
      max_width: None,
      min_height: None,
      max_height: None,
      padding: Edges::ZERO,
      margin: Edges::ZERO,
      gap: 0.0,
      align: Align::Stretch,
      justify: Justify::Start,
      align_self: None,
      grow: 0.0,
      wrap: false,
      aspect: None,
      scroll_x: false,
      scroll_y: false,
      anchor: Anchor::TopLeft,
      offset: Vec2::ZERO,
      absolute: false,
    }
  }
}

#[cfg(test)]
mod hash_tests {
  use super::*;

  /// A named change to a style
  type Change = (&'static str, fn(&mut UiStyle));

  /// Every field changes the hash, and so does a value's kind: a field
  /// left out of the packing would never be laid out again on its own
  #[test]
  fn every_style_field_moves_the_shape_hash() {
    let base = UiStyle::default();
    let changes: Vec<Change> = vec![
      ("flow", |s| s.flow = Flow::Row),
      ("grid", |s| s.flow = Flow::Grid(Tracks::Equal(3))),
      ("tracks", |s| s.flow = Flow::Grid(Tracks::Fixed(vec![1.0]))),
      ("width", |s| s.width = Length::Px(10.0)),
      ("width kind", |s| s.width = Length::Px(0.0)),
      ("width fill", |s| s.width = Length::Fill),
      ("width pct", |s| s.width = Length::Pct(0.0)),
      ("height", |s| s.height = Length::Px(10.0)),
      ("height kind", |s| s.height = Length::Fill),
      ("min width", |s| s.min_width = Some(0.0)),
      ("max width", |s| s.max_width = Some(0.0)),
      ("min height", |s| s.min_height = Some(0.0)),
      ("max height", |s| s.max_height = Some(0.0)),
      ("min width value", |s| s.min_width = Some(3.0)),
      ("max height value", |s| s.max_height = Some(3.0)),
      ("padding left", |s| s.padding.left = 1.0),
      ("padding top", |s| s.padding.top = 1.0),
      ("padding right", |s| s.padding.right = 1.0),
      ("padding bottom", |s| s.padding.bottom = 1.0),
      ("margin left", |s| s.margin.left = 1.0),
      ("margin top", |s| s.margin.top = 1.0),
      ("margin right", |s| s.margin.right = 1.0),
      ("margin bottom", |s| s.margin.bottom = 1.0),
      ("gap", |s| s.gap = 1.0),
      ("align", |s| s.align = Align::Center),
      ("justify", |s| s.justify = Justify::SpaceEvenly),
      ("align self", |s| s.align_self = Some(Align::Start)),
      ("align self value", |s| s.align_self = Some(Align::Stretch)),
      ("grow", |s| s.grow = 1.0),
      ("wrap", |s| s.wrap = true),
      ("aspect", |s| s.aspect = Some(0.0)),
      ("aspect value", |s| s.aspect = Some(2.0)),
      ("scroll x", |s| s.scroll_x = true),
      ("scroll y", |s| s.scroll_y = true),
      ("anchor", |s| s.anchor = Anchor::BottomRight),
      ("offset x", |s| s.offset.x = 1.0),
      ("offset y", |s| s.offset.y = 1.0),
      ("absolute", |s| s.absolute = true),
    ];
    let mut seen = vec![("default", base.shape_hash())];
    for (name, change) in changes {
      let mut style = base.clone();
      change(&mut style);
      let hash = style.shape_hash();
      if let Some((same, _)) = seen.iter().find(|(_, h)| *h == hash) {
        panic!("{name} hashes as {same}");
      }
      seen.push((name, hash));
    }
  }
}

#[cfg(test)]
mod guard {
  /// The layout crate stays inside src/core/ui/layout/
  #[test]
  fn taffy_is_named_only_in_the_layout_module() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut offenders = Vec::new();
    let mut stack = vec![root.clone()];
    let mut scanned = 0;
    while let Some(dir) = stack.pop() {
      for entry in std::fs::read_dir(&dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
          stack.push(path);
        } else if path.extension().is_some_and(|e| e == "rs")
          && !path.starts_with(root.join("core/ui/layout"))
        {
          scanned += 1;
          let source = std::fs::read_to_string(&path).unwrap();
          if source.contains("taffy::") {
            offenders.push(format!("{} names taffy", path.display()));
          }
        }
      }
    }
    assert!(
      scanned > 20,
      "scanned only {scanned} files; is the root right?"
    );
    assert!(offenders.is_empty(), "{offenders:#?}");
  }
}
