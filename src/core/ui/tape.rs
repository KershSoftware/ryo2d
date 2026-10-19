//! Tapes: what a UI builder pushed, recorded while it ran, to replay later
//!
//! While a tape records, the context notes what the builder pushes into the
//! open subtree (nodes as given, draws, closes, popup brackets), where each
//! child's output goes, the ids it made and the widget states it touched. A
//! replay pushes the same again through `push_node`, `push_draw` and
//! `close`, so order, layer, focus scope, parent, depth and scroll are
//! stamped from the context it is replayed into, as a build's would be.
//!
//! Tapes nest through child slots: `begin_child` notes the slot in the open
//! tape and pauses it, and what the child pushes, makes and touches belongs
//! to the child (its own tape, or its replay) until `end_child`. A build
//! that does something a replay would not repeat (asks for the focus,
//! consumes input, plays a sound, draws outside its subtree) taints its tape
//! and every tape around it: they are recorded but never replayed. What a
//! build read from outside its own node (another node's id, the rect it
//! placed by, the size of a sprite) is noted as a [`Read`] in its tape and
//! every tape around it, and a replay inside a recording tape adds the
//! replayed reads to it: a tape replays only while every read of its
//! subtree still holds, so a clean parent never holds a child that is not.

use crate::core::asset::SpriteHandle;
use crate::core::ecs::kernel::Entity;
use crate::core::ui::immediate::UiContext;
use crate::core::ui::tree::{DrawKind, Node};
use crate::core::ui::{Rect, UiId};

/// What one builder call pushed, to push again in a later frame. A tape
/// recorded again into the same value reuses its buffers.
#[derive(Clone, Default)]
pub(crate) struct Tape {
  /// What it pushed, in order
  pub(crate) ops: Vec<Op>,
  /// The nodes it pushed, as given to `push_node` before they were stamped:
  /// `Op::Node { node: k, .. }` is the k-th
  pub(crate) nodes: Vec<Node>,
  /// The ids its own builder made, in order (not its children's)
  pub(crate) ids: Vec<UiId>,
  /// The widget states it touched, in order
  pub(crate) states: Vec<UiId>,
  /// Its build did nothing a replay would not repeat
  /// ([`UiContext::taint`])
  pub(crate) replayable: bool,
  /// What its build read from outside its own node, in order
  pub(crate) reads: Vec<Read>,
}

impl Tape {
  /// Empty and not replayable, its buffers kept
  pub(crate) fn clear(&mut self) {
    self.ops.clear();
    self.nodes.clear();
    self.ids.clear();
    self.states.clear();
    self.reads.clear();
    self.replayable = false;
  }

  /// Where a replay of `self` and one of `other` would first differ, op by
  /// op in `Debug` form (a node as it was given), then the ids made, the
  /// states touched and whether each replays: what differs, `self`'s and
  /// `other`'s. What each read is not compared: a replay pushes none of it.
  #[cfg(debug_assertions)]
  pub(crate) fn first_difference(&self, other: &Tape) -> Option<(String, String, String)> {
    for k in 0..self.ops.len().max(other.ops.len()) {
      let (was, now) = (self.op_debug(k), other.op_debug(k));
      if was != now {
        return Some((format!("op {k}"), was, now));
      }
    }
    let parts = [
      (
        "the ids made",
        format!("{:?}", self.ids),
        format!("{:?}", other.ids),
      ),
      (
        "the states touched",
        format!("{:?}", self.states),
        format!("{:?}", other.states),
      ),
      (
        "replayable",
        self.replayable.to_string(),
        other.replayable.to_string(),
      ),
    ];
    parts
      .into_iter()
      .find(|(_, was, now)| was != now)
      .map(|(what, was, now)| (what.to_string(), was, now))
  }

  /// Op `k` in `Debug` form, a node op with the node it pushed
  #[cfg(debug_assertions)]
  fn op_debug(&self, k: usize) -> String {
    match self.ops.get(k) {
      None => "nothing".to_string(),
      Some(Op::Node { node, at }) => {
        let given = usize::try_from(*node).ok().and_then(|i| self.nodes.get(i));
        format!("Node {node} {given:?} at {at:?}")
      }
      Some(op) => format!("{op:?}"),
    }
  }
}

