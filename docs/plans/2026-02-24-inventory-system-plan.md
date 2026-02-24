# Inventory & Item System - Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build a generic engine-level inventory system with item registry, slot-based containers, abstract equipment, weighted loot tables, and hybrid value/entity world integration.

**Architecture:** Standalone `src/core/inventory/` module following existing engine patterns. Hybrid model: items in inventory are lightweight value types (`ItemStack`), items in the world are ECS entities with `ItemDrop` component. `ItemData` trait for game-dev custom properties. `ItemRegistry` in `Resources` for cross-scene access.

**Tech Stack:** Ryo2D engine (ECS, Events, Resources, Scene, Procgen RNG), `thiserror`, `serde`/`serde_json`, `rand`

**Reference Skills:** `@ryo-engine-dev`, `@ryo-api`, `@ryo-ecs-internals`, `@ryo-events`

---

## Task 1: Error Types

**Files:**
- Modify: `src/core/error.rs`
- Modify: `src/lib.rs` (prelude)

**Step 1: Write the failing test**

Add to the bottom of `src/core/error.rs` inside the existing `#[cfg(test)] mod tests` block:

```rust
#[test]
fn inventory_error_display() {
    let err = InventoryError::SlotOutOfBounds { slot: 5, capacity: 3 };
    assert_eq!(format!("{err}"), "Slot 5 out of bounds (capacity: 3)");
}

#[test]
fn inventory_error_full_display() {
    let err = InventoryError::InventoryFull;
    assert_eq!(format!("{err}"), "Inventory is full");
}

#[test]
fn engine_error_from_inventory_error() {
    let inv_err = InventoryError::SlotOutOfBounds { slot: 0, capacity: 0 };
    let engine_err: EngineError = inv_err.into();
    assert!(matches!(engine_err, EngineError::Inventory(_)));
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --lib -- error::tests::inventory_error`
Expected: FAIL — `InventoryError` not defined

**Step 3: Write minimal implementation**

In `src/core/error.rs`, add the `InventoryError` enum and `InventoryResult` type after the `ReplicationResult` typedef (line 274):

```rust
/// Inventory system errors
#[derive(Error, Debug)]
pub enum InventoryError {
    /// Slot index exceeds inventory capacity
    #[error("Slot {slot} out of bounds (capacity: {capacity})")]
    SlotOutOfBounds { slot: usize, capacity: usize },

    /// Attempted to interact with an empty slot
    #[error("Slot {slot} is empty")]
    SlotEmpty { slot: usize },

    /// Inventory has no space for the item
    #[error("Inventory is full")]
    InventoryFull,

    /// Item ID not found in registry
    #[error("Item not found: {0}")]
    ItemNotFound(String),

    /// Invalid quantity (zero or negative)
    #[error("Invalid quantity: {0}")]
    InvalidQuantity(u32),

    /// Unknown custom data type during deserialization
    #[error("Unknown item data type: {type_name}")]
    UnknownDataType { type_name: String },

    /// Validation failed (e.g., equip validator rejected item)
    #[error("Validation failed: {0}")]
    ValidationFailed(String),

    /// I/O error (file loading)
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON deserialization error
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Result type for inventory operations
pub type InventoryResult<T> = Result<T, InventoryError>;
```

Add `Inventory` variant to `EngineError`:

```rust
/// Inventory system error
#[error(transparent)]
Inventory(#[from] InventoryError),
```

**Step 4: Add prelude export**

In `src/lib.rs`, add `InventoryError, InventoryResult,` to the error types prelude section (line 254-258).

**Step 5: Run test to verify it passes**

Run: `cargo test --lib -- error::tests::inventory_error`
Expected: PASS (3 tests)

**Step 6: Commit**

```
git add src/core/error.rs src/lib.rs
git commit -m "feat(inventory): add InventoryError type and Result alias"
```

---

## Task 2: Core Data Types — ItemId, ItemData, ItemStack

**Files:**
- Create: `src/core/inventory/mod.rs`
- Create: `src/core/inventory/item.rs`
- Modify: `src/core/mod.rs`
- Create: `tests/integration/inventory.rs`
- Modify: `tests/integration.rs`

**Step 1: Write the failing tests**

Create `tests/integration/inventory.rs`:

```rust
//! Integration tests for the Inventory & Item System
//!
//! Tests cover: ItemId, ItemData trait, ItemStack, ItemTemplate,
//! ItemRegistry, Inventory container, Equipment, LootTable, and world integration.

use ryo2d::prelude::*;
use std::any::Any;

// ============================================================================
// TEST HELPERS - Game-dev custom data types
// ============================================================================

#[derive(Clone, Debug, PartialEq)]
struct WeaponStats {
    damage: f64,
    attack_speed: f64,
}

impl ItemData for WeaponStats {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    fn clone_box(&self) -> Box<dyn ItemData> {
        Box::new(self.clone())
    }
}

#[derive(Clone, Debug, PartialEq)]
struct ArmorStats {
    defense: f64,
}

impl ItemData for ArmorStats {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    fn clone_box(&self) -> Box<dyn ItemData> {
        Box::new(self.clone())
    }
}

// ============================================================================
// ITEM ID TESTS
// ============================================================================

#[test]
fn item_id_equality() {
    let a = ItemId::new(1);
    let b = ItemId::new(1);
    let c = ItemId::new(2);
    assert_eq!(a, b);
    assert_ne!(a, c);
}

#[test]
fn item_id_copy() {
    let a = ItemId::new(1);
    let b = a;
    assert_eq!(a, b); // a still valid after copy
}

// ============================================================================
// ITEM STACK TESTS
// ============================================================================

#[test]
fn item_stack_creation() {
    let stack = ItemStack::new(ItemId::new(0), 5);
    assert_eq!(stack.item_id(), ItemId::new(0));
    assert_eq!(stack.quantity(), 5);
    assert!(stack.instance_data().is_none());
}

#[test]
fn item_stack_with_instance_data() {
    let stats = WeaponStats { damage: 15.0, attack_speed: 1.2 };
    let stack = ItemStack::new(ItemId::new(0), 1)
        .with_data(stats.clone());
    assert!(stack.instance_data().is_some());
    let retrieved = stack.instance_data().unwrap().as_any().downcast_ref::<WeaponStats>();
    assert_eq!(retrieved, Some(&stats));
}

#[test]
fn item_stack_clone_preserves_data() {
    let stats = WeaponStats { damage: 10.0, attack_speed: 1.0 };
    let original = ItemStack::new(ItemId::new(0), 1).with_data(stats.clone());
    let cloned = original.clone();
    let cloned_stats = cloned.instance_data().unwrap().as_any().downcast_ref::<WeaponStats>();
    assert_eq!(cloned_stats, Some(&stats));
}

#[test]
fn item_stack_set_quantity() {
    let mut stack = ItemStack::new(ItemId::new(0), 5);
    stack.set_quantity(10);
    assert_eq!(stack.quantity(), 10);
}
```

