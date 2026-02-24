# Inventory System

Ryo2D includes a generic inventory framework for items, containers, equipment, and loot tables. Items use a hybrid value/entity model: lightweight `ItemStack` values inside inventories, full ECS entities when dropped in the game world.

## Quick Start

```rust
use ryo2d::prelude::*;

// 1. Create a registry and define items
let mut registry = ItemRegistry::new();
let potion_id = registry.register(
    ItemTemplate::builder("health_potion")
        .display_name("Health Potion")
        .max_stack(10)
        .tag("consumable")
        .build()
);

// 2. Create an inventory and add items
let mut inv = Inventory::new(20);
inv.add(ItemStack::new(potion_id, 3), &registry).unwrap();
assert_eq!(inv.count(potion_id), 3);
```

## Core Concepts

The inventory system has four layers:

| Layer | Type | Purpose |
|-------|------|---------|
| **Registry** | `ItemRegistry` | Central catalog of item definitions |
| **Container** | `Inventory` | Slot-based storage with stacking |
| **Equipment** | `Equipment` | Indexed gear slots with validation |
| **Loot** | `LootTable` | Weighted random drop tables |

Items are defined once in the registry, then referenced by `ItemId` everywhere else. An `ItemStack` holds an `ItemId` plus a quantity.

## Item Registration

### From Code

```rust
let mut registry = ItemRegistry::new();

let sword_id = registry.register(
    ItemTemplate::builder("iron_sword")
        .display_name("Iron Sword")
        .description("A sturdy blade.")
        .max_stack(1)
        .tag("weapon")
        .tag("melee")
        .build()
);

let potion_id = registry.register(
    ItemTemplate::builder("health_potion")
        .display_name("Health Potion")
        .description("Restores 50 HP.")
        .max_stack(10)
        .tag("consumable")
        .build()
);
```

### From JSON

Define items in a JSON file for data-driven workflows:

```json
[
  {
    "name": "iron_sword",
    "display_name": "Iron Sword",
    "description": "A sturdy blade",
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
]
```

Load with optional custom data types:

```rust
// Register a factory for custom item data
registry.register_data_type("weapon_stats", |json| {
    let damage = json.get("damage")?.as_f64()?;
    let speed = json.get("attack_speed")?.as_f64()?;
    Some(Box::new(WeaponStats { damage, attack_speed: speed }))
});

// Load items from file
let ids = registry.load_from_file("assets/items.json")?;
```

Only the `name` field is required in JSON. All other fields have sensible defaults.

### Custom Item Data

Attach game-specific properties to items by implementing the `ItemData` trait:

```rust
#[derive(Clone, Debug)]
struct WeaponStats {
    damage: f64,
    attack_speed: f64,
}

impl ItemData for WeaponStats {
    fn as_any(&self) -> &dyn Any { self }
    fn as_any_mut(&mut self) -> &mut dyn Any { self }
    fn clone_box(&self) -> Box<dyn ItemData> { Box::new(self.clone()) }
}

// Attach to template (shared default)
let sword = ItemTemplate::builder("iron_sword")
    .data(WeaponStats { damage: 10.0, attack_speed: 1.0 })
    .build();

// Or attach per-instance on a stack
let stack = ItemStack::new(sword_id, 1)
    .with_data(WeaponStats { damage: 15.0, attack_speed: 0.8 });

// Read back via downcast
if let Some(stats) = stack.instance_data()
    .and_then(|d| d.as_any().downcast_ref::<WeaponStats>())
{
    println!("Damage: {}", stats.damage);
}
```

## Inventory Operations

### Adding Items

Items are automatically stacked into existing partial stacks before occupying new slots:

```rust
let mut inv = Inventory::new(20);

inv.add(ItemStack::new(potion_id, 3), &registry)?;
inv.add(ItemStack::new(potion_id, 5), &registry)?;

assert_eq!(inv.count(potion_id), 8);
assert_eq!(inv.empty_slots(), 19);  // Merged into one slot
```

### Removing Items

Remove a specific quantity from a slot:

```rust
let removed = inv.remove(0, 2)?;  // Remove 2 from slot 0
assert_eq!(removed.quantity(), 2);
```

### Querying

```rust
inv.get(slot)             // Peek at a slot
inv.count(item_id)        // Total quantity across all slots
inv.has(item_id, 5)       // Has at least 5?
inv.find(item_id)         // First slot containing item
inv.is_full()             // All slots occupied?
inv.empty_slots()         // Free slot count
```

### Organizing

```rust
// Swap two slots (drag-and-drop)
inv.swap(0, 5);

// Merge partial stacks to free up slots
inv.compact(&registry);
```

### Transferring Between Inventories

```rust
let mut player_inv = Inventory::new(20);
let mut chest = Inventory::new(10);

// Move 3 items from player slot 0 to chest (auto-placed)
Inventory::transfer(&mut player_inv, 0, &mut chest, None, 3, &registry)?;

// Move to specific chest slot
Inventory::transfer(&mut player_inv, 1, &mut chest, Some(0), 1, &registry)?;
```

Transfer is atomic: if the destination rejects the items, the source inventory is restored.

## Equipment System

Equipment provides indexed slots without prescribing what each slot means. Define your own slot semantics:

