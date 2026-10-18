//! A fast hasher for the engine's internal maps
//!
//! Words are folded one at a time with a folded multiply. The maps that use
//! it are keyed by ids and hashes the engine made, not by player input, so a
//! hash built for speed rather than flood resistance is safe. Values may
//! change between versions: never save them.

use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasherDefault, Hash, Hasher};

/// Where a hash starts (the fractional part of the golden ratio)
pub(crate) const SEED: u64 = 0x9e37_79b9_7f4a_7c15;
/// The odd multiplier each word is folded with (splitmix64's)
pub(crate) const MULTIPLIER: u64 = 0xbf58_476d_1ce4_e5b9;
/// The multiplier a hash is finished with (splitmix64's other)
const FINISH: u64 = 0x94d0_49bb_1331_11eb;

/// `a * b` as 128 bits, the halves xor'd together: every bit of `a` reaches
/// the high half, so one multiply mixes a whole word
pub(crate) fn fold(a: u64, b: u64) -> u64 {
  let full = u128::from(a) * u128::from(b);
  (full as u64) ^ ((full >> 64) as u64)
}

/// Hashes words as `fold` folds them
#[derive(Debug, Clone, Copy)]
pub(crate) struct FastHasher(u64);

impl Default for FastHasher {
  fn default() -> Self {
    FastHasher(SEED)
  }
}

impl Hasher for FastHasher {
  fn write(&mut self, bytes: &[u8]) {
    let (words, rest) = bytes.as_chunks::<8>();
    for word in words {
      self.0 = fold(self.0 ^ u64::from_le_bytes(*word), MULTIPLIER);
    }
    if !rest.is_empty() {
      let mut tail = [0u8; 8];
      for (to, from) in tail.iter_mut().zip(rest) {
        *to = *from;
      }
      // The length tells "a\0" from "a"
      let word = u64::from_le_bytes(tail) ^ ((rest.len() as u64) << 56);
      self.0 = fold(self.0 ^ word, MULTIPLIER);
    }
  }

  fn write_u8(&mut self, n: u8) {
    self.write_u64(u64::from(n));
  }

  fn write_u16(&mut self, n: u16) {
    self.write_u64(u64::from(n));
  }

  fn write_u32(&mut self, n: u32) {
    self.write_u64(u64::from(n));
  }

  fn write_u64(&mut self, n: u64) {
    self.0 = fold(self.0 ^ n, MULTIPLIER);
  }

  fn write_u128(&mut self, n: u128) {
    self.write_u64(n as u64);
    self.write_u64((n >> 64) as u64);
  }

  fn write_usize(&mut self, n: usize) {
    self.write_u64(n as u64);
  }

  fn finish(&self) -> u64 {
    fold(self.0, FINISH)
  }
}

/// Builds a [`FastHasher`] for a map
pub(crate) type FastBuild = BuildHasherDefault<FastHasher>;
/// A `HashMap` on [`FastHasher`]
pub(crate) type FastMap<K, V> = HashMap<K, V, FastBuild>;
/// A `HashSet` on [`FastHasher`]
pub(crate) type FastSet<K> = HashSet<K, FastBuild>;

/// `value`'s hash under [`FastHasher`]
pub(crate) fn fast_hash<T: Hash + ?Sized>(value: &T) -> u64 {
  let mut hasher = FastHasher::default();
  value.hash(&mut hasher);
  hasher.finish()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn equal_values_hash_equal_and_maps_work() {
    assert_eq!(fast_hash(&"score"), fast_hash(&"score"));
    let mut map: FastMap<u64, &str> = FastMap::default();
    map.insert(7, "seven");
    assert_eq!(map.get(&7), Some(&"seven"));
  }

  #[test]
  fn similar_strings_do_not_collide() {
    let mut seen = FastSet::default();
    for i in 0..100_000u32 {
      // Labels that differ in one character or one digit, and by length
      let label = format!("Row {i}");
      assert!(seen.insert(fast_hash(&label)), "collision at {label}");
    }
    assert_ne!(fast_hash(&"ab"), fast_hash(&"ab\0"));
    assert_ne!(fast_hash(&("a", "bc")), fast_hash(&("ab", "c")));
  }

  #[test]
  fn small_integers_spread_across_buckets() {
    // A map's bucket index comes from the low and high bits; consecutive ids must not cluster
    let mut low = FastSet::default();
    for i in 0..4096u64 {
      low.insert(fast_hash(&i) & 0xfff);
    }
    assert!(
      low.len() > 2400,
      "{} distinct low-12-bit values of 4096",
      low.len()
    );
  }
}
