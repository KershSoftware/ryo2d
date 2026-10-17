//! Drag and drop: a typed payload carried from a drag source to a drop
//! target, by the mouse (dragged past the threshold, dropped on release) or
//! the pad (Confirm picks it up and puts it down, Back cancels), with a
//! ghost of it drawn while it is held

use std::any::{Any, TypeId};
use std::sync::Arc;

use glam::Vec2;

use super::common::WidgetOpts;
use crate::core::asset::SpriteHandle;
use crate::core::render::batch::Filter;
use crate::core::ui::immediate::{POPUP_LAYER, UiContext};
use crate::core::ui::layout::{Edges, Measure, UiStyle};
use crate::core::ui::memory::Flags;
use crate::core::ui::response::{Response, Sense};
use crate::core::ui::tree::{DrawKind, Node};
use crate::core::ui::{Color, Rect, UiId};

/// The ghost's opacity: 80%
const GHOST_ALPHA: u8 = 204;
/// The pad's ghost sits this far from the focused widget's corner
const PAD_GHOST_OFFSET: Vec2 = Vec2::splat(-8.0);
/// Separates the ghost's node from the scene's widgets
const GHOST_SALT: u64 = 0x0067_686f_7374;

/// A payload being dragged, from its pick-up until it is dropped or
/// cancelled
#[derive(Debug, Clone)]
pub(crate) struct DragState {
  /// The drag source it was picked up from
  pub(crate) source: UiId,
  pub(crate) payload: Arc<dyn Any + Send + Sync>,
  /// The payload's type, which a target must accept
  pub(crate) type_id: TypeId,
  pub(crate) ghost: Ghost,
  /// Picked up with Confirm: it follows the focus and goes down with Confirm
  pub(crate) pad: bool,
  /// Where in the source the pointer took hold of it
  pub(crate) grab: Vec2,
  /// The source's size, which the ghost is drawn at
  pub(crate) size: Vec2,
}

/// What is drawn for a payload being dragged
#[derive(Debug, Clone)]
pub(crate) enum Ghost {
  /// The source's `drag_ghost`, or a slot's icon
  Sprite(SpriteHandle, Filter),
  /// A box in the accent colour
  Box,
}

/// A widget's or container's drag and drop: `.drag_source(payload)`,
/// `.drop_target::<T>()` and `.drag_ghost(sprite)`
#[derive(Debug, Clone, Default)]
pub(crate) struct DragOpts {
  /// The payload it carries, and its type
  pub(crate) payload: Option<(Arc<dyn Any + Send + Sync>, TypeId)>,
  /// The payload type it takes drops of
  pub(crate) accepts: Option<TypeId>,
  /// Drawn while its payload is dragged
  pub(crate) ghost: Option<SpriteHandle>,
}

impl DragOpts {
  /// Carry `payload`
  pub(crate) fn source<T: Clone + Send + Sync + 'static>(&mut self, payload: T) {
    self.payload = Some((Arc::new(payload), TypeId::of::<T>()));
  }

  /// Whether it is a drag source or a drop target
  pub(crate) fn is_set(&self) -> bool {
    self.payload.is_some() || self.accepts.is_some()
  }

  /// Its record flags: `DRAG_SOURCE | DRAG` for a source (so the pointer
  /// drags it), `DROP_TARGET` for a target
  pub(crate) fn flags(&self) -> Flags {
    let mut flags = Flags::default();
    if self.payload.is_some() {
      flags |= Flags::DRAG_SOURCE | Flags::DRAG;
    }
    if self.accepts.is_some() {
      flags |= Flags::DROP_TARGET;
    }
    flags
  }

  /// `sense`, with drags for a source
  pub(crate) fn sense(&self, sense: Sense) -> Sense {
    Sense {
      drag: sense.drag || self.payload.is_some(),
      ..sense
    }
  }
}