/// Something a build read from outside its own node, baked into what it
/// pushed: a replay is right only while it is still as read
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Read {
  /// Node `node`'s response id, as its `UiInteraction` held it (`NONE`
  /// before it was drawn): a `nav_*` neighbour, a popup's anchor
  Id { node: Entity, id: UiId },
  /// Where widget `id` was (`UiContext::anchor_rect`; `None`: nowhere) when
  /// the build placed something by it: an open dropdown's list is as tall
  /// as the room beside the dropdown itself
  Rect { id: UiId, rect: Option<Rect> },
  /// The version of `sprite` whose size the build sized something by: an
  /// image without a size, a prompt's glyph. A hot reload replaces a
  /// sprite in place, perhaps at another size, and no component changes.
  Sprite {
    sprite: SpriteHandle,
    generation: u64,
  },
}

/// One thing a builder pushed
#[derive(Debug, Clone)]
pub(crate) enum Op {
  /// The k-th node the tape pushed (`Tape::nodes`), at `at`
  Node { node: u32, at: Option<Rect> },
  /// The close of the k-th node the tape pushed
  Close(u32),
  /// A draw on the k-th node the tape pushed, as given to `push_draw`
  Draw { node: u32, kind: DrawKind },
  /// A popup begun below widget `anchor`, whose rect a replay looks up
  /// again (`NONE`: no anchor)
  PopupBegin { anchor: UiId, modal: bool },
  /// The popup begun last ended
  PopupEnd,
  /// Where a child's output goes
  Child(Entity),
}

/// Where a node a tape pushed went
#[derive(Clone, Copy)]
pub(crate) struct Place {
  /// The popups being recorded around it (`subtree_stack`'s length)
  depth: usize,
  /// Its index in that subtree
  index: usize,
  id: UiId,
}

/// A tape being recorded
pub(crate) struct Recorder {
  tape: Tape,
  /// Where each node it pushed went, by its place in the tape: from here on
  /// in the memory's `Scratch::places`, which nested tapes share
  places: usize,
  /// Child slots (and replays) under way: nothing is recorded while above 0
  paused: u32,
  /// Replays under way while it was the open tape: a replay inside another
  /// adds no reads, the outer one's hold its
  replaying: u32,
  /// Where its ids and touches start in the memory's logs
  ids: usize,
  states: usize,
}

