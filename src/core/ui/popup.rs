//! Popups: subtrees built where they are called (a dropdown's list, a
//! context menu, a modal) and drawn when their scene's UI ends, above its page
//!
//! `begin_popup` sets the subtree being built aside on the context's subtree
//! stack and starts the popup's own; widgets record into it until
//! `end_popup` queues it and brings the set-aside subtree back. `end_scene`
//! lays the queued popups out, in the order they were begun (a popup opened
//! inside another draws over it), after everything the scene drew.

use glam::Vec2;

use super::immediate::{PAGE_LAYER, POPUP_LAYER, UiContext};
use super::tree::Subtree;
use super::{Anchor, Rect};
use crate::core::ui::theme::warn_once;

/// Room between a popup and its anchor
const GAP: f32 = 2.0;

/// A popup recorded this scene, waiting for its UI to end
pub(crate) struct Popup {
  pub(crate) subtree: Subtree,
  /// The rect it opens below (`None`: centred on the UI)
  pub(crate) anchor: Option<Rect>,
  /// A modal: it covers the whole UI
  pub(crate) modal: bool,
}

/// What a popup being recorded set aside: the subtree that was being built
/// and its open node, and the popup's own settings
pub(crate) struct SetAside {
  subtree: Option<Subtree>,
  open: Option<usize>,
  anchor: Option<Rect>,
  modal: bool,
  /// Its place in the queue, ahead of the popups opened inside it
  slot: usize,
}

/// Where a popup of `size` goes for `anchor` in a UI of `ui`: 2 units below
/// it, or above it when only that fits; when neither fits, on the side with
/// more room, moved to stay inside. Its left edge is the anchor's, moved left
/// to stay inside.
pub(crate) fn below(anchor: Rect, size: Vec2, ui: Vec2) -> Vec2 {
  let x = anchor.x.min(ui.x - size.x).max(0.0);
  let under = anchor.bottom() + GAP;
  let over = anchor.y - GAP - size.y;
  let y = if under + size.y <= ui.y {
    under
  } else if over >= 0.0 {
    over
  } else if ui.y - under >= anchor.y - GAP {
    under.min(ui.y - size.y).max(0.0)
  } else {
    0.0
  };
  Vec2::new(x, y)
}

/// The tallest a popup opened at `anchor` in a UI of `ui` can be and stay
/// inside it: the room on the roomier side, below or above. Capped at that,
/// [`below`] puts it on the side it chooses anyway (below or above when it
/// fits there, else that roomier side) without moving it over the anchor.
/// The whole UI without an anchor.
pub(crate) fn room(anchor: Option<Rect>, ui: Vec2) -> f32 {
  let room = anchor.map_or(ui.y, |anchor| {
    (ui.y - anchor.bottom() - GAP).max(anchor.y - GAP)
  });
  if room.is_finite() { room.max(0.0) } else { 0.0 }
}