Register the test module in `tests/integration.rs`:

```rust
#[path = "integration/inventory.rs"]
mod inventory;
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --test integration inventory`
Expected: FAIL — module `inventory` not found, types not defined

**Step 3: Create module structure**

Create `src/core/inventory/mod.rs`:

```rust
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

mod item;

pub use item::*;
```

Create `src/core/inventory/item.rs`:

```rust
//! Core item types: ItemId, ItemData trait, ItemStack

use serde::{Deserialize, Serialize};
use std::any::Any;
use std::fmt;

/// Lightweight identifier for item templates.
///
/// Internally a `u32` index into the [`ItemRegistry`]. Cheap to copy, compare,
/// hash, and serialize.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ItemId(u32);

impl ItemId {
    /// Create a new ItemId with the given index.
    pub fn new(index: u32) -> Self {
        Self(index)
    }

    /// Get the raw index value.
    pub fn index(self) -> u32 {
        self.0
    }
}

impl fmt::Debug for ItemId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ItemId({})", self.0)
    }
}

impl fmt::Display for ItemId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Extension trait for game-developer custom item properties.
///
/// Implement this for your own stat types (weapon damage, armor defense, etc.)
/// to attach them to item templates and individual item stacks.
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Clone)]
/// struct WeaponStats {
///     damage: f64,
///     attack_speed: f64,
/// }
///
/// impl ItemData for WeaponStats {
///     fn as_any(&self) -> &dyn Any { self }
///     fn as_any_mut(&mut self) -> &mut dyn Any { self }
///     fn clone_box(&self) -> Box<dyn ItemData> { Box::new(self.clone()) }
/// }
/// ```
pub trait ItemData: Any + Send + Sync {
    /// Downcast to concrete type via `Any`.
    fn as_any(&self) -> &dyn Any;
    /// Downcast to concrete type via `Any` (mutable).
    fn as_any_mut(&mut self) -> &mut dyn Any;
    /// Clone into a boxed trait object.
    fn clone_box(&self) -> Box<dyn ItemData>;
}

impl Clone for Box<dyn ItemData> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

/// A stack of items in an inventory slot or the world.
///
/// This is a lightweight value type. Items in inventory are `ItemStack` values;
/// items in the world are ECS entities with an [`ItemDrop`] component containing
/// an `ItemStack`.
///
/// `instance_data` allows per-stack overrides (e.g., enchantments, durability)
/// while the base stats come from the [`ItemTemplate`] in the registry.
#[derive(Clone)]
pub struct ItemStack {
    item_id: ItemId,
    quantity: u32,
    instance_data: Option<Box<dyn ItemData>>,
}

impl ItemStack {
    /// Create a new item stack with the given ID and quantity.
    pub fn new(item_id: ItemId, quantity: u32) -> Self {
        Self {
            item_id,
            quantity,
            instance_data: None,
        }
    }

    /// Attach per-instance custom data to this stack.
    pub fn with_data(mut self, data: impl ItemData) -> Self {
        self.instance_data = Some(Box::new(data));
        self
    }

    /// Get the item template ID.
    pub fn item_id(&self) -> ItemId {
        self.item_id
    }

    /// Get the current quantity.
    pub fn quantity(&self) -> u32 {
        self.quantity
    }

    /// Set the quantity.
    pub fn set_quantity(&mut self, quantity: u32) {
        self.quantity = quantity;
    }

    /// Get the per-instance custom data, if any.
    pub fn instance_data(&self) -> Option<&dyn ItemData> {
        self.instance_data.as_deref()
    }

    /// Get mutable reference to per-instance custom data, if any.
    pub fn instance_data_mut(&mut self) -> Option<&mut dyn ItemData> {
        self.instance_data.as_deref_mut()
    }
}

impl fmt::Debug for ItemStack {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ItemStack")
            .field("item_id", &self.item_id)
            .field("quantity", &self.quantity)
            .field("has_data", &self.instance_data.is_some())
            .finish()
    }
}
```

**Step 4: Wire up the module**

In `src/core/mod.rs`, add under "Game systems" comment (after line 53):

```rust
pub mod inventory;
```

In `src/lib.rs`, add a new prelude section (after Procedural Generation, before State Machines):

```rust
// Inventory & Item System
pub use crate::core::inventory::{ItemData, ItemId, ItemStack};
```

**Step 5: Run tests to verify they pass**

Run: `cargo test --test integration inventory`
Expected: PASS (5 tests)

**Step 6: Commit**

```
git add src/core/inventory/ src/core/mod.rs src/lib.rs tests/integration/inventory.rs tests/integration.rs
git commit -m "feat(inventory): add ItemId, ItemData trait, and ItemStack"
```

---

## Task 3: ItemTemplate and Builder

**Files:**
- Modify: `src/core/inventory/item.rs`
- Modify: `src/core/inventory/mod.rs`
- Modify: `tests/integration/inventory.rs`

**Step 1: Write the failing tests**

Append to `tests/integration/inventory.rs`:

```rust
// ============================================================================
// ITEM TEMPLATE TESTS
// ============================================================================

#[test]
fn item_template_builder() {
    let template = ItemTemplate::builder("iron_sword")
        .display_name("Iron Sword")
        .description("A trusty blade")
        .max_stack(1)
        .tag("weapon")
        .tag("melee")
        .build();

    assert_eq!(template.name(), "iron_sword");
    assert_eq!(template.display_name(), "Iron Sword");
    assert_eq!(template.description(), "A trusty blade");
    assert_eq!(template.max_stack(), 1);
    assert!(template.has_tag("weapon"));
    assert!(template.has_tag("melee"));
    assert!(!template.has_tag("consumable"));
}

#[test]
fn item_template_builder_defaults() {
    let template = ItemTemplate::builder("potion").build();
    assert_eq!(template.display_name(), "potion"); // defaults to name
    assert_eq!(template.description(), "");
    assert_eq!(template.max_stack(), 1);
}

#[test]
fn item_template_with_data() {
    let stats = WeaponStats { damage: 10.0, attack_speed: 1.0 };
    let template = ItemTemplate::builder("sword")
        .data(stats.clone())
        .build();

    let retrieved = template.data().unwrap().as_any().downcast_ref::<WeaponStats>();
    assert_eq!(retrieved, Some(&stats));
}

