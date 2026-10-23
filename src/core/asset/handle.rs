use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::sync::Arc;

use crossbeam_channel::Sender;

use super::{Asset, AssetId};

/// What a slot's strong handles share; dropping the last one tells the store
pub(crate) struct Shared {
  pub(crate) index: u32,
  pub(crate) generation: u32,
  pub(crate) drops: Sender<(u32, u32)>,
}

impl Drop for Shared {
  fn drop(&mut self) {
    // The store may be gone already (the world is being torn down); nothing to tell then
    let _ = self.drops.send((self.index, self.generation));
  }
}

/// A strong reference to an asset of type `T`: while any clone of it lives,
/// the asset stays loaded. Cloning bumps a count; dropping the last clone
/// frees the asset at the next pump. Compares and hashes by id
pub struct Handle<T: Asset> {
  shared: Arc<Shared>,
  marker: PhantomData<fn() -> T>,
}

impl<T: Asset> Handle<T> {
  pub(crate) fn from_shared(shared: Arc<Shared>) -> Self {
    Self {
      shared,
      marker: PhantomData,
    }
  }

  /// A handle to no slot, for an asset with nowhere to live (its type is not
  /// registered): its index and generation are never live in a store, so it
  /// finds nothing, even once the type is registered
  pub(crate) fn detached() -> Self {
    let (drops, _) = crossbeam_channel::unbounded();
    Self::from_shared(Arc::new(Shared {
      index: u32::MAX,
      generation: u32::MAX,
      drops,
    }))
  }

  /// The asset's id, which does not keep it alive
  pub fn id(&self) -> AssetId<T> {
    AssetId::new(self.shared.index, self.shared.generation)
  }
}

impl<T: Asset> Clone for Handle<T> {
  fn clone(&self) -> Self {
    Self {
      shared: Arc::clone(&self.shared),
      marker: PhantomData,
    }
  }
}
impl<T: Asset> PartialEq for Handle<T> {
  fn eq(&self, other: &Self) -> bool {
    self.id() == other.id()
  }
}
impl<T: Asset> Eq for Handle<T> {}
impl<T: Asset> Hash for Handle<T> {
  fn hash<H: Hasher>(&self, state: &mut H) {
    self.id().hash(state);
  }
}
impl<T: Asset> fmt::Debug for Handle<T> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "Handle({:?})", self.id())
  }
}
impl<T: Asset> From<&Handle<T>> for AssetId<T> {
  fn from(handle: &Handle<T>) -> Self {
    handle.id()
  }
}
