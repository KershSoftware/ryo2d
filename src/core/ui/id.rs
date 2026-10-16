//! Widget ids: a hash of the enclosing scope, the call site, and how often
//! that site ran in the scope this frame (or an explicit key in its place)

use std::collections::{HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::panic::Location;

/// A widget's identity across frames. Made from where the widget is called;
/// `.id(key)` makes it from a key instead, for lists that reorder
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct UiId(pub(crate) u64);

impl UiId {
  /// No widget
  pub const NONE: UiId = UiId(0);

  /// The root scope of a scene's UI
  pub(crate) fn root(scene_scope: u64) -> UiId {
    UiId(mix(FNV_OFFSET, &[0x00ec_0e00, scene_scope]))
  }
}

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
const KEYED: u64 = 0x006b_6579;

/// FNV-1a over little-endian words; never returns 0 (`UiId::NONE`)
fn mix(mut hash: u64, words: &[u64]) -> u64 {
  for word in words {
    for byte in word.to_le_bytes() {
      hash ^= u64::from(byte);
      hash = hash.wrapping_mul(FNV_PRIME);
    }
  }
  hash.max(1)
}

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
  counts: HashMap<(UiId, Site), u64>,
  used: HashSet<UiId>,
  warned: HashSet<UiId>,
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
  let mut hasher = DefaultHasher::new();
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