/// The drag and drop builder methods every widget and container shares, for
/// a builder whose `DragOpts` are at `self.<path>`: `drag_source`,
/// `drop_target` and `drag_ghost`. Expands to methods, inside an `impl`. The
/// payload's type parameter is `P`, so a builder generic over `T` (a radio)
/// can have them.
macro_rules! drag_builders {
  ($($field:ident).+) => {
    /// Make it a drag source carrying `payload`. The mouse drags it past the
    /// drag threshold, or Confirm picks it up while it has the pad's focus
    /// (Back cancels); released over (or put down with Confirm on) a
    /// [`drop_target`](Self::drop_target) of `P`, a clone of `payload`
    /// arrives on that target's
    /// [`dropped`](crate::core::ui::Response::dropped). Its response reports
    /// [`drag_started_here`](crate::core::ui::Response::drag_started_here)
    /// when it is picked up, and Confirm is then not a click.
    pub fn drag_source<P: Clone + Send + Sync + 'static>(mut self, payload: P) -> Self {
      self.$($field).+.source(payload);
      self
    }

    /// Take drops of `P` payloads: read them with
    /// [`dropped`](crate::core::ui::Response::dropped), and see one held
    /// over it with [`drag_hover`](crate::core::ui::Response::drag_hover)
    pub fn drop_target<P: 'static>(mut self) -> Self {
      self.$($field).+.accepts = Some(::std::any::TypeId::of::<P>());
      self
    }

    /// Draw `sprite` while its payload is dragged, at its size and 80%
    /// opacity, under the pointer or beside the pad's focus (without one, a
    /// slot draws its icon and anything else an accent box)
    pub fn drag_ghost(mut self, sprite: &$crate::core::asset::SpriteHandle) -> Self {
      self.$($field).+.ghost = Some(sprite.clone());
      self
    }
  };
}

pub(crate) use drag_builders;

