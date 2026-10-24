use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError, Weak};

use crossbeam_channel::{Receiver, Sender, unbounded};

use super::handle::Shared;
use super::{Asset, AssetEvent, AssetId, Handle};
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

/// Every asset of type `T`: a slot table. A world resource per type, made
/// when the type is registered.
///
/// A store comes only from the world: `ctx.assets()`, `Res<Assets<T>>` and
/// `ResMut<Assets<T>>` in systems, `HeadlessEngine::assets()` and
/// `ryo2d::testing::asset_world()` in tests. A handle names a slot of the
/// store that made it, and everything that resolves handles (the renderer,
/// the UI, `ctx.assets().get`) reads the world's store, so a handle from a
/// store of its own would name nothing there, or another asset (the first
/// image is the engine's checker). So a store has no public constructor:
///
/// ```compile_fail,E0599
/// use ryo2d::prelude::*;
///
/// let images = Assets::<Image>::default();
/// ```
///
/// and a world makes one only through the registry:
///
/// ```compile_fail,E0277
/// use ryo2d::prelude::*;
///
/// World::new().init_resource::<Assets<Image>>();
/// ```
#[derive(Resource)]
pub struct Assets<T: Asset> {
  ledger: Arc<Mutex<Ledger>>,
  drops: Receiver<(u32, u32)>,
  values: Vec<Option<Entry<T>>>,
  next_version: u64,
  /// The slots changed since its follower last took them (`take_changes`);
  /// `None` until one first does, so a store nobody follows lists nothing
  changes: Option<Changes>,
  /// Tells this store from every other, so a follower handed another starts over
  serial: u64,
  /// The changes made in code (`add`, `insert`, `get_mut`) for the pump to
  /// send as events; `None` in a store no server owns
  events: Option<Queued>,
}

/// What a slot's queued event is
#[derive(Clone, Copy, PartialEq, Eq)]
enum Queue {
  Added,
  Modified,
}

/// Events waiting for the pump, one per slot: the first change decides it
/// (an asset added, then changed, is only added)
#[derive(Default)]
struct Queued {
  /// The slots with an event, in the order they first changed
  order: Vec<u32>,
  /// By slot: its event
  kinds: Vec<Option<Queue>>,
}

struct Entry<T> {
  generation: u32,
  version: u64,
  value: T,
}

/// The slots changed since the last `take_changes`, each listed once, for
/// the one follower that takes them
struct Changes {
  slots: Vec<u32>,
  /// By slot: whether it is in `slots`
  listed: Vec<bool>,
  /// The follower the list is kept for, and the one before it
  follower: u64,
  before: Option<u64>,
}

/// A token no follower of a change list has had: each follower holds its own
pub(crate) fn follower_token() -> u64 {
  static NEXT: AtomicU64 = AtomicU64::new(1);
  NEXT.fetch_add(1, Ordering::Relaxed)
}

