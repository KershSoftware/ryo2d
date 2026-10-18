//! The layout engine: Taffy flexbox and grid over the UI's nodes, kept
//! between frames by widget id. The only file that names Taffy.

use std::collections::HashSet;

use glam::Vec2;
use taffy::prelude::*;
use taffy::{Overflow, Point, TaffyResult, compute_leaf_layout};

use super::{Align, Flow, Justify, Length, Measure, Tracks, UiStyle};
use crate::core::hash::FastMap;
use crate::core::ui::{Rect, UiId};

/// How much room a measured node is offered along one axis
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Avail {
  /// This many units
  Definite(f32),
  /// As narrow as the content can be
  MinContent,
  /// As wide as the content wants
  MaxContent,
}

/// Sizes a text node: its index, the dimensions already known and the space offered
pub(crate) type MeasureFn<'a> =
  dyn FnMut(usize, (Option<f32>, Option<f32>), (Avail, Avail)) -> Vec2 + 'a;

/// One node to lay out: the tree is a slice of these, node 0 the root
pub(crate) struct NodeDesc<'s> {
  pub(crate) id: UiId,
  pub(crate) style: &'s UiStyle,
  pub(crate) measure: Measure,
  /// Changes when the measured content changes (a text's hash)
  pub(crate) content_key: u64,
  /// Indices into the slice
  pub(crate) children: &'s [usize],
}

/// A node's result: its rect relative to node 0's origin, and the extent of
/// its content (larger than the rect for a scroll node)
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Placed {
  pub(crate) rect: Rect,
  pub(crate) content: Vec2,
}

struct Entry {
  node: NodeId,
  style: UiStyle,
  parent_flow: Option<Flow>,
  measure: Measure,
  children: Vec<NodeId>,
  content_key: u64,
  frame: u64,
}

/// What one layout call works in, kept between calls: cleared and filled
/// again, never allocated anew once warm
#[derive(Default)]
struct Buffers {
  /// Each node that counts, by id: its index
  index_of: FastMap<UiId, usize>,
  parents: Vec<Option<usize>>,
  visited: Vec<bool>,
  /// Every node's valid children, run by run
  kids: Vec<usize>,
  /// Each node's run of `kids`
  kid_range: Vec<(usize, usize)>,
  /// The nodes that count, parents before children
  order: Vec<usize>,
  /// Each node's style made safe for layout
  clean: Vec<Option<UiStyle>>,
  stack: Vec<usize>,
  child_ids: Vec<NodeId>,
  /// The result
  placed: Vec<Placed>,
}

/// Flexbox and grid layout over UI nodes, cached by widget id
pub(crate) struct LayoutEngine {
  taffy: TaffyTree<UiId>,
  entries: FastMap<UiId, Entry>,
  frame: u64,
  warned: HashSet<&'static str>,
  buffers: Buffers,
}

impl LayoutEngine {
  pub(crate) fn new() -> LayoutEngine {
    let mut taffy = TaffyTree::new();
    // tree::resolve snaps rects to render pixels itself
    taffy.disable_rounding();
    LayoutEngine {
      taffy,
      entries: FastMap::default(),
      frame: 0,
      warned: HashSet::new(),
      buffers: Buffers::default(),
    }
  }

  /// Starts a frame: nodes laid out from here on are kept by `end_frame`
  pub(crate) fn begin_frame(&mut self) {
    self.frame += 1;
  }

  /// Removes the nodes that were not laid out since `begin_frame`
  pub(crate) fn end_frame(&mut self) {
    let frame = self.frame;
    let taffy = &mut self.taffy;
    self.entries.retain(|_, entry| {
      if entry.frame == frame {
        return true;
      }
      // A node that is already gone has nothing left to remove
      let _ = taffy.remove(entry.node);
      false
    });
  }

  #[cfg(test)]
  pub(crate) fn node_count(&self) -> usize {
    self.entries.len()
  }

