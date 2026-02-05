//! NetworkedValue wrapper for preventing silent mutations
//!
//! This module provides [`NetworkedValue<T>`], a wrapper that prevents
//! accidental modification of network-replicated values without proper
//! dirty tracking.
//!
//! # Problem
//!
//! When using network replication, forgetting to mark a component dirty
//! after modification causes the change to not be sent to clients:
//!
//! ```ignore
//! // Bug: change is not replicated!
//! if let Some(mut pos) = world.get_component_mut::<Position>(entity) {
//!     pos.x += 10.0;  // Forgot to call mark_dirty!
//! }
//! ```
//!
//! # Solution
//!
//! `NetworkedValue<T>` tracks modifications automatically:
//!
//! ```ignore
//! #[derive(Component)]
//! struct Position {
//!     x: NetworkedValue<f32>,
//!     y: NetworkedValue<f32>,
//! }
//!
//! // Modification is tracked
//! if let Some(mut pos) = world.get_component_mut::<Position>(entity) {
//!     pos.x.set(pos.x.get() + 10.0);  // Marks dirty automatically
//!     // Or use modify:
//!     pos.y.modify(|y| *y += 5.0);
//! }
//!
//! // Check if changes need to be sent
//! if pos.x.is_dirty() || pos.y.is_dirty() {
//!     replication.mark_dirty::<Position>(entity);
//! }
//! ```
//!
//! The key insight is that `NetworkedValue` implements `Deref` for read access
//! but NOT `DerefMut`, forcing you to use `set()` or `modify()` which track
//! modifications.

use std::fmt;
use std::ops::Deref;

use serde::{Deserialize, Serialize};

/// A wrapper for values that need network synchronization.
///
/// `NetworkedValue<T>` prevents silent modifications by:
/// - Allowing read access via `Deref` (`.get()` or `*value`)
/// - Requiring explicit `set()` or `modify()` for writes (which mark dirty)
/// - NOT implementing `DerefMut` (compile error if you try `*value = x`)
///
/// # Example
///
/// ```
/// use ryo2d::core::network::replication::NetworkedValue;
///
/// let mut health = NetworkedValue::new(100);
///
/// // Read access (doesn't mark dirty)
/// assert_eq!(*health, 100);
/// assert_eq!(health.get(), &100);
///
/// // Write access (marks dirty)
/// health.set(80);
/// assert!(health.is_dirty());
///
/// // Modify in place
/// health.clear_dirty();
/// health.modify(|h| *h -= 10);
/// assert_eq!(*health, 70);
/// assert!(health.is_dirty());
///
/// // Server correction (doesn't mark dirty)
/// health.apply_server_correction(100);
/// assert!(!health.is_dirty());
/// ```
#[derive(Clone, Serialize, Deserialize)]
pub struct NetworkedValue<T> {
  value: T,
  #[serde(skip)]
  dirty: bool,
}

impl<T> NetworkedValue<T> {
  /// Create a new networked value.
  ///
  /// New values start as dirty since they need to be sent initially.
  #[inline]
  pub fn new(value: T) -> Self {
    Self { value, dirty: true }
  }

  /// Create a new networked value that is not dirty.
  ///
  /// Use this when initializing from a received network state.
  #[inline]
  pub fn from_network(value: T) -> Self {
    Self {
      value,
      dirty: false,
    }
  }

  /// Get a reference to the inner value.
  #[inline]
  pub fn get(&self) -> &T {
    &self.value
  }

  /// Set a new value, marking it as dirty.
  ///
  /// This marks the value dirty regardless of whether the value changed.
  /// If you want to avoid unnecessary dirty marks, compare first:
  ///
  /// ```ignore
  /// if *value != new_value {
  ///     value.set(new_value);
  /// }
  /// ```
  #[inline]
  pub fn set(&mut self, value: T) {
    self.value = value;
    self.dirty = true;
  }