#[test]
fn item_template_stackable() {
    let template = ItemTemplate::builder("arrow").max_stack(64).build();
    assert_eq!(template.max_stack(), 64);
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --test integration inventory::item_template`
Expected: FAIL — `ItemTemplate` not defined

**Step 3: Implement ItemTemplate and builder**

Add to `src/core/inventory/item.rs`:

```rust
use std::collections::HashSet;

/// Blueprint for an item type, stored in the [`ItemRegistry`].
///
/// Create templates using the builder API:
///
/// ```rust,ignore
/// let template = ItemTemplate::builder("health_potion")
///     .display_name("Health Potion")
///     .description("Restores 50 HP")
///     .max_stack(10)
///     .tag("consumable")
///     .build();
/// ```
pub struct ItemTemplate {
    id: ItemId,
    name: String,
    display_name: String,
    description: String,
    max_stack: u32,
    tags: HashSet<String>,
    data: Option<Box<dyn ItemData>>,
}

impl ItemTemplate {
    /// Start building a new item template with the given internal name.
    pub fn builder(name: &str) -> ItemTemplateBuilder {
        ItemTemplateBuilder::new(name)
    }

    /// Get the item's registry ID. Set when registered.
    pub fn id(&self) -> ItemId {
        self.id
    }

    /// Get the internal name (used for lookups).
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the display name (shown to players).
    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    /// Get the item description.
    pub fn description(&self) -> &str {
        &self.description
    }

    /// Get the maximum stack size (1 = non-stackable).
    pub fn max_stack(&self) -> u32 {
        self.max_stack
    }

    /// Check if this item has a specific tag.
    pub fn has_tag(&self, tag: &str) -> bool {
        self.tags.contains(tag)
    }

    /// Get all tags.
    pub fn tags(&self) -> &HashSet<String> {
        &self.tags
    }

    /// Get the custom data attached to this template, if any.
    pub fn data(&self) -> Option<&dyn ItemData> {
        self.data.as_deref()
    }

    /// Set the registry ID. Called internally by `ItemRegistry::register()`.
    pub(crate) fn set_id(&mut self, id: ItemId) {
        self.id = id;
    }
}

/// Builder for [`ItemTemplate`].
pub struct ItemTemplateBuilder {
    name: String,
    display_name: Option<String>,
    description: String,
    max_stack: u32,
    tags: HashSet<String>,
    data: Option<Box<dyn ItemData>>,
}

impl ItemTemplateBuilder {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            display_name: None,
            description: String::new(),
            max_stack: 1,
            tags: HashSet::new(),
            data: None,
        }
    }

    /// Set the display name (defaults to internal name if not set).
    pub fn display_name(mut self, name: &str) -> Self {
        self.display_name = Some(name.to_string());
        self
    }

    /// Set the item description.
    pub fn description(mut self, desc: &str) -> Self {
        self.description = desc.to_string();
        self
    }

    /// Set the maximum stack size (1 = non-stackable).
    pub fn max_stack(mut self, max: u32) -> Self {
        self.max_stack = max;
        self
    }

    /// Add a tag to this item template.
    pub fn tag(mut self, tag: &str) -> Self {
        self.tags.insert(tag.to_string());
        self
    }

    /// Attach custom data to this template.
    pub fn data(mut self, data: impl ItemData) -> Self {
        self.data = Some(Box::new(data));
        self
    }

    /// Build the item template.
    pub fn build(self) -> ItemTemplate {
        let display_name = self.display_name.unwrap_or_else(|| self.name.clone());
        ItemTemplate {
            id: ItemId::new(0), // Set when registered
            name: self.name,
            display_name,
            description: self.description,
            max_stack: self.max_stack,
            tags: self.tags,
            data: self.data,
        }
    }
}
```

Update `src/core/inventory/mod.rs` to export `ItemTemplate` and `ItemTemplateBuilder`.

Update `src/lib.rs` prelude to add `ItemTemplate, ItemTemplateBuilder,`.

**Step 4: Run tests to verify they pass**

Run: `cargo test --test integration inventory`
Expected: PASS (9 tests)

**Step 5: Commit**

```
git add src/core/inventory/item.rs src/core/inventory/mod.rs src/lib.rs tests/integration/inventory.rs
git commit -m "feat(inventory): add ItemTemplate with builder API"
```

---

## Task 4: ItemRegistry

**Files:**
- Create: `src/core/inventory/registry.rs`
- Modify: `src/core/inventory/mod.rs`
- Modify: `src/lib.rs`
- Modify: `tests/integration/inventory.rs`

**Step 1: Write the failing tests**

Append to `tests/integration/inventory.rs`:

```rust
// ============================================================================
// ITEM REGISTRY TESTS
// ============================================================================

#[test]
fn registry_register_and_get() {
    let mut registry = ItemRegistry::new();
    let id = registry.register(
        ItemTemplate::builder("iron_sword")
            .display_name("Iron Sword")
            .max_stack(1)
            .build()
    );
    let template = registry.get(id).unwrap();
    assert_eq!(template.name(), "iron_sword");
    assert_eq!(template.display_name(), "Iron Sword");
    assert_eq!(template.id(), id);
}

#[test]
fn registry_get_by_name() {
    let mut registry = ItemRegistry::new();
    registry.register(ItemTemplate::builder("health_potion").build());
    let template = registry.get_by_name("health_potion").unwrap();
    assert_eq!(template.name(), "health_potion");
}

#[test]
fn registry_get_by_name_missing() {
    let registry = ItemRegistry::new();
    assert!(registry.get_by_name("nonexistent").is_none());
}

#[test]
fn registry_find_by_tag() {
    let mut registry = ItemRegistry::new();
    registry.register(ItemTemplate::builder("sword").tag("weapon").tag("melee").build());
    registry.register(ItemTemplate::builder("bow").tag("weapon").tag("ranged").build());
    registry.register(ItemTemplate::builder("potion").tag("consumable").build());

    let weapons = registry.find_by_tag("weapon");
    assert_eq!(weapons.len(), 2);

    let melee = registry.find_by_tag("melee");
    assert_eq!(melee.len(), 1);
    assert_eq!(melee[0].name(), "sword");
}

