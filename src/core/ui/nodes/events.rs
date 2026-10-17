//! What a node's widget reports

use crate::core::ecs::kernel::{Entity, EntityEvent, Message};

/// Something a UI node reported: a click, a change, a drop
///
/// It is both an engine message (read it with a `MessageReader<UiEvent>`) and
/// an entity event (observe it on the node, or on an ancestor).
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