impl UiContext<'_> {
  /// Start recording a tape of what is pushed from here. Inside another
  /// tape, call it in a child slot ([`begin_child`](Self::begin_child)):
  /// the tape around it would miss what this one takes, so it is tainted.
  #[cfg(test)]
  pub(crate) fn start_tape(&mut self) {
    self.start_tape_in(Tape::default());
  }

  /// [`start_tape`](Self::start_tape) into `tape`, cleared: a tape recorded
  /// again reuses the buffers of the one it replaces
  pub(crate) fn start_tape_in(&mut self, mut tape: Tape) {
    if self.recording() {
      self.taint();
    }
    tape.clear();
    tape.replayable = true;
    let places = &mut self.memory.scratch.places;
    if self.tapes.is_empty() {
      places.clear();
    }
    let recorder = Recorder {
      tape,
      places: places.len(),
      paused: 0,
      replaying: 0,
      ids: self.memory.scopes.log.mark(),
      states: self.memory.touched.mark(),
    };
    self.tapes.push(recorder);
    self.sync_logs();
  }

  /// Stop recording the tape started last: what it recorded (an empty
  /// tape, not replayable, when none was started)
  pub(crate) fn end_tape(&mut self) -> Tape {
    let Some(recorder) = self.tapes.pop() else {
      return Tape::default();
    };
    let mut tape = recorder.tape;
    self
      .memory
      .scopes
      .log
      .take_into(recorder.ids, &mut tape.ids);
    self
      .memory
      .touched
      .take_into(recorder.states, &mut tape.states);
    self.memory.scratch.places.truncate(recorder.places);
    self.sync_logs();
    tape
  }

  /// Where `child`'s output goes: noted in the open tape, which records
  /// nothing more until [`end_child`](Self::end_child). What the child
  /// pushes, makes and touches is its own: its tape, or its replay.
  pub(crate) fn begin_child(&mut self, child: Entity) {
    self.tape_op(Op::Child(child));
    self.pause();
  }

  /// The child slot begun last is filled: the open tape records again
  pub(crate) fn end_child(&mut self) {
    self.resume();
  }

  /// The build being recorded read `read` from outside its own node: noted
  /// in the open tape and every tape around it, since a replay of any of
  /// them would push what it made of it
  pub(crate) fn note_read(&mut self, read: Read) {
    for recorder in &mut self.tapes {
      recorder.tape.reads.push(read.clone());
    }
  }

  /// The build being recorded sized something by `sprite`: noted as a read
  /// of its version ([`Read::Sprite`])
  pub(crate) fn note_sprite(&mut self, sprite: &SpriteHandle) {
    if self.tapes.is_empty() {
      return;
    }
    self.note_read(Read::Sprite {
      sprite: sprite.clone(),
      generation: sprite.generation(),
    });
  }

  /// The build being recorded did something a replay would not repeat:
  /// neither the open tape nor any tape around it is replayable
  pub(crate) fn taint(&mut self) {
    for recorder in &mut self.tapes {
      recorder.tape.replayable = false;
    }
  }

  /// Push again what `tape` recorded, stamped from the context here as a
  /// build would be: `child` fills each child slot. Its ids are made and
  /// its widget states kept this frame. An open tape records none of it
  /// (it holds the child slot this is drawn in, if any) but its reads.
  ///
  /// Called while a tape records outside a child slot, that tape is
  /// tainted, as [`start_tape_in`](Self::start_tape_in) taints it. A
  /// replay is a build's equal only where the build was made:
  /// - in the same id scope, since the ids it made are kept, not made
  ///   again;
  /// - with no own id made after a child slot it holds, and none unkeyed
  ///   made in the scope it is replayed into: a replay registers its ids
  ///   but does not advance the per-site counts an unkeyed id is made from,
  ///   so an unkeyed build after it at the same site would make the id the
  ///   replay holds. Node trees key every node by its entity, so neither
  ///   arises there.
  pub(crate) fn replay(&mut self, tape: &Tape, child: &mut dyn FnMut(&mut UiContext<'_>, Entity)) {
    // The tape recording here would miss what the replay pushes
    if self.recording() {
      self.taint();
    }
    // The tapes around carry what it read, as they would had it been built
    if self
      .tapes
      .last()
      .is_some_and(|recorder| recorder.replaying == 0)
    {
      for recorder in &mut self.tapes {
        recorder.tape.reads.extend_from_slice(&tape.reads);
      }
    }
    if let Some(recorder) = self.tapes.last_mut() {
      recorder.replaying += 1;
    }
    self.pause();
    // Made before the nodes, as a build makes a node's ids before its
    // children's, so a capture sees them in a build's order
    for &id in &tape.ids {
      self.memory.scopes.replay_id(id);
    }
    let (scene, _) = self.scene();
    for &id in &tape.states {
      self.memory.touch(id, scene);
    }
    let base = self.memory.scratch.replayed.len();
    for op in &tape.ops {
      match op {
        Op::Node { node, at } => {
          let Some(node) = usize::try_from(*node).ok().and_then(|k| tape.nodes.get(k)) else {
            continue;
          };
          let index = self.push_node(node.clone(), *at);
          self.memory.scratch.replayed.push(index);
        }
        Op::Draw { node, kind } => {
          if let Some(index) = self.replayed(base, *node) {
            self.push_draw(index, kind.clone());
          }
        }
        Op::Close(node) => {
          if let Some(index) = self.replayed(base, *node) {
            self.close(index);
          }
        }
        Op::PopupBegin { anchor, modal } => self.begin_popup(*anchor, *modal),
        Op::PopupEnd => self.end_popup(),
        Op::Child(entity) => child(self, *entity),
      }
    }
    self.memory.scratch.replayed.truncate(base);
    self.resume();
    if let Some(recorder) = self.tapes.last_mut() {
      recorder.replaying = recorder.replaying.saturating_sub(1);
    }
  }

  /// The subtree index the replay from `base` gave its k-th node
  fn replayed(&self, base: usize, k: u32) -> Option<usize> {
    let k = usize::try_from(k).ok()?;
    self.memory.scratch.replayed.get(base + k).copied()
  }

  /// Whether a tape is recording: one is open and not paused
  pub(crate) fn recording(&self) -> bool {
    self
      .tapes
      .last()
      .is_some_and(|recorder| recorder.paused == 0)
  }

  /// The open tape records nothing until [`resume`](Self::resume)
  fn pause(&mut self) {
    if let Some(recorder) = self.tapes.last_mut() {
      recorder.paused += 1;
    }
    self.sync_logs();
  }

  fn resume(&mut self) {
    if let Some(recorder) = self.tapes.last_mut() {
      recorder.paused = recorder.paused.saturating_sub(1);
    }
    self.sync_logs();
  }

  /// The id and state logs keep what is made and touched while a tape
  /// records, and nothing otherwise
  fn sync_logs(&mut self) {
    let on = self.recording();
    self.memory.scopes.log.on = on;
    self.memory.touched.on = on;
  }

  /// `op` into the recording tape, if one is
  pub(crate) fn tape_op(&mut self, op: Op) {
    if !self.recording() {
      return;
    }
    if let Some(recorder) = self.tapes.last_mut() {
      recorder.tape.ops.push(op);
    }
  }

  /// `node` (as given to `push_node`, at `at`) into the recording tape: it
  /// went to `index` in the open subtree
  pub(crate) fn tape_node(&mut self, node: Node, at: Option<Rect>, index: usize) {
    let depth = self.subtree_stack.len();
    let Some(recorder) = self.tapes.last_mut() else {
      return;
    };
    let tape = &mut recorder.tape;
    let Ok(k) = u32::try_from(tape.nodes.len()) else {
      tape.replayable = false;
      return;
    };
    self.memory.scratch.places.push(Place {
      depth,
      index,
      id: node.id,
    });
    tape.nodes.push(node);
    tape.ops.push(Op::Node { node: k, at });
  }

  /// A draw of `kind` on `node` of the open subtree into the recording
  /// tape; on a node it did not push, the tape is tainted
  pub(crate) fn tape_draw(&mut self, node: usize, kind: &DrawKind) {
    if !self.recording() {
      return;
    }
    match self.tape_place(node) {
      Some(k) => self.tape_op(Op::Draw {
        node: k,
        kind: kind.clone(),
      }),
      None => self.taint(),
    }
  }

  /// The close of `node` of the open subtree into the recording tape; of a
  /// node it did not push, the tape is tainted
  pub(crate) fn tape_close(&mut self, node: usize) {
    if !self.recording() {
      return;
    }
    match self.tape_place(node) {
      Some(k) => self.tape_op(Op::Close(k)),
      None => self.taint(),
    }
  }

  /// Which node of the recording tape `node` of the open subtree is: the
  /// last it pushed there, if that is still the node there
  fn tape_place(&self, node: usize) -> Option<u32> {
    let id = self.subtree_node_id(node)?;
    let depth = self.subtree_stack.len();
    let base = self.tapes.last()?.places;
    let places = self.memory.scratch.places.get(base..)?;
    let k = places
      .iter()
      .rposition(|place| place.depth == depth && place.index == node)?;
    let place = places.get(k)?;
    if place.id != id {
      return None;
    }
    u32::try_from(k).ok()
  }
}

