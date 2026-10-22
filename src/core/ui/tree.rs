//! The subtree being built: the nodes a top-level container (or a lone leaf)
//! opens, the draws placed on them, and their resolution into commands and
//! records once the layout engine has placed them

use std::cell::RefCell;
use std::ops::Range;
use std::sync::Arc;

use glam::Vec2;

use super::fragment::{Fragment, Inputs, KeptDraw, KeptRecord, Role, sized_by};
use super::immediate::{FragmentRun, ScrollSlot, UiCommand, UiCommands, intersect, meet, outside};
use super::layout::engine::{Avail, LayoutEngine, Measured, NodeDesc, Placed};
use super::layout::{Edges, Flow, Length, Measure, UiStyle};
use super::memory::{Flags, Record, Scratch, UiMemory, recycle};
use super::theme::warn_once;
use super::widgets::scroll::{ScrollState, drawn_px};
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
  /// How it takes part in fragments: a kept fragment's root, or a clean
  /// subtree's root whose resolved output is kept
  pub(crate) role: Role,
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
      role: Role::None,
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
  /// Where a kept fragment's commands go in the painter's order: on its
  /// root, the kept node. Never taped, and drawn from the fragment, never
  /// by `command_of`.
  Kept,
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
///
/// A kept node (`Role::Kept`) is a fragment's root: its fragment, taken
/// from last frame's, is copied when its `Inputs` are unchanged and
/// resolved again from what it kept otherwise, then kept for next frame. A
/// captured node (`Role::Capture`) is a clean subtree's root: what its
/// subtree resolves to is kept as its fragment.
pub(crate) fn resolve(subtree: &mut Subtree, cx: ResolveCx<'_, '_>) -> Option<Rect> {
  let (at, clip, below) = (subtree.at, subtree.clip, subtree.below);
  let nodes: &[Node] = &subtree.nodes;
  let leaves: &[TextLeaf] = &subtree.leaves;
  if nodes.is_empty() {
    return None;
  }
  let ResolveCx {
    layout,
    text,
    memory,
    scratch,
    commands,
    space,
    scene,
  } = cx;
  let ui_size = space.size;
  let scale = space.px_scale;

  // Each kept node's fragment, taken out of last frame's; and a nested
  // node with an `at` rect leaves its parent's flow, at that size
  let mut keeping = std::mem::take(&mut scratch.keeping);
  keeping.kept.clear();
  let mut captures = false;
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
    match node.role {
      Role::Kept => {
        let fragment = memory.fragments_prev.remove(&node.id);
        if fragment.is_none() {
          warn_once("ui-fragment:missing".to_string(), || {
            "a kept UI fragment was missing when its subtree was laid out; drawn empty".to_string()
          });
        }
        keeping.kept.push((i, fragment));
      }
      Role::Capture => captures = true,
      Role::None => {}
    }
  }

  // Lay out: an `at` rect is the room offered and the root's size
  let root_style = at.map(|at| UiStyle {
    width: Length::Px(at.width),
    height: Length::Px(at.height),
    ..nodes[0].style.clone()
  });
  child_runs(nodes, &mut scratch.children, &mut scratch.child_ends);
  let (children, ends) = (&scratch.children, &scratch.child_ends);
  let mut floating = scratch.floating.iter().peekable();
  let mut descs: Vec<NodeDesc<'_>> = recycle(std::mem::take(&mut scratch.descs));
  // A kept node is laid out with its subtree as the layout keeps it: its
  // descendants are never described here (the layout would count them
  // twice), since a fragment's nodes are pushed only through its root
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
      kept: node.role == Role::Kept,
    }
  }));
  let available = at.map_or(ui_size, |at| Vec2::new(at.width, at.height));
  {
    let kept = &keeping.kept;
    let mut measure =
      |target: Measured, known: (Option<f32>, Option<f32>), avail: (Avail, Avail)| {
        let leaf = match target {
          Measured::Index(i) => nodes
            .get(i)
            .and_then(|node| node.text.as_ref()?.get(leaves)),
          // Asked only when the layout lays a kept subtree out again
          Measured::Kept(id) => kept_leaf(kept, id),
        };
        leaf.map_or(Vec2::ZERO, |leaf| {
          measure_text(text, leaf, known.0, avail.0, scale)
        })
      };
    // Only a subtree capturing a fragment counts its nodes' subtrees, for
    // the frames that keep them
    if captures {
      layout.layout_counting(&descs, available, &mut measure);
    } else {
      layout.layout(&descs, available, &mut measure);
    }
  }
  scratch.descs = recycle(descs);
  let layout: &LayoutEngine = layout;
  let placed = layout.placed();

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

  // Each node's rect and clip: absolute for its record, and relative to its
  // slot for its draws. A parent always comes before its children.
  let s = if scale.is_finite() && scale > 0.0 {
    scale
  } else {
    1.0
  };
  let from = Origin { origin, clip, s };
  let mut placing = std::mem::take(&mut scratch.placing);
  let placing = &mut placing;
  placing.clear(s);
  for (i, node) in nodes.iter().enumerate() {
    let parent = node.parent.filter(|&p| p < i).map(|p| (p, &nodes[p]));
    placing.place(node, parent, placed[i].rect, from, memory, commands);
  }

  // A scroll area keeps the size of its view and contents for next frame's
  // wheel, scrollbar and `scroll_to`; an axis it does not scroll has no more
  // contents than view
  for (i, node) in nodes.iter().enumerate() {
    let (x, y) = (node.style.scroll_x, node.style.scroll_y);
    if !(x || y) {
      continue;
    }
    if let Some(state) = memory.state::<ScrollState>(node.id, scene) {
      let at = &placing.nodes[i];
      let shown = at.rect;
      let view = Vec2::new(shown.width, shown.height);
      // The contents overflow by what they pass the unsnapped view: an area
      // snapped a fraction smaller than its layout still fits them
      let exact = at.exact;
      let snapping = view - Vec2::new(exact.width, exact.height);
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

  // Each kept fragment whose inputs changed is resolved again from what it
  // kept, against the layout as it is now; then every one is copied
  let Keeping {
    kept,
    sizes,
    span_of,
    captures: capturing,
    run,
  } = &mut keeping;
  for (i, slot) in kept.iter_mut() {
    let Some(fragment) = slot else {
      continue;
    };
    let laid = placed[*i].rect;
    let inputs = placing.inputs(*i, nodes[*i].layer, laid, from, scale);
    if inputs == fragment.inputs && fragment.sprites_hold() {
      memory.fragment_stats.reused += 1;
      continue;
    }
    let again = Again {
      layout,
      text,
      scale,
      from,
      scene,
    };
    if !again.resolve(fragment, *i, laid, placing, run, memory, commands) {
      warn_once("ui-fragment:unheld".to_string(), || {
        "a kept UI fragment's nodes were no longer laid out; drawn empty".to_string()
      });
      *slot = None;
      continue;
    }
    fragment.inputs = inputs;
    fragment.interest = placing.slot_of(*i).interest_bits();
    fragment.version = memory.next_fragment_version();
    memory.fragment_stats.resolved_again += 1;
  }

  // The captured nodes whose subtrees can be kept: each one's span of nodes
  // is marked for the records and draws loops to keep what they write. A
  // span whose nodes and draws do not take one run of orders is not kept,
  // and its draws are culled as loose ones.
  capturing.clear();
  if captures {
    let mut keepable = |id| {
      let log = scratch.capture_of(id)?;
      Some(!(log.popup || log.missed))
    };
    spans(nodes, sizes, span_of, capturing, &mut keepable);
    count_runs(&subtree.draws, span_of, capturing);
  }
  let captured = |i: usize| -> Option<usize> {
    span_of
      .get(i)
      .copied()
      .filter(|&c| c != NO_SPAN)
      .map(|c| c as usize)
  };

  // Records, for next frame's hit tests and navigation: absolute
  for (i, node) in nodes.iter().enumerate() {
    if node.role == Role::Kept {
      if let Some(fragment) = kept_at(kept, i) {
        let slot = placing.slot_of(i);
        let copy = CopyAt {
          root: node,
          slot,
          s,
          scene,
        };
        copy_records(fragment, copy, memory);
      }
      continue;
    }
    let Some(spec) = node.record else {
      continue;
    };
    let record = placing.record(i, node, spec, placed[i].content, scene);
    if let Some(c) = captures.then(|| captured(i)).flatten() {
      let capture = &mut capturing[c];
      let order = node.order.wrapping_sub(capture.base);
      let depth = node.depth.wrapping_sub(capture.depth);
      let kept = placing.kept_record(i, node.id, record, order, depth);
      capture.records.push(kept);
    }
    memory.record(node.id, record);
  }

  // Draws, relative to their node's slot and culled by its clip there. Each
  // fragment's commands are one run: a kept one's are pushed together, and
  // a capture's go from where the first draw at or past its root's order
  // lands (its own, or the next one's when none of its draws is pushed)
  let runs_from = commands.runs().len();
  if captures {
    let mut opening = 0;
    for draw in subtree.draws.drain(..) {
      start_runs(capturing, &mut opening, draw.order, commands.position());
      if matches!(draw.kind, DrawKind::Kept) {
        if let (Some(node), Some(fragment)) = (nodes.get(draw.node), kept_at_mut(kept, draw.node)) {
          let slot = placing.slot_of(draw.node);
          copy_draws(fragment, node, slot, memory, commands);
        }
        continue;
      }
      let (Some(node), Some(at)) = (nodes.get(draw.node), placing.nodes.get(draw.node)) else {
        continue;
      };
      let slot = placing.slots[at.slot];
      let (index, mut place) = placement(at, &slot, draw.layer);
      let mut capture = captured(draw.node).and_then(|c| capturing.get_mut(c));
      if let Some(capture) = capture.as_deref_mut() {
        capture.keep_draw(&draw);
        // A fragment's draws are culled by its slot's interest
        place.cull = slot.fragment_cull(place.clip);
      }
      let order = draw.order;
      draw_commands(
        draw.kind,
        &place,
        node,
        leaves,
        text,
        scale,
        |command, seen| {
          if let Some(capture) = capture.as_deref_mut() {
            capture.draws.push(KeptDraw {
              order: order.wrapping_sub(capture.base),
              command: command.clone(),
              cull: seen,
            });
          }
          if !seen.is_some_and(|seen| outside(place.cull, seen)) {
            commands.push(order, index, command);
            if let Some(capture) = capture.as_deref_mut() {
              capture.pushed += 1;
              capture.run.end = commands.position();
            }
          }
        },
      );
    }
    start_runs(capturing, &mut opening, u32::MAX, commands.position());
  } else {
    // Nothing is captured: a draw's commands are made only where they show
    for draw in subtree.draws.drain(..) {
      if matches!(draw.kind, DrawKind::Kept) {
        if let (Some(node), Some(fragment)) = (nodes.get(draw.node), kept_at_mut(kept, draw.node)) {
          let slot = placing.slot_of(draw.node);
          copy_draws(fragment, node, slot, memory, commands);
        }
        continue;
      }
      let (Some(node), Some(at)) = (nodes.get(draw.node), placing.nodes.get(draw.node)) else {
        continue;
      };
      let (index, place) = placement(at, &placing.slots[at.slot], draw.layer);
      let order = draw.order;
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
        nine_slice(room, &slice, &place, scale, place.cull, |command, _| {
          commands.push(order, index, command);
        });
      } else if let Some((command, _)) =
        command_of(draw.kind, &place, node, leaves, text, scale, place.cull)
      {
        commands.push(order, index, command);
      }
    }
  }

  // What each capture kept becomes its fragment
  for capture in capturing.drain(..) {
    let i = capture.root;
    let node = &nodes[i];
    let Some(count) = capture.count() else {
      continue;
    };
    let Some(log) = scratch.capture_of(node.id) else {
      continue;
    };
    #[cfg(debug_assertions)]
    let built = log.spot;
    let inputs = placing.inputs(i, node.layer, placed[i].rect, from, scale);
    let slot = placing.slot_of(i);
    let mut root = node.clone();
    root.role = Role::None;
    let span = nodes[i..capture.end].iter().enumerate().map(|(k, n)| {
      let mut n = n.clone();
      n.role = Role::None;
      n.order = n.order.wrapping_sub(capture.base);
      n.depth = n.depth.wrapping_sub(capture.depth);
      n.parent = if k == 0 {
        None
      } else {
        n.parent.map(|p| p - i)
      };
      n
    });
    let fragment = Fragment {
      built: log.built,
      scene,
      inputs,
      count,
      root,
      at: if i == 0 { at } else { None },
      ids: std::mem::take(&mut log.ids),
      states: std::mem::take(&mut log.states),
      entities: std::mem::take(&mut log.entities),
      records: capture.records,
      draws: capture.draws,
      nodes: span.collect(),
      node_draws: capture.node_draws,
      sprites: capture.sprites,
      interest: slot.interest_bits(),
      version: memory.next_fragment_version(),
    };
    // One the spot checks captured in place of a copy is counted apart
    #[cfg(debug_assertions)]
    let (fragment, spotted) = {
      let copy = CopyAt {
        root: node,
        slot,
        s,
        scene,
      };
      spot_check(fragment, copy, built, memory)
    };
    #[cfg(not(debug_assertions))]
    let spotted = false;
    if !spotted {
      memory.fragment_stats.captured += 1;
    }
    // Its commands pushed above, which nothing else's came between
    let run = capture.run;
    let together = run.end.checked_sub(run.start) == Some(capture.pushed);
    debug_assert!(together, "a capture's commands are pushed together");
    if together {
      commands.push_run(FragmentRun {
        id: node.id,
        version: fragment.version,
        base: capture.base,
        layer: node.layer,
        slot: slot.table_index(),
        draws: run,
      });
    }
    memory.fragments_next.insert(node.id, Box::new(fragment));
  }
  // Captured runs were pushed after the kept ones
  if captures {
    commands.sort_runs(runs_from);
  }

  // Every kept fragment is kept for next frame
  for (i, fragment) in kept.drain(..) {
    if let Some(fragment) = fragment {
      memory.fragments_next.insert(nodes[i].id, fragment);
    }
  }
  let rect = placing.nodes.first().map(|at| at.rect);
  scratch.keeping = keeping;
  scratch.placing = std::mem::take(placing);
  rect
}

