//! Entity - A unique identifier for game objects

/// A unique identifier for an entity in the ECS world.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Entity(u32);

impl Entity {
  pub(crate) fn from_raw(id: u32) -> Self {
    Entity(id)
  }

  pub fn id(&self) -> u32 {
    self.0
  }
}

/// Generates unique entity IDs
#[derive(Default)]
pub struct EntityAllocator {
  next_id: u32,
}

impl EntityAllocator {
  pub fn new() -> Self {
    EntityAllocator { next_id: 0 }
  }

  pub fn allocate(&mut self) -> Entity {
    let id = self.next_id;
    self.next_id += 1;
    Entity::from_raw(id)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn entities_are_unique() {
    let mut allocator = EntityAllocator::new();
    let e1 = allocator.allocate();
    let e2 = allocator.allocate();
    let e3 = allocator.allocate();

    assert_ne!(e1, e2);
    assert_ne!(e2, e3);
    assert_ne!(e1, e3);
  }

  #[test]
  fn entity_ids_are_sequential() {
    let mut allocator = EntityAllocator::new();
    assert_eq!(allocator.allocate().id(), 0);
    assert_eq!(allocator.allocate().id(), 1);
    assert_eq!(allocator.allocate().id(), 2);
  }

  #[test]
  fn entities_are_comparable() {
    let e1 = Entity::from_raw(42);
    let e2 = Entity::from_raw(42);
    let e3 = Entity::from_raw(99);

    assert_eq!(e1, e2);
    assert_ne!(e1, e3);
  }
}
