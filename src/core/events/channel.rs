//! Internal event channel storage
//!
//! This module provides the type-erased storage mechanism for events.

use std::any::Any;

/// Type-erased trait for event channels
pub(super) trait AnyEventChannel: Any + Send + Sync {
  fn as_any(&self) -> &dyn Any;
  fn as_any_mut(&mut self) -> &mut dyn Any;
  fn clear(&mut self);
  fn len(&self) -> usize;
  fn is_empty(&self) -> bool;
}

/// Typed storage for events of a specific type
pub(super) struct EventChannel<E> {
  events: Vec<E>,
}

impl<E> EventChannel<E> {
  pub fn new() -> Self {
    Self { events: Vec::new() }
  }

  pub fn push(&mut self, event: E) {
    self.events.push(event);
  }

  pub fn iter(&self) -> impl Iterator<Item = &E> {
    self.events.iter()
  }
}

// These methods exist on the concrete type for convenience but are primarily
// accessed through the AnyEventChannel trait. Allow dead_code since the trait
// implementations call the underlying Vec methods directly.
#[allow(dead_code)]
impl<E> EventChannel<E> {
  pub fn len(&self) -> usize {
    self.events.len()
  }

  pub fn is_empty(&self) -> bool {
    self.events.is_empty()
  }

  pub fn clear(&mut self) {
    self.events.clear();
  }
}

impl<E> Default for EventChannel<E> {
  fn default() -> Self {
    Self::new()
  }
}

impl<E: 'static + Send + Sync> AnyEventChannel for EventChannel<E> {
  fn as_any(&self) -> &dyn Any {
    self
  }

  fn as_any_mut(&mut self) -> &mut dyn Any {
    self
  }

  fn clear(&mut self) {
    self.events.clear();
  }

  fn len(&self) -> usize {
    self.events.len()
  }

  fn is_empty(&self) -> bool {
    self.events.is_empty()
  }
}
