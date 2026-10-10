//! Integration tests for the Inventory & Item System

use ryo2d::prelude::*;
use std::any::Any;

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// ItemId tests
// ---------------------------------------------------------------------------

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
  let b = a; // Copy
  assert_eq!(a, b);
  // Both are still usable after copy
  assert_eq!(a.index(), 1);
  assert_eq!(b.index(), 1);
}

#[test]
fn item_id_debug_display() {
  let id = ItemId::new(42);
  assert_eq!(format!("{:?}", id), "ItemId(42)");
  assert_eq!(format!("{}", id), "item#42");
}

#[test]
fn item_id_hash() {
  use std::collections::HashSet;
  let mut set = HashSet::new();
  set.insert(ItemId::new(1));
  set.insert(ItemId::new(2));
  set.insert(ItemId::new(1)); // duplicate
  assert_eq!(set.len(), 2);
}

// ---------------------------------------------------------------------------
// ItemStack tests
// ---------------------------------------------------------------------------

#[test]
fn item_stack_creation() {
  let id = ItemId::new(1);
  let stack = ItemStack::new(id, 5);
  assert_eq!(stack.item_id(), id);
  assert_eq!(stack.quantity(), 5);
  assert!(stack.instance_data().is_none());
}

#[test]
fn item_stack_with_instance_data() {
  let id = ItemId::new(1);
  let stats = WeaponStats {
    damage: 25.0,
    attack_speed: 1.5,
  };
  let stack = ItemStack::new(id, 1).with_data(stats);

  assert!(stack.instance_data().is_some());

  let data = stack.instance_data().unwrap();
  let weapon = data.as_any().downcast_ref::<WeaponStats>().unwrap();
  assert_eq!(weapon.damage, 25.0);
  assert_eq!(weapon.attack_speed, 1.5);
}

#[test]
fn item_stack_clone_preserves_data() {
  let id = ItemId::new(1);
  let stats = WeaponStats {
    damage: 10.0,
    attack_speed: 2.0,
  };
  let original = ItemStack::new(id, 3).with_data(stats);
  let cloned = original.clone();

  assert_eq!(cloned.item_id(), id);
  assert_eq!(cloned.quantity(), 3);
  assert!(cloned.instance_data().is_some());

  let cloned_data = cloned
    .instance_data()
    .unwrap()
    .as_any()
    .downcast_ref::<WeaponStats>()
    .unwrap();
  assert_eq!(cloned_data.damage, 10.0);
  assert_eq!(cloned_data.attack_speed, 2.0);
}

#[test]
fn item_stack_set_quantity() {
  let id = ItemId::new(1);
  let mut stack = ItemStack::new(id, 5);
  assert_eq!(stack.quantity(), 5);
  stack.set_quantity(10);
  assert_eq!(stack.quantity(), 10);
}

#[test]
fn item_stack_instance_data_mut() {
  let id = ItemId::new(1);
  let stats = WeaponStats {
    damage: 10.0,
    attack_speed: 1.0,
  };
  let mut stack = ItemStack::new(id, 1).with_data(stats);

  // Mutate the instance data
  let data = stack.instance_data_mut().unwrap();
  let weapon = data.as_any_mut().downcast_mut::<WeaponStats>().unwrap();
  weapon.damage = 50.0;

  // Verify mutation persisted
  let data = stack.instance_data().unwrap();
  let weapon = data.as_any().downcast_ref::<WeaponStats>().unwrap();
  assert_eq!(weapon.damage, 50.0);
}

// ---------------------------------------------------------------------------
// ItemTemplate tests
// ---------------------------------------------------------------------------

#[test]
fn item_template_builder() {
  let template = ItemTemplate::builder("iron_sword")
    .display_name("Iron Sword")
    .description("A sturdy blade.")
    .max_stack(1)
    .tag("weapon")
    .tag("melee")
    .build();

  assert_eq!(template.name(), "iron_sword");
  assert_eq!(template.display_name(), "Iron Sword");
  assert_eq!(template.description(), "A sturdy blade.");
  assert_eq!(template.max_stack(), 1);
  assert!(template.has_tag("weapon"));
  assert!(template.has_tag("melee"));
  assert!(!template.has_tag("consumable"));
  // ID defaults to 0 before registry assignment
  assert_eq!(template.id(), ItemId::new(0));
}

