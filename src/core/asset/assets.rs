use std::sync::{Arc, Mutex, PoisonError, Weak};

use crossbeam_channel::{Receiver, Sender, unbounded};

use super::handle::Shared;
use super::{Asset, AssetId, Handle};
use crate::core::ecs::Resource;

/// Which slots are live, shared by a type's store and the asset server so
/// ids can be reserved from any thread
pub(crate) struct Ledger {
  slots: Vec<SlotMeta>,
  free: Vec<u32>,
  drops: Sender<(u32, u32)>,
}

struct SlotMeta {
  generation: u32,
  live: bool,
  /// The slot's strong handles, if any still live
  shared: Weak<Shared>,
}

// Used by the asset server, which comes next
#[allow(dead_code)]
impl Ledger {
  fn new(drops: Sender<(u32, u32)>) -> Self {
    Self {
      slots: Vec::new(),
      free: Vec::new(),
      drops,
    }
  }

  /// A live slot with no value yet, and its first strong handle
  pub(crate) fn reserve<T: Asset>(&mut self) -> Handle<T> {
    let index = match self.free.pop() {
      Some(index) if (index as usize) < self.slots.len() => index,
      _ => {
        self.slots.push(SlotMeta {
          generation: 0,
          live: false,
          shared: Weak::new(),
        });
        u32::try_from(self.slots.len() - 1).unwrap_or(u32::MAX)
      }
    };
    let drops = self.drops.clone();
    let Some(slot) = self.slots.get_mut(index as usize) else {
      // Unreachable short of 2^32 slots: hand out a handle to nothing live
      return Handle::from_shared(Arc::new(Shared {
        index,
        generation: 0,
        drops,
      }));
    };
    slot.live = true;
    let shared = Arc::new(Shared {
      index,
      generation: slot.generation,
      drops,
    });
    slot.shared = Arc::downgrade(&shared);
    Handle::from_shared(shared)
  }

  /// A strong handle to a live slot: one of its live handles, or a new one
  /// when every handle was dropped and the slot is not freed yet
  pub(crate) fn strong<T: Asset>(&mut self, id: AssetId<T>) -> Option<Handle<T>> {
    let slot = self.slots.get_mut(id.index() as usize)?;
    if !slot.live || slot.generation != id.generation() {
      return None;
    }
    if let Some(shared) = slot.shared.upgrade() {
      return Some(Handle::from_shared(shared));
    }
    // The old handles are all gone and their drop notice is on its way; it
    // will find this new handle alive and be ignored
    let shared = Arc::new(Shared {
      index: id.index(),
      generation: id.generation(),
      drops: self.drops.clone(),
    });
    slot.shared = Arc::downgrade(&shared);
    Some(Handle::from_shared(shared))
  }

  /// Whether `id` is live
  pub(crate) fn is_live<T: Asset>(&self, id: AssetId<T>) -> bool {
    self
      .slots
      .get(id.index() as usize)
      .is_some_and(|s| s.live && s.generation == id.generation())
  }

  /// Frees the slot if `id` is live and no strong handle remains; the slot's
  /// generation moves on and its index is reused. True if freed
  fn release(&mut self, index: u32, generation: u32) -> bool {
    let Some(slot) = self.slots.get_mut(index as usize) else {
      return false;
    };
    if !slot.live || slot.generation != generation || slot.shared.upgrade().is_some() {
      return false;
    }
    slot.live = false;
    slot.generation = slot.generation.wrapping_add(1);
    self.free.push(index);
    true
  }
}

/// Every asset of type `T`: a slot table. A world resource per type
// Used by the asset server, which comes next
#[allow(dead_code)]
#[derive(Resource)]
pub struct Assets<T: Asset> {
  ledger: Arc<Mutex<Ledger>>,
  drops: Receiver<(u32, u32)>,
  values: Vec<Option<Entry<T>>>,
  next_version: u64,
}

struct Entry<T> {
  generation: u32,
  version: u64,
  value: T,
}

impl<T: Asset> Default for Assets<T> {
  fn default() -> Self {
    let (sender, drops) = unbounded();
    Self {
      ledger: Arc::new(Mutex::new(Ledger::new(sender))),
      drops,
      values: Vec::new(),
      next_version: 1,
    }
  }
}

// Used by the asset server, which comes next
#[allow(dead_code)]
impl<T: Asset> Assets<T> {
  /// Adds an asset made in code; it lives while the returned handle (or a clone) does
  pub fn add(&mut self, value: T) -> Handle<T> {
    let handle = self
      .ledger
      .lock()
      .unwrap_or_else(PoisonError::into_inner)
      .reserve::<T>();
    self.insert(handle.id(), value);
    handle
  }

  fn entry(&self, id: AssetId<T>) -> Option<&Entry<T>> {
    self
      .values
      .get(id.index() as usize)?
      .as_ref()
      .filter(|e| e.generation == id.generation())
  }

  /// The asset, if `id` is live and loaded
  pub fn get(&self, id: impl Into<AssetId<T>>) -> Option<&T> {
    self.entry(id.into()).map(|e| &e.value)
  }