#[cfg(all(test, feature = "default-fonts"))]
mod tests {
  use super::*;
  use crate::core::testing::UiHarness;
  use crate::core::ui::layout::UiStyle;
  use crate::core::ui::memory::Flags;
  use crate::core::ui::tree::Leaf;
  use crate::core::ui::{Color, Edges};
  use crate::prelude::*;
  use std::sync::Arc;

  /// A panel of a title, a button and a row of two buttons: the ids of the
  /// panel, the Play button and the A button
  fn panel(ui: &mut UiContext<'_>) -> [UiId; 3] {
    let (mut play, mut a) = (UiId::NONE, UiId::NONE);
    let panel = ui.column().id("panel").gap(4.0).padding(8.0).show(|ui| {
      ui.text("Title");
      play = ui.button("Play").id("play").show().id;
      ui.row().id("pair").gap(2.0).show(|ui| {
        a = ui.button("A").id("a").show().id;
        ui.button("B").id("b").show();
      });
    });
    [panel.id, play, a]
  }

  /// The frame's draw commands and the records of `ids`, to compare two frames
  fn snapshot(h: &UiHarness, ids: &[UiId]) -> String {
    let records: Vec<String> = ids
      .iter()
      .map(|id| format!("{:?}", h.record_of(*id)))
      .collect();
    format!("{:?}\n{:?}", h.commands_debug(), records)
  }

