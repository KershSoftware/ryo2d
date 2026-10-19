//! The subtree being built: the nodes a top-level container (or a lone leaf)
//! opens, the draws placed on them, and their resolution into commands and
//! records once the layout engine has placed them

use std::cell::RefCell;
use std::sync::Arc;

use glam::Vec2;

use super::immediate::{UiCommand, UiCommands};
use super::layout::engine::{Avail, LayoutEngine, NodeDesc};
use super::layout::{Edges, Flow, Length, Measure, UiStyle};
use super::memory::{Flags, Record, Scratch, UiMemory, recycle};
use super::widgets::scroll::ScrollState;
use super::{Color, Rect, UiId};
use crate::core::asset::SpriteHandle;
use crate::core::render::UiSpace;
use crate::core::render::batch::Filter;
use crate::core::render::ui::snap_rect;
use crate::core::text::{RichText, TextAlign, TextEngine, TextKey, TextLayoutOptions, TextStyle};

/// Added to every wrap width a text node is measured and drawn at, so float
/// rounding in layout never wraps a text at its own width
const WRAP_SLACK: f32 = 0.01;

/// The nodes and draws of one top-level container (or lone leaf) being
/// recorded; node 0 is its root
pub(crate) struct Subtree {
  pub(crate) nodes: Vec<Node>,
  pub(crate) draws: Vec<Draw>,
  /// The text leaves of the nodes built into it (`Leaf::Owned` indexes
  /// them), apart from the nodes so a node stays small to push and clone
  pub(crate) leaves: Vec<TextLeaf>,
  /// The rect the root was placed at, if any
  pub(crate) at: Option<Rect>,
  /// The clip in force when the subtree started
  pub(crate) clip: Option<Rect>,
  /// The focus scope the subtree started in (`NONE`): its root's scope. A
  /// popup's is the scope it was opened in.
  pub(crate) scope: UiId,
  /// Placed below this rect once its size is known (a popup's anchor),
  /// unless it has an `at` rect
  pub(crate) below: Option<Rect>,
}

impl Subtree {
  /// An empty subtree started under `clip`, in focus scope `scope`
  pub(crate) fn new(at: Option<Rect>, clip: Option<Rect>, scope: UiId) -> Subtree {
    Subtree {
      nodes: Vec::new(),
      draws: Vec::new(),
      leaves: Vec::new(),
      at,
      clip,
      scope,
      below: None,
    }
  }
}

/// One container or leaf in a subtree
#[derive(Debug, Clone)]
pub(crate) struct Node {
  pub(crate) id: UiId,
  pub(crate) parent: Option<usize>,
  pub(crate) style: UiStyle,
  pub(crate) measure: Measure,
  /// The text a `Measure::Text` node is measured by
  pub(crate) text: Option<Leaf>,
  /// Changes when what is measured changes
  pub(crate) content_key: u64,
  /// Its style's [`UiStyle::shape_hash`] when known (0: not): a tape keeps
  /// it, so a replayed node is not hashed again. A style changed after the
  /// node is pushed sets it back to 0.
  pub(crate) shape: u64,
  /// What its record says; `None` writes no record
  pub(crate) record: Option<RecordSpec>,
  pub(crate) layer: u8,
  pub(crate) order: u32,
  pub(crate) depth: u16,
  /// Nearest focus-scope ancestor (`NONE`)
  pub(crate) scope: UiId,
  /// Nearest scrollable ancestor (`NONE`)
  pub(crate) scroll: UiId,
  /// Its children are clipped to its rect (scroll nodes)
  pub(crate) clips: bool,
  /// Placed at this rect (UI units) out of its parent's flow
  pub(crate) at: Option<Rect>,
}

impl Node {
  /// A node of `style` sized by `measure`; its place in the tree (parent,
  /// layer, order, depth, scopes) is filled in when it is pushed
  pub(crate) fn new(id: UiId, style: UiStyle, measure: Measure) -> Node {
    Node {
      id,
      parent: None,
      style,
      measure,
      text: None,
      content_key: 0,
      shape: 0,
      record: None,
      layer: 0,
      order: 0,
      depth: 0,
      scope: UiId::NONE,
      scroll: UiId::NONE,
      clips: false,
      at: None,
    }
  }
}

/// What a node's record carries besides its placement
#[derive(Debug, Clone, Copy)]
pub(crate) struct RecordSpec {
  pub(crate) flags: Flags,
  /// Explicit Up, Down, Left, Right neighbours (`NONE`)
  pub(crate) nav: [UiId; 4],
  /// A drop target: the payload type it takes
  pub(crate) accepts: Option<std::any::TypeId>,
}

impl RecordSpec {
  /// A record of `flags` with no neighbours and no drops
  pub(crate) fn flags(flags: Flags) -> RecordSpec {
    RecordSpec {
      flags,
      nav: [UiId::NONE; 4],
      accepts: None,
    }
  }
}

/// A text to measure and draw
#[derive(Debug, Clone)]
pub(crate) struct TextLeaf {
  pub(crate) text: RichText,
  pub(crate) style: TextStyle,
  /// Its wrap, when set, is the widest it is laid out
  pub(crate) options: TextLayoutOptions,
  /// The text, style and options hashed once, for every engine call on the
  /// leaf: made again whenever they change
  pub(crate) key: TextKey,
}

impl TextLeaf {
  /// A leaf of `text` in `style`, keyed
  pub(crate) fn new(text: RichText, style: TextStyle, options: TextLayoutOptions) -> TextLeaf {
    let key = TextKey::of(&text, &style, &options);
    TextLeaf {
      text,
      style,
      options,
      key,
    }
  }
}

/// A node's text: moved into its subtree by a live build, which allocates
/// nothing, and shared when recorded
#[derive(Debug, Clone)]
pub(crate) enum Leaf {
  /// Built this frame: its subtree's `leaves[k]`
  Owned(usize),
  /// Recorded: cloning shares the one copy
  Shared(Arc<TextLeaf>),
}

