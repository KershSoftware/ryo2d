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
  /// All-or-nothing: if the full quantity cannot be placed, the inventory is
  /// left unchanged and [`InventoryError::InventoryFull`] reports how many
  /// items would not fit. Use [`add_partial`](Self::add_partial) to place as
  /// many as fit instead.
  ///
  /// Placement follows the rules described on [`add_partial`](Self::add_partial).
  pub fn add(&mut self, stack: ItemStack, registry: &ItemRegistry) -> Result<(), InventoryError> {
    let quantity = stack.quantity();
    let room = self.room_for(&stack, Self::max_stack_of(&stack, registry));
    if room < quantity {
      return Err(InventoryError::InventoryFull {
        remainder: quantity - room,
      });
    }

    self.add_partial(stack, registry);
    Ok(())
  }

  /// Add as much of an [`ItemStack`] as fits, returning whatever did not.
  ///
  /// The method follows this logic:
  /// 1. Look up the item's `max_stack` from the registry (defaults to 1 if
  ///    the template is not found).
  /// 2. For stackable items (`max_stack > 1`) **without** per-instance data,
  ///    fill existing partial stacks of the same `item_id` that also have no
  ///    per-instance data, up to `max_stack`.
  /// 3. Place any remaining quantity into the first available empty slot(s).
  /// 4. Return the unplaced remainder, or `None` if everything was placed.
  ///
  /// Non-stackable items (`max_stack == 1`) always go directly to empty slots.
  ///
  /// **Note:** Items with [`instance_data`](ItemStack::instance_data) are never
  /// merged into existing stacks — they always occupy their own slot to preserve
  /// per-instance state (e.g. enchantments, durability). If no slot is free,
  /// the original stack is returned with its data intact.
  pub fn add_partial(&mut self, stack: ItemStack, registry: &ItemRegistry) -> Option<ItemStack> {
    let item_id = stack.item_id();
    let mut remaining = stack.quantity();
    let max_stack = Self::max_stack_of(&stack, registry);

    // Items with instance data are placed as a single stack to preserve data.
    if stack.instance_data().is_some() {
      return match self.slots.iter().position(|s| s.is_none()) {
        Some(idx) => {
          self.slots[idx] = Some(stack);
          None
        }
        None => Some(stack),
      };
    }

    // Fill existing partial stacks first.
    if max_stack > 1 {
      for existing in self.slots.iter_mut().flatten() {
        if remaining == 0 {
          break;
        }
        if can_merge(existing, &stack) {
          let to_add = remaining.min(max_stack.saturating_sub(existing.quantity()));
          existing.set_quantity(existing.quantity() + to_add);
          remaining -= to_add;
        }
      }
    }

    // Place the rest into empty slots.
    while remaining > 0 {
      let Some(idx) = self.slots.iter().position(|s| s.is_none()) else {
        return Some(ItemStack::new(item_id, remaining));
      };
      let to_place = remaining.min(max_stack);
      self.slots[idx] = Some(ItemStack::new(item_id, to_place));
      remaining -= to_place;
    }

    None
  }

  /// `max_stack` for the stack's template, or 1 if it isn't registered.
  fn max_stack_of(stack: &ItemStack, registry: &ItemRegistry) -> u32 {
    registry
      .get(stack.item_id())
      .map(|t| t.max_stack())
      .unwrap_or(1)
  }

  /// How many items of `stack` could be placed without exceeding capacity.
  fn room_for(&self, stack: &ItemStack, max_stack: u32) -> u32 {
    let empty = u32::try_from(self.empty_slots()).unwrap_or(u32::MAX);
    if stack.instance_data().is_some() {
      return if empty > 0 { stack.quantity() } else { 0 };
    }

    let mut room = empty.saturating_mul(max_stack);
    if max_stack > 1 {
      for existing in self.slots.iter().flatten() {
        if can_merge(existing, stack) {
          room = room.saturating_add(max_stack.saturating_sub(existing.quantity()));
        }
      }
    }
    room
  }

  /// Remove `quantity` items from the given slot.
  ///
  /// Returns an [`ItemStack`] representing the removed items, carrying the
  /// slot's [`instance_data`](ItemStack::instance_data) if it has any. If the
  /// entire slot is consumed, the slot is set to `None`.
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

    let entry = &mut self.slots[slot];
    let stack = entry.as_mut().ok_or(InventoryError::SlotEmpty { slot })?;
    let current_qty = stack.quantity();

    if quantity > current_qty {
      return Err(InventoryError::InvalidQuantity(quantity));
    }

    if quantity == current_qty {
      // Consume the entire slot, preserving instance_data.
      return entry.take().ok_or(InventoryError::SlotEmpty { slot });
    }

    // Partial removal: both parts keep the stack's instance_data
    let mut removed = stack.clone();
    removed.set_quantity(quantity);
    stack.set_quantity(current_qty - quantity);
    Ok(removed)
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
          Some(None) => {
            // Slot empty — place directly.
            to.slots[slot] = Some(removed.clone());
            Ok(())
          }
          Some(Some(_)) => {
            // Slot occupied — stack only onto a compatible stack with room.
            let max_stack = Self::max_stack_of(&removed, registry);
            match to.slots[slot].as_mut() {
              Some(existing)
                if can_merge(existing, &removed)
                  && existing.quantity() + removed.quantity() <= max_stack =>
              {
                existing.set_quantity(existing.quantity() + removed.quantity());
                Ok(())
              }
              _ => Err(InventoryError::InventoryFull {
                remainder: removed.quantity(),
              }),
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
      match from.slots.get_mut(from_slot) {
        Some(Some(existing)) => {
          // Partial removal — add the quantity back.
          existing.set_quantity(existing.quantity() + qty);
        }
        Some(entry) => {
          // Slot was emptied — restore original stack with instance_data intact.
          *entry = Some(removed);
        }
        None => {}
      }
    }

    result
  }
}

/// Two stacks can merge only when they hold the same item and neither carries
/// per-instance data (merging would copy or drop that data).
fn can_merge(a: &ItemStack, b: &ItemStack) -> bool {
  a.item_id() == b.item_id() && a.instance_data().is_none() && b.instance_data().is_none()
}
