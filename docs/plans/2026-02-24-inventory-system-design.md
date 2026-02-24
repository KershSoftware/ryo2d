# Inventory & Item System - Design

## Overview

Generic engine-level inventory system for Ryo2D. Provides item definitions, slot-based inventory containers, abstract equipment slots, and weighted loot tables. Designed as a reusable subsystem that game developers can extend with their own item properties via a trait-based extension pattern.

**Scope:** Core data model, ItemRegistry (code + JSON loading), Inventory container, Equipment with optional validation, LootTable, and world integration (hybrid value/entity model). No UI, no crafting, no drag-and-drop.

## Architecture: Standalone Module

New `src/core/inventory/` module following existing engine patterns (particles, physics, etc.). Self-contained subsystem exposed through the prelude.

### Module Structure

```
src/core/inventory/
├── mod.rs          — Module root, public re-exports
├── item.rs         — ItemId, ItemData trait, ItemTemplate, ItemStack, builder
├── registry.rs     — ItemRegistry (code + JSON loading, type registry)
├── container.rs    — Inventory container (slots, add/remove/transfer/compact)
├── equipment.rs    — Equipment slots, EquipValidator trait
├── loot.rs         — LootTable, LootEntry, weighted rolling
├── world.rs        — ItemDrop component, spawn/pickup helpers, events
└── error.rs        — InventoryError enum
```

## Core Data Model

### ItemId

Lightweight, copyable identifier for item templates:

```rust
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub struct ItemId(u32);
```

Internally a `u32` index into the registry.

### ItemData Trait

Extension point for game-dev custom properties:

```rust
pub trait ItemData: Any + Send + Sync {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn clone_box(&self) -> Box<dyn ItemData>;
}
```

Game devs implement this for their stat types:

```rust
#[derive(Clone)]
struct WeaponStats { damage: f64, attack_speed: f64 }
impl ItemData for WeaponStats { ... }
```

### ItemTemplate

Blueprint for an item type, stored in the registry:

```rust
pub struct ItemTemplate {
    pub id: ItemId,
    pub name: String,
    pub description: String,
    pub max_stack: u32,           // 1 = non-stackable
    pub tags: HashSet<String>,    // "weapon", "consumable", "quest", etc.
    pub data: Option<Box<dyn ItemData>>,
}
```

### ItemStack

Value type representing items in an inventory slot:

```rust
#[derive(Clone)]
pub struct ItemStack {
    pub item_id: ItemId,
    pub quantity: u32,
    pub instance_data: Option<Box<dyn ItemData>>,  // Per-instance overrides
}
```

`instance_data` allows individual stacks to have unique state (enchantments, durability) while referencing the template for base stats.

## ItemRegistry

Central catalog of all item templates. Lives in `Resources` for cross-scene access.

```rust
pub struct ItemRegistry {
    templates: Vec<ItemTemplate>,
    name_index: HashMap<String, ItemId>,
}

impl ItemRegistry {
    pub fn new() -> Self;
    pub fn register(&mut self, template: ItemTemplate) -> ItemId;
    pub fn load_from_file(&mut self, path: &str) -> Result<Vec<ItemId>, InventoryError>;
    pub fn get(&self, id: ItemId) -> Option<&ItemTemplate>;
    pub fn get_by_name(&self, name: &str) -> Option<&ItemTemplate>;
    pub fn find_by_tag(&self, tag: &str) -> Vec<&ItemTemplate>;
}
```

### Builder API (code-defined items)

```rust
let sword_id = registry.register(
    ItemTemplate::builder("iron_sword")
        .display_name("Iron Sword")
        .description("A trusty blade")
        .max_stack(1)
        .tag("weapon").tag("melee")
        .data(WeaponStats { damage: 10.0, attack_speed: 1.0 })
        .build()
);
```

### JSON Loading (data-driven items)

Game devs register custom data types for deserialization, then load from JSON files:

```rust
registry.register_data_type::<WeaponStats>("weapon_stats");
registry.load_from_file("assets/items/weapons.json")?;
```

JSON uses the same `ItemTemplate` structure. The `ItemData` field references a registered type name for reconstruction.

## Inventory Container

Generic slot-based container:

```rust
pub struct Inventory {
    slots: Vec<Option<ItemStack>>,
    capacity: usize,
}

impl Inventory {
    pub fn new(capacity: usize) -> Self;
    pub fn add(&mut self, stack: ItemStack, registry: &ItemRegistry) -> Result<(), AddError>;
    pub fn remove(&mut self, slot: usize, quantity: u32) -> Result<ItemStack, InventoryError>;
    pub fn get(&self, slot: usize) -> Option<&ItemStack>;
    pub fn get_mut(&mut self, slot: usize) -> Option<&mut ItemStack>;
    pub fn transfer(
        from: &mut Inventory, from_slot: usize,
        to: &mut Inventory, to_slot: Option<usize>,
        quantity: u32, registry: &ItemRegistry,
    ) -> Result<(), InventoryError>;
    pub fn swap(&mut self, slot_a: usize, slot_b: usize);
    pub fn count(&self, item_id: ItemId) -> u32;
    pub fn has(&self, item_id: ItemId, qty: u32) -> bool;
    pub fn find(&self, item_id: ItemId) -> Option<usize>;
    pub fn is_full(&self) -> bool;
    pub fn empty_slots(&self) -> usize;
    pub fn compact(&mut self, registry: &ItemRegistry);
}
```

**`add` behavior:** Fills existing partial stacks first (respecting `max_stack`), then spills remainder into empty slots. Returns `AddError::Full { remainder }` if not everything fits.

