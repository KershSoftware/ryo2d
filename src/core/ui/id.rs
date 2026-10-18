//! Widget ids: a hash of the enclosing scope, the call site, and how often
//! that site ran in the scope this frame (or an explicit key in its place)
//!
//! Ids are hashed a word at a time with a folded multiply, and `.id(key)`
//! keys with the same mix (`IdHasher`). Ids live only in memory, never in a
//! save, so their values may change between versions.

use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasherDefault, Hash, Hasher};
use std::panic::Location;

/// A widget's identity across frames. Made from where the widget is called;
/// `.id(key)` makes it from a key instead, for lists that reorder
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct UiId(pub(crate) u64);

impl UiId {
  /// No widget
  pub const NONE: UiId = UiId(0);

  /// An id of its own derived from this one: for state or nodes that belong
  /// to a widget but are not it
  pub(crate) fn derived(self, salt: u64) -> UiId {
    UiId(mix(self.0, &[salt]))
  }

  /// The root scope of a scene's UI
  pub(crate) fn root(scene_scope: u64) -> UiId {
    UiId(mix(SEED, &[0x00ec_0e00, scene_scope]))
  }
}

/// Where a hash starts (the fractional part of the golden ratio)
const SEED: u64 = 0x9e37_79b9_7f4a_7c15;
/// The odd multiplier each word is folded with (splitmix64's)
const MULTIPLIER: u64 = 0xbf58_476d_1ce4_e5b9;
/// The multiplier a key's hash is finished with (splitmix64's other)
const FINISH: u64 = 0x94d0_49bb_1331_11eb;
const KEYED: u64 = 0x006b_6579;

/// `a * b` as 128 bits, the halves xor'd together: every bit of `a` reaches
/// the high half, so one multiply mixes a whole word
fn fold(a: u64, b: u64) -> u64 {
  let full = u128::from(a) * u128::from(b);
  (full as u64) ^ ((full >> 64) as u64)
}

/// `words` folded into `hash` one at a time; never returns 0 (`UiId::NONE`)
fn mix(mut hash: u64, words: &[u64]) -> u64 {
  for &word in words {
    hash = fold(hash ^ word, MULTIPLIER);
  }
  hash.max(1)
}

/// A hasher for the keys of `.id(key)`: words folded as `mix` folds them.
/// The id maps here (`counts`, `used`) use it too: their keys are ids, not
/// player input, so a hash built for speed rather than flood resistance is
/// safe.
#[derive(Debug, Clone, Copy)]
struct IdHasher(u64);

impl Default for IdHasher {
  fn default() -> Self {
    IdHasher(SEED)
  }
}

impl Hasher for IdHasher {
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

/// Builds an [`IdHasher`] for a map
type IdHashing = BuildHasherDefault<IdHasher>;

/// A call site: the static file name's address, its length, line and column
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Site([u64; 4]);

impl Site {
  fn of(location: &'static Location<'static>) -> Site {
    let file = location.file();
    Site([
      file.as_ptr() as u64,
      file.len() as u64,
      u64::from(location.line()),
      u64::from(location.column()),
    ])
  }
}

/// The scopes and per-site counts of the frame being recorded
#[derive(Debug, Default)]
pub(crate) struct IdScopes {
  stack: Vec<UiId>,
  counts: HashMap<(UiId, Site), u64, IdHashing>,
  used: HashSet<UiId, IdHashing>,
  warned: HashSet<UiId, IdHashing>,
}

impl IdScopes {
  pub(crate) fn begin_frame(&mut self) {
    self.stack.clear();
    self.counts.clear();
    self.used.clear();
  }

  /// Start a scene's UI: `root` is the only open scope
  pub(crate) fn enter_root(&mut self, root: UiId) {
    self.stack.clear();
    self.stack.push(root);
  }

  pub(crate) fn current(&self) -> UiId {
    self.stack.last().copied().unwrap_or(UiId::NONE)
  }

  pub(crate) fn push(&mut self, scope: UiId) {
    self.stack.push(scope);
  }

  pub(crate) fn pop(&mut self) {
    // The root stays
    if self.stack.len() > 1 {
      self.stack.pop();
    }
  }