#[test]
fn registry_sequential_ids() {
    let mut registry = ItemRegistry::new();
    let id0 = registry.register(ItemTemplate::builder("a").build());
    let id1 = registry.register(ItemTemplate::builder("b").build());
    let id2 = registry.register(ItemTemplate::builder("c").build());
    assert_eq!(id0.index(), 0);
    assert_eq!(id1.index(), 1);
    assert_eq!(id2.index(), 2);
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --test integration inventory::registry`
Expected: FAIL — `ItemRegistry` not defined

**Step 3: Implement ItemRegistry**

Create `src/core/inventory/registry.rs`:

```rust
//! Item registry — central catalog of all item templates.

use std::collections::HashMap;

use super::item::{ItemData, ItemId, ItemTemplate};
use crate::core::error::InventoryError;

/// Central catalog of all item templates.
///
/// Store in [`Resources`](crate::core::resources::Resources) for cross-scene access.
/// Supports both code-defined and JSON-loaded item templates.
///
/// # Example
///
/// ```rust,ignore
/// let mut registry = ItemRegistry::new();
/// let sword_id = registry.register(
///     ItemTemplate::builder("iron_sword")
///         .display_name("Iron Sword")
///         .max_stack(1)
///         .tag("weapon")
///         .build()
/// );
///
/// let template = registry.get(sword_id).unwrap();
/// ```
pub struct ItemRegistry {
    templates: Vec<ItemTemplate>,
    name_index: HashMap<String, ItemId>,
    data_type_factories: HashMap<String, Box<dyn Fn(&serde_json::Value) -> Option<Box<dyn ItemData>> + Send + Sync>>,
}

impl ItemRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            templates: Vec::new(),
            name_index: HashMap::new(),
            data_type_factories: HashMap::new(),
        }
    }

    /// Register an item template and return its assigned ID.
    pub fn register(&mut self, mut template: ItemTemplate) -> ItemId {
        let id = ItemId::new(self.templates.len() as u32);
        template.set_id(id);
        self.name_index.insert(template.name().to_string(), id);
        self.templates.push(template);
        id
    }

    /// Look up a template by ID.
    pub fn get(&self, id: ItemId) -> Option<&ItemTemplate> {
        self.templates.get(id.index() as usize)
    }

    /// Look up a template by internal name.
    pub fn get_by_name(&self, name: &str) -> Option<&ItemTemplate> {
        self.name_index.get(name).and_then(|id| self.get(*id))
    }

    /// Find all templates with a given tag.
    pub fn find_by_tag(&self, tag: &str) -> Vec<&ItemTemplate> {
        self.templates.iter().filter(|t| t.has_tag(tag)).collect()
    }

    /// Get the ID for a named item, if it exists.
    pub fn id_of(&self, name: &str) -> Option<ItemId> {
        self.name_index.get(name).copied()
    }

    /// Get the total number of registered templates.
    pub fn len(&self) -> usize {
        self.templates.len()
    }

    /// Check if the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.templates.is_empty()
    }

    /// Register a custom data type for JSON deserialization.
    ///
    /// When loading items from JSON, the `data_type` field is matched against
    /// registered type names, and the corresponding factory is called to
    /// construct the `ItemData` trait object.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// registry.register_data_type::<WeaponStats>("weapon_stats", |value| {
    ///     serde_json::from_value::<WeaponStats>(value.clone()).ok().map(|s| Box::new(s) as _)
    /// });
    /// ```
    pub fn register_data_type(
        &mut self,
        type_name: &str,
        factory: impl Fn(&serde_json::Value) -> Option<Box<dyn ItemData>> + Send + Sync + 'static,
    ) {
        self.data_type_factories.insert(type_name.to_string(), Box::new(factory));
    }

    /// Load item templates from a JSON file.
    ///
    /// The JSON should be an array of objects with fields matching `ItemTemplate`:
    ///
    /// ```json
    /// [
    ///   {
    ///     "name": "iron_sword",
    ///     "display_name": "Iron Sword",
    ///     "description": "A trusty blade",
    ///     "max_stack": 1,
    ///     "tags": ["weapon", "melee"],
    ///     "data_type": "weapon_stats",
    ///     "data": { "damage": 10.0, "attack_speed": 1.0 }
    ///   }
    /// ]
    /// ```
    pub fn load_from_file(&mut self, path: &str) -> Result<Vec<ItemId>, InventoryError> {
        let contents = std::fs::read_to_string(path)?;
        self.load_from_json(&contents)
    }

    /// Load item templates from a JSON string.
    pub fn load_from_json(&mut self, json: &str) -> Result<Vec<ItemId>, InventoryError> {
        let entries: Vec<serde_json::Value> = serde_json::from_str(json)?;
        let mut ids = Vec::new();

        for entry in entries {
            let name = entry["name"].as_str().unwrap_or("").to_string();
            let display_name = entry["display_name"].as_str().map(|s| s.to_string());
            let description = entry["description"].as_str().unwrap_or("").to_string();
            let max_stack = entry["max_stack"].as_u64().unwrap_or(1) as u32;

            let mut builder = ItemTemplate::builder(&name);
            if let Some(dn) = display_name {
                builder = builder.display_name(&dn);
            }
            builder = builder.description(&description).max_stack(max_stack);

            if let Some(tags) = entry["tags"].as_array() {
                for tag in tags {
                    if let Some(t) = tag.as_str() {
                        builder = builder.tag(t);
                    }
                }
            }

            // Resolve custom data type
            let mut item_data: Option<Box<dyn ItemData>> = None;
            if let Some(type_name) = entry["data_type"].as_str() {
                if let Some(factory) = self.data_type_factories.get(type_name) {
                    item_data = factory(&entry["data"]);
                    if item_data.is_none() {
                        return Err(InventoryError::UnknownDataType {
                            type_name: type_name.to_string(),
                        });
                    }
                } else {
                    return Err(InventoryError::UnknownDataType {
                        type_name: type_name.to_string(),
                    });
                }
            }

            let mut template = builder.build();
            if let Some(data) = item_data {
                template.data = Some(data);
            }

            ids.push(self.register(template));
        }

        Ok(ids)
    }
}
```

Note: The `template.data` field needs to be `pub(crate)` in `item.rs` for the registry to set it during JSON loading.

Update `src/core/inventory/mod.rs`:

```rust
mod item;
mod registry;

pub use item::*;
pub use registry::*;
```

Update `src/lib.rs` prelude to add `ItemRegistry,`.

**Step 4: Run tests to verify they pass**

Run: `cargo test --test integration inventory`
Expected: PASS (14 tests)

**Step 5: Commit**

```
git add src/core/inventory/registry.rs src/core/inventory/mod.rs src/core/inventory/item.rs src/lib.rs tests/integration/inventory.rs
git commit -m "feat(inventory): add ItemRegistry with code and JSON loading"
```

---

## Task 5: Inventory Container

**Files:**
- Create: `src/core/inventory/container.rs`
- Modify: `src/core/inventory/mod.rs`
- Modify: `src/lib.rs`
- Modify: `tests/integration/inventory.rs`

**Step 1: Write the failing tests**

Append to `tests/integration/inventory.rs`:

```rust
// ============================================================================
// INVENTORY CONTAINER TESTS
// ============================================================================