impl UiContext<'_> {
  /// Start recording a popup: the nodes that follow go into a subtree of
  /// their own, opened in the focus scope here, until
  /// [`end_popup`](Self::end_popup). It opens below `anchor` (centred when
  /// `None`), or covers the UI when `modal`.
  pub(crate) fn begin_popup(&mut self, anchor: Option<Rect>, modal: bool) {
    let scope = self.focus_scope_here();
    // Popups draw over everything, so no clip of the page reaches them
    let (subtree, open) = self.swap_subtree(Some(Subtree::new(None, None, scope)), None);
    let slot = self.popups.len();
    self.subtree_stack.push(SetAside {
      subtree,
      open,
      anchor,
      modal,
      slot,
    });
  }

  /// Stop recording the popup begun last: it is queued until the scene's UI
  /// ends, and the subtree it set aside is built on again
  pub(crate) fn end_popup(&mut self) {
    let Some(aside) = self.subtree_stack.pop() else {
      warn_once("end_popup".to_string(), || {
        "end_popup with no popup begun; ignored".to_string()
      });
      return;
    };
    let (popup, _) = self.swap_subtree(aside.subtree, aside.open);
    if let Some(subtree) = popup.filter(|subtree| !subtree.nodes.is_empty()) {
      let slot = aside.slot.min(self.popups.len());
      self.popups.insert(
        slot,
        Popup {
          subtree,
          anchor: aside.anchor,
          modal: aside.modal,
        },
      );
    }
  }

  /// Whether a popup is being recorded: its root does not lay out when it
  /// closes
  pub(crate) fn in_popup(&self) -> bool {
    !self.subtree_stack.is_empty()
  }

  /// Whether the top scene has queued a popup that is not a modal this
  /// pass (an open list or `ui.popup`)
  pub(crate) fn popup_queued(&self) -> bool {
    let (_, top) = self.scene();
    top && self.popups.iter().any(|popup| !popup.modal)
  }

  /// Lay out the scene's queued popups, in order, after everything it drew:
  /// their draws and records take fresh places in the painter's order, on
  /// the popup layer for the top scene and on the page layer for a lower one
  /// (so the scenes above still cover them)
  pub(crate) fn flush_popups(&mut self) {
    let popups = std::mem::take(&mut self.popups);
    let (_, top) = self.scene();
    let layer = if top { POPUP_LAYER } else { PAGE_LAYER };
    let ui = self.ui_size();
    for Popup {
      mut subtree,
      anchor,
      modal,
    } in popups
    {
      self.renumber(&mut subtree, layer);
      if subtree.at.is_none() {
        match (modal, anchor) {
          (true, _) => subtree.at = Some(Rect::new(0.0, 0.0, ui.x, ui.y)),
          (false, Some(anchor)) => {
            subtree.below = Some(anchor);
            // At least as wide as what it opens from
            if let Some(root) = subtree.nodes.first_mut() {
              let width = root
                .style
                .min_width
                .map_or(anchor.width, |w| w.max(anchor.width));
              root.style.min_width = Some(width);
            }
          }
          (false, None) => {
            if let Some(root) = subtree.nodes.first_mut() {
              root.style.anchor = Anchor::Center;
            }
          }
        }
      }
      self.finish_subtree(subtree);
    }
  }

  /// Give `subtree`'s nodes and draws fresh places in the painter's order,
  /// keeping their order among themselves, on `layer`
  fn renumber(&mut self, subtree: &mut Subtree, layer: u8) {
    let mut old: Vec<u32> = subtree
      .nodes
      .iter()
      .map(|node| node.order)
      .chain(subtree.draws.iter().map(|draw| draw.order))
      .collect();
    old.sort_unstable();
    old.dedup();
    let fresh: Vec<u32> = old.iter().map(|_| self.next_order()).collect();
    let new = |order: u32| {
      old
        .binary_search(&order)
        .ok()
        .and_then(|k| fresh.get(k).copied())
        .unwrap_or(order)
    };
    for node in &mut subtree.nodes {
      node.order = new(node.order);
      node.layer = layer;
    }
    for draw in &mut subtree.draws {
      draw.order = new(draw.order);
      draw.layer = layer;
    }
  }
}

#[cfg(test)]
mod tests {
  use glam::Vec2;

  use super::{below, room};
  use crate::core::ui::Rect;

  const UI: Vec2 = Vec2::new(300.0, 200.0);

  #[test]
  fn a_popup_goes_two_units_below_its_anchor() {
    let anchor = Rect::new(10.0, 10.0, 120.0, 28.0);
    assert_eq!(
      below(anchor, Vec2::new(120.0, 90.0), UI),
      Vec2::new(10.0, 40.0)
    );
  }

  #[test]
  fn a_popup_flips_above_an_anchor_near_the_bottom() {
    let anchor = Rect::new(10.0, 150.0, 120.0, 28.0);
    assert_eq!(
      below(anchor, Vec2::new(120.0, 90.0), UI),
      Vec2::new(10.0, 58.0)
    );
  }

  #[test]
  fn a_popup_is_shifted_inside_the_ui() {
    let anchor = Rect::new(250.0, 10.0, 40.0, 20.0);
    assert_eq!(
      below(anchor, Vec2::new(100.0, 50.0), UI),
      Vec2::new(200.0, 32.0)
    );
  }

  #[test]
  fn a_popup_capped_at_its_room_stays_inside_on_the_roomier_side() {
    // Near the top the room is below, near the bottom above
    for (anchor, below_it) in [
      (Rect::new(10.0, 10.0, 120.0, 28.0), true),
      (Rect::new(10.0, 150.0, 120.0, 28.0), false),
    ] {
      let height = room(Some(anchor), UI);
      let at = below(anchor, Vec2::new(120.0, height), UI);
      assert!(at.y >= 0.0 && at.y + height <= UI.y, "{anchor:?}: inside");
      if below_it {
        assert_eq!(at.y, anchor.bottom() + 2.0, "{anchor:?}: below it");
      } else {
        assert!(at.y + height <= anchor.y, "{anchor:?}: above it");
      }
    }
    assert_eq!(room(None, UI), UI.y, "without an anchor, the whole UI");
  }

  #[test]
  fn a_popup_that_fits_neither_way_takes_the_roomier_side_inside_the_ui() {
    // 118 below, 68 above: below, moved up to stay inside
    let anchor = Rect::new(10.0, 70.0, 40.0, 10.0);
    assert_eq!(
      below(anchor, Vec2::new(40.0, 150.0), UI),
      Vec2::new(10.0, 50.0)
    );
    // Taller than the UI: at its top
    assert_eq!(
      below(anchor, Vec2::new(40.0, 400.0), UI),
      Vec2::new(10.0, 0.0)
    );
  }
}