#[test]
fn item_template_builder_defaults() {
  let template = ItemTemplate::builder("potion").build();

  assert_eq!(template.name(), "potion");
  // display_name defaults to internal name
  assert_eq!(template.display_name(), "potion");
  assert_eq!(template.description(), "");
  assert_eq!(template.max_stack(), 1);
  assert_eq!(template.id(), ItemId::new(0));
  assert!(template.data().is_none());
}

#[test]
fn item_template_with_data() {
  let stats = WeaponStats {
    damage: 30.0,
    attack_speed: 1.2,
  };
  let template = ItemTemplate::builder("magic_staff")
    .display_name("Magic Staff")
    .data(stats)
    .build();

  assert!(template.data().is_some());
  let data = template.data().unwrap();
  let weapon = data.as_any().downcast_ref::<WeaponStats>().unwrap();
  assert_eq!(weapon.damage, 30.0);
  assert_eq!(weapon.attack_speed, 1.2);
}

#[test]
fn item_template_stackable() {
  let template = ItemTemplate::builder("arrow")
    .display_name("Arrow")
    .max_stack(64)
    .build();

  assert_eq!(template.max_stack(), 64);
}

#[test]
fn item_template_tags_iteration() {
  let template = ItemTemplate::builder("shield")
    .tag("armor")
    .tag("shield")
    .tag("defensive")
    .build();

  let tags: Vec<&String> = template.tags().collect();
  assert_eq!(tags.len(), 3);
  assert!(template.has_tag("armor"));
  assert!(template.has_tag("shield"));
  assert!(template.has_tag("defensive"));
}

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
      .build(),
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
  registry.register(
    ItemTemplate::builder("sword")
      .tag("weapon")
      .tag("melee")
      .build(),
  );
  registry.register(
    ItemTemplate::builder("bow")
      .tag("weapon")
      .tag("ranged")
      .build(),
  );
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
fn inventory_add_full_leaves_inventory_unchanged() {
  let (registry, _, potion, _) = setup_registry();
  let mut inv = Inventory::new(1);
  inv.add(ItemStack::new(potion, 8), &registry).unwrap();
  let result = inv.add(ItemStack::new(potion, 5), &registry);
  assert!(matches!(
    result,
    Err(InventoryError::InventoryFull { remainder: 3 })
  ));
  assert_eq!(inv.count(potion), 8);
}

#[test]
fn inventory_add_partial_returns_leftover() {
  let (registry, _, potion, _) = setup_registry();
  let mut inv = Inventory::new(1);
  inv.add(ItemStack::new(potion, 8), &registry).unwrap();
  let leftover = inv
    .add_partial(ItemStack::new(potion, 5), &registry)
    .expect("3 potions should not fit");
  assert_eq!(leftover.item_id(), potion);
  assert_eq!(leftover.quantity(), 3);
  assert_eq!(inv.count(potion), 10);
}

#[test]
fn inventory_add_partial_all_fit() {
  let (registry, _, potion, _) = setup_registry();
  let mut inv = Inventory::new(5);
  assert!(
    inv
      .add_partial(ItemStack::new(potion, 5), &registry)
      .is_none()
  );
  assert_eq!(inv.count(potion), 5);
}

#[test]
fn inventory_add_partial_keeps_instance_data_on_leftover() {
  let (registry, sword, _, _) = setup_registry();
  let mut inv = Inventory::new(1);
  inv.add(ItemStack::new(sword, 1), &registry).unwrap();
  let stats = WeaponStats {
    damage: 10.0,
    attack_speed: 1.5,
  };
  let leftover = inv
    .add_partial(ItemStack::new(sword, 1).with_data(stats), &registry)
    .expect("inventory is full");
  assert!(leftover.instance_data().is_some());
  assert_eq!(inv.count(sword), 1);
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
  let mut inv = Inventory::new(5);
  let result = inv.remove(0, 1);
  assert!(result.is_err());
}

#[test]
fn inventory_remove_preserves_instance_data() {
  let (registry, sword, _, _) = setup_registry();
  let mut inv = Inventory::new(5);
  let stats = WeaponStats {
    damage: 25.0,
    attack_speed: 1.5,
  };
  inv
    .add(ItemStack::new(sword, 1).with_data(stats), &registry)
    .unwrap();
  let removed = inv.remove(0, 1).unwrap();
  assert!(removed.instance_data().is_some());
  let data = removed
    .instance_data()
    .unwrap()
    .as_any()
    .downcast_ref::<WeaponStats>()
    .unwrap();
  assert_eq!(data.damage, 25.0);
}

fn brewed_potion(potion: ItemId, quantity: u32) -> ItemStack {
  ItemStack::new(potion, quantity).with_data(ArmorStats { defense: 7.0 })
}