impl Leaf {
  /// The text, from `leaves` (its subtree's) when it is owned
  pub(crate) fn get<'a>(&'a self, leaves: &'a [TextLeaf]) -> Option<&'a TextLeaf> {
    match self {
      Leaf::Owned(k) => leaves.get(*k),
      Leaf::Shared(leaf) => Some(leaf),
    }
  }
}

/// A draw on a node, placed when the subtree is laid out
#[derive(Clone)]
pub(crate) struct Draw {
  /// Taken when it was recorded, so it keeps its place in the painter's order
  pub(crate) order: u32,
  pub(crate) layer: u8,
  pub(crate) node: usize,
  pub(crate) kind: DrawKind,
}

/// What a draw is, relative to its node
#[derive(Debug, Clone)]
pub(crate) enum DrawKind {
  /// The node's rect shrunk by `inset`
  Fill {
    inset: Edges,
    fill: Color,
    radius: f32,
    border: Option<(f32, Color)>,
  },
  /// The left `to` (0 to 1) of the inset rect: a bar's filled part, full
  /// height, or a `band` that thick centred vertically
  Fraction {
    inset: Edges,
    to: f32,
    band: Option<f32>,
    fill: Color,
    radius: f32,
  },
  /// A box of `size` at `frac` of the room left in the inset rect
  Box {
    inset: Edges,
    frac: Vec2,
    size: Vec2,
    fill: Color,
    radius: f32,
    border: Option<(f32, Color)>,
  },
  /// Text at `frac` of the room left in the node's content box, wrapped to
  /// its width when `wrap`; its effect seed is its key's
  Text {
    /// Shared: a text is far larger than the other draws, and a replayed
    /// draw must not copy it
    leaf: Arc<TextLeaf>,
    frac: Vec2,
    wrap: bool,
    opacity: f32,
    reveal: Option<(f32, f32)>,
  },
  /// The text its node is measured by (`Node::text`), drawn as `Text` is
  NodeText {
    frac: Vec2,
    wrap: bool,
    opacity: f32,
    reveal: Option<(f32, f32)>,
  },
  /// A sprite over the inset rect, or, with a `size`, a sprite that size at
  /// `frac` of the room left in the inset rect
  Sprite {
    inset: Edges,
    frac: Vec2,
    size: Option<Vec2>,
    sprite: SpriteHandle,
    uv: [f32; 4],
    tint: Color,
    filter: Filter,
  },
  /// A sprite stretched over the inset rect with its corners kept: the
  /// corners reach `insets` (left, top, right, bottom, in sprite pixels and
  /// drawn at that size in UI units), the edges and the middle stretch
  NineSlice {
    inset: Edges,
    sprite: SpriteHandle,
    insets: [f32; 4],
    tint: Color,
    filter: Filter,
  },
  /// Geometry in node-local units, translated after layout
  Local(UiCommand),
}

/// What resolving a subtree reads and writes
pub(crate) struct ResolveCx<'r, 'a> {
  pub(crate) layout: &'r mut LayoutEngine,
  pub(crate) text: &'r RefCell<&'a mut TextEngine>,
  pub(crate) memory: &'r mut UiMemory,
  /// The buffers it works in, kept between subtrees
  pub(crate) scratch: &'r mut Scratch,
  pub(crate) commands: &'r mut UiCommands,
  pub(crate) space: UiSpace,
  /// The scope id of the scene the subtree belongs to
  pub(crate) scene: u64,
}

/// Where a text's block sits across its content box
pub(crate) fn align_fraction(align: TextAlign) -> f32 {
  match align {
    TextAlign::Center => 0.5,
    TextAlign::End => 1.0,
    _ => 0.0,
  }
}