  /// Lays out `nodes` (node 0 is the subtree's root) in `available` UI units.
  /// `measure` sizes `Measure::Text` nodes: it gets the node's index, the
  /// dimensions already known and the space offered. One result per node,
  /// valid until the next call.
  pub(crate) fn layout(
    &mut self,
    nodes: &[NodeDesc<'_>],
    available: Vec2,
    measure: &mut MeasureFn<'_>,
  ) -> &[Placed] {
    let mut buffers = std::mem::take(&mut self.buffers);
    buffers.placed.clear();
    buffers.placed.resize(nodes.len(), Placed::default());
    if !nodes.is_empty()
      && let Err(e) = self.try_layout(nodes, available, measure, &mut buffers)
    {
      if self.warned.insert("layout failed") {
        crate::engine_warn!(module: "ui", "layout failed: {e}");
      }
      // Start over rather than trust a tree that failed half way
      self.taffy.clear();
      self.entries.clear();
      buffers.placed.fill(Placed::default());
    }
    self.buffers = buffers;
    &self.buffers.placed
  }

  fn try_layout(
    &mut self,
    nodes: &[NodeDesc<'_>],
    available: Vec2,
    measure: &mut MeasureFn<'_>,
    buffers: &mut Buffers,
  ) -> TaffyResult<()> {
    let Buffers {
      index_of,
      parents,
      visited,
      kids,
      kid_range,
      order,
      clean,
      stack,
      child_ids,
      placed,
    } = buffers;
    // Pre-order walk: which nodes count (reachable, unique ids), their parents
    // and their valid children
    let count = nodes.len();
    index_of.clear();
    index_of.reserve(count);
    parents.clear();
    parents.resize(count, None);
    visited.clear();
    visited.resize(count, false);
    kids.clear();
    kid_range.clear();
    kid_range.resize(count, (0, 0));
    order.clear();
    clean.clear();
    clean.resize(count, None);
    stack.clear();
    stack.push(0);
    visited[0] = true;
    index_of.insert(nodes[0].id, 0);
    while let Some(i) = stack.pop() {
      order.push(i);
      clean[i] = Some(self.sanitize(nodes[i].style));
      let start = kids.len();
      for &c in nodes[i].children {
        if c >= nodes.len() || visited[c] || index_of.contains_key(&nodes[c].id) {
          continue;
        }
        visited[c] = true;
        index_of.insert(nodes[c].id, c);
        parents[c] = Some(i);
        kids.push(c);
      }
      kid_range[i] = (start, kids.len());
      stack.extend(kids[start..].iter().rev());
    }

    // Children before parents
    for &i in order.iter().rev() {
      let desc = &nodes[i];
      let Some(style) = clean[i].as_ref() else {
        continue;
      };
      let parent_flow = parents[i].and_then(|p| clean[p].as_ref()).map(|s| &s.flow);
      let measure_kind = clean_measure(desc.measure);
      child_ids.clear();
      let (start, end) = kid_range[i];
      for &c in &kids[start..end] {
        if let Some(entry) = self.entries.get(&nodes[c].id) {
          child_ids.push(entry.node);
        }
      }

      match self.entries.get_mut(&desc.id) {
        Some(entry) => {
          entry.frame = self.frame;
          if entry.style != *style
            || entry.parent_flow.as_ref() != parent_flow
            || entry.measure != measure_kind
          {
            self
              .taffy
              .set_style(entry.node, to_taffy(style, parent_flow, &measure_kind))?;
            entry.style = style.clone();
            entry.parent_flow = parent_flow.cloned();
            entry.measure = measure_kind;
          }
          if entry.children != *child_ids {
            self.taffy.set_children(entry.node, child_ids)?;
            entry.children.clone_from(child_ids);
          }
          if entry.content_key != desc.content_key {
            self.taffy.mark_dirty(entry.node)?;
            entry.content_key = desc.content_key;
          }
        }
        None => {
          let node = self
            .taffy
            .new_leaf_with_context(to_taffy(style, parent_flow, &measure_kind), desc.id)?;
          if !child_ids.is_empty() {
            self.taffy.set_children(node, child_ids)?;
          }
          self.entries.insert(
            desc.id,
            Entry {
              node,
              style: style.clone(),
              parent_flow: parent_flow.cloned(),
              measure: measure_kind,
              children: child_ids.clone(),
              content_key: desc.content_key,
              frame: self.frame,
            },
          );
        }
      }
    }

    let Some(root) = self.entries.get(&nodes[0].id).map(|e| e.node) else {
      return Ok(());
    };
    let fit = |v: f32| if v.is_finite() && v > 0.0 { v } else { 0.0 };
    let space = Size {
      width: AvailableSpace::Definite(fit(available.x)),
      height: AvailableSpace::Definite(fit(available.y)),
    };
    self
      .taffy
      .compute_layout_with_measure(root, space, |inputs, _node, ctx, style| {
        compute_leaf_layout(
          inputs,
          style,
          |_, _| 0.0,
          |known, avail| {
            let Some(i) = ctx.and_then(|id| index_of.get(&*id).copied()) else {
              return Size::ZERO;
            };
            if nodes[i].measure != Measure::Text {
              return Size::ZERO;
            }
            let size = measure(
              i,
              (known.width, known.height),
              (to_avail(avail.width), to_avail(avail.height)),
            );
            Size {
              width: fit(size.x),
              height: fit(size.y),
            }
          },
        )
      })?;

    // Taffy's locations are relative to the parent: accumulate from the root
    let finite = |v: f32| if v.is_finite() { v } else { 0.0 };
    for &i in order.iter() {
      let Some(entry) = self.entries.get(&nodes[i].id) else {
        continue;
      };
      let layout = self.taffy.layout(entry.node)?;
      let (x, y) = match parents[i] {
        Some(p) => (
          placed[p].rect.x + layout.location.x,
          placed[p].rect.y + layout.location.y,
        ),
        None => (0.0, 0.0),
      };
      let rect = Rect::new(
        finite(x),
        finite(y),
        finite(layout.size.width),
        finite(layout.size.height),
      );
      let scrolls = clean[i].as_ref().is_some_and(|s| s.scroll_x || s.scroll_y);
      let content = if scrolls {
        let overflow = layout.scrollable_overflow_rect;
        Vec2::new(
          finite(overflow.right).max(rect.width),
          finite(overflow.bottom).max(rect.height),
        )
      } else {
        Vec2::new(rect.width, rect.height)
      };
      placed[i] = Placed { rect, content };
    }
    Ok(())
  }

  /// Replaces what layout cannot use, warning once per field
  fn sanitize(&mut self, style: &UiStyle) -> UiStyle {
    let mut s = style.clone();
    s.width = self.clean_length(s.width, "width");
    s.height = self.clean_length(s.height, "height");
    s.min_width = self.clean_opt(s.min_width, "min_width");
    s.max_width = self.clean_opt(s.max_width, "max_width");
    s.min_height = self.clean_opt(s.min_height, "min_height");
    s.max_height = self.clean_opt(s.max_height, "max_height");
    s.padding.left = self.clean_value(s.padding.left, "padding");
    s.padding.top = self.clean_value(s.padding.top, "padding");
    s.padding.right = self.clean_value(s.padding.right, "padding");
    s.padding.bottom = self.clean_value(s.padding.bottom, "padding");
    s.margin.left = self.clean_value(s.margin.left, "margin");
    s.margin.top = self.clean_value(s.margin.top, "margin");
    s.margin.right = self.clean_value(s.margin.right, "margin");
    s.margin.bottom = self.clean_value(s.margin.bottom, "margin");
    s.gap = self.clean_value(s.gap, "gap");
    s.grow = self.clean_value(s.grow, "grow");
    if s.aspect.is_some_and(|a| !a.is_finite() || a <= 0.0) {
      self.warn_field("aspect");
      s.aspect = None;
    }
    if let Flow::Grid(Tracks::Fixed(widths)) = &mut s.flow {
      for w in widths.iter_mut() {
        if !w.is_finite() || *w < 0.0 {
          if self.warned.insert("grid track") {
            warn_bad("grid track");
          }
          *w = 0.0;
        }
      }
    }
    if !s.offset.is_finite() {
      self.warn_field("offset");
      s.offset = Vec2::ZERO;
    }
    s
  }

  fn clean_value(&mut self, v: f32, field: &'static str) -> f32 {
    if v.is_finite() && v >= 0.0 {
      v
    } else {
      self.warn_field(field);
      0.0
    }
  }

  fn clean_opt(&mut self, v: Option<f32>, field: &'static str) -> Option<f32> {
    match v {
      Some(v) if !v.is_finite() || v < 0.0 => {
        self.warn_field(field);
        None
      }
      v => v,
    }
  }

  fn clean_length(&mut self, l: Length, field: &'static str) -> Length {
    match l {
      Length::Px(v) | Length::Pct(v) if !v.is_finite() || v < 0.0 => {
        self.warn_field(field);
        Length::Fit
      }
      l => l,
    }
  }

  fn warn_field(&mut self, field: &'static str) {
    if self.warned.insert(field) {
      warn_bad(field);
    }
  }
}

fn warn_bad(field: &str) {
  crate::engine_warn!(
    module: "ui",
    "a UI style has a non-finite or negative {field}; ignored"
  );
}

fn clean_measure(measure: Measure) -> Measure {
  match measure {
    Measure::Fixed(s) => Measure::Fixed(Vec2::new(
      if s.x.is_finite() { s.x.max(0.0) } else { 0.0 },
      if s.y.is_finite() { s.y.max(0.0) } else { 0.0 },
    )),
    m => m,
  }
}

fn to_avail(space: AvailableSpace) -> Avail {
  match space {
    AvailableSpace::Definite(v) => Avail::Definite(v),
    AvailableSpace::MinContent => Avail::MinContent,
    AvailableSpace::MaxContent => Avail::MaxContent,
  }
}

fn dimension(l: Length) -> Dimension {
  match l {
    Length::Px(v) => length(v),
    Length::Pct(p) => percent(p),
    Length::Fill | Length::Fit => auto(),
  }
}

fn align_items(a: Align) -> AlignItems {
  match a {
    Align::Start => AlignItems::START,
    Align::Center => AlignItems::CENTER,
    Align::End => AlignItems::END,
    Align::Stretch => AlignItems::STRETCH,
  }
}

fn justify_content(j: Justify) -> JustifyContent {
  match j {
    Justify::Start => JustifyContent::START,
    Justify::Center => JustifyContent::CENTER,
    Justify::End => JustifyContent::END,
    Justify::SpaceBetween => JustifyContent::SPACE_BETWEEN,
    Justify::SpaceAround => JustifyContent::SPACE_AROUND,
    Justify::SpaceEvenly => JustifyContent::SPACE_EVENLY,
  }
}

/// START, CENTER or END for where an anchor sits along one axis
fn anchor_align(fraction: f32) -> AlignSelf {
  if fraction <= 0.0 {
    AlignSelf::START
  } else if fraction >= 1.0 {
    AlignSelf::END
  } else {
    AlignSelf::CENTER
  }
}

/// Our style as Taffy's. `parent` is the flow of the node's parent (`None`
/// for the subtree root), which decides what `Fill` and an anchor mean
fn to_taffy(style: &UiStyle, parent: Option<&Flow>, measure: &Measure) -> taffy::Style {
  let mut t = taffy::Style::default();
  match &style.flow {
    Flow::Column | Flow::Row => {
      t.display = Display::Flex;
      t.flex_direction = if style.flow == Flow::Row {
        FlexDirection::Row
      } else {
        FlexDirection::Column
      };
    }
    Flow::Grid(tracks) => {
      t.display = Display::Grid;
      t.grid_template_columns = match tracks {
        Tracks::Equal(n) => vec![repeat((*n).max(1), vec![fr(1.0)])],
        Tracks::Fixed(widths) => widths.iter().map(|&w| length(w)).collect(),
      };
      t.justify_items = align_items(style.align);
    }
    Flow::Stack => {
      t.display = Display::Grid;
      t.grid_template_columns = vec![fr(1.0)];
      t.grid_template_rows = vec![fr(1.0)];
      t.justify_items = align_items(style.align);
    }
  }

  t.size = Size {
    width: dimension(style.width),
    height: dimension(style.height),
  };
  t.align_self = style.align_self.map(align_items);
  t.flex_grow = style.grow;
  if style.absolute {
    t.position = Position::Absolute;
  }
  // Only text gives way, so it can wrap
  t.flex_shrink = 0.0;

  if matches!(parent, Some(Flow::Stack)) {
    let fraction = style.anchor.fraction();
    t.grid_row = line(1);
    t.grid_column = line(1);
    t.justify_self = Some(anchor_align(fraction.x));
    t.align_self = Some(anchor_align(fraction.y));
  }

  for (is_width, len) in [(true, style.width), (false, style.height)] {
    if len != Length::Fill {
      continue;
    }
    match parent {
      None => {
        let full = percent(1.0);
        if is_width {
          t.size.width = full;
        } else {
          t.size.height = full;
        }
      }
      Some(Flow::Column | Flow::Row) => {
        let main_is_width = parent == Some(&Flow::Row);
        if is_width == main_is_width {
          t.flex_grow = t.flex_grow.max(1.0);
          t.flex_basis = length(0.0);
        } else {
          t.align_self = Some(AlignSelf::STRETCH);
        }
      }
      Some(Flow::Grid(_) | Flow::Stack) => {
        if is_width {
          t.justify_self = Some(AlignSelf::STRETCH);
        } else {
          t.align_self = Some(AlignSelf::STRETCH);
        }
      }
    }
  }

  match measure {
    Measure::Fixed(s) => {
      if style.width == Length::Fit {
        t.size.width = length(s.x);
      }
      if style.height == Length::Fit {
        t.size.height = length(s.y);
      }
      // Filling, it has no size of its own, so a parent sized to its
      // contents would make it zero: its fixed size is its floor, or the
      // caller's cap when that is smaller (a min above the max would beat it)
      let floor = |max: Option<f32>, size: f32| max.map_or(size, |max| max.min(size));
      if style.width == Length::Fill {
        t.min_size.width = length(floor(style.max_width, s.x));
      }
      if style.height == Length::Fill {
        t.min_size.height = length(floor(style.max_height, s.y));
      }
    }
    Measure::Text => {
      t.flex_shrink = 1.0;
      if style.min_width.is_none() {
        t.min_size.width = length(0.0);
      }
    }
    Measure::None => {}
  }

  t.padding = taffy::Rect {
    left: length(style.padding.left),
    right: length(style.padding.right),
    top: length(style.padding.top),
    bottom: length(style.padding.bottom),
  };
  t.margin = taffy::Rect {
    left: length(style.margin.left),
    right: length(style.margin.right),
    top: length(style.margin.top),
    bottom: length(style.margin.bottom),
  };
  t.gap = Size {
    width: length(style.gap),
    height: length(style.gap),
  };
  if let Some(v) = style.min_width {
    t.min_size.width = length(v);
  }
  if let Some(v) = style.min_height {
    t.min_size.height = length(v);
  }
  if let Some(v) = style.max_width {
    t.max_size.width = length(v);
  }
  if let Some(v) = style.max_height {
    t.max_size.height = length(v);
  }
  t.align_items = align_items(style.align);
  t.justify_content = justify_content(style.justify);
  if style.wrap {
    t.flex_wrap = FlexWrap::Wrap;
  }
  t.aspect_ratio = style.aspect;
  t.overflow = Point {
    x: if style.scroll_x {
      Overflow::Scroll
    } else {
      Overflow::Visible
    },
    y: if style.scroll_y {
      Overflow::Scroll
    } else {
      Overflow::Visible
    },
  };
  t.scrollbar_width = 0.0;
  t
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ui::layout::{Align, Edges, Flow, Length, Measure, Tracks, UiStyle};
  use crate::core::ui::{Anchor, Rect, UiId};
  use glam::Vec2;

  /// The spike's text: `chars` 7-unit glyphs, 17 units a line, wrapped to the width offered
  #[allow(clippy::type_complexity)]
  fn measure_text(
    chars: f32,
  ) -> impl FnMut(usize, (Option<f32>, Option<f32>), (Avail, Avail)) -> Vec2 {
    move |_, known, avail| {
      let full = chars * 7.0;
      let width = match (known.0, avail.0) {
        (Some(w), _) => w,
        (None, Avail::Definite(w)) => w.min(full),
        (None, Avail::MinContent) => 7.0,
        (None, Avail::MaxContent) => full,
      };
      Vec2::new(width, (full / width.max(1.0)).ceil().max(1.0) * 17.0)
    }
  }

  fn node<'s>(
    id: u64,
    style: &'s UiStyle,
    measure: Measure,
    children: &'s [usize],
  ) -> NodeDesc<'s> {
    NodeDesc {
      id: UiId(id),
      style,
      measure,
      content_key: 0,
      children,
    }
  }