/// What placing a node reads besides its parent: its subtree's origin (UI
/// units) and clip, and the render scale
#[derive(Debug, Clone, Copy)]
struct Origin {
  origin: Vec2,
  clip: Option<Rect>,
  s: f32,
}

/// Where resolve put each node, in node order: what its placement fills
/// and its records and draws read. Kept between subtrees in `Scratch`.
#[derive(Default)]
pub(crate) struct Placing {
  nodes: Vec<NodeAt>,
  /// The subtree's slots: slot 0, then each scroll area's
  slots: Vec<SlotPlace>,
  /// Render pixels per UI unit
  s: f32,
}

/// Where resolve put one node, in the coordinates its draws carry
#[derive(Debug, Clone, Copy)]
struct NodeAt {
  /// How far it is moved from where the layout put it, scrolling aside
  shift: Vec2,
  /// Its rect before snapping, unscrolled
  exact: Rect,
  /// Its slot, in the subtree's slots
  slot: usize,
  /// Its contents' slot: a scroll area's own, else its slot
  content: usize,
  /// Its rect snapped to render pixels: relative to its slot when its slot
  /// is in the table, else absolute
  rect: Rect,
  /// Its draws' clip in the same coordinates; in a slot, `None` is the
  /// slot's clip alone (a clipping node between adds its own)
  clip: Option<Rect>,
  /// Its edges in whole render pixels from its slot's base: x0, y0, x1, y1
  edges: [f32; 4],
}