```rust
const SLOT_WEAPON: usize = 0;
const SLOT_ARMOR: usize = 1;
const SLOT_RING: usize = 2;
const SLOT_ACCESSORY: usize = 3;

let mut equip = Equipment::new(4);

// Equip an item (returns previously equipped item, if any)
let prev = equip.equip(SLOT_WEAPON, sword_stack, &registry)?;

// Check what's equipped
if let Some(item) = equip.get(SLOT_ARMOR) {
    println!("Wearing: {:?}", item.item_id());
}

// Unequip
let removed = equip.unequip(SLOT_WEAPON)?;

// Iterate equipped items
for (slot, item) in equip.equipped_items() {
    println!("Slot {}: {:?}", slot, item.item_id());
}
```

### Equipment Validation

Add slot restrictions with the `EquipValidator` trait:

```rust
struct RpgValidator;

impl EquipValidator for RpgValidator {
    fn can_equip(&self, slot: usize, item: &ItemStack, registry: &ItemRegistry) -> bool {
        let template = registry.get(item.item_id()).unwrap();
        match slot {
            0 => template.has_tag("weapon"),
            1 => template.has_tag("armor"),
            2 | 3 => template.has_tag("ring"),
            _ => false,
        }
    }
}

let mut equip = Equipment::new(4);
equip.set_validator(Box::new(RpgValidator));

// This will fail if the item doesn't have the "weapon" tag
let result = equip.equip(0, armor_stack, &registry);
assert!(result.is_err());
```

## Loot Tables

Define weighted drop tables for enemies, chests, or any randomized reward:

```rust
let goblin_loot = LootTable::new()
    .entry(gold_id, 1.0, 5..=20)       // Always drops 5-20 gold
    .entry(potion_id, 0.3, 1..=2)      // 30% chance of 1-2 potions
    .entry(rare_sword_id, 0.05, 1..=1) // 5% chance of rare sword
    .nothing(0.2);                       // 20% chance of nothing

let mut rng = rand::thread_rng();

// Roll once
if let Some(drop) = goblin_loot.roll(&mut rng) {
    inv.add(drop, &registry)?;
}

// Roll multiple times
let drops = goblin_loot.roll_multiple(3, &mut rng);
for drop in drops {
    inv.add(drop, &registry)?;
}
```

Weights are relative: an entry with weight `2.0` is twice as likely as one with weight `1.0`.

## World Integration

Items transition between inventory (value types) and world (ECS entities) via `ItemDrop`:

### Dropping Items

```rust
// Remove from inventory
let stack = inv.remove(slot, 1)?;

// Spawn as world entity
let entity = spawn_item_drop(&mut world, stack, (player_x, player_y));

// Add visual components
world.insert_one(entity, SpriteRenderer::new(item_sprite));
world.insert_one(entity, Collider::circle(8.0).as_trigger());
```

### Picking Up Items

```rust
// Despawns the entity and returns the ItemStack
let stack = pickup_item(&mut world, entity)?;

// Add to inventory
inv.add(stack, &registry)?;
```

## Events

The engine defines five inventory-related events for UI updates and game logic:

| Event | Fields | When |
|-------|--------|------|
| `ItemPickedUp` | `entity`, `item_id`, `quantity` | Item picked up from world |
| `ItemDropped` | `entity`, `item_id`, `quantity` | Item dropped into world |
| `InventoryChanged` | `item_id`, `new_total` | Inventory contents changed |
| `ItemEquipped` | `slot`, `item_id` | Item equipped to slot |
| `ItemUnequipped` | `slot`, `item_id` | Item removed from slot |

Emit events after inventory operations to keep UI and other systems in sync:

```rust
// After picking up an item
let stack = pickup_item(&mut world, entity)?;
inv.add(stack.clone(), &registry)?;
ctx.events.emit(ItemPickedUp {
    entity,
    item_id: stack.item_id(),
    quantity: stack.quantity(),
});
ctx.events.emit(InventoryChanged {
    item_id: stack.item_id(),
    new_total: inv.count(stack.item_id()),
});
```

## Complete Example

```rust
use ryo2d::prelude::*;

struct GameScene {
    registry: ItemRegistry,
    player_inv: Inventory,
    equipment: Equipment,
}

impl Scene for GameScene {
    fn on_enter(&mut self, ctx: &mut SceneContext) {
        // Set up items
        let sword_id = self.registry.register(
            ItemTemplate::builder("iron_sword")
                .display_name("Iron Sword")
                .max_stack(1)
                .tag("weapon")
                .build()
        );
        let potion_id = self.registry.register(
            ItemTemplate::builder("health_potion")
                .display_name("Health Potion")
                .max_stack(10)
                .tag("consumable")
                .build()
        );

        // Give player starting items
        self.player_inv.add(ItemStack::new(sword_id, 1), &self.registry).unwrap();
        self.player_inv.add(ItemStack::new(potion_id, 5), &self.registry).unwrap();

        // Equip the sword
        let sword_stack = self.player_inv.remove(0, 1).unwrap();
        self.equipment.equip(0, sword_stack, &self.registry).unwrap();
    }

    fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
        SceneAction::Continue
    }
}
```

## Next Steps

- [ECS Architecture](./ecs.md) - Entity-Component-System fundamentals
- [Event Bus](./events.md) - Decoupled communication for inventory events
- [Save System](./save-system.md) - Persisting inventory state