  #[test]
  fn a_column_stacks_children_with_gap_and_padding() {
    let column = UiStyle {
      padding: Edges::all(10.0),
      gap: 8.0,
      width: Length::Px(200.0),
      ..UiStyle::default()
    };
    let leaf = UiStyle::default();
    let nodes = [
      node(1, &column, Measure::None, &[1, 2]),
      node(2, &leaf, Measure::Text, &[]),
      node(3, &leaf, Measure::Fixed(Vec2::new(60.0, 20.0)), &[]),
    ];
    let mut engine = LayoutEngine::new();
    engine.begin_frame();
    let placed = engine.layout(&nodes, Vec2::new(800.0, 600.0), &mut measure_text(100.0));
    assert_eq!(placed[0].rect, Rect::new(0.0, 0.0, 200.0, 116.0));
    assert_eq!(
      placed[1].rect,
      Rect::new(10.0, 10.0, 180.0, 68.0),
      "4 lines of 17 at 180 wide"
    );
    assert_eq!(
      placed[2].rect,
      Rect::new(10.0, 86.0, 60.0, 20.0),
      "a fixed leaf is not stretched"
    );
  }

  #[test]
  fn fill_takes_what_a_row_has_left() {
    let row = UiStyle {
      flow: Flow::Row,
      width: Length::Px(300.0),
      ..UiStyle::default()
    };
    let fill = UiStyle {
      width: Length::Fill,
      ..UiStyle::default()
    };
    let leaf = UiStyle::default();
    let nodes = [
      node(1, &row, Measure::None, &[1, 2, 3]),
      node(2, &leaf, Measure::Fixed(Vec2::new(50.0, 10.0)), &[]),
      node(3, &leaf, Measure::Fixed(Vec2::new(30.0, 10.0)), &[]),
      node(4, &fill, Measure::Fixed(Vec2::new(0.0, 10.0)), &[]),
    ];
    let mut engine = LayoutEngine::new();
    engine.begin_frame();
    let placed = engine.layout(&nodes, Vec2::new(800.0, 600.0), &mut |_, _, _| Vec2::ZERO);
    assert_eq!(placed[3].rect.x, 80.0);
    assert_eq!(placed[3].rect.width, 220.0);
  }

