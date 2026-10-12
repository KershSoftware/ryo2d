//! Inventory errors.

use thiserror::Error;

/// Inventory system errors
#[derive(Error, Debug)]
pub enum InventoryError {
  /// Slot index exceeds inventory capacity
  #[error("Slot {slot} out of bounds (capacity: {capacity})")]
  SlotOutOfBounds { slot: usize, capacity: usize },

  /// Attempted to access an empty slot
  #[error("Slot {slot} is empty")]
  SlotEmpty { slot: usize },

  /// No free slots remaining; `remainder` is the quantity that could not be placed
  #[error("Inventory is full ({remainder} items could not be placed)")]
  InventoryFull { remainder: u32 },

  /// Item with the given identifier was not found
  #[error("Item not found: {0}")]
  ItemNotFound(String),

  /// Quantity is invalid (e.g. zero)
  #[error("Invalid quantity: {0}")]
  InvalidQuantity(u32),

  /// Unrecognised item data type during (de)serialization
  #[error("Unknown item data type: {type_name}")]
  UnknownDataType { type_name: String },

  /// A validation check failed
  #[error("Validation failed: {0}")]
  ValidationFailed(String),

  /// I/O error
  #[error("I/O error: {0}")]
  Io(#[from] std::io::Error),

  /// JSON (de)serialization error
  #[error("JSON error: {0}")]
  Json(#[from] serde_json::Error),
}

/// Result type for inventory operations
pub type InventoryResult<T> = Result<T, InventoryError>;

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn inventory_error_display() {
    let err = InventoryError::SlotOutOfBounds {
      slot: 5,
      capacity: 3,
    };
    assert_eq!(format!("{err}"), "Slot 5 out of bounds (capacity: 3)");
  }

  #[test]
  fn inventory_error_full_display() {
    let err = InventoryError::InventoryFull { remainder: 5 };
    assert_eq!(
      format!("{err}"),
      "Inventory is full (5 items could not be placed)"
    );
  }
}
