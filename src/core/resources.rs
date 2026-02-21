//! Typed resource map for cross-scene shared state.
//!
//! `Resources` is a type-erased container where each type can have exactly one
//! instance stored. It is used to share data between scenes (e.g. player
//! score, settings, audio manager handles) without coupling them together.
//!
//! # Example
//!
//! ```rust
//! use ryo2d::core::resources::Resources;
//!
//! let mut res = Resources::new();
//! res.insert(42u32);
//! assert_eq!(res.get::<u32>(), Some(&42));
//! ```

use std::any::{Any, TypeId};
use std::collections::HashMap;

/// A typed resource map that stores at most one value per concrete type.
///
/// Internally backed by a `HashMap<TypeId, Box<dyn Any>>`, every insert,
/// get, or remove operation is O(1) amortised.
pub struct Resources {
    map: HashMap<TypeId, Box<dyn Any>>,
}

impl Resources {
    /// Creates an empty resource map.
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    /// Inserts a resource of type `T`, replacing any previous value of the
    /// same type and returning it if present.
    pub fn insert<T: Any>(&mut self, value: T) -> Option<T> {
        self.map
            .insert(TypeId::of::<T>(), Box::new(value))
            .and_then(|prev| prev.downcast::<T>().ok().map(|b| *b))
    }

    /// Returns a shared reference to the resource of type `T`, or `None` if
    /// no such resource has been inserted.
    pub fn get<T: Any>(&self) -> Option<&T> {
        self.map
            .get(&TypeId::of::<T>())
            .and_then(|boxed| boxed.downcast_ref::<T>())
    }

    /// Returns an exclusive reference to the resource of type `T`, or `None`
    /// if no such resource has been inserted.
    pub fn get_mut<T: Any>(&mut self) -> Option<&mut T> {
        self.map
            .get_mut(&TypeId::of::<T>())
            .and_then(|boxed| boxed.downcast_mut::<T>())
    }

    /// Returns a mutable reference to the resource of type `T`, inserting a
    /// value created by `f` if none exists.
    pub fn get_or_insert_with<T: Any>(&mut self, f: impl FnOnce() -> T) -> &mut T {
        self.map
            .entry(TypeId::of::<T>())
            .or_insert_with(|| Box::new(f()))
            .downcast_mut::<T>()
            .expect("type mismatch in Resources::get_or_insert_with (should be unreachable)")
    }

    /// Removes and returns the resource of type `T`, or `None` if it was not
    /// present.
    pub fn remove<T: Any>(&mut self) -> Option<T> {
        self.map
            .remove(&TypeId::of::<T>())
            .and_then(|boxed| boxed.downcast::<T>().ok().map(|b| *b))
    }

    /// Returns `true` if a resource of type `T` is currently stored.
    pub fn contains<T: Any>(&self) -> bool {
        self.map.contains_key(&TypeId::of::<T>())
    }
}

impl Default for Resources {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_get() {
        let mut res = Resources::new();
        res.insert(42u32);
        assert_eq!(res.get::<u32>(), Some(&42));
    }

    #[test]
    fn get_returns_none_when_missing() {
        let res = Resources::new();
        assert_eq!(res.get::<u32>(), None);
    }

    #[test]
    fn get_mut_modifies_in_place() {
        let mut res = Resources::new();
        res.insert(10i32);
        if let Some(val) = res.get_mut::<i32>() {
            *val += 5;
        }
        assert_eq!(res.get::<i32>(), Some(&15));
    }

    #[test]
    fn insert_overwrites_existing() {
        let mut res = Resources::new();
        let prev = res.insert(1u32);
        assert!(prev.is_none());

        let prev = res.insert(2u32);
        assert_eq!(prev, Some(1));
        assert_eq!(res.get::<u32>(), Some(&2));
    }

    #[test]
    fn remove_returns_value() {
        let mut res = Resources::new();
        res.insert(String::from("hello"));
        let removed = res.remove::<String>();
        assert_eq!(removed, Some(String::from("hello")));
        assert!(!res.contains::<String>());
    }

    #[test]
    fn remove_returns_none_when_missing() {
        let mut res = Resources::new();
        assert_eq!(res.remove::<f64>(), None);
    }

    #[test]
    fn contains_checks_presence() {
        let mut res = Resources::new();
        assert!(!res.contains::<u8>());
        res.insert(255u8);
        assert!(res.contains::<u8>());
    }

    #[test]
    fn get_or_insert_with_creates_if_missing() {
        let mut res = Resources::new();
        let val = res.get_or_insert_with(|| 99u64);
        assert_eq!(*val, 99);
    }

    #[test]
    fn get_or_insert_with_returns_existing() {
        let mut res = Resources::new();
        res.insert(50u64);
        let val = res.get_or_insert_with(|| 99u64);
        assert_eq!(*val, 50);
    }

    #[test]
    fn multiple_types_coexist() {
        let mut res = Resources::new();
        res.insert(1u32);
        res.insert(2.0f64);
        res.insert(String::from("three"));

        assert_eq!(res.get::<u32>(), Some(&1));
        assert_eq!(res.get::<f64>(), Some(&2.0));
        assert_eq!(res.get::<String>(), Some(&String::from("three")));
    }

    #[test]
    fn default_creates_empty() {
        let res = Resources::default();
        assert!(!res.contains::<u32>());
    }
}
