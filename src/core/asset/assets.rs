use std::sync::atomic::{AtomicU64, Ordering};
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
    self.is_live_slot(id.index(), id.generation())
  }

  /// Whether the slot at `index` is live in `generation`
  pub(crate) fn is_live_slot(&self, index: u32, generation: u32) -> bool {
    self
      .slots
      .get(index as usize)
      .is_some_and(|s| s.live && s.generation == generation)
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
#[derive(Resource)]
pub struct Assets<T: Asset> {
  ledger: Arc<Mutex<Ledger>>,
  drops: Receiver<(u32, u32)>,
  values: Vec<Option<Entry<T>>>,
  next_version: u64,
  /// The slots changed since the follower last took them (`take_changes`);
  /// `None` until it first does, so a store nobody follows lists nothing
  changes: Option<Changes>,
  /// Tells this store from every other, so a follower handed another starts over
  serial: u64,
}

struct Entry<T> {
  generation: u32,
  version: u64,
  value: T,
}

/// The slots changed since the last `take_changes`, each listed once
#[derive(Default)]
struct Changes {
  slots: Vec<u32>,
  /// By slot: whether it is in `slots`
  listed: Vec<bool>,
}

/// Lists slot `index` as changed, once until the changes are next taken
fn note(changes: &mut Option<Changes>, index: u32) {
  let Some(changes) = changes else {
    return;
  };
  let at = index as usize;
  if changes.listed.len() <= at {
    changes.listed.resize(at + 1, false);
  }
  if let Some(listed) = changes.listed.get_mut(at)
    && !*listed
  {
    *listed = true;
    changes.slots.push(index);
  }
}

/// What a changed slot holds, as `Assets::take_changes` gives it
pub(crate) struct Stored<'a, T> {
  /// The live id's generation
  pub(crate) generation: u32,
  /// The content version
  pub(crate) version: u64,
  /// The asset
  pub(crate) value: &'a T,
}

/// What a slot holds, as a follower sees it
fn stored<T>(entry: &Option<Entry<T>>) -> Option<Stored<'_, T>> {
  entry.as_ref().map(|entry| Stored {
    generation: entry.generation,
    version: entry.version,
    value: &entry.value,
  })
}

static NEXT_SERIAL: AtomicU64 = AtomicU64::new(0);