fn setup_registry() -> (ItemRegistry, ItemId, ItemId, ItemId) {
    let mut registry = ItemRegistry::new();
    let sword = registry.register(ItemTemplate::builder("sword").max_stack(1).build());
    let potion = registry.register(ItemTemplate::builder("potion").max_stack(10).build());
    let arrow = registry.register(ItemTemplate::builder("arrow").max_stack(64).build());
    (registry, sword, potion, arrow)
}

#[test]
fn inventory_add_to_empty() {
    let (registry, _, potion, _) = setup_registry();
    let mut inv = Inventory::new(5);
    inv.add(ItemStack::new(potion, 3), &registry).unwrap();
    assert_eq!(inv.count(potion), 3);
    assert_eq!(inv.empty_slots(), 4);
}

#[test]
fn inventory_add_stacks() {
    let (registry, _, potion, _) = setup_registry();
    let mut inv = Inventory::new(5);
    inv.add(ItemStack::new(potion, 3), &registry).unwrap();
    inv.add(ItemStack::new(potion, 5), &registry).unwrap();
    assert_eq!(inv.count(potion), 8);
    assert_eq!(inv.empty_slots(), 4); // stacked into same slot
}

#[test]
fn inventory_add_overflow_stack() {
    let (registry, _, potion, _) = setup_registry();
    let mut inv = Inventory::new(5);
    inv.add(ItemStack::new(potion, 8), &registry).unwrap();
    inv.add(ItemStack::new(potion, 5), &registry).unwrap();
    // 8 + 5 = 13, max_stack 10, so should be 10 + 3 across 2 slots
    assert_eq!(inv.count(potion), 13);
    assert_eq!(inv.empty_slots(), 3);
}

#[test]
fn inventory_add_full() {
    let (registry, sword, _, _) = setup_registry();
    let mut inv = Inventory::new(2);
    inv.add(ItemStack::new(sword, 1), &registry).unwrap();
    inv.add(ItemStack::new(sword, 1), &registry).unwrap();
    let result = inv.add(ItemStack::new(sword, 1), &registry);
    assert!(result.is_err());
}

#[test]
fn inventory_remove() {
    let (registry, _, potion, _) = setup_registry();
    let mut inv = Inventory::new(5);
    inv.add(ItemStack::new(potion, 5), &registry).unwrap();
    let removed = inv.remove(0, 3).unwrap();
    assert_eq!(removed.quantity(), 3);
    assert_eq!(inv.count(potion), 2);
}

#[test]
fn inventory_remove_all() {
    let (registry, _, potion, _) = setup_registry();
    let mut inv = Inventory::new(5);
    inv.add(ItemStack::new(potion, 5), &registry).unwrap();
    inv.remove(0, 5).unwrap();
    assert_eq!(inv.count(potion), 0);
    assert_eq!(inv.empty_slots(), 5);
}

#[test]
fn inventory_remove_empty_slot() {
    let inv = Inventory::new(5);
    let result = inv.remove(0, 1);
    assert!(result.is_err());
}

#[test]
fn inventory_get_slot() {
    let (registry, _, potion, _) = setup_registry();
    let mut inv = Inventory::new(5);
    inv.add(ItemStack::new(potion, 3), &registry).unwrap();
    let slot = inv.get(0).unwrap();
    assert_eq!(slot.item_id(), potion);
    assert_eq!(slot.quantity(), 3);
}

#[test]
fn inventory_swap() {
    let (registry, sword, potion, _) = setup_registry();
    let mut inv = Inventory::new(5);
    inv.add(ItemStack::new(sword, 1), &registry).unwrap();
    inv.add(ItemStack::new(potion, 5), &registry).unwrap();
    inv.swap(0, 1);
    assert_eq!(inv.get(0).unwrap().item_id(), potion);
    assert_eq!(inv.get(1).unwrap().item_id(), sword);
}

#[test]
fn inventory_has() {
    let (registry, _, potion, _) = setup_registry();
    let mut inv = Inventory::new(5);
    inv.add(ItemStack::new(potion, 3), &registry).unwrap();
    assert!(inv.has(potion, 3));
    assert!(inv.has(potion, 1));
    assert!(!inv.has(potion, 4));
}

#[test]
fn inventory_find() {
    let (registry, sword, potion, _) = setup_registry();
    let mut inv = Inventory::new(5);
    inv.add(ItemStack::new(sword, 1), &registry).unwrap();
    inv.add(ItemStack::new(potion, 3), &registry).unwrap();
    assert_eq!(inv.find(potion), Some(1));
}

#[test]
fn inventory_compact() {
    let (registry, _, potion, _) = setup_registry();
    let mut inv = Inventory::new(5);
    // Manually place partial stacks in non-adjacent slots
    inv.set_slot(0, Some(ItemStack::new(potion, 4)));
    inv.set_slot(2, Some(ItemStack::new(potion, 4)));
    inv.compact(&registry);
    // Should merge into one slot of 8 (max_stack 10)
    assert_eq!(inv.count(potion), 8);
    // One of the slots should now be empty
    let occupied: usize = (0..5).filter(|i| inv.get(*i).is_some()).count();
    assert_eq!(occupied, 1);
}

#[test]
fn inventory_is_full() {
    let (registry, sword, _, _) = setup_registry();
    let mut inv = Inventory::new(2);
    assert!(!inv.is_full());
    inv.add(ItemStack::new(sword, 1), &registry).unwrap();
    assert!(!inv.is_full());
    inv.add(ItemStack::new(sword, 1), &registry).unwrap();
    assert!(inv.is_full());
}