/// Queues `kind` for slot `index`, unless it has an event already
fn queue(events: &mut Option<Queued>, index: u32, kind: Queue) {
  let Some(queued) = events else {
    return;
  };
  let at = index as usize;
  if queued.kinds.len() <= at {
    queued.kinds.resize(at + 1, None);
  }
  if let Some(slot) = queued.kinds.get_mut(at)
    && slot.is_none()
  {
    *slot = Some(kind);
    queued.order.push(index);
  }
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

impl<T: Asset> Assets<T> {
  /// An empty store. Crate-only, and not `Default`, so a store is made
  /// only by the registry (`register_asset`) and by the crate's own tests
  pub(crate) fn new() -> Self {
    let (sender, drops) = unbounded();
    Self {
      ledger: Arc::new(Mutex::new(Ledger::new(sender))),
      drops,
      values: Vec::new(),
      next_version: 1,
      changes: None,
      serial: NEXT_SERIAL.fetch_add(1, Ordering::Relaxed),
      events: None,
    }
  }

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

  /// The asset for changing in place; its content version moves on, and
  /// the next pump sends `AssetEvent::Modified` (once, however many times
  /// it is changed before then)
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
    queue(&mut self.events, id.index(), Queue::Modified);
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

  /// Puts a value in a reserved (or loaded) slot, `Added` (or `Modified`)
  /// queued for the pump; false if `id` is stale
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
    let kind = match slot {
      Some(entry) if entry.generation == id.generation() => Queue::Modified,
      _ => Queue::Added,
    };
    queue(&mut self.events, id.index(), kind);
    *slot = Some(Entry {
      generation: id.generation(),
      version: self.next_version,
      value,
    });
    self.next_version += 1;
    note(&mut self.changes, id.index());
    true
  }

  /// Frees every asset whose last handle dropped, adding their ids to
  /// `freed`, each with whether it held a value (a load dropped before it
  /// landed, or one that failed, did not)
  pub(crate) fn free_unheld(&mut self, freed: &mut Vec<(AssetId<T>, bool)>) {
    if self.drops.is_empty() {
      return;
    }
    let mut ledger = self.ledger.lock().unwrap_or_else(PoisonError::into_inner);
    for (index, generation) in self.drops.try_iter() {
      if ledger.release(index, generation) {
        let held = self
          .values
          .get_mut(index as usize)
          .and_then(Option::take)
          .is_some_and(|entry| entry.generation == generation);
        note(&mut self.changes, index);
        freed.push((AssetId::new(index, generation), held));
      }
    }
  }

  /// Which store this is: no two stores share a serial
  pub(crate) fn serial(&self) -> u64 {
    self.serial
  }

  /// From now on, changes made in code are queued as events for the pump
  /// to send (the server does this for the stores it registers)
  pub(crate) fn send_events(&mut self) {
    self.events.get_or_insert_with(Queued::default);
  }

  /// Whether an event waits for the pump
  pub(crate) fn has_events(&self) -> bool {
    self
      .events
      .as_ref()
      .is_some_and(|queued| !queued.order.is_empty())
  }

  /// Gives `send` each queued event, in the order its slot first changed
  pub(crate) fn take_events(&mut self, mut send: impl FnMut(AssetEvent<T>)) {
    let Some(queued) = &mut self.events else {
      return;
    };
    for index in queued.order.drain(..) {
      let kind = queued.kinds.get_mut(index as usize).and_then(Option::take);
      let live = self.values.get(index as usize).and_then(Option::as_ref);
      if let (Some(kind), Some(entry)) = (kind, live) {
        let id = AssetId::new(index, entry.generation);
        send(match kind {
          Queue::Added => AssetEvent::Added { id },
          Queue::Modified => AssetEvent::Modified { id },
        });
      }
    }
  }

  /// Drops `id`'s queued event: the pump sends its own for it. A stale
  /// `id` drops nothing, as its slot's event is another asset's
  pub(crate) fn unqueue(&mut self, id: AssetId<T>) {
    let live = self
      .values
      .get(id.index() as usize)
      .and_then(Option::as_ref)
      .is_some_and(|entry| entry.generation == id.generation());
    if !live {
      return;
    }
    if let Some(kind) = self
      .events
      .as_mut()
      .and_then(|queued| queued.kinds.get_mut(id.index() as usize))
    {
      *kind = None;
    }
  }

  /// Whether `follower` is the one this store's change list is kept for
  pub(crate) fn is_followed_by(&self, follower: u64) -> bool {
    self
      .changes
      .as_ref()
      .is_some_and(|changes| changes.follower == follower)
  }

  /// Gives `seen` each slot changed since `follower` last took them (an
  /// asset added or loaded into it, changed through `get_mut`, or freed)
  /// once, with what it holds now (`None`: nothing). A store keeps one list,
  /// for one follower (`follower_token`): a follower it is not kept for (the
  /// first, or another) gets every slot, and the list is kept for it from
  /// then on, so no follower misses a change. So does one asking for the
  /// `whole` store (one that followed another store meanwhile). Two
  /// followers taking turns would each be given the whole store every time:
  /// a debug assertion catches them
  pub(crate) fn take_changes(
    &mut self,
    follower: u64,
    whole: bool,
    mut seen: impl FnMut(u32, Option<Stored<'_, T>>),
  ) {
    match &mut self.changes {
      Some(changes) if !whole && changes.follower == follower => {
        for index in changes.slots.drain(..) {
          if let Some(listed) = changes.listed.get_mut(index as usize) {
            *listed = false;
          }
          seen(index, self.values.get(index as usize).and_then(stored));
        }
      }
      changes => {
        let changes = changes.get_or_insert_with(|| Changes {
          slots: Vec::new(),
          listed: Vec::new(),
          follower,
          before: None,
        });
        if changes.follower != follower {
          debug_assert!(
            changes.before != Some(follower),
            "two followers take turns on one store's change list: a store has one follower"
          );
          changes.before = Some(changes.follower);
          changes.follower = follower;
        }
        // Every slot is given now: the list starts over
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
  use crate::core::asset::{Asset, AssetEvent};

  struct Text(String);
  impl Asset for Text {}

  /// A slot `take_changes` gave, with its generation, version and text
  type Taken = (u32, Option<(u32, u64, String)>);

  /// The follower the single-follower tests take changes as
  const FOLLOWER: u64 = u64::MAX;

  /// What `take_changes` gives
  fn taken(assets: &mut Assets<Text>, whole: bool) -> Vec<Taken> {
    taken_by(assets, FOLLOWER, whole)
  }

  /// What `take_changes` gives `follower`
  fn taken_by(assets: &mut Assets<Text>, follower: u64, whole: bool) -> Vec<Taken> {
    let mut taken = Vec::new();
    assets.take_changes(follower, whole, |index, now| {
      taken.push((
        index,
        now.map(|now| (now.generation, now.version, now.value.0.clone())),
      ));
    });
    taken
  }

  #[test]
  fn each_changed_slot_is_given_once_with_what_it_holds_now() {
    let mut assets = Assets::<Text>::new();
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
    let mut assets = Assets::<Text>::new();
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
  fn a_second_follower_gets_the_whole_store_then_its_own_changes() {
    let mut assets = Assets::<Text>::new();
    let a = assets.add(Text("a".into()));
    let kept = assets.add(Text("kept".into()));
    let (first, second) = (follower_token(), follower_token());
    assert_ne!(first, second);
    assert_eq!(taken_by(&mut assets, first, false).len(), 2);
    assert!(assets.is_followed_by(first));
    assets.get_mut(&a).unwrap().0 = "a2".into();
    let b = assets.add(Text("b".into()));
    // Not the first's list (a and b): every slot, as it is now, so the
    // second misses nothing, the unchanged one included
    let versions = [&a, &kept, &b].map(|handle| assets.version(handle).unwrap());
    assert_eq!(
      taken_by(&mut assets, second, false),
      vec![
        (0, Some((0, versions[0], "a2".into()))),
        (1, Some((0, versions[1], "kept".into()))),
        (2, Some((0, versions[2], "b".into()))),
      ]
    );
    // The list is the second's now: its own changes from here
    assert!(assets.is_followed_by(second) && !assets.is_followed_by(first));
    assert!(taken_by(&mut assets, second, false).is_empty());
    assets.get_mut(&b).unwrap().0 = "b2".into();
    assert_eq!(taken_by(&mut assets, second, false).len(), 1);
  }

  #[cfg(debug_assertions)]
  #[test]
  #[should_panic(expected = "two followers")]
  fn followers_taking_turns_on_one_store_are_caught() {
    let mut assets = Assets::<Text>::new();
    assets.add(Text("a".into()));
    let (first, second) = (follower_token(), follower_token());
    taken_by(&mut assets, first, false);
    taken_by(&mut assets, second, false);
    // Back to the first: each would take the other's changes
    taken_by(&mut assets, first, false);
  }

  #[test]
  fn a_follower_handed_the_store_back_gets_the_whole_store() {
    // Three followers, so the first comes back after a third, not straight
    // after the one that took the list from it (which the check catches)
    let mut assets = Assets::<Text>::new();
    let a = assets.add(Text("a".into()));
    let kept = assets.add(Text("kept".into()));
    let [first, second, third] = [(); 3].map(|()| follower_token());
    taken_by(&mut assets, first, false);
    assets.get_mut(&a).unwrap().0 = "a2".into();
    taken_by(&mut assets, second, false);
    taken_by(&mut assets, third, false);
    // The changes the others took are not lost to it: every slot, as it is
    let back = taken_by(&mut assets, first, false);
    let texts: Vec<_> = back
      .iter()
      .map(|(index, now)| (*index, now.as_ref().map(|now| now.2.clone())))
      .collect();
    assert_eq!(
      texts,
      vec![(0, Some("a2".to_string())), (1, Some("kept".to_string()))]
    );
    assert!(assets.is_followed_by(first));
    drop(kept);
  }

  #[test]
  fn a_store_no_server_owns_queues_no_events() {
    let mut assets = Assets::<Text>::new();
    let a = assets.add(Text("a".into()));
    assets.get_mut(&a).unwrap().0 = "a2".into();
    let mut sent = Vec::new();
    assets.take_events(|event| sent.push(event));
    assert!(sent.is_empty() && assets.events.is_none());
    // A store a server owns queues them, each id once
    assets.send_events();
    let b = assets.add(Text("b".into()));
    assets.get_mut(&a).unwrap().0 = "a3".into();
    assets.get_mut(&a).unwrap().0 = "a4".into();
    assets.take_events(|event| sent.push(event));
    assert!(
      matches!(sent.as_slice(), [AssetEvent::Added { id: added }, AssetEvent::Modified { id: modified }]
        if *added == b.id() && *modified == a.id()),
      "{sent:?}"
    );
  }

  #[test]
  fn unqueueing_a_stale_id_leaves_its_slots_new_event() {
    let mut assets = Assets::<Text>::new();
    assets.send_events();
    let old = assets.add(Text("old".into())).id();
    assets.take_events(|_| {});
    assets.free_unheld(&mut Vec::new());
    let new = assets.add(Text("new".into()));
    assert_eq!(new.id().index(), old.index(), "the slot is reused");
    assert!(!assets.insert(old, Text("stale".into())));
    assets.unqueue(old);
    let mut sent = Vec::new();
    assets.take_events(|event| sent.push(event));
    assert!(
      matches!(sent.as_slice(), [AssetEvent::Added { id }] if *id == new.id()),
      "the new asset's event stays: {sent:?}"
    );
  }

  #[test]
  fn no_two_stores_share_a_serial() {
    let (a, b) = (Assets::<Text>::new(), Assets::<Text>::new());
    assert_ne!(a.serial(), b.serial());
  }

  #[test]
  fn an_added_asset_is_got_by_its_handle() {
    let mut assets = Assets::<Text>::new();
    let handle = assets.add(Text("a".into()));
    assert_eq!(assets.get(&handle).map(|t| t.0.as_str()), Some("a"));
    assert_eq!(assets.len(), 1);
  }

  #[test]
  fn the_last_dropped_handle_frees_its_asset_at_the_next_free() {
    let mut assets = Assets::<Text>::new();
    let handle = assets.add(Text("a".into()));
    let id = handle.id();
    let copy = handle.clone();
    drop(handle);
    let mut freed = Vec::new();
    assets.free_unheld(&mut freed);
    assert!(freed.is_empty(), "a clone still holds it");
    drop(copy);
    assets.free_unheld(&mut freed);
    assert_eq!(freed, vec![(id, true)]);
    assert!(assets.get(id).is_none());
  }

  #[test]
  fn an_unheld_handle_equals_its_assets_handles_and_holds_nothing() {
    let mut assets = Assets::<Text>::new();
    let handle = assets.add(Text("a".into()));
    let id = handle.id();
    let unheld = Handle::<Text>::unheld(id);
    assert_eq!(unheld, handle, "equal, as handles compare by id");
    drop(handle);
    let mut freed = Vec::new();
    assets.free_unheld(&mut freed);
    assert_eq!(freed, vec![(id, true)], "the unheld handle did not keep it");
    drop(unheld);
    assets.free_unheld(&mut freed);
    assert_eq!(
      freed,
      vec![(id, true)],
      "and its drop tells the store nothing"
    );
  }

  #[test]
  fn a_stale_id_finds_nothing_in_its_reused_slot() {
    let mut assets = Assets::<Text>::new();
    let old = assets.add(Text("old".into())).id();
    assets.free_unheld(&mut Vec::new());
    let new = assets.add(Text("new".into()));
    assert_eq!(new.id().index(), old.index(), "the slot is reused");
    assert!(assets.get(old).is_none());
    assert_eq!(assets.get(&new).map(|t| t.0.as_str()), Some("new"));
  }

  #[test]
  fn a_new_handle_before_the_drop_is_seen_keeps_the_asset() {
    let mut assets = Assets::<Text>::new();
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
    let mut assets = Assets::<Text>::new();
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
    assert_eq!(freed, vec![(id, true)]);
    assets.free_unheld(&mut freed);
    assert_eq!(freed, vec![(id, true)], "freed once");
    // The slot's next asset is not freed by anything the old one left behind
    let next = assets.add(Text("b".into()));
    assert_eq!(next.id().index(), id.index());
    assets.free_unheld(&mut freed);
    assert_eq!(freed, vec![(id, true)]);
    assert_eq!(assets.get(&next).map(|t| t.0.as_str()), Some("b"));
  }

  #[test]
  fn both_drop_notices_pending_free_the_slot_once() {
    let mut assets = Assets::<Text>::new();
    let ledger = assets.ledger();
    let id = assets.add(Text("a".into())).id();
    let again = ledger
      .lock()
      .unwrap_or_else(PoisonError::into_inner)
      .strong(id);
    drop(again);
    let mut freed = Vec::new();
    assets.free_unheld(&mut freed);
    assert_eq!(freed, vec![(id, true)]);
  }

  #[test]
  fn insert_with_a_stale_id_changes_nothing() {
    let mut assets = Assets::<Text>::new();
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
    let mut assets = Assets::<Text>::new();
    let handle = assets.add(Text("a".into()));
    let before = assets.version(&handle);
    if let Some(text) = assets.get_mut(&handle) {
      text.0.push('b');
    }
    assert!(assets.version(&handle) > before);
  }

  #[test]
  fn a_handle_outliving_its_store_drops_quietly() {
    let mut assets = Assets::<Text>::new();
    let handle = assets.add(Text("a".into()));
    drop(assets);
    drop(handle);
  }
}