  #[test]
  fn a_six_column_grid_of_48_unit_cells() {
    let grid = UiStyle {
      flow: Flow::Grid(Tracks::Fixed(vec![48.0; 6])),
      gap: 4.0,
      ..UiStyle::default()
    };
    let cell = UiStyle::default();
    let children: Vec<usize> = (1..=14).collect();
    let mut nodes = vec![node(1, &grid, Measure::None, &children)];
    for i in 0..14 {
      nodes.push(node(
        10 + i,
        &cell,
        Measure::Fixed(Vec2::new(48.0, 48.0)),
        &[],
      ));
    }
    let mut engine = LayoutEngine::new();
    engine.begin_frame();
    let placed = engine.layout(&nodes, Vec2::new(800.0, 600.0), &mut |_, _, _| Vec2::ZERO);
    assert_eq!(
      (placed[0].rect.width, placed[0].rect.height),
      (308.0, 152.0)
    );
    assert_eq!(
      (placed[7].rect.x, placed[7].rect.y),
      (0.0, 52.0),
      "cell 6 starts the second row"
    );
    assert_eq!((placed[14].rect.x, placed[14].rect.y), (52.0, 104.0));
  }

  #[test]
  fn a_stack_overlaps_its_children_by_anchor() {
    let stack = UiStyle {
      flow: Flow::Stack,
      width: Length::Px(100.0),
      height: Length::Px(50.0),
      ..UiStyle::default()
    };
    let badge = UiStyle {
      anchor: Anchor::TopRight,
      ..UiStyle::default()
    };
    let icon = UiStyle::default();
    let nodes = [
      node(1, &stack, Measure::None, &[1, 2]),
      node(2, &icon, Measure::Fixed(Vec2::new(100.0, 50.0)), &[]),
      node(3, &badge, Measure::Fixed(Vec2::new(20.0, 20.0)), &[]),
    ];
    let mut engine = LayoutEngine::new();
    engine.begin_frame();
    let placed = engine.layout(&nodes, Vec2::new(800.0, 600.0), &mut |_, _, _| Vec2::ZERO);
    assert_eq!(placed[1].rect, Rect::new(0.0, 0.0, 100.0, 50.0));
    assert_eq!(placed[2].rect, Rect::new(80.0, 0.0, 20.0, 20.0));
  }