/// Lay `subtree` out, then write its records into memory and its draws into
/// the commands, each with the order it was recorded in. Its draws are
/// taken; its nodes stay for the caller. The root's rect (absolute, snapped
/// to render pixels), or `None` when it has no nodes.
pub(crate) fn resolve(subtree: &mut Subtree, cx: ResolveCx<'_, '_>) -> Option<Rect> {
  let (at, clip, below) = (subtree.at, subtree.clip, subtree.below);
  let nodes: &[Node] = &subtree.nodes;
  let leaves: &[TextLeaf] = &subtree.leaves;
  if nodes.is_empty() {
    return None;
  }
  let scratch = cx.scratch;
  let ui_size = cx.space.size;
  let scale = cx.space.px_scale;
  let count = nodes.len();

  // Lay out: an `at` rect is the room offered and the root's size
  let root_style = at.map(|at| UiStyle {
    width: Length::Px(at.width),
    height: Length::Px(at.height),
    ..nodes[0].style.clone()
  });
  // A nested node with an `at` rect leaves its parent's flow, at that size
  scratch.floating.clear();
  for (i, node) in nodes.iter().enumerate() {
    if let Some(at) = node.at.filter(|_| node.parent.is_some()) {
      let style = UiStyle {
        width: Length::Px(at.width),
        height: Length::Px(at.height),
        absolute: true,
        ..node.style.clone()
      };
      scratch.floating.push((i, style));
    }
  }
  child_runs(nodes, &mut scratch.children, &mut scratch.child_ends);
  let (children, ends) = (&scratch.children, &scratch.child_ends);
  let mut floating = scratch.floating.iter().peekable();
  let mut descs: Vec<NodeDesc<'_>> = recycle(std::mem::take(&mut scratch.descs));
  descs.extend(nodes.iter().enumerate().map(|(i, node)| {
    let start = if i == 0 { 0 } else { ends[i - 1] };
    let float = floating.next_if(|(k, _)| *k == i).map(|(_, style)| style);
    let (style, shape) = match (&root_style, float, i) {
      (Some(style), _, 0) => (style, 0),
      (_, Some(style), _) => (style, 0),
      _ => (&node.style, node.shape),
    };
    NodeDesc {
      id: node.id,
      style,
      shape,
      measure: node.measure,
      content_key: node.content_key,
      children: &children[start..ends[i]],
    }
  }));
  let available = at.map_or(ui_size, |at| Vec2::new(at.width, at.height));
  let text = cx.text;
  let mut measure = |i: usize, known: (Option<f32>, Option<f32>), avail: (Avail, Avail)| {
    nodes
      .get(i)
      .and_then(|node| node.text.as_ref()?.get(leaves))
      .map_or(Vec2::ZERO, |leaf| {
        measure_text(text, leaf, known.0, avail.0, scale)
      })
  };
  let placed = cx.layout.layout(&descs, available, &mut measure);
  scratch.descs = recycle(descs);

  // Place the root: at its rect, below its popup anchor, or by its anchor
  // and offset in the UI
  let root = placed[0].rect;
  let root_size = Vec2::new(root.width, root.height);
  let origin = match (at, below) {
    (Some(at), _) => Vec2::new(at.x, at.y),
    (None, Some(anchor)) => super::popup::below(anchor, root_size, ui_size),
    (None, None) => {
      nodes[0].style.anchor.fraction() * (ui_size - root_size) + finite(nodes[0].style.offset)
    }
  };

  // Absolute rects and clips; a parent always comes before its children
  let shift = &mut scratch.shift;
  let exact = &mut scratch.exact;
  let rects = &mut scratch.rects;
  let clips = &mut scratch.clips;
  shift.clear();
  shift.resize(count, Vec2::ZERO);
  exact.clear();
  rects.clear();
  clips.clear();
  for (i, node) in nodes.iter().enumerate() {
    let mut node_clip = clip;
    if let Some(p) = node.parent.filter(|&p| p < i) {
      shift[i] = shift[p];
      // A stack places its children by anchor, then moves them by their offset
      if matches!(nodes[p].style.flow, Flow::Stack) {
        shift[i] += finite(node.style.offset);
      }
      // A scroll area shows its contents moved up and left by its offset
      let (scroll_x, scroll_y) = (nodes[p].style.scroll_x, nodes[p].style.scroll_y);
      if (scroll_x || scroll_y)
        && let Some(state) = cx.memory.peek_state::<ScrollState>(nodes[p].id)
      {
        let by = Vec2::new(
          if scroll_x { state.offset.x } else { 0.0 },
          if scroll_y { state.offset.y } else { 0.0 },
        );
        shift[i] -= by;
      }
      node_clip = clips[p];
      if nodes[p].clips {
        node_clip = Some(match node_clip {
          Some(c) => intersect(c, rects[p]),
          None => rects[p],
        });
      }
    }
    let r = placed[i].rect;
    // A node placed at a rect goes there, wherever its parent is
    if let Some(at) = node.at.filter(|_| node.parent.is_some()) {
      shift[i] = Vec2::new(at.x, at.y) - origin - Vec2::new(r.x, r.y);
    }
    let at = origin + Vec2::new(r.x, r.y) + shift[i];
    let rect = Rect::new(at.x, at.y, r.width, r.height);
    exact.push(rect);
    rects.push(snap_rect(rect, scale));
    clips.push(node_clip);
  }

  // A scroll area keeps the size of its view and contents for next frame's
  // wheel, scrollbar and `scroll_to`; an axis it does not scroll has no more
  // contents than view
  for (i, node) in nodes.iter().enumerate() {
    let (x, y) = (node.style.scroll_x, node.style.scroll_y);
    if !(x || y) {
      continue;
    }
    if let Some(state) = cx.memory.state::<ScrollState>(node.id, cx.scene) {
      let view = Vec2::new(rects[i].width, rects[i].height);
      // The contents overflow by what they pass the unsnapped view: an area
      // snapped a fraction smaller than its layout still fits them
      let snapping = view - Vec2::new(exact[i].width, exact[i].height);
      let content = placed[i].content + snapping;
      // A height its owner gave stands, whatever the contents measure
      let height = match state.content_height {
        Some(h) => {
          let padding = clean_edges(node.style.padding);
          h + padding.top + padding.bottom + snapping.y
        }
        None => content.y.max(view.y),
      };
      state.view = view;
      state.content = Vec2::new(
        if x { content.x.max(view.x) } else { view.x },
        if y { height } else { view.y },
      );
    }
  }

  // Records, for next frame's hit tests and navigation
  for (i, node) in nodes.iter().enumerate() {
    let Some(spec) = node.record else {
      continue;
    };
    cx.memory.record(
      node.id,
      Record {
        rect: rects[i],
        clip: clips[i],
        layer: node.layer,
        order: node.order,
        depth: node.depth,
        scene: cx.scene,
        flags: spec.flags,
        scope: node.scope,
        scroll: node.scroll,
        content: placed[i].content,
        nav: spec.nav,
        accepts: spec.accepts,
      },
    );
  }

  for draw in subtree.draws.drain(..) {
    let Some(node) = nodes.get(draw.node) else {
      continue;
    };
    let place = Placement {
      rect: rects[draw.node],
      exact: exact[draw.node],
      padding: clean_edges(node.style.padding),
      clip: clips[draw.node],
      layer: draw.layer,
    };
    if let DrawKind::NineSlice {
      inset,
      sprite,
      insets,
      tint,
      filter,
    } = &draw.kind
    {
      let room = inset_rect(place.rect, *inset);
      let slice = NineSlice {
        sprite,
        insets: *insets,
        tint: *tint,
        filter: *filter,
      };
      nine_slice(room, &slice, place.clip, place.layer, scale, |command| {
        cx.commands.push(draw.order, command);
      });
    } else if let Some(command) = command_of(draw.kind, &place, node, leaves, text, scale) {
      cx.commands.push(draw.order, command);
    }
  }
  rects.first().copied()
}

