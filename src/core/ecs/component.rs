//! Component trait for ECS

use std::any::Any;

/// Marker trait for ECS components.
/// Any `'static + Send + Sync` type automatically implements this.
pub trait Component: 'static + Send + Sync {
  fn as_any(&self) -> &dyn Any;
  fn as_any_mut(&mut self) -> &mut dyn Any;
}

/// Blanket implementation for all compatible types
impl<T: 'static + Send + Sync> Component for T {
  fn as_any(&self) -> &dyn Any {
    self
  }

  fn as_any_mut(&mut self) -> &mut dyn Any {
    self
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  struct Position {
    x: f32,
    y: f32,
  }

  #[test]
  fn component_downcast_works() {
    let pos = Position { x: 10.0, y: 20.0 };
    let any_ref: &dyn Any = pos.as_any();
    let pos_ref = any_ref.downcast_ref::<Position>().unwrap();

    assert_eq!(pos_ref.x, 10.0);
    assert_eq!(pos_ref.y, 20.0);
  }
}
