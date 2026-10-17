//! The Inventory page built from node trees: each slot is a node whose drag
//! payload is the node itself, and the page writes what the slots show
//! whenever an item moves

use demo_kit::prelude::*;

use super::inventory_model::{
  BAG, EquipSlot, GAP, IDLE, Icons, Inventory, Item, ItemKind, SlotRef,
};

/// Which bag or equipment place a slot node shows
#[derive(Component, Debug, Clone, Copy)]
struct SlotAt(SlotRef);

fn slot(at: SlotRef) -> UiNode {
  let slot = UiNode::slot()
    .filter(Filter::Nearest)
    .drag_source()
    .drop_target()
    .insert(SlotAt(at));
  if at == SlotRef::Bag(0) {
    slot.default_focus()
  } else {
    slot
  }
}

/// Every slot node under `root`, in tree order, with the place it shows
fn slots_under(world: &World, root: Entity) -> Vec<(SlotRef, Entity)> {
  let mut slots = Vec::new();
  let mut stack = vec![root];
  while let Some(node) = stack.pop() {
    if let Some(at) = world.get::<SlotAt>(node) {
      slots.push((at.0, node));
    }
    if let Some(children) = world.get::<Children>(node) {
      stack.extend(children.iter().rev());
    }
  }
  slots
}

/// The details panel's nodes: a column for an item and one for an empty
/// slot, one of them hidden
struct Details {
  item: Entity,
  name: Entity,
  text: Entity,
  empty: Entity,
  /// What the item column shows; `None` while the empty one is shown
  shown: Option<ItemKind>,
}

impl Details {
  fn find(ctx: &SceneContext, root: Entity) -> Option<Self> {
    Some(Self {
      item: ctx.find_ui_in(root, "item")?,
      name: ctx.find_ui_in(root, "item-name")?,
      text: ctx.find_ui_in(root, "item-text")?,
      empty: ctx.find_ui_in(root, "empty")?,
      shown: None,
    })
  }
}

pub struct NodeInventoryPage {
  model: Inventory,
  icons: Icons,
  /// An item was held (by the pad or a mouse drag) on the last frame
  /// (`ui.held`): Escape is the UI's Back then, which puts it back
  holding: bool,
  /// The slot nodes and the places they show
  slots: Vec<(SlotRef, Entity)>,
  discard: Option<Entity>,
  status: Option<Entity>,
  details: Option<Details>,
}

impl NodeInventoryPage {
  pub fn new() -> Self {
    Self {
      model: Inventory::new(),
      icons: Icons::new(),
      holding: false,
      slots: Vec::new(),
      discard: None,
      status: None,
      details: None,
    }
  }

  /// The status line: what the last pick-up, move or refusal did
  pub fn status(&self) -> &str {
    self.model.status()
  }

  /// What slot `at` holds
  pub fn item(&self, at: SlotRef) -> Option<Item> {
    self.model.item(at)
  }

  fn tree() -> UiNode {
    let equipment = EquipSlot::ALL.map(|equip| {
      UiNode::row()
        .gap(12.0)
        .align(Align::Center)
        .child(UiNode::text(equip.name()).role(TextRole::Body).muted())
        .child(UiNode::spacer())
        .child(slot(SlotRef::Equip(equip)))
    });
    let bag = (0..BAG).map(|i| slot(SlotRef::Bag(i)));
    UiNode::column()
      .focus_scope()
      .anchor(Anchor::Center)
      .gap(12.0)
      .align(Align::Center)
      .child(
        UiNode::row()
          .gap(16.0)
          .child(
            UiNode::panel()
              .gap(10.0)
              .child(UiNode::text("Equipment").heading())
              .child(UiNode::column().gap(GAP).children(equipment)),
          )
          .child(
            UiNode::panel()
              .gap(10.0)
              .child(UiNode::text("Backpack").heading())
              .child(UiNode::grid(6).gap(GAP).children(bag)),
          ),
      )
      // What the slot under the pointer, or the focused one, holds
      .child(
        UiNode::panel()
          .fill_width()
          .height(76.0)
          .gap(4.0)
          .child(
            UiNode::column()
              .gap(4.0)
              .hidden()
              .name("item")
              .child(UiNode::text("").heading().name("item-name"))
              .child(UiNode::text("").role(TextRole::Body).name("item-text")),
          )
          .child(
            UiNode::column()
              .gap(4.0)
              .name("empty")
              .child(UiNode::text("Nothing here").heading().muted())
              .child(
                UiNode::text("Point at a slot, or move the focus to one, to see what it holds.")
                  .role(TextRole::Body)
                  .muted(),
              ),
          ),
      )
      .child(
        UiNode::row()
          .fill_width()
          .gap(12.0)
          .align(Align::Center)
          .child(UiNode::button("Sort").action("sort"))
          .child(UiNode::spacer())
          .child(UiNode::text("Discard").role(TextRole::Body).muted())
          .child(
            UiNode::slot()
              .drop_target()
              .tooltip("Drop an item here to throw it away")
              .name("discard"),
          ),
      )
      .child(UiNode::text(IDLE).role(TextRole::Body).name("status"))
      .child(
        UiNode::text("[prompt=ui.confirm] Pick up / put down    [prompt=ui.back] Cancel")
          .markup()
          .muted(),
      )
  }

