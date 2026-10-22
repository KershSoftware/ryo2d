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
use crate::core::hash::FastMap;
use crate::core::ui::id::IdScopes;
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
  /// the fragment in this slot (`Fragments`), or resolves its kept nodes
  /// again
  Kept(u32),
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
  /// or resolved again) and when its slot's interest moves. Every version
  /// comes from `UiMemory::next_fragment_version` or is copied from a
  /// fragment carried into the frame (the spot check's), so a rerun pass
  /// tells last frame's fragments by `version <= carried`.
  pub(crate) version: u64,
  /// The version it last pushed, and the bounds of the draws it pushed
  /// (`FragmentRun::bounds`)
  pub(crate) bounds: Option<(u64, Option<[f32; 4]>)>,
  /// Its ids are indexed by its slot (`IdScopes::index_kept`): pushing it
  /// makes them at once. Otherwise each is registered when it is pushed.
  pub(crate) indexed: bool,
  /// Its nodes' records are held by its slot (`UiCache::hold_kept`): drawn
  /// in every frame it is pushed in. Set at its first push.
  pub(crate) holding: bool,
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

/// Where the kept fragments live: last frame's, for this frame's resolves
/// to take, and this frame's, kept for the next. Each fragment has a slot
/// of its own for its life, so a frame never moves it between maps: it is
/// taken out to be copied (or resolved again) and put back. The slot also
/// numbers its ids (`IdScopes::index_kept`) and its nodes' hold on their
/// records (`UiCache::hold_kept`).
#[derive(Default)]
pub(crate) struct Fragments {
  slots: Vec<Slot>,
  free: Vec<u32>,
  /// Each root's newest fragment, by slot
  newest: FastMap<UiId, u32>,
}

/// A kept fragment's hold on the records of its nodes: its slot and the
/// slot's generation, so a slot given to another fragment holds none of
/// them (`Fragments::holds`)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Hold {
  slot: u32,
  generation: u32,
}

#[derive(Default)]
struct Slot {
  /// `None` while resolve has it out, or while the slot is free
  fragment: Option<Box<Fragment>>,
  /// Its fragment's root (`NONE`: the slot is free)
  root: UiId,
  /// The memory frame it was last kept for the next in: it is last frame's,
  /// to take, in the frame after
  kept: u64,
  /// The memory frame it was last pushed in
  pushed: u64,
  /// The fragment of the same root kept before it, while that one lives
  /// (last frame's, not taken, when this one was captured)
  older: Option<u32>,
  /// Changes each time the slot is given to a fragment
  generation: u32,
}

impl Fragments {
  /// The slot of `root`'s fragment from the frame before `frame`, to take
  pub(crate) fn last_of(&self, root: UiId, frame: u64) -> Option<u32> {
    let mut at = self.newest.get(&root).copied();
    while let Some(slot) = at {
      let kept = self.slots.get(slot as usize)?;
      if kept.kept.wrapping_add(1) == frame && kept.fragment.is_some() {
        return Some(slot);
      }
      at = kept.older;
    }
    None
  }

  /// The fragment in `slot`, unless it is out
  pub(crate) fn get(&self, slot: u32) -> Option<&Fragment> {
    self.slots.get(slot as usize)?.fragment.as_deref()
  }

  /// The fragment in `slot`, out for resolve to copy or resolve again
  pub(crate) fn take(&mut self, slot: u32) -> Option<Box<Fragment>> {
    self.slots.get_mut(slot as usize)?.fragment.take()
  }

  /// `fragment`, taken from `slot`, back in it, kept for the frame after
  /// `frame`
  pub(crate) fn keep(&mut self, slot: u32, fragment: Box<Fragment>, frame: u64) {
    if let Some(at) = self.slots.get_mut(slot as usize) {
      at.fragment = Some(fragment);
      at.kept = frame;
    }
  }

