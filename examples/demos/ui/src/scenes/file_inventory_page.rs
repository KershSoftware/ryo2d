//! The Inventory page built from a UI file: the layout is the file's root and
//! each slot a template the page builds into it; a slot's drag payload is the
//! node itself, and the page writes what the slots show whenever an item moves

use demo_kit::prelude::*;

use super::inventory_model::{BAG, EquipSlot, IDLE, Icons, Inventory, Item, ItemKind, SlotRef};

/// The page's file, relative to the working directory
const INVENTORY: &str = "assets/ui/inventory.ui.ron";

/// Which bag or equipment place a slot node shows
#[derive(Component, Debug, Clone, Copy)]
struct SlotAt(SlotRef);

/// A slot node of the file's template, marked with the place it shows
fn slot(ctx: &mut SceneContext, at: SlotRef) -> Result<UiNode, UiFileError> {
  let slot = ctx
    .ui_template(INVENTORY, "InvSlot", &UiArgs::new())?
    .insert(SlotAt(at));
  Ok(if at == SlotRef::Bag(0) {
    slot.default_focus()
  } else {
    slot
  })
}

/// The page's trees, all built before any is spawned
struct Built {
  root: UiNode,
  equipment: Vec<UiNode>,
  bag: Vec<UiNode>,
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

pub struct FileInventoryPage {
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
  /// Why the file did not load, drawn in place of the page
  error: Option<String>,
}

impl FileInventoryPage {
  pub fn new() -> Self {
    Self {
      model: Inventory::new(),
      icons: Icons::new(),
      holding: false,
      slots: Vec::new(),
      discard: None,
      status: None,
      details: None,
      error: None,
    }
  }

  fn fail(&mut self, error: UiFileError) {
    let text = error.to_string();
    if self.error.as_deref() != Some(text.as_str()) {
      game_error!("{error}");
      self.error = Some(text);
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

  fn build(ctx: &mut SceneContext) -> Result<Built, UiFileError> {
    let root = ctx.assets.load_ui(INVENTORY)?.root(ctx.assets)?;
    let mut equipment = Vec::new();
    for equip in EquipSlot::ALL {
      let args = UiArgs::new().with("label", equip.name());
      let row = ctx.ui_template(INVENTORY, "EquipRow", &args)?;
      equipment.push(row.child(slot(ctx, SlotRef::Equip(equip))?));
    }
    let bag = (0..BAG)
      .map(|i| slot(ctx, SlotRef::Bag(i)))
      .collect::<Result<_, _>>()?;
    Ok(Built {
      root,
      equipment,
      bag,
    })
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

impl Default for FileInventoryPage {
  fn default() -> Self {
    Self::new()
  }
}

impl DemoPage for FileInventoryPage {
  fn name(&self) -> &str {
    "File Inventory"
  }

  fn description(&self) -> &str {
    "The Inventory page built from a UI file: the same backpack and equipment, played the same way.\n\
     Each slot is a template of the file whose drag payload is the node itself; the page writes what the slots show when an item moves."
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
    let built = match Self::build(ctx) {
      Ok(built) => built,
      Err(error) => return self.fail(error),
    };
    let root = ctx.spawn_ui(built.root);
    if let Some(column) = ctx.find_ui_in(root, "equipment") {
      for row in built.equipment {
        ctx.spawn_ui_in(column, row);
      }
    }
    if let Some(grid) = ctx.find_ui_in(root, "bag") {
      for slot in built.bag {
        ctx.spawn_ui_in(grid, slot);
      }
    }
    self.slots = slots_under(ctx.world, root);
    self.discard = ctx.find_ui_in(root, "discard");
    self.status = ctx.find_ui_in(root, "status");
    self.details = Details::find(ctx, root);
    self.refresh(ctx);
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) {
    if let Some(error) = &self.error {
      ui.text(error.as_str());
    }
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
