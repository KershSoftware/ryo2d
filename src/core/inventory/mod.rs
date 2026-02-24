//! Inventory & Item System
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
mod item;
mod loot;
mod registry;
mod world;

pub use container::*;
pub use equipment::*;
pub use item::*;
pub use loot::*;
pub use registry::*;
pub use world::*;
