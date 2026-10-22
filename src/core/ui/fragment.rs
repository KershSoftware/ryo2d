//! Fragments: the outermost clean subtrees of node trees, kept as they were
//! resolved and copied into later frames instead of being replayed, laid
//! out and resolved again
//!
//! The walk pushes a clean subtree that has a fragment as one kept node
//! (`UiContext::push_kept`): its ids are made and its states kept as a
//! replay would, and the orders its subtree takes are reserved. Its
//! subtree stays in the layout engine's tree untouched. Resolve then
//! compares the fragment's [`Inputs`] with where its root landed: equal, its
//! records and commands are copied, re-based to this frame's orders, depths
//! and slot; otherwise its kept nodes are resolved again against the
//! layout's current results. A clean subtree with no fragment is replayed
//! and captured (`UiContext::begin_capture`): resolve keeps what it
//! resolved to.

use glam::Vec2;

use crate::core::asset::SpriteHandle;
use crate::core::ecs::kernel::Entity;
use crate::core::ui::immediate::UiCommand;
use crate::core::ui::memory::Record;
use crate::core::ui::tree::{Draw, DrawKind, Node};
use crate::core::ui::{Rect, UiId};

/// How a node takes part in fragments
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Role {
  #[default]
  None,
  /// The root of a clean subtree replayed this frame: what its subtree
  /// resolves to is kept as its fragment
  Capture,
  /// A kept fragment's root, pushed without its subtree: resolve copies
  /// the fragment, or resolves its kept nodes again
  Kept,
}

/// What a fragment's output follows from beyond its own nodes: while these
/// are the same, a copy is what resolving it would give, bit for bit
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Inputs {
  /// The subtree's origin (UI units), in bits
  pub(crate) origin: [u32; 2],
  /// The root's rect as laid out, relative to its subtree's root, in bits
  pub(crate) rect: [u32; 4],
  /// The root's layout shift (stack offsets, never a scroll offset), in bits
  pub(crate) shift: [u32; 2],
  /// Its slot's base (render pixels) and the render scale, in bits
  pub(crate) base: [u32; 2],
  pub(crate) scale: u32,
  /// Its slot: none (0), one in the frame's slot table, its contents
  /// relative to it (1), or one past the table, its contents placed
  /// absolute (2)
  pub(crate) slot: u8,
  /// Its slot's offset (render pixels), in bits, when its slot is not in
  /// the slot table (a full table places a slot's contents absolute); zero
  /// in a slot of the table, whose offset applies when it is copied
  pub(crate) offset: [u32; 2],
  /// Its clip, in bits: absolute outside a slot, relative to its slot in
  /// one (`None` there: the slot's clip alone, which applies when it is
  /// copied)
  pub(crate) clip: Option<[u32; 4]>,
  pub(crate) layer: u8,
}

impl Inputs {
  /// The bits of a vector
  pub(crate) fn vec(v: Vec2) -> [u32; 2] {
    [v.x.to_bits(), v.y.to_bits()]
  }

  /// The bits of a rect
  pub(crate) fn rect(r: Rect) -> [u32; 4] {
    [
      r.x.to_bits(),
      r.y.to_bits(),
      r.width.to_bits(),
      r.height.to_bits(),
    ]
  }
}

/// A record of a fragment's node, its rect relative to the fragment's slot
#[derive(Debug, Clone)]
pub(crate) struct KeptRecord {
  pub(crate) id: UiId,
  /// The record as resolved; its rect, clip, order and depth are made again
  /// when it is copied
  pub(crate) record: Record,
  /// Its edges in render pixels, relative to its slot's base: x0, y0, x1, y1
  pub(crate) edges: [f32; 4],
  /// Its clip as its draws carry it: relative to its slot in one, else
  /// absolute (its record's)
  pub(crate) clip: Option<Rect>,
  /// Its order and depth less the fragment root's
  pub(crate) order: u32,
  pub(crate) depth: u16,
}

/// A command of a fragment, relative to its slot, before culling
#[derive(Debug, Clone)]
pub(crate) struct KeptDraw {
  /// Its order less the fragment root's
  pub(crate) order: u32,
  pub(crate) command: UiCommand,
  /// The box it is culled by (`None`: never culled)
  pub(crate) cull: Option<Rect>,
}