#[test]
fn inventory_transfer() {
    let (registry, _, potion, _) = setup_registry();
    let mut from = Inventory::new(5);
    let mut to = Inventory::new(5);
    from.add(ItemStack::new(potion, 5), &registry).unwrap();
    Inventory::transfer(&mut from, 0, &mut to, None, 3, &registry).unwrap();
    assert_eq!(from.count(potion), 2);
    assert_eq!(to.count(potion), 3);
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --test integration inventory::inventory_`
Expected: FAIL — `Inventory` not defined

**Step 3: Implement Inventory container**

Create `src/core/inventory/container.rs` with the `Inventory` struct implementing:
- `new(capacity)`, `add(stack, registry)`, `remove(slot, qty)`, `get(slot)`, `get_mut(slot)`
- `swap(a, b)`, `count(id)`, `has(id, qty)`, `find(id)`, `is_full()`, `empty_slots()`
- `compact(registry)`, `set_slot(idx, option)`, `capacity()`
- `transfer(from, from_slot, to, to_slot, qty, registry)` as associated function

The `add` method fills existing partial stacks first, then spills to empty slots. Returns `Err(InventoryError::InventoryFull)` if not everything fits.

Update `mod.rs` to add `mod container; pub use container::*;`

Update `src/lib.rs` prelude to add `Inventory,`.

**Step 4: Run tests to verify they pass**

Run: `cargo test --test integration inventory`
Expected: PASS (all container tests)

**Step 5: Commit**

```
git add src/core/inventory/container.rs src/core/inventory/mod.rs src/lib.rs tests/integration/inventory.rs
git commit -m "feat(inventory): add Inventory container with stacking and transfer"
```

---

## Task 6: Equipment System

**Files:**
- Create: `src/core/inventory/equipment.rs`
- Modify: `src/core/inventory/mod.rs`
- Modify: `src/lib.rs`
- Modify: `tests/integration/inventory.rs`

**Step 1: Write the failing tests**

Append to `tests/integration/inventory.rs`:

```rust
// ============================================================================
// EQUIPMENT TESTS
// ============================================================================

#[test]
fn equipment_equip_and_get() {
    let mut equip = Equipment::new(4);
    let stack = ItemStack::new(ItemId::new(0), 1);
    let prev = equip.equip(0, stack.clone(), None).unwrap();
    assert!(prev.is_none());
    assert_eq!(equip.get(0).unwrap().item_id(), ItemId::new(0));
}

#[test]
fn equipment_equip_replaces() {
    let mut equip = Equipment::new(4);
    let sword = ItemStack::new(ItemId::new(0), 1);
    let axe = ItemStack::new(ItemId::new(1), 1);
    equip.equip(0, sword, None).unwrap();
    let prev = equip.equip(0, axe, None).unwrap();
    assert!(prev.is_some());
    assert_eq!(prev.unwrap().item_id(), ItemId::new(0));
}

#[test]
fn equipment_unequip() {
    let mut equip = Equipment::new(4);
    equip.equip(0, ItemStack::new(ItemId::new(0), 1), None).unwrap();
    let removed = equip.unequip(0).unwrap();
    assert_eq!(removed.item_id(), ItemId::new(0));
    assert!(equip.is_slot_empty(0));
}

#[test]
fn equipment_unequip_empty() {
    let mut equip = Equipment::new(4);
    let result = equip.unequip(0);
    assert!(result.is_err());
}

#[test]
fn equipment_out_of_bounds() {
    let mut equip = Equipment::new(2);
    let result = equip.equip(5, ItemStack::new(ItemId::new(0), 1), None);
    assert!(result.is_err());
}

#[test]
fn equipment_equipped_items_iterator() {
    let mut equip = Equipment::new(4);
    equip.equip(0, ItemStack::new(ItemId::new(0), 1), None).unwrap();
    equip.equip(2, ItemStack::new(ItemId::new(1), 1), None).unwrap();
    let items: Vec<_> = equip.equipped_items().collect();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].0, 0);
    assert_eq!(items[1].0, 2);
}