  /// Write what every slot holds, its tooltip, and the status line
  fn refresh(&self, ctx: &mut SceneContext) {
    for &(at, node) in &self.slots {
      let item = self.model.item(at);
      if let Some(mut slot) = ctx.world.get_mut::<UiSlot>(node) {
        slot.icon = item.map(|item| self.icons.of(item.kind).clone());
        slot.count = item.map_or(0, |item| item.count);
      }
      let tooltip = match (item, at) {
        (Some(item), _) => Some(item.kind.name().to_string()),
        (None, SlotRef::Equip(equip)) => Some(format!("{} slot", equip.name())),
        (None, SlotRef::Bag(_)) => None,
      };
      let Ok(mut entity) = ctx.world.get_entity_mut(node) else {
        continue;
      };
      match tooltip {
        Some(text) => {
          if entity.get::<UiTooltip>().is_none_or(|t| t.0 != text) {
            entity.insert(UiTooltip(text));
          }
        }
        None => {
          entity.remove::<UiTooltip>();
        }
      }
    }
    self.write_status(ctx);
  }

  fn write_status(&self, ctx: &mut SceneContext) {
    if let Some(node) = self.status
      && let Some(mut label) = ctx.world.get_mut::<UiLabel>(node)
      && label.0 != self.model.status()
    {
      label.0 = self.model.status().to_string();
    }
  }

  /// Show the details of the slot under the pointer, else the focused one
  fn show_details(&mut self, ctx: &mut SceneContext) {
    let interaction = |pick: fn(&UiInteraction) -> bool| {
      self
        .slots
        .iter()
        .find(|(_, node)| ctx.world.get::<UiInteraction>(*node).is_some_and(pick))
        .map(|(at, _)| *at)
    };
    let at = interaction(UiInteraction::hovered).or_else(|| interaction(UiInteraction::focused));
    let kind = at.and_then(|at| self.model.item(at)).map(|item| item.kind);
    let Some(details) = self.details.as_mut() else {
      return;
    };
    if details.shown == kind {
      return;
    }
    details.shown = kind;
    let (show, hide) = match kind {
      Some(kind) => {
        for (node, text) in [
          (details.name, kind.name()),
          (details.text, kind.description()),
        ] {
          if let Some(mut label) = ctx.world.get_mut::<UiLabel>(node) {
            label.0 = text.to_string();
          }
        }
        (details.item, details.empty)
      }
      None => (details.empty, details.item),
    };
    if let Ok(mut column) = ctx.world.get_entity_mut(show) {
      column.remove::<UiHidden>();
    }
    if let Ok(mut column) = ctx.world.get_entity_mut(hide) {
      column.insert(UiHidden);
    }
  }
}

impl Default for NodeInventoryPage {
  fn default() -> Self {
    Self::new()
  }
}

impl DemoPage for NodeInventoryPage {
  fn name(&self) -> &str {
    "Node Inventory"
  }

  fn description(&self) -> &str {
    "The Inventory page built from node trees: the same backpack and equipment, played the same way.\n\
     Each slot is a node whose drag payload is the node itself; the page writes what the slots show when an item moves."
  }

  fn controls(&self) -> Vec<Control> {
    let mut controls = vec![
      Control::mouse("Drag an item to another slot"),
      Control::keys(
        "Enter / Space",
        &[KeyCode::Enter, KeyCode::Space],
        "Pick up, put down",
      ),
    ];
    if self.holding {
      controls.push(Control::key(KeyCode::Escape, "Put the item back"));
    }
    controls
  }

  /// While an item is held, Escape puts it back instead of quitting
  fn wants_escape(&self) -> bool {
    self.holding
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    let root = ctx.spawn_ui(Self::tree());
    self.slots = slots_under(ctx.world, root);
    self.discard = ctx.find_ui_in(root, "discard");
    self.status = ctx.find_ui_in(root, "status");
    self.details = Details::find(ctx, root);
    self.refresh(ctx);
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) {
    let slot_at = |ctx: &SceneContext, node: Entity| ctx.world.get::<SlotAt>(node).map(|at| at.0);
    let (mut told, mut changed) = (false, false);
    for event in ctx.ui_events() {
      match (event.kind, event.action.as_deref()) {
        (UiEventKind::Dropped { source }, _) => {
          let Some(from) = slot_at(ctx, source) else {
            continue;
          };
          if Some(event.node) == self.discard {
            told = self.model.discard(from);
          } else if let Some(to) = slot_at(ctx, event.node) {
            told = self.model.move_item(from, to);
          }
          changed = true;
        }
        (UiEventKind::Clicked, Some("sort")) => {
          self.model.sort();
          (told, changed) = (true, true);
        }
        _ => {}
      }
    }
    if changed {
      self.refresh(ctx);
    }
    // What is held now, after this frame's pick-ups; once it is put back,
    // released over nothing or put down where it was, the status stops
    // saying it is held
    let held = ui
      .held::<Entity>()
      .and_then(|node| slot_at(ctx, node))
      .and_then(|at| self.model.item(at));
    match held {
      Some(item) => self
        .model
        .set_status(format!("Holding the {}.", item.kind.name())),
      None if self.holding && !told => self.model.set_status(IDLE),
      None => {}
    }
    self.holding = held.is_some();
    self.write_status(ctx);
    self.show_details(ctx);
  }
}
