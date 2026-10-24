use std::any::TypeId;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;

use super::Asset;

/// An asset of type `T` by its slot: the slot's index and generation. A
/// slot's generation moves on when the slot is reused, so a stale id finds
/// nothing, never a newer asset. An id does not keep its asset alive; a
/// `Handle` does
pub struct AssetId<T: Asset> {
  index: u32,
  generation: u32,
  marker: PhantomData<fn() -> T>,
}

impl<T: Asset> AssetId<T> {
  pub(crate) const fn new(index: u32, generation: u32) -> Self {
    Self {
      index,
      generation,
      marker: PhantomData,
    }
  }

  /// The slot's index
  pub fn index(self) -> u32 {
    self.index
  }

  /// The slot's generation when this id was made
  pub fn generation(self) -> u32 {
    self.generation
  }

  /// The id with its type erased, for tables over every asset type
  pub fn untyped(self) -> UntypedAssetId {
    UntypedAssetId {
      type_id: TypeId::of::<T>(),
      index: self.index,
      generation: self.generation,
    }
  }
}

impl<T: Asset> Clone for AssetId<T> {
  fn clone(&self) -> Self {
    *self
  }
}
impl<T: Asset> Copy for AssetId<T> {}
impl<T: Asset> PartialEq for AssetId<T> {
  fn eq(&self, other: &Self) -> bool {
    (self.index, self.generation) == (other.index, other.generation)
  }
}
impl<T: Asset> Eq for AssetId<T> {}
impl<T: Asset> Hash for AssetId<T> {
  fn hash<H: Hasher>(&self, state: &mut H) {
    (self.index, self.generation).hash(state);
  }
}
impl<T: Asset> fmt::Debug for AssetId<T> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    let name = std::any::type_name::<T>()
      .rsplit("::")
      .next()
      .unwrap_or("?");
    write!(f, "AssetId<{name}>({}v{})", self.index, self.generation)
  }
}

/// An asset id of any type, for a group that spans types (`progress`).
/// Typed ids and handles convert into it (`From`), so a group of one type
/// is its handles: `progress(&handles)`, or `progress([&a, &b])`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UntypedAssetId {
  pub(crate) type_id: TypeId,
  pub(crate) index: u32,
  pub(crate) generation: u32,
}

impl UntypedAssetId {
  /// The typed id, if it is of type `T`
  pub fn typed<T: Asset>(self) -> Option<AssetId<T>> {
    (self.type_id == TypeId::of::<T>()).then(|| AssetId::new(self.index, self.generation))
  }
}

impl<T: Asset> From<AssetId<T>> for UntypedAssetId {
  fn from(id: AssetId<T>) -> Self {
    id.untyped()
  }
}

impl<T: Asset> From<&AssetId<T>> for UntypedAssetId {
  fn from(id: &AssetId<T>) -> Self {
    id.untyped()
  }
}

impl From<&UntypedAssetId> for UntypedAssetId {
  fn from(id: &UntypedAssetId) -> Self {
    *id
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  struct A;
  impl Asset for A {}
  struct B;
  impl Asset for B {}

  #[test]
  fn typed_round_trips_and_rejects_another_type() {
    let id = AssetId::<A>::new(3, 7);
    let erased = id.untyped();
    assert_eq!(erased.typed::<A>(), Some(id));
    assert_eq!(erased.typed::<B>(), None);
  }
}