  #[test]
  fn a_replayed_subtree_draws_what_a_build_draws() {
    let mut built = UiHarness::new(640.0, 480.0);
    let mut tape = Tape::default();
    built.frame(|ui| {
      ui.start_tape();
      panel(ui);
      tape = ui.end_tape();
    });
    built.frame(panel);
    let ids = built.frame(panel);

    let mut replayed = UiHarness::new(640.0, 480.0);
    replayed.frame(panel);
    replayed.frame(panel);
    replayed.frame(|ui| ui.replay(&tape, &mut |_, _| {}));
    assert!(tape.replayable);
    assert_eq!(snapshot(&replayed, &ids), snapshot(&built, &ids));
  }

  #[test]
  fn a_replay_takes_the_orders_a_build_would() {
    let mut h = UiHarness::new(640.0, 480.0);
    let mut tape = Tape::default();
    let before = h.frame(|ui| {
      ui.start_tape();
      panel(ui);
      tape = ui.end_tape();
      ui.next_order()
    });
    let after = h.frame(|ui| {
      ui.replay(&tape, &mut |_, _| {});
      ui.next_order()
    });
    assert_eq!(after, before);
  }

  #[test]
  fn a_replay_keeps_the_state_it_touched_and_registers_its_ids() {
    let mut h = UiHarness::new(640.0, 480.0);
    let mut tape = Tape::default();
    let scroll = h.frame(|ui| {
      ui.start_tape();
      let r = ui.scroll().id("s").height(100.0).show(|ui| {
        for i in 0..20 {
          ui.button(format!("{i}")).id(i).show();
        }
      });
      tape = ui.end_tape();
      r.id
    });
    h.frame(|ui| ui.set_scroll_offset(scroll, Vec2::new(0.0, 40.0)));
    for _ in 0..3 {
      h.frame(|ui| {
        ui.replay(&tape, &mut |_, _| {});
        assert!(
          ui.memory.scopes.made(scroll),
          "the replayed ids are registered"
        );
      });
    }
    assert!(
      (h.scroll_offset(scroll).y - 40.0).abs() < 0.5,
      "the scroll state survived the replays"
    );
  }

  /// A scroll area of 20 rows, 100 tall: its id and its first row's
  fn rows(ui: &mut UiContext<'_>) -> [UiId; 2] {
    let mut first = UiId::NONE;
    let area = ui.scroll().id("s").height(100.0).show(|ui| {
      for i in 0..20 {
        let row = ui.button(format!("Row {i}")).id(i).show();
        if i == 0 {
          first = row.id;
        }
      }
    });
    [area.id, first]
  }

  #[test]
  fn a_replayed_scroll_area_draws_its_bar_after_its_rows() {
    // Taped once the area knows it overflows, so its bar is drawn
    let mut built = UiHarness::new(640.0, 480.0);
    built.move_to(630.0, 470.0);
    built.frame(rows);
    let mut tape = Tape::default();
    built.frame(|ui| {
      ui.start_tape();
      rows(ui);
      tape = ui.end_tape();
    });
    let ids = built.frame(rows);

    // The area's own draws (k = 0) after its rows' nodes: the bar
    let last_row = tape
      .ops
      .iter()
      .rposition(|op| matches!(op, Op::Node { .. }))
      .expect("rows");
    let bar = tape
      .ops
      .iter()
      .rposition(|op| matches!(op, Op::Draw { node: 0, .. }))
      .expect("a bar draw");
    assert!(bar > last_row, "the bar is drawn after the rows");

    let mut replayed = UiHarness::new(640.0, 480.0);
    replayed.move_to(630.0, 470.0);
    replayed.frame(rows);
    replayed.frame(rows);
    replayed.frame(|ui| ui.replay(&tape, &mut |_, _| {}));
    assert!(tape.replayable);
    assert_eq!(snapshot(&replayed, &ids), snapshot(&built, &ids));
  }

  #[test]
  fn a_child_slot_is_filled_by_its_own_output() {
    let mut h = UiHarness::new(640.0, 480.0);
    let (mut outer, mut inner) = (Tape::default(), Tape::default());
    let child = Entity::from_raw_u32(7).unwrap();
    h.frame(|ui| {
      ui.start_tape();
      ui.column().id("outer").show(|ui| {
        ui.begin_child(child);
        ui.start_tape();
        ui.button("Inner").id("inner").show();
        inner = ui.end_tape();
        ui.end_child();
      });
      outer = ui.end_tape();
    });
    assert!(
      outer
        .ops
        .iter()
        .any(|op| matches!(op, Op::Child(e) if *e == child))
    );
    assert!(
      !outer.nodes.iter().any(|node| node.id == inner_id(&inner)),
      "the parent's tape does not hold its child's nodes"
    );
    let count = h.frame(|ui| {
      ui.replay(&outer, &mut |ui, _| ui.replay(&inner, &mut |_, _| {}));
      ui.next_order()
    });
    assert!(count > 0);
  }