/// Each node's children in `children`, in the order they were added (by
/// index, as each child names its parent): node `i`'s run ends at
/// `ends[i]` and starts where node `i - 1`'s ends
fn child_runs(nodes: &[Node], children: &mut Vec<usize>, ends: &mut Vec<usize>) {
  let count = nodes.len();
  let parent = |node: &Node| node.parent.filter(|&p| p < count);
  ends.clear();
  ends.resize(count, 0);
  for node in nodes {
    if let Some(p) = parent(node) {
      ends[p] += 1;
    }
  }
  // Each count becomes its run's start; placing a child moves its parent's
  // start on, so each ends as its run's end
  let mut start = 0;
  for end in ends.iter_mut() {
    let run = *end;
    *end = start;
    start += run;
  }
  children.clear();
  children.resize(start, 0);
  for (i, node) in nodes.iter().enumerate() {
    if let Some(p) = parent(node) {
      children[ends[p]] = i;
      ends[p] += 1;
    }
  }
}

/// A nine-slice sprite to place
#[derive(Clone, Copy)]
struct NineSlice<'s> {
  sprite: &'s SpriteHandle,
  insets: [f32; 4],
  tint: Color,
  filter: Filter,
}

/// The pieces of `sprite` stretched over `room` with corners of `insets`
/// (left, top, right, bottom): up to nine sprite commands under `clip` on
/// `layer`, each given to `emit`, edges snapped to render pixels so the
/// pieces meet without gaps. Corners shrink together when `room` is smaller
/// than they are.
fn nine_slice(
  room: Rect,
  slice: &NineSlice<'_>,
  clip: Option<Rect>,
  layer: u8,
  scale: f32,
  mut emit: impl FnMut(UiCommand),
) {
  let NineSlice {
    sprite,
    insets,
    tint,
    filter,
  } = *slice;
  let (width, height) = (sprite.width() as f32, sprite.height() as f32);
  if !(drawable(room) && width > 0.0 && height > 0.0) {
    return;
  }
  let clean = |v: f32| if v.is_finite() { v.max(0.0) } else { 0.0 };
  let [left, top, right, bottom] = insets.map(clean);
  // The two corners along an axis share the room if they do not fit it
  let fit = |a: f32, b: f32, room: f32| {
    let total = a + b;
    if total > room && total > 0.0 {
      (a * room / total, b * room / total)
    } else {
      (a, b)
    }
  };
  let (l, r) = fit(left, right, room.width);
  let (t, b) = fit(top, bottom, room.height);
  let snap = |v: f32| (v * scale).round() / scale;
  let xs = [room.x, room.x + l, room.right() - r, room.right()].map(snap);
  let ys = [room.y, room.y + t, room.bottom() - b, room.bottom()].map(snap);
  // The same cuts in the sprite, in fractions of it
  let us = [
    0.0,
    left.min(width) / width,
    1.0 - right.min(width) / width,
    1.0,
  ];
  let vs = [
    0.0,
    top.min(height) / height,
    1.0 - bottom.min(height) / height,
    1.0,
  ];
  for row in 0..3 {
    for column in 0..3 {
      let rect = Rect::new(
        xs[column],
        ys[row],
        xs[column + 1] - xs[column],
        ys[row + 1] - ys[row],
      );
      if drawable(rect) && !outside(clip, rect) {
        emit(UiCommand::Sprite {
          sprite: sprite.clone(),
          rect,
          uv: [us[column], vs[row], us[column + 1], vs[row + 1]],
          tint,
          filter,
          clip,
          layer,
        });
      }
    }
  }
}

/// The size of a text node offered `known` or `avail` width: wrapped to the
/// width it is given (and no wider than its own wrap), unwrapped for its
/// max-content width, and wrapped at its widest word for its min-content
/// width
fn measure_text(
  text: &RefCell<&mut TextEngine>,
  leaf: &TextLeaf,
  known: Option<f32>,
  avail: Avail,
  scale: f32,
) -> Vec2 {
  let Ok(mut engine) = text.try_borrow_mut() else {
    return Vec2::ZERO;
  };
  let offered = match (known, avail) {
    (Some(w), _) | (None, Avail::Definite(w)) => Some(w + WRAP_SLACK),
    (None, Avail::MinContent) => {
      Some(engine.min_content_width(&leaf.text, &leaf.style, scale) + WRAP_SLACK)
    }
    (None, Avail::MaxContent) => None,
  };
  let mut options = leaf.options;
  options.wrap = narrowest(offered, leaf.options.wrap);
  // Measured left-aligned: an aligned block is as wide as its wrap width,
  // which would claim all the room offered
  options.align = TextAlign::Start;
  let key = leaf.key.with_options(&options);
  engine.measure_keyed(key, &leaf.text, &leaf.style, &options, scale)
}

/// The narrower of two optional widths
fn narrowest(a: Option<f32>, b: Option<f32>) -> Option<f32> {
  match (a, b) {
    (Some(a), Some(b)) => Some(a.min(b)),
    (a, b) => a.or(b),
  }
}

/// Where a draw's node landed
struct Placement {
  /// Snapped to render pixels
  rect: Rect,
  /// Before snapping (text wraps at this width, so snapping never wraps it)
  exact: Rect,
  padding: Edges,
  clip: Option<Rect>,
  layer: u8,
}

/// How a text draw sits in its node's content box
#[derive(Clone, Copy)]
struct TextAt {
  frac: Vec2,
  wrap: bool,
  opacity: f32,
  reveal: Option<(f32, f32)>,
}