#[test]
fn equipment_validator_rejects() {
    struct WeaponsOnlySlot0;
    impl EquipValidator for WeaponsOnlySlot0 {
        fn can_equip(&self, slot: usize, _item: &ItemStack, registry: &ItemRegistry) -> bool {
            if slot == 0 {
                // Only allow items tagged "weapon"
                if let Some(template) = registry.get(_item.item_id()) {
                    return template.has_tag("weapon");
                }
            }
            true
        }
    }

    let mut registry = ItemRegistry::new();
    let sword = registry.register(ItemTemplate::builder("sword").tag("weapon").build());
    let potion = registry.register(ItemTemplate::builder("potion").tag("consumable").build());

    let mut equip = Equipment::new(4);
    equip.set_validator(Box::new(WeaponsOnlySlot0));

    // Sword in slot 0 should work
    assert!(equip.equip(0, ItemStack::new(sword, 1), Some(&registry)).is_ok());
    equip.unequip(0).unwrap();

    // Potion in slot 0 should fail
    let result = equip.equip(0, ItemStack::new(potion, 1), Some(&registry));
    assert!(result.is_err());

    // Potion in slot 1 should work (validator only checks slot 0)
    assert!(equip.equip(1, ItemStack::new(potion, 1), Some(&registry)).is_ok());
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --test integration inventory::equipment`
Expected: FAIL — `Equipment` not defined

**Step 3: Implement Equipment**

Create `src/core/inventory/equipment.rs` with:
- `Equipment` struct with `slots: Vec<Option<ItemStack>>` and `validator: Option<Box<dyn EquipValidator>>`
- `new(slot_count)`, `equip(slot, item, registry)`, `unequip(slot)`, `get(slot)`
- `is_slot_empty(slot)`, `equipped_items()`, `slot_count()`, `set_validator(validator)`
- `EquipValidator` trait with `can_equip(slot, item, registry) -> bool`
- `equip` calls validator if present and registry is provided; returns `Err(ValidationFailed)` if rejected
- `equip` returns `Ok(Option<ItemStack>)` — the previously equipped item, if any

Update `mod.rs`, `lib.rs` prelude.

**Step 4: Run tests to verify they pass**

Run: `cargo test --test integration inventory`
Expected: PASS (all equipment tests)

**Step 5: Commit**

```
git add src/core/inventory/equipment.rs src/core/inventory/mod.rs src/lib.rs tests/integration/inventory.rs
git commit -m "feat(inventory): add Equipment with optional EquipValidator"
```

---

## Task 7: Loot Tables

**Files:**
- Create: `src/core/inventory/loot.rs`
- Modify: `src/core/inventory/mod.rs`
- Modify: `src/lib.rs`
- Modify: `tests/integration/inventory.rs`

**Step 1: Write the failing tests**

Append to `tests/integration/inventory.rs`:

```rust
// ============================================================================
// LOOT TABLE TESTS
// ============================================================================

#[test]
fn loot_table_roll_returns_item() {
    let mut registry = ItemRegistry::new();
    let sword = registry.register(ItemTemplate::builder("sword").build());

    let table = LootTable::new()
        .entry(sword, 1.0, 1..=1);

    let mut rng = rand::thread_rng();
    let result = table.roll(&mut rng);
    assert!(result.is_some());
    let stack = result.unwrap();
    assert_eq!(stack.item_id(), sword);
    assert_eq!(stack.quantity(), 1);
}

#[test]
fn loot_table_nothing_weight() {
    let mut registry = ItemRegistry::new();
    let sword = registry.register(ItemTemplate::builder("sword").build());

    // 100% nothing chance
    let table = LootTable::new()
        .nothing(1.0);

    let mut rng = rand::thread_rng();
    // With only nothing weight, should always return None
    let results: Vec<_> = (0..20).map(|_| table.roll(&mut rng)).collect();
    assert!(results.iter().all(|r| r.is_none()));
}

#[test]
fn loot_table_roll_multiple() {
    let mut registry = ItemRegistry::new();
    let potion = registry.register(ItemTemplate::builder("potion").build());

    let table = LootTable::new()
        .entry(potion, 1.0, 1..=3);

    let mut rng = rand::thread_rng();
    let results = table.roll_multiple(5, &mut rng);
    assert_eq!(results.len(), 5);
    for stack in &results {
        assert_eq!(stack.item_id(), potion);
        assert!(stack.quantity() >= 1 && stack.quantity() <= 3);
    }
}

#[test]
fn loot_table_quantity_range() {
    let mut registry = ItemRegistry::new();
    let arrow = registry.register(ItemTemplate::builder("arrow").build());

    let table = LootTable::new()
        .entry(arrow, 1.0, 5..=10);

    let mut rng = rand::thread_rng();
    for _ in 0..50 {
        let result = table.roll(&mut rng).unwrap();
        assert!(result.quantity() >= 5 && result.quantity() <= 10);
    }
}

#[test]
fn loot_table_weighted_distribution() {
    let mut registry = ItemRegistry::new();
    let common = registry.register(ItemTemplate::builder("common").build());
    let rare = registry.register(ItemTemplate::builder("rare").build());

    // 90% common, 10% rare
    let table = LootTable::new()
        .entry(common, 0.9, 1..=1)
        .entry(rare, 0.1, 1..=1);

    let mut rng = rand::thread_rng();
    let mut common_count = 0;
    let mut rare_count = 0;
    for _ in 0..1000 {
        let result = table.roll(&mut rng).unwrap();
        if result.item_id() == common {
            common_count += 1;
        } else {
            rare_count += 1;
        }
    }
    // Common should be roughly 900, rare roughly 100
    assert!(common_count > 700, "Common count too low: {common_count}");
    assert!(rare_count > 30, "Rare count too low: {rare_count}");
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --test integration inventory::loot`
Expected: FAIL — `LootTable` not defined

**Step 3: Implement LootTable**

Create `src/core/inventory/loot.rs` with:
- `LootEntry { item_id: ItemId, weight: f64, quantity: RangeInclusive<u32> }`
- `LootTable` with `entries: Vec<LootEntry>`, `total_weight: f64`, `nothing_weight: f64`
- Builder methods: `new()`, `entry(id, weight, qty_range)`, `nothing(weight)`
- `roll(rng) -> Option<ItemStack>` — weighted random selection
- `roll_multiple(count, rng) -> Vec<ItemStack>` — rolls that returned `None` are excluded

Use `rand::Rng` for the `rng` parameter.

Update `mod.rs`, `lib.rs` prelude.

**Step 4: Run tests to verify they pass**

Run: `cargo test --test integration inventory`
Expected: PASS (all loot tests)

**Step 5: Commit**

```
git add src/core/inventory/loot.rs src/core/inventory/mod.rs src/lib.rs tests/integration/inventory.rs
git commit -m "feat(inventory): add LootTable with weighted randomization"
```

---

## Task 8: World Integration (ItemDrop + Events)

**Files:**
- Create: `src/core/inventory/world.rs`
- Modify: `src/core/inventory/mod.rs`
- Modify: `src/core/events/builtin.rs`
- Modify: `src/lib.rs`
- Modify: `tests/integration/inventory.rs`

**Step 1: Write the failing tests**

Append to `tests/integration/inventory.rs`:

```rust
// ============================================================================
// WORLD INTEGRATION TESTS
// ============================================================================

#[test]
fn spawn_item_drop_creates_entity() {
    let mut world = World::new();
    let stack = ItemStack::new(ItemId::new(0), 3);
    let entity = spawn_item_drop(&mut world, stack, (100.0, 200.0));

    // Entity should have Transform and ItemDrop
    assert!(world.get::<Transform>(entity).is_some());
    let drop = world.get::<ItemDrop>(entity).unwrap();
    assert_eq!(drop.item_stack.item_id(), ItemId::new(0));
    assert_eq!(drop.item_stack.quantity(), 3);
}

#[test]
fn pickup_item_removes_entity() {
    let mut world = World::new();
    let stack = ItemStack::new(ItemId::new(0), 1);
    let entity = spawn_item_drop(&mut world, stack, (0.0, 0.0));

    let picked_up = pickup_item(&mut world, entity).unwrap();
    assert_eq!(picked_up.item_id(), ItemId::new(0));
    assert_eq!(picked_up.quantity(), 1);

    // Entity should be despawned
    assert!(world.get::<ItemDrop>(entity).is_none());
}

#[test]
fn pickup_nonexistent_entity() {
    let mut world = World::new();
    let entity = world.spawn((Transform::default(),));
    world.despawn(entity).unwrap();
    let result = pickup_item(&mut world, entity);
    assert!(result.is_err());
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --test integration inventory::spawn`
Expected: FAIL — `spawn_item_drop`, `ItemDrop` not defined

**Step 3: Implement world integration**

Create `src/core/inventory/world.rs` with:
- `ItemDrop` component struct (holds `pub item_stack: ItemStack`)
- `spawn_item_drop(world, stack, position) -> Entity`
- `pickup_item(world, entity) -> Result<ItemStack, InventoryError>`

Add inventory events to `src/core/events/builtin.rs` (new section at the end):

```rust
// ============================================================================
// Inventory Events
// ============================================================================

/// Emitted when an item is picked up from the world.
#[derive(Debug, Clone)]
pub struct ItemPickedUp {
    pub entity: Entity,
    pub item_id: ItemId,
    pub quantity: u32,
}

/// Emitted when an item is dropped into the world.
#[derive(Debug, Clone)]
pub struct ItemDropped {
    pub entity: Entity,
    pub item_id: ItemId,
    pub quantity: u32,
}

/// Emitted when inventory contents change.
#[derive(Debug, Clone)]
pub struct InventoryChanged {
    pub item_id: ItemId,
    pub new_total: u32,
}

/// Emitted when an item is equipped.
#[derive(Debug, Clone)]
pub struct ItemEquipped {
    pub slot: usize,
    pub item_id: ItemId,
}

/// Emitted when an item is unequipped.
#[derive(Debug, Clone)]
pub struct ItemUnequipped {
    pub slot: usize,
    pub item_id: ItemId,
}
```

Note: The events module will need to import `ItemId` from the inventory module. Use `use crate::core::inventory::ItemId;`.

Update `mod.rs`, `lib.rs` prelude (add `ItemDrop`, `spawn_item_drop`, `pickup_item` and all event types).

**Step 4: Run tests to verify they pass**

Run: `cargo test --test integration inventory`
Expected: PASS (all world tests)

**Step 5: Commit**

```
git add src/core/inventory/world.rs src/core/inventory/mod.rs src/core/events/builtin.rs src/lib.rs tests/integration/inventory.rs
git commit -m "feat(inventory): add ItemDrop component, world helpers, and events"
```

---

## Task 9: JSON Loading Integration Test

**Files:**
- Modify: `tests/integration/inventory.rs`

**Step 1: Write JSON loading test**

Append to `tests/integration/inventory.rs`:

```rust
// ============================================================================
// JSON LOADING TESTS
// ============================================================================

#[test]
fn registry_load_from_json() {
    let mut registry = ItemRegistry::new();

    registry.register_data_type("weapon_stats", |value| {
        let damage = value["damage"].as_f64()?;
        let attack_speed = value["attack_speed"].as_f64()?;
        Some(Box::new(WeaponStats { damage, attack_speed }) as Box<dyn ItemData>)
    });

    let json = r#"[
        {
            "name": "iron_sword",
            "display_name": "Iron Sword",
            "description": "A trusty blade",
            "max_stack": 1,
            "tags": ["weapon", "melee"],
            "data_type": "weapon_stats",
            "data": { "damage": 10.0, "attack_speed": 1.0 }
        },
        {
            "name": "health_potion",
            "display_name": "Health Potion",
            "max_stack": 10,
            "tags": ["consumable"]
        }
    ]"#;

    let ids = registry.load_from_json(json).unwrap();
    assert_eq!(ids.len(), 2);

    let sword = registry.get_by_name("iron_sword").unwrap();
    assert_eq!(sword.display_name(), "Iron Sword");
    assert!(sword.has_tag("weapon"));
    let stats = sword.data().unwrap().as_any().downcast_ref::<WeaponStats>().unwrap();
    assert_eq!(stats.damage, 10.0);

    let potion = registry.get_by_name("health_potion").unwrap();
    assert_eq!(potion.max_stack(), 10);
    assert!(potion.has_tag("consumable"));
}

#[test]
fn registry_load_unknown_data_type() {
    let mut registry = ItemRegistry::new();
    let json = r#"[{ "name": "thing", "data_type": "unknown", "data": {} }]"#;
    let result = registry.load_from_json(json);
    assert!(result.is_err());
}
```

**Step 2: Run tests**

Run: `cargo test --test integration inventory::registry_load`
Expected: PASS

**Step 3: Commit**

```
git add tests/integration/inventory.rs
git commit -m "test(inventory): add JSON loading integration tests"
```

---

## Task 10: Documentation and AGENTS.md

**Files:**
- Create: `src/core/inventory/AGENTS.md`
- Create: `docs/src/concepts/inventory.md`
- Modify: `docs/src/SUMMARY.md`
- Modify: `src/core/AGENTS.md`
- Modify: `AGENTS.md`
- Modify: `README.md`

**Step 1: Create `src/core/inventory/AGENTS.md`**

Follow the pattern from `src/core/particles/AGENTS.md`. Include:
- Overview of the inventory module
- Module structure table (file → purpose)
- Core types: ItemId, ItemData, ItemTemplate, ItemStack, ItemRegistry, Inventory, Equipment, LootTable, ItemDrop
- Builder API examples
- JSON loading examples
- Event types table
- World integration patterns (spawn/pickup)
- Usage patterns for common scenarios
- Tips for AI agents

**Step 2: Create `docs/src/concepts/inventory.md`**

Follow the pattern from `docs/src/concepts/particles.md`. Include:
- Quick start example
- Core concepts explained
- Item registration (code + JSON)
- Inventory operations
- Equipment system
- Loot tables
- World integration
- Events

**Step 3: Update `docs/src/SUMMARY.md`**

Add after the Procedural Generation line (line 37, before Networking):

```markdown
- [Inventory System](./concepts/inventory.md)
```

**Step 4: Update `src/core/AGENTS.md`**

Add an `inventory/` entry to the Module Structure table:

```
| `inventory/` | Inventory & item system: registry, containers, equipment, loot (see `inventory/AGENTS.md`) |
```

**Step 5: Update root `AGENTS.md`**

Add to the "Nested AGENTS.md Files" table:

```
| `src/core/inventory/AGENTS.md` | Inventory & item system |
```

Add to the "Agent Skills" table:

```
| `ryo-inventory` | Inventory system - items, containers, equipment, loot |
```

**Step 6: Update `README.md`**

Add to the "Gameplay Systems" section (after Procedural Generation line ~124):

```markdown
- **Inventory System** — Item registry, slot-based containers, equipment, loot tables
```

**Step 7: Commit**

```
git add src/core/inventory/AGENTS.md docs/src/concepts/inventory.md docs/src/SUMMARY.md src/core/AGENTS.md AGENTS.md README.md
git commit -m "docs(inventory): add AGENTS.md, mdbook page, and update README"
```

---

## Task 11: Full Workspace Check

**Step 1: Run full check suite**

```bash
cargo check 2>&1
cargo clippy -- -D warnings 2>&1
cargo test --test integration inventory 2>&1
cargo test --lib 2>&1
```

All must pass with zero warnings.

**Step 2: Fix any issues**

Address any compilation errors, clippy warnings, or test failures.

**Step 3: Final commit (if fixes needed)**

```
git add -A
git commit -m "fix(inventory): address clippy warnings and test issues"
```

---

## Task 12: Update RESEARCH.md

**Files:**
- Modify: `RESEARCH.md`

**Step 1: Update the inventory entry status**

Change line 262 in the summary table from:

```
| 6 | Inventory System | Medium | Medium | Not Started |
```

To:

```
| 6 | Inventory System | Medium | Medium | **Core Complete** — registry, containers, equipment, loot tables |
```

Update the section 6 body to note what's implemented and what remains (UI, crafting, drag-and-drop).

**Step 2: Commit**

```
git add RESEARCH.md
git commit -m "docs: update RESEARCH.md inventory status"
```
