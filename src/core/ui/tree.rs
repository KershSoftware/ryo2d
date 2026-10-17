//! The subtree being built: the nodes a top-level container (or a lone leaf)
//! opens, the draws placed on them, and their resolution into commands and
//! records once the layout engine has placed them

use std::cell::RefCell;

use glam::Vec2;

use super::immediate::{UiCommand, UiCommands};
use super::layout::engine::{Avail, LayoutEngine, NodeDesc};
use super::layout::{Edges, Flow, Length, Measure, UiStyle};
use super::memory::{Flags, Record, UiMemory};
use super::widgets::scroll::ScrollState;
use super::{Color, Rect, UiId};
use crate::core::asset::SpriteHandle;
use crate::core::render::UiSpace;
use crate::core::render::batch::Filter;
use crate::core::render::ui::snap_rect;
use crate::core::text::{RichText, TextAlign, TextEngine, TextLayoutOptions, TextStyle};

/// Added to every wrap width a text node is measured and drawn at, so float
/// rounding in layout never wraps a text at its own width
const WRAP_SLACK: f32 = 0.01;

/// The nodes and draws of one top-level container (or lone leaf) being
/// recorded; node 0 is its root
pub(crate) struct Subtree {
  pub(crate) nodes: Vec<Node>,
  pub(crate) draws: Vec<Draw>,
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
      at,
      clip,
      scope,
      below: None,
    }
  }
}

/// One container or leaf in a subtree
pub(crate) struct Node {
  pub(crate) id: UiId,
  pub(crate) parent: Option<usize>,
  pub(crate) style: UiStyle,
  pub(crate) measure: Measure,
  /// The text a `Measure::Text` node is measured by
  pub(crate) text: Option<TextLeaf>,
  /// Changes when what is measured changes
  pub(crate) content_key: u64,
  pub(crate) children: Vec<usize>,
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
      children: Vec::new(),
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
}

/// A draw on a node, placed when the subtree is laid out
pub(crate) struct Draw {
  /// Taken when it was recorded, so it keeps its place in the painter's order
  pub(crate) order: u32,
  pub(crate) layer: u8,
  pub(crate) node: usize,
  pub(crate) kind: DrawKind,
}