/// A draw on `node` (its text owned in `leaves`) as a command in UI units,
/// or `None` when nothing of it would show
fn command_of(
  kind: DrawKind,
  place: &Placement,
  node: &Node,
  leaves: &[TextLeaf],
  text: &RefCell<&mut TextEngine>,
  scale: f32,
) -> Option<UiCommand> {
  let (clip, layer) = (place.clip, place.layer);
  let visible = |rect: Rect| drawable(rect) && !outside(clip, rect);
  match kind {
    DrawKind::Fill {
      inset,
      fill,
      radius,
      border,
    } => {
      let rect = inset_rect(place.rect, inset);
      visible(rect).then_some(UiCommand::Rect {
        rect,
        fill,
        radius,
        border,
        clip,
        layer,
      })
    }
    DrawKind::Box {
      inset,
      frac,
      size,
      fill,
      radius,
      border,
    } => {
      let room = inset_rect(place.rect, inset);
      let at = Vec2::new(room.x, room.y) + frac * (Vec2::new(room.width, room.height) - size);
      let rect = snap_rect(Rect::new(at.x, at.y, size.x, size.y), scale);
      visible(rect).then_some(UiCommand::Rect {
        rect,
        fill,
        radius,
        border,
        clip,
        layer,
      })
    }
    DrawKind::Text {
      leaf,
      frac,
      wrap,
      opacity,
      reveal,
    } => {
      let at = TextAt {
        frac,
        wrap,
        opacity,
        reveal,
      };
      text_command(&leaf, at, place, text, scale)
    }
    DrawKind::NodeText {
      frac,
      wrap,
      opacity,
      reveal,
    } => {
      let at = TextAt {
        frac,
        wrap,
        opacity,
        reveal,
      };
      text_command(node.text.as_ref()?.get(leaves)?, at, place, text, scale)
    }
    DrawKind::Sprite {
      inset,
      frac,
      size,
      sprite,
      uv,
      tint,
      filter,
    } => {
      let room = inset_rect(place.rect, inset);
      let rect = match size {
        Some(size) => {
          let at = Vec2::new(room.x, room.y) + frac * (Vec2::new(room.width, room.height) - size);
          snap_rect(Rect::new(at.x, at.y, size.x, size.y), scale)
        }
        None => room,
      };
      visible(rect).then_some(UiCommand::Sprite {
        sprite,
        rect,
        uv,
        tint,
        filter,
        clip,
        layer,
      })
    }
    DrawKind::Fraction {
      inset,
      to,
      band,
      fill,
      radius,
    } => {
      let mut room = inset_rect(place.rect, inset);
      if let Some(band) = band.filter(|b| b.is_finite() && *b >= 0.0) {
        room.y += (room.height - band) / 2.0;
        room.height = band;
      }
      let to = if to.is_finite() {
        to.clamp(0.0, 1.0)
      } else {
        0.0
      };
      let rect = snap_rect(
        Rect::new(room.x, room.y, room.width * to, room.height),
        scale,
      );
      visible(rect).then_some(UiCommand::Rect {
        rect,
        fill,
        radius,
        border: None,
        clip,
        layer,
      })
    }
    // Placed as nine commands by the caller
    DrawKind::NineSlice { .. } => None,
    DrawKind::Local(command) => {
      let by = Vec2::new(place.rect.x, place.rect.y);
      match command {
        UiCommand::Rect {
          rect,
          fill,
          radius,
          border,
          ..
        } => {
          let rect = moved(rect, by);
          visible(rect).then_some(UiCommand::Rect {
            rect,
            fill,
            radius,
            border,
            clip,
            layer,
          })
        }
        UiCommand::Sprite {
          sprite,
          rect,
          uv,
          tint,
          filter,
          ..
        } => {
          let rect = moved(rect, by);
          visible(rect).then_some(UiCommand::Sprite {
            sprite,
            rect,
            uv,
            tint,
            filter,
            clip,
            layer,
          })
        }
        UiCommand::Line {
          points,
          width,
          color,
          closed,
          ..
        } => {
          let mut points = points;
          // Moved in place when the draw is the only holder
          match Arc::get_mut(&mut points) {
            Some(own) => own.iter_mut().for_each(|point| *point += by),
            None => points = points.iter().map(|point| *point + by).collect(),
          }
          Some(UiCommand::Line {
            points,
            width,
            color,
            closed,
            clip,
            layer,
          })
        }
        UiCommand::Text {
          layout,
          origin,
          reveal,
          opacity,
          seed,
          ..
        } => {
          let origin = origin + by;
          let size = layout.size / layout.scale;
          let block = Rect::new(origin.x, origin.y, size.x, size.y);
          (origin.is_finite() && !outside(clip, block)).then_some(UiCommand::Text {
            layout,
            origin,
            reveal,
            opacity,
            seed,
            clip,
            layer,
          })
        }
      }
    }
  }
}

/// `leaf` laid out in its node's content box, wrapped to its width when
/// `at.wrap`; its effect seed is its key's
fn text_command(
  leaf: &TextLeaf,
  at: TextAt,
  place: &Placement,
  text: &RefCell<&mut TextEngine>,
  scale: f32,
) -> Option<UiCommand> {
  let content = inset_rect(place.rect, place.padding);
  let mut options = leaf.options;
  let mut key = leaf.key;
  if at.wrap {
    let width = (place.exact.width - place.padding.width()).max(0.0) + WRAP_SLACK;
    options.wrap = narrowest(Some(width), leaf.options.wrap);
    key = key.with_options(&options);
  }
  let layout = {
    let mut engine = text.try_borrow_mut().ok()?;
    engine.layout_keyed(key, &leaf.text, &leaf.style, &options, scale)?
  };
  let size = layout.size / scale;
  let origin =
    Vec2::new(content.x, content.y) + at.frac * (Vec2::new(content.width, content.height) - size);
  let block = Rect::new(origin.x, origin.y, size.x, size.y);
  (origin.is_finite() && !outside(place.clip, block)).then_some(UiCommand::Text {
    layout,
    origin,
    reveal: at.reveal,
    opacity: at.opacity,
    seed: leaf.key.seed(),
    clip: place.clip,
    layer: place.layer,
  })
}

/// A vector with any non-finite part made zero
fn finite(v: Vec2) -> Vec2 {
  Vec2::new(
    if v.x.is_finite() { v.x } else { 0.0 },
    if v.y.is_finite() { v.y } else { 0.0 },
  )
}

