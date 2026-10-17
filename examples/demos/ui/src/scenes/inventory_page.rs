//! Inventory page - a backpack and equipment slots: items move by mouse
//! drag or by pad (A picks up, A puts down), and equipment refuses the
//! wrong kind of item

use demo_kit::prelude::*;

use super::art;

/// The slots' gap: the bag's grid and the equipment column share it, so
/// the equipment slots line up with the bag's rows
const GAP: f32 = 6.0;
const BAG: usize = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EquipSlot {
  Head,
  Body,
  Weapon,
}

impl EquipSlot {
  const ALL: [EquipSlot; 3] = [EquipSlot::Head, EquipSlot::Body, EquipSlot::Weapon];

  fn name(self) -> &'static str {
    match self {
      EquipSlot::Head => "Head",
      EquipSlot::Body => "Body",
      EquipSlot::Weapon => "Weapon",
    }
  }

  fn index(self) -> usize {
    self as usize
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
  Sword,
  Shield,
  Helmet,
  Armor,
  Potion,
  Gem,
  Key,
  Bread,
}

impl ItemKind {
  fn name(self) -> &'static str {
    match self {
      ItemKind::Sword => "Sword",
      ItemKind::Shield => "Shield",
      ItemKind::Helmet => "Helmet",
      ItemKind::Armor => "Armor",
      ItemKind::Potion => "Potion",
      ItemKind::Gem => "Gem",
      ItemKind::Key => "Key",
      ItemKind::Bread => "Bread",
    }
  }

  fn description(self) -> &'static str {
    match self {
      ItemKind::Sword => "A plain blade, sharp on both edges. Goes in the Weapon slot.",
      ItemKind::Shield => "Painted blue, dented once or twice. Goes in the Weapon slot.",
      ItemKind::Helmet => "Keeps the rain off and the arrows out. Goes in the Head slot.",
      ItemKind::Armor => "Red leather over chain. Goes in the Body slot.",
      ItemKind::Potion => "Restores a little health. Tastes of cherries.",
      ItemKind::Gem => "A blue gem. Shopkeepers love these.",
      ItemKind::Key => "Opens one door, somewhere.",
      ItemKind::Bread => "Yesterday's loaf, still good.",
    }
  }

  /// The equipment slot it goes in, if any
  fn equips(self) -> Option<EquipSlot> {
    match self {
      ItemKind::Sword | ItemKind::Shield => Some(EquipSlot::Weapon),
      ItemKind::Helmet => Some(EquipSlot::Head),
      ItemKind::Armor => Some(EquipSlot::Body),
      _ => None,
    }
  }

  /// Several of it share one slot
  fn stacks(self) -> bool {
    self.equips().is_none()
  }

  /// "A Potion", "An Armor"
  fn with_article(self) -> String {
    let name = self.name();
    let vowel = name.starts_with(['A', 'E', 'I', 'O', 'U']);
    format!("{} {name}", if vowel { "An" } else { "A" })
  }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Item {
  pub kind: ItemKind,
  pub count: u32,
}

impl Item {
  fn new(kind: ItemKind, count: u32) -> Self {
    Self { kind, count }
  }
}

/// Where an item is: the drag payload
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotRef {
  Bag(usize),
  Equip(EquipSlot),
}

/// The icon of each kind, in `ItemKind` order
struct Icons([SpriteHandle; 8]);

impl Icons {
  fn new() -> Self {
    Icons([
      art::sword(),
      art::shield(),
      art::helmet(),
      art::armor(),
      art::potion(),
      art::gem(),
      art::key(),
      art::bread(),
    ])
  }

  fn of(&self, kind: ItemKind) -> &SpriteHandle {
    &self.0[kind as usize]
  }
}

pub struct InventoryPage {
  bag: [Option<Item>; BAG],
  /// Head, Body, Weapon
  equipment: [Option<Item>; 3],
  icons: Icons,
  status: String,
  /// An item is held (by the pad or a mouse drag), as the last frame
  /// showed: Escape is the UI's Back then, which puts it back
  holding: bool,
}