  fn inner_id(tape: &Tape) -> UiId {
    tape.nodes.first().map_or(UiId::NONE, |node| node.id)
  }

  /// Three tapes nested two deep: a column holding a child (a column with
  /// a button of its own) that holds a child (a button). The tapes, and
  /// the ids of the outer column, the middle button and the inner button.
  fn nested(ui: &mut UiContext<'_>) -> ([Tape; 3], [UiId; 3]) {
    let (c1, c2) = (
      Entity::from_raw_u32(1).unwrap(),
      Entity::from_raw_u32(2).unwrap(),
    );
    let (mut mid, mut inner) = (Tape::default(), Tape::default());
    let (mut own, mut deep) = (UiId::NONE, UiId::NONE);
    ui.start_tape();
    let outer = ui.column().id("outer").gap(3.0).show(|ui| {
      ui.begin_child(c1);
      ui.start_tape();
      ui.column().id("mid").padding(2.0).show(|ui| {
        ui.begin_child(c2);
        ui.start_tape();
        deep = ui.button("Inner").id("inner").show().id;
        inner = ui.end_tape();
        ui.end_child();
        own = ui.button("Mid").id("own").show().id;
      });
      mid = ui.end_tape();
      ui.end_child();
      ui.text("After");
    });
    let outer_tape = ui.end_tape();
    ([outer_tape, mid, inner], [outer.id, own, deep])
  }

  /// Replay `tapes` (from `nested`) into their slots
  fn replay_nested(ui: &mut UiContext<'_>, tapes: &[Tape; 3]) {
    let [outer, mid, inner] = tapes;
    ui.replay(outer, &mut |ui, _| {
      ui.replay(mid, &mut |ui, _| ui.replay(inner, &mut |_, _| {}));
    });
  }

  #[test]
  fn a_tape_taken_inside_a_paused_one_holds_only_its_own_output() {
    let mut built = UiHarness::new(640.0, 480.0);
    let (tapes, _) = built.frame(nested);
    built.frame(nested);
    let (_, ids) = built.frame(nested);
    let [outer, mid, inner] = &tapes;
    let [_, own, deep] = ids;
    let nodes = |tape: &Tape| -> Vec<UiId> { tape.nodes.iter().map(|node| node.id).collect() };
    let children = |tape: &Tape| {
      tape
        .ops
        .iter()
        .filter(|op| matches!(op, Op::Child(_)))
        .count()
    };
    assert_eq!((children(outer), children(mid), children(inner)), (1, 1, 0));
    assert_eq!(nodes(outer).len(), 2, "the outer column and its text");
    assert!(nodes(mid).contains(&own) && !nodes(mid).contains(&deep));
    assert_eq!(nodes(inner), [deep]);
    assert!(mid.ids.contains(&own) && !mid.ids.contains(&deep));
    assert!(!outer.ids.contains(&own) && inner.ids.contains(&deep));
    assert!(outer.replayable && mid.replayable && inner.replayable);

    let mut replayed = UiHarness::new(640.0, 480.0);
    replayed.frame(nested);
    replayed.frame(nested);
    replayed.frame(|ui| {
      replay_nested(ui, &tapes);
      for id in ids {
        assert!(ui.memory.scopes.made(id), "every tape's ids are registered");
      }
    });
    assert_eq!(snapshot(&replayed, &ids), snapshot(&built, &ids));
  }