/// `edges` as layout used them: a side that is not finite or is negative
/// is 0 (layout warned about it)
pub(crate) fn clean_edges(edges: Edges) -> Edges {
  let clean = |v: f32| if v.is_finite() && v >= 0.0 { v } else { 0.0 };
  Edges::new(
    clean(edges.left),
    clean(edges.top),
    clean(edges.right),
    clean(edges.bottom),
  )
}

/// `rect` shrunk by `edges` on each side
fn inset_rect(rect: Rect, edges: Edges) -> Rect {
  Rect::new(
    rect.x + edges.left,
    rect.y + edges.top,
    rect.width - edges.width(),
    rect.height - edges.height(),
  )
}

fn moved(rect: Rect, by: Vec2) -> Rect {
  Rect::new(rect.x + by.x, rect.y + by.y, rect.width, rect.height)
}

/// The overlap of two rects (empty when they do not meet)
fn intersect(a: Rect, b: Rect) -> Rect {
  let (x, y) = (a.x.max(b.x), a.y.max(b.y));
  let (right, bottom) = (a.right().min(b.right()), a.bottom().min(b.bottom()));
  Rect::new(x, y, (right - x).max(0.0), (bottom - y).max(0.0))
}

/// Whether `rect` lies wholly outside `clip`
fn outside(clip: Option<Rect>, rect: Rect) -> bool {
  clip.is_some_and(|clip| {
    clip.width <= 0.0
      || clip.height <= 0.0
      || rect.right() <= clip.x
      || rect.x >= clip.right()
      || rect.bottom() <= clip.y
      || rect.y >= clip.bottom()
  })
}

/// Whether `rect` has a finite position and a positive finite size
fn drawable(rect: Rect) -> bool {
  rect.x.is_finite()
    && rect.y.is_finite()
    && rect.width.is_finite()
    && rect.height.is_finite()
    && rect.width > 0.0
    && rect.height > 0.0
}

#[cfg(all(test, feature = "default-fonts"))]
mod tests {
  use super::*;
  use crate::core::graphics::Sprite;
  use crate::core::ui::Anchor;
  use crate::core::ui::immediate::UiContext;
  use crate::core::ui::test_util::{context, context_scaled, text_engine};

  fn rects(commands: &UiCommands) -> Vec<Rect> {
    commands
      .commands()
      .filter_map(|c| match c {
        UiCommand::Rect { rect, .. } => Some(*rect),
        _ => None,
      })
      .collect()
  }

  fn text_lines(commands: &UiCommands) -> Vec<usize> {
    commands
      .commands()
      .filter_map(|c| match c {
        UiCommand::Text { layout, .. } => Some(layout.lines.len()),
        _ => None,
      })
      .collect()
  }

  #[test]
  fn each_nodes_children_run_in_the_order_they_were_added() {
    let parents = [None, Some(0), Some(1), Some(0), Some(1), Some(3)];
    let nodes: Vec<Node> = parents
      .iter()
      .enumerate()
      .map(|(i, &parent)| {
        let mut node = Node::new(UiId(i as u64), UiStyle::default(), Measure::None);
        node.parent = parent;
        node
      })
      .collect();
    let (mut children, mut ends) = (vec![9; 3], vec![9; 2]);
    child_runs(&nodes, &mut children, &mut ends);
    let run = |i: usize| {
      let start = if i == 0 { 0 } else { ends[i - 1] };
      children[start..ends[i]].to_vec()
    };
    let runs: Vec<Vec<usize>> = (0..nodes.len()).map(run).collect();
    assert_eq!(
      runs,
      vec![vec![1, 3], vec![2, 4], vec![], vec![5], vec![], vec![]]
    );
  }

