//! Slot-based item container with stacking, transfer, and compact operations.

use super::item::{ItemId, ItemStack};
use super::registry::ItemRegistry;
use crate::core::error::InventoryError;

/// Slot-based item container with stacking, transfer, and compact operations.
///
/// `Inventory` holds a fixed number of slots, each of which may contain an
/// [`ItemStack`]. Items are automatically stacked up to the template's
/// `max_stack` when added, and partial stacks can be merged via [`compact`](Self::compact).
///
/// # Example
///
/// ```rust,ignore
/// use ryo2d::prelude::*;
///
/// let mut registry = ItemRegistry::new();
/// let potion = registry.register(
///     ItemTemplate::builder("health_potion").max_stack(10).build()
/// );
///
/// let mut inv = Inventory::new(20);
/// inv.add(ItemStack::new(potion, 3), &registry).unwrap();
/// inv.add(ItemStack::new(potion, 5), &registry).unwrap();
/// assert_eq!(inv.count(potion), 8);
/// assert_eq!(inv.empty_slots(), 19); // stacked into one slot
/// ```
pub struct Inventory {
  slots: Vec<Option<ItemStack>>,
  capacity: usize,
}

impl Inventory {
  /// Create a new inventory with `capacity` empty slots.
  pub fn new(capacity: usize) -> Self {
    let mut slots = Vec::with_capacity(capacity);
    slots.resize_with(capacity, || None);
    Self { slots, capacity }
  }

  /// The total number of slots (occupied + empty).
  pub fn capacity(&self) -> usize {
    self.capacity
  }

  /// Add an [`ItemStack`] to the inventory, respecting stacking rules.
  ///
  /// The method follows this logic:
  /// 1. Look up the item's `max_stack` from the registry (defaults to 1 if
  ///    the template is not found).
  /// 2. For stackable items (`max_stack > 1`) **without** per-instance data,
  ///    fill existing partial stacks of the same `item_id` up to `max_stack`.
  /// 3. Place any remaining quantity into the first available empty slot(s).
  /// 4. If the full quantity cannot be placed, return
  ///    [`InventoryError::InventoryFull`] with the unplaced remainder.
  ///
  /// Non-stackable items (`max_stack == 1`) always go directly to empty slots.
  ///
  /// **Note:** Items with [`instance_data`](ItemStack::instance_data) are never
  /// merged into existing stacks — they always occupy their own slot to preserve
  /// per-instance state (e.g. enchantments, durability).
  pub fn add(&mut self, stack: ItemStack, registry: &ItemRegistry) -> Result<(), InventoryError> {
    let item_id = stack.item_id();
    let mut remaining = stack.quantity();
    let has_instance_data = stack.instance_data().is_some();

    // Look up max_stack from registry; default to 1 if template not found.
    let max_stack = registry.get(item_id).map(|t| t.max_stack()).unwrap_or(1);

    // Step 1: For stackable items without instance data, try to fill existing partial stacks.
    if max_stack > 1 && !has_instance_data {
      for slot in self.slots.iter_mut() {
        if remaining == 0 {
          break;
        }
        if let Some(existing) = slot
          && existing.item_id() == item_id
        {
          let space = max_stack.saturating_sub(existing.quantity());
          if space > 0 {
            let to_add = remaining.min(space);
            existing.set_quantity(existing.quantity() + to_add);
            remaining -= to_add;
          }
        }
      }
    }

    // Step 2: Place remaining quantity into empty slots.
    // Items with instance data are placed as a single stack to preserve data.
    if has_instance_data && remaining > 0 {
      let empty_slot = self.slots.iter().position(|s| s.is_none());
      match empty_slot {
        Some(idx) => {
          self.slots[idx] = Some(stack);
        }
        None => {
          return Err(InventoryError::InventoryFull {
            remainder: remaining,
          });
        }
      }
    } else {
      while remaining > 0 {
        let empty_slot = self.slots.iter().position(|s| s.is_none());
        match empty_slot {
          Some(idx) => {
            let to_place = remaining.min(max_stack);
            self.slots[idx] = Some(ItemStack::new(item_id, to_place));
            remaining -= to_place;
          }
          None => {
            return Err(InventoryError::InventoryFull {
              remainder: remaining,
            });
          }
        }
      }
    }

    Ok(())
  }

