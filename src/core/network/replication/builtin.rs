//! `Replicated` for the engine's own components.

use super::{ComponentTypeId, Replicated};
use crate::core::ecs::{Transform, Velocity};

/// Translation, rotation and scale, local to the parent.
impl Replicated for Transform {
  fn component_id() -> ComponentTypeId {
    ComponentTypeId::from_name("Transform")
  }

  fn component_name() -> &'static str {
    "Transform"
  }

  fn replication_priority() -> u8 {
    10
  }
}

impl Replicated for Velocity {
  fn component_id() -> ComponentTypeId {
    ComponentTypeId::from_name("Velocity")
  }

  fn component_name() -> &'static str {
    "Velocity"
  }

  fn replication_priority() -> u8 {
    10
  }
}

#[cfg(test)]
mod tests {
  use glam::Vec2;

  use super::super::{ComponentRegistry, Replicated, SerializedComponent};
  use crate::core::ecs::{GlobalTransform, Transform, Velocity, World};

  /// Send `value` through a registry onto a fresh entity and read it back.
  fn through_the_wire<T: Replicated + PartialEq + std::fmt::Debug>(value: T) -> (World, T) {
    let mut registry = ComponentRegistry::new();
    registry.register::<T>();
    let serialized = SerializedComponent::from_component(&value).expect("serialize");
    let mut world = World::new();
    let entity = world.spawn_empty().id();
    registry
      .deserialize_and_apply(&serialized, &mut world, entity)
      .expect("apply");
    let received = world.get::<T>(entity).cloned().expect("component");
    (world, received)
  }

  #[test]
  fn a_transform_replicates_translation_rotation_and_scale() {
    let sent = Transform::from_xy(12.0, -3.5)
      .with_rotation(0.75)
      .with_scale(Vec2::new(2.0, 0.5));
    let (mut world, received) = through_the_wire(sent);
    assert_eq!(received, sent);
    // Arriving places the entity, as any inserted Transform does
    let global = world
      .query::<&GlobalTransform>()
      .single(&world)
      .expect("one entity");
    assert_eq!(global.translation(), Vec2::new(12.0, -3.5));
  }

  #[test]
  fn a_velocity_replicates() {
    let sent = Velocity::new(30.0, -4.0);
    assert_eq!(through_the_wire(sent).1, sent);
  }

  #[test]
  fn the_ids_are_stable() {
    assert_eq!(
      Transform::component_id(),
      super::super::ComponentTypeId::from_name("Transform")
    );
    assert_eq!(
      Velocity::component_id(),
      super::super::ComponentTypeId::from_name("Velocity")
    );
  }
}