  /// The id of a widget started at `site` in the current scope
  pub(crate) fn make(&mut self, site: &'static Location<'static>, key: Option<u64>) -> UiId {
    let parent = self.current();
    let site = Site::of(site);
    let id = match key {
      Some(key) => UiId(mix(
        parent.0,
        &[site.0[0], site.0[1], site.0[2], site.0[3], KEYED, key],
      )),
      None => {
        let count = self.counts.entry((parent, site)).or_insert(0);
        let id = UiId(mix(
          parent.0,
          &[site.0[0], site.0[1], site.0[2], site.0[3], *count],
        ));
        *count += 1;
        id
      }
    };
    if self.used.insert(id) {
      return id;
    }
    // Only repeated explicit keys collide; never share their state
    if self.warned.insert(id) {
      crate::engine_warn!(module: "ui", "two widgets share the id key in one scope; use distinct .id() keys");
    }
    let mut n = 1;
    loop {
      let candidate = UiId(mix(id.0, &[n]));
      if self.used.insert(candidate) {
        return candidate;
      }
      n += 1;
    }
  }
}

/// A key's hash for `.id(key)`
pub(crate) fn key_hash<K: Hash + ?Sized>(key: &K) -> u64 {
  let mut hasher = IdHasher::default();
  key.hash(&mut hasher);
  hasher.finish()
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::panic::Location;

  #[track_caller]
  fn here() -> &'static Location<'static> {
    Location::caller()
  }

  fn scopes() -> IdScopes {
    let mut scopes = IdScopes::default();
    scopes.begin_frame();
    scopes.enter_root(UiId::root(1));
    scopes
  }

  #[test]
  fn one_site_run_twice_gives_two_ids() {
    let mut s = scopes();
    let site = here();
    let a = s.make(site, None);
    let b = s.make(site, None);
    assert_ne!(a, b);
    assert_ne!(a, UiId::NONE);
  }

  #[test]
  fn ids_are_stable_across_frames() {
    let site_a = here();
    let site_b = here();
    let mut s = scopes();
    let first = (s.make(site_a, None), s.make(site_b, None));
    s.begin_frame();
    s.enter_root(UiId::root(1));
    let second = (s.make(site_a, None), s.make(site_b, None));
    assert_eq!(first, second);
    assert_ne!(first.0, first.1, "two lines, two ids");
  }

  #[test]
  fn keys_survive_a_reorder() {
    let site = here();
    let mut s = scopes();
    let a1 = s.make(site, Some(key_hash(&"sword")));
    let b1 = s.make(site, Some(key_hash(&"shield")));
    s.begin_frame();
    s.enter_root(UiId::root(1));
    let b2 = s.make(site, Some(key_hash(&"shield")));
    let a2 = s.make(site, Some(key_hash(&"sword")));
    assert_eq!((a1, b1), (a2, b2));
  }

  #[test]
  fn skipping_an_earlier_call_shifts_occurrence_ids() {
    // Documents the rule: a helper's widgets are told apart by order
    let site = here();
    let mut s = scopes();
    let _first = s.make(site, None);
    let second = s.make(site, None);
    s.begin_frame();
    s.enter_root(UiId::root(1));
    let now_first = s.make(site, None);
    assert_ne!(
      now_first, second,
      "the second widget now gets the first slot's id"
    );
  }

  #[test]
  fn a_duplicate_key_is_suffixed_and_warned_once() {
    let site = here();
    let mut s = scopes();
    let a = s.make(site, Some(7));
    let b = s.make(site, Some(7));
    let c = s.make(site, Some(7));
    assert!(a != b && b != c && a != c);
    let warnings = crate::core::dev_log::get_entries()
      .iter()
      .filter(|e| e.message.contains("share the id key"))
      .count();
    assert_eq!(warnings, 1);
  }

  #[test]
  fn entity_keys_never_collide_and_no_id_is_none() {
    use crate::core::ecs::kernel::World;
    // Entities as a node tree keys them: one parent, thousands of children,
    // in two generations of the same slots
    let mut world = World::new();
    let mut keys: Vec<_> = (0..4000).map(|_| world.spawn_empty().id()).collect();
    for &entity in keys.iter().take(500) {
      world.despawn(entity);
    }
    keys.extend((0..500).map(|_| world.spawn_empty().id()));
    let site = here();
    let mut s = scopes();
    let mut seen = HashSet::new();
    for key in &keys {
      let id = s.make(site, Some(key_hash(key)));
      assert_ne!(id, UiId::NONE);
      assert!(seen.insert(id), "two entity keys made one id");
    }
    assert!(s.warned.is_empty(), "no key was taken for a duplicate");
    // Small integers and strings that differ by one character, as `.id(i)`
    // and `.id(name)` key list rows
    let mut hashes = HashSet::new();
    for i in 0..4000u64 {
      assert!(hashes.insert(key_hash(&i)));
      assert!(hashes.insert(key_hash(&format!("row {i}"))));
    }
    assert_ne!(key_hash(&"a"), key_hash(&"a\0"));
    assert_ne!(mix(0, &[0]), 0, "a mix is never NONE");
  }

  #[test]
  fn scopes_separate_the_same_site() {
    let site = here();
    let mut s = scopes();
    let outer = s.make(site, None);
    s.push(outer);
    let inner = s.make(site, None);
    s.pop();
    assert_ne!(outer, inner);
    assert_ne!(UiId::root(1), UiId::root(2));
  }
}
