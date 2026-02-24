use std::marker::PhantomData;

/// A generational index into a slot array.
/// The type parameter `M` prevents mixing handles from different slot maps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Handle<M> {
  pub(crate) index: u32,
  pub(crate) generation: u32,
  pub(crate) _marker: PhantomData<M>,
}

/// Marker type for individual textures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TextureMarker;

/// Marker type for texture arrays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TextureArrayMarker;

/// Handle to an individual GPU texture managed by TextureManager.
pub type TextureHandle = Handle<TextureMarker>;

/// Handle to a GPU texture array managed by TextureManager.
pub type TextureArrayHandle = Handle<TextureArrayMarker>;

/// A slot that tracks its generation for stale-handle detection.
pub(crate) struct Slot<T> {
  pub(crate) value: Option<T>,
  pub(crate) generation: u32,
}

/// Generic slot-based allocator with generational handles.
pub(crate) struct SlotMap<M, T> {
  slots: Vec<Slot<T>>,
  free_list: Vec<u32>,
  count: usize,
  _marker: PhantomData<M>,
}

impl<M, T> SlotMap<M, T> {
  /// Creates an empty slot map.
  pub fn new() -> Self {
    Self {
      slots: Vec::new(),
      free_list: Vec::new(),
      count: 0,
      _marker: PhantomData,
    }
  }

  /// Inserts a value, returning a handle to it.
  ///
  /// Reuses a free slot if available, otherwise appends a new one.
  pub fn insert(&mut self, value: T) -> Handle<M> {
    let index = if let Some(free_index) = self.free_list.pop() {
      let slot = &mut self.slots[free_index as usize];
      slot.value = Some(value);
      free_index
    } else {
      let index = self.slots.len() as u32;
      self.slots.push(Slot {
        value: Some(value),
        generation: 0,
      });
      index
    };

    self.count += 1;

    Handle {
      index,
      generation: self.slots[index as usize].generation,
      _marker: PhantomData,
    }
  }

  /// Returns a reference to the value behind `handle`, or `None` if the
  /// handle is stale or out of bounds.
  pub fn get(&self, handle: Handle<M>) -> Option<&T> {
    let slot = self.slots.get(handle.index as usize)?;
    if slot.generation != handle.generation {
      return None;
    }
    slot.value.as_ref()
  }

  /// Returns a mutable reference to the value behind `handle`, or `None` if
  /// the handle is stale or out of bounds.
  #[allow(dead_code)]
  pub fn get_mut(&mut self, handle: Handle<M>) -> Option<&mut T> {
    let slot = self.slots.get_mut(handle.index as usize)?;
    if slot.generation != handle.generation {
      return None;
    }
    slot.value.as_mut()
  }

  /// Removes and returns the value behind `handle`, or `None` if the handle
  /// is stale or out of bounds.
  ///
  /// The slot's generation is incremented so that any existing handles
  /// pointing to this slot become invalid.
  pub fn remove(&mut self, handle: Handle<M>) -> Option<T> {
    let slot = self.slots.get_mut(handle.index as usize)?;
    if slot.generation != handle.generation {
      return None;
    }
    let value = slot.value.take()?;
    slot.generation = slot.generation.wrapping_add(1);
    self.free_list.push(handle.index);
    self.count -= 1;
    Some(value)
  }

  /// Returns the number of occupied slots.
  #[allow(dead_code)]
  pub fn len(&self) -> usize {
    self.count
  }

  /// Returns true if there are no occupied slots.
  #[allow(dead_code)]
  pub fn is_empty(&self) -> bool {
    self.count == 0
  }
}

impl<M, T> Default for SlotMap<M, T> {
  fn default() -> Self {
    Self::new()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  /// Test marker type for unit tests.
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
  struct TestMarker;

  type TestMap<T> = SlotMap<TestMarker, T>;
  type TestHandle = Handle<TestMarker>;

  #[test]
  fn test_insert_and_get() {
    let mut map = TestMap::new();
    let handle: TestHandle = map.insert(42);
    assert_eq!(map.get(handle), Some(&42));
  }

  #[test]
  fn test_generation_invalidation() {
    let mut map = TestMap::new();
    let old_handle = map.insert("first".to_string());
    map.remove(old_handle);
    let _new_handle = map.insert("second".to_string());

    // The old handle must no longer resolve.
    assert_eq!(map.get(old_handle), None);
  }

  #[test]
  fn test_multiple_inserts() {
    let mut map = TestMap::new();
    let h1 = map.insert(10);
    let h2 = map.insert(20);
    let h3 = map.insert(30);

    assert_eq!(map.get(h1), Some(&10));
    assert_eq!(map.get(h2), Some(&20));
    assert_eq!(map.get(h3), Some(&30));
    assert_eq!(map.len(), 3);
  }

  #[test]
  fn test_free_list_reuse() {
    let mut map = TestMap::new();
    let h1 = map.insert(100);
    let original_index = h1.index;
    map.remove(h1);

    let h2 = map.insert(200);
    // The new handle should reuse the same slot index.
    assert_eq!(h2.index, original_index);
    assert_eq!(map.get(h2), Some(&200));
  }

  #[test]
  fn test_remove_returns_value() {
    let mut map = TestMap::new();
    let handle = map.insert("hello".to_string());
    let removed = map.remove(handle);
    assert_eq!(removed, Some("hello".to_string()));
    assert_eq!(map.len(), 0);
  }

  #[test]
  fn test_double_remove() {
    let mut map = TestMap::new();
    let handle = map.insert(99);
    let first = map.remove(handle);
    assert_eq!(first, Some(99));

    // Second remove with the now-stale handle must return None.
    let second = map.remove(handle);
    assert_eq!(second, None);
  }

  #[test]
  fn test_get_mut() {
    let mut map = TestMap::new();
    let handle = map.insert(42);
    if let Some(val) = map.get_mut(handle) {
      *val = 100;
    }
    assert_eq!(map.get(handle), Some(&100));
  }
}