  #[test]
  fn a_scroll_reports_its_content() {
    let scroll = UiStyle {
      width: Length::Px(200.0),
      height: Length::Px(150.0),
      gap: 4.0,
      scroll_y: true,
      ..UiStyle::default()
    };
    let row = UiStyle::default();
    let children: Vec<usize> = (1..=10).collect();
    let mut nodes = vec![node(1, &scroll, Measure::None, &children)];
    for i in 0..10 {
      nodes.push(node(
        10 + i,
        &row,
        Measure::Fixed(Vec2::new(180.0, 40.0)),
        &[],
      ));
    }
    let mut engine = LayoutEngine::new();
    engine.begin_frame();
    let placed = engine.layout(&nodes, Vec2::new(800.0, 600.0), &mut |_, _, _| Vec2::ZERO);
    assert_eq!(placed[0].rect.height, 150.0);
    assert_eq!(placed[0].content.y, 436.0, "10 rows of 40 and 9 gaps of 4");
    assert_eq!(
      placed[10].rect.height, 40.0,
      "scroll content does not shrink"
    );
  }

  #[test]
  fn a_second_layout_with_nothing_changed_measures_nothing() {
    let column = UiStyle {
      width: Length::Px(120.0),
      ..UiStyle::default()
    };
    let leaf = UiStyle::default();
    let nodes = [
      node(1, &column, Measure::None, &[1]),
      node(2, &leaf, Measure::Text, &[]),
    ];
    let mut engine = LayoutEngine::new();
    let calls = std::cell::Cell::new(0);
    let mut text = measure_text(40.0);
    let mut counting = |i, k, a| {
      calls.set(calls.get() + 1);
      text(i, k, a)
    };
    engine.begin_frame();
    let first = engine
      .layout(&nodes, Vec2::new(800.0, 600.0), &mut counting)
      .to_vec();
    let after_first = calls.get();
    engine.end_frame();
    engine.begin_frame();
    let second = engine.layout(&nodes, Vec2::new(800.0, 600.0), &mut counting);
    assert!(after_first > 0);
    assert_eq!(calls.get(), after_first, "cached");
    assert_eq!(first[1].rect, second[1].rect);
    assert!(
      second[1].rect.height > 17.0,
      "wrapped to the 120-wide column"
    );
  }