impl InventoryPage {
  pub fn new() -> Self {
    let mut bag = [None; BAG];
    let start = [
      Item::new(ItemKind::Sword, 1),
      Item::new(ItemKind::Helmet, 1),
      Item::new(ItemKind::Armor, 1),
      Item::new(ItemKind::Potion, 5),
      Item::new(ItemKind::Gem, 12),
      Item::new(ItemKind::Key, 1),
      Item::new(ItemKind::Bread, 3),
    ];
    for (slot, item) in bag.iter_mut().zip(start) {
      *slot = Some(item);
    }
    Self {
      bag,
      equipment: [None; 3],
      icons: Icons::new(),
      status: "Drag an item to move it, or pick it up and put it down with the pad.".to_string(),
      holding: false,
    }
  }

  fn get(&self, at: SlotRef) -> Option<Item> {
    match at {
      SlotRef::Bag(i) => self.bag.get(i).copied().flatten(),
      SlotRef::Equip(slot) => self.equipment[slot.index()],
    }
  }

  fn put(&mut self, at: SlotRef, item: Option<Item>) {
    match at {
      SlotRef::Bag(i) => {
        if let Some(slot) = self.bag.get_mut(i) {
          *slot = item;
        }
      }
      SlotRef::Equip(slot) => self.equipment[slot.index()] = item,
    }
  }

  /// Why `item` cannot go to `to`, if it cannot
  fn refusal(item: Item, to: SlotRef) -> Option<String> {
    match to {
      SlotRef::Equip(slot) if item.kind.equips() != Some(slot) => Some(format!(
        "{} doesn't go in the {} slot.",
        item.kind.with_article(),
        slot.name()
      )),
      _ => None,
    }
  }

  /// Move the item at `from` to `to`, swapping with what is there; an
  /// equipment slot refuses an item of the wrong kind, and nothing moves
  fn move_item(&mut self, from: SlotRef, to: SlotRef) {
    let Some(moving) = self.get(from).filter(|_| from != to) else {
      return;
    };
    let there = self.get(to);
    let refused = Self::refusal(moving, to).or_else(|| there.and_then(|t| Self::refusal(t, from)));
    if let Some(reason) = refused {
      self.status = reason;
      return;
    }
    self.put(to, Some(moving));
    self.put(from, there);
    self.status = match there {
      Some(other) => format!(
        "Swapped the {} and the {}.",
        moving.kind.name(),
        other.kind.name()
      ),
      None => format!("Moved the {}.", moving.kind.name()),
    };
  }

  fn discard(&mut self, from: SlotRef) {
    if let Some(item) = self.get(from) {
      self.put(from, None);
      self.status = format!("Discarded the {}.", item.kind.name());
    }
  }

  /// The bag by kind, stacks of a kind merged into one
  fn sort(&mut self) {
    let mut items: Vec<Item> = Vec::new();
    for item in self.bag.iter().flatten() {
      match items
        .iter_mut()
        .find(|held| held.kind == item.kind && item.kind.stacks())
      {
        Some(held) => held.count += item.count,
        None => items.push(*item),
      }
    }
    items.sort_by_key(|item| item.kind as usize);
    self.bag = [None; BAG];
    for (slot, item) in self.bag.iter_mut().zip(items) {
      *slot = Some(item);
    }
    self.status = "Sorted the backpack.".to_string();
  }

  /// One slot: a target for any item, the source of the one it holds
  fn slot(&self, ui: &mut UiContext, at: SlotRef, key: usize) -> Response {
    let mut slot = ui
      .slot()
      .id(key)
      .filter(Filter::Nearest)
      .drop_target::<SlotRef>();
    // The bag's first slot has the first focus
    if at == SlotRef::Bag(0) {
      slot = slot.default_focus();
    }
    match self.get(at) {
      Some(item) => {
        slot = slot
          .icon(self.icons.of(item.kind))
          .count(item.count)
          .tooltip(item.kind.name())
          .drag_source(at);
      }
      None => {
        if let SlotRef::Equip(equip) = at {
          slot = slot.tooltip(format!("{} slot", equip.name()));
        }
      }
    }
    slot.show()
  }
}

/// What a frame's slots reported
#[derive(Default)]
struct Seen {
  /// A drop: from, to (`None` for Discard)
  drop: Option<(SlotRef, Option<SlotRef>)>,
  /// The slot the pad or a drag picked an item up from
  picked: Option<SlotRef>,
  /// The slot under the pointer, and the focused one: the details show the
  /// first
  hovered: Option<SlotRef>,
  focused: Option<SlotRef>,
  /// An item is held over a slot, or dragged by the mouse
  holding: bool,
}