  /// Remove `quantity` items from the given slot.
  ///
  /// Returns an [`ItemStack`] representing the removed items. If the entire
  /// slot is consumed, the slot is set to `None`.
  ///
  /// # Errors
  ///
  /// - [`InventoryError::SlotOutOfBounds`] if `slot >= capacity`
  /// - [`InventoryError::SlotEmpty`] if the slot contains no items
  /// - [`InventoryError::InvalidQuantity`] if `quantity` exceeds the slot's
  ///   current quantity
  pub fn remove(&mut self, slot: usize, quantity: u32) -> Result<ItemStack, InventoryError> {
    if slot >= self.capacity {
      return Err(InventoryError::SlotOutOfBounds {
        slot,
        capacity: self.capacity,
      });
    }

    let current_qty = {
      let stack = self.slots[slot]
        .as_ref()
        .ok_or(InventoryError::SlotEmpty { slot })?;
      stack.quantity()
    };

    if quantity > current_qty {
      return Err(InventoryError::InvalidQuantity(quantity));
    }

    if quantity == current_qty {
      // Consume the entire slot, preserving instance_data.
      let stack = self.slots[slot].take().unwrap();
      Ok(stack)
    } else {
      // Partial removal — reduce the slot quantity and return a plain sub-stack.
      let item_id = self.slots[slot].as_ref().unwrap().item_id();
      self.slots[slot]
        .as_mut()
        .unwrap()
        .set_quantity(current_qty - quantity);
      Ok(ItemStack::new(item_id, quantity))
    }
  }

  /// Return a reference to the [`ItemStack`] in the given slot, if occupied.
  pub fn get(&self, slot: usize) -> Option<&ItemStack> {
    self.slots.get(slot).and_then(|s| s.as_ref())
  }

  /// Return a mutable reference to the [`ItemStack`] in the given slot, if
  /// occupied.
  pub fn get_mut(&mut self, slot: usize) -> Option<&mut ItemStack> {
    self.slots.get_mut(slot).and_then(|s| s.as_mut())
  }

  /// Swap the contents of two slots.
  ///
  /// This works even if one or both slots are empty.
  ///
  /// # Errors
  ///
  /// Returns [`InventoryError::SlotOutOfBounds`] if either index is out of range.
  pub fn swap(&mut self, slot_a: usize, slot_b: usize) -> Result<(), InventoryError> {
    if slot_a >= self.capacity {
      return Err(InventoryError::SlotOutOfBounds {
        slot: slot_a,
        capacity: self.capacity,
      });
    }
    if slot_b >= self.capacity {
      return Err(InventoryError::SlotOutOfBounds {
        slot: slot_b,
        capacity: self.capacity,
      });
    }
    self.slots.swap(slot_a, slot_b);
    Ok(())
  }

  /// Total quantity of a given item across all slots.
  pub fn count(&self, item_id: ItemId) -> u32 {
    self
      .slots
      .iter()
      .filter_map(|s| s.as_ref())
      .filter(|stack| stack.item_id() == item_id)
      .map(|stack| stack.quantity())
      .sum()
  }

  /// Returns `true` if the inventory contains at least `qty` of the given item.
  pub fn has(&self, item_id: ItemId, qty: u32) -> bool {
    self.count(item_id) >= qty
  }

  /// Return the index of the first slot containing the given item, if any.
  pub fn find(&self, item_id: ItemId) -> Option<usize> {
    self
      .slots
      .iter()
      .position(|s| s.as_ref().is_some_and(|stack| stack.item_id() == item_id))
  }

  /// Returns `true` if every slot is occupied.
  pub fn is_full(&self) -> bool {
    self.slots.iter().all(|s| s.is_some())
  }

  /// Number of empty (unoccupied) slots.
  pub fn empty_slots(&self) -> usize {
    self.slots.iter().filter(|s| s.is_none()).count()
  }

  /// Directly set a slot's contents.
  ///
  /// This bypasses stacking logic and is intended for operations like
  /// [`compact`](Self::compact) and testing.
  ///
  /// # Errors
  ///
  /// Returns [`InventoryError::SlotOutOfBounds`] if `index >= capacity`.
  pub fn set_slot(&mut self, index: usize, stack: Option<ItemStack>) -> Result<(), InventoryError> {
    if index >= self.capacity {
      return Err(InventoryError::SlotOutOfBounds {
        slot: index,
        capacity: self.capacity,
      });
    }
    self.slots[index] = stack;
    Ok(())
  }