  #[test]
  fn nodes_not_laid_out_are_removed_at_frame_end() {
    let leaf = UiStyle::default();
    let mut engine = LayoutEngine::new();
    engine.begin_frame();
    engine.layout(
      &[node(1, &leaf, Measure::Fixed(Vec2::ONE), &[])],
      Vec2::splat(10.0),
      &mut |_, _, _| Vec2::ZERO,
    );
    engine.end_frame();
    assert_eq!(engine.node_count(), 1);
    engine.begin_frame();
    engine.end_frame();
    assert_eq!(engine.node_count(), 0);
  }

  #[test]
  fn degenerate_styles_lay_out_finite_rects() {
    let bad = UiStyle {
      width: Length::Px(f32::NAN),
      height: Length::Pct(-1.0),
      gap: -5.0,
      padding: Edges::all(f32::INFINITY),
      min_width: Some(f32::NAN),
      aspect: Some(0.0),
      grow: f32::NEG_INFINITY,
      ..UiStyle::default()
    };
    let leaf = UiStyle::default();
    let nodes = [
      node(1, &bad, Measure::None, &[1]),
      node(2, &leaf, Measure::Fixed(Vec2::new(5.0, 5.0)), &[]),
    ];
    let mut engine = LayoutEngine::new();
    for available in [
      Vec2::new(100.0, 100.0),
      Vec2::ZERO,
      Vec2::new(f32::NAN, -3.0),
    ] {
      engine.begin_frame();
      let placed = engine.layout(&nodes, available, &mut |_, _, _| Vec2::ZERO);
      for p in placed {
        assert!(
          p.rect.x.is_finite()
            && p.rect.y.is_finite()
            && p.rect.width.is_finite()
            && p.rect.height.is_finite(),
          "{p:?}"
        );
      }
      engine.end_frame();
    }
    let warnings = crate::core::dev_log::get_entries()
      .iter()
      .filter(|e| e.message.contains("non-finite or negative"))
      .count();
    assert!(
      (1..=7).contains(&warnings),
      "each bad field warned once, got {warnings}"
    );
  }