impl UiContext<'_> {
  /// The payload being dragged, when it is a `P`: from the frame a source
  /// is picked up (a mouse drag starting, or Confirm on the pad) until it is
  /// dropped, put back with Back or released over nothing. `None` when
  /// nothing is held, the payload is another type, or this is not the top
  /// scene. It shows what the player carries, and tells a game that Back
  /// (Escape) is the drag's while it is `Some`.
  ///
  /// ```ignore
  /// if let Some(from) = ui.held::<SlotRef>() {
  ///     self.status = format!("Holding the {}.", self.item(from).name());
  /// }
  /// ```
  pub fn held<P: Clone + 'static>(&self) -> Option<P> {
    let (_, top) = self.scene();
    let drag = self.memory.interact.drag.as_ref().filter(|_| top)?;
    drag.payload.downcast_ref::<P>().cloned()
  }

  /// A widget's [`response`](UiContext::response), sensing drags too when
  /// it is a drag source, with its drag and drop applied
  /// ([`drag_tick`](Self::drag_tick))
  pub(crate) fn widget_response(&mut self, id: UiId, sense: Sense, opts: &WidgetOpts) -> Response {
    let mut response = self.response(id, opts.drag.sense(sense), opts.disabled);
    self.drag_tick(
      &mut response,
      &opts.drag,
      opts.disabled,
      (None, Filter::Default),
    );
    response
  }

  /// Drag and drop for the widget or container `response` is for, in the
  /// top scene, before it reads its clicks. A target gets this frame's drop
  /// and whether a payload it takes is over it. A source with nothing held
  /// is picked up when the mouse's drag of it starts (`grab` where it was
  /// pressed), or when Confirm activates it, which is then not a click. The
  /// ghost is `drag.ghost`, else `ghost`'s sprite (a slot's icon), else a
  /// box; a sprite is sampled with `ghost`'s filter.
  pub(crate) fn drag_tick(
    &mut self,
    response: &mut Response,
    drag: &DragOpts,
    disabled: bool,
    ghost: (Option<&SpriteHandle>, Filter),
  ) {
    let id = response.id;
    let (_, top) = self.scene();
    if !top || disabled || id == UiId::NONE || !drag.is_set() {
      return;
    }
    let occluded = self.is_mouse_occluded();
    let i = &mut self.memory.interact;
    if drag.accepts.is_some() {
      if let Some((target, payload)) = &i.dropped
        && *target == id
      {
        response.dropped = Some(Arc::clone(payload));
      }
      if let Some(held) = &i.drag
        && drag.accepts == Some(held.type_id)
      {
        let over = if held.pad {
          i.focused == id
        } else {
          i.drop_hover == id
        };
        if over {
          response.drag_over = Some(held.type_id);
        }
      }
    }

    let Some((payload, type_id)) = &drag.payload else {
      return;
    };
    if i.drag.is_some() {
      return;
    }
    let mouse = i.drag_started == id && i.active_button == 0 && !occluded;
    if !mouse && i.activated != id {
      return;
    }
    let rect = response.rect;
    let (fallback, filter) = ghost;
    let ghost = match drag.ghost.as_ref().or(fallback) {
      Some(sprite) => Ghost::Sprite(sprite.clone(), filter),
      None => Ghost::Box,
    };
    let grab = if mouse {
      i.press_pos - Vec2::new(rect.x, rect.y)
    } else {
      Vec2::ZERO
    };
    i.drag = Some(DragState {
      source: id,
      payload: Arc::clone(payload),
      type_id: *type_id,
      ghost,
      pad: !mouse,
      grab,
      size: Vec2::new(rect.width, rect.height),
    });
    if !mouse {
      response.states &= !Response::CLICKED;
    }
    response.states |= Response::DRAG_STARTED_HERE;
  }

  /// Draw the held payload's ghost, at the end of the top scene's UI when
  /// its source is one of the scene's widgets: on the popup layer over the
  /// popups, at 80% opacity, the source's size, under the pointer where it
  /// took hold or (from the pad) up and left of the focused widget
  pub(crate) fn flush_ghost(&mut self) {
    let (scene, top) = self.scene();
    let Some(drag) = self.memory.interact.drag.clone().filter(|_| top) else {
      return;
    };
    let shown = self
      .memory
      .current(drag.source)
      .is_some_and(|record| record.scene == scene);
    if !shown {
      return;
    }
    let at = if drag.pad {
      let focused = self.memory.interact.focused;
      let Some(rect) = self
        .memory
        .current(focused)
        .or_else(|| self.memory.prev(focused))
        .map(|record| record.rect)
      else {
        return;
      };
      Vec2::new(rect.x, rect.y) + PAD_GHOST_OFFSET
    } else {
      self.memory.interact.pointer - drag.grab
    };
    let rect = Rect::new(at.x, at.y, drag.size.x, drag.size.y);
    let kind = match drag.ghost {
      Ghost::Sprite(sprite, filter) => DrawKind::Sprite {
        inset: Edges::ZERO,
        frac: Vec2::splat(0.5),
        size: None,
        sprite,
        uv: [0.0, 0.0, 1.0, 1.0],
        tint: Color::WHITE.with_alpha(GHOST_ALPHA),
        filter,
      },
      Ghost::Box => {
        let theme = self.theme();
        let accent = theme.palette.accent;
        let alpha = u16::from(accent.a) * u16::from(GHOST_ALPHA) / 255;
        DrawKind::Fill {
          inset: Edges::ZERO,
          fill: accent.with_alpha(alpha as u8),
          radius: theme.metrics.radius,
          border: None,
        }
      }
    };
    let id = UiId::root(scene).derived(GHOST_SALT);
    let page = self.set_layer(POPUP_LAYER);
    self.with_no_clip(|ui| {
      let node = Node::new(id, UiStyle::default(), Measure::Fixed(drag.size));
      let index = ui.push_node(node, Some(rect));
      ui.push_draw(index, kind);
      ui.close_node(index);
    });
    self.set_layer(page);
  }
}

#[cfg(all(test, feature = "default-fonts"))]
mod tests {
  use crate::core::input::Direction;
  use crate::core::ui::test_util::UiDriver;
  use crate::core::ui::{Rect, UiContext};