impl Placing {
  /// Nothing placed, in slot 0 alone, at `s` render pixels per UI unit
  fn clear(&mut self, s: f32) {
    self.nodes.clear();
    self.slots.clear();
    self.slots.push(SlotPlace::NONE);
    self.s = s;
  }

  /// How many nodes are placed
  fn len(&self) -> usize {
    self.nodes.len()
  }

  /// Back to the first `len` nodes placed
  fn truncate(&mut self, len: usize) {
    self.nodes.truncate(len);
  }

  /// Node `i` placed again after the last: a fragment resolved again has
  /// its root where its kept node is
  fn repeat(&mut self, i: usize) {
    self.nodes.push(self.nodes[i]);
  }

  /// Place `node`, laid out at `r` (relative to its subtree's root), after
  /// the last node placed: `parent` is its parent's place here and node
  /// (`None` for the subtree's root). A scroll area's contents are placed
  /// relative to its slot, on whole render pixels counted from the slot's
  /// origin, so its offset moves them rigidly; a scroll area with a state
  /// adds its slot to the frame's. Inlined: resolve places every node.
  #[inline(always)]
  fn place(
    &mut self,
    node: &Node,
    parent: Option<(usize, &Node)>,
    r: Rect,
    from: Origin,
    memory: &UiMemory,
    commands: &mut UiCommands,
  ) {
    let Origin { origin, clip, s } = from;
    let mut shift = Vec2::ZERO;
    let mut slot = 0;
    if let Some((p, up)) = parent {
      shift = self.nodes[p].shift;
      // A stack places its children by anchor, then moves them by their offset
      if matches!(up.style.flow, Flow::Stack) {
        shift += finite(node.style.offset);
      }
      slot = self.nodes[p].content;
    }
    // A node placed at a rect goes there, wherever its parent is: it does
    // not scroll
    if let Some(at) = node.at.filter(|_| node.parent.is_some()) {
      shift = Vec2::new(at.x, at.y) - origin - Vec2::new(r.x, r.y);
      slot = 0;
    }
    let at = origin + Vec2::new(r.x, r.y) + shift;
    let unsnapped = Rect::new(at.x, at.y, r.width, r.height);

    // Its edges in whole render pixels from its slot's origin
    let place = self.slots[slot];
    let edge = |v: f32, base: f32| (v * s - base).round();
    let (x0, x1) = (
      edge(unsnapped.x, place.base.x),
      edge(unsnapped.right(), place.base.x),
    );
    let (y0, y1) = (
      edge(unsnapped.y, place.base.y),
      edge(unsnapped.bottom(), place.base.y),
    );
    let relative = rect_of(x0, y0, x1, y1, s);
    // In a slot of the table it is placed relative to the slot (its record
    // makes it absolute); in one past the table, absolute
    let rect = if slot == 0 || place.real() {
      relative
    } else {
      let (dx, dy) = (place.offset.x, place.offset.y);
      rect_of(x0 + dx, y0 + dy, x1 + dx, y1 + dy, s)
    };

    // Its clip: inside a slot, relative to it from the slot's first
    // contents (whose clip is the slot's own) on; elsewhere absolute
    let clip = match parent {
      None => clip,
      Some((p, up)) if place.real() => {
        let above = &self.nodes[p];
        let starts_slot = above.content == slot && above.slot != slot;
        if starts_slot {
          None
        } else if up.clips {
          meet(above.clip, Some(above.rect))
        } else {
          above.clip
        }
      }
      Some((p, up)) => {
        let (rect, mut abs) = self.absolute(p);
        if up.clips {
          abs = Some(match abs {
            Some(c) => intersect(c, rect),
            None => rect,
          });
        }
        abs
      }
    };

    // A scroll area is a slot for its contents, drawn at its offset rounded
    // to whole render pixels
    let mut content = slot;
    let (scroll_x, scroll_y) = (node.style.scroll_x, node.style.scroll_y);
    if (scroll_x || scroll_y)
      && let Some(state) = memory.peek_state::<ScrollState>(node.id)
    {
      let drawn = drawn_px(state.offset, [scroll_x, scroll_y], s);
      let corner = Vec2::new(x0, y0);
      let offset = place.offset + corner - Vec2::new(drawn[0] as f32, drawn[1] as f32);
      let (absolute, clip_abs) = if place.real() {
        let (dx, dy) = (place.offset.x, place.offset.y);
        let absolute = rect_of(x0 + dx, y0 + dy, x1 + dx, y1 + dy, s);
        (absolute, slot_clip(&place, clip))
      } else {
        (rect, clip)
      };
      let view = Some(match clip_abs {
        Some(c) => intersect(c, absolute),
        None => absolute,
      });
      let cull = view.map(|c| moved(c, -offset / s));
      let size = Vec2::new(relative.width, relative.height);
      let index = commands.push_slot(ScrollSlot {
        offset: [offset.x as i32, offset.y as i32],
        clip: view,
        cull,
      });
      self.slots.push(SlotPlace {
        base: place.base + corner,
        offset,
        units: offset / s,
        clip: view,
        cull,
        interest: Some(interest(size, drawn, [scroll_x, scroll_y], s)),
        index,
      });
      content = self.slots.len() - 1;
    }
    self.nodes.push(NodeAt {
      shift,
      exact: unsnapped,
      slot,
      content,
      rect,
      clip,
      edges: [x0, y0, x1, y1],
    });
  }