  #[test]
  fn deep_nesting_lays_out() {
    let column = UiStyle::default();
    let children: Vec<Vec<usize>> = (0..64)
      .map(|i| if i < 63 { vec![i + 1] } else { vec![] })
      .collect();
    let nodes: Vec<NodeDesc<'_>> = (0..64)
      .map(|i| {
        node(
          100 + i as u64,
          &column,
          if i == 63 {
            Measure::Fixed(Vec2::new(10.0, 10.0))
          } else {
            Measure::None
          },
          &children[i],
        )
      })
      .collect();
    let mut engine = LayoutEngine::new();
    engine.begin_frame();
    let placed = engine.layout(&nodes, Vec2::new(800.0, 600.0), &mut |_, _, _| Vec2::ZERO);
    assert_eq!(placed[63].rect, Rect::new(0.0, 0.0, 10.0, 10.0));
    let _ = Align::Stretch;
  }

  #[test]
  fn equal_columns_share_the_width() {
    let grid = UiStyle {
      flow: Flow::Grid(Tracks::Equal(3)),
      width: Length::Px(300.0),
      ..UiStyle::default()
    };
    let cell = UiStyle::default();
    let nodes = [
      node(1, &grid, Measure::None, &[1, 2, 3, 4]),
      node(2, &cell, Measure::Fixed(Vec2::new(0.0, 10.0)), &[]),
      node(3, &cell, Measure::Fixed(Vec2::new(0.0, 10.0)), &[]),
      node(4, &cell, Measure::Fixed(Vec2::new(0.0, 10.0)), &[]),
      node(5, &cell, Measure::Fixed(Vec2::new(0.0, 10.0)), &[]),
    ];
    let mut engine = LayoutEngine::new();
    engine.begin_frame();
    let placed = engine.layout(&nodes, Vec2::new(800.0, 600.0), &mut |_, _, _| Vec2::ZERO);
    assert_eq!(placed[3].rect.x, 200.0);
    assert_eq!(placed[4].rect.x, 0.0);
    assert_eq!(placed[4].rect.y, 10.0);
  }