## Equipment System

Abstract indexed slots with optional validation:

```rust
pub struct Equipment {
    slots: Vec<Option<ItemStack>>,
}

impl Equipment {
    pub fn new(slot_count: usize) -> Self;
    pub fn equip(&mut self, slot: usize, item: ItemStack) -> Result<Option<ItemStack>, InventoryError>;
    pub fn unequip(&mut self, slot: usize) -> Result<ItemStack, InventoryError>;
    pub fn get(&self, slot: usize) -> Option<&ItemStack>;
    pub fn is_slot_empty(&self, slot: usize) -> bool;
    pub fn equipped_items(&self) -> impl Iterator<Item = (usize, &ItemStack)>;
    pub fn slot_count(&self) -> usize;
    pub fn set_validator(&mut self, validator: Box<dyn EquipValidator>);
}

pub trait EquipValidator: Send + Sync {
    fn can_equip(&self, slot: usize, item: &ItemStack, registry: &ItemRegistry) -> bool;
}
```

Slots are purely indexed — game devs define semantic meaning with constants:

```rust
const SLOT_HEAD: usize = 0;
const SLOT_WEAPON: usize = 2;
```

Validator is optional. Without one, any item fits any slot.

## Loot Tables

Weighted randomization using engine's existing seeded RNG:

```rust
pub struct LootEntry {
    pub item_id: ItemId,
    pub weight: f64,
    pub quantity: RangeInclusive<u32>,
}

pub struct LootTable {
    entries: Vec<LootEntry>,
    total_weight: f64,
    nothing_weight: f64,
}

impl LootTable {
    pub fn new() -> Self;
    pub fn entry(mut self, item_id: ItemId, weight: f64, quantity: RangeInclusive<u32>) -> Self;
    pub fn nothing(mut self, weight: f64) -> Self;
    pub fn roll(&self, rng: &mut impl Rng) -> Option<ItemStack>;
    pub fn roll_multiple(&self, count: usize, rng: &mut impl Rng) -> Vec<ItemStack>;
}
```

Loot tables can also be loaded from JSON files (item names resolved to `ItemId` at load time).

## World Integration (Hybrid Bridge)

Items in inventory are lightweight value types. Items in the world are ECS entities.

### ItemDrop Component

```rust
#[derive(Clone)]
pub struct ItemDrop {
    pub item_stack: ItemStack,
}
```

### Helper Functions

```rust
pub fn spawn_item_drop(world: &mut World, stack: ItemStack, position: (f32, f32)) -> Entity;
pub fn pickup_item(world: &mut World, entity: Entity) -> Result<ItemStack, InventoryError>;
pub fn query_item_drops(world: &World) -> Vec<(Entity, &ItemDrop)>;
```

Game devs add their own components (sprite, collision) after spawning. The engine provides the minimal bridge.

### Events

```rust
pub struct ItemPickedUp { pub entity: Entity, pub item_id: ItemId, pub quantity: u32 }
pub struct ItemDropped { pub entity: Entity, pub item_id: ItemId, pub quantity: u32 }
pub struct InventoryChanged { pub item_id: ItemId, pub new_total: u32 }
pub struct ItemEquipped { pub slot: usize, pub item_id: ItemId }
pub struct ItemUnequipped { pub slot: usize, pub item_id: ItemId }
```

Events are defined by the engine as shared vocabulary. Game devs emit them from game logic; UI, audio, and other systems react.

## Error Type

```rust
pub enum InventoryError {
    SlotOutOfBounds { slot: usize, capacity: usize },
    SlotEmpty { slot: usize },
    InventoryFull { remainder: ItemStack },
    ItemNotFound { item_id: ItemId },
    InvalidQuantity,
    UnknownDataType { type_name: String },
    IoError(std::io::Error),
}
```

## Prelude Exports

```rust
pub use inventory::{
    ItemId, ItemData, ItemTemplate, ItemStack,
    ItemRegistry, Inventory, Equipment,
    EquipValidator, LootTable, LootEntry,
    ItemDrop, ItemPickedUp, ItemDropped,
    InventoryChanged, ItemEquipped, ItemUnequipped,
    InventoryError,
    spawn_item_drop, pickup_item,
};
```

## Engine Subsystems Leveraged

| Subsystem | Usage |
|-----------|-------|
| ECS (World) | ItemDrop entities in world, spawn/despawn helpers |
| Resources | ItemRegistry + player Inventory as cross-scene singletons |
| Events | ItemPickedUp, ItemDropped, InventoryChanged, etc. |
| Save System | Tier 3 GameData for inventory persistence (serde) |
| Procgen RNG | Seeded randomization for loot table rolls |
| Assets | JSON item template loading via AssetManager patterns |

## Design Decisions

- **Hybrid value/entity model** — Industry standard. Items in inventory are data (fast serialize/sort/stack/transfer). Items in world are entities (position, sprite, collision).
- **Trait-based extensions** — `ItemData` trait gives game devs full Rust type safety for custom stats without engine prescribing anything.
- **Abstract equipment slots** — Indexed slots with no engine-prescribed meaning. Game devs define semantics. Optional `EquipValidator` for equip rules.
- **JSON over RON** — Consistent with existing codebase precedent (`system.json`).
- **Registry pattern** — Accepts both code-defined and file-loaded items. Type registry for JSON deserialization of custom `ItemData`.

## Future Extensions (Not in Scope)

- Drag-and-drop UI for inventory management
- Crafting recipe system
- Item rarity/quality tiers
- Durability/condition system
- Shop/trading system
