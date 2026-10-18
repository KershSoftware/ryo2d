//! What a node's widget reports

use crate::core::ecs::kernel::{Entity, EntityEvent, Message, Messages, World};
use crate::core::messages::register_message;

/// Something a UI node reported: a click, a change, a drop
///
/// It is both an engine message (read it with `ctx.ui_events()` or a
/// `MessageReader<UiEvent>`) and an entity event triggered on its node (observe
/// it there). Observers run during the UI pass, while the scene stack is out
/// of the world: an observer must not change scenes, so it sends a message or
/// sets state for the scene to act on.
#[derive(Message, EntityEvent, Debug, Clone, PartialEq)]
pub struct UiEvent {
  /// The node that reported it
  #[event_target]
  pub node: Entity,
  /// The node's `.name(..)`, if it has one
  pub name: Option<String>,
  /// The node's `.action(..)`, if it has one
  pub action: Option<String>,
  /// What happened
  pub kind: UiEventKind,
  /// The scene scope the tree was spawned in
  pub(crate) scene: u64,
}

/// What a `UiEvent` reports
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiEventKind {
  /// A button, tab, choice or slot was activated
  Clicked,
  /// A slot or button was activated with the secondary button
  SecondaryClicked,
  /// A checkbox, toggle, radio group, slider, dropdown or tab changed
  Changed,
  /// A text input was submitted
  Submitted,
  /// A drag source was dropped on this node
  Dropped {
    /// The dragged node
    source: Entity,
  },
  /// Back was pressed inside the node's focus scope
  Back,
  /// The node gained focus
  FocusGained,
  /// The node lost focus
  FocusLost,
}

/// Write `events` as messages, in order, and trigger each on its node (a
/// node an earlier observer despawned gets its message and no trigger)
pub(crate) fn deliver(world: &mut World, events: Vec<UiEvent>) {
  if events.is_empty() {
    return;
  }
  register_message::<UiEvent>(world);
  for event in events {
    let node = event.node;
    if let Some(mut messages) = world.get_resource_mut::<Messages<UiEvent>>() {
      messages.write(event.clone());
    }
    if world.get_entity(node).is_ok() {
      world.trigger(event);
    }
  }
}

#[cfg(test)]
mod tests {
  use std::sync::{Arc, Mutex};

  use super::*;
  use crate::core::ecs::kernel::{On, World};
  use crate::core::messages::register_engine_messages;

  fn clicked(node: Entity) -> UiEvent {
    UiEvent {
      node,
      name: None,
      action: Some("play".into()),
      kind: UiEventKind::Clicked,
      scene: 1,
    }
  }

  #[test]
  fn ui_events_are_engine_messages() {
    let mut world = World::new();
    register_engine_messages(&mut world);
    assert!(world.contains_resource::<crate::core::ecs::kernel::Messages<UiEvent>>());
  }

  #[test]
  fn a_ui_event_reaches_an_observer_on_its_node() {
    let mut world = World::new();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let node = world.spawn_empty().id();
    let other = world.spawn_empty().id();
    let log = Arc::clone(&seen);
    world.entity_mut(node).observe(move |event: On<UiEvent>| {
      if let Ok(mut log) = log.lock() {
        log.push(event.action.clone());
      }
    });
    world.trigger(clicked(other));
    world.trigger(clicked(node));
    assert_eq!(
      *seen.lock().unwrap(),
      [Some("play".to_string())],
      "only its own node's"
    );
  }
}
