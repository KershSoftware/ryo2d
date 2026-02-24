# AGENTS.md - Inventory & Item System

## Overview

This module provides a generic inventory framework for games: an item registry for defining item types (via code or JSON), slot-based containers with automatic stacking, abstract equipment slots with optional validation, weighted loot tables for randomized drops, and an ECS bridge for items in the game world. Items use a hybrid value/entity model -- lightweight `ItemStack` values inside inventories, full ECS entities when dropped in the world.

## Module Structure

| File | Purpose |
|------|---------|
| `mod.rs` | Module exports and crate-level documentation |
| `item.rs` | `ItemId`, `ItemData` trait, `ItemStack`, `ItemTemplate`, `ItemTemplateBuilder` |
| `registry.rs` | `ItemRegistry` — central catalog with name lookup, tag search, JSON loading |
| `container.rs` | `Inventory` — slot-based container with stacking, transfer, compact |
| `equipment.rs` | `Equipment`, `EquipValidator` trait — abstract indexed equipment slots |
| `loot.rs` | `LootTable`, `LootEntry` — weighted randomization for item drops |
| `world.rs` | `ItemDrop` component, `spawn_item_drop`, `pickup_item` — world bridge |

## Core Types

### ItemId

Lightweight `Copy` wrapper around `u32`. Two `ItemId` values are equal iff they refer to the same registered template.

```rust
ItemId::new(index)    // From raw u32 (prefer registry.register())
id.index()            // Raw u32 index
```

### ItemData Trait

Extension point for custom per-item-type or per-instance properties. Game code implements this on structs like `WeaponStats` or `PotionEffect`.

```rust
#[derive(Clone, Debug)]
struct WeaponStats { damage: f64, attack_speed: f64 }

impl ItemData for WeaponStats {
    fn as_any(&self) -> &dyn Any { self }
    fn as_any_mut(&mut self) -> &mut dyn Any { self }
    fn clone_box(&self) -> Box<dyn ItemData> { Box::new(self.clone()) }
}
```

Attach to `ItemTemplate` (shared defaults) or `ItemStack` (per-instance overrides). Downcast at runtime via `data.as_any().downcast_ref::<WeaponStats>()`.

### ItemStack

Value type representing a stack of identical items in an inventory slot. Cheap to move and clone. Never stored as an ECS entity on its own.

```rust
ItemStack::new(item_id, quantity)       // Create stack
stack.with_data(my_data)                // Attach per-instance data (builder-style)
stack.item_id()                         // Which item
stack.quantity()                        // How many
stack.set_quantity(n)                   // Update count
stack.instance_data()                   // Option<&dyn ItemData>
stack.instance_data_mut()               // Option<&mut dyn ItemData>
```

### ItemTemplate + Builder

Blueprint for an item type's static properties. Registered in the `ItemRegistry` and referenced by `ItemId`.

```rust
let sword = ItemTemplate::builder("iron_sword")
    .display_name("Iron Sword")
    .description("A sturdy blade.")
    .max_stack(1)
    .tag("weapon")
    .tag("melee")
    .data(WeaponStats { damage: 10.0, attack_speed: 1.0 })
    .build();
```

**Key methods:**
```rust
template.id()              // Registry-assigned ItemId
template.name()            // Internal name ("iron_sword")
template.display_name()    // Human-readable ("Iron Sword")
template.description()     // Tooltip text
template.max_stack()       // Max per slot (default 1)
template.has_tag(tag)      // Check tag
template.tags()            // Iterate tags
template.data()            // Option<&dyn ItemData>
```

### ItemRegistry

Central catalog that stores templates and maps names to `ItemId`s. Sequential IDs starting from 0, contiguous `Vec` storage, `HashMap` name index.

```rust
let mut registry = ItemRegistry::new();

// Register via code
let id = registry.register(
    ItemTemplate::builder("health_potion")
        .display_name("Health Potion")
        .max_stack(10)
        .tag("consumable")
        .build()
);

// Look up
registry.get(id)                       // Option<&ItemTemplate> by ID
registry.get_by_name("health_potion")  // Option<&ItemTemplate> by name
registry.id_of("health_potion")        // Option<ItemId>
registry.find_by_tag("consumable")     // Vec<&ItemTemplate>
registry.len()                         // Number of templates
registry.is_empty()                    // Whether empty

// JSON loading
registry.register_data_type("weapon_stats", |json| {
    let damage = json.get("damage")?.as_f64()?;
    let speed = json.get("attack_speed")?.as_f64()?;
    Some(Box::new(WeaponStats { damage, attack_speed: speed }))
});
registry.load_from_json(json_str)?;    // Parse JSON array
registry.load_from_file("items.json")?;// Load from disk
```