  fn frame(
    engine: &mut LayoutEngine,
    nodes: &[NodeDesc<'_>],
    measure: &mut MeasureFn<'_>,
  ) -> Vec<Placed> {
    engine.begin_frame();
    let placed = engine
      .layout(nodes, Vec2::new(800.0, 600.0), measure)
      .to_vec();
    engine.end_frame();
    placed
  }

  #[test]
  fn a_changed_width_is_laid_out_next_frame() {
    let leaf = UiStyle {
      height: Length::Px(10.0),
      ..UiStyle::default()
    };
    let mut engine = LayoutEngine::new();
    let mut none = |_, _, _| Vec2::ZERO;
    for width in [100.0, 150.0] {
      let column = UiStyle {
        width: Length::Px(width),
        ..UiStyle::default()
      };
      let nodes = [
        node(1, &column, Measure::None, &[1]),
        node(2, &leaf, Measure::None, &[]),
      ];
      let placed = frame(&mut engine, &nodes, &mut none);
      assert_eq!(placed[1].rect.width, width);
    }
  }

  #[test]
  fn a_changed_content_key_measures_again() {
    let column = UiStyle {
      width: Length::Px(120.0),
      ..UiStyle::default()
    };
    let leaf = UiStyle::default();
    let calls = std::cell::Cell::new(0);
    let mut text = measure_text(40.0);
    let mut counting = |i, k, a| {
      calls.set(calls.get() + 1);
      text(i, k, a)
    };
    let mut engine = LayoutEngine::new();
    let desc = |key: u64| {
      [
        node(1, &column, Measure::None, &[1]),
        NodeDesc {
          id: UiId(2),
          style: &leaf,
          measure: Measure::Text,
          content_key: key,
          children: &[],
        },
      ]
    };
    frame(&mut engine, &desc(1), &mut counting);
    let first = calls.get();
    frame(&mut engine, &desc(1), &mut counting);
    assert_eq!(calls.get(), first, "same key: cached");
    frame(&mut engine, &desc(2), &mut counting);
    assert!(calls.get() > first, "new key: measured again");
  }

  #[test]
  fn a_changed_child_list_is_laid_out() {
    let column = UiStyle::default();
    let leaf = UiStyle::default();
    let mut engine = LayoutEngine::new();
    let mut none = |_, _, _| Vec2::ZERO;
    let fixed = Measure::Fixed(Vec2::new(10.0, 10.0));
    let one = [
      node(1, &column, Measure::None, &[1]),
      node(2, &leaf, fixed, &[]),
    ];
    let placed = frame(&mut engine, &one, &mut none);
    assert_eq!(placed[0].rect.height, 10.0);
    let two = [
      node(1, &column, Measure::None, &[1, 2]),
      node(2, &leaf, fixed, &[]),
      node(3, &leaf, fixed, &[]),
    ];
    let placed = frame(&mut engine, &two, &mut none);
    assert_eq!(placed[0].rect.height, 20.0);
    assert_eq!(placed[2].rect.y, 10.0);
    let swapped = [
      node(1, &column, Measure::None, &[2]),
      node(2, &leaf, fixed, &[]),
      node(3, &leaf, Measure::Fixed(Vec2::new(10.0, 30.0)), &[]),
    ];
    let placed = frame(&mut engine, &swapped, &mut none);
    assert_eq!(placed[0].rect.height, 30.0);
  }

  #[test]
  fn a_changed_parent_flow_moves_the_children() {
    let leaf = UiStyle::default();
    let fill = UiStyle {
      width: Length::Fill,
      ..UiStyle::default()
    };
    let mut engine = LayoutEngine::new();
    let mut none = |_, _, _| Vec2::ZERO;
    let fixed = Measure::Fixed(Vec2::new(50.0, 10.0));
    let tall = Measure::Fixed(Vec2::new(0.0, 10.0));
    let mut results = Vec::new();
    for flow in [Flow::Column, Flow::Row] {
      let parent = UiStyle {
        flow,
        width: Length::Px(300.0),
        ..UiStyle::default()
      };
      let nodes = [
        node(1, &parent, Measure::None, &[1, 2]),
        node(2, &leaf, fixed, &[]),
        node(3, &fill, tall, &[]),
      ];
      results.push(frame(&mut engine, &nodes, &mut none));
    }
    assert_eq!(
      (results[0][2].rect.x, results[0][2].rect.width),
      (0.0, 300.0),
      "stretched in a column"
    );
    assert_eq!(
      (results[1][2].rect.x, results[1][2].rect.width),
      (50.0, 250.0),
      "fills the rest of a row"
    );
  }
}