  /// The slot node `i` is placed in
  fn slot_of(&self, i: usize) -> SlotPlace {
    self.slots[self.nodes[i].slot]
  }

  /// Node `i`'s rect and clip, absolute (its record's): in a slot of the
  /// table, made so from its edges and clip there by the slot's offset.
  /// Inlined: every record asks.
  #[inline(always)]
  fn absolute(&self, i: usize) -> (Rect, Option<Rect>) {
    let at = &self.nodes[i];
    let slot = &self.slots[at.slot];
    if slot.real() {
      let [x0, y0, x1, y1] = at.edges;
      let (dx, dy) = (slot.offset.x, slot.offset.y);
      let rect = rect_of(x0 + dx, y0 + dy, x1 + dx, y1 + dy, self.s);
      (rect, slot_clip(slot, at.clip))
    } else {
      (at.rect, at.clip)
    }
  }

  /// Node `i`'s record (`node` placed there, with `spec` and the size of
  /// its `content`), in `scene`
  fn record(&self, i: usize, node: &Node, spec: RecordSpec, content: Vec2, scene: u64) -> Record {
    let (rect, clip) = self.absolute(i);
    Record {
      rect,
      clip,
      layer: node.layer,
      order: node.order,
      depth: node.depth,
      scene,
      flags: spec.flags,
      scope: node.scope,
      scroll: node.scroll,
      content,
      nav: spec.nav,
      accepts: spec.accepts,
    }
  }

  /// Node `i`'s `record`, of widget `id`, kept in a fragment, with its
  /// order and depth less its fragment root's
  fn kept_record(&self, i: usize, id: UiId, record: Record, order: u32, depth: u16) -> KeptRecord {
    let at = &self.nodes[i];
    KeptRecord {
      id,
      record,
      edges: at.edges,
      clip: at.clip,
      order,
      depth,
    }
  }

  /// What node `i` on `layer`, laid out at `laid`, is resolved from
  /// beyond its own subtree, placed from `from` at render scale `scale`:
  /// a fragment rooted there is copied while they are the same
  fn inputs(&self, i: usize, layer: u8, laid: Rect, from: Origin, scale: f32) -> Inputs {
    let slot = self.slot_of(i);
    let kind = match (slot.index, slot.real()) {
      (0, _) => 0,
      (_, true) => 1,
      (_, false) => 2,
    };
    Inputs {
      origin: Inputs::vec(from.origin),
      rect: Inputs::rect(laid),
      shift: Inputs::vec(self.nodes[i].shift),
      base: Inputs::vec(slot.base),
      scale: scale.to_bits(),
      slot: kind,
      offset: if slot.real() {
        [0, 0]
      } else {
        Inputs::vec(slot.offset)
      },
      clip: self.nodes[i].clip.map(Inputs::rect),
      layer,
    }
  }
}

/// A rect from its edges in render pixels, at `s` render pixels per UI unit
fn rect_of(x0: f32, y0: f32, x1: f32, y1: f32, s: f32) -> Rect {
  Rect::new(x0 / s, y0 / s, x1 / s - x0 / s, y1 / s - y0 / s)
}

/// The absolute clip of a node in slot `place` whose clip relative to the
/// slot is `rel` (`None`: the slot's alone)
fn slot_clip(place: &SlotPlace, rel: Option<Rect>) -> Option<Rect> {
  let moved_out = rel.map(|c| moved(c, place.units));
  meet(place.clip, moved_out)
}

/// Where a draw on a node placed at `at` in `slot` lands, on `layer`: its
/// slot's index in the frame's table (0 for none) and its placement
fn placement(at: &NodeAt, slot: &SlotPlace, layer: u8) -> (u16, Placement) {
  let unsnapped = at.exact;
  let (index, clip, cull) = if slot.real() {
    (slot.index, at.clip, meet(at.clip, slot.cull))
  } else {
    (0, at.clip, at.clip)
  };
  let place = Placement {
    rect: at.rect,
    exact: Rect::new(at.rect.x, at.rect.y, unsnapped.width, unsnapped.height),
    clip,
    cull,
    layer,
  };
  (index, place)
}

