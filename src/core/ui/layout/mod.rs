//! UI layout: the style vocabulary (lengths, edges, alignment) and the
//! engine that lays containers out with flexbox and grid. Only `engine.rs`
//! names the layout crate.

use glam::Vec2;

use crate::core::ecs::Anchor;

pub(crate) mod engine;

/// A size along one axis
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Length {
  /// A fixed size in UI units
  Px(f32),
  /// A fraction of the parent, 0.0 to 1.0
  Pct(f32),
  /// Take what the parent has left (the rest of the main axis, all of the cross axis)
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
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
