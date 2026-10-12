//! Buffered messages: written anywhere in a frame, readable throughout the next.
//!
//! Systems write with `MessageWriter<T>` and read with `MessageReader<T>`;
//! scenes go through `SceneContext::write_message` and `messages::<T>()`,
//! which keep a cursor per scene. Buffers advance once per frame.

mod builtin;
mod scene;

pub use builtin::*;
pub use scene::SceneMessages;

use crate::core::ecs::kernel::{Message, Messages, Resource, World};

/// One updater and one clearer per registered message type.
#[derive(Resource, Default)]
struct MessageUpdaters {
  update: Vec<fn(&mut World)>,
  clear: Vec<fn(&mut World)>,
}

fn update_one<T: Message>(world: &mut World) {
  world.resource_mut::<Messages<T>>().update();
}

fn clear_one<T: Message>(world: &mut World) {
  world.resource_mut::<Messages<T>>().clear();
}

/// Make `T` writable and readable in this world. Registering twice is a no-op.
pub fn register_message<T: Message>(world: &mut World) {
  if world.contains_resource::<Messages<T>>() {
    return;
  }
  world.init_resource::<Messages<T>>();
  let mut updaters = world.get_resource_or_insert_with(MessageUpdaters::default);
  updaters.update.push(update_one::<T>);
  updaters.clear.push(clear_one::<T>);
}

/// Advance every buffer by one frame.
pub fn update_messages(world: &mut World) {
  let updaters: Vec<fn(&mut World)> = world
    .get_resource::<MessageUpdaters>()
    .map(|u| u.update.clone())
    .unwrap_or_default();
  for update in updaters {
    update(world);
  }
}

/// Drop everything in every buffer.
pub fn clear_messages(world: &mut World) {
  let clearers: Vec<fn(&mut World)> = world
    .get_resource::<MessageUpdaters>()
    .map(|u| u.clear.clone())
    .unwrap_or_default();
  for clear in clearers {
    clear(world);
  }
}

/// Register the engine's own message types.
pub fn register_engine_messages(world: &mut World) {
  register_message::<CollisionStarted>(world);
  register_message::<CollisionStopped>(world);
  register_message::<TweenStarted>(world);
  register_message::<TweenCompleted>(world);
  register_message::<KnockbackStarted>(world);
  register_message::<KnockbackFinished>(world);
  register_message::<SquashStretchStarted>(world);
  register_message::<SquashStretchFinished>(world);
  register_message::<AssetReloaded>(world);
  register_message::<AssetLoadFailed>(world);
  register_message::<crate::core::particles::ParticleCollision>(world);
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ecs::kernel::{MessageReader, Schedule, ScheduleLabel};

  #[derive(Message, Debug, Clone, PartialEq)]
  struct Ping(u32);

  #[derive(ScheduleLabel, Debug, Clone, Copy, PartialEq, Eq, Hash)]
  struct Step;

  #[test]
  fn a_message_written_this_frame_is_readable_next_frame_and_then_gone() {
    let mut world = World::new();
    register_message::<Ping>(&mut world);
    world.resource_mut::<Messages<Ping>>().write(Ping(1));
    // frame boundary
    update_messages(&mut world);
    let mut s = Schedule::new(Step);
    s.add_systems(|mut reader: MessageReader<Ping>| {
      let seen: Vec<Ping> = reader.read().cloned().collect();
      assert_eq!(seen, [Ping(1)]);
    });
    s.run(&mut world);
    update_messages(&mut world);
    update_messages(&mut world);
    assert_eq!(
      world
        .resource::<Messages<Ping>>()
        .iter_current_update_messages()
        .count(),
      0
    );
  }

  #[test]
  fn clear_empties_every_registered_buffer() {
    let mut world = World::new();
    register_message::<Ping>(&mut world);
    world.resource_mut::<Messages<Ping>>().write(Ping(9));
    clear_messages(&mut world);
    let mut cursor = world.resource::<Messages<Ping>>().get_cursor();
    assert_eq!(cursor.read(world.resource::<Messages<Ping>>()).count(), 0);
  }

  #[test]
  fn registering_twice_is_harmless() {
    let mut world = World::new();
    register_message::<Ping>(&mut world);
    register_message::<Ping>(&mut world);
    world.resource_mut::<Messages<Ping>>().write(Ping(2));
    update_messages(&mut world);
    let mut cursor = world.resource::<Messages<Ping>>().get_cursor();
    assert_eq!(cursor.read(world.resource::<Messages<Ping>>()).count(), 1);
  }

  #[test]
  fn engine_messages_register() {
    let mut world = World::new();
    register_engine_messages(&mut world);
    assert!(world.contains_resource::<Messages<CollisionStarted>>());
    assert!(world.contains_resource::<Messages<AssetLoadFailed>>());
  }
}