/// What a draw is, relative to its node
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
  /// its width when `wrap`
  Text {
    /// Boxed: a text is far larger than the other draws
    leaf: Box<TextLeaf>,
    frac: Vec2,
    wrap: bool,
    opacity: f32,
    reveal: Option<(f32, f32)>,
    seed: u64,
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

/// A laid-out subtree: each node's rect (absolute, snapped to render pixels)
/// and its content extent
#[derive(Debug, Default)]
pub(crate) struct Resolved {
  pub(crate) rects: Vec<Rect>,
  pub(crate) content: Vec<Vec2>,
}

/// What resolving a subtree reads and writes
pub(crate) struct ResolveCx<'r, 'a> {
  pub(crate) layout: &'r mut LayoutEngine,
  pub(crate) text: &'r RefCell<&'a mut TextEngine>,
  pub(crate) memory: &'r mut UiMemory,
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
/// the commands, each with the order it was recorded in
pub(crate) fn resolve(subtree: Subtree, cx: ResolveCx<'_, '_>) -> Resolved {
  let Subtree {
    nodes,
    draws,
    at,
    clip,
    below,
    ..
  } = subtree;
  if nodes.is_empty() {
    return Resolved::default();
  }
  let ui_size = cx.space.size;
  let scale = cx.space.px_scale;

  // Lay out: an `at` rect is the room offered and the root's size
  let root_style = at.map(|at| UiStyle {
    width: Length::Px(at.width),
    height: Length::Px(at.height),
    ..nodes[0].style.clone()
  });
  // A nested node with an `at` rect leaves its parent's flow, at that size
  let floating: Vec<Option<UiStyle>> = nodes
    .iter()
    .map(|node| {
      node.at.filter(|_| node.parent.is_some()).map(|at| UiStyle {
        width: Length::Px(at.width),
        height: Length::Px(at.height),
        absolute: true,
        ..node.style.clone()
      })
    })
    .collect();
  let descs: Vec<NodeDesc<'_>> = nodes
    .iter()
    .enumerate()
    .map(|(i, node)| NodeDesc {
      id: node.id,
      style: match (&root_style, &floating[i], i) {
        (Some(style), _, 0) => style,
        (_, Some(style), _) => style,
        _ => &node.style,
      },
      measure: node.measure,
      content_key: node.content_key,
      children: &node.children,
    })
    .collect();
  let available = at.map_or(ui_size, |at| Vec2::new(at.width, at.height));
  let text = cx.text;
  let mut measure = |i: usize, known: (Option<f32>, Option<f32>), avail: (Avail, Avail)| {
    nodes
      .get(i)
      .and_then(|node| node.text.as_ref())
      .map_or(Vec2::ZERO, |leaf| {
        measure_text(text, leaf, known.0, avail.0, scale)
      })
  };
  let placed = cx.layout.layout(&descs, available, &mut measure);

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
  let count = nodes.len();
  let mut shift = vec![Vec2::ZERO; count];
  let mut exact: Vec<Rect> = Vec::with_capacity(count);
  let mut rects: Vec<Rect> = Vec::with_capacity(count);
  let mut clips: Vec<Option<Rect>> = Vec::with_capacity(count);
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
      if let Some(state) = cx
        .memory
        .peek_state::<ScrollState>(nodes[p].id)
        .filter(|_| scroll_x || scroll_y)
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
  let resolved = Resolved {
    rects,
    content: placed.iter().map(|p| p.content).collect(),
  };

  // A scroll area keeps the size of its view and contents for next frame's
  // wheel, scrollbar and `scroll_to`; an axis it does not scroll has no more
  // contents than view
  for (i, node) in nodes.iter().enumerate() {
    let (x, y) = (node.style.scroll_x, node.style.scroll_y);
    if !(x || y) {
      continue;
    }
    if let Some(state) = cx.memory.state::<ScrollState>(node.id, cx.scene) {
      let view = Vec2::new(resolved.rects[i].width, resolved.rects[i].height);
      // The contents overflow by what they pass the unsnapped view: an area
      // snapped a fraction smaller than its layout still fits them
      let snapping = view - Vec2::new(exact[i].width, exact[i].height);
      let content = resolved.content[i] + snapping;
      state.view = view;
      state.content = Vec2::new(
        if x { content.x.max(view.x) } else { view.x },
        if y { content.y.max(view.y) } else { view.y },
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
        rect: resolved.rects[i],
        clip: clips[i],
        layer: node.layer,
        order: node.order,
        depth: node.depth,
        scene: cx.scene,
        flags: spec.flags,
        scope: node.scope,
        scroll: node.scroll,
        content: resolved.content[i],
        nav: spec.nav,
        accepts: spec.accepts,
      },
    );
  }

  for draw in draws {
    let Some(node) = nodes.get(draw.node) else {
      continue;
    };
    let place = Placement {
      rect: resolved.rects[draw.node],
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
      for command in nine_slice(room, &slice, place.clip, place.layer, scale) {
        cx.commands.push(draw.order, command);
      }
    } else if let Some(command) = command_of(draw.kind, &place, text, scale) {
      cx.commands.push(draw.order, command);
    }
  }
  resolved
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
/// (left, top, right, bottom): up to nine sprite commands, edges snapped to
/// render pixels so the pieces meet without gaps. Corners shrink together
/// when `room` is smaller than they are.
fn nine_slice(
  room: Rect,
  slice: &NineSlice<'_>,
  clip: Option<Rect>,
  layer: u8,
  scale: f32,
) -> Vec<UiCommand> {
  let NineSlice {
    sprite,
    insets,
    tint,
    filter,
  } = *slice;
  let (width, height) = (sprite.width() as f32, sprite.height() as f32);
  if !(drawable(room) && width > 0.0 && height > 0.0) {
    return Vec::new();
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
  let mut commands = Vec::with_capacity(9);
  for row in 0..3 {
    for column in 0..3 {
      let rect = Rect::new(
        xs[column],
        ys[row],
        xs[column + 1] - xs[column],
        ys[row + 1] - ys[row],
      );
      if drawable(rect) && !outside(clip, rect) {
        commands.push(UiCommand::Sprite {
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
  commands
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
  engine.measure(&leaf.text, &leaf.style, &options, scale)
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

/// A draw as a command in UI units, or `None` when nothing of it would show
fn command_of(
  kind: DrawKind,
  place: &Placement,
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
      seed,
    } => {
      let content = inset_rect(place.rect, place.padding);
      let mut options = leaf.options;
      if wrap {
        let width = (place.exact.width - place.padding.width()).max(0.0) + WRAP_SLACK;
        options.wrap = narrowest(Some(width), leaf.options.wrap);
      }
      let layout = text
        .try_borrow_mut()
        .ok()?
        .layout(&leaf.text, &leaf.style, &options, scale)?;
      let size = layout.size / scale;
      let origin =
        Vec2::new(content.x, content.y) + frac * (Vec2::new(content.width, content.height) - size);
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
          mut points,
          width,
          color,
          closed,
          ..
        } => {
          for point in &mut points {
            *point += by;
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

/// A vector with any non-finite part made zero
fn finite(v: Vec2) -> Vec2 {
  Vec2::new(
    if v.x.is_finite() { v.x } else { 0.0 },
    if v.y.is_finite() { v.y } else { 0.0 },
  )
}

/// `edges` as layout used them: a side that is not finite or is negative
/// is 0 (layout warned about it)
fn clean_edges(edges: Edges) -> Edges {
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
          leaf: Box::new(TextLeaf {
            text: RichText::new("Hi there"),
            style: TextStyle::default(),
            options: TextLayoutOptions::default(),
          }),
          frac: Vec2::ZERO,
          wrap: true,
          opacity: 1.0,
          reveal: None,
          seed: 0,
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
        points: vec![Vec2::ZERO, Vec2::new(4.0, 0.0)],
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
      Some(vec![Vec2::new(10.0, 20.0), Vec2::new(14.0, 20.0)])
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