  #[test]
  fn a_stack_places_its_children_by_anchor_then_offset() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context(&mut commands, &mut text, (400.0, 300.0));
    let mut badge = UiId::NONE;
    ui.stack().width(100.0).height(50.0).show(|ui| {
      badge = ui
        .panel()
        .width(20.0)
        .height(20.0)
        .anchor(Anchor::TopRight)
        .offset(-5.0, 5.0)
        .show(|_| {})
        .id;
    });
    let rect = ui.memory_for_test().current(badge).map(|r| r.rect);
    assert_eq!(rect, Some(Rect::new(75.0, 5.0, 20.0, 20.0)));
  }

  #[test]
  fn a_container_keeps_the_clip_it_started_under() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context(&mut commands, &mut text, (400.0, 300.0));
    let clip = Rect::new(0.0, 0.0, 50.0, 50.0);
    ui.push_clip(clip);
    let id = ui.panel().width(100.0).height(100.0).show(|_| {}).id;
    ui.pop_clip();
    assert_eq!(
      ui.memory_for_test().current(id).and_then(|r| r.clip),
      Some(clip)
    );
    drop(ui);
    let clips: Vec<Option<Rect>> = commands.commands().map(UiCommand::clip).collect();
    assert_eq!(clips, vec![Some(clip)]);
  }

  #[test]
  fn records_name_their_focus_scope_and_depth() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context(&mut commands, &mut text, (400.0, 300.0));
    let mut inner = UiId::NONE;
    let menu = ui
      .column()
      .focus_scope()
      .show(|ui| {
        inner = ui.panel().width(10.0).height(10.0).show(|_| {}).id;
      })
      .id;
    let record = |id| ui.memory_for_test().current(id).copied().unwrap();
    assert_eq!((record(menu).scope, record(menu).depth), (UiId::NONE, 0));
    assert_eq!((record(inner).scope, record(inner).depth), (menu, 1));
  }

  #[test]
  fn a_text_left_without_draw_is_drawn_when_dropped() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context(&mut commands, &mut text, (400.0, 300.0));
    ui.text("in the layout");
    ui.text("placed").at(50.0, 60.0);
    drop(ui);
    let origins: Vec<Vec2> = commands
      .commands()
      .filter_map(UiCommand::text_origin)
      .collect();
    assert_eq!(origins, vec![Vec2::ZERO, Vec2::new(50.0, 60.0)]);
  }

  #[test]
  fn a_lone_text_is_placed_by_its_anchor_and_returns_its_size() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context(&mut commands, &mut text, (400.0, 300.0));
    let measured = ui.measure_text("Hello", &TextStyle::default());
    let size = ui.text("Hello").anchor(Anchor::BottomRight).draw();
    drop(ui);
    assert!(
      (size - measured).abs().max_element() < 1.0,
      "{size} {measured}"
    );
    let origin = commands.commands().find_map(UiCommand::text_origin);
    assert_eq!(origin, Some(Vec2::new(400.0, 300.0) - size));
  }

  #[test]
  fn text_in_a_container_reports_its_unwrapped_size() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context(&mut commands, &mut text, (400.0, 300.0));
    let line = "several words that cannot fit on one line";
    let measured = ui.measure_text(line, &TextStyle::default());
    let mut size = Vec2::ZERO;
    ui.column().width(60.0).show(|ui| {
      size = ui.text(line).draw();
    });
    assert_eq!(size, measured);
  }

  #[test]
  fn a_centred_text_does_not_widen_its_column() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context(&mut commands, &mut text, (400.0, 300.0));
    let measured = ui.measure_text("Title", &TextStyle::default());
    let column = ui
      .column()
      .show(|ui| {
        ui.text("Title").align(TextAlign::Center);
      })
      .rect;
    assert!(
      (column.width - measured.x).abs() < 1.0,
      "{column:?} for text {measured}"
    );
  }

  #[test]
  fn text_in_a_padded_panel_keeps_to_one_line() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    // A scale where snapping moves the panel's edges
    let mut ui = context_scaled(&mut commands, &mut text, (400.0, 300.0), 1.5);
    for line in ["Resume", "Settings", "Quit to the main menu", "Volume 0.73"] {
      ui.panel().padding(7.3).show(|ui| {
        ui.text(line);
      });
    }
    drop(ui);
    assert_eq!(text_lines(&commands), vec![1, 1, 1, 1]);
  }

  #[test]
  fn a_leaf_texts_wrap_is_the_widest_it_gets() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context(&mut commands, &mut text, (400.0, 300.0));
    let column = ui
      .column()
      .show(|ui| {
        ui.text("a few words that need to wrap").wrap(80.0);
      })
      .rect;
    drop(ui);
    assert!(column.width <= 80.5, "{column:?}");
    assert!(text_lines(&commands)[0] > 1);
  }

  #[test]
  fn a_containers_min_content_is_its_longest_word() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context(&mut commands, &mut text, (400.0, 300.0));
    let word = ui.measure_text("wonderful", &TextStyle::default());
    let line = ui.line_height(&TextStyle::default());
    let mut column = UiId::NONE;
    // A narrow grid's track is as wide as its column's min-content width
    ui.grid(1).width(10.0).show(|ui| {
      column = ui
        .column()
        .show(|ui| {
          ui.text("hello wonderful world");
        })
        .id;
    });
    let rect = ui.memory_for_test().current(column).map(|r| r.rect);
    let rect = rect.unwrap_or_default();
    assert!(
      (rect.width - word.x).abs() < 1.0,
      "{rect:?} for the word {word}"
    );
    assert!(
      (rect.height - 3.0 * line).abs() < 1.0,
      "{rect:?} for three lines of {line}"
    );
  }

  #[test]
  fn text_drawn_on_a_node_with_bad_padding_stays_inside_it() {
    for padding in [f32::NAN, -20.0] {
      let (mut commands, mut text) = (UiCommands::default(), text_engine());
      let mut ui = context(&mut commands, &mut text, (400.0, 300.0));
      let style = UiStyle {
        padding: Edges::all(padding),
        ..UiStyle::default()
      };
      let at = Rect::new(10.0, 20.0, 100.0, 40.0);
      let node = ui.open_node(UiId(5), style, Some(at), Flags::default());
      ui.push_draw(
        node,
        DrawKind::Text {
          leaf: Arc::new(TextLeaf::new(
            RichText::new("Hi there"),
            TextStyle::default(),
            TextLayoutOptions::default(),
          )),
          frac: Vec2::ZERO,
          wrap: true,
          opacity: 1.0,
          reveal: None,
        },
      );
      ui.close_node(node);
      drop(ui);
      let origin = commands.commands().find_map(UiCommand::text_origin);
      assert_eq!(origin, Some(Vec2::new(10.0, 20.0)), "padding {padding}");
      assert_eq!(text_lines(&commands), vec![1], "padding {padding}");
    }
  }

  #[test]
  fn a_container_at_a_rect_that_is_not_finite_is_placed_at_zero() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context(&mut commands, &mut text, (400.0, 300.0));
    let rect = ui
      .column()
      .at(Rect::new(f32::NAN, 5.0, f32::INFINITY, 10.0))
      .show(|_| {})
      .rect;
    assert_eq!(rect, Rect::new(0.0, 5.0, 0.0, 10.0));
  }

  #[test]
  fn box_sprite_and_local_draws_land_on_their_node() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui: UiContext<'_> = context(&mut commands, &mut text, (400.0, 300.0));
    let sprite = SpriteHandle::from_sprite(Sprite::solid_rect(4, 4, 255, 255, 255, 255), "s");
    let at = Rect::new(10.0, 20.0, 100.0, 40.0);
    let node = ui.open_node(UiId(77), UiStyle::default(), Some(at), Flags::default());
    ui.push_draw(
      node,
      DrawKind::Box {
        inset: Edges::all(5.0),
        frac: Vec2::new(1.0, 0.5),
        size: Vec2::new(10.0, 10.0),
        fill: Color::RED,
        radius: 0.0,
        border: None,
      },
    );
    ui.push_draw(
      node,
      DrawKind::Sprite {
        inset: Edges::xy(10.0, 0.0),
        frac: Vec2::ZERO,
        size: None,
        sprite: sprite.clone(),
        uv: [0.0, 0.0, 1.0, 1.0],
        tint: Color::WHITE,
        filter: Filter::Default,
      },
    );
    ui.push_draw(
      node,
      DrawKind::Sprite {
        inset: Edges::all(5.0),
        frac: Vec2::new(0.0, 1.0),
        size: Some(Vec2::new(8.0, 6.0)),
        sprite,
        uv: [0.0, 0.0, 1.0, 1.0],
        tint: Color::WHITE,
        filter: Filter::Default,
      },
    );
    ui.push_draw(
      node,
      DrawKind::Local(UiCommand::Line {
        points: [Vec2::ZERO, Vec2::new(4.0, 0.0)].into(),
        width: 1.0,
        color: Color::WHITE,
        closed: false,
        clip: None,
        layer: 0,
      }),
    );
    ui.close_node(node);
    drop(ui);
    assert_eq!(rects(&commands), vec![Rect::new(95.0, 35.0, 10.0, 10.0)]);
    let sprite_rects: Vec<Rect> = commands
      .commands()
      .filter_map(|c| match c {
        UiCommand::Sprite { rect, .. } => Some(*rect),
        _ => None,
      })
      .collect();
    assert_eq!(
      sprite_rects,
      vec![
        Rect::new(20.0, 20.0, 80.0, 40.0),
        Rect::new(15.0, 49.0, 8.0, 6.0)
      ]
    );
    let points = commands.commands().find_map(|c| match c {
      UiCommand::Line { points, .. } => Some(points.clone()),
      _ => None,
    });
    assert_eq!(
      points,
      Some([Vec2::new(10.0, 20.0), Vec2::new(14.0, 20.0)].into())
    );
  }

  #[test]
  fn a_container_left_open_is_laid_out_when_the_scene_ends() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context(&mut commands, &mut text, (400.0, 300.0));
    let style = UiStyle {
      width: Length::Px(30.0),
      height: Length::Px(30.0),
      ..UiStyle::default()
    };
    // As if its contents had panicked: opened, never closed
    let node = ui.open_node(UiId(9), style, None, Flags::BLOCKING);
    ui.panel_fill(node, false);
    ui.end_scene();
    assert!(ui.memory_for_test().current(UiId(9)).is_some());
    // The next container starts a subtree of its own
    let next = ui.panel().width(20.0).height(20.0).show(|_| {});
    assert_eq!(next.rect, Rect::new(0.0, 0.0, 20.0, 20.0));
    drop(ui);
    assert_eq!(rects(&commands).len(), 2);
    let warnings = crate::core::dev_log::get_entries()
      .iter()
      .filter(|e| e.message.contains("still open when the scene's ui ended"))
      .count();
    assert_eq!(warnings, 1);
  }
}