  /// `fragment`, just captured in `frame`, in a slot of its own, kept for
  /// the next frame and its ids indexed in `scopes`: its root's newest
  pub(crate) fn insert(
    &mut self,
    mut fragment: Box<Fragment>,
    frame: u64,
    scopes: &mut IdScopes,
  ) -> u32 {
    let root = fragment.root.id;
    let slot = self.free.pop().unwrap_or_else(|| {
      self.slots.push(Slot::default());
      u32::try_from(self.slots.len() - 1).unwrap_or(u32::MAX)
    });
    fragment.indexed = scopes.index_kept(slot, root, &fragment.ids);
    fragment.holding = false;
    let older = self.newest.insert(root, slot);
    if let Some(at) = self.slots.get_mut(slot as usize) {
      let generation = at.generation.wrapping_add(1);
      *at = Slot {
        fragment: Some(fragment),
        root,
        kept: frame,
        pushed: 0,
        older,
        generation,
      };
    }
    slot
  }

  /// The fragment in `slot` is pushed in `frame`
  pub(crate) fn pushed(&mut self, slot: u32, frame: u64) {
    if let Some(at) = self.slots.get_mut(slot as usize) {
      at.pushed = frame;
    }
  }

  /// Whether the fragment `hold` names is pushed in `frame`
  pub(crate) fn holds(&self, hold: Hold, frame: u64) -> bool {
    self.slots.get(hold.slot as usize).is_some_and(|at| {
      at.root != UiId::NONE && at.generation == hold.generation && at.pushed == frame
    })
  }

  /// The nodes of the fragment in `slot` whose records it is to hold, and
  /// its hold, at its first push: none after
  pub(crate) fn first_hold(&mut self, slot: u32) -> Option<(&[Entity], Hold)> {
    let at = self.slots.get_mut(slot as usize)?;
    let fragment = at.fragment.as_deref_mut()?;
    if fragment.holding {
      return None;
    }
    fragment.holding = true;
    let hold = Hold {
      slot,
      generation: at.generation,
    };
    Some((&fragment.entities, hold))
  }

  /// Drop the fragment in `slot` (its ids unindexed in `scopes`). When it
  /// was its root's newest, an older one of the root that lives takes its
  /// place, and its ids are indexed again.
  pub(crate) fn drop_slot(&mut self, slot: u32, scopes: &mut IdScopes) {
    let Some(at) = self.slots.get_mut(slot as usize) else {
      return;
    };
    if at.root == UiId::NONE {
      return;
    }
    let (root, older) = (at.root, at.older.take());
    let fragment = at.fragment.take();
    at.root = UiId::NONE;
    at.kept = 0;
    at.pushed = 0;
    scopes.unindex_kept(slot, fragment.as_ref().map(|f| f.ids.as_slice()));
    self.free.push(slot);
    if self.newest.get(&root) == Some(&slot) {
      match older {
        Some(older) => {
          self.newest.insert(root, older);
          let again = self.slots.get_mut(older as usize);
          if let Some(fragment) = again.and_then(|at| at.fragment.as_deref_mut()) {
            fragment.indexed = scopes.index_kept(older, root, &fragment.ids);
          }
        }
        None => {
          self.newest.remove(&root);
        }
      }
      return;
    }
    // Out of the chain of the root's fragments
    let mut at = self.newest.get(&root).copied();
    while let Some(newer) = at {
      let Some(kept) = self.slots.get_mut(newer as usize) else {
        break;
      };
      if kept.older == Some(slot) {
        kept.older = older;
        break;
      }
      at = kept.older;
    }
  }

  /// Drop every fragment in a slot that `drop` picks, given the slot's
  /// fragment (`None`: out) and the frame it was kept in
  fn drop_where(
    &mut self,
    scopes: &mut IdScopes,
    mut drop: impl FnMut(Option<&Fragment>, u64) -> bool,
  ) {
    for slot in 0..self.slots.len() {
      let gone = self
        .slots
        .get(slot)
        .is_some_and(|at| at.root != UiId::NONE && drop(at.fragment.as_deref(), at.kept));
      if gone {
        self.drop_slot(u32::try_from(slot).unwrap_or(u32::MAX), scopes);
      }
    }
  }

