//! Loot tables: weighted randomization for item drops.

use std::ops::RangeInclusive;

use rand::Rng;

use super::item::{ItemId, ItemStack};

/// A single entry in a loot table.
pub struct LootEntry {
  pub item_id: ItemId,
  pub weight: f64,
  pub quantity: RangeInclusive<u32>,
}

/// Weighted randomization for item drops.
///
/// A `LootTable` holds a list of [`LootEntry`] values, each with a relative
/// weight and a quantity range.  Calling [`roll`](LootTable::roll) picks one
/// entry at random (or nothing, if `nothing_weight` is set) and returns an
/// [`ItemStack`] with a randomised quantity.
///
/// # Example
///
/// ```rust,ignore
/// use ryo2d::prelude::*;
/// use ryo2d_inventory::prelude::*;
///
/// let table = LootTable::new()
///     .entry(potion_id, 0.8, 1..=3)
///     .entry(rare_gem_id, 0.1, 1..=1)
///     .nothing(0.1);
///
/// let mut rng = rand::thread_rng();
/// if let Some(stack) = table.roll(&mut rng) {
///     println!("Dropped: {:?}", stack);
/// }
/// ```
pub struct LootTable {
  entries: Vec<LootEntry>,
  total_weight: f64,
  nothing_weight: f64,
}

impl LootTable {
  /// Create an empty loot table.
  pub fn new() -> Self {
    Self {
      entries: Vec::new(),
      total_weight: 0.0,
      nothing_weight: 0.0,
    }
  }

  /// Add an entry to the loot table (builder pattern).
  ///
  /// `weight` is a relative probability; `quantity` is the inclusive range
  /// of stack sizes that can be rolled for this item.
  pub fn entry(mut self, item_id: ItemId, weight: f64, quantity: RangeInclusive<u32>) -> Self {
    self.total_weight += weight;
    self.entries.push(LootEntry {
      item_id,
      weight,
      quantity,
    });
    self
  }

  /// Set the weight for "no drop" (builder pattern).
  ///
  /// When rolling, there is a `nothing_weight / (total_weight + nothing_weight)`
  /// chance that no item is returned.
  pub fn nothing(mut self, weight: f64) -> Self {
    self.nothing_weight = weight;
    self
  }

  /// Roll the loot table once, returning `None` for "no drop".
  ///
  /// The algorithm:
  /// 1. Compute total = `total_weight + nothing_weight`.
  /// 2. Generate a random `f64` in `0..total`.
  /// 3. Walk entries, subtracting each weight.  If the roll falls below zero,
  ///    pick that entry.
  /// 4. Otherwise the roll landed in the nothing-weight region: return `None`.
  /// 5. For the chosen entry, roll a random quantity within its range.
  pub fn roll(&self, rng: &mut impl Rng) -> Option<ItemStack> {
    let total = self.total_weight + self.nothing_weight;
    if total <= 0.0 {
      return None;
    }

    let mut roll = rng.gen_range(0.0..total);

    for entry in &self.entries {
      roll -= entry.weight;
      if roll < 0.0 {
        let qty = rng.gen_range(entry.quantity.clone());
        return Some(ItemStack::new(entry.item_id, qty));
      }
    }

    // Landed in the nothing-weight region.
    None
  }

  /// Roll the table `count` times and collect all non-`None` results.
  pub fn roll_multiple(&self, count: usize, rng: &mut impl Rng) -> Vec<ItemStack> {
    (0..count).filter_map(|_| self.roll(rng)).collect()
  }
}

impl Default for LootTable {
  fn default() -> Self {
    Self::new()
  }
}