  /// Modify the value in place, marking it as dirty.
  ///
  /// The closure receives a mutable reference and can return a value.
  ///
  /// # Example
  ///
  /// ```
  /// use ryo2d::core::network::replication::NetworkedValue;
  ///
  /// let mut counter = NetworkedValue::new(0);
  /// let old = counter.modify(|c| {
  ///     let old = *c;
  ///     *c += 1;
  ///     old
  /// });
  /// assert_eq!(old, 0);
  /// assert_eq!(*counter, 1);
  /// ```
  #[inline]
  pub fn modify<F, R>(&mut self, f: F) -> R
  where
    F: FnOnce(&mut T) -> R,
  {
    self.dirty = true;
    f(&mut self.value)
  }

  /// Check if the value has been modified since last clear.
  #[inline]
  pub fn is_dirty(&self) -> bool {
    self.dirty
  }

  /// Clear the dirty flag.
  ///
  /// Call this after sending the value over the network.
  #[inline]
  pub fn clear_dirty(&mut self) {
    self.dirty = false;
  }

  /// Apply a correction from the server without marking dirty.
  ///
  /// Use this when receiving authoritative state from the server
  /// that should override local state without triggering a re-send.
  ///
  /// # Example
  ///
  /// ```
  /// use ryo2d::core::network::replication::NetworkedValue;
  ///
  /// let mut position = NetworkedValue::new(0.0);
  /// position.set(10.0);  // Local prediction
  /// assert!(position.is_dirty());
  ///
  /// // Server says we're at 9.5
  /// position.apply_server_correction(9.5);
  /// assert_eq!(*position, 9.5);
  /// assert!(!position.is_dirty());  // Don't re-send corrected value
  /// ```
  #[inline]
  pub fn apply_server_correction(&mut self, value: T) {
    self.value = value;
    self.dirty = false;
  }

  /// Get the inner value, consuming the wrapper.
  #[inline]
  pub fn into_inner(self) -> T {
    self.value
  }
}

impl<T: PartialEq> NetworkedValue<T> {
  /// Set a new value only if it differs from the current value.
  ///
  /// Returns `true` if the value was changed.
  ///
  /// # Example
  ///
  /// ```
  /// use ryo2d::core::network::replication::NetworkedValue;
  ///
  /// let mut value = NetworkedValue::new(10);
  /// value.clear_dirty();
  ///
  /// assert!(!value.set_if_changed(10));  // Same value, no change
  /// assert!(!value.is_dirty());
  ///
  /// assert!(value.set_if_changed(20));   // Different value, changed
  /// assert!(value.is_dirty());
  /// ```
  #[inline]
  pub fn set_if_changed(&mut self, value: T) -> bool {
    if self.value != value {
      self.value = value;
      self.dirty = true;
      true
    } else {
      false
    }
  }
}

impl<T: Default> Default for NetworkedValue<T> {
  fn default() -> Self {
    Self::new(T::default())
  }
}

impl<T> Deref for NetworkedValue<T> {
  type Target = T;

  #[inline]
  fn deref(&self) -> &Self::Target {
    &self.value
  }
}

// NOTE: We intentionally do NOT implement DerefMut.
// This forces users to use set() or modify() which mark dirty.

impl<T: fmt::Debug> fmt::Debug for NetworkedValue<T> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_struct("NetworkedValue")
      .field("value", &self.value)
      .field("dirty", &self.dirty)
      .finish()
  }
}

impl<T: PartialEq> PartialEq for NetworkedValue<T> {
  fn eq(&self, other: &Self) -> bool {
    // Only compare values, not dirty state
    self.value == other.value
  }
}

impl<T: PartialEq + Eq> Eq for NetworkedValue<T> {}

impl<T: PartialOrd> PartialOrd for NetworkedValue<T> {
  fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
    self.value.partial_cmp(&other.value)
  }
}

impl<T: Ord> Ord for NetworkedValue<T> {
  fn cmp(&self, other: &Self) -> std::cmp::Ordering {
    self.value.cmp(&other.value)
  }
}