/// The commands a draw of `kind` on `node` (its text owned in `leaves`)
/// placed at `place` makes, each given to `emit` with the box it is culled
/// by (`None`: never culled), before culling
fn draw_commands(
  kind: DrawKind,
  place: &Placement,
  node: &Node,
  leaves: &[TextLeaf],
  text: &RefCell<&mut TextEngine>,
  scale: f32,
  mut emit: impl FnMut(UiCommand, Option<Rect>),
) {
  if let DrawKind::NineSlice {
    inset,
    sprite,
    insets,
    tint,
    filter,
  } = &kind
  {
    let room = inset_rect(place.rect, *inset);
    let slice = NineSlice {
      sprite,
      insets: *insets,
      tint: *tint,
      filter: *filter,
    };
    nine_slice(room, &slice, place, scale, None, |command, rect| {
      emit(command, Some(rect))
    });
  } else if let Some((command, seen)) = command_of(kind, place, node, leaves, text, scale, None) {
    emit(command, seen);
  }
}

/// The text leaf of the node `id` in one of `kept`'s fragments, if it is a
/// text node
fn kept_leaf(kept: &[(usize, Option<Box<Fragment>>)], id: UiId) -> Option<&TextLeaf> {
  kept
    .iter()
    .filter_map(|(_, fragment)| fragment.as_ref())
    .flat_map(|fragment| fragment.nodes.iter())
    .find(|node| node.id == id && node.text.is_some())?
    .text
    .as_ref()?
    .get(&[])
}

/// The fragment of the kept node `i` (`kept` is in node order)
fn kept_at(kept: &[(usize, Option<Box<Fragment>>)], i: usize) -> Option<&Fragment> {
  let k = kept.binary_search_by_key(&i, |(j, _)| *j).ok()?;
  kept[k].1.as_deref()
}

/// [`kept_at`], to change
fn kept_at_mut(kept: &mut [(usize, Option<Box<Fragment>>)], i: usize) -> Option<&mut Fragment> {
  let k = kept.binary_search_by_key(&i, |(j, _)| *j).ok()?;
  kept[k].1.as_deref_mut()
}

/// Where a fragment is copied: its root the kept node `root` in `slot`, at
/// `s` render pixels per UI unit, in `scene`
#[derive(Clone, Copy)]
struct CopyAt<'n> {
  root: &'n Node,
  slot: SlotPlace,
  s: f32,
  scene: u64,
}

impl CopyAt<'_> {
  /// `kept`'s record as copied here: its rect made absolute by the slot's
  /// offset in a slot (as kept otherwise), its order and depth re-based to
  /// the root's
  fn record(&self, kept: &KeptRecord) -> Record {
    let CopyAt {
      root,
      slot,
      s,
      scene,
    } = *self;
    let (rect, clip) = if slot.real() {
      let [x0, y0, x1, y1] = kept.edges;
      let (dx, dy) = (slot.offset.x, slot.offset.y);
      let rect = rect_of(x0 + dx, y0 + dy, x1 + dx, y1 + dy, s);
      (rect, slot_clip(&slot, kept.clip))
    } else {
      (kept.record.rect, kept.record.clip)
    };
    Record {
      rect,
      clip,
      order: root.order.saturating_add(kept.order),
      depth: root.depth.saturating_add(kept.depth),
      scene,
      ..kept.record
    }
  }
}

/// Write `fragment`'s records as copied at `copy`
fn copy_records(fragment: &Fragment, copy: CopyAt<'_>, memory: &mut UiMemory) {
  for kept in &fragment.records {
    memory.record(kept.id, copy.record(kept));
  }
}

/// Push `fragment`'s commands as its run, its root the kept node `root` in
/// `slot`: each culled by its clip there (met with the slot's interest in a
/// slot), its orders re-based to the root's. A slot's interest other than
/// the one it was last pushed under gives it a new version: it pushes other
/// commands.
fn copy_draws(
  fragment: &mut Fragment,
  root: &Node,
  slot: SlotPlace,
  memory: &mut UiMemory,
  commands: &mut UiCommands,
) {
  let interest = slot.interest_bits();
  if fragment.interest != interest {
    fragment.interest = interest;
    fragment.version = memory.next_fragment_version();
  }
  let index = slot.table_index();
  let start = commands.position();
  for kept in &fragment.draws {
    let cull = slot.fragment_cull(kept.command.clip_rect());
    if !kept.cull.is_some_and(|seen| outside(cull, seen)) {
      let order = root.order.saturating_add(kept.order);
      commands.push(order, index, kept.command.clone());
    }
  }
  commands.push_run(FragmentRun {
    id: root.id,
    version: fragment.version,
    base: root.order,
    layer: root.layer,
    slot: index,
    draws: start..commands.position(),
  });
}

/// The debug spot checks, for `fresh`, just captured at `copy`; `built`
/// when its root was built by the node spot check in place of its replay.
/// `fresh`, and whether a spot check captured it.
///
/// - Built so, where last frame's fragment of the subtree would have been
///   copied: `fresh` keeps that fragment's version when it is the same (by
///   `fragment_difference`) and pushed under the same interest, since it
///   pushed the same commands. The node spot check compares the build.
/// - Replayed and captured in place of the copy of `kept`, which the
///   fragment spot check set aside this frame (`UiMemory::spot_held`):
///   `fresh` is compared with what copying `kept` there would have given,
///   only when `kept` would have been copied (else it would have been
///   resolved again). A difference warns once and is counted. With none,
///   `fresh` keeps `kept`'s version as above.
#[cfg(debug_assertions)]
fn spot_check(
  mut fresh: Fragment,
  copy: CopyAt<'_>,
  built: bool,
  memory: &mut UiMemory,
) -> (Fragment, bool) {
  if built {
    if let Some(kept) = memory.fragments_prev.get(&copy.root.id)
      && copied_as(kept, &fresh)
      && kept.interest == fresh.interest
      && fragment_difference(kept, &fresh, copy).is_none()
    {
      fresh.version = kept.version;
      memory.fragment_stats.spot_kept += 1;
    }
    return (fresh, true);
  }
  let Some(kept) = memory.take_spot_held(copy.root.id) else {
    return (fresh, false);
  };
  if !copied_as(&kept, &fresh) {
    return (fresh, true);
  }
  let stats = &mut memory.fragment_stats;
  stats.spot = Some(copy.root.id);
  let Some((at, was, now)) = fragment_difference(&kept, &fresh, copy) else {
    if kept.interest == fresh.interest {
      fresh.version = kept.version;
      stats.spot_kept += 1;
    }
    return (fresh, true);
  };
  stats.spot_differed += 1;
  let node = kept
    .entities
    .first()
    .map_or_else(|| format!("{:?}", copy.root.id), |root| format!("{root:?}"));
  let (was, now) = super::nodes::around_difference(&was, &now);
  warn_once(format!("ui-fragment:spot:{node}"), || {
    format!(
      "a kept UI fragment differs from its node's resolve: {node} at {at}: kept {was}, \
       resolved {now}. An input its output follows from is not checked; it is drawn as \
       resolved from now on"
    )
  });
  (fresh, true)
}

