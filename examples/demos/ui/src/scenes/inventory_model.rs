//! What the Inventory page and its node version share: the items, the
//! slots and the bag and equipment that hold them

use demo_kit::prelude::*;

use super::art;

/// The slots' gap: the bag's grid and the equipment column share it, so
/// the equipment slots line up with the bag's rows
pub(super) const GAP: f32 = 6.0;
pub(super) const BAG: usize = 24;
/// The status line with nothing to report
pub(super) const IDLE: &str =
  "Drag an item to move it, or pick it up and put it down with the pad.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EquipSlot {
  Head,
  Body,
  Weapon,
}

impl EquipSlot {
  pub(super) const ALL: [EquipSlot; 3] = [EquipSlot::Head, EquipSlot::Body, EquipSlot::Weapon];

  pub(super) fn name(self) -> &'static str {
    match self {
      EquipSlot::Head => "Head",
      EquipSlot::Body => "Body",
      EquipSlot::Weapon => "Weapon",
    }
  }

  pub(super) fn index(self) -> usize {
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
  pub(super) fn name(self) -> &'static str {
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

  pub(super) fn description(self) -> &'static str {
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
pub(super) struct Icons([SpriteHandle; 8]);

impl Icons {
  pub(super) fn new() -> Self {
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

  pub(super) fn of(&self, kind: ItemKind) -> &SpriteHandle {
    &self.0[kind as usize]
  }
}

/// The backpack, the equipment and the status line saying what last
/// happened to them
pub(super) struct Inventory {
  bag: [Option<Item>; BAG],
  /// Head, Body, Weapon
  equipment: [Option<Item>; 3],
  status: String,
}

impl Inventory {
  pub(super) fn new() -> Self {
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
      status: IDLE.to_string(),
    }
  }

  /// The status line: what the last pick-up, move or refusal did
  pub(super) fn status(&self) -> &str {
    &self.status
  }

  pub(super) fn set_status(&mut self, status: impl Into<String>) {
    self.status = status.into();
  }

  /// What slot `at` holds
  pub(super) fn item(&self, at: SlotRef) -> Option<Item> {
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
  /// equipment slot refuses an item of the wrong kind, and nothing moves.
  /// Whether it said what happened (not for an item put down where it was)
  pub(super) fn move_item(&mut self, from: SlotRef, to: SlotRef) -> bool {
    let Some(moving) = self.item(from).filter(|_| from != to) else {
      return false;
    };
    let there = self.item(to);
    let refused = Self::refusal(moving, to).or_else(|| there.and_then(|t| Self::refusal(t, from)));
    if let Some(reason) = refused {
      self.status = reason;
      return true;
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
    true
  }

  /// Throw away the item at `from`; whether there was one
  pub(super) fn discard(&mut self, from: SlotRef) -> bool {
    let Some(item) = self.item(from) else {
      return false;
    };
    self.put(from, None);
    self.status = format!("Discarded the {}.", item.kind.name());
    true
  }

  /// The bag by kind, stacks of a kind merged into one
  pub(super) fn sort(&mut self) {
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
}