  /// Merge partial stacks of the same item type, freeing up slots.
  ///
  /// After compaction, each item type will occupy the minimum number of slots
  /// needed to hold its total quantity (respecting `max_stack`).
  ///
  /// **Note:** Stacks with per-instance [`ItemData`](super::ItemData) are never
  /// merged — they are moved to the front but otherwise left untouched.
  pub fn compact(&mut self, registry: &ItemRegistry) {
    // Separate stacks with instance data (preserve as-is) from plain stacks (merge).
    let mut instance_stacks: Vec<ItemStack> = Vec::new();
    let mut totals: Vec<(ItemId, u32)> = Vec::new();

    for stack in self.slots.iter().flatten() {
      if stack.instance_data().is_some() {
        instance_stacks.push(stack.clone());
      } else {
        let id = stack.item_id();
        if let Some(entry) = totals.iter_mut().find(|(eid, _)| *eid == id) {
          entry.1 += stack.quantity();
        } else {
          totals.push((id, stack.quantity()));
        }
      }
    }

    // Clear all slots.
    for slot in self.slots.iter_mut() {
      *slot = None;
    }

    // Re-fill: first place instance-data stacks (preserved), then merged totals.
    let mut write_idx = 0;
    for stack in instance_stacks {
      if write_idx < self.capacity {
        self.slots[write_idx] = Some(stack);
        write_idx += 1;
      }
    }
    for (item_id, mut total) in totals {
      let max_stack = registry.get(item_id).map(|t| t.max_stack()).unwrap_or(1);
      while total > 0 && write_idx < self.capacity {
        let to_place = total.min(max_stack);
        self.slots[write_idx] = Some(ItemStack::new(item_id, to_place));
        total -= to_place;
        write_idx += 1;
      }
    }
  }

  /// Transfer items from one inventory to another.
  ///
  /// Removes `quantity` items from `from_slot` in `from`, then adds them
  /// to `to`. If `to_slot` is `Some`, the items are placed directly into
  /// that specific slot; if `None`, normal [`add`](Self::add) logic is used.
  ///
  /// # Errors
  ///
  /// Returns an error if the source slot doesn't have enough items or if
  /// the destination inventory cannot accept them.
  pub fn transfer(
    from: &mut Inventory,
    from_slot: usize,
    to: &mut Inventory,
    to_slot: Option<usize>,
    quantity: u32,
    registry: &ItemRegistry,
  ) -> Result<(), InventoryError> {
    let removed = from.remove(from_slot, quantity)?;

    let result = match to_slot {
      Some(slot) => {
        // Place directly into the specified slot.
        match to.slots.get(slot) {
          None => Err(InventoryError::SlotOutOfBounds {
            slot,
            capacity: to.capacity,
          }),
          Some(existing) => {
            if existing.is_some() {
              // Slot occupied — try stacking if same item.
              let existing_stack = existing.as_ref().unwrap();
              if existing_stack.item_id() == removed.item_id() {
                let max_stack = registry
                  .get(removed.item_id())
                  .map(|t| t.max_stack())
                  .unwrap_or(1);
                let new_qty = existing_stack.quantity() + removed.quantity();
                if new_qty <= max_stack {
                  to.slots[slot].as_mut().unwrap().set_quantity(new_qty);
                  Ok(())
                } else {
                  Err(InventoryError::InventoryFull {
                    remainder: removed.quantity(),
                  })
                }
              } else {
                Err(InventoryError::InventoryFull {
                  remainder: removed.quantity(),
                })
              }
            } else {
              // Slot empty — place directly.
              to.slots[slot] = Some(removed.clone());
              Ok(())
            }
          }
        }
      }
      None => to.add(removed.clone(), registry),
    };

    // If the destination rejected the items, put them back.
    if result.is_err() {
      // Restore the removed items to the source, preserving instance_data.
      let qty = removed.quantity();
      match from.slots.get(from_slot) {
        Some(Some(existing)) => {
          // Slot has something — add back quantity.
          let new_qty = existing.quantity() + qty;
          from.slots[from_slot]
            .as_mut()
            .unwrap()
            .set_quantity(new_qty);
        }
        _ => {
          // Slot was emptied — restore original stack with instance_data intact.
          from.slots[from_slot] = Some(removed);
        }
      }
    }

    result
  }
}