/// Whether `kept` would have been copied where `fresh` was resolved: its
/// inputs the same and its sprites as they were
#[cfg(debug_assertions)]
fn copied_as(kept: &Fragment, fresh: &Fragment) -> bool {
  kept.inputs == fresh.inputs && kept.sprites_hold()
}

/// Where `kept`, copied at `copy`, first differs from `fresh`, resolved
/// there: what, `kept`'s and `fresh`'s, in `Debug` form
#[cfg(debug_assertions)]
fn fragment_difference(
  kept: &Fragment,
  fresh: &Fragment,
  copy: CopyAt<'_>,
) -> Option<(String, String, String)> {
  if kept.count != fresh.count {
    let count = |f: &Fragment| f.count.to_string();
    return Some(("the order count".to_string(), count(kept), count(fresh)));
  }
  let record = |f: &Fragment, k: usize| {
    f.records.get(k).map_or_else(
      || "nothing".to_string(),
      |r| format!("{:?} {:?}", r.id, copy.record(r)),
    )
  };
  for k in 0..kept.records.len().max(fresh.records.len()) {
    let (was, now) = (record(kept, k), record(fresh, k));
    if was != now {
      return Some((format!("record {k}"), was, now));
    }
  }
  let draw = |f: &Fragment, k: usize| {
    f.draws
      .get(k)
      .map_or_else(|| "nothing".to_string(), |d| format!("{d:?}"))
  };
  for k in 0..kept.draws.len().max(fresh.draws.len()) {
    let (was, now) = (draw(kept, k), draw(fresh, k));
    if was != now {
      return Some((format!("draw {k}"), was, now));
    }
  }
  None
}

/// What resolving a kept fragment again reads
struct Again<'r, 'a> {
  layout: &'r LayoutEngine,
  text: &'r RefCell<&'a mut TextEngine>,
  scale: f32,
  from: Origin,
  scene: u64,
}

impl Again<'_, '_> {
  /// Resolve `fragment` again, its root the kept node `i` laid out at
  /// `laid` and placed in `placing`: its nodes as the layout left them,
  /// placed, recorded and drawn as the subtree's own are, into its records
  /// and draws. False when the layout holds none of it.
  #[allow(clippy::too_many_arguments)]
  fn resolve(
    &self,
    fragment: &mut Fragment,
    i: usize,
    laid: Rect,
    placing: &mut Placing,
    run: &mut Run,
    memory: &UiMemory,
    commands: &mut UiCommands,
  ) -> bool {
    let Fragment {
      nodes,
      node_draws,
      records,
      draws,
      sprites,
      ..
    } = fragment;
    run.ids.clear();
    run.ids.extend(nodes.iter().map(|node| node.id));
    run.parents.clear();
    run.parents.extend(nodes.iter().map(|node| node.parent));
    if !self
      .layout
      .placed_below(laid, &run.ids, &run.parents, &mut run.laid)
    {
      return false;
    }
    // Its root is where its kept node is; the rest are placed from it
    let start = placing.len();
    placing.repeat(i);
    for (k, node) in nodes.iter().enumerate().skip(1) {
      let parent = node
        .parent
        .filter(|&p| p < k)
        .map(|p| (start + p, &nodes[p]));
      placing.place(node, parent, run.laid[k].rect, self.from, memory, commands);
    }
    records.clear();
    for (k, node) in nodes.iter().enumerate() {
      let Some(spec) = node.record else {
        continue;
      };
      let record = placing.record(start + k, node, spec, run.laid[k].content, self.scene);
      records.push(placing.kept_record(start + k, node.id, record, node.order, node.depth));
    }
    draws.clear();
    sprites.clear();
    for draw in node_draws.iter() {
      sprites.extend(sized_by(&draw.kind));
      let j = start + draw.node;
      let (Some(node), Some(at)) = (nodes.get(draw.node), placing.nodes.get(j)) else {
        continue;
      };
      let (_, place) = placement(at, &placing.slots[at.slot], draw.layer);
      let kind = draw.kind.clone();
      draw_commands(
        kind,
        &place,
        node,
        &[],
        self.text,
        self.scale,
        |command, cull| {
          draws.push(KeptDraw {
            order: draw.order,
            command,
            cull,
          });
        },
      );
    }
    placing.truncate(start);
    true
  }
}

/// Marks a node in no capture's span
const NO_SPAN: u32 = u32::MAX;

/// What resolve keeps of fragments between subtrees: buffers cleared at
/// each use
#[derive(Default)]
pub(crate) struct Keeping {
  /// The subtree's kept nodes, in node order, with their fragments (`None`:
  /// missing)
  kept: Vec<(usize, Option<Box<Fragment>>)>,
  /// Each node's subtree size, itself included
  sizes: Vec<usize>,
  /// The capture whose span each node is in (`NO_SPAN`)
  span_of: Vec<u32>,
  /// The captures under way
  captures: Vec<Capturing>,
  /// A fragment being resolved again
  run: Run,
}

/// A kept fragment resolved again: its nodes' ids and parents, and where
/// the layout put them
#[derive(Default)]
struct Run {
  ids: Vec<UiId>,
  parents: Vec<Option<usize>>,
  laid: Vec<Placed>,
}

/// A capture under way: the span of a captured node, and what the records
/// and draws loops keep of it
struct Capturing {
  /// The span: nodes `root..end`
  root: usize,
  end: usize,
  /// The root's order and depth: what is kept is less them
  base: u32,
  depth: u16,
  /// The lowest and highest orders the span's nodes and draws take, and
  /// how many draws it has
  low: u32,
  high: u32,
  drawn: u32,
  records: Vec<KeptRecord>,
  draws: Vec<KeptDraw>,
  node_draws: Vec<Draw>,
  sprites: Vec<(SpriteHandle, u64)>,
  /// Where its commands went in the frame's (`start_runs` starts it), and
  /// how many were pushed
  run: Range<u32>,
  pushed: u32,
}