  #[derive(Debug, Clone, Copy, PartialEq)]
  struct Item(u32);

  /// Two slots side by side: 0 holds Item(7); returns what each received
  fn slots(ui: &mut UiContext<'_>) -> [Option<Item>; 2] {
    let mut got = [None, None];
    ui.row().gap(4.0).focus_scope().show(|ui| {
      for (i, received) in got.iter_mut().enumerate() {
        let mut slot = ui.slot().id(i).drop_target::<Item>();
        if i == 0 {
          slot = slot.drag_source(Item(7)).default_focus();
        }
        *received = slot.show().dropped::<Item>();
      }
    });
    got
  }

  #[test]
  fn dragging_a_slot_onto_another_drops_its_payload() {
    let mut d = UiDriver::new((200.0, 100.0));
    d.frame(slots);
    d.move_to(10.0, 10.0);
    d.press();
    d.frame(slots);
    d.move_to(30.0, 10.0);
    d.frame(slots); // past the threshold: the drag starts
    d.move_to(70.0, 20.0); // over slot 1 (48 + 4 from x 0)
    d.frame(slots);
    d.release();
    assert_eq!(d.frame(slots), [None, Some(Item(7))]);
    assert_eq!(d.frame(slots), [None, None], "one frame only");
  }

  #[test]
  fn releasing_over_nothing_cancels_the_drag() {
    let mut d = UiDriver::new((200.0, 100.0));
    d.frame(slots);
    d.move_to(10.0, 10.0);
    d.press();
    d.frame(slots);
    d.move_to(150.0, 80.0);
    d.frame(slots);
    d.release();
    assert_eq!(d.frame(slots), [None, None]);
  }