  /// A new memory frame, `frame`: a fragment not taken in the last one is
  /// gone
  pub(crate) fn begin_frame(&mut self, frame: u64, scopes: &mut IdScopes) {
    if self.slots.len() == self.free.len() {
      return;
    }
    self.drop_where(scopes, |_, kept| kept.wrapping_add(2) <= frame);
  }

  /// `frame`'s pass is run again: of the fragments it kept, those it only
  /// copied (a version up to `carried`) are last frame's again; those it
  /// captured or resolved again go with it. `latest` is the last version
  /// given out.
  pub(crate) fn restart(&mut self, frame: u64, carried: u64, latest: u64, scopes: &mut IdScopes) {
    self.drop_where(scopes, |fragment, kept| {
      debug_assert!(
        fragment.is_none_or(|fragment| fragment.version <= latest),
        "every fragment version is given out by `next_fragment_version` or carried"
      );
      kept == frame && fragment.is_some_and(|fragment| fragment.version > carried)
    });
    for at in &mut self.slots {
      if at.root != UiId::NONE && at.kept == frame {
        at.kept = frame.wrapping_sub(1);
      }
    }
  }

  /// `frame` never was (a frame that drew nothing): last frame's fragments
  /// it did not take are the frame before the next's, and what it kept goes
  #[cfg(not(target_arch = "wasm32"))]
  pub(crate) fn rewind(&mut self, frame: u64, scopes: &mut IdScopes) {
    self.drop_where(scopes, |_, kept| kept == frame);
    for at in &mut self.slots {
      if at.root != UiId::NONE && at.kept.wrapping_add(1) == frame {
        at.kept = frame;
      }
    }
  }

  /// `scene`'s fragments go
  pub(crate) fn forget_scene(&mut self, scene: u64, scopes: &mut IdScopes) {
    self.drop_where(scopes, |fragment, _| {
      fragment.is_some_and(|f| f.scene == scene)
    });
  }

  /// Every fragment goes; the slots stay, their generations counting on
  pub(crate) fn clear(&mut self, scopes: &mut IdScopes) {
    self.drop_where(scopes, |_, _| true);
    scopes.clear_kept();
  }

  /// The fragments from the frame before `frame`, to take, each with its
  /// root's id
  #[cfg(any(test, debug_assertions))]
  pub(crate) fn of_last_frame(&self, frame: u64) -> impl Iterator<Item = (UiId, &Fragment)> {
    self.slots.iter().filter_map(move |at| {
      let fragment = at.fragment.as_deref()?;
      (at.root != UiId::NONE && at.kept.wrapping_add(1) == frame).then_some((at.root, fragment))
    })
  }

  /// The fragments kept in `frame` for the next, each with its root's id
  #[cfg(test)]
  pub(crate) fn kept_in(&self, frame: u64) -> impl Iterator<Item = (UiId, &Fragment)> {
    self.slots.iter().filter_map(move |at| {
      let fragment = at.fragment.as_deref()?;
      (at.root != UiId::NONE && at.kept == frame).then_some((at.root, fragment))
    })
  }

  /// [`kept_in`](Self::kept_in), to change
  #[cfg(test)]
  pub(crate) fn kept_in_mut(&mut self, frame: u64) -> impl Iterator<Item = &mut Fragment> {
    self.slots.iter_mut().filter_map(move |at| {
      let kept = at.root != UiId::NONE && at.kept == frame;
      at.fragment.as_deref_mut().filter(|_| kept)
    })
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
  /// Kept fragments' commands cloned into the frame's
  #[cfg(test)]
  pub(crate) cloned: usize,
}