impl Capturing {
  /// A draw on a node of the span takes `order`
  fn count_draw(&mut self, order: u32) {
    self.low = self.low.min(order);
    self.high = self.high.max(order);
    self.drawn = self.drawn.saturating_add(1);
  }

  /// A draw on a node of the span, kept to resolve the fragment again, and
  /// the sprite it is cut by
  fn keep_draw(&mut self, draw: &Draw) {
    self.sprites.extend(sized_by(&draw.kind));
    self.node_draws.push(Draw {
      order: draw.order.wrapping_sub(self.base),
      layer: draw.layer,
      node: draw.node - self.root,
      kind: draw.kind.clone(),
    });
  }

  /// The orders the span takes (its nodes and draws), when they are the
  /// one run from its root's order a replay of it takes again: else it is
  /// not kept
  fn count(&self) -> Option<u32> {
    let nodes = u32::try_from(self.end - self.root).ok()?;
    let count = nodes.checked_add(self.drawn)?;
    let last = self.base.checked_add(count.checked_sub(1)?)?;
    (self.low == self.base && self.high == last).then_some(count)
  }
}

/// Count the orders `draws` take in each of `captures`' spans (marked in
/// `span_of`), and unmark each span whose nodes and draws do not take one
/// run of orders from its root's: it is not kept
fn count_runs(draws: &[Draw], span_of: &mut [u32], captures: &mut [Capturing]) {
  for draw in draws {
    let c = span_of.get(draw.node).copied().filter(|&c| c != NO_SPAN);
    if let Some(capture) = c.and_then(|c| captures.get_mut(c as usize)) {
      capture.count_draw(draw.order);
    }
  }
  for capture in captures.iter() {
    if capture.count().is_none() {
      span_of[capture.root..capture.end].fill(NO_SPAN);
    }
  }
}

/// Start the run of each capture from `captures[*next]` on whose root's
/// order is at most `order` (they are in node order, so in their roots'
/// orders) at `at`, where the draw of that order goes in the frame's
/// commands
fn start_runs(captures: &mut [Capturing], next: &mut usize, order: u32, at: u32) {
  while let Some(capture) = captures.get_mut(*next).filter(|c| c.base <= order) {
    capture.run = at..at;
    *next += 1;
  }
}

/// Find the spans of `nodes`' captured nodes that can be kept, into
/// `captures`, each node of one marked in `span_of`: a span holds a node
/// placed at a rect (it would not be in its root's slot), a scroll area, a
/// kept node, or a text not shared, is not kept, nor is one whose capture
/// saw a popup or a miss
fn spans(
  nodes: &[Node],
  sizes: &mut Vec<usize>,
  span_of: &mut Vec<u32>,
  captures: &mut Vec<Capturing>,
  logs: &mut dyn FnMut(UiId) -> Option<bool>,
) {
  let count = nodes.len();
  sizes.clear();
  sizes.resize(count, 1);
  for i in (1..count).rev() {
    if let Some(p) = nodes[i].parent.filter(|&p| p < i) {
      sizes[p] += sizes[i];
    }
  }
  span_of.clear();
  span_of.resize(count, NO_SPAN);
  for (i, node) in nodes.iter().enumerate() {
    if node.role != Role::Capture || span_of[i] != NO_SPAN {
      continue;
    }
    if logs(node.id) != Some(true) {
      continue;
    }
    let end = i + sizes[i];
    let span = &nodes[i..end];
    let keepable = span.iter().enumerate().all(|(k, n)| {
      let inside = k == 0 || n.parent.is_some_and(|p| p >= i && p < i + k);
      inside
        && n.at.is_none()
        && n.role != Role::Kept
        && !(n.style.scroll_x || n.style.scroll_y)
        && !matches!(n.text, Some(Leaf::Owned(_)))
    });
    let Ok(c) = u32::try_from(captures.len()) else {
      continue;
    };
    if !keepable || c == NO_SPAN {
      continue;
    }
    let (low, high) = span.iter().fold((u32::MAX, 0), |(low, high), n| {
      (low.min(n.order), high.max(n.order))
    });
    captures.push(Capturing {
      root: i,
      end,
      base: node.order,
      depth: node.depth,
      low,
      high,
      drawn: 0,
      records: Vec::new(),
      draws: Vec::new(),
      node_draws: Vec::new(),
      sprites: Vec::new(),
      run: 0..0,
      pushed: 0,
    });
    span_of[i..end].fill(c);
  }
}

/// One of a subtree's slots as resolve places nodes in it: slot 0 (no
/// scroll area) or a scroll area's
#[derive(Debug, Clone, Copy)]
pub(crate) struct SlotPlace {
  /// Its origin unscrolled, in whole render pixels: its nodes' edges are
  /// rounded from it
  base: Vec2,
  /// Where its origin is drawn, in whole render pixels
  offset: Vec2,
  /// That offset in UI units
  units: Vec2,
  /// The clip its contents draw under (absolute)
  clip: Option<Rect>,
  /// That clip in the slot's own coordinates: what its draws are culled by
  cull: Option<Rect>,
  /// What a kept fragment's draws in it are culled by, in its own
  /// coordinates: the area's own view, uncut by the clips around, grown
  /// along each axis it scrolls ([`interest`])
  interest: Option<Rect>,
  /// Its index in the frame's slot table: 0 for none, `u16::MAX` when the
  /// table was full (its contents are then placed absolute)
  index: u16,
}

impl SlotPlace {
  const NONE: SlotPlace = SlotPlace {
    base: Vec2::ZERO,
    offset: Vec2::ZERO,
    units: Vec2::ZERO,
    clip: None,
    cull: None,
    interest: None,
    index: 0,
  };

  /// Whether its contents' draws are relative to it (a slot in the table)
  fn real(&self) -> bool {
    self.index != 0 && self.index != u16::MAX
  }

  /// The slot its contents' draws carry: its index in the table, or 0
  /// (absolute) when it is not in it
  fn table_index(&self) -> u16 {
    if self.real() { self.index } else { 0 }
  }

  /// What a fragment's draw in it with its own `clip` is culled by: in a
  /// slot of the table, the clip met with the slot's interest; elsewhere
  /// the clip alone, which holds the clips around
  fn fragment_cull(&self, clip: Option<Rect>) -> Option<Rect> {
    if self.real() {
      meet(clip, self.interest)
    } else {
      clip
    }
  }

  /// The interest a fragment's draws in it are culled by, in bits (`None`
  /// outside a slot of the table)
  fn interest_bits(&self) -> Option<[u32; 4]> {
    self.interest.filter(|_| self.real()).map(Inputs::rect)
  }
}