#[cfg(test)]
mod clone_tests {
  use super::*;
  use std::sync::Arc;

  fn leaf(text: &str) -> Arc<TextLeaf> {
    Arc::new(TextLeaf::new(
      RichText::new(text),
      TextStyle::default(),
      TextLayoutOptions::default(),
    ))
  }

  #[test]
  fn a_cloned_text_draw_shares_its_leaf() {
    let leaf = leaf("Hello");
    let draw = Draw {
      order: 3,
      layer: 1,
      node: 0,
      kind: DrawKind::Text {
        leaf: Arc::clone(&leaf),
        frac: Vec2::ZERO,
        wrap: false,
        opacity: 1.0,
        reveal: None,
      },
    };
    let copy = draw.clone();
    let DrawKind::Text { leaf: copied, .. } = &copy.kind else {
      panic!("a text draw")
    };
    assert!(Arc::ptr_eq(copied, &leaf));
    assert_eq!((copy.order, copy.layer, copy.node), (3, 1, 0));
  }

  fn shared_of(node: &Node) -> &Arc<TextLeaf> {
    match node.text.as_ref() {
      Some(Leaf::Shared(leaf)) => leaf,
      _ => panic!("a shared leaf"),
    }
  }

  #[test]
  fn a_cloned_node_shares_its_text() {
    let mut node = Node::new(UiId(1), UiStyle::default(), Measure::None);
    node.text = Some(Leaf::Shared(leaf("Label")));
    let copy = node.clone();
    assert!(Arc::ptr_eq(shared_of(&copy), shared_of(&node)));
  }

  #[test]
  fn an_owned_leaf_is_read_from_its_subtree_and_a_shared_one_from_itself() {
    let leaves = [TextLeaf::new(
      RichText::new("Label"),
      TextStyle::default(),
      TextLayoutOptions::default(),
    )];
    let owned = Leaf::Owned(0)
      .get(&leaves)
      .map(|leaf| leaf.text.text.as_str());
    assert_eq!(owned, Some("Label"));
    assert!(Leaf::Owned(1).get(&leaves).is_none(), "out of its subtree");
    let shared = leaf("Shared");
    let read = Leaf::Shared(Arc::clone(&shared));
    assert!(
      read
        .get(&[])
        .is_some_and(|leaf| std::ptr::eq(leaf, &*shared))
    );
  }

  #[test]
  fn a_cloned_line_draw_shares_its_points() {
    let points: Arc<[Vec2]> = Arc::from(vec![Vec2::ZERO, Vec2::ONE]);
    let draw = DrawKind::Local(UiCommand::Line {
      points: Arc::clone(&points),
      width: 1.0,
      color: Color::WHITE,
      closed: false,
      clip: None,
      layer: 0,
    });
    let DrawKind::Local(UiCommand::Line { points: copied, .. }) = draw.clone() else {
      panic!("a line")
    };
    assert!(Arc::ptr_eq(&copied, &points));
  }
}
