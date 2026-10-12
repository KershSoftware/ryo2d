//! Per-scene read cursors, so a scene sees each message once.

use std::any::{Any, TypeId};
use std::collections::HashMap;

use crate::core::ecs::kernel::{Message, MessageCursor, Messages, World};

/// A scene's read position in every message buffer it has looked at.
#[derive(Default)]
pub struct SceneMessages {
  cursors: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
}

impl SceneMessages {
  /// Every `T` written since this scene last read, cloned out.
  pub fn read<T: Message + Clone>(&mut self, world: &World) -> Vec<T> {
    let Some(messages) = world.get_resource::<Messages<T>>() else {
      return Vec::new();
    };
    let cursor = self
      .cursors
      .entry(TypeId::of::<T>())
      .or_insert_with(|| Box::new(messages.get_cursor()));
    let cursor: &mut MessageCursor<T> = cursor
      .downcast_mut()
      .expect("cursor type is keyed by the message type");
    cursor.read(messages).cloned().collect()
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::messages::{register_message, update_messages};

  #[derive(Message, Debug, Clone, PartialEq)]
  struct Ping(u32);

  #[test]
  fn each_scene_cursor_sees_each_message_once() {
    let mut world = World::new();
    register_message::<Ping>(&mut world);
    let mut a = SceneMessages::default();
    let mut b = SceneMessages::default();
    world.resource_mut::<Messages<Ping>>().write(Ping(1));
    assert_eq!(a.read::<Ping>(&world), [Ping(1)]);
    assert_eq!(a.read::<Ping>(&world), [], "already seen by this scene");
    assert_eq!(
      b.read::<Ping>(&world),
      [Ping(1)],
      "a different scene has its own cursor"
    );
    update_messages(&mut world);
    world.resource_mut::<Messages<Ping>>().write(Ping(2));
    assert_eq!(a.read::<Ping>(&world), [Ping(2)]);
  }

  #[test]
  fn reading_an_unregistered_message_type_is_empty_not_a_panic() {
    let world = World::new();
    let mut a = SceneMessages::default();
    assert!(a.read::<Ping>(&world).is_empty());
  }
}