**JSON format:**
```json
[
  {
    "name": "iron_sword",
    "display_name": "Iron Sword",
    "description": "A trusty blade",
    "max_stack": 1,
    "tags": ["weapon", "melee"],
    "data_type": "weapon_stats",
    "data": { "damage": 10.0, "attack_speed": 1.0 }
  }
]
```

Only `name` is required. `data_type` triggers the registered factory for that type name.

### Inventory

Slot-based container with stacking, transfer, and compact operations. Fixed capacity. Items auto-stack up to `max_stack` from the registry.

```rust
let mut inv = Inventory::new(20);      // 20 slots

// Add/remove
inv.add(ItemStack::new(id, 5), &registry)?;  // Auto-stacks
inv.remove(slot, quantity)?;                  // Returns ItemStack

// Query
inv.get(slot)                          // Option<&ItemStack>
inv.get_mut(slot)                      // Option<&mut ItemStack>
inv.count(item_id)                     // Total quantity across all slots
inv.has(item_id, qty)                  // At least qty?
inv.find(item_id)                      // First slot index containing item
inv.is_full()                          // All slots occupied?
inv.empty_slots()                      // Number of free slots
inv.capacity()                         // Total slot count

// Organize
inv.swap(slot_a, slot_b);             // Swap two slots
inv.compact(&registry);               // Merge partial stacks
inv.set_slot(index, Some(stack));      // Direct slot access

// Transfer between inventories
Inventory::transfer(&mut from, from_slot, &mut to, to_slot, qty, &registry)?;
```

### Equipment + EquipValidator

Abstract indexed equipment slots. Game devs define slot semantics (e.g. `const SLOT_HEAD: usize = 0`). The optional `EquipValidator` trait adds restrictions without the engine prescribing rules.

```rust
let mut equip = Equipment::new(4);     // 4 slots

// Equip/unequip
let prev = equip.equip(slot, item, &registry)?;  // Ok(Option<prev_item>)
let item = equip.unequip(slot)?;

// Query
equip.get(slot)                        // Option<&ItemStack>
equip.is_slot_empty(slot)              // bool
equip.equipped_items()                 // Iterator<(usize, &ItemStack)>
equip.slot_count()                     // Total slots

// Validation
equip.set_validator(Box::new(my_validator));
```

**Validator example:**
```rust
struct RpgValidator;

impl EquipValidator for RpgValidator {
    fn can_equip(&self, slot: usize, item: &ItemStack, registry: &ItemRegistry) -> bool {
        let template = registry.get(item.item_id()).unwrap();
        match slot {
            0 => template.has_tag("weapon"),
            1 => template.has_tag("armor"),
            2 => template.has_tag("ring"),
            _ => false,
        }
    }
}
```

### LootTable

Weighted randomization for item drops. Builder pattern, supports "nothing" weight for empty rolls.

```rust
let table = LootTable::new()
    .entry(potion_id, 0.8, 1..=3)     // 80% weight, 1-3 potions
    .entry(rare_gem_id, 0.1, 1..=1)   // 10% weight, 1 gem
    .nothing(0.1);                     // 10% chance of no drop

let mut rng = rand::thread_rng();
let item: Option<ItemStack> = table.roll(&mut rng);
let items: Vec<ItemStack> = table.roll_multiple(5, &mut rng);
```

### ItemDrop Component

ECS component bridging inventory values and world entities. Items become entities with `Transform` + `ItemDrop` when dropped.

```rust
// Spawn a dropped item at position
let entity = spawn_item_drop(&mut world, stack, (100.0, 200.0));

// Pick up an item entity (despawns it, returns the ItemStack)
let stack = pickup_item(&mut world, entity)?;
```

Game devs can add additional components (sprite, collision) after spawning for visual representation.

## Events

| Event | Fields | Description |
|-------|--------|-------------|
| `ItemPickedUp` | `entity`, `item_id`, `quantity` | Item entity picked up from world |
| `ItemDropped` | `entity`, `item_id`, `quantity` | Item dropped into the world |
| `InventoryChanged` | `item_id`, `new_total` | Inventory contents changed |
| `ItemEquipped` | `slot`, `item_id` | Item equipped to slot |
| `ItemUnequipped` | `slot`, `item_id` | Item removed from slot |

