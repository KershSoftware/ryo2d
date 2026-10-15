//! Buffered messages: written anywhere in a frame, readable throughout the next.
//!
//! Systems write with `MessageWriter<T>` and read with `MessageReader<T>`;
//! scenes go through `SceneContext::write_message` and `messages::<T>()`,
//! which keep a cursor per scene. Buffers advance at the start of a frame
//! once a fixed step has run since the last advance, so a reader in the fixed
//! update never misses a message, at any frame rate. While no fixed step runs
//! (the game clock paused) nothing is dropped.

mod builtin;
mod scene;

pub use builtin::*;
pub use scene::SceneMessages;

use std::any::{Any, TypeId};

use crate::core::ecs::kernel::{Message, Messages, Resource, World};
use crate::core::physics::{
  CollisionStarted, CollisionStopped, ContactForce, JointBroken, TriggerEntered, TriggerExited,
};

/// A type-erased `MessageCursor<T>`.
pub(crate) type ErasedCursor = Box<dyn Any + Send + Sync>;

/// Makes a cursor past every `T` written so far.
type CursorNow = fn(&World) -> ErasedCursor;

/// One updater, one clearer and one "read from now on" cursor maker per
/// registered message type.
#[derive(Resource, Default)]
struct MessageUpdaters {
  update: Vec<fn(&mut World)>,
  clear: Vec<fn(&mut World)>,
  cursor_now: Vec<(TypeId, CursorNow)>,
}

fn update_one<T: Message>(world: &mut World) {
  world.resource_mut::<Messages<T>>().update();
}

fn clear_one<T: Message>(world: &mut World) {
  world.resource_mut::<Messages<T>>().clear();
}

fn cursor_now<T: Message>(world: &World) -> ErasedCursor {
  Box::new(world.resource::<Messages<T>>().get_cursor_current())
}

/// A cursor past every message already written, per registered type.
pub(crate) fn cursors_now(world: &World) -> Vec<(TypeId, ErasedCursor)> {
  world
    .get_resource::<MessageUpdaters>()
    .map(|u| {
      u.cursor_now
        .iter()
        .map(|(id, make)| (*id, make(world)))
        .collect()
    })
    .unwrap_or_default()
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
  updaters
    .cursor_now
    .push((TypeId::of::<T>(), cursor_now::<T>));
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
  register_message::<TriggerEntered>(world);
  register_message::<TriggerExited>(world);
  register_message::<ContactForce>(world);
  register_message::<JointBroken>(world);
  register_message::<TweenStarted>(world);
  register_message::<TweenCompleted>(world);
  register_message::<KnockbackStarted>(world);
  register_message::<KnockbackFinished>(world);
  register_message::<SquashStretchStarted>(world);
  register_message::<SquashStretchFinished>(world);
  register_message::<AssetReloaded>(world);
  register_message::<AssetLoadFailed>(world);
  register_message::<crate::core::particles::ParticleCollision>(world);
  register_message::<crate::core::gameplay::MovementEvent>(world);
  register_message::<crate::core::gameplay::PickedUp>(world);
  register_message::<crate::core::gameplay::Killed>(world);
  register_message::<crate::core::gameplay::CheckpointReached>(world);
  register_message::<crate::core::text::TextRevealed>(world);
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