fn has_data(stack: &ItemStack) -> bool {
  stack.instance_data().is_some()
}

#[test]
fn plain_items_do_not_merge_into_a_stack_with_data() {
  let (registry, _, potion, _) = setup_registry();
  let mut inv = Inventory::new(5);
  inv.add(brewed_potion(potion, 1), &registry).unwrap();

  inv.add(ItemStack::new(potion, 2), &registry).unwrap();

  let first = inv.get(0).unwrap();
  assert_eq!(first.quantity(), 1);
  assert!(has_data(first));
  let second = inv.get(1).unwrap();
  assert_eq!(second.quantity(), 2);
  assert!(!has_data(second));
}

#[test]
fn space_in_a_stack_with_data_is_not_room_for_plain_items() {
  let (registry, _, potion, _) = setup_registry();
  let mut inv = Inventory::new(1);
  inv.add(brewed_potion(potion, 1), &registry).unwrap();

  let result = inv.add(ItemStack::new(potion, 1), &registry);

  assert!(matches!(result, Err(InventoryError::InventoryFull { remainder: 1 })));
  assert_eq!(inv.get(0).unwrap().quantity(), 1);
}

#[test]
fn partial_remove_keeps_instance_data_on_both_parts() {
  let (registry, _, potion, _) = setup_registry();
  let mut inv = Inventory::new(5);
  inv.add(brewed_potion(potion, 3), &registry).unwrap();

  let removed = inv.remove(0, 1).unwrap();

  assert_eq!(removed.quantity(), 1);
  assert!(has_data(&removed));
  assert_eq!(inv.get(0).unwrap().quantity(), 2);
  assert!(has_data(inv.get(0).unwrap()));
}