/// An outermost clean subtree as resolved
pub(crate) struct Fragment {
  /// The frame its root's node record was built in: a newer record means
  /// the subtree changed
  pub(crate) built: u64,
  pub(crate) scene: u64,
  pub(crate) inputs: Inputs,
  /// The orders its subtree takes (its nodes and draws)
  pub(crate) count: u32,
  /// Its root as pushed (stamped again when pushed) and the rect it was
  /// pushed at
  pub(crate) root: Node,
  pub(crate) at: Option<Rect>,
  /// The ids made and widget states touched by every tape in it
  pub(crate) ids: Vec<UiId>,
  pub(crate) states: Vec<UiId>,
  /// The nodes replayed in it: their records are drawn while it is kept
  pub(crate) entities: Vec<Entity>,
  pub(crate) records: Vec<KeptRecord>,
  pub(crate) draws: Vec<KeptDraw>,
  /// Its nodes as pushed (orders and depths less the root's, parents as
  /// indices into `nodes`, the root's `None`) and their draws (`node` an
  /// index into `nodes`, orders less the root's): to resolve it again
  pub(crate) nodes: Vec<Node>,
  pub(crate) node_draws: Vec<Draw>,
  /// The sprites its commands were cut by (a nine-slice skin's pieces and
  /// their UVs follow its size), each with the generation it had: a sprite
  /// replaced in place since (a hot reload) changes no other input
  pub(crate) sprites: Vec<(SpriteHandle, u64)>,
  /// The interest rect its draws were last culled by, in bits: its slot's,
  /// in its slot's coordinates (`None` outside a slot, where they are
  /// culled by their clips)
  pub(crate) interest: Option<[u32; 4]>,
  /// Changes each time what it pushes may: when it is resolved (captured,
  /// or resolved again) and when its slot's interest moves
  pub(crate) version: u64,
  /// The version it last pushed, and the bounds of the draws it pushed
  /// (`FragmentRun::bounds`)
  pub(crate) bounds: Option<(u64, Option<[f32; 4]>)>,
}

impl Fragment {
  /// Whether every sprite its commands were cut by is as it was
  pub(crate) fn sprites_hold(&self) -> bool {
    self
      .sprites
      .iter()
      .all(|(sprite, generation)| sprite.generation() == *generation)
  }
}

/// The sprite whose size `kind`'s commands are cut by, with its generation
/// now, if it has one: a nine-slice's
pub(crate) fn sized_by(kind: &DrawKind) -> Option<(SpriteHandle, u64)> {
  match kind {
    DrawKind::NineSlice { sprite, .. } => Some((sprite.clone(), sprite.generation())),
    _ => None,
  }
}

/// What a capture saw while its subtree was replayed
#[derive(Debug, Default)]
pub(crate) struct CaptureLog {
  pub(crate) root: UiId,
  pub(crate) built: u64,
  pub(crate) ids: Vec<UiId>,
  pub(crate) states: Vec<UiId>,
  /// The nodes replayed in it
  pub(crate) entities: Vec<Entity>,
  /// A popup was begun in it, or it began in one: it is never a fragment
  pub(crate) popup: bool,
  /// A child in it was drawn from a record that was not clean (a miss): it
  /// is never a fragment
  pub(crate) missed: bool,
  /// Its root has been pushed (and marked `Role::Capture`)
  pub(crate) rooted: bool,
  /// Its root was built by the debug spot check in place of its replay or
  /// its kept fragment's copy
  pub(crate) spot: bool,
}

impl CaptureLog {
  /// Empty, for the subtree rooted at `root` (record built in frame
  /// `built`), its buffers kept
  pub(crate) fn reset(&mut self, root: UiId, built: u64) {
    self.root = root;
    self.built = built;
    self.ids.clear();
    self.states.clear();
    self.entities.clear();
    self.popup = false;
    self.missed = false;
    self.rooted = false;
    self.spot = false;
  }
}

/// What resolve did with fragments in a UI pass
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct FragmentStats {
  /// Clean subtrees resolved and kept
  pub(crate) captured: usize,
  /// Kept fragments copied
  pub(crate) reused: usize,
  /// Kept fragments resolved again from what they kept
  pub(crate) resolved_again: usize,
  /// The kept fragment the debug spot check resolved in place of its copy
  /// and compared with what it kept, by its root's id (not counted above)
  pub(crate) spot: Option<UiId>,
  /// Kept fragments the spot check found differing from their resolve:
  /// never so while a fragment checks every input its output follows from
  pub(crate) spot_differed: usize,
  /// Kept fragments the spot checks captured afresh in place of their copy
  /// (the fragment spot check's, or the subtree of the node it builds) and
  /// found the same: they keep their versions
  pub(crate) spot_kept: usize,
}