impl Seen {
  fn note(&mut self, at: SlotRef, response: &Response) {
    if let Some(from) = response.dropped::<SlotRef>() {
      self.drop = Some((from, Some(at)));
    }
    if response.drag_started_here() {
      self.picked = Some(at);
    }
    if response.hovered() {
      self.hovered = Some(at);
    }
    if response.focused() {
      self.focused = Some(at);
    }
    self.holding |= response.drag_hover::<SlotRef>() || response.dragging();
  }
}

impl Default for InventoryPage {
  fn default() -> Self {
    Self::new()
  }
}

impl DemoPage for InventoryPage {
  fn name(&self) -> &str {
    "Inventory"
  }

  fn description(&self) -> &str {
    "Drag and drop between a backpack and equipment slots, with the mouse or the pad: A picks an item up, A puts it down, B puts it back.\n\
     An equipment slot takes only its kind of item. Sort stacks the backpack by kind; Discard throws away what is dropped on it."
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

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) {
    let heading = ui.theme().text.heading.style();
    let body = ui.theme().text.body.style();
    let muted = ui.theme().palette.muted;
    let mut seen = Seen::default();
    let mut sort = false;

    ui.column()
      .focus_scope()
      .anchor(Anchor::Center)
      .gap(12.0)
      .align(Align::Center)
      .show(|ui| {
        ui.row().gap(16.0).show(|ui| {
          ui.panel().gap(10.0).show(|ui| {
            ui.text("Equipment").style(&heading);
            ui.column().gap(GAP).show(|ui| {
              for equip in EquipSlot::ALL {
                ui.row()
                  .id(equip.index())
                  .gap(12.0)
                  .align(Align::Center)
                  .show(|ui| {
                    ui.text(equip.name()).style(&body).color(muted);
                    ui.spacer();
                    let at = SlotRef::Equip(equip);
                    let response = self.slot(ui, at, equip.index());
                    seen.note(at, &response);
                  });
              }
            });
          });
          ui.panel().gap(10.0).show(|ui| {
            ui.text("Backpack").style(&heading);
            ui.grid(6).gap(GAP).show(|ui| {
              for i in 0..BAG {
                let at = SlotRef::Bag(i);
                let response = self.slot(ui, at, i);
                seen.note(at, &response);
              }
            });
          });
        });

        // What the slot under the pointer, or the focused one, holds
        ui.panel().fill_width().height(76.0).gap(4.0).show(|ui| {
          match seen.hovered.or(seen.focused).and_then(|at| self.get(at)) {
            Some(item) => {
              ui.text(item.kind.name()).style(&heading);
              ui.text(item.kind.description()).style(&body);
            }
            None => {
              ui.text("Nothing here").style(&heading).color(muted);
              ui.text("Point at a slot, or move the focus to one, to see what it holds.")
                .style(&body)
                .color(muted);
            }
          }
        });

        ui.row()
          .fill_width()
          .gap(12.0)
          .align(Align::Center)
          .show(|ui| {
            sort = ui.button("Sort").clicked();
            ui.spacer();
            ui.text("Discard").style(&body).color(muted);
            let discard = ui
              .slot()
              .drop_target::<SlotRef>()
              .tooltip("Drop an item here to throw it away")
              .show();
            if let Some(from) = discard.dropped::<SlotRef>() {
              seen.drop = Some((from, None));
            }
            seen.holding |= discard.drag_hover::<SlotRef>();
          });

        ui.text(self.status.as_str()).style(&body);
        ui.text(RichText::markup(
          "[prompt=ui.confirm] Pick up / put down    [prompt=ui.back] Cancel",
        ))
        .color(muted);
      });

    if let Some(at) = seen.picked
      && let Some(item) = self.get(at)
    {
      self.status = format!("Holding the {}.", item.kind.name());
    }
    match seen.drop {
      Some((from, Some(to))) => self.move_item(from, to),
      Some((from, None)) => self.discard(from),
      None => {}
    }
    if sort {
      self.sort();
    }
    self.holding = seen.holding || seen.picked.is_some();
  }
}