  /// The asset for changing in place; its content version moves on
  pub fn get_mut(&mut self, id: impl Into<AssetId<T>>) -> Option<&mut T> {
    let id = id.into();
    let entry = self
      .values
      .get_mut(id.index() as usize)?
      .as_mut()
      .filter(|e| e.generation == id.generation())?;
    entry.version = self.next_version;
    self.next_version += 1;
    Some(&mut entry.value)
  }

  /// The asset's content version: it changes whenever the value does, so
  /// caches keyed by (id, version) never see stale content
  pub fn version(&self, id: impl Into<AssetId<T>>) -> Option<u64> {
    self.entry(id.into()).map(|e| e.version)
  }

  /// The number of loaded assets
  pub fn len(&self) -> usize {
    self.values.iter().flatten().count()
  }

  pub fn is_empty(&self) -> bool {
    self.values.iter().all(Option::is_none)
  }

  /// Every loaded asset with its id
  pub fn iter(&self) -> impl Iterator<Item = (AssetId<T>, &T)> {
    self.values.iter().enumerate().filter_map(|(index, entry)| {
      let entry = entry.as_ref()?;
      Some((
        AssetId::new(u32::try_from(index).ok()?, entry.generation),
        &entry.value,
      ))
    })
  }

  pub(crate) fn ledger(&self) -> Arc<Mutex<Ledger>> {
    Arc::clone(&self.ledger)
  }

  /// Puts a value in a reserved (or loaded) slot; false if `id` is stale
  pub(crate) fn insert(&mut self, id: AssetId<T>, value: T) -> bool {
    if !self
      .ledger
      .lock()
      .unwrap_or_else(PoisonError::into_inner)
      .is_live(id)
    {
      return false;
    }
    let index = id.index() as usize;
    if self.values.len() <= index {
      self.values.resize_with(index + 1, || None);
    }
    let Some(slot) = self.values.get_mut(index) else {
      return false;
    };
    *slot = Some(Entry {
      generation: id.generation(),
      version: self.next_version,
      value,
    });
    self.next_version += 1;
    true
  }

  /// Frees every asset whose last handle dropped, adding their ids to `freed`
  pub(crate) fn free_unheld(&mut self, freed: &mut Vec<AssetId<T>>) {
    if self.drops.is_empty() {
      return;
    }
    let mut ledger = self.ledger.lock().unwrap_or_else(PoisonError::into_inner);
    for (index, generation) in self.drops.try_iter() {
      if ledger.release(index, generation) {
        if let Some(slot) = self.values.get_mut(index as usize) {
          *slot = None;
        }
        freed.push(AssetId::new(index, generation));
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::asset::Asset;

  struct Text(String);
  impl Asset for Text {}

  #[test]
  fn an_added_asset_is_got_by_its_handle() {
    let mut assets = Assets::<Text>::default();
    let handle = assets.add(Text("a".into()));
    assert_eq!(assets.get(&handle).map(|t| t.0.as_str()), Some("a"));
    assert_eq!(assets.len(), 1);
  }

  #[test]
  fn the_last_dropped_handle_frees_its_asset_at_the_next_free() {
    let mut assets = Assets::<Text>::default();
    let handle = assets.add(Text("a".into()));
    let id = handle.id();
    let copy = handle.clone();
    drop(handle);
    let mut freed = Vec::new();
    assets.free_unheld(&mut freed);
    assert!(freed.is_empty(), "a clone still holds it");
    drop(copy);
    assets.free_unheld(&mut freed);
    assert_eq!(freed, vec![id]);
    assert!(assets.get(id).is_none());
  }

  #[test]
  fn a_stale_id_finds_nothing_in_its_reused_slot() {
    let mut assets = Assets::<Text>::default();
    let old = assets.add(Text("old".into())).id();
    assets.free_unheld(&mut Vec::new());
    let new = assets.add(Text("new".into()));
    assert_eq!(new.id().index(), old.index(), "the slot is reused");
    assert!(assets.get(old).is_none());
    assert_eq!(assets.get(&new).map(|t| t.0.as_str()), Some("new"));
  }

  #[test]
  fn a_new_handle_before_the_drop_is_seen_keeps_the_asset() {
    let mut assets = Assets::<Text>::default();
    let ledger = assets.ledger();
    let id = assets.add(Text("a".into())).id();
    // The handle is gone, but the store has not seen it yet; a load of the
    // same asset hands out a new handle
    let again = ledger
      .lock()
      .unwrap_or_else(PoisonError::into_inner)
      .strong(id);
    assert!(again.is_some());
    let mut freed = Vec::new();
    assets.free_unheld(&mut freed);
    assert!(freed.is_empty());
    assert_eq!(assets.get(id).map(|t| t.0.as_str()), Some("a"));
  }

  #[test]
  fn changing_an_asset_moves_its_version_on() {
    let mut assets = Assets::<Text>::default();
    let handle = assets.add(Text("a".into()));
    let before = assets.version(&handle);
    if let Some(text) = assets.get_mut(&handle) {
      text.0.push('b');
    }
    assert!(assets.version(&handle) > before);
  }

  #[test]
  fn a_handle_outliving_its_store_drops_quietly() {
    let mut assets = Assets::<Text>::default();
    let handle = assets.add(Text("a".into()));
    drop(assets);
    drop(handle);
  }
}