  #[test]
  fn a_replay_in_a_recording_parents_child_slot_leaves_only_the_slot() {
    let mut h = UiHarness::new(640.0, 480.0);
    let child = Entity::from_raw_u32(9).unwrap();
    // A row with one child slot, filled by `fill`: the row's tape
    let parent = |ui: &mut UiContext<'_>, fill: &mut dyn FnMut(&mut UiContext<'_>)| {
      ui.start_tape();
      ui.row().id("parent").gap(2.0).show(|ui| {
        ui.begin_child(child);
        fill(ui);
        ui.end_child();
      });
      ui.end_tape()
    };
    let (mut inner, mut inner_id) = (Tape::default(), UiId::NONE);
    h.frame(|ui| {
      parent(ui, &mut |ui| {
        ui.start_tape();
        inner_id = ui.button("Inner").id("inner").show().id;
        inner = ui.end_tape();
      })
    });
    assert!(inner.replayable && inner.ids.contains(&inner_id));
    let outer = h.frame(|ui| parent(ui, &mut |ui| ui.replay(&inner, &mut |_, _| {})));
    assert!(outer.replayable, "a replay in a child slot taints nothing");
    let slots: Vec<&Op> = outer
      .ops
      .iter()
      .filter(|op| matches!(op, Op::Child(_)))
      .collect();
    assert!(matches!(slots.as_slice(), [Op::Child(e)] if *e == child));
    assert_eq!(outer.nodes.len(), 1, "only the parent's own row");
    assert!(
      !outer.nodes.iter().any(|node| node.id == inner_id),
      "none of the child's nodes"
    );
    assert!(!outer.ids.contains(&inner_id), "none of the child's ids");
    assert!(
      outer
        .ops
        .iter()
        .all(|op| !matches!(op, Op::Draw { node, .. } if *node > 0)),
      "no draw on a node of the child"
    );
  }

  #[test]
  fn a_replay_outside_a_child_slot_taints_the_recording_tape() {
    let mut h = UiHarness::new(640.0, 480.0);
    let inner = h.frame(|ui| {
      ui.start_tape();
      ui.button("Inner").id("inner").show();
      ui.end_tape()
    });
    let outer = h.frame(|ui| {
      ui.start_tape();
      ui.column()
        .id("c")
        .show(|ui| ui.replay(&inner, &mut |_, _| {}));
      ui.end_tape()
    });
    assert!(!outer.replayable, "it would miss what the replay pushed");
  }

  #[test]
  fn a_build_with_a_side_effect_is_not_replayable() {
    let mut h = UiHarness::new(640.0, 480.0);
    let mut tape = Tape::default();
    h.frame(|ui| {
      ui.start_tape();
      ui.column().id("c").show(|ui| {
        let id = ui.button("x").id("x").show().id;
        ui.request_focus(id);
      });
      tape = ui.end_tape();
    });
    assert!(!tape.replayable);
  }

  #[test]
  fn a_side_effect_in_a_child_taints_the_tapes_below_it() {
    let mut h = UiHarness::new(640.0, 480.0);
    let (mut outer, mut inner, mut sibling) = (Tape::default(), Tape::default(), Tape::default());
    let child = Entity::from_raw_u32(3).unwrap();
    h.frame(|ui| {
      ui.start_tape();
      ui.column().id("c").show(|ui| {
        ui.begin_child(child);
        ui.start_tape();
        ui.draw_rect(Rect::new(0.0, 0.0, 4.0, 4.0), Color::WHITE);
        inner = ui.end_tape();
        ui.end_child();
        ui.begin_child(child);
        ui.start_tape();
        ui.button("Fine").show();
        sibling = ui.end_tape();
        ui.end_child();
      });
      outer = ui.end_tape();
    });
    assert!(!inner.replayable, "an absolute draw");
    assert!(!outer.replayable, "the tape below it");
    assert!(sibling.replayable, "a later sibling is its own");
  }

  #[test]
  fn a_draw_on_or_a_close_of_a_node_the_tape_did_not_push_taints_it() {
    let mut h = UiHarness::new(640.0, 480.0);
    let (drawn, closed) = h.frame(|ui| {
      let outer = ui.open_node(UiId(11), UiStyle::default(), None, Flags::default());
      ui.start_tape();
      let fill = DrawKind::Fill {
        inset: Edges::ZERO,
        fill: Color::WHITE,
        radius: 0.0,
        border: None,
      };
      ui.push_draw(outer, fill);
      let drawn = ui.end_tape();
      ui.start_tape();
      ui.close_node(outer);
      (drawn, ui.end_tape())
    });
    assert!(!drawn.replayable && !closed.replayable);
  }

  #[test]
  fn a_popup_replays_into_the_popup_queue() {
    let mut built = UiHarness::new(640.0, 480.0);
    let mut tape = Tape::default();
    let menu = |ui: &mut UiContext<'_>| {
      let mut ok = UiId::NONE;
      ui.modal().id("m").show(|ui| {
        ok = ui.button("Ok").id("ok").show().id;
      });
      ok
    };
    built.frame(|ui| {
      ui.start_tape();
      menu(ui);
      tape = ui.end_tape();
    });
    let ok = built.frame(menu);
    let mut replayed = UiHarness::new(640.0, 480.0);
    replayed.frame(menu);
    replayed.frame(|ui| ui.replay(&tape, &mut |_, _| {}));
    assert!(tape.replayable);
    assert_eq!(snapshot(&replayed, &[ok]), snapshot(&built, &[ok]));
  }

  /// How `anchored` draws its popup
  enum Menu<'t> {
    Build,
    Tape(&'t mut Tape),
    Replay(&'t Tape),
  }

  /// A button at `x` with a popup below it, built, taped or replayed: the
  /// popup's id (`NONE` when replayed)
  fn anchored(ui: &mut UiContext<'_>, x: f32, menu: Menu<'_>) -> UiId {
    let more = ui
      .button("More")
      .id("more")
      .at(Rect::new(x, 20.0, 80.0, 24.0))
      .show();
    let tape = match menu {
      Menu::Replay(tape) => {
        ui.replay(tape, &mut |_, _| {});
        return UiId::NONE;
      }
      Menu::Tape(tape) => {
        ui.start_tape();
        Some(tape)
      }
      Menu::Build => None,
    };
    let id = ui
      .popup(more.id)
      .id("menu")
      .show(|ui| {
        ui.button("Rename").id("rename").show();
      })
      .id;
    if let Some(tape) = tape {
      *tape = ui.end_tape();
    }
    id
  }

  #[test]
  fn a_replayed_popup_opens_below_where_its_anchor_is_now() {
    // Taped with its anchor at 10, replayed with it at 300
    let mut built = UiHarness::new(640.0, 480.0);
    built.move_to(630.0, 470.0);
    let mut tape = Tape::default();
    built.frame(|ui| anchored(ui, 10.0, Menu::Tape(&mut tape)));
    built.frame(|ui| anchored(ui, 300.0, Menu::Build));
    let menu = built.frame(|ui| anchored(ui, 300.0, Menu::Build));

    let mut replayed = UiHarness::new(640.0, 480.0);
    replayed.move_to(630.0, 470.0);
    replayed.frame(|ui| anchored(ui, 300.0, Menu::Build));
    replayed.frame(|ui| anchored(ui, 300.0, Menu::Build));
    replayed.frame(|ui| anchored(ui, 300.0, Menu::Replay(&tape)));
    assert!(tape.replayable);
    let rect = replayed.record_of(menu).map(|r| r.rect).unwrap_or_default();
    assert_eq!(rect.x, 300.0, "below the anchor where it is now");
    assert_eq!(snapshot(&replayed, &[menu]), snapshot(&built, &[menu]));
  }

  #[test]
  fn a_taped_text_node_shares_its_leaf_with_every_replay() {
    let mut h = UiHarness::new(640.0, 480.0);
    let mut tape = Tape::default();
    h.frame(|ui| {
      ui.start_tape();
      ui.column().show(|ui| {
        ui.text("Hello");
      });
      tape = ui.end_tape();
    });
    let leaf = tape
      .nodes
      .iter()
      .find_map(|node| match &node.text {
        Some(Leaf::Shared(leaf)) => Some(Arc::clone(leaf)),
        Some(Leaf::Owned(_)) => panic!("a taped leaf is shared"),
        None => None,
      })
      .expect("a text node");
    assert_eq!(leaf.text.text, "Hello");
    // The tape's copy and this one: the subtree's went with its frame
    assert_eq!(Arc::strong_count(&leaf), 2);
    let copy = tape.clone();
    assert_eq!(Arc::strong_count(&leaf), 3, "a copy of the tape shares it");
    drop(copy);
    h.frame(|ui| ui.replay(&tape, &mut |_, _| {}));
    assert_eq!(Arc::strong_count(&leaf), 2, "a replay keeps no copy");
  }

  #[test]
  fn an_untaped_frame_records_nothing_and_taints_nothing() {
    let mut h = UiHarness::new(640.0, 480.0);
    h.frame(|ui| {
      ui.taint();
      ui.begin_child(Entity::from_raw_u32(5).unwrap());
      panel(ui);
      ui.end_child();
      assert!(ui.tapes.is_empty());
      assert!(!ui.end_tape().replayable, "no tape was started");
    });
  }
}
