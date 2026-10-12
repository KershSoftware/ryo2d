//! Equipment slots: abstract indexed positions with optional validation.

use super::item::ItemStack;
use super::registry::ItemRegistry;
use crate::InventoryError;

/// Validation trait for equipment slot restrictions.
/// Optional -- without a validator, any item fits any slot.
pub trait EquipValidator: Send + Sync {
  fn can_equip(&self, slot: usize, item: &ItemStack, registry: &ItemRegistry) -> bool;
}

/// Abstract indexed equipment slots with optional validation.
///
/// Equipment slots are just indexed positions. Game developers define semantics
/// (e.g., `const SLOT_HEAD: usize = 0`). The optional [`EquipValidator`] trait
/// lets game code add equip restrictions without the engine prescribing rules.
pub struct Equipment {
  slots: Vec<Option<ItemStack>>,
  validator: Option<Box<dyn EquipValidator>>,
}

impl Equipment {
  /// Create equipment with `slot_count` empty slots and no validator.
  pub fn new(slot_count: usize) -> Self {
    Self {
      slots: (0..slot_count).map(|_| None).collect(),
      validator: None,
    }
  }

  /// Equip an item into the given slot.
  ///
  /// If a validator is set, it is consulted before equipping. Returns
  /// `Err(ValidationFailed(...))` if the validator rejects the item. Returns
  /// `Ok(Some(prev))` if the slot already had an item (which is returned),
  /// or `Ok(None)` if the slot was empty. Returns an error if the slot index
  /// is out of bounds.
  pub fn equip(
    &mut self,
    slot: usize,
    item: ItemStack,
    registry: &ItemRegistry,
  ) -> Result<Option<ItemStack>, InventoryError> {
    if slot >= self.slots.len() {
      return Err(InventoryError::SlotOutOfBounds {
        slot,
        capacity: self.slots.len(),
      });
    }

    // Validate if a validator is set
    if let Some(validator) = &self.validator
      && !validator.can_equip(slot, &item, registry)
    {
      return Err(InventoryError::ValidationFailed(format!(
        "item cannot be equipped in slot {slot}"
      )));
    }

    let prev = self.slots[slot].take();
    self.slots[slot] = Some(item);
    Ok(prev)
  }

  /// Remove and return the item in the given slot.
  ///
  /// Returns an error if the slot is empty or out of bounds.
  pub fn unequip(&mut self, slot: usize) -> Result<ItemStack, InventoryError> {
    if slot >= self.slots.len() {
      return Err(InventoryError::SlotOutOfBounds {
        slot,
        capacity: self.slots.len(),
      });
    }

    self.slots[slot]
      .take()
      .ok_or(InventoryError::SlotEmpty { slot })
  }

  /// Returns the item in the given slot, or `None` if empty or out of bounds.
  pub fn get(&self, slot: usize) -> Option<&ItemStack> {
    self.slots.get(slot).and_then(|s| s.as_ref())
  }

  /// Returns `true` if the slot is empty or out of bounds.
  pub fn is_slot_empty(&self, slot: usize) -> bool {
    self.get(slot).is_none()
  }

  /// Iterate over occupied slots, yielding `(slot_index, &ItemStack)`.
  pub fn equipped_items(&self) -> impl Iterator<Item = (usize, &ItemStack)> {
    self
      .slots
      .iter()
      .enumerate()
      .filter_map(|(i, s)| s.as_ref().map(|item| (i, item)))
  }

  /// Return the total number of equipment slots.
  pub fn slot_count(&self) -> usize {
    self.slots.len()
  }

  /// Set (or replace) the equip validator.
  pub fn set_validator(&mut self, validator: Box<dyn EquipValidator>) {
    self.validator = Some(validator);
  }
}