impl<T: std::hash::Hash> std::hash::Hash for NetworkedValue<T> {
  fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
    self.value.hash(state);
  }
}

impl<T: Copy> NetworkedValue<T> {
  /// Get a copy of the inner value.
  ///
  /// This is a convenience method for `Copy` types.
  #[inline]
  pub fn value(&self) -> T {
    self.value
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn new_is_dirty() {
    let value = NetworkedValue::new(42);
    assert!(value.is_dirty());
    assert_eq!(*value, 42);
  }

  #[test]
  fn from_network_not_dirty() {
    let value = NetworkedValue::from_network(42);
    assert!(!value.is_dirty());
  }

  #[test]
  fn set_marks_dirty() {
    let mut value = NetworkedValue::from_network(0);
    assert!(!value.is_dirty());

    value.set(10);
    assert!(value.is_dirty());
    assert_eq!(*value, 10);
  }

  #[test]
  fn modify_marks_dirty() {
    let mut value = NetworkedValue::from_network(10);
    assert!(!value.is_dirty());

    let old = value.modify(|v| {
      let old = *v;
      *v += 5;
      old
    });

    assert_eq!(old, 10);
    assert_eq!(*value, 15);
    assert!(value.is_dirty());
  }

  #[test]
  fn server_correction_not_dirty() {
    let mut value = NetworkedValue::new(0);
    value.set(10);
    assert!(value.is_dirty());

    value.apply_server_correction(15);
    assert_eq!(*value, 15);
    assert!(!value.is_dirty());
  }

  #[test]
  fn clear_dirty() {
    let mut value = NetworkedValue::new(42);
    assert!(value.is_dirty());

    value.clear_dirty();
    assert!(!value.is_dirty());
  }

  #[test]
  fn set_if_changed_same_value() {
    let mut value = NetworkedValue::from_network(10);

    let changed = value.set_if_changed(10);
    assert!(!changed);
    assert!(!value.is_dirty());
  }

  #[test]
  fn set_if_changed_different_value() {
    let mut value = NetworkedValue::from_network(10);

    let changed = value.set_if_changed(20);
    assert!(changed);
    assert!(value.is_dirty());
    assert_eq!(*value, 20);
  }

  #[test]
  fn deref_read_access() {
    let value = NetworkedValue::new(42);

    // Deref for read
    assert_eq!(*value, 42);

    // get() method
    assert_eq!(value.get(), &42);

    // value() for Copy types
    assert_eq!(value.value(), 42);
  }

  #[test]
  fn into_inner() {
    let value = NetworkedValue::new(String::from("hello"));
    let inner = value.into_inner();
    assert_eq!(inner, "hello");
  }

  #[test]
  fn equality_ignores_dirty() {
    let mut a = NetworkedValue::new(42);
    let b = NetworkedValue::from_network(42);

    // a is dirty, b is not, but they should be equal
    assert!(a.is_dirty());
    assert!(!b.is_dirty());
    assert_eq!(a, b);

    a.clear_dirty();
    assert_eq!(a, b);
  }

  #[test]
  fn default_is_dirty() {
    let value: NetworkedValue<i32> = NetworkedValue::default();
    assert!(value.is_dirty());
    assert_eq!(*value, 0);
  }

  #[test]
  fn debug_format() {
    let value = NetworkedValue::new(42);
    let debug = format!("{:?}", value);
    assert!(debug.contains("NetworkedValue"));
    assert!(debug.contains("42"));
    assert!(debug.contains("dirty"));
  }

  #[test]
  fn serialization_roundtrip() {
    let original = NetworkedValue::new(42);
    let json = serde_json::to_string(&original).unwrap();
    let deserialized: NetworkedValue<i32> = serde_json::from_str(&json).unwrap();

    assert_eq!(*deserialized, 42);
    // Dirty flag is not serialized, so deserialized value is not dirty
    assert!(!deserialized.is_dirty());
  }
}