Events are defined in `src/core/events/builtin.rs` and published via the engine's event bus.

## Usage Patterns

### Setting Up a Registry

```rust
let mut registry = ItemRegistry::new();

// Register items from code
let sword_id = registry.register(
    ItemTemplate::builder("iron_sword")
        .display_name("Iron Sword")
        .max_stack(1)
        .tag("weapon").tag("melee")
        .build()
);
let potion_id = registry.register(
    ItemTemplate::builder("health_potion")
        .display_name("Health Potion")
        .max_stack(10)
        .tag("consumable")
        .build()
);

// Or load from JSON
registry.load_from_file("assets/items.json")?;
```

### Adding/Removing Inventory Items

```rust
let mut inv = Inventory::new(20);

// Add stacks (auto-merges into existing partial stacks)
inv.add(ItemStack::new(potion_id, 3), &registry)?;
inv.add(ItemStack::new(potion_id, 5), &registry)?;
assert_eq!(inv.count(potion_id), 8);
assert_eq!(inv.empty_slots(), 19); // Stacked into one slot

// Remove from a specific slot
let removed = inv.remove(0, 2)?;
assert_eq!(removed.quantity(), 2);
assert_eq!(inv.count(potion_id), 6);

// Transfer between inventories
let mut chest = Inventory::new(10);
Inventory::transfer(&mut inv, 0, &mut chest, None, 3, &registry)?;
```

### Equipment with Validation

```rust
const SLOT_WEAPON: usize = 0;
const SLOT_ARMOR: usize = 1;
const SLOT_RING: usize = 2;

let mut equip = Equipment::new(3);
equip.set_validator(Box::new(RpgValidator));

// Equip a sword in weapon slot
let sword_stack = ItemStack::new(sword_id, 1);
equip.equip(SLOT_WEAPON, sword_stack, &registry)?;

// Trying to equip armor in weapon slot will fail
let armor_stack = ItemStack::new(armor_id, 1);
let result = equip.equip(SLOT_WEAPON, armor_stack, &registry);
assert!(result.is_err()); // ValidationFailed
```

### Loot Drops

```rust
let goblin_loot = LootTable::new()
    .entry(gold_id, 1.0, 5..=20)
    .entry(potion_id, 0.3, 1..=2)
    .entry(rare_sword_id, 0.05, 1..=1)
    .nothing(0.2);

let mut rng = rand::thread_rng();
let drops = goblin_loot.roll_multiple(3, &mut rng);
for drop in drops {
    inv.add(drop, &registry)?;
}
```

### World Pickup Flow

```rust
// Drop item from inventory into world
let removed = inv.remove(slot, 1)?;
let entity = spawn_item_drop(&mut world, removed, (player_x, player_y));
// Add visual components to the entity as needed

// Pick up item from world into inventory
let stack = pickup_item(&mut world, entity)?;
inv.add(stack, &registry)?;
```

## Tips for AI Agents

1. **Registry first** -- always register item templates before creating `ItemStack` values that reference them; the registry assigns valid `ItemId`s.
2. **Stacking is automatic** -- `Inventory::add` merges into existing partial stacks before using empty slots; you rarely need manual slot management.
3. **Equipment slots are abstract** -- define slot constants (`const SLOT_WEAPON: usize = 0`) in game code; the engine doesn't prescribe slot meanings.
4. **Validation is optional** -- `EquipValidator` is only needed when slot restrictions are required; omit it for simple equipment systems.
5. **ItemData is the extension point** -- attach custom gameplay data (damage, effects, durability) via `ItemData` on templates or stacks; downcast with `as_any().downcast_ref::<T>()`.
6. **JSON loading** -- register data type factories *before* calling `load_from_json`; the `data_type` field in JSON maps to registered factory names.
7. **Transfer is atomic** -- `Inventory::transfer` rolls back if the destination rejects the items, so the source inventory is never left in a broken state.
8. **Events are separate** -- the event types exist in `events/builtin.rs` but are not auto-emitted by the inventory structs; game code should emit them after operations for UI updates.
9. **World bridge** -- use `spawn_item_drop` / `pickup_item` for the inventory-to-world transition; add sprite/collider components to the spawned entity for visual representation.
10. **Compact sparingly** -- `Inventory::compact` rewrites all slots; call it on explicit user action (e.g. "sort" button), not every frame.