#[test]
fn transfer_into_a_slot_does_not_mix_plain_and_data_stacks() {
  let (registry, _, potion, _) = setup_registry();

  // Plain items into a slot holding a stack with data
  let mut from = Inventory::new(2);
  from.add(ItemStack::new(potion, 2), &registry).unwrap();
  let mut to = Inventory::new(2);
  to.add(brewed_potion(potion, 1), &registry).unwrap();
  let result = Inventory::transfer(&mut from, 0, &mut to, Some(0), 2, &registry);
  assert!(result.is_err());
  assert_eq!(from.get(0).unwrap().quantity(), 2);
  assert!(!has_data(from.get(0).unwrap()));
  assert_eq!(to.get(0).unwrap().quantity(), 1);

  // A stack with data into a slot holding plain items
  let mut from = Inventory::new(2);
  from.add(brewed_potion(potion, 1), &registry).unwrap();
  let mut to = Inventory::new(2);
  to.add(ItemStack::new(potion, 2), &registry).unwrap();
  let result = Inventory::transfer(&mut from, 0, &mut to, Some(0), 1, &registry);
  assert!(result.is_err());
  assert!(has_data(from.get(0).unwrap()));
  assert_eq!(to.get(0).unwrap().quantity(), 2);
  assert!(!has_data(to.get(0).unwrap()));
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
  inv.swap(0, 1).unwrap();
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
  inv.set_slot(0, Some(ItemStack::new(potion, 4))).unwrap();
  inv.set_slot(2, Some(ItemStack::new(potion, 4))).unwrap();
  inv.compact(&registry);
  assert_eq!(inv.count(potion), 8);
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

#[test]
fn inventory_transfer_rejected_does_not_duplicate() {
  let (registry, _, potion, _) = setup_registry();
  let mut from = Inventory::new(5);
  let mut to = Inventory::new(1);
  from.add(ItemStack::new(potion, 5), &registry).unwrap();
  to.add(ItemStack::new(potion, 8), &registry).unwrap();
  let result = Inventory::transfer(&mut from, 0, &mut to, None, 5, &registry);
  assert!(result.is_err());
  assert_eq!(from.count(potion), 5);
  assert_eq!(to.count(potion), 8);
}

// ============================================================================
// EQUIPMENT TESTS
// ============================================================================

#[test]
fn equipment_equip_and_get() {
  let registry = ItemRegistry::new();
  let mut equip = Equipment::new(4);
  let stack = ItemStack::new(ItemId::new(0), 1);
  let prev = equip.equip(0, stack.clone(), &registry).unwrap();
  assert!(prev.is_none());
  assert_eq!(equip.get(0).unwrap().item_id(), ItemId::new(0));
}

#[test]
fn equipment_equip_replaces() {
  let registry = ItemRegistry::new();
  let mut equip = Equipment::new(4);
  let sword = ItemStack::new(ItemId::new(0), 1);
  let axe = ItemStack::new(ItemId::new(1), 1);
  equip.equip(0, sword, &registry).unwrap();
  let prev = equip.equip(0, axe, &registry).unwrap();
  assert!(prev.is_some());
  assert_eq!(prev.unwrap().item_id(), ItemId::new(0));
}

#[test]
fn equipment_unequip() {
  let registry = ItemRegistry::new();
  let mut equip = Equipment::new(4);
  equip
    .equip(0, ItemStack::new(ItemId::new(0), 1), &registry)
    .unwrap();
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
  let registry = ItemRegistry::new();
  let mut equip = Equipment::new(2);
  let result = equip.equip(5, ItemStack::new(ItemId::new(0), 1), &registry);
  assert!(result.is_err());
}

#[test]
fn equipment_equipped_items_iterator() {
  let registry = ItemRegistry::new();
  let mut equip = Equipment::new(4);
  equip
    .equip(0, ItemStack::new(ItemId::new(0), 1), &registry)
    .unwrap();
  equip
    .equip(2, ItemStack::new(ItemId::new(1), 1), &registry)
    .unwrap();
  let items: Vec<_> = equip.equipped_items().collect();
  assert_eq!(items.len(), 2);
  assert_eq!(items[0].0, 0);
  assert_eq!(items[1].0, 2);
}

#[test]
fn equipment_validator_rejects() {
  struct WeaponsOnlySlot0;
  impl EquipValidator for WeaponsOnlySlot0 {
    fn can_equip(&self, slot: usize, item: &ItemStack, registry: &ItemRegistry) -> bool {
      if slot == 0 {
        if let Some(template) = registry.get(item.item_id()) {
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
  assert!(equip.equip(0, ItemStack::new(sword, 1), &registry).is_ok());
  equip.unequip(0).unwrap();

  // Potion in slot 0 should fail
  let result = equip.equip(0, ItemStack::new(potion, 1), &registry);
  assert!(result.is_err());

  // Potion in slot 1 should work (validator only checks slot 0)
  assert!(equip.equip(1, ItemStack::new(potion, 1), &registry).is_ok());
}

// ============================================================================
// LOOT TABLE TESTS
// ============================================================================

#[test]
fn loot_table_roll_returns_item() {
  let mut registry = ItemRegistry::new();
  let sword = registry.register(ItemTemplate::builder("sword").build());

  let table = LootTable::new().entry(sword, 1.0, 1..=1);

  let mut rng = rand::thread_rng();
  let result = table.roll(&mut rng);
  assert!(result.is_some());
  let stack = result.unwrap();
  assert_eq!(stack.item_id(), sword);
  assert_eq!(stack.quantity(), 1);
}

#[test]
fn loot_table_nothing_weight() {
  // 100% nothing chance (no entries, only nothing weight)
  let table = LootTable::new().nothing(1.0);

  let mut rng = rand::thread_rng();
  let results: Vec<_> = (0..20).map(|_| table.roll(&mut rng)).collect();
  assert!(results.iter().all(|r| r.is_none()));
}

#[test]
fn loot_table_roll_multiple() {
  let mut registry = ItemRegistry::new();
  let potion = registry.register(ItemTemplate::builder("potion").build());

  let table = LootTable::new().entry(potion, 1.0, 1..=3);

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

  let table = LootTable::new().entry(arrow, 1.0, 5..=10);

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
  assert!(common_count > 700, "Common count too low: {common_count}");
  assert!(rare_count > 30, "Rare count too low: {rare_count}");
}

// ============================================================================
// WORLD INTEGRATION TESTS
// ============================================================================

#[test]
fn spawn_item_drop_creates_entity() {
  let mut world = World::new();
  let stack = ItemStack::new(ItemId::new(0), 3);
  let entity = spawn_item_drop(&mut world, stack, (100.0, 200.0));

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

// ============================================================================
// JSON LOADING TESTS
// ============================================================================

#[test]
fn registry_load_from_json() {
  let mut registry = ItemRegistry::new();

  registry.register_data_type("weapon_stats", |value| {
    let damage = value["damage"].as_f64()?;
    let attack_speed = value["attack_speed"].as_f64()?;
    Some(Box::new(WeaponStats {
      damage,
      attack_speed,
    }) as Box<dyn ItemData>)
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
  let stats = sword
    .data()
    .unwrap()
    .as_any()
    .downcast_ref::<WeaponStats>()
    .unwrap();
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
