//! Inventory messages for game code to write as items move (`ctx.write_message`)
//! and for other logic to react to (`ctx.messages`). Nothing writes them
//! automatically.

// The glob brings the name `#[derive(Message)]` expands to
use ryo2d::prelude::*;

use crate::ItemId;

/// An item was picked up from the world.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemPickedUp {
  /// Who picked it up
  pub entity: Entity,
  pub item_id: ItemId,
  pub quantity: u32,
}

/// An item was dropped into the world.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemDropped {
  /// The dropped item's entity
  pub entity: Entity,
  pub item_id: ItemId,
  pub quantity: u32,
}

/// An inventory's count of an item changed.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct InventoryChanged {
  pub item_id: ItemId,
  pub new_total: u32,
}

/// An item was equipped into a slot.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemEquipped {
  pub slot: usize,
  pub item_id: ItemId,
}

/// An item was taken out of a slot.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemUnequipped {
  pub slot: usize,
  pub item_id: ItemId,
}