impl<T: Asset> Default for Assets<T> {
  fn default() -> Self {
    let (sender, drops) = unbounded();
    Self {
      ledger: Arc::new(Mutex::new(Ledger::new(sender))),
      drops,
      values: Vec::new(),
      next_version: 1,
      changes: None,
      serial: NEXT_SERIAL.fetch_add(1, Ordering::Relaxed),
    }
  }
}

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
    note(&mut self.changes, id.index());
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

  /// Whether no asset is loaded
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
    note(&mut self.changes, id.index());
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
        note(&mut self.changes, index);
        freed.push(AssetId::new(index, generation));
      }
    }
  }

  /// Which store this is: no two stores share a serial
  pub(crate) fn serial(&self) -> u64 {
    self.serial
  }

  /// Gives `seen` each slot changed since the last call (an asset added or
  /// loaded into it, changed through `get_mut`, or freed) once, with what
  /// it holds now (`None`: nothing). The first call, or one asking for the
  /// `whole` store, gives every slot. For the one follower that mirrors the
  /// store (the UI's image sizes): a second would take the first's changes
  pub(crate) fn take_changes(
    &mut self,
    whole: bool,
    mut seen: impl FnMut(u32, Option<Stored<'_, T>>),
  ) {
    match &mut self.changes {
      Some(changes) if !whole => {
        for index in changes.slots.drain(..) {
          if let Some(listed) = changes.listed.get_mut(index as usize) {
            *listed = false;
          }
          seen(index, self.values.get(index as usize).and_then(stored));
        }
      }
      changes => {
        // Every slot is given now: the list starts over
        let changes = changes.get_or_insert_with(Changes::default);
        changes.slots.clear();
        changes.listed.clear();
        for (index, entry) in self.values.iter().enumerate() {
          if let Ok(index) = u32::try_from(index) {
            seen(index, stored(entry));
          }
        }
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

  /// A slot `take_changes` gave, with its generation, version and text
  type Taken = (u32, Option<(u32, u64, String)>);

  /// What `take_changes` gives
  fn taken(assets: &mut Assets<Text>, whole: bool) -> Vec<Taken> {
    let mut taken = Vec::new();
    assets.take_changes(whole, |index, now| {
      taken.push((
        index,
        now.map(|now| (now.generation, now.version, now.value.0.clone())),
      ));
    });
    taken
  }

  #[test]
  fn each_changed_slot_is_given_once_with_what_it_holds_now() {
    let mut assets = Assets::<Text>::default();
    let a = assets.add(Text("a".into()));
    let b = assets.add(Text("b".into()));
    assert!(
      assets.changes.is_none(),
      "nothing listed before a follower asks"
    );
    assert_eq!(
      taken(&mut assets, false).len(),
      2,
      "the first take: every slot"
    );
    assert!(
      taken(&mut assets, false).is_empty(),
      "nothing changed since"
    );

    assets.get_mut(&a).unwrap().0 = "a2".into();
    assets.get_mut(&a).unwrap().0 = "a3".into();
    let c = assets.add(Text("c".into()));
    drop(b);
    assets.free_unheld(&mut Vec::new());
    let (va, vc) = (assets.version(&a).unwrap(), assets.version(&c).unwrap());
    assert_eq!(
      taken(&mut assets, false),
      vec![
        (0, Some((0, va, "a3".into()))),
        (2, Some((0, vc, "c".into()))),
        (1, None),
      ]
    );
    assert!(taken(&mut assets, false).is_empty());
  }

  #[test]
  fn a_whole_take_gives_every_slot_and_starts_the_list_over() {
    let mut assets = Assets::<Text>::default();
    let a = assets.add(Text("a".into()));
    taken(&mut assets, false);
    assets.get_mut(&a).unwrap().0 = "a2".into();
    let whole = taken(&mut assets, true);
    assert_eq!(whole.len(), 1);
    assert!(
      taken(&mut assets, false).is_empty(),
      "the change was in the whole"
    );
    assets.get_mut(&a).unwrap().0 = "a3".into();
    assert_eq!(
      taken(&mut assets, false).len(),
      1,
      "and is listed again after"
    );
  }

  #[test]
  fn no_two_stores_share_a_serial() {
    let (a, b) = (Assets::<Text>::default(), Assets::<Text>::default());
    assert_ne!(a.serial(), b.serial());
  }

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
  fn an_unheld_handle_equals_its_assets_handles_and_holds_nothing() {
    let mut assets = Assets::<Text>::default();
    let handle = assets.add(Text("a".into()));
    let id = handle.id();
    let unheld = Handle::<Text>::unheld(id);
    assert_eq!(unheld, handle, "equal, as handles compare by id");
    drop(handle);
    let mut freed = Vec::new();
    assets.free_unheld(&mut freed);
    assert_eq!(freed, vec![id], "the unheld handle did not keep it");
    drop(unheld);
    assets.free_unheld(&mut freed);
    assert_eq!(freed, vec![id], "and its drop tells the store nothing");
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
  fn a_rebound_slot_dropped_again_is_freed_exactly_once() {
    let mut assets = Assets::<Text>::default();
    let ledger = assets.ledger();
    let id = assets.add(Text("a".into())).id();
    // Every handle dropped, then the slot rebound before the store sees it
    let again = ledger
      .lock()
      .unwrap_or_else(PoisonError::into_inner)
      .strong(id);
    let mut freed = Vec::new();
    assets.free_unheld(&mut freed);
    assert!(freed.is_empty(), "the rebound handle holds it");
    drop(again);
    assets.free_unheld(&mut freed);
    assert_eq!(freed, vec![id]);
    assets.free_unheld(&mut freed);
    assert_eq!(freed, vec![id], "freed once");
    // The slot's next asset is not freed by anything the old one left behind
    let next = assets.add(Text("b".into()));
    assert_eq!(next.id().index(), id.index());
    assets.free_unheld(&mut freed);
    assert_eq!(freed, vec![id]);
    assert_eq!(assets.get(&next).map(|t| t.0.as_str()), Some("b"));
  }

  #[test]
  fn both_drop_notices_pending_free_the_slot_once() {
    let mut assets = Assets::<Text>::default();
    let ledger = assets.ledger();
    let id = assets.add(Text("a".into())).id();
    let again = ledger
      .lock()
      .unwrap_or_else(PoisonError::into_inner)
      .strong(id);
    drop(again);
    let mut freed = Vec::new();
    assets.free_unheld(&mut freed);
    assert_eq!(freed, vec![id]);
  }

  #[test]
  fn insert_with_a_stale_id_changes_nothing() {
    let mut assets = Assets::<Text>::default();
    let old = assets.add(Text("old".into())).id();
    assets.free_unheld(&mut Vec::new());
    assert!(!assets.insert(old, Text("stale".into())), "a freed slot");
    assert!(assets.is_empty());
    let new = assets.add(Text("new".into()));
    let version = assets.version(&new);
    assert!(!assets.insert(old, Text("stale".into())), "a reused slot");
    assert_eq!(assets.get(&new).map(|t| t.0.as_str()), Some("new"));
    assert_eq!(assets.version(&new), version);
    assert_eq!(assets.len(), 1);
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