  #[test]
  fn a_target_of_another_type_ignores_the_drop() {
    let mut d = UiDriver::new((200.0, 100.0));
    let draw = |ui: &mut UiContext<'_>| {
      ui.slot()
        .at(Rect::new(0.0, 0.0, 48.0, 48.0))
        .drag_source(Item(1))
        .show();
      ui.slot()
        .at(Rect::new(60.0, 0.0, 48.0, 48.0))
        .drop_target::<u8>()
        .show()
        .dropped::<u8>()
    };
    d.frame(draw);
    d.move_to(10.0, 10.0);
    d.press();
    d.frame(draw);
    d.move_to(80.0, 10.0);
    d.frame(draw);
    d.release();
    assert_eq!(d.frame(draw), None);
  }

  #[test]
  fn the_pad_picks_up_moves_and_puts_down() {
    let mut d = UiDriver::new((200.0, 100.0));
    for _ in 0..2 {
      d.frame(slots);
    }
    d.confirm(); // pick up slot 0
    assert_eq!(d.frame(slots), [None, None]);
    d.nav(Direction::Right);
    d.frame(slots);
    d.confirm(); // put down on slot 1
    assert_eq!(d.frame(slots), [None, Some(Item(7))]);
  }

  #[test]
  fn held_is_the_payload_from_pick_up_until_the_drag_ends() {
    let mut d = UiDriver::new((200.0, 100.0));
    let draw = |ui: &mut UiContext<'_>| {
      slots(ui);
      (ui.held::<Item>(), ui.held::<u8>())
    };
    for _ in 0..2 {
      assert_eq!(d.frame(draw), (None, None), "nothing held yet");
    }
    d.confirm(); // pick up slot 0
    assert_eq!(
      d.frame(draw),
      (Some(Item(7)), None),
      "held from the pick-up"
    );
    d.nav(Direction::Right);
    assert_eq!(d.frame(draw).0, Some(Item(7)), "still held as it moves");
    d.back(); // put it back
    assert_eq!(d.frame(draw), (None, None), "Back ended it");
    // A mouse drag holds it too, until its release
    d.move_to(10.0, 10.0);
    d.press();
    d.frame(draw);
    d.move_to(150.0, 80.0);
    assert_eq!(d.frame(draw).0, Some(Item(7)), "the mouse holds it");
    d.release();
    assert_eq!(d.frame(draw).0, None, "released over nothing");
  }

  #[test]
  fn back_cancels_a_pad_drag() {
    let mut d = UiDriver::new((200.0, 100.0));
    for _ in 0..2 {
      d.frame(slots);
    }
    d.confirm();
    d.frame(slots);
    d.back();
    let back = d.frame(|ui| {
      slots(ui);
      ui.back()
    });
    assert!(!back, "the drag used Back");
    d.nav(Direction::Right);
    d.frame(slots);
    d.confirm();
    assert_eq!(d.frame(slots), [None, None], "nothing was held any more");
  }

  #[test]
  fn the_ghost_follows_the_pointer_at_80_percent() {
    use crate::core::asset::SpriteHandle;
    use crate::core::graphics::Sprite;
    use crate::core::ui::immediate::{POPUP_LAYER, UiCommand};
    let icon =
      SpriteHandle::from_sprite(Sprite::solid_rect(16, 16, 255, 255, 255, 255), "ghost-icon");
    let mut d = UiDriver::new((200.0, 100.0));
    let draw = |ui: &mut UiContext<'_>| {
      ui.slot()
        .at(Rect::new(0.0, 0.0, 48.0, 48.0))
        .icon(&icon)
        .drag_source(Item(3))
        .show();
    };
    d.frame(draw);
    d.move_to(10.0, 10.0); // grabbed 10, 10 into the slot
    d.press();
    d.frame(draw);
    d.move_to(100.0, 50.0);
    d.frame(draw); // past the threshold: the drag starts
    d.frame(draw);
    let ghost: Vec<(Rect, u8)> = d
      .commands
      .iter()
      .filter_map(|(_, c)| match c {
        UiCommand::Sprite {
          rect, tint, layer, ..
        } if *layer == POPUP_LAYER => Some((*rect, tint.a)),
        _ => None,
      })
      .collect();
    assert_eq!(ghost, vec![(Rect::new(90.0, 40.0, 48.0, 48.0), 204)]);
  }

  #[test]
  fn confirm_picks_a_source_up_instead_of_clicking_it() {
    let mut d = UiDriver::new((200.0, 100.0));
    let draw = |ui: &mut UiContext<'_>| {
      let mut seen = (false, false);
      ui.row().focus_scope().show(|ui| {
        let slot = ui.slot().drag_source(Item(1)).default_focus().show();
        seen = (slot.clicked(), slot.drag_started_here());
      });
      seen
    };
    for _ in 0..2 {
      d.frame(draw);
    }
    d.confirm();
    assert_eq!(d.frame(draw), (false, true), "picked up, not clicked");
    assert_eq!(d.frame(draw), (false, false), "once");
    assert!(d.memory.interact.drag.as_ref().is_some_and(|drag| drag.pad));
  }

  #[test]
  fn a_target_sees_a_payload_it_takes_held_over_it() {
    let hover = |ui: &mut UiContext<'_>| {
      let mut over = [false; 2];
      ui.row().gap(4.0).focus_scope().show(|ui| {
        for (i, held) in over.iter_mut().enumerate() {
          let mut slot = ui.slot().id(i).drop_target::<Item>();
          if i == 0 {
            slot = slot.drag_source(Item(7)).default_focus();
          }
          *held = slot.show().drag_hover::<Item>();
        }
      });
      over
    };
    // The mouse: the target under the pointer
    let mut d = UiDriver::new((200.0, 100.0));
    d.frame(hover);
    d.move_to(10.0, 10.0);
    d.press();
    d.frame(hover);
    d.move_to(70.0, 20.0);
    assert_eq!(d.frame(hover), [false, false], "picked up this frame");
    assert_eq!(d.frame(hover), [false, true]);
    d.release();
    d.frame(hover);
    assert_eq!(d.frame(hover), [false, false], "dropped");
    // The pad: the focused target
    let mut d = UiDriver::new((200.0, 100.0));
    for _ in 0..2 {
      d.frame(hover);
    }
    d.confirm();
    d.frame(hover);
    assert_eq!(d.frame(hover), [true, false]);
    d.nav(Direction::Right);
    assert_eq!(d.frame(hover), [false, true]);
  }

  #[test]
  fn the_pads_ghost_is_a_box_up_and_left_of_the_focus() {
    use crate::core::ui::Theme;
    use crate::core::ui::immediate::{POPUP_LAYER, UiCommand};
    let mut d = UiDriver::new((200.0, 100.0));
    for _ in 0..2 {
      d.frame(slots);
    }
    d.confirm();
    d.frame(slots);
    d.nav(Direction::Right);
    d.frame(slots);
    let ghost: Vec<(Rect, crate::core::ui::Color)> = d
      .commands
      .iter()
      .filter_map(|(_, c)| match c {
        UiCommand::Rect {
          rect, fill, layer, ..
        } if *layer == POPUP_LAYER => Some((*rect, *fill)),
        _ => None,
      })
      .collect();
    let accent = Theme::dark().palette.accent.with_alpha(204);
    // Slot 1 is at x 52
    assert_eq!(ghost, vec![(Rect::new(44.0, -8.0, 48.0, 48.0), accent)]);
  }

  #[test]
  fn a_modal_over_a_target_keeps_the_drop_from_it() {
    let mut d = UiDriver::new((300.0, 200.0));
    let draw = |ui: &mut UiContext<'_>| {
      let page = ui
        .slot()
        .at(Rect::new(0.0, 0.0, 48.0, 48.0))
        .drop_target::<Item>()
        .show()
        .dropped::<Item>();
      let mut source = Rect::default();
      ui.modal().show(|ui| {
        source = ui.slot().drag_source(Item(5)).show().rect;
      });
      (page, source)
    };
    d.frame(draw);
    let (_, source) = d.frame(draw);
    let (x, y) = source.center();
    d.move_to(x, y);
    d.press();
    d.frame(draw);
    d.move_to(24.0, 24.0); // over the page's slot, under the modal's dim
    d.frame(draw);
    d.release();
    assert_eq!(d.frame(draw).0, None);
  }

  #[test]
  fn a_payload_whose_source_is_gone_is_held_no_more() {
    let mut d = UiDriver::new((200.0, 100.0));
    for _ in 0..2 {
      d.frame(slots);
    }
    d.confirm();
    d.frame(slots);
    // A frame without the slots
    d.frame(|ui| {
      ui.button("Elsewhere").show();
    });
    for _ in 0..2 {
      d.frame(slots);
    }
    assert!(d.memory.interact.drag.is_none());
    d.nav(Direction::Right);
    d.frame(slots);
    d.confirm();
    assert_eq!(d.frame(slots), [None, None], "nothing to put down");
  }

  #[test]
  fn a_container_takes_drops_through_the_widgets_in_it() {
    let mut d = UiDriver::new((300.0, 200.0));
    let draw = |ui: &mut UiContext<'_>| {
      ui.slot()
        .at(Rect::new(0.0, 0.0, 48.0, 48.0))
        .drag_source(Item(2))
        .show();
      let mut label = Rect::default();
      let bin = ui
        .panel()
        .at(Rect::new(100.0, 0.0, 150.0, 100.0))
        .drop_target::<Item>()
        .show(|ui| label = ui.button("Bin").show().rect);
      (bin.dropped::<Item>(), bin.drag_hover::<Item>(), label)
    };
    d.frame(draw);
    let (.., label) = d.frame(draw);
    d.move_to(10.0, 10.0);
    d.press();
    d.frame(draw);
    let (x, y) = label.center();
    d.move_to(x, y); // over the button in the panel
    d.frame(draw); // the drag starts
    assert!(d.frame(draw).1, "held over the panel, through its button");
    d.release();
    assert_eq!(d.frame(draw).0, Some(Item(2)));
  }
}