/// The rect a kept fragment's draws in a scroll area's slot are culled by,
/// in the slot's coordinates (UI units): the area's own view there, `view`
/// its size and `drawn` its drawn offset (render pixels, `s` to a UI unit),
/// never cut by the clips around it, so an area an outer one scrolls keeps
/// its interest.
///
/// Along an axis the area `scrolls`, it is three views from a view before
/// the drawn offset taken down to a whole number of half views: it moves
/// only every half view the contents travel, and holds the view with at
/// least half a view to spare each side. Along an axis it does not scroll,
/// it is the view itself grown by a render pixel each side: the slot's
/// clip there is the view cut by the clips around, which float rounding
/// may leave an ulp past the view, and must lie inside it.
fn interest(view: Vec2, drawn: [i32; 2], scrolls: [bool; 2], s: f32) -> Rect {
  let axis = |k: usize| {
    let (size, step) = (view[k], view[k] / 2.0);
    if scrolls[k] && step.is_finite() && step > 0.0 {
      let d = drawn[k] as f32 / s;
      ((d / step).floor() * step - size, 3.0 * size)
    } else {
      let px = 1.0 / s;
      (-px, size + 2.0 * px)
    }
  };
  let ((x, width), (y, height)) = (axis(0), axis(1));
  Rect::new(x, y, width, height)
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
/// (left, top, right, bottom): up to nine sprite commands under the clip
/// and on the layer of `place`, each given to `emit` with its rect (what it
/// is culled by), edges snapped to render pixels so the pieces meet
/// without gaps. Corners shrink together when `room` is smaller than they
/// are.
fn nine_slice(
  room: Rect,
  slice: &NineSlice<'_>,
  place: &Placement,
  scale: f32,
  cull: Option<Rect>,
  mut emit: impl FnMut(UiCommand, Rect),
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
      if drawable(rect) && !outside(cull, rect) {
        let piece = UiCommand::Sprite {
          sprite: sprite.clone(),
          rect,
          uv: [us[column], vs[row], us[column + 1], vs[row + 1]],
          tint,
          filter,
          clip: place.clip,
          layer: place.layer,
        };
        emit(piece, rect);
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

/// Where a draw's node landed, relative to its slot (absolute in slot 0)
struct Placement {
  /// Snapped to render pixels
  rect: Rect,
  /// Its size before snapping (text wraps at this width, so snapping never
  /// wraps it)
  exact: Rect,
  /// The clip its commands carry (in a scroll area's slot, `None` is the
  /// slot's clip alone)
  clip: Option<Rect>,
  /// What its commands are culled by: its clip, with the slot's
  cull: Option<Rect>,
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
/// with the box it is culled by (`None`: never culled), or `None` when it
/// draws nothing or its box lies outside `cull`. Inlined into each draw
/// loop, so a command culled there is never built.
#[inline(always)]
#[allow(clippy::too_many_arguments)]
fn command_of(
  kind: DrawKind,
  place: &Placement,
  node: &Node,
  leaves: &[TextLeaf],
  text: &RefCell<&mut TextEngine>,
  scale: f32,
  cull: Option<Rect>,
) -> Option<(UiCommand, Option<Rect>)> {
  let (clip, layer) = (place.clip, place.layer);
  match kind {
    DrawKind::Fill {
      inset,
      fill,
      radius,
      border,
    } => {
      let rect = inset_rect(place.rect, inset);
      boxed(rect, cull, || UiCommand::Rect {
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
      boxed(rect, cull, || UiCommand::Rect {
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
      text_command(&leaf, at, place, node, text, scale, cull)
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
      let leaf = node.text.as_ref()?.get(leaves)?;
      text_command(leaf, at, place, node, text, scale, cull)
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
      boxed(rect, cull, || UiCommand::Sprite {
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
      boxed(rect, cull, || UiCommand::Rect {
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
    // Drawn from its fragment
    DrawKind::Kept => None,
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
          boxed(rect, cull, || UiCommand::Rect {
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
          boxed(rect, cull, || UiCommand::Sprite {
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
          at,
          width,
          color,
          closed,
          ..
        } => {
          // The points are shared, moved where they are drawn; never culled
          let line = UiCommand::Line {
            points,
            at: at + by,
            width,
            color,
            closed,
            clip,
            layer,
          };
          Some((line, None))
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
          let text = || UiCommand::Text {
            layout,
            origin,
            reveal,
            opacity,
            seed,
            clip,
            layer,
          };
          (origin.is_finite() && !outside(cull, block)).then(|| (text(), Some(block)))
        }
      }
    }
  }
}

/// `leaf` laid out in the content box of `node` (inside its padding),
/// wrapped to its width when `at.wrap`, with its block (what it is culled
/// by), unless the block lies outside `cull`; its effect seed is its key's
fn text_command(
  leaf: &TextLeaf,
  at: TextAt,
  place: &Placement,
  node: &Node,
  text: &RefCell<&mut TextEngine>,
  scale: f32,
  cull: Option<Rect>,
) -> Option<(UiCommand, Option<Rect>)> {
  let padding = clean_edges(node.style.padding);
  let content = inset_rect(place.rect, padding);
  let mut options = leaf.options;
  let mut key = leaf.key;
  if at.wrap {
    let width = (place.exact.width - padding.width()).max(0.0) + WRAP_SLACK;
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
  let command = || UiCommand::Text {
    layout,
    origin,
    reveal: at.reveal,
    opacity: at.opacity,
    seed: leaf.key.seed(),
    clip: place.clip,
    layer: place.layer,
  };
  (origin.is_finite() && !outside(cull, block)).then(|| (command(), Some(block)))
}

/// `command` with its box `rect`, when `rect` can draw and does not lie
/// outside `cull`
#[inline(always)]
fn boxed(
  rect: Rect,
  cull: Option<Rect>,
  command: impl FnOnce() -> UiCommand,
) -> Option<(UiCommand, Option<Rect>)> {
  (drawable(rect) && !outside(cull, rect)).then(|| (command(), Some(rect)))
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
        at: Vec2::ZERO,
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
    // Where the line is drawn: its points moved to its node
    let points = commands.commands().find_map(|c| match c {
      UiCommand::Line { points, at, .. } => Some(points.iter().map(|p| *p + *at).collect()),
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
      at: Vec2::ZERO,
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
