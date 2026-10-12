//! Inventory and items for Ryo2D
//!
//! A generic framework for items, inventories, equipment, and loot tables.
//!
//! # Overview
//!
//! The inventory system provides:
//! - [`ItemRegistry`] — Central catalog for item templates (code + JSON)
//! - [`Inventory`] — Slot-based item container with stacking and transfer
//! - [`Equipment`] — Abstract indexed equipment slots with optional validation
//! - [`LootTable`] — Weighted randomization for item drops
//! - [`ItemDrop`] — ECS component for items in the game world
//!
//! Items use a hybrid value/entity model: lightweight [`ItemStack`] values in
//! inventory, full ECS entities when dropped in the world.
//!
//! # Quick Start
//!
//! ```rust,ignore
//! use ryo2d::prelude::*;
//! use ryo2d_inventory::prelude::*;
//!
//! // Register item templates
//! let mut registry = ItemRegistry::new();
//! let potion_id = registry.register(
//!     ItemTemplate::builder("health_potion")
//!         .display_name("Health Potion")
//!         .max_stack(10)
//!         .tag("consumable")
//!         .build()
//! );
//!
//! // Create an inventory and add items
//! let mut inv = Inventory::new(20);
//! inv.add(ItemStack::new(potion_id, 3), &registry).unwrap();
//! assert_eq!(inv.count(potion_id), 3);
//! ```

mod container;
mod equipment;
mod error;
mod item;
mod loot;
mod registry;
mod world;

pub use container::*;
pub use equipment::*;
pub use error::{InventoryError, InventoryResult};
pub use item::*;
pub use loot::*;
pub use registry::*;
pub use world::*;

/// Everything a game uses, for `use ryo2d_inventory::prelude::*;` beside
/// `ryo2d::prelude::*`.
pub mod prelude {
  pub use crate::{
    EquipValidator, Equipment, Inventory, InventoryError, InventoryResult, ItemData, ItemDrop,
    ItemId, ItemRegistry, ItemStack, ItemTemplate, ItemTemplateBuilder, LootEntry, LootTable,
    pickup_item, spawn_item_drop,
  };
}
